//! gotolib 찜한 책 소장 알림(Web Push): 찜 목록의 책이 '자주 가는 도서관'에 새로 들어오면 알린다.
//!
//! 흐름
//! - 앱이 푸시 구독(endpoint·키)을 서버에 등록한다(`routes::gotolib_push`). 기기마다 한 행.
//! - 수집 작업이 정해진 주기(기본 12시간)마다 구독한 계정의 동기화 문서에서 찜 목록과 자주 가는 도서관을 읽고,
//!   (책, 도서관) 쌍마다 소장 여부(bookExist)를 확인한다. 도서관 API 키는 nginx에만 있어서 같은 주소
//!   (`/api/library/`)로 부른다.
//! - '지난번에는 미소장 → 이번에는 소장'으로 바뀐 쌍만 알린다. 처음 보는 쌍은 상태만 기록한다(이미 소장인 책을
//!   알림으로 쏟아내지 않는다).
//! - 알림이 한 기기에도 가지 못하면 상태를 바꾸지 않아 다음 라운드에 다시 시도한다.
//!
//! 지켜야 할 것
//! - 구독 endpoint는 서버가 POST를 보내는 주소라서 알려진 푸시 서비스 호스트만 받는다(SSRF 방지).
//! - 도서관 API 호출은 라운드당 상한(`max_checks`)과 호출 사이 간격을 둔다(한도를 모르는 상태).
//! - VAPID 키는 서버가 처음 켜질 때 만들어 파일로 보관한다(Forgejo 비밀값으로 다루지 않으려고).
//!   파일을 잃으면 기존 구독은 못 쓰게 되어 앱이 다시 구독해야 한다.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    env, fs,
    path::Path,
    sync::Arc,
    time::Duration,
};

use base64::{decode_config, encode_config, URL_SAFE_NO_PAD};
use reqwest::Url;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{query, query_scalar, Row, SqlitePool};
use web_push_native::{
    jwt_simple::algorithms::ES256KeyPair,
    p256::{elliptic_curve::sec1::ToEncodedPoint, PublicKey},
    Auth, WebPushBuilder,
};

use crate::book_info::is_valid_isbn13;

/// 연락처를 따로 주지 않았을 때 VAPID `sub`에 넣는 주소(공개된 서비스 주소). 애플은 mailto: 또는 https: 를 요구한다.
pub const DEFAULT_CONTACT: &str = "https://gotolib.tyange.com";
/// 기기(구독)당 알림 유효 시간. 소장 소식은 하루가 지나면 의미가 줄어든다.
const PUSH_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);
/// 계정당 구독(기기) 수 상한. 넘으면 가장 오래된 것부터 지운다.
pub const MAX_SUBSCRIPTIONS_PER_USER: i64 = 5;
/// 연속으로 이만큼 실패한 구독은 지운다.
const MAX_FAILURES: i64 = 5;
/// 찜 목록 한도(앱의 WATCHLIST_LIMIT)와 자주 가는 도서관 읽기 상한.
const MAX_WATCHED_BOOKS: usize = 23;
const MAX_LIBS: usize = 10;
/// 한 라운드에 한 계정에게 책마다 따로 보내는 알림의 상한. 넘으면 한 통으로 합쳐 보낸다.
const MAX_INDIVIDUAL_NOTIFICATIONS: usize = 3;

pub const DEFAULT_INTERVAL_HOURS: u64 = 12;
pub const DEFAULT_MAX_CHECKS: usize = 300;
const DEFAULT_CHECK_DELAY: Duration = Duration::from_millis(400);
/// 서버가 켜진 뒤 첫 확인까지 기다리는 시간(배포 직후 곧바로 도서관 API를 두드리지 않게).
const STARTUP_DELAY: Duration = Duration::from_secs(5 * 60);
/// 라운드가 돌 때가 됐는지 이 간격으로 확인한다.
const TICK: Duration = Duration::from_secs(10 * 60);

/// 구독 endpoint로 허용하는 푸시 서비스 호스트(이 호스트이거나 그 하위 도메인).
const ALLOWED_PUSH_HOSTS: [&str; 4] = [
    "fcm.googleapis.com",
    "push.apple.com",
    "push.services.mozilla.com",
    "notify.windows.com",
];

