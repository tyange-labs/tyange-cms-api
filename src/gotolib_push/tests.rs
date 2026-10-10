use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use base64::{decode_config, encode_config, URL_SAFE_NO_PAD};
use poem::{
    get, handler,
    http::StatusCode,
    listener::TcpAcceptor,
    post,
    web::{Data, Path, Query},
    EndpointExt, Request, Response, Route, Server,
};
use serde_json::{json, Value};
use sqlx::{query, query_scalar, SqlitePool};
use web_push_native::{jwt_simple::algorithms::ES256KeyPair, p256::SecretKey, Auth};

use super::*;
use crate::{covers::tests::isbn, db::init_db};

// ── 구독자(받는 쪽) 키 ────────────────────────────────────────────────────────

/// 브라우저가 만드는 구독 키 한 벌. 테스트가 직접 복호화할 수 있게 비밀 키를 들고 있다.
struct Device {
    secret: SecretKey,
    auth: [u8; 16],
}

impl Device {
    fn new(seed: u8) -> Self {
        let key_pair = ES256KeyPair::generate();
        let secret = SecretKey::from_slice(&key_pair.to_bytes()).unwrap();
        Self { secret, auth: [seed; 16] }
    }

    fn keys(&self) -> SubscriptionKeys {
        let public = self.secret.public_key().to_encoded_point(false);
        SubscriptionKeys {
            p256dh: encode_config(public.as_bytes(), URL_SAFE_NO_PAD),
            auth: encode_config(self.auth, URL_SAFE_NO_PAD),
        }
    }

    fn input(&self, endpoint: &str) -> SubscriptionInput {
        SubscriptionInput { endpoint: endpoint.to_string(), keys: self.keys() }
    }

    fn decrypt(&self, body: &[u8]) -> Value {
        let auth = Auth::clone_from_slice(&self.auth);
        let plain = web_push_native::decrypt(body.to_vec(), &self.secret, &auth).expect("복호화");
        serde_json::from_slice(&plain).expect("JSON")
    }
}

// ── 가짜 푸시 서비스 + 가짜 도서관 API ─────────────────────────────────────────

/// 가짜 푸시 서비스가 받은 요청: (경로 이름, 헤더 일부, 본문)
type ReceivedPush = (String, HashMap<String, String>, Vec<u8>);

#[derive(Clone, Default)]
struct Fake {
    /// (isbn13, libCode) → hasBook. 없으면 500을 돌려준다.
    library: Arc<Mutex<HashMap<(String, String), bool>>>,
    library_calls: Arc<Mutex<Vec<(String, String)>>>,
    /// 푸시 경로 이름 → 돌려줄 상태 코드(없으면 201)
    push_status: Arc<Mutex<HashMap<String, u16>>>,
    /// 받은 푸시
    pushes: Arc<Mutex<Vec<ReceivedPush>>>,
}

#[handler]
fn fake_book_exist(Query(params): Query<HashMap<String, String>>, fake: Data<&Fake>) -> Response {
    let key = (
        params.get("isbn13").cloned().unwrap_or_default(),
        params.get("libCode").cloned().unwrap_or_default(),
    );
    fake.library_calls.lock().unwrap().push(key.clone());
    match fake.library.lock().unwrap().get(&key) {
        Some(has) => Response::builder().content_type("application/json").body(
            json!({ "response": { "result": { "hasBook": if *has { "Y" } else { "N" }, "loanAvailable": "N" } } }).to_string(),
        ),
        None => Response::builder().status(StatusCode::INTERNAL_SERVER_ERROR).finish(),
    }
}

#[handler]
async fn fake_push(Path(name): Path<String>, req: &Request, fake: Data<&Fake>, body: poem::Body) -> Response {
    let bytes = body.into_bytes().await.unwrap().to_vec();
    let headers = ["authorization", "content-encoding", "ttl", "content-type"]
        .iter()
        .filter_map(|h| req.header(h).map(|v| (h.to_string(), v.to_string())))
        .collect();
    fake.pushes.lock().unwrap().push((name.clone(), headers, bytes));
    let status = fake.push_status.lock().unwrap().get(&name).copied().unwrap_or(201);
    Response::builder().status(StatusCode::from_u16(status).unwrap()).finish()
}

