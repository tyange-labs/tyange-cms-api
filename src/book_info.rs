//! gotolib 책 정보: ISBN으로 알라딘 상품 페이지(`/shop/wproduct.aspx`)를 한 번 받아 표지·주제 분류
//! 경로·형식어(장편소설/소설집 등)·쪽수를 함께 뽑아 `book_covers`에 저장한다. 표지는 이 수집이 내놓는
//! 결과물 중 하나일 뿐이다(그래서 "cover"가 아니라 "book_info"가 이 모듈의 이름이고, 썸네일 생성·저장
//! 같은 표지 전용 로직은 `cover` 하위 모듈로 들어가 있다) — "한국문학" 탭의 장편/소설집 판정이 쓰는
//! 분류·형식어·쪽수도 같은 한 번의 요청에서 나온다. 표지와 책 정보를 따로 긁으면 같은 ISBN에 알라딘
//! 요청을 두 번 보내게 되므로 반드시 한 번에 뽑는다.
//!
//! 설계 약속(어기면 안 된다):
//! - 앱의 조회 요청 경로에서는 알라딘에 가지 않는다. 요청은 DB만 보고, 알라딘 호출은 아래 수집 작업 한 개만 한다.
//! - 알라딘에는 동시에 한 건, 최소 간격(`MIN_INTERVAL`) 이상으로만 요청한다.
//! - 403/429가 오면 작업 전체가 한참 쉰다(행 하나의 실패로 보지 않는다).
//! - 원본 이미지는 저장하지 않고 줄인 썸네일만 저장한다. 페이지 HTML 자체도 저장하지 않는다(뽑은 필드만 남긴다).
//! - 실패도 기록한다(`missing`/`error` + 다음 시도 시각). 못 찾은 ISBN을 매번 다시 긁지 않는다.
//!
//! DB 테이블 이름(`book_covers`)과 환경 변수 이름(`COVERS_DIR`·`COVER_FETCH_ENABLED` 등)은 표지만
//! 다루던 시절 이름 그대로 둔다 — 운영 배포 설정(Forgejo vars/secrets)을 같이 바꿔야 해서 코드 구조만
//! 정리하고 이름은 건드리지 않았다.

use std::{collections::HashMap, env, path::Path, time::Duration};

use reqwest::{redirect, StatusCode, Url};
use sqlx::{query, query_scalar, Row, Sqlite, SqlitePool};

mod cover;
pub use cover::{cover_path, covers_dir, make_thumbnail, store_thumbnail, THUMB_MAX_HEIGHT, THUMB_QUALITY, THUMB_WIDTH};

// ── 상태 값(문자열은 한 곳에서만 쓴다) ─────────────────────────────────────────
pub const STATUS_PENDING: &str = "pending";
pub const STATUS_OK: &str = "ok";
pub const STATUS_MISSING: &str = "missing";
pub const STATUS_ERROR: &str = "error";

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

// ── 상품 페이지에서 표지 주소·책 정보 뽑기 ────────────────────────────────────────
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