// ── VAPID 키 ─────────────────────────────────────────────────────────────────
pub struct VapidKeys {
    key_pair: ES256KeyPair,
    /// 앱이 `pushManager.subscribe`의 applicationServerKey로 쓰는 공개 키(base64url, 65바이트)
    pub public_key: String,
    contact: String,
}

impl VapidKeys {
    /// 파일에서 읽고, 없으면 새로 만들어 저장한다(권한 600).
    pub fn load_or_create(path: &Path, contact: &str) -> Result<Self, String> {
        let key_pair = if path.exists() {
            let raw = fs::read_to_string(path).map_err(|e| format!("VAPID 키를 읽지 못했습니다: {e}"))?;
            let bytes = decode_config(raw.trim(), URL_SAFE_NO_PAD)
                .map_err(|e| format!("VAPID 키 형식이 올바르지 않습니다: {e}"))?;
            ES256KeyPair::from_bytes(&bytes).map_err(|e| format!("VAPID 키가 올바르지 않습니다: {e}"))?
        } else {
            let key_pair = ES256KeyPair::generate();
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("VAPID 키 폴더를 만들지 못했습니다: {e}"))?;
            }
            write_private(path, &encode_config(key_pair.to_bytes(), URL_SAFE_NO_PAD))
                .map_err(|e| format!("VAPID 키를 저장하지 못했습니다: {e}"))?;
            key_pair
        };
        Ok(Self::from_key_pair(key_pair, contact))
    }

    /// 파일 없이 메모리에서만(테스트).
    #[cfg(test)]
    pub fn generate(contact: &str) -> Self {
        Self::from_key_pair(ES256KeyPair::generate(), contact)
    }

    fn from_key_pair(key_pair: ES256KeyPair, contact: &str) -> Self {
        // applicationServerKey는 압축하지 않은 65바이트 점이다(ES256PublicKey::to_bytes는 압축형 33바이트).
        let compressed = key_pair.public_key().to_bytes();
        let uncompressed = PublicKey::from_sec1_bytes(&compressed)
            .expect("방금 만든 키의 공개 키는 올바르다")
            .to_encoded_point(false);
        let public_key = encode_config(uncompressed.as_bytes(), URL_SAFE_NO_PAD);
        Self {
            key_pair,
            public_key,
            contact: contact.to_string(),
        }
    }
}

#[cfg(unix)]
fn write_private(path: &Path, content: &str) -> std::io::Result<()> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let mut file = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
    file.write_all(content.as_bytes())
}

#[cfg(not(unix))]
fn write_private(path: &Path, content: &str) -> std::io::Result<()> {
    fs::write(path, content)
}

// ── 구독 검증 ────────────────────────────────────────────────────────────────
#[derive(Debug, Deserialize)]
pub struct SubscriptionKeys {
    pub p256dh: String,
    pub auth: String,
}

#[derive(Debug, Deserialize)]
pub struct SubscriptionInput {
    pub endpoint: String,
    pub keys: SubscriptionKeys,
}

fn host_allowed(host: &str) -> bool {
    ALLOWED_PUSH_HOSTS
        .iter()
        .any(|allowed| host == *allowed || host.ends_with(&format!(".{allowed}")))
}

fn is_loopback(host: &str) -> bool {
    host == "127.0.0.1" || host == "localhost" || host == "[::1]"
}

