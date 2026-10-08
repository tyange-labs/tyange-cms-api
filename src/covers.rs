//! gotolib 책 표지: ISBN으로 알라딘 상품 페이지에서 표지를 찾아 작은 썸네일로 줄여 디스크에 저장한다.
//!
//! 설계 약속(어기면 안 된다):
//! - 앱의 조회 요청 경로에서는 알라딘에 가지 않는다. 요청은 DB만 보고, 알라딘 호출은 아래 수집 작업 한 개만 한다.
//! - 알라딘에는 동시에 한 건, 최소 간격(`MIN_INTERVAL`) 이상으로만 요청한다.
//! - 403/429가 오면 작업 전체가 한참 쉰다(행 하나의 실패로 보지 않는다).
//! - 원본은 저장하지 않고 줄인 썸네일만 저장한다.
//! - 실패도 기록한다(`missing`/`error` + 다음 시도 시각). 못 찾은 ISBN을 매번 다시 긁지 않는다.

use std::{
    collections::HashMap,
    env, fs,
    io::Cursor,
    path::{Path, PathBuf},
    time::Duration,
};

use image::{codecs::jpeg::JpegEncoder, imageops::FilterType, ImageReader, Limits};
use reqwest::{redirect, StatusCode, Url};
use sqlx::{query, query_scalar, Row, Sqlite, SqlitePool};

// ── 상태 값(문자열은 한 곳에서만 쓴다) ─────────────────────────────────────────
pub const STATUS_PENDING: &str = "pending";
pub const STATUS_OK: &str = "ok";
pub const STATUS_MISSING: &str = "missing";
pub const STATUS_ERROR: &str = "error";

// ── 썸네일 ───────────────────────────────────────────────────────────────────
/// 앱 카드에 보이는 크기의 2배쯤(고해상도 화면). 원본이 더 작으면 키우지 않는다.
pub const THUMB_WIDTH: u32 = 200;
pub const THUMB_MAX_HEIGHT: u32 = 320;
pub const THUMB_QUALITY: u8 = 80;
/// 디코딩 전에 거르는 한도: 비정상적으로 큰 그림(압축 폭탄)을 메모리에 풀지 않는다.
const MAX_DECODE_DIMENSION: u32 = 6000;
const MAX_DECODE_ALLOC_BYTES: u64 = 256 * 1024 * 1024;

// ── 수집 ─────────────────────────────────────────────────────────────────────
/// 알라딘 요청 사이의 최소 간격. 환경 변수로 늘릴 수는 있어도 줄일 수는 없다.
pub const MIN_INTERVAL: Duration = Duration::from_secs(3);
const DEFAULT_IDLE: Duration = Duration::from_secs(15);
/// 403/429를 받으면 작업 전체가 이만큼 쉰다.
const BLOCK_PAUSE: Duration = Duration::from_secs(6 * 60 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_IMAGE_BYTES: usize = 3 * 1024 * 1024;
/// 표지가 없다고 나온 ISBN을 다시 확인하기까지의 시간.
const MISSING_RETRY_SECS: i64 = 30 * 24 * 60 * 60;
/// 일시 오류를 이만큼 겪으면 `missing`으로 보고 한참 뒤에 다시 본다.
const MAX_ATTEMPTS: i64 = 5;
const EXHAUSTED_RETRY_SECS: i64 = 7 * 24 * 60 * 60;
const RETRY_BASE_SECS: i64 = 10 * 60;
const RETRY_MAX_SECS: i64 = 12 * 60 * 60;

pub const DEFAULT_QUEUE_MAX: i64 = 500;

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

// ── ISBN ─────────────────────────────────────────────────────────────────────
/// 정상적인 ISBN-13인가: 숫자 13자리, 접두 978/979, 검사 숫자 일치.
/// 파일 이름·SQL·알라딘 주소에 ISBN이 들어가므로 서버로 들어오는 모든 ISBN은 이 함수를 먼저 지난다.
pub fn is_valid_isbn13(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 13 || !bytes.iter().all(u8::is_ascii_digit) {
        return false;
    }
    if !(value.starts_with("978") || value.starts_with("979")) {
        return false;
    }
    let sum: u32 = bytes[..12]
        .iter()
        .enumerate()
        .map(|(i, digit)| u32::from(digit - b'0') * if i % 2 == 0 { 1 } else { 3 })
        .sum();
    let check = (10 - sum % 10) % 10;
    check == u32::from(bytes[12] - b'0')
}

// ── 상품 페이지에서 표지 주소 뽑기 ───────────────────────────────────────────────
/// `<meta property="og:image" content="…">`의 content. 속성 순서·따옴표 종류는 가리지 않는다.
/// 주소를 믿어도 되는지는 여기서 판단하지 않는다(`SourceConfig::allows`).
pub fn extract_og_image(html: &str) -> Option<String> {
    let mut rest = html;
    while let Some(start) = rest.find("<meta") {
        let tag_and_after = &rest[start..];
        let end = tag_and_after.find('>')?;
        let tag = &tag_and_after[..end];
        if attribute(tag, "property") == Some("og:image") {
            if let Some(content) = attribute(tag, "content") {
                let content = content.trim();
                if !content.is_empty() {
                    return Some(content.to_string());
                }
            }
        }
        rest = &tag_and_after[end + 1..];
    }
    None
}

/// 태그 문자열에서 `name="값"` 또는 `name='값'`의 값을 꺼낸다(속성 이름 앞은 공백이어야 한다).
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=");
    let mut from = 0;
    while let Some(found) = tag[from..].find(&needle) {
        let at = from + found;
        let preceded_by_space = tag[..at].chars().next_back().is_some_and(char::is_whitespace);
        let value_start = at + needle.len();
        if preceded_by_space {
            let quote = tag[value_start..].chars().next()?;
            if quote == '"' || quote == '\'' {
                let body = &tag[value_start + 1..];
                let close = body.find(quote)?;
                return Some(&body[..close]);
            }
        }
        from = value_start;
    }
    None
}