struct Env {
    pool: SqlitePool,
    service: PushService,
    fake: Fake,
    base: String,
}

async fn env_with(max_checks: usize) -> Env {
    let fake = Fake::default();
    let app = Route::new()
        .at("/api/library/bookExist", get(fake_book_exist))
        .at("/push/:name", post(fake_push))
        .data(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(Server::new_with_acceptor(TcpAcceptor::from_tokio(listener).unwrap()).run(app));

    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    init_db(&pool).await.unwrap();
    let base = format!("http://127.0.0.1:{port}");
    let service = PushService::new(VapidKeys::generate("https://gotolib.example"), &base, true, max_checks, Duration::ZERO).unwrap();
    Env { pool, service, fake, base }
}

async fn env() -> Env {
    env_with(DEFAULT_MAX_CHECKS).await
}

impl Env {
    fn endpoint(&self, name: &str) -> String {
        format!("{}/push/{name}", self.base)
    }

    async fn add_user(&self, user_id: &str) {
        crate::routes::add_user::create_user(&self.pool, user_id, "password123", "gotolib")
            .await
            .expect("create user");
    }

    /// 계정을 만들고 찜 목록·자주 가는 도서관이 담긴 동기화 문서를 넣는다.
    async fn user_with_doc(&self, user_id: &str, books: &[(&str, &str)], libs: &[(&str, &str)]) {
        self.add_user(user_id).await;
        let watchlist: serde_json::Map<String, Value> = books
            .iter()
            .map(|(isbn, title)| (isbn.to_string(), json!({ "at": 1, "value": { "id": isbn, "isbn13": isbn, "title": title, "watchedAt": 1 } })))
            .collect();
        let fav: serde_json::Map<String, Value> = libs
            .iter()
            .map(|(code, name)| (code.to_string(), json!({ "at": 1, "value": { "libCode": code, "libName": name, "addedAt": 1 } })))
            .collect();
        let doc = json!({ "v": 2, "books": {}, "watchlist": watchlist, "favLibraries": fav });
        query("INSERT INTO gotolib_sync (user_id, version, data) VALUES (?, 1, ?)")
            .bind(user_id)
            .bind(doc.to_string())
            .execute(&self.pool)
            .await
            .unwrap();
    }

    async fn subscribe(&self, user_id: &str, device: &Device, name: &str) {
        upsert_subscription(&self.pool, user_id, &device.input(&self.endpoint(name)), Some("test-agent"), 1)
            .await
            .unwrap();
    }

    fn set_library(&self, isbn: &str, lib: &str, has: bool) {
        self.fake.library.lock().unwrap().insert((isbn.to_string(), lib.to_string()), has);
    }

    async fn round(&self) -> RoundSummary {
        run_round(&self.pool, &self.service, 1_000).await
    }

    async fn state(&self, user_id: &str, isbn: &str, lib: &str) -> Option<bool> {
        query_scalar::<_, i64>("SELECT has_book FROM gotolib_watch_state WHERE user_id = ? AND isbn13 = ? AND lib_code = ?")
            .bind(user_id)
            .bind(isbn)
            .bind(lib)
            .fetch_optional(&self.pool)
            .await
            .unwrap()
            .map(|v| v != 0)
    }

    async fn sub_count(&self) -> i64 {
        query_scalar("SELECT COUNT(*) FROM gotolib_push_subscriptions").fetch_one(&self.pool).await.unwrap()
    }

    fn pushes(&self) -> Vec<ReceivedPush> {
        self.fake.pushes.lock().unwrap().clone()
    }
}

// ── 순수 함수 ────────────────────────────────────────────────────────────────

#[test]
fn only_known_push_hosts_are_allowed() {
    let device = Device::new(1);
    let ok = |endpoint: &str| validate_subscription(&device.input(endpoint), false).is_ok();
    assert!(ok("https://fcm.googleapis.com/fcm/send/abc"));
    assert!(ok("https://web.push.apple.com/QGxyz"));
    assert!(ok("https://updates.push.services.mozilla.com/wpush/v2/abc"));
    assert!(ok("https://wns2-par02p.notify.windows.com/w/?token=abc"));
    // 서버가 요청을 보내는 주소라서 그 밖의 주소는 모두 거부한다(SSRF 방지).
    assert!(!ok("https://evil.example/push"));
    assert!(!ok("http://fcm.googleapis.com/fcm/send/abc")); // http
    assert!(!ok("https://fcm.googleapis.com.evil.example/x")); // 호스트 위장
    assert!(!ok("https://evilfcm.googleapis.com.example/x"));
    assert!(!ok("https://notfcm.googleapis.com/x")); // 허용 호스트의 하위 도메인이 아니라 이름만 비슷한 호스트
    assert!(!ok("http://127.0.0.1/push")); // 내부 주소
    assert!(!ok("http://169.254.169.254/latest/meta-data"));
    assert!(!ok("not a url"));
    assert!(!ok(&format!("https://fcm.googleapis.com/{}", "a".repeat(700))));
}

#[test]
fn loopback_is_allowed_only_for_tests() {
    let device = Device::new(1);
    assert!(validate_subscription(&device.input("http://127.0.0.1:9999/push/a"), true).is_ok());
    assert!(validate_subscription(&device.input("http://127.0.0.1:9999/push/a"), false).is_err());
    // 테스트 허용이어도 외부 http 주소는 안 된다
    assert!(validate_subscription(&device.input("http://evil.example/push"), true).is_err());
}

#[test]
fn malformed_keys_are_rejected() {
    let device = Device::new(1);
    let endpoint = "https://fcm.googleapis.com/fcm/send/abc";
    let mut input = device.input(endpoint);
    input.keys.p256dh = "AAAA".to_string();
    assert!(validate_subscription(&input, false).is_err());
    let mut input = device.input(endpoint);
    input.keys.p256dh = encode_config([4u8; 65], URL_SAFE_NO_PAD); // 곡선 위의 점이 아님
    assert!(validate_subscription(&input, false).is_err());
    let mut input = device.input(endpoint);
    input.keys.auth = encode_config([1u8; 8], URL_SAFE_NO_PAD); // 16바이트가 아님
    assert!(validate_subscription(&input, false).is_err());
    let mut input = device.input(endpoint);
    input.keys.auth = "***".to_string();
    assert!(validate_subscription(&input, false).is_err());
}

#[test]
fn only_a_book_that_was_missing_and_is_now_held_notifies() {
    assert_eq!(change(Some(false), true), Change::Notify);
    assert_eq!(change(None, true), Change::Record, "처음 보는 쌍은 알리지 않는다");
    assert_eq!(change(None, false), Change::Record);
    assert_eq!(change(Some(true), true), Change::Record, "계속 소장");
    assert_eq!(change(Some(true), false), Change::Record, "소장 → 미소장");
    assert_eq!(change(Some(false), false), Change::Record);
}

#[test]
fn targets_skip_deleted_and_invalid_entries() {
    let good = isbn(1);
    let doc = json!({
        "watchlist": {
            "a": { "at": 1, "value": { "isbn13": good, "title": " 좋은 책 " } },
            "deleted": { "at": 2, "deleted": true },
            "bad": { "at": 1, "value": { "isbn13": "123", "title": "ISBN이 이상함" } },
            "none": { "at": 1, "value": { "title": "ISBN 없음" } },
        },
        "favLibraries": {
            "1": { "at": 1, "value": { "libCode": "111005", "libName": "도서관 A" } },
            "2": { "at": 1, "deleted": true },
            "3": { "at": 1, "value": { "libCode": "11 10", "libName": "공백" } },
            "4": { "at": 1, "value": { "libCode": "../x", "libName": "경로" } },
            "5": { "at": 1, "value": { "libName": "코드 없음" } },
        },
    });
    let targets = parse_targets(&doc);
    assert_eq!(targets.books, vec![WatchedBook { isbn13: good, title: "좋은 책".to_string() }]);
    assert_eq!(targets.libraries, vec![WatchedLibrary { lib_code: "111005".to_string(), name: "도서관 A".to_string() }]);
    assert_eq!(parse_targets(&json!({})), Targets::default());
    assert_eq!(parse_targets(&json!("not an object")), Targets::default());
}

#[test]
fn targets_are_capped() {
    let watchlist: serde_json::Map<String, Value> = (1..=40)
        .map(|n| {
            let id = isbn(n);
            (id.clone(), json!({ "at": 1, "value": { "isbn13": id, "title": format!("책 {n}") } }))
        })
        .collect();
    let fav: serde_json::Map<String, Value> = (1..=20)
        .map(|n| (n.to_string(), json!({ "at": 1, "value": { "libCode": format!("11{n:04}"), "libName": "도" } })))
        .collect();
    let targets = parse_targets(&json!({ "watchlist": watchlist, "favLibraries": fav }));
    assert_eq!(targets.books.len(), MAX_WATCHED_BOOKS);
    assert_eq!(targets.libraries.len(), MAX_LIBS);
}

#[test]
fn vapid_key_is_created_once_and_reloaded_unchanged() {
    let dir = std::env::temp_dir().join(format!("vapid-test-{}", uuid::Uuid::new_v4()));
    let path = dir.join("nested").join("vapid.key");
    let first = VapidKeys::load_or_create(&path, "https://gotolib.example").unwrap();
    assert!(path.exists());
    let second = VapidKeys::load_or_create(&path, "https://gotolib.example").unwrap();
    assert_eq!(first.public_key, second.public_key, "다시 읽어도 같은 키");
    // 압축하지 않은 65바이트 점이어야 브라우저가 applicationServerKey로 받는다
    let raw = decode_config(&first.public_key, URL_SAFE_NO_PAD).unwrap();
    assert_eq!(raw.len(), 65);
    assert_eq!(raw[0], 4);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
    // 깨진 파일은 조용히 새로 만들지 않고 오류로 알린다
    std::fs::write(&path, "not-a-key").unwrap();
    assert!(VapidKeys::load_or_create(&path, "https://gotolib.example").is_err());
    let _ = std::fs::remove_dir_all(dir);
}

// ── 발송 ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn sent_message_is_encrypted_signed_and_decryptable_by_the_device() {
    let env = env().await;
    let device = Device::new(7);
    let target = PushTarget {
        endpoint: env.endpoint("direct"),
        p256dh: device.keys().p256dh,
        auth: device.keys().auth,
    };
    let payload = notification_payload("제목", "본문", "/", "tag1");
    let outcome = send_payload(&env.service.http, &env.service.keys, &target, &payload).await;
    assert_eq!(outcome, SendOutcome::Delivered);

    let pushes = env.pushes();
    assert_eq!(pushes.len(), 1);
    let (_, headers, body) = &pushes[0];
    assert_eq!(headers.get("content-encoding").map(String::as_str), Some("aes128gcm"));
    assert_eq!(headers.get("ttl").map(String::as_str), Some("86400"));
    let authorization = headers.get("authorization").expect("VAPID 서명 헤더");
    assert!(authorization.starts_with("vapid "), "{authorization}");
    assert!(authorization.contains(&env.service.keys.public_key), "공개 키가 헤더에 실린다");
    assert!(!body.windows(6).any(|w| w == "본문".as_bytes()), "본문이 평문으로 나가면 안 된다");
    let plain = device.decrypt(body);
    assert_eq!(plain["title"], "제목");
    assert_eq!(plain["body"], "본문");
    assert_eq!(plain["tag"], "tag1");
}