/// 서버가 POST를 보낼 주소와 암호화에 쓸 키가 올바른지 본다. `allow_loopback`은 테스트에서만 true.
pub fn validate_subscription(input: &SubscriptionInput, allow_loopback: bool) -> Result<(), &'static str> {
    if input.endpoint.len() > 600 {
        return Err("endpoint가 너무 깁니다.");
    }
    let url = Url::parse(&input.endpoint).map_err(|_| "endpoint 형식이 올바르지 않습니다.")?;
    let host = url.host_str().ok_or("endpoint에 호스트가 없습니다.")?;
    let loopback_ok = allow_loopback && is_loopback(host);
    if !loopback_ok && (url.scheme() != "https" || !host_allowed(host)) {
        return Err("지원하지 않는 푸시 서비스 주소입니다.");
    }
    let p256dh = decode_config(&input.keys.p256dh, URL_SAFE_NO_PAD).map_err(|_| "p256dh 키 형식이 올바르지 않습니다.")?;
    if p256dh.len() != 65 || PublicKey::from_sec1_bytes(&p256dh).is_err() {
        return Err("p256dh 키가 올바르지 않습니다.");
    }
    let auth = decode_config(&input.keys.auth, URL_SAFE_NO_PAD).map_err(|_| "auth 키 형식이 올바르지 않습니다.")?;
    if auth.len() != 16 {
        return Err("auth 키가 올바르지 않습니다.");
    }
    Ok(())
}

// ── 발송 ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct PushTarget {
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SendOutcome {
    Delivered,
    /// 푸시 서비스가 이 구독이 사라졌다고 답했다(404·410) → 지운다
    Gone,
    Failed,
}

/// 암호화(RFC 8291)·VAPID 서명을 해서 푸시 서비스에 보낸다.
pub async fn send_payload(http: &reqwest::Client, keys: &VapidKeys, target: &PushTarget, payload: &[u8]) -> SendOutcome {
    let Ok(request) = build_request(keys, target, payload) else {
        return SendOutcome::Failed;
    };
    let Ok(request) = reqwest::Request::try_from(request) else {
        return SendOutcome::Failed;
    };
    match http.execute(request).await {
        Ok(response) if response.status().is_success() => SendOutcome::Delivered,
        Ok(response) if matches!(response.status().as_u16(), 404 | 410) => SendOutcome::Gone,
        _ => SendOutcome::Failed,
    }
}

fn build_request(keys: &VapidKeys, target: &PushTarget, payload: &[u8]) -> Result<poem::http::Request<Vec<u8>>, String> {
    let endpoint = target.endpoint.parse().map_err(|_| "endpoint".to_string())?;
    let p256dh = decode_config(&target.p256dh, URL_SAFE_NO_PAD).map_err(|_| "p256dh".to_string())?;
    let public = PublicKey::from_sec1_bytes(&p256dh).map_err(|_| "p256dh".to_string())?;
    let auth = decode_config(&target.auth, URL_SAFE_NO_PAD).map_err(|_| "auth".to_string())?;
    if auth.len() != 16 {
        return Err("auth".to_string());
    }
    WebPushBuilder::new(endpoint, public, Auth::clone_from_slice(&auth))
        .with_valid_duration(PUSH_TTL)
        .with_vapid(&keys.key_pair, &keys.contact)
        .build(payload.to_vec())
        .map_err(|e| e.to_string())
}

/// 알림 본문(JSON). 서비스 워커가 읽어 `showNotification`에 쓴다.
pub fn notification_payload(title: &str, body: &str, url: &str, tag: &str) -> Vec<u8> {
    json!({ "title": title, "body": body, "url": url, "tag": tag }).to_string().into_bytes()
}

// ── 서비스(설정 + 클라이언트) ──────────────────────────────────────────────────
pub struct PushService {
    pub keys: VapidKeys,
    http: reqwest::Client,
    /// 도서관 API를 부르는 앞부분(경로 제외). 운영은 우리 사이트의 nginx 프록시를 쓴다.
    library_base: String,
    allow_loopback: bool,
    max_checks: usize,
    check_delay: Duration,
}

impl PushService {
    pub fn new(keys: VapidKeys, library_base: &str, allow_loopback: bool, max_checks: usize, check_delay: Duration) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder()
            .user_agent(format!("gotolib-push/0.1 (+{})", keys.contact))
            .timeout(HTTP_TIMEOUT)
            .build()?;
        Ok(Self {
            keys,
            http,
            library_base: library_base.trim_end_matches('/').to_string(),
            allow_loopback,
            max_checks,
            check_delay,
        })
    }

    pub fn allow_loopback(&self) -> bool {
        self.allow_loopback
    }
}