// ── 썸네일 만들기 ────────────────────────────────────────────────────────────
/// 이미지 바이트 → 줄인 JPEG 바이트. 순수 변환이라 네트워크·파일을 쓰지 않는다.
/// 이미지가 아니거나 깨졌으면 `Err`(패닉하지 않는다).
pub fn make_thumbnail(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| format!("형식을 알아내지 못했습니다: {e}"))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODE_DIMENSION);
    limits.max_image_height = Some(MAX_DECODE_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOC_BYTES);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|e| format!("이미지를 읽지 못했습니다: {e}"))?;

    // 원본이 상자보다 작으면 키우지 않는다.
    let fitted = if decoded.width() > THUMB_WIDTH || decoded.height() > THUMB_MAX_HEIGHT {
        decoded.resize(THUMB_WIDTH, THUMB_MAX_HEIGHT, FilterType::Lanczos3)
    } else {
        decoded
    };

    let rgb = fitted.to_rgb8();
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, THUMB_QUALITY)
        .encode_image(&rgb)
        .map_err(|e| format!("JPEG로 저장하지 못했습니다: {e}"))?;
    Ok(out)
}

// ── 파일 저장 ────────────────────────────────────────────────────────────────
/// 썸네일 폴더. 배포가 지우지 않는 곳(DB와 같은 `data` 아래)을 기본으로 쓴다.
pub fn covers_dir() -> PathBuf {
    env::var("COVERS_DIR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./data/covers"))
}

/// `<폴더>/<isbn>.jpg`. 이름은 검증을 통과한 ISBN과 고정 확장자로만 만든다(외부 문자열을 섞지 않는다).
pub fn cover_path(dir: &Path, isbn: &str) -> Option<PathBuf> {
    is_valid_isbn13(isbn).then(|| dir.join(format!("{isbn}.jpg")))
}

/// 원본 바이트를 줄여 임시 파일로 쓴 뒤 이름을 바꾼다. 쓰다가 죽어도 반쪽 파일이 최종 이름으로 남지 않는다.
/// 블로킹 작업이라 `spawn_blocking` 안에서 부른다.
pub fn store_thumbnail(dir: &Path, isbn: &str, source: &[u8]) -> Result<(), String> {
    let path = cover_path(dir, isbn).ok_or("ISBN이 올바르지 않습니다.")?;
    let thumbnail = make_thumbnail(source)?;
    fs::create_dir_all(dir).map_err(|e| format!("폴더를 만들지 못했습니다: {e}"))?;
    let temp = path.with_extension("jpg.tmp");
    fs::write(&temp, &thumbnail).map_err(|e| format!("파일을 쓰지 못했습니다: {e}"))?;
    fs::rename(&temp, &path).map_err(|e| {
        let _ = fs::remove_file(&temp);
        format!("파일 이름을 바꾸지 못했습니다: {e}")
    })
}