/// `<meta {attr}="{value}" content="…">`의 content. 속성 순서·따옴표 종류는 가리지 않는다.
fn extract_meta(html: &str, attr: &str, value: &str) -> Option<String> {
    let mut rest = html;
    while let Some(start) = rest.find("<meta") {
        let tag_and_after = &rest[start..];
        let end = tag_and_after.find('>')?;
        let tag = &tag_and_after[..end];
        if attribute(tag, attr) == Some(value) {
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

/// `<meta property="og:image" content="…">`의 content.
/// 주소를 믿어도 되는지는 여기서 판단하지 않는다(`SourceConfig::allows`).
pub fn extract_og_image(html: &str) -> Option<String> {
    extract_meta(html, "property", "og:image")
}

/// 태그 안의 글자만 남긴다(`<b>…</b>` 같은 강조는 지우고 글자는 남긴다).
fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// 알라딘 상품 페이지의 주제 분류(`<ul id="ulCategory">`) 경로.
/// "국내도서 > 소설/시/희곡 > 한국소설 > 2000년대 이후 한국소설"처럼 " > "로 이어 붙인다.
/// `href`가 있는 링크만 센다 — 같은 `<li>` 안의 "접기" 토글(`href` 없음)을 섞지 않기 위함.
pub fn extract_category_path(html: &str) -> Option<String> {
    let block_start = html.find("id=\"ulCategory\"")?;
    let li_start = html[block_start..].find("<li")? + block_start;
    let li_end = html[li_start..].find("</li>")? + li_start;
    let li = &html[li_start..li_end];

    let mut parts = Vec::new();
    let mut rest = li;
    while let Some(a_start) = rest.find("<a") {
        let after_a = &rest[a_start..];
        let Some(tag_end) = after_a.find('>') else { break };
        let open_tag = &after_a[..tag_end];
        let text_start = tag_end + 1;
        let Some(close_rel) = after_a[text_start..].find("</a>") else { break };
        let inner = &after_a[text_start..text_start + close_rel];
        if open_tag.contains("href") {
            let text = strip_tags(inner).trim().to_string();
            if !text.is_empty() {
                parts.push(text);
            }
        }
        rest = &after_a[text_start + close_rel + 4..];
    }
    (!parts.is_empty()).then(|| parts.join(" > "))
}

/// 책 소개 문구(`<meta name="description">`)에서 이 책 자체의 형식어를 뽑는다. "장편소설"·"소설집" 같은
/// 낱말은 작가의 다른 책 이력을 말할 때도 나오므로(예: "그의 출간작 절반 이상이 장편소설이기에"), 아무 데나
/// 있는 낱말을 줍지 않고 **이 책 제목(『…』) 바로 앞에 붙은 것만** 믿는다
/// ("…신작 장편소설 『재이』를", "…6년 만의 소설집 『태풍의 이름을…』을" 처럼 알라딘 편집 문구가 일관되게 그 자리에 형식어를 둔다).
pub fn extract_form_hint(html: &str) -> Option<&'static str> {
    const FORMS: [&str; 4] = ["경장편소설", "장편소설", "연작소설", "소설집"];
    let description = extract_meta(html, "name", "description").or_else(|| extract_meta(html, "property", "og:description"))?;
    let title_at = description.find('『')?;
    let before = description[..title_at].trim_end();
    FORMS.into_iter().find(|form| before.ends_with(form))
}

/// 기본정보(`.conts_info_list1`)의 "232쪽" 같은 항목에서 쪽수를 뽑는다. 전자책처럼 이 목록 자체가
/// 없거나 쪽수 항목이 없으면 `None`(전자책은 종이책과 달리 쪽수를 안 보여준다).
pub fn extract_pages(html: &str) -> Option<i64> {
    let start = html.find("conts_info_list1")?;
    let ul_start = html[start..].find("<ul")? + start;
    let ul_end = html[ul_start..].find("</ul>")? + ul_start;
    let block = &html[ul_start..ul_end];
    for part in block.split("<li>") {
        let text = strip_tags(part);
        let text = text.trim();
        if let Some(digits) = text.strip_suffix('쪽') {
            if let Ok(n) = digits.trim().parse::<i64>() {
                return Some(n);
            }
        }
    }
    None
}

/// 상품 페이지 한 번에서 함께 뽑히는 책 정보(표지 주소 제외 — 표지는 `PageOutcome`에 따로 담는다).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BookPageInfo {
    pub category_path: Option<String>,
    pub form_hint: Option<String>,
    pub pages: Option<i64>,
}

impl BookPageInfo {
    pub fn from_html(html: &str) -> Self {
        Self {
            category_path: extract_category_path(html),
            form_hint: extract_form_hint(html).map(str::to_string),
            pages: extract_pages(html),
        }
    }
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
    /// 표지 주소를 찾았다 (+ 같은 페이지에서 함께 뽑은 책 정보)
    Found(String, BookPageInfo),
    /// 페이지는 정상인데 표지가 없다(또는 믿을 수 없는 주소다). 책 정보는 뽑았을 수 있다.
    NoCover(BookPageInfo),
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

    /// ISBN으로 상품 페이지를 가져와 표지 주소와 책 정보(분류·형식어·쪽수)를 찾는다.
    /// `/shop/wproduct.aspx`만 부른다 — 이 한 번의 요청으로 표지 수집과 "한국문학" 탭의 장편/소설집
    /// 판정에 필요한 정보를 모두 얻는다(알라딘에 추가 요청을 보내지 않는다).
    pub async fn find_cover_url(&self, isbn: &str) -> PageOutcome {
        if !is_valid_isbn13(isbn) {
            return PageOutcome::NoCover(BookPageInfo::default());
        }
        let url = format!("{}/shop/wproduct.aspx?ISBN={isbn}", self.config.page_base);
        let response = match self.http.get(&url).send().await {
            Ok(response) => response,
            Err(_) => return PageOutcome::Transient,
        };
        match response.status() {
            StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS => return PageOutcome::Blocked,
            StatusCode::NOT_FOUND | StatusCode::GONE => return PageOutcome::NoCover(BookPageInfo::default()),
            status if !status.is_success() => return PageOutcome::Transient,
            _ => {}
        }
        let Some(body) = read_capped(response, MAX_PAGE_BYTES).await else {
            return PageOutcome::Transient;
        };
        let html = String::from_utf8_lossy(&body);
        let info = BookPageInfo::from_html(&html);
        match extract_og_image(&html) {
            Some(found) if self.config.allows(&found) => PageOutcome::Found(found, info),
            _ => PageOutcome::NoCover(info),
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

/// `GET /gotolib/book-info`가 보는 한 책의 상태. `category_path`·`form_hint`·`pages`는 `status`가
/// `ok`/`missing`이어도 페이지에서 못 뽑았으면 `None`일 수 있다(예: 전자책은 `pages`가 거의 항상 없다).
pub struct BookInfoRow {
    pub status: String,
    pub category_path: Option<String>,
    pub form_hint: Option<String>,
    pub pages: Option<i64>,
}

/// 주어진 ISBN들의 책 정보(DB에 없는 ISBN은 빠진다). 표지 수집과 같은 행·같은 큐를 본다.
pub async fn book_infos(pool: &SqlitePool, isbns: &[String]) -> Result<HashMap<String, BookInfoRow>, sqlx::Error> {
    if isbns.is_empty() {
        return Ok(HashMap::new());
    }
    let sql = format!(
        "SELECT isbn13, status, category_path, form_hint, pages FROM book_covers WHERE isbn13 IN ({})",
        placeholders(isbns.len())
    );
    let mut q = query(&sql);
    for isbn in isbns {
        q = q.bind(isbn);
    }
    let rows = q.fetch_all(pool).await?;
    let mut map = HashMap::new();
    for row in rows {
        map.insert(
            row.try_get("isbn13")?,
            BookInfoRow {
                status: row.try_get("status")?,
                category_path: row.try_get("category_path")?,
                form_hint: row.try_get("form_hint")?,
                pages: row.try_get("pages")?,
            },
        );
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

async fn mark_ok(
    pool: &SqlitePool,
    isbn: &str,
    source_url: &str,
    info: &BookPageInfo,
    now: i64,
) -> Result<(), sqlx::Error> {
    query(
        "UPDATE book_covers SET status = ?, source_url = ?, category_path = ?, form_hint = ?, pages = ?,
         fetched_at = ?, next_try_at = 0 WHERE isbn13 = ?",
    )
    .bind(STATUS_OK)
    .bind(source_url)
    .bind(&info.category_path)
    .bind(&info.form_hint)
    .bind(info.pages)
    .bind(now)
    .bind(isbn)
    .execute(pool)
    .await?;
    Ok(())
}

async fn mark_missing(
    pool: &SqlitePool,
    isbn: &str,
    info: &BookPageInfo,
    retry_after_secs: i64,
    now: i64,
) -> Result<(), sqlx::Error> {
    query(
        "UPDATE book_covers SET status = ?, category_path = ?, form_hint = ?, pages = ?,
         next_try_at = ?, fetched_at = ? WHERE isbn13 = ?",
    )
    .bind(STATUS_MISSING)
    .bind(&info.category_path)
    .bind(&info.form_hint)
    .bind(info.pages)
    .bind(now + retry_after_secs)
    .bind(now)
    .bind(isbn)
    .execute(pool)
    .await?;
    Ok(())
}

/// 일시 오류: 시도 횟수를 올리고 점점 늘어나는 간격 뒤로 미룬다. 여러 번 겹치면 `missing`으로 본다.
/// 일시 오류는 페이지를 받지 못한 경우라 책 정보가 없다(`BookPageInfo::default()`).
async fn mark_error(pool: &SqlitePool, isbn: &str, attempts: i64, now: i64) -> Result<(), sqlx::Error> {
    let attempts = attempts + 1;
    if attempts >= MAX_ATTEMPTS {
        return mark_missing(pool, isbn, &BookPageInfo::default(), EXHAUSTED_RETRY_SECS, now).await;
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
            eprintln!("[book_info] 큐를 읽지 못했습니다: {e}");
            return Step::Idle;
        }
    };
    let (Ok(isbn), Ok(attempts)) = (row.try_get::<String, _>("isbn13"), row.try_get::<i64, _>("attempts"))
    else {
        return Step::Idle;
    };

    let recorded = match source.find_cover_url(&isbn).await {
        PageOutcome::Blocked => return Step::Blocked,
        PageOutcome::NoCover(info) => mark_missing(pool, &isbn, &info, MISSING_RETRY_SECS, now).await,
        PageOutcome::Transient => mark_error(pool, &isbn, attempts, now).await,
        PageOutcome::Found(url, info) => match source.download(&url).await {
            Err(DownloadError::Blocked) => return Step::Blocked,
            Err(DownloadError::Invalid) => mark_missing(pool, &isbn, &info, MISSING_RETRY_SECS, now).await,
            Err(DownloadError::Transient) => mark_error(pool, &isbn, attempts, now).await,
            Ok(bytes) => {
                let dir = dir.to_path_buf();
                let target = isbn.clone();
                let stored = tokio::task::spawn_blocking(move || store_thumbnail(&dir, &target, &bytes)).await;
                match stored {
                    Ok(Ok(())) => mark_ok(pool, &isbn, &url, &info, now).await,
                    // 받은 이미지를 줄이거나 쓰지 못함: 일시 오류와 같이 간격을 두고 다시 보고, 반복되면 missing이 된다.
                    Ok(Err(reason)) => {
                        eprintln!("[book_info] {isbn} 저장 실패: {reason}");
                        mark_error(pool, &isbn, attempts, now).await
                    }
                    Err(_) => mark_error(pool, &isbn, attempts, now).await,
                }
            }
        },
    };
    if let Err(e) = recorded {
        eprintln!("[book_info] {isbn} 결과를 기록하지 못했습니다: {e}");
    }
    Step::Processed
}

#[derive(Clone, Debug)]
pub struct WorkerConfig {
    pub dir: std::path::PathBuf,
    /// 알라딘 요청 사이 간격(`MIN_INTERVAL` 미만이면 `MIN_INTERVAL`로 올린다)
    pub interval: Duration,
    pub idle: Duration,
    pub block_pause: Duration,
}

impl WorkerConfig {
    pub fn new(dir: std::path::PathBuf, interval: Duration) -> Self {
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
    println!("[book_info] 수집 작업 시작 (간격 {}초)", config.interval.as_secs());
    loop {
        let wait = match process_next(&pool, &source, &config.dir, now()).await {
            Step::Idle => config.idle,
            Step::Processed => config.interval,
            Step::Blocked => {
                eprintln!(
                    "[book_info] 알라딘이 요청을 막았습니다(403/429). {}시간 쉽니다.",
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
        println!("[book_info] 수집 작업이 꺼져 있습니다(COVER_FETCH_ENABLED).");
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
            eprintln!("[book_info] HTTP 클라이언트를 만들지 못했습니다: {e}");
            return;
        }
    };
    let config = WorkerConfig::new(covers_dir(), interval);
    // 작업이 패닉으로 죽으면 조용히 사라지므로, 감독 루프가 잠시 뒤 다시 띄운다.
    tokio::spawn(async move {
        loop {
            let handle = tokio::spawn(run_worker(pool.clone(), source.clone(), config.clone()));
            if let Err(e) = handle.await {
                eprintln!("[book_info] 수집 작업이 비정상 종료했습니다: {e}. 1분 뒤 다시 시작합니다.");
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
}

#[cfg(test)]
pub(crate) mod tests;