/// 라우트가 `.data()`로 받는 값. 알림이 꺼져 있으면 비어 있다(구독·키 요청은 503).
#[derive(Clone)]
pub struct PushHandle(pub Option<Arc<PushService>>);

/// 환경 변수로 알림을 켠다. 기본은 꺼짐.
/// - `GOTOLIB_PUSH_ENABLED=true` 일 때만 켠다
/// - `VAPID_KEY_PATH`(기본 `./data/vapid_private.key`): 없으면 새로 만든다
/// - `VAPID_SUBJECT`(선택): VAPID 연락처(mailto: 또는 https:). 없으면 `DEFAULT_CONTACT`
/// - `GOTOLIB_LIBRARY_BASE`(선택): 소장 조회를 보낼 주소. 없으면 https://gotolib.tyange.com
/// - `GOTOLIB_PUSH_MAX_CHECKS`(선택): 라운드당 도서관 API 호출 상한(기본 300)
pub fn handle_from_env() -> PushHandle {
    if env::var("GOTOLIB_PUSH_ENABLED").map(|v| v != "true").unwrap_or(true) {
        println!("[push] 찜 소장 알림이 꺼져 있습니다(GOTOLIB_PUSH_ENABLED).");
        return PushHandle(None);
    }
    let path = env::var("VAPID_KEY_PATH")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "./data/vapid_private.key".to_string());
    let contact = env::var("VAPID_SUBJECT")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| v.starts_with("mailto:") || v.starts_with("https://"))
        .unwrap_or_else(|| DEFAULT_CONTACT.to_string());
    let keys = match VapidKeys::load_or_create(Path::new(&path), &contact) {
        Ok(keys) => keys,
        Err(e) => {
            eprintln!("[push] {e} — 알림을 켜지 않습니다.");
            return PushHandle(None);
        }
    };
    let base = env::var("GOTOLIB_LIBRARY_BASE")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_CONTACT.to_string());
    let max_checks = env::var("GOTOLIB_PUSH_MAX_CHECKS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(DEFAULT_MAX_CHECKS);
    match PushService::new(keys, &base, false, max_checks, DEFAULT_CHECK_DELAY) {
        Ok(service) => PushHandle(Some(Arc::new(service))),
        Err(e) => {
            eprintln!("[push] HTTP 클라이언트를 만들지 못했습니다: {e}");
            PushHandle(None)
        }
    }
}

// ── 구독 저장 ────────────────────────────────────────────────────────────────
pub async fn upsert_subscription(
    pool: &SqlitePool,
    user_id: &str,
    input: &SubscriptionInput,
    user_agent: Option<&str>,
    now: i64,
) -> Result<(), sqlx::Error> {
    let agent = user_agent.map(|a| a.chars().take(200).collect::<String>());
    // 같은 기기(endpoint)가 다른 계정으로 다시 구독하면 새 계정 것으로 옮긴다.
    query(
        "INSERT INTO gotolib_push_subscriptions (user_id, endpoint, p256dh, auth, user_agent, created_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(endpoint) DO UPDATE SET user_id = excluded.user_id, p256dh = excluded.p256dh,
             auth = excluded.auth, user_agent = excluded.user_agent, failure_count = 0",
    )
    .bind(user_id)
    .bind(&input.endpoint)
    .bind(&input.keys.p256dh)
    .bind(&input.keys.auth)
    .bind(agent)
    .bind(now)
    .execute(pool)
    .await?;
    // 계정당 상한: 가장 오래된 구독부터 지운다.
    query(
        "DELETE FROM gotolib_push_subscriptions WHERE user_id = ?1 AND sub_id NOT IN (
             SELECT sub_id FROM gotolib_push_subscriptions WHERE user_id = ?1 ORDER BY created_at DESC, sub_id DESC LIMIT ?2)",
    )
    .bind(user_id)
    .bind(MAX_SUBSCRIPTIONS_PER_USER)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_subscription(pool: &SqlitePool, user_id: &str, endpoint: &str) -> Result<u64, sqlx::Error> {
    Ok(query("DELETE FROM gotolib_push_subscriptions WHERE user_id = ? AND endpoint = ?")
        .bind(user_id)
        .bind(endpoint)
        .execute(pool)
        .await?
        .rows_affected())
}