// ── 알라딘 ───────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct SourceConfig {
    /// 상품 페이지 주소의 앞부분(경로 제외). 예: `https://www.aladin.co.kr`
    pub page_base: String,
    /// 표지 이미지를 받아도 되는 호스트
    pub image_hosts: Vec<String>,
    /// 테스트에서만 true: 로컬 가짜 서버(http)를 허용한다
    pub allow_http: bool,
}

impl SourceConfig {
    pub fn aladin() -> Self {
        Self {
            page_base: "https://www.aladin.co.kr".to_string(),
            image_hosts: vec!["image.aladin.co.kr".to_string()],
            allow_http: false,
        }
    }

    /// og:image 값을 믿고 내려받아도 되는가: https, 허용 호스트, 표지 경로(`/product/…`).
    /// 외부 페이지가 알려 준 주소를 그대로 믿으면 서버가 임의의 주소로 요청을 보내게 된다.
    pub fn allows(&self, url: &str) -> bool {
        let Ok(parsed) = Url::parse(url) else {
            return false;
        };
        let scheme_ok = parsed.scheme() == "https" || (self.allow_http && parsed.scheme() == "http");
        let host_ok = parsed
            .host_str()
            .is_some_and(|host| self.image_hosts.iter().any(|allowed| allowed == host));
        scheme_ok && host_ok && parsed.path().starts_with("/product/")
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PageOutcome {
    /// 표지 주소를 찾았다
    Found(String),
    /// 페이지는 정상인데 표지가 없다(또는 믿을 수 없는 주소다)
    NoCover,
    /// 막혔다는 신호(403, 429): 작업 전체가 쉬어야 한다
    Blocked,
    /// 잠시 뒤 다시 하면 풀릴 수 있다(5xx, 타임아웃, 연결 실패)
    Transient,
}

#[derive(Debug, PartialEq, Eq)]
pub enum DownloadError {
    Blocked,
    Transient,
    /// 이미지가 아니거나 너무 크다 → 이 책은 표지가 없는 것으로 본다
    Invalid,
}

#[derive(Clone)]
pub struct CoverSource {
    http: reqwest::Client,
    config: SourceConfig,
}

impl CoverSource {
    pub fn new(config: SourceConfig, user_agent: &str) -> Result<Self, reqwest::Error> {
        // 리다이렉트는 우리가 아는 호스트(페이지 + 이미지) 안에서만 따라간다.
        let mut allowed_hosts: Vec<String> = config.image_hosts.clone();
        if let Some(host) = Url::parse(&config.page_base)
            .ok()
            .and_then(|url| url.host_str().map(str::to_string))
        {
            allowed_hosts.push(host);
        }
        let policy = redirect::Policy::custom(move |attempt| {
            let allowed = attempt
                .url()
                .host_str()
                .is_some_and(|host| allowed_hosts.iter().any(|h| h == host));
            if attempt.previous().len() >= 5 || !allowed {
                attempt.stop()
            } else {
                attempt.follow()
            }
        });
        let http = reqwest::Client::builder()
            .user_agent(user_agent)
            .timeout(REQUEST_TIMEOUT)
            .redirect(policy)
            .build()?;
        Ok(Self { http, config })
    }

    /// ISBN으로 상품 페이지를 가져와 표지 주소를 찾는다. `/shop/wproduct.aspx`만 부른다.
    pub async fn find_cover_url(&self, isbn: &str) -> PageOutcome {
        if !is_valid_isbn13(isbn) {
            return PageOutcome::NoCover;
        }
        let url = format!("{}/shop/wproduct.aspx?ISBN={isbn}", self.config.page_base);
        let response = match self.http.get(&url).send().await {
            Ok(response) => response,
            Err(_) => return PageOutcome::Transient,
        };
        match response.status() {
            StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS => return PageOutcome::Blocked,
            StatusCode::NOT_FOUND | StatusCode::GONE => return PageOutcome::NoCover,
            status if !status.is_success() => return PageOutcome::Transient,
            _ => {}
        }
        let Some(body) = read_capped(response, MAX_PAGE_BYTES).await else {
            return PageOutcome::Transient;
        };
        let html = String::from_utf8_lossy(&body);
        match extract_og_image(&html) {
            Some(found) if self.config.allows(&found) => PageOutcome::Found(found),
            _ => PageOutcome::NoCover,
        }
    }

    /// 허용된 표지 주소의 이미지를 내려받는다(크기 상한, 이미지 형식 확인).
    pub async fn download(&self, url: &str) -> Result<Vec<u8>, DownloadError> {
        if !self.config.allows(url) {
            return Err(DownloadError::Invalid);
        }
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|_| DownloadError::Transient)?;
        match response.status() {
            StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS => {
                return Err(DownloadError::Blocked)
            }
            StatusCode::NOT_FOUND | StatusCode::GONE => return Err(DownloadError::Invalid),
            status if !status.is_success() => return Err(DownloadError::Transient),
            _ => {}
        }
        let is_image = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("image/"));
        if !is_image {
            return Err(DownloadError::Invalid);
        }
        match read_capped(response, MAX_IMAGE_BYTES).await {
            Some(bytes) => Ok(bytes),
            None => Err(DownloadError::Invalid),
        }
    }
}