#[tokio::test]
async fn gone_and_failed_responses_are_told_apart() {
    let env = env().await;
    let device = Device::new(7);
    let target = |name: &str| PushTarget { endpoint: env.endpoint(name), p256dh: device.keys().p256dh, auth: device.keys().auth };
    env.fake.push_status.lock().unwrap().insert("gone".into(), 410);
    env.fake.push_status.lock().unwrap().insert("missing".into(), 404);
    env.fake.push_status.lock().unwrap().insert("busy".into(), 429);
    env.fake.push_status.lock().unwrap().insert("down".into(), 500);
    let send = |name: &str| {
        let t = target(name);
        let service = &env.service;
        async move { send_payload(&service.http, &service.keys, &t, b"{}").await }
    };
    assert_eq!(send("gone").await, SendOutcome::Gone);
    assert_eq!(send("missing").await, SendOutcome::Gone);
    assert_eq!(send("busy").await, SendOutcome::Failed);
    assert_eq!(send("down").await, SendOutcome::Failed);
    assert_eq!(send("ok").await, SendOutcome::Delivered);
    // 서버가 아예 없을 때도 패닉하지 않고 실패로 본다
    let dead = PushTarget { endpoint: "http://127.0.0.1:1/push/x".into(), p256dh: device.keys().p256dh, auth: device.keys().auth };
    assert_eq!(send_payload(&env.service.http, &env.service.keys, &dead, b"{}").await, SendOutcome::Failed);
}