pub async fn subscriptions_of(pool: &SqlitePool, user_id: &str) -> Result<Vec<(i64, PushTarget)>, sqlx::Error> {
    let rows = query("SELECT sub_id, endpoint, p256dh, auth FROM gotolib_push_subscriptions WHERE user_id = ? ORDER BY sub_id")
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    rows.iter()
        .map(|row| {
            Ok((
                row.try_get("sub_id")?,
                PushTarget {
                    endpoint: row.try_get("endpoint")?,
                    p256dh: row.try_get("p256dh")?,
                    auth: row.try_get("auth")?,
                },
            ))
        })
        .collect()
}

/// 한 계정의 모든 구독에 보내고 (전달된 수, 지워진 수)를 돌려준다. 실패가 쌓인 구독은 지운다.
pub async fn send_to_user(pool: &SqlitePool, service: &PushService, user_id: &str, payload: &[u8], now: i64) -> (usize, usize) {
    let Ok(subs) = subscriptions_of(pool, user_id).await else {
        return (0, 0);
    };
    let (mut delivered, mut removed) = (0, 0);
    for (sub_id, target) in subs {
        match send_payload(&service.http, &service.keys, &target, payload).await {
            SendOutcome::Delivered => {
                delivered += 1;
                let _ = query("UPDATE gotolib_push_subscriptions SET last_success_at = ?, failure_count = 0 WHERE sub_id = ?")
                    .bind(now)
                    .bind(sub_id)
                    .execute(pool)
                    .await;
            }
            SendOutcome::Gone => {
                removed += 1;
                let _ = query("DELETE FROM gotolib_push_subscriptions WHERE sub_id = ?").bind(sub_id).execute(pool).await;
            }
            SendOutcome::Failed => {
                let _ = query("UPDATE gotolib_push_subscriptions SET failure_count = failure_count + 1 WHERE sub_id = ?")
                    .bind(sub_id)
                    .execute(pool)
                    .await;
                let failures: i64 = query_scalar("SELECT failure_count FROM gotolib_push_subscriptions WHERE sub_id = ?")
                    .bind(sub_id)
                    .fetch_one(pool)
                    .await
                    .unwrap_or(0);
                if failures >= MAX_FAILURES {
                    removed += 1;
                    let _ = query("DELETE FROM gotolib_push_subscriptions WHERE sub_id = ?").bind(sub_id).execute(pool).await;
                }
            }
        }
    }
    (delivered, removed)
}

