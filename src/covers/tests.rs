use std::{
    collections::HashMap,
    io::Cursor,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use image::{DynamicImage, ImageFormat, RgbImage};
use poem::{
    get, handler,
    http::StatusCode,
    listener::TcpAcceptor,
    web::{Data, Path},
    EndpointExt, Request, Response, Route, Server,
};
use sqlx::{query, query_scalar, SqlitePool};

use super::*;
use crate::db::init_db;

/// 9자리 번호로 검사 숫자까지 맞는 ISBN-13을 만든다.
pub(crate) fn isbn(n: u32) -> String {
    let body = format!("978{n:09}");
    let sum: u32 = body
        .bytes()
        .enumerate()
        .map(|(i, d)| u32::from(d - b'0') * if i % 2 == 0 { 1 } else { 3 })
        .sum();
    format!("{body}{}", (10 - sum % 10) % 10)
}

fn jpeg(width: u32, height: u32) -> Vec<u8> {
    let img = RgbImage::from_fn(width, height, |x, y| image::Rgb([(x % 255) as u8, (y % 255) as u8, 90]));
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(img).write_to(&mut out, ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

// ── 순수 함수 ────────────────────────────────────────────────────────────────

#[test]
fn accepts_real_isbn13s() {
    for value in ["9788934972464", "9788936434267", "9791189799847", "9788962624601"] {
        assert!(is_valid_isbn13(value), "{value}");
    }
}

#[test]
fn rejects_malformed_isbns() {
    let cases = [
        "",
        "978893497246",      // 12자리
        "97889349724644",    // 14자리
        "9788934972465",     // 검사 숫자 틀림
        "978893497246x",     // 문자
        " 9788934972464",    // 공백
        "9788934972464 ",
        "../etc/passwd",
        "9788934972464%00",
        "1288934972464",     // 접두 978/979 아님
        "９７８８９３４９７２４６４", // 전각 숫자
    ];
    for value in cases {
        assert!(!is_valid_isbn13(value), "{value:?}");
    }
}

#[test]
fn generated_isbns_pass_the_validator() {
    for n in [1, 2, 99, 123456789] {
        assert!(is_valid_isbn13(&isbn(n)));
    }
}

#[test]
fn extracts_og_image_regardless_of_attribute_order_and_quotes() {
    let a = r#"<html><head><meta charset="utf-8"><meta property="og:image" content="https://image.aladin.co.kr/product/1/2/cover500/x.jpg" /></head>"#;
    let b = r#"<meta content='https://image.aladin.co.kr/product/1/2/cover500/y.jpg' property='og:image'>"#;
    let c = r#"<meta  property="og:title" content="x"><meta property="og:image"   content="https://image.aladin.co.kr/product/3/cover500/z.jpg">"#;
    assert_eq!(extract_og_image(a).as_deref(), Some("https://image.aladin.co.kr/product/1/2/cover500/x.jpg"));
    assert_eq!(extract_og_image(b).as_deref(), Some("https://image.aladin.co.kr/product/1/2/cover500/y.jpg"));
    assert_eq!(extract_og_image(c).as_deref(), Some("https://image.aladin.co.kr/product/3/cover500/z.jpg"));
}

#[test]
fn og_image_is_none_when_absent_or_empty() {
    assert_eq!(extract_og_image("<html></html>"), None);
    assert_eq!(extract_og_image(r#"<meta property="og:image" content="">"#), None);
    assert_eq!(extract_og_image(r#"<meta property="og:title" content="x">"#), None);
    assert_eq!(extract_og_image(r#"<meta property="og:image""#), None); // 닫히지 않은 태그
}

#[test]
fn config_only_allows_https_aladin_cover_paths() {
    let config = SourceConfig::aladin();
    assert!(config.allows("https://image.aladin.co.kr/product/31424/4/cover500/k482832219_1.jpg"));
    assert!(!config.allows("http://image.aladin.co.kr/product/1/cover500/a.jpg")); // http
    assert!(!config.allows("https://evil.example/product/1/cover500/a.jpg")); // 다른 호스트
    assert!(!config.allows("https://image.aladin.co.kr.evil.example/product/1/a.jpg")); // 호스트 위장
    assert!(!config.allows("https://image.aladin.co.kr/img/common/logo.png")); // 표지 경로 아님
    assert!(!config.allows("not a url"));
}

#[test]
fn thumbnail_shrinks_large_images_and_never_enlarges_small_ones() {
    let big = make_thumbnail(&jpeg(800, 1200)).unwrap();
    let decoded = image::load_from_memory(&big).unwrap();
    assert!(decoded.width() <= THUMB_WIDTH && decoded.height() <= THUMB_MAX_HEIGHT);
    assert!(decoded.width() == THUMB_WIDTH || decoded.height() == THUMB_MAX_HEIGHT);

    let small = make_thumbnail(&jpeg(60, 90)).unwrap();
    let decoded = image::load_from_memory(&small).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (60, 90));
}

#[test]
fn user_agent_names_the_service_and_the_contact() {
    assert_eq!(user_agent("https://gotolib.tyange.com"), "gotolib-covers/0.1 (+https://gotolib.tyange.com)");
}

#[test]
fn contact_falls_back_to_the_public_site_when_unset_or_blank() {
    assert_eq!(contact_from(None), DEFAULT_CONTACT);
    assert_eq!(contact_from(Some("")), DEFAULT_CONTACT);
    assert_eq!(contact_from(Some("   ")), DEFAULT_CONTACT);
    assert_eq!(contact_from(Some(" me@example.com ")), "me@example.com");
}

#[test]
fn thumbnail_rejects_garbage_without_panicking() {
    assert!(make_thumbnail(b"this is not an image").is_err());
    assert!(make_thumbnail(b"").is_err());
    assert!(make_thumbnail(&jpeg(40, 40)[..50]).is_err()); // 잘린 파일
}

// ── 파일 저장 ────────────────────────────────────────────────────────────────

fn temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("covers-test-{}", uuid::Uuid::new_v4()))
}

#[test]
fn store_writes_final_file_and_leaves_no_temp_file() {
    let dir = temp_dir();
    let id = isbn(7);
    store_thumbnail(&dir, &id, &jpeg(500, 700)).unwrap();
    let path = cover_path(&dir, &id).unwrap();
    assert!(path.exists());
    assert!(!path.with_extension("jpg.tmp").exists());
    let written = fs::read(&path).unwrap();
    assert_eq!(&written[..2], &[0xFF, 0xD8]); // JPEG 시작 표지
    assert!(written.len() < 40 * 1024, "썸네일이 너무 크다: {}", written.len());
    // 같은 ISBN을 다시 저장해도 덮어쓴다
    store_thumbnail(&dir, &id, &jpeg(300, 400)).unwrap();
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn store_refuses_bad_isbn_and_bad_image() {
    let dir = temp_dir();
    assert!(store_thumbnail(&dir, "../evil", &jpeg(50, 50)).is_err());
    assert!(store_thumbnail(&dir, &isbn(8), b"nope").is_err());
    assert!(cover_path(&dir, "../evil").is_none());
    assert!(!dir.join("../evil.jpg").exists());
    let _ = fs::remove_dir_all(dir);
}

// ── 수집 작업 (가짜 알라딘 서버) ───────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Scenario {
    Ok,
    NoCover,
    Forbidden,
    ServerError,
    NotAnImage,
    ForeignImage,
}

type Scenarios = Arc<Mutex<HashMap<String, Scenario>>>;

fn scenario_of(scenarios: &Scenarios, isbn: &str) -> Scenario {
    scenarios.lock().unwrap().get(isbn).copied().unwrap_or(Scenario::NoCover)
}

#[handler]
fn fake_page(req: &Request, scenarios: Data<&Scenarios>) -> Response {
    let isbn = req
        .uri()
        .query()
        .and_then(|q| q.strip_prefix("ISBN="))
        .unwrap_or_default()
        .to_string();
    let host = req.header("host").unwrap_or("127.0.0.1").to_string();
    let og = |url: String| format!(r#"<html><head><meta property="og:image" content="{url}"></head></html>"#);
    match scenario_of(&scenarios, &isbn) {
        Scenario::Forbidden => Response::builder().status(StatusCode::FORBIDDEN).finish(),
        Scenario::ServerError => Response::builder().status(StatusCode::INTERNAL_SERVER_ERROR).finish(),
        Scenario::NoCover => Response::builder().body("<html><head></head></html>"),
        Scenario::ForeignImage => Response::builder().body(og("http://evil.example/product/1/x.jpg".to_string())),
        Scenario::Ok | Scenario::NotAnImage => {
            Response::builder().body(og(format!("http://{host}/product/1/cover500/{isbn}.jpg")))
        }
    }
}

#[handler]
fn fake_image(Path(name): Path<String>, scenarios: Data<&Scenarios>) -> Response {
    let isbn = name.trim_end_matches(".jpg");
    match scenario_of(&scenarios, isbn) {
        Scenario::NotAnImage => Response::builder().content_type("text/html").body("nope"),
        _ => Response::builder().content_type("image/jpeg").body(jpeg(600, 900)),
    }
}

struct Fixture {
    pool: SqlitePool,
    source: CoverSource,
    dir: PathBuf,
    scenarios: Scenarios,
}

async fn fixture() -> Fixture {
    let scenarios: Scenarios = Arc::default();
    let app = Route::new()
        .at("/shop/wproduct.aspx", get(fake_page))
        .at("/product/1/cover500/:name", get(fake_image))
        .data(scenarios.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(Server::new_with_acceptor(TcpAcceptor::from_tokio(listener).unwrap()).run(app));

    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    init_db(&pool).await.unwrap();
    let config = SourceConfig {
        page_base: format!("http://127.0.0.1:{port}"),
        image_hosts: vec!["127.0.0.1".to_string()],
        allow_http: true,
    };
    Fixture {
        pool,
        source: CoverSource::new(config, "gotolib-test").unwrap(),
        dir: temp_dir(),
        scenarios,
    }
}

impl Fixture {
    async fn add(&self, isbn: &str, scenario: Scenario) {
        self.scenarios.lock().unwrap().insert(isbn.to_string(), scenario);
        enqueue(&self.pool, &[isbn.to_string()], 1_000).await.unwrap();
    }

    async fn step(&self, now: i64) -> Step {
        process_next(&self.pool, &self.source, &self.dir, now).await
    }

    async fn status(&self, isbn: &str) -> String {
        query_scalar("SELECT status FROM book_covers WHERE isbn13 = ?")
            .bind(isbn)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    async fn field(&self, isbn: &str, column: &str) -> i64 {
        query_scalar(&format!("SELECT {column} FROM book_covers WHERE isbn13 = ?"))
            .bind(isbn)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn idle_when_queue_is_empty() {
    let f = fixture().await;
    assert_eq!(f.step(2_000).await, Step::Idle);
}

#[tokio::test]
async fn found_cover_is_downloaded_shrunk_and_recorded() {
    let f = fixture().await;
    let id = isbn(1);
    f.add(&id, Scenario::Ok).await;

    assert_eq!(f.step(2_000).await, Step::Processed);
    assert_eq!(f.status(&id).await, STATUS_OK);
    let written = fs::read(cover_path(&f.dir, &id).unwrap()).unwrap();
    let decoded = image::load_from_memory(&written).unwrap();
    assert!(decoded.width() <= THUMB_WIDTH);
    assert_eq!(f.field(&id, "fetched_at").await, 2_000);
    // 처리한 건은 큐에서 빠진다
    assert_eq!(f.step(3_000).await, Step::Idle);
}

#[tokio::test]
async fn page_without_cover_becomes_missing_with_long_retry() {
    let f = fixture().await;
    let id = isbn(2);
    f.add(&id, Scenario::NoCover).await;

    assert_eq!(f.step(2_000).await, Step::Processed);
    assert_eq!(f.status(&id).await, STATUS_MISSING);
    assert_eq!(f.field(&id, "next_try_at").await, 2_000 + MISSING_RETRY_SECS);
    assert!(!cover_path(&f.dir, &id).unwrap().exists());
}

#[tokio::test]
async fn forbidden_blocks_the_whole_worker_and_leaves_the_row_untouched() {
    let f = fixture().await;
    let blocked = isbn(3);
    let other = isbn(4);
    f.add(&blocked, Scenario::Forbidden).await;
    f.add(&other, Scenario::Ok).await;

    assert_eq!(f.step(2_000).await, Step::Blocked);
    assert_eq!(f.status(&blocked).await, STATUS_PENDING);
    assert_eq!(f.field(&blocked, "attempts").await, 0);
    // 막힘은 행 하나의 실패가 아니라서 아무것도 기록하지 않는다(다음 행도 그대로)
    assert_eq!(f.status(&other).await, STATUS_PENDING);
}

#[tokio::test]
async fn server_error_backs_off_and_eventually_gives_up() {
    let f = fixture().await;
    let id = isbn(5);
    f.add(&id, Scenario::ServerError).await;

    let mut now = 2_000;
    assert_eq!(f.step(now).await, Step::Processed);
    assert_eq!(f.status(&id).await, STATUS_ERROR);
    assert_eq!(f.field(&id, "attempts").await, 1);
    let first_retry = f.field(&id, "next_try_at").await;
    assert!(first_retry > now);

    // 아직 때가 아니면 건드리지 않는다
    assert_eq!(f.step(now + 1).await, Step::Idle);

    // 간격은 점점 늘어난다
    now = first_retry;
    assert_eq!(f.step(now).await, Step::Processed);
    let second_retry = f.field(&id, "next_try_at").await;
    assert!(second_retry - now > first_retry - 2_000);

    // 반복 실패하면 missing으로 두고 한참 뒤에 본다
    for _ in 0..(MAX_ATTEMPTS - 2) {
        now = f.field(&id, "next_try_at").await;
        assert_eq!(f.step(now).await, Step::Processed);
    }
    assert_eq!(f.status(&id).await, STATUS_MISSING);
    assert_eq!(f.field(&id, "next_try_at").await, now + EXHAUSTED_RETRY_SECS);
}

#[tokio::test]
async fn non_image_response_is_treated_as_missing() {
    let f = fixture().await;
    let id = isbn(6);
    f.add(&id, Scenario::NotAnImage).await;
    assert_eq!(f.step(2_000).await, Step::Processed);
    assert_eq!(f.status(&id).await, STATUS_MISSING);
    assert!(!cover_path(&f.dir, &id).unwrap().exists());
}

#[tokio::test]
async fn cover_url_on_a_foreign_host_is_never_fetched() {
    let f = fixture().await;
    let id = isbn(9);
    f.add(&id, Scenario::ForeignImage).await;
    assert_eq!(f.step(2_000).await, Step::Processed);
    assert_eq!(f.status(&id).await, STATUS_MISSING);
}

#[tokio::test]
async fn queue_is_processed_oldest_first_one_at_a_time() {
    let f = fixture().await;
    let (a, b) = (isbn(10), isbn(11));
    f.scenarios.lock().unwrap().insert(a.clone(), Scenario::Ok);
    f.scenarios.lock().unwrap().insert(b.clone(), Scenario::Ok);
    enqueue(&f.pool, std::slice::from_ref(&a), 100).await.unwrap();
    enqueue(&f.pool, std::slice::from_ref(&b), 200).await.unwrap();

    assert_eq!(f.step(2_000).await, Step::Processed);
    assert_eq!(f.status(&a).await, STATUS_OK);
    assert_eq!(f.status(&b).await, STATUS_PENDING);
}

// ── DB 도우미 ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn enqueue_does_not_touch_existing_rows() {
    let f = fixture().await;
    let id = isbn(12);
    f.add(&id, Scenario::Ok).await;
    f.step(2_000).await;
    assert_eq!(enqueue(&f.pool, std::slice::from_ref(&id), 9_999).await.unwrap(), 0);
    assert_eq!(f.status(&id).await, STATUS_OK);
}

#[tokio::test]
async fn stale_missing_rows_return_to_the_queue_but_fresh_ones_do_not() {
    let f = fixture().await;
    let (stale, fresh) = (isbn(13), isbn(14));
    f.add(&stale, Scenario::NoCover).await;
    f.add(&fresh, Scenario::NoCover).await;
    f.step(2_000).await;
    f.step(2_000).await;
    let retry_at = 2_000 + MISSING_RETRY_SECS;
    assert_eq!(f.status(&stale).await, STATUS_MISSING);
    assert_eq!(f.status(&fresh).await, STATUS_MISSING);

    // 다시 볼 때가 되기 전에는 건드리지 않는다
    requeue_stale_missing(&f.pool, &[stale.clone(), fresh.clone()], retry_at - 1).await.unwrap();
    assert_eq!(f.status(&stale).await, STATUS_MISSING);

    // fresh는 더 최근에 확인한 것으로 만들고, 때가 된 시각에 부르면 stale만 돌아온다
    query("UPDATE book_covers SET next_try_at = ? WHERE isbn13 = ?")
        .bind(retry_at + 10_000)
        .bind(&fresh)
        .execute(&f.pool)
        .await
        .unwrap();
    requeue_stale_missing(&f.pool, &[stale.clone(), fresh.clone()], retry_at).await.unwrap();
    assert_eq!(f.status(&stale).await, STATUS_PENDING);
    assert_eq!(f.status(&fresh).await, STATUS_MISSING);
}