// ── 라운드 ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn first_round_only_records_then_a_new_holding_notifies_exactly_once() {
    let env = env().await;
    let (book_a, book_b) = (isbn(1), isbn(2));
    env.user_with_doc("u1", &[(&book_a, "책 A"), (&book_b, "책 B")], &[("111005", "행복도서관")]).await;
    let device = Device::new(3);
    env.subscribe("u1", &device, "phone").await;
    env.set_library(&book_a, "111005", false);
    env.set_library(&book_b, "111005", true);

    // 1라운드: 처음 보는 쌍은 상태만 기록한다 — 이미 소장인 책 B도 알리지 않는다.
    let first = env.round().await;
    assert_eq!((first.users, first.checked, first.errors, first.notified), (1, 2, 0, 0));
    assert!(env.pushes().is_empty());
    assert_eq!(env.state("u1", &book_a, "111005").await, Some(false));
    assert_eq!(env.state("u1", &book_b, "111005").await, Some(true));

    // 책 A가 도서관에 들어온다.
    env.set_library(&book_a, "111005", true);
    let second = env.round().await;
    assert_eq!(second.notified, 1);
    let pushes = env.pushes();
    assert_eq!(pushes.len(), 1);
    let message = device.decrypt(&pushes[0].2);
    assert_eq!(message["title"], "찜한 책이 소장됐어요");
    assert_eq!(message["body"], "‘책 A’이(가) 행복도서관에 들어왔어요.");
    assert_eq!(message["tag"], book_a);
    assert_eq!(env.state("u1", &book_a, "111005").await, Some(true));

    // 이미 알렸으니 다음 라운드에는 조용하다.
    let third = env.round().await;
    assert_eq!(third.notified, 0);
    assert_eq!(env.pushes().len(), 1);
}