/// 본문을 `limit` 바이트까지만 읽는다. 넘거나 읽다 실패하면 `None`.
async fn read_capped(mut response: reqwest::Response, limit: usize) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    while let Ok(Some(chunk)) = response.chunk().await {
        if body.len() + chunk.len() > limit {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    Some(body)
}

// ── DB ───────────────────────────────────────────────────────────────────────
fn placeholders(count: usize) -> String {
    vec!["?"; count].join(",")
}

/// 표지가 없다고 기록된 지 오래된 ISBN을 다시 대기 상태로 돌린다(알라딘이 나중에 표지를 올릴 수 있다).
pub async fn requeue_stale_missing(
    pool: &SqlitePool,
    isbns: &[String],
    now: i64,
) -> Result<(), sqlx::Error> {
    if isbns.is_empty() {
        return Ok(());
    }
    let sql = format!(
        "UPDATE book_covers SET status = ?, attempts = 0, next_try_at = 0
         WHERE status = ? AND next_try_at <= ? AND isbn13 IN ({})",
        placeholders(isbns.len())
    );
    let mut q = query(&sql)
        .bind(STATUS_PENDING)
        .bind(STATUS_MISSING)
        .bind(now);
    for isbn in isbns {
        q = q.bind(isbn);
    }
    q.execute(pool).await?;
    Ok(())
}

/// 주어진 ISBN들의 현재 상태(DB에 없는 ISBN은 빠진다).
pub async fn statuses(
    pool: &SqlitePool,
    isbns: &[String],
) -> Result<HashMap<String, String>, sqlx::Error> {
    if isbns.is_empty() {
        return Ok(HashMap::new());
    }
    let sql = format!(
        "SELECT isbn13, status FROM book_covers WHERE isbn13 IN ({})",
        placeholders(isbns.len())
    );
    let mut q = query(&sql);
    for isbn in isbns {
        q = q.bind(isbn);
    }
    let rows = q.fetch_all(pool).await?;
    let mut map = HashMap::new();
    for row in rows {
        map.insert(row.try_get("isbn13")?, row.try_get("status")?);
    }
    Ok(map)
}

/// 수집을 기다리는 건수(대기 + 일시 오류). 큐 상한 검사에 쓴다.
pub async fn queue_len(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    query_scalar::<Sqlite, i64>("SELECT COUNT(*) FROM book_covers WHERE status IN (?, ?)")
        .bind(STATUS_PENDING)
        .bind(STATUS_ERROR)
        .fetch_one(pool)
        .await
}

/// 새 ISBN을 대기 상태로 넣는다. 이미 있으면 건드리지 않는다. 실제로 넣은 개수를 돌려준다.
pub async fn enqueue(pool: &SqlitePool, isbns: &[String], now: i64) -> Result<u64, sqlx::Error> {
    let mut inserted = 0;
    for isbn in isbns {
        let result = query(
            "INSERT OR IGNORE INTO book_covers (isbn13, status, attempts, next_try_at, created_at)
             VALUES (?, ?, 0, 0, ?)",
        )
        .bind(isbn)
        .bind(STATUS_PENDING)
        .bind(now)
        .execute(pool)
        .await?;
        inserted += result.rows_affected();
    }
    Ok(inserted)
}

async fn mark_ok(pool: &SqlitePool, isbn: &str, source_url: &str, now: i64) -> Result<(), sqlx::Error> {
    query(
        "UPDATE book_covers SET status = ?, source_url = ?, fetched_at = ?, next_try_at = 0 WHERE isbn13 = ?",
    )
    .bind(STATUS_OK)
    .bind(source_url)
    .bind(now)
    .bind(isbn)
    .execute(pool)
    .await?;
    Ok(())
}

async fn mark_missing(pool: &SqlitePool, isbn: &str, retry_after_secs: i64, now: i64) -> Result<(), sqlx::Error> {
    query("UPDATE book_covers SET status = ?, next_try_at = ?, fetched_at = ? WHERE isbn13 = ?")
        .bind(STATUS_MISSING)
        .bind(now + retry_after_secs)
        .bind(now)
        .bind(isbn)
        .execute(pool)
        .await?;
    Ok(())
}

/// 일시 오류: 시도 횟수를 올리고 점점 늘어나는 간격 뒤로 미룬다. 여러 번 겹치면 `missing`으로 본다.
async fn mark_error(pool: &SqlitePool, isbn: &str, attempts: i64, now: i64) -> Result<(), sqlx::Error> {
    let attempts = attempts + 1;
    if attempts >= MAX_ATTEMPTS {
        return mark_missing(pool, isbn, EXHAUSTED_RETRY_SECS, now).await;
    }
    let delay = (RETRY_BASE_SECS << (attempts - 1)).min(RETRY_MAX_SECS);
    query("UPDATE book_covers SET status = ?, attempts = ?, next_try_at = ? WHERE isbn13 = ?")
        .bind(STATUS_ERROR)
        .bind(attempts)
        .bind(now + delay)
        .bind(isbn)
        .execute(pool)
        .await?;
    Ok(())
}

// ── 수집 작업 ────────────────────────────────────────────────────────────────
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// 지금 처리할 것이 없다
    Idle,
    /// 한 건을 처리했다(결과가 무엇이든)
    Processed,
    /// 알라딘이 막았다 — 작업이 한참 쉬어야 한다
    Blocked,
}