// ── 동기화 문서에서 감시 대상 읽기 ─────────────────────────────────────────────
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct WatchedBook {
    pub isbn13: String,
    pub title: String,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct WatchedLibrary {
    pub lib_code: String,
    pub name: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Targets {
    pub books: Vec<WatchedBook>,
    pub libraries: Vec<WatchedLibrary>,
}

/// 앱의 동기화 문서(`{ watchlist: { id: { at, deleted?, value } }, favLibraries: {...} }`)에서 찜한 책과
/// 자주 가는 도서관을 읽는다. 지운 항목·ISBN이 올바르지 않은 책·이상한 도서관 코드는 건너뛴다.
pub fn parse_targets(doc: &Value) -> Targets {
    let mut books = BTreeMap::new();
    if let Some(watchlist) = doc.get("watchlist").and_then(Value::as_object) {
        for entry in watchlist.values() {
            if entry.get("deleted").is_some() {
                continue;
            }
            let Some(value) = entry.get("value") else { continue };
            let Some(isbn) = value.get("isbn13").and_then(Value::as_str) else { continue };
            if !is_valid_isbn13(isbn) {
                continue;
            }
            let title = value.get("title").and_then(Value::as_str).unwrap_or("").trim().to_string();
            books.entry(isbn.to_string()).or_insert(title);
        }
    }
    let mut libraries = BTreeMap::new();
    if let Some(favorites) = doc.get("favLibraries").and_then(Value::as_object) {
        for entry in favorites.values() {
            if entry.get("deleted").is_some() {
                continue;
            }
            let Some(value) = entry.get("value") else { continue };
            let Some(code) = value.get("libCode").and_then(Value::as_str) else { continue };
            if code.is_empty() || code.len() > 12 || !code.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            let name = value.get("libName").and_then(Value::as_str).unwrap_or("").trim().to_string();
            libraries.entry(code.to_string()).or_insert(name);
        }
    }
    Targets {
        books: books
            .into_iter()
            .take(MAX_WATCHED_BOOKS)
            .map(|(isbn13, title)| WatchedBook { isbn13, title })
            .collect(),
        libraries: libraries
            .into_iter()
            .take(MAX_LIBS)
            .map(|(lib_code, name)| WatchedLibrary { lib_code, name })
            .collect(),
    }
}

async fn load_targets(pool: &SqlitePool, user_id: &str) -> Targets {
    let data: Option<String> = query_scalar("SELECT data FROM gotolib_sync WHERE user_id = ?")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
    data.and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .map(|doc| parse_targets(&doc))
        .unwrap_or_default()
}

// ── 변화 판정 ────────────────────────────────────────────────────────────────
#[derive(Debug, PartialEq, Eq)]
pub enum Change {
    /// 미소장이던 것이 소장이 됐다 → 알린다
    Notify,
    /// 알릴 일은 없고 지금 값을 기록만 한다(처음 보는 쌍, 그대로, 소장 → 미소장)
    Record,
}

pub fn change(previous: Option<bool>, now_has_book: bool) -> Change {
    if previous == Some(false) && now_has_book {
        Change::Notify
    } else {
        Change::Record
    }
}

// ── 소장 조회 ────────────────────────────────────────────────────────────────
/// 도서관 정보나루 `bookExist`(우리 nginx 프록시 경유)로 이 도서관의 소장 여부를 본다. 응답이 이상하면 `Err`.
pub async fn check_has_book(http: &reqwest::Client, base: &str, isbn13: &str, lib_code: &str) -> Result<bool, String> {
    let response = http
        .get(format!("{base}/api/library/bookExist"))
        .query(&[("isbn13", isbn13), ("libCode", lib_code)])
        .send()
        .await
        .map_err(|e| format!("요청 실패: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let body: Value = response.json().await.map_err(|e| format!("응답을 읽지 못했습니다: {e}"))?;
    body.pointer("/response/result/hasBook")
        .and_then(Value::as_str)
        .map(|flag| flag == "Y")
        .ok_or_else(|| "응답에 hasBook이 없습니다.".to_string())
}

// ── 상태 저장 ────────────────────────────────────────────────────────────────
async fn load_states(pool: &SqlitePool, user_id: &str) -> HashMap<(String, String), bool> {
    let rows = query("SELECT isbn13, lib_code, has_book FROM gotolib_watch_state WHERE user_id = ?")
        .bind(user_id)
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    rows.iter()
        .filter_map(|row| {
            Some((
                (row.try_get("isbn13").ok()?, row.try_get("lib_code").ok()?),
                row.try_get::<i64, _>("has_book").ok()? != 0,
            ))
        })
        .collect()
}

async fn save_state(pool: &SqlitePool, user_id: &str, isbn13: &str, lib_code: &str, has_book: bool, now: i64) {
    let _ = query(
        "INSERT INTO gotolib_watch_state (user_id, isbn13, lib_code, has_book, checked_at) VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(user_id, isbn13, lib_code) DO UPDATE SET has_book = excluded.has_book, checked_at = excluded.checked_at",
    )
    .bind(user_id)
    .bind(isbn13)
    .bind(lib_code)
    .bind(i64::from(has_book))
    .bind(now)
    .execute(pool)
    .await;
}

// ── 한 라운드 ────────────────────────────────────────────────────────────────
/// (도서관 코드, 도서관 이름)
type LibraryRef = (String, String);
/// 새로 소장이 된 책 하나: (ISBN, 제목, 소장한 도서관들)
type HeldBook = (String, String, Vec<LibraryRef>);
/// 한 통의 알림: (담고 있는 책들의 `HeldBook` 위치, 알림 본문)
type NotificationGroup = (Vec<usize>, Vec<u8>);

#[derive(Debug, Default, PartialEq, Eq)]
pub struct RoundSummary {
    pub users: usize,
    pub checked: usize,
    pub errors: usize,
    pub notified: usize,
    pub removed_subscriptions: usize,
}

/// 구독한 계정 모두에 대해 소장 여부를 확인하고, 새로 소장이 된 책을 알린다.
pub async fn run_round(pool: &SqlitePool, service: &PushService, now: i64) -> RoundSummary {
    let mut summary = RoundSummary::default();
    let users: Vec<String> = query_scalar("SELECT DISTINCT user_id FROM gotolib_push_subscriptions ORDER BY user_id")
        .fetch_all(pool)
        .await
        .unwrap_or_default();

    // 여러 계정이 같은 (책, 도서관)을 보면 한 번만 부른다.
    let mut cache: HashMap<(String, String), Option<bool>> = HashMap::new();
    let mut budget = service.max_checks;

    for user_id in users {
        summary.users += 1;
        let targets = load_targets(pool, &user_id).await;
        let previous = load_states(pool, &user_id).await;

        // 책별로 새로 소장이 된 도서관 이름들
        let mut newly_held: BTreeMap<String, (String, Vec<LibraryRef>)> = BTreeMap::new();
        let mut current_pairs: HashSet<(String, String)> = HashSet::new();

        for book in &targets.books {
            for library in &targets.libraries {
                let key = (book.isbn13.clone(), library.lib_code.clone());
                current_pairs.insert(key.clone());

                let result = if let Some(cached) = cache.get(&key) {
                    *cached
                } else if budget == 0 {
                    None
                } else {
                    budget -= 1;
                    summary.checked += 1;
                    let result = check_has_book(&service.http, &service.library_base, &key.0, &key.1).await.ok();
                    if result.is_none() {
                        summary.errors += 1;
                    }
                    cache.insert(key.clone(), result);
                    tokio::time::sleep(service.check_delay).await;
                    result
                };
                // 확인하지 못한 쌍은 상태를 그대로 둔다(알리지도, 기록하지도 않는다).
                let Some(has_book) = result else { continue };

                match change(previous.get(&key).copied(), has_book) {
                    Change::Notify => {
                        let entry = newly_held
                            .entry(book.isbn13.clone())
                            .or_insert_with(|| (book.title.clone(), Vec::new()));
                        entry.1.push((library.lib_code.clone(), library.name.clone()));
                    }
                    Change::Record => save_state(pool, &user_id, &key.0, &key.1, has_book, now).await,
                }
            }
        }

        // 더는 찜하지 않는 책·도서관의 상태는 지운다(다시 찜하면 처음 보는 쌍으로 시작한다).
        for key in previous.keys() {
            if !current_pairs.contains(key) {
                let _ = query("DELETE FROM gotolib_watch_state WHERE user_id = ? AND isbn13 = ? AND lib_code = ?")
                    .bind(&user_id)
                    .bind(&key.0)
                    .bind(&key.1)
                    .execute(pool)
                    .await;
            }
        }

        if newly_held.is_empty() {
            continue;
        }
        let books: Vec<HeldBook> = newly_held
            .into_iter()
            .map(|(isbn, (title, libs))| (isbn, title, libs))
            .collect();

        // 알림 묶음: 몇 권 안 되면 책마다, 많으면 한 통으로 합친다.
        let groups: Vec<NotificationGroup> = if books.len() <= MAX_INDIVIDUAL_NOTIFICATIONS {
            books
                .iter()
                .enumerate()
                .map(|(i, (isbn, title, libs))| (vec![i], held_payload(isbn, title, libs)))
                .collect()
        } else {
            vec![((0..books.len()).collect(), summary_payload(books.len()))]
        };

        for (members, payload) in groups {
            let (delivered, removed) = send_to_user(pool, service, &user_id, &payload, now).await;
            summary.removed_subscriptions += removed;
            // 한 기기에도 가지 못했으면 상태를 바꾸지 않아 다음 라운드에 다시 시도한다.
            if delivered == 0 {
                continue;
            }
            summary.notified += members.len();
            for index in members {
                let (isbn, _, libs) = &books[index];
                for (lib_code, _) in libs {
                    save_state(pool, &user_id, isbn, lib_code, true, now).await;
                }
            }
        }
    }
    summary
}

fn held_payload(isbn13: &str, title: &str, libs: &[LibraryRef]) -> Vec<u8> {
    let names: Vec<&str> = libs.iter().map(|(_, name)| name.as_str()).filter(|n| !n.is_empty()).collect();
    let place = if names.is_empty() { "자주 가는 도서관".to_string() } else { names.join(", ") };
    let title = if title.is_empty() { "찜한 책" } else { title };
    notification_payload("찜한 책이 소장됐어요", &format!("‘{title}’이(가) {place}에 들어왔어요."), "/", isbn13)
}

fn summary_payload(count: usize) -> Vec<u8> {
    notification_payload(
        "찜한 책이 소장됐어요",
        &format!("찜한 책 {count}권이 자주 가는 도서관에 들어왔어요."),
        "/",
        "gotolib-held",
    )
}

// ── 주기 ─────────────────────────────────────────────────────────────────────
async fn is_due(pool: &SqlitePool, now: i64, interval: Duration) -> bool {
    let last: Option<i64> = query_scalar("SELECT last_run_at FROM gotolib_push_runs WHERE id = 1")
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
    last.is_none_or(|last| now - last >= interval.as_secs() as i64)
}

async fn mark_run(pool: &SqlitePool, now: i64) {
    let _ = query(
        "INSERT INTO gotolib_push_runs (id, last_run_at) VALUES (1, ?) ON CONFLICT(id) DO UPDATE SET last_run_at = excluded.last_run_at",
    )
    .bind(now)
    .execute(pool)
    .await;
}

/// 라운드가 돌 때가 됐으면 한 번 돌린다(주기는 DB에 기록해서 서버를 자주 재시작해도 지켜진다). 돌았으면 요약을 돌려준다.
pub async fn run_if_due(pool: &SqlitePool, service: &PushService, now: i64, interval: Duration) -> Option<RoundSummary> {
    if !is_due(pool, now, interval).await {
        return None;
    }
    // 라운드가 중간에 죽어도 곧바로 다시 돌지 않게 시작할 때 기록한다.
    mark_run(pool, now).await;
    Some(run_round(pool, service, now).await)
}

/// 감독 루프가 수집 작업을 띄운다. 패닉으로 죽으면 잠시 뒤 다시 띄운다.
pub fn spawn_worker(pool: SqlitePool, service: Arc<PushService>) {
    let hours = env::var("GOTOLIB_PUSH_INTERVAL_HOURS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|h| *h >= 1)
        .unwrap_or(DEFAULT_INTERVAL_HOURS);
    let interval = Duration::from_secs(hours * 60 * 60);
    println!("[push] 찜 소장 알림 시작 (확인 주기 {hours}시간)");
    tokio::spawn(async move {
        loop {
            let handle = tokio::spawn(worker_loop(pool.clone(), service.clone(), interval));
            if let Err(e) = handle.await {
                eprintln!("[push] 작업이 비정상 종료했습니다: {e}. 1분 뒤 다시 시작합니다.");
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
}

async fn worker_loop(pool: SqlitePool, service: Arc<PushService>, interval: Duration) {
    tokio::time::sleep(STARTUP_DELAY).await;
    loop {
        if let Some(summary) = run_if_due(&pool, &service, crate::book_info::now(), interval).await {
            println!(
                "[push] 라운드 완료: 계정 {}, 조회 {}(실패 {}), 알림 {}, 정리된 구독 {}",
                summary.users, summary.checked, summary.errors, summary.notified, summary.removed_subscriptions
            );
        }
        tokio::time::sleep(TICK).await;
    }
}

#[cfg(test)]
pub(crate) mod tests;