#[tokio::test]
async fn several_libraries_for_one_book_make_one_notification_naming_them_all() {
    let env = env().await;
    let book = isbn(1);
    env.user_with_doc("u1", &[(&book, "책")], &[("111001", "가도서관"), ("111002", "나도서관")]).await;
    let device = Device::new(3);
    env.subscribe("u1", &device, "phone").await;
    env.set_library(&book, "111001", false);
    env.set_library(&book, "111002", false);
    env.round().await;
    env.set_library(&book, "111001", true);
    env.set_library(&book, "111002", true);
    let summary = env.round().await;
    assert_eq!(summary.notified, 1);
    let pushes = env.pushes();
    assert_eq!(pushes.len(), 1);
    assert_eq!(device.decrypt(&pushes[0].2)["body"], "‘책’이(가) 가도서관, 나도서관에 들어왔어요.");
}

#[tokio::test]
async fn the_notification_goes_to_every_device_of_the_account() {
    let env = env().await;
    let book = isbn(1);
    env.user_with_doc("u1", &[(&book, "책")], &[("111005", "도서관")]).await;
    let (phone, laptop) = (Device::new(3), Device::new(4));
    env.subscribe("u1", &phone, "phone").await;
    env.subscribe("u1", &laptop, "laptop").await;
    env.set_library(&book, "111005", false);
    env.round().await;
    env.set_library(&book, "111005", true);
    env.round().await;
    let pushes = env.pushes();
    assert_eq!(pushes.len(), 2);
    let names: Vec<&str> = pushes.iter().map(|p| p.0.as_str()).collect();
    assert!(names.contains(&"phone") && names.contains(&"laptop"));
    // 각 기기는 자기 키로만 풀 수 있다
    for (name, _, body) in &pushes {
        let device = if name == "phone" { &phone } else { &laptop };
        assert_eq!(device.decrypt(body)["tag"], book);
    }
}