/// 큐에서 지금 해도 되는 한 건을 처리한다. 시간 대기는 하지 않는다(대기는 `run_worker`의 몫).
pub async fn process_next(pool: &SqlitePool, source: &CoverSource, dir: &Path, now: i64) -> Step {
    let next = query(
        "SELECT isbn13, attempts FROM book_covers
         WHERE status IN (?, ?) AND next_try_at <= ?
         ORDER BY created_at LIMIT 1",
    )
    .bind(STATUS_PENDING)
    .bind(STATUS_ERROR)
    .bind(now)
    .fetch_optional(pool)
    .await;
    let row = match next {
        Ok(Some(row)) => row,
        Ok(None) => return Step::Idle,
        Err(e) => {
            eprintln!("[covers] 큐를 읽지 못했습니다: {e}");
            return Step::Idle;
        }
    };
    let (Ok(isbn), Ok(attempts)) = (row.try_get::<String, _>("isbn13"), row.try_get::<i64, _>("attempts"))
    else {
        return Step::Idle;
    };

    let recorded = match source.find_cover_url(&isbn).await {
        PageOutcome::Blocked => return Step::Blocked,
        PageOutcome::NoCover => mark_missing(pool, &isbn, MISSING_RETRY_SECS, now).await,
        PageOutcome::Transient => mark_error(pool, &isbn, attempts, now).await,
        PageOutcome::Found(url) => match source.download(&url).await {
            Err(DownloadError::Blocked) => return Step::Blocked,
            Err(DownloadError::Invalid) => mark_missing(pool, &isbn, MISSING_RETRY_SECS, now).await,
            Err(DownloadError::Transient) => mark_error(pool, &isbn, attempts, now).await,
            Ok(bytes) => {
                let dir = dir.to_path_buf();
                let target = isbn.clone();
                let stored = tokio::task::spawn_blocking(move || store_thumbnail(&dir, &target, &bytes)).await;
                match stored {
                    Ok(Ok(())) => mark_ok(pool, &isbn, &url, now).await,
                    // 받은 이미지를 줄이거나 쓰지 못함: 일시 오류와 같이 간격을 두고 다시 보고, 반복되면 missing이 된다.
                    Ok(Err(reason)) => {
                        eprintln!("[covers] {isbn} 저장 실패: {reason}");
                        mark_error(pool, &isbn, attempts, now).await
                    }
                    Err(_) => mark_error(pool, &isbn, attempts, now).await,
                }
            }
        },
    };
    if let Err(e) = recorded {
        eprintln!("[covers] {isbn} 결과를 기록하지 못했습니다: {e}");
    }
    Step::Processed
}