#[tokio::test]
async fn when_no_device_receives_it_the_state_stays_so_the_next_round_retries() {
    let env = env().await;
    let book = isbn(1);
    env.user_with_doc("u1", &[(&book, "책")], &[("111005", "도서관")]).await;
    let device = Device::new(3);
    env.subscribe("u1", &device, "phone").await;
    env.set_library(&book, "111005", false);
    env.round().await;
    env.set_library(&book, "111005", true);

    env.fake.push_status.lock().unwrap().insert("phone".into(), 500);
    let failed = env.round().await;
    assert_eq!(failed.notified, 0);
    assert_eq!(env.state("u1", &book, "111005").await, Some(false), "알림이 못 갔으니 아직 미소장으로 둔다");

    env.fake.push_status.lock().unwrap().insert("phone".into(), 201);
    let retried = env.round().await;
    assert_eq!(retried.notified, 1);
    assert_eq!(env.state("u1", &book, "111005").await, Some(true));
}

#[tokio::test]
async fn gone_subscriptions_are_removed_and_repeated_failures_too() {
    let env = env().await;
    let book = isbn(1);
    env.user_with_doc("u1", &[(&book, "책")], &[("111005", "도서관")]).await;
    let device = Device::new(3);
    env.subscribe("u1", &device, "stale").await;
    env.set_library(&book, "111005", false);
    env.round().await;
    env.set_library(&book, "111005", true);

    env.fake.push_status.lock().unwrap().insert("stale".into(), 410);
    let summary = env.round().await;
    assert_eq!(summary.removed_subscriptions, 1);
    assert_eq!(env.sub_count().await, 0);

    // 계속 실패하는 구독은 MAX_FAILURES번째에 지운다
    env.subscribe("u1", &device, "flaky").await;
    env.fake.push_status.lock().unwrap().insert("flaky".into(), 500);
    for _ in 0..(MAX_FAILURES - 1) {
        env.round().await;
        assert_eq!(env.sub_count().await, 1);
    }
    env.round().await;
    assert_eq!(env.sub_count().await, 0);
}

#[tokio::test]
async fn a_failed_library_lookup_changes_nothing() {
    let env = env().await;
    let book = isbn(1);
    env.user_with_doc("u1", &[(&book, "책")], &[("111005", "도서관")]).await;
    env.subscribe("u1", &Device::new(3), "phone").await;
    // 도서관 API가 500을 돌려준다(등록하지 않은 쌍)
    let summary = env.round().await;
    assert_eq!((summary.checked, summary.errors, summary.notified), (1, 1, 0));
    assert_eq!(env.state("u1", &book, "111005").await, None, "확인하지 못한 쌍은 기록하지 않는다");
    assert!(env.pushes().is_empty());
}

#[tokio::test]
async fn a_lookup_failure_after_a_baseline_does_not_look_like_a_change() {
    let env = env().await;
    let book = isbn(1);
    env.user_with_doc("u1", &[(&book, "책")], &[("111005", "도서관")]).await;
    env.subscribe("u1", &Device::new(3), "phone").await;
    env.set_library(&book, "111005", false);
    env.round().await;
    env.fake.library.lock().unwrap().clear(); // 이번에는 API 오류
    env.round().await;
    assert_eq!(env.state("u1", &book, "111005").await, Some(false));
    assert!(env.pushes().is_empty());
}

#[tokio::test]
async fn the_check_budget_limits_library_calls() {
    let env = env_with(2).await;
    let books: Vec<String> = (1..=4).map(isbn).collect();
    let docs: Vec<(&str, &str)> = books.iter().map(|b| (b.as_str(), "책")).collect();
    env.user_with_doc("u1", &docs, &[("111005", "도서관")]).await;
    env.subscribe("u1", &Device::new(3), "phone").await;
    for book in &books {
        env.set_library(book, "111005", false);
    }
    let summary = env.round().await;
    assert_eq!(summary.checked, 2);
    assert_eq!(env.fake.library_calls.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn the_same_book_and_library_are_looked_up_once_across_accounts() {
    let env = env().await;
    let book = isbn(1);
    for user in ["u1", "u2"] {
        env.user_with_doc(user, &[(&book, "책")], &[("111005", "도서관")]).await;
    }
    env.subscribe("u1", &Device::new(3), "phone1").await;
    env.subscribe("u2", &Device::new(4), "phone2").await;
    env.set_library(&book, "111005", false);
    let summary = env.round().await;
    assert_eq!(summary.users, 2);
    assert_eq!(summary.checked, 1);
    assert_eq!(env.fake.library_calls.lock().unwrap().len(), 1);
    // 두 계정 모두 기준 상태가 기록된다
    assert_eq!(env.state("u1", &book, "111005").await, Some(false));
    assert_eq!(env.state("u2", &book, "111005").await, Some(false));
}

#[tokio::test]
async fn accounts_are_notified_only_about_their_own_books() {
    let env = env().await;
    let (mine, theirs) = (isbn(1), isbn(2));
    env.user_with_doc("u1", &[(&mine, "내 책")], &[("111005", "도서관")]).await;
    env.user_with_doc("u2", &[(&theirs, "남의 책")], &[("111005", "도서관")]).await;
    let (d1, d2) = (Device::new(3), Device::new(4));
    env.subscribe("u1", &d1, "d1").await;
    env.subscribe("u2", &d2, "d2").await;
    env.set_library(&mine, "111005", false);
    env.set_library(&theirs, "111005", false);
    env.round().await;
    env.set_library(&theirs, "111005", true);
    env.round().await;
    let pushes = env.pushes();
    assert_eq!(pushes.len(), 1);
    assert_eq!(pushes[0].0, "d2");
    assert_eq!(d2.decrypt(&pushes[0].2)["tag"], theirs);
}

#[tokio::test]
async fn many_new_holdings_are_merged_into_one_notification() {
    let env = env().await;
    let books: Vec<String> = (1..=5).map(isbn).collect();
    let docs: Vec<(&str, &str)> = books.iter().map(|b| (b.as_str(), "책")).collect();
    env.user_with_doc("u1", &docs, &[("111005", "도서관")]).await;
    let device = Device::new(3);
    env.subscribe("u1", &device, "phone").await;
    for book in &books {
        env.set_library(book, "111005", false);
    }
    env.round().await;
    for book in &books {
        env.set_library(book, "111005", true);
    }
    let summary = env.round().await;
    assert_eq!(summary.notified, 5);
    let pushes = env.pushes();
    assert_eq!(pushes.len(), 1, "5권이어도 한 통");
    assert_eq!(device.decrypt(&pushes[0].2)["body"], "찜한 책 5권이 자주 가는 도서관에 들어왔어요.");
    for book in &books {
        assert_eq!(env.state("u1", book, "111005").await, Some(true));
    }
}

#[tokio::test]
async fn unwatched_books_and_dropped_libraries_lose_their_state() {
    let env = env().await;
    let (keep, drop_book) = (isbn(1), isbn(2));
    env.user_with_doc("u1", &[(&keep, "남길 책"), (&drop_book, "뺄 책")], &[("111005", "도서관")]).await;
    env.subscribe("u1", &Device::new(3), "phone").await;
    env.set_library(&keep, "111005", false);
    env.set_library(&drop_book, "111005", false);
    env.round().await;
    assert_eq!(env.state("u1", &drop_book, "111005").await, Some(false));

    // 찜을 풀면(문서에서 사라지면) 그 책의 상태도 정리한다 — 다시 찜하면 처음 보는 쌍으로 시작한다.
    let doc = json!({ "v": 2, "books": {}, "favLibraries": { "111005": { "at": 1, "value": { "libCode": "111005", "libName": "도서관" } } },
        "watchlist": { keep.clone(): { "at": 1, "value": { "isbn13": keep, "title": "남길 책" } } } });
    query("UPDATE gotolib_sync SET data = ? WHERE user_id = 'u1'").bind(doc.to_string()).execute(&env.pool).await.unwrap();
    env.round().await;
    assert_eq!(env.state("u1", &drop_book, "111005").await, None);
    assert_eq!(env.state("u1", &keep, "111005").await, Some(false));
}

#[tokio::test]
async fn accounts_without_a_sync_document_or_targets_are_skipped_quietly() {
    let env = env().await;
    env.add_user("nodoc").await;
    env.subscribe("nodoc", &Device::new(3), "a").await;
    env.user_with_doc("empty", &[], &[]).await;
    env.subscribe("empty", &Device::new(4), "b").await;
    let summary = env.round().await;
    assert_eq!((summary.users, summary.checked, summary.notified), (2, 0, 0));
}

// ── 주기 ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn rounds_run_only_when_due_and_the_interval_survives_restarts() {
    let env = env().await;
    let interval = Duration::from_secs(12 * 3600);
    assert!(run_if_due(&env.pool, &env.service, 1_000, interval).await.is_some(), "처음에는 바로 돈다");
    assert!(run_if_due(&env.pool, &env.service, 1_000 + 3600, interval).await.is_none(), "1시간 뒤엔 아직");
    assert!(run_if_due(&env.pool, &env.service, 1_000 + 12 * 3600 - 1, interval).await.is_none());
    assert!(run_if_due(&env.pool, &env.service, 1_000 + 12 * 3600, interval).await.is_some());
    // 시각은 DB에 있으니 서비스를 새로 만들어도(재시작) 같은 판단을 한다
    let restarted = PushService::new(VapidKeys::generate("https://gotolib.example"), &env.base, true, 10, Duration::ZERO).unwrap();
    assert!(run_if_due(&env.pool, &restarted, 1_000 + 12 * 3600 + 5, interval).await.is_none());
}

// ── 구독 저장 ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn subscribing_twice_with_the_same_endpoint_keeps_one_row_and_a_device_can_move_accounts() {
    let env = env().await;
    env.add_user("u1").await;
    env.add_user("u2").await;
    let device = Device::new(3);
    env.subscribe("u1", &device, "phone").await;
    env.subscribe("u1", &device, "phone").await;
    assert_eq!(env.sub_count().await, 1);
    env.subscribe("u2", &device, "phone").await; // 같은 기기를 다른 계정이 구독 → 새 계정으로 옮겨진다
    assert_eq!(env.sub_count().await, 1);
    assert!(subscriptions_of(&env.pool, "u1").await.unwrap().is_empty());
    assert_eq!(subscriptions_of(&env.pool, "u2").await.unwrap().len(), 1);
}