#[derive(Clone, Debug)]
pub struct WorkerConfig {
    pub dir: PathBuf,
    /// 알라딘 요청 사이 간격(`MIN_INTERVAL` 미만이면 `MIN_INTERVAL`로 올린다)
    pub interval: Duration,
    pub idle: Duration,
    pub block_pause: Duration,
}

impl WorkerConfig {
    pub fn new(dir: PathBuf, interval: Duration) -> Self {
        Self {
            dir,
            interval: interval.max(MIN_INTERVAL),
            idle: DEFAULT_IDLE,
            block_pause: BLOCK_PAUSE,
        }
    }
}

/// 연락처 환경 변수 값에서 실제로 쓸 연락처를 고른다: 비었거나 공백뿐이면 기본값.
pub fn contact_from(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or(DEFAULT_CONTACT)
        .to_string()
}

/// 알라딘에 보내는 `User-Agent`: 서비스 이름과 연락처로 식별 가능하게 한다.
pub fn user_agent(contact: &str) -> String {
    format!("gotolib-covers/0.1 (+{contact})")
}

/// 한 건씩 처리하고 쉬기를 영원히 반복한다.
pub async fn run_worker(pool: SqlitePool, source: CoverSource, config: WorkerConfig) {
    println!("[covers] 수집 작업 시작 (간격 {}초)", config.interval.as_secs());
    loop {
        let wait = match process_next(&pool, &source, &config.dir, now()).await {
            Step::Idle => config.idle,
            Step::Processed => config.interval,
            Step::Blocked => {
                eprintln!(
                    "[covers] 알라딘이 요청을 막았습니다(403/429). {}시간 쉽니다.",
                    config.block_pause.as_secs() / 3600
                );
                config.block_pause
            }
        };
        tokio::time::sleep(wait).await;
    }
}

/// 연락처를 따로 주지 않았을 때 `User-Agent`에 넣는 주소(공개된 서비스 주소).
pub const DEFAULT_CONTACT: &str = "https://gotolib.tyange.com";

/// 환경 변수로 수집 작업을 켠다. 기본은 꺼짐.
/// - `COVER_FETCH_ENABLED=true` 일 때만 시작한다
/// - `COVER_CONTACT`(선택): 알라딘이 연락할 수 있는 주소. 없으면 `DEFAULT_CONTACT`를 쓴다.
///   `User-Agent`는 `gotolib-covers/0.1 (+연락처)`로 코드가 조립한다(.env에 공백·괄호를 넣지 않으려고).
/// - `COVER_FETCH_INTERVAL_SECS`(기본 3, 3 미만은 3으로)
pub fn spawn_worker_from_env(pool: SqlitePool) {
    if env::var("COVER_FETCH_ENABLED").map(|v| v != "true").unwrap_or(true) {
        println!("[covers] 수집 작업이 꺼져 있습니다(COVER_FETCH_ENABLED).");
        return;
    }
    let contact = contact_from(env::var("COVER_CONTACT").ok().as_deref());
    let user_agent = user_agent(&contact);
    let interval = env::var("COVER_FETCH_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(MIN_INTERVAL);
    let source = match CoverSource::new(SourceConfig::aladin(), &user_agent) {
        Ok(source) => source,
        Err(e) => {
            eprintln!("[covers] HTTP 클라이언트를 만들지 못했습니다: {e}");
            return;
        }
    };
    let config = WorkerConfig::new(covers_dir(), interval);
    // 작업이 패닉으로 죽으면 조용히 사라지므로, 감독 루프가 잠시 뒤 다시 띄운다.
    tokio::spawn(async move {
        loop {
            let handle = tokio::spawn(run_worker(pool.clone(), source.clone(), config.clone()));
            if let Err(e) = handle.await {
                eprintln!("[covers] 수집 작업이 비정상 종료했습니다: {e}. 1분 뒤 다시 시작합니다.");
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
}

#[cfg(test)]
pub(crate) mod tests;