#[tokio::test]
async fn an_account_keeps_at_most_the_newest_subscriptions() {
    let env = env().await;
    env.add_user("u1").await;
    for n in 0..(MAX_SUBSCRIPTIONS_PER_USER + 3) {
        let device = Device::new(n as u8 + 1);
        upsert_subscription(&env.pool, "u1", &device.input(&env.endpoint(&format!("d{n}"))), None, 100 + n)
            .await
            .unwrap();
    }
    let subs = subscriptions_of(&env.pool, "u1").await.unwrap();
    assert_eq!(subs.len() as i64, MAX_SUBSCRIPTIONS_PER_USER);
    let endpoints: Vec<&str> = subs.iter().map(|(_, t)| t.endpoint.as_str()).collect();
    assert!(!endpoints.iter().any(|e| e.ends_with("/d0")), "가장 오래된 구독부터 지워진다");
    assert!(endpoints.iter().any(|e| e.ends_with(&format!("/d{}", MAX_SUBSCRIPTIONS_PER_USER + 2))));
}

#[tokio::test]
async fn unsubscribing_only_removes_the_callers_own_subscription() {
    let env = env().await;
    env.add_user("u1").await;
    env.add_user("u2").await;
    env.subscribe("u1", &Device::new(3), "phone").await;
    assert_eq!(delete_subscription(&env.pool, "u2", &env.endpoint("phone")).await.unwrap(), 0);
    assert_eq!(env.sub_count().await, 1);
    assert_eq!(delete_subscription(&env.pool, "u1", &env.endpoint("phone")).await.unwrap(), 1);
    assert_eq!(env.sub_count().await, 0);
}

#[tokio::test]
async fn purging_an_inactive_account_also_removes_its_push_data() {
    let env = env().await;
    env.user_with_doc("u1", &[(&isbn(1), "책")], &[("111005", "도서관")]).await;
    env.subscribe("u1", &Device::new(3), "phone").await;
    save_state(&env.pool, "u1", &isbn(1), "111005", false, 1).await;
    query("UPDATE gotolib_sync SET updated_at = datetime('now', '-400 days') WHERE user_id = 'u1'")
        .execute(&env.pool)
        .await
        .unwrap();
    let removed = crate::db::purge_inactive_gotolib_accounts(&env.pool, 365).await.unwrap();
    assert_eq!(removed, 1);
    assert_eq!(env.sub_count().await, 0);
    assert_eq!(env.state("u1", &isbn(1), "111005").await, None);
}
