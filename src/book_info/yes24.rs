//! YES24 분류로 장편/단편을 가른다: "한국 장편소설"(`001001046001002`)과 "한국 단편소설"(`001001046001001`)
//! 목록을 신상품순으로 읽어 **17개 대상 출판사 책만** 모으고, 그 책들의 상품 페이지에서 ISBN을 받아 둔다.
//! gotolib "한국문학" 탭은 ISBN으로 이 분류를 찾아 장편/소설집 판정에 쓴다(`GET /gotolib/book-info`의 `yes24Form`).
//!
//! 설계 약속(알라딘 수집과 같다):
//! - 앱의 조회 요청 경로에서는 YES24에 가지 않는다. 요청은 수집 작업 한 개만, 동시 한 건, 최소 간격 이상으로 한다.
//! - 403/429가 오면 작업 전체가 한참 쉰다.
//! - robots.txt가 막지 않은 경로만 부른다: 목록 조각(`/product/category/CategoryProductContents`)과
//!   상품 페이지(`/product/goods/번호`). 검색·`JsonGoodsList`·중고(`display/018`)는 부르지 않는다.
//! - 저장은 사실 정보(제목·부제·저자 표기·출판사·출간 연월·ISBN·분류)만 한다. 소개글·표지·리뷰·가격은 저장하지 않는다
//!   (YES24 이용약관 제18조의 지적재산권 침해·업무 방해 금지를 지키려고).
//!
//! 진행: 처음에는 두 분류를 1쪽부터 끝(또는 2000년 이전 책이 나올 때)까지 거슬러 올라가며 모으고(백필),
//! 다 모은 뒤에는 `SCAN_EVERY_SECS`마다 앞쪽 몇 쪽만 다시 본다. 진행 상태는 `yes24_scan`에 있어 재시작해도 이어간다.

use std::{collections::HashMap, env, time::Duration};

use reqwest::{redirect, StatusCode, Url};
use sqlx::{query, Row, SqlitePool};

use super::html::{clean_text, inner_html};
use super::{contact_from, extract_meta, is_valid_isbn13, now, read_capped, user_agent, Step};

pub const CATEGORY_NOVEL: &str = "001001046001002";
pub const CATEGORY_SHORT: &str = "001001046001001";
pub const FORM_NOVEL: &str = "장편";
pub const FORM_SHORT: &str = "단편";

/// (분류 번호, 형식). 백필·재확인은 이 순서(장편 먼저)로 한다.
const CATEGORIES: [(&str, &str); 2] = [(CATEGORY_NOVEL, FORM_NOVEL), (CATEGORY_SHORT, FORM_SHORT)];

/// 대상 출판사. gotolib `src/lib/novelClassify.ts`의 `TARGET_PUBLISHERS`와 같은 목록이다 — 함께 고친다.
pub const TARGET_PUBLISHERS: [&str; 17] = [
    "문학동네",
    "민음사",
    "창비",
    "열린책들",
    "현대문학",
    "문학과지성사",
    "은행나무",
    "자음과모음",
    "을유문화사",
    "아침달",
    "교유서가",
    "열림원",
    "에피케",
    "북다",
    "위즈덤하우스",
    "봄날의책",
    "걷는사람",
];

const PAGE_SIZE: u32 = 24;
/// 백필은 이 달보다 오래된 책이 나오면 멈춘다(목록이 신상품순이라 그 뒤는 더 오래됐다).
const OLDEST_MONTH: &str = "2000-01";
/// 백필이 끝난 뒤 앞쪽을 다시 보는 간격과, 한 번에 볼 최대 쪽 수.
const SCAN_EVERY_SECS: i64 = 6 * 60 * 60;
const SCAN_MAX_PAGES: i64 = 10;
/// 목록 요청이 일시 오류면 그 분류는 이만큼 쉬었다 다시 본다.
const LIST_RETRY_SECS: i64 = 10 * 60;

pub const MIN_INTERVAL: Duration = Duration::from_secs(3);
const DEFAULT_INTERVAL: Duration = Duration::from_secs(5);
const IDLE: Duration = Duration::from_secs(60);
const BLOCK_PAUSE: Duration = Duration::from_secs(6 * 60 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_PAGE_BYTES: usize = 3 * 1024 * 1024;

/// 상품 페이지에서 ISBN을 못 찾았을 때 다시 볼 때까지.
const MISSING_RETRY_SECS: i64 = 30 * 24 * 60 * 60;
const MAX_ATTEMPTS: i64 = 5;
const EXHAUSTED_RETRY_SECS: i64 = 7 * 24 * 60 * 60;
const RETRY_BASE_SECS: i64 = 10 * 60;
const RETRY_MAX_SECS: i64 = 12 * 60 * 60;

pub const STATUS_PENDING: &str = "pending";
pub const STATUS_OK: &str = "ok";
pub const STATUS_MISSING: &str = "missing";
pub const STATUS_ERROR: &str = "error";

// ── 파싱 ─────────────────────────────────────────────────────────────────────
/// 분류 목록의 책 한 권.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem {
    pub goods_no: i64,
    pub title: String,
    pub subtitle: Option<String>,
    /// 표기 그대로("양귀자 저", "A, B 저/C 역")
    pub author: Option<String>,
    pub publisher: Option<String>,
    /// "2026-11"
    pub pub_month: Option<String>,
}

/// 목록 조각(`CategoryProductContents`)의 책들. 각 책은 `<li … data-goods-no="번호">`로 시작한다.
pub fn parse_list(html: &str) -> Vec<ListItem> {
    const MARKER: &str = "data-goods-no=\"";
    let starts: Vec<usize> = html.match_indices(MARKER).map(|(at, _)| at).collect();
    starts
        .iter()
        .enumerate()
        .filter_map(|(i, &start)| {
            let end = starts.get(i + 1).copied().unwrap_or(html.len());
            let chunk = &html[start..end];
            let digits: String = chunk[MARKER.len()..].chars().take_while(char::is_ascii_digit).collect();
            let goods_no = digits.parse().ok()?;
            let text = |tag: &str, marker: &str| {
                inner_html(chunk, tag, marker).map(clean_text).filter(|t| !t.is_empty())
            };
            Some(ListItem {
                goods_no,
                title: text("a", r#"class="gd_name""#)?,
                subtitle: text("span", r#"class="gd_nameE""#),
                author: text("span", r#"class="authPub info_auth""#),
                publisher: text("span", r#"class="authPub info_pub""#),
                pub_month: text("span", r#"class="authPub info_date""#).as_deref().and_then(month_of),
            })
        })
        .collect()
}

/// "2026년 11월" → "2026-11"
fn month_of(text: &str) -> Option<String> {
    let (year, rest) = text.split_once('년')?;
    let month = rest.trim().strip_suffix('월')?.trim();
    let (year, month): (u32, u32) = (year.trim().parse().ok()?, month.parse().ok()?);
    (1..=12).contains(&month).then(|| format!("{year:04}-{month:02}"))
}

/// 상품 페이지의 ISBN-13(`<meta property="books:isbn" content="…">`). 검사 숫자까지 맞아야 한다.
pub fn parse_isbn(html: &str) -> Option<String> {
    extract_meta(html, "property", "books:isbn").filter(|isbn| is_valid_isbn13(isbn))
}

/// 출판사 표기가 대상 출판사인가. "(주)"·공백을 떼고 같거나, 뒤에 "출판사"·"출판"만 붙었을 때만 인정한다
/// (예: "창비교육"은 창비가 아니다).
pub fn publisher_matches(raw: &str) -> bool {
    let name: String = raw
        .replace("(주)", "")
        .replace('㈜', "")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let base = name.strip_suffix("출판사").or_else(|| name.strip_suffix("출판")).unwrap_or(&name);
    TARGET_PUBLISHERS.contains(&base)
}

// ── YES24 ────────────────────────────────────────────────────────────────────
#[derive(Debug, PartialEq, Eq)]
pub enum Fetched {
    Page(String),
    NotFound,
    /// 403/429: 작업 전체가 쉬어야 한다
    Blocked,
    /// 5xx·타임아웃·연결 실패
    Transient,
}

#[derive(Clone)]
pub struct Yes24Source {
    http: reqwest::Client,
    /// 예: `https://www.yes24.com` (테스트에서는 로컬 가짜 서버)
    base: String,
}

impl Yes24Source {
    pub fn new(base: &str, user_agent: &str) -> Result<Self, reqwest::Error> {
        // 리다이렉트는 같은 호스트 안에서만 따라간다.
        let host = Url::parse(base).ok().and_then(|url| url.host_str().map(str::to_string));
        let policy = redirect::Policy::custom(move |attempt| {
            let same_host = attempt.url().host_str() == host.as_deref();
            if attempt.previous().len() >= 5 || !same_host {
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
        Ok(Self { http, base: base.trim_end_matches('/').to_string() })
    }

    async fn get(&self, path_and_query: &str) -> Fetched {
        let response = match self.http.get(format!("{}{path_and_query}", self.base)).send().await {
            Ok(response) => response,
            Err(_) => return Fetched::Transient,
        };
        match response.status() {
            StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS => return Fetched::Blocked,
            StatusCode::NOT_FOUND | StatusCode::GONE => return Fetched::NotFound,
            status if !status.is_success() => return Fetched::Transient,
            _ => {}
        }
        match read_capped(response, MAX_PAGE_BYTES).await {
            Some(body) => Fetched::Page(String::from_utf8_lossy(&body).into_owned()),
            None => Fetched::Transient,
        }
    }

    pub async fn list(&self, category: &str, page: i64) -> Fetched {
        self.get(&format!(
            "/product/category/CategoryProductContents?dispNo={category}&order=RECENT&page={page}&size={PAGE_SIZE}"
        ))
        .await
    }

    pub async fn goods(&self, goods_no: i64) -> Fetched {
        self.get(&format!("/product/goods/{goods_no}")).await
    }
}

// ── DB ───────────────────────────────────────────────────────────────────────
/// 대상 출판사 책만 넣는다(이미 있으면 그대로). (새로 넣은 수, 대상 출판사 책 중 이미 있던 수)
async fn insert_items(pool: &SqlitePool, form: &str, items: &[ListItem], now: i64) -> Result<(u64, u64), sqlx::Error> {
    let (mut new, mut known) = (0, 0);
    for item in items.iter().filter(|item| item.publisher.as_deref().is_some_and(publisher_matches)) {
        let inserted = query(
            "INSERT OR IGNORE INTO yes24_books
             (goods_no, form, title, subtitle, author, publisher, pub_month, status, attempts, next_try_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, 0, ?)",
        )
        .bind(item.goods_no)
        .bind(form)
        .bind(&item.title)
        .bind(&item.subtitle)
        .bind(&item.author)
        .bind(&item.publisher)
        .bind(&item.pub_month)
        .bind(STATUS_PENDING)
        .bind(now)
        .execute(pool)
        .await?
        .rows_affected();
        if inserted > 0 {
            new += 1;
        } else {
            known += 1;
        }
    }
    Ok((new, known))
}

/// ISBN별 YES24 형식("장편"/"단편"). 한 ISBN이 두 분류에 다 걸리면 장편으로 본다.
pub async fn forms_by_isbn(pool: &SqlitePool, isbns: &[String]) -> Result<HashMap<String, &'static str>, sqlx::Error> {
    if isbns.is_empty() {
        return Ok(HashMap::new());
    }
    let sql = format!(
        "SELECT isbn13, form FROM yes24_books WHERE isbn13 IN ({})",
        vec!["?"; isbns.len()].join(",")
    );
    let mut q = query(&sql);
    for isbn in isbns {
        q = q.bind(isbn);
    }
    let mut map: HashMap<String, &'static str> = HashMap::new();
    for row in q.fetch_all(pool).await? {
        let isbn: String = row.try_get("isbn13")?;
        let form = if row.try_get::<String, _>("form")? == FORM_NOVEL { FORM_NOVEL } else { FORM_SHORT };
        let entry = map.entry(isbn).or_insert(form);
        if form == FORM_NOVEL {
            *entry = FORM_NOVEL;
        }
    }
    Ok(map)
}

/// "한국문학" 탭 목록 한 권: YES24 "한국 장편소설"/"한국 단편소설"에 있고 상품 페이지에서 ISBN까지 받은 책.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NovelListItem {
    pub isbn13: String,
    pub goods_no: i64,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// 표기 그대로("양귀자 저")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    /// "2026-11"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pub_month: Option<String>,
}

/// 한 해(`"2026"`)에 나온 YES24 분류(`form`: `FORM_NOVEL`/`FORM_SHORT`) 책(대상 출판사, ISBN 있는 것만).
/// 최근 달·최근 상품 순. 같은 ISBN이 상품 번호 여러 개로 올라 있으면 가장 최근 상품 하나만 둔다.
pub async fn novels_of_year(pool: &SqlitePool, form: &str, year: &str) -> Result<Vec<NovelListItem>, sqlx::Error> {
    let rows = query(
        "SELECT goods_no, title, subtitle, author, publisher, pub_month, isbn13 FROM yes24_books
         WHERE form = ? AND isbn13 IS NOT NULL AND pub_month LIKE ?
         ORDER BY pub_month DESC, goods_no DESC",
    )
    .bind(form)
    .bind(format!("{year}-%"))
    .fetch_all(pool)
    .await?;
    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();
    for row in rows {
        let isbn13: String = row.try_get("isbn13")?;
        if !seen.insert(isbn13.clone()) {
            continue;
        }
        items.push(NovelListItem {
            isbn13,
            goods_no: row.try_get("goods_no")?,
            title: row.try_get("title")?,
            subtitle: row.try_get("subtitle")?,
            author: row.try_get("author")?,
            publisher: row.try_get("publisher")?,
            pub_month: row.try_get("pub_month")?,
        });
    }
    Ok(items)
}

// ── 수집 작업 ────────────────────────────────────────────────────────────────
/// 지금 해도 되는 한 건을 처리한다. 순서: ① ISBN이 아직 없는 책의 상품 페이지 ② 백필(아직 끝나지 않은 분류)
/// ③ 진행 중이거나 때가 된 앞쪽 재확인. 시간 대기는 하지 않는다(대기는 `run_worker`의 몫).
pub async fn process_next(pool: &SqlitePool, source: &Yes24Source, now: i64) -> Step {
    match next_detail(pool, now).await {
        Ok(Some((goods_no, attempts))) => return fetch_detail(pool, source, goods_no, attempts, now).await,
        Ok(None) => {}
        Err(e) => {
            eprintln!("[yes24] 상품 대기열을 읽지 못했습니다: {e}");
            return Step::Idle;
        }
    }
    for (category, form) in CATEGORIES {
        let row = match query(
            "SELECT backfill_page, backfill_done, scan_page, last_scan_at, retry_at FROM yes24_scan WHERE category = ?",
        )
        .bind(category)
        .fetch_optional(pool)
        .await
        {
            Ok(Some(row)) => row,
            Ok(None) => continue,
            Err(e) => {
                eprintln!("[yes24] 진행 상태를 읽지 못했습니다: {e}");
                return Step::Idle;
            }
        };
        let get = |name: &str| row.try_get::<Option<i64>, _>(name).ok().flatten();
        if get("retry_at").unwrap_or(0) > now {
            continue;
        }
        if get("backfill_done").unwrap_or(0) == 0 {
            return walk(pool, source, category, form, get("backfill_page").unwrap_or(1), Walk::Backfill, now).await;
        }
        if let Some(page) = get("scan_page") {
            return walk(pool, source, category, form, page, Walk::Scan, now).await;
        }
        if get("last_scan_at").unwrap_or(0) + SCAN_EVERY_SECS <= now {
            return walk(pool, source, category, form, 1, Walk::Scan, now).await;
        }
    }
    Step::Idle
}

async fn next_detail(pool: &SqlitePool, now: i64) -> Result<Option<(i64, i64)>, sqlx::Error> {
    let row = query(
        "SELECT goods_no, attempts FROM yes24_books
         WHERE isbn13 IS NULL AND status IN (?, ?, ?) AND next_try_at <= ?
         ORDER BY created_at, goods_no LIMIT 1",
    )
    .bind(STATUS_PENDING)
    .bind(STATUS_ERROR)
    .bind(STATUS_MISSING)
    .bind(now)
    .fetch_optional(pool)
    .await?;
    row.map(|row| Ok((row.try_get("goods_no")?, row.try_get("attempts")?))).transpose()
}

async fn fetch_detail(pool: &SqlitePool, source: &Yes24Source, goods_no: i64, attempts: i64, now: i64) -> Step {
    let recorded = match source.goods(goods_no).await {
        Fetched::Blocked => return Step::Blocked,
        Fetched::Page(html) => match parse_isbn(&html) {
            Some(isbn) => query("UPDATE yes24_books SET isbn13 = ?, status = ?, fetched_at = ? WHERE goods_no = ?")
                .bind(isbn)
                .bind(STATUS_OK)
                .bind(now)
                .bind(goods_no)
                .execute(pool)
                .await
                .map(|_| ()),
            None => mark_missing(pool, goods_no, MISSING_RETRY_SECS, now).await,
        },
        Fetched::NotFound => mark_missing(pool, goods_no, MISSING_RETRY_SECS, now).await,
        Fetched::Transient => {
            let attempts = attempts + 1;
            if attempts >= MAX_ATTEMPTS {
                mark_missing(pool, goods_no, EXHAUSTED_RETRY_SECS, now).await
            } else {
                query("UPDATE yes24_books SET status = ?, attempts = ?, next_try_at = ? WHERE goods_no = ?")
                    .bind(STATUS_ERROR)
                    .bind(attempts)
                    .bind(now + (RETRY_BASE_SECS << (attempts - 1)).min(RETRY_MAX_SECS))
                    .bind(goods_no)
                    .execute(pool)
                    .await
                    .map(|_| ())
            }
        }
    };
    if let Err(e) = recorded {
        eprintln!("[yes24] 상품 {goods_no} 결과를 기록하지 못했습니다: {e}");
    }
    Step::Processed
}

/// ISBN을 못 찾은 상품은 `missing`으로 두고 한참 뒤에 다시 본다(`next_detail`이 때가 된 missing도 고른다).
async fn mark_missing(pool: &SqlitePool, goods_no: i64, retry_after_secs: i64, now: i64) -> Result<(), sqlx::Error> {
    query("UPDATE yes24_books SET status = ?, attempts = 0, next_try_at = ?, fetched_at = ? WHERE goods_no = ?")
        .bind(STATUS_MISSING)
        .bind(now + retry_after_secs)
        .bind(now)
        .bind(goods_no)
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Walk {
    /// 처음 한 번, 끝(또는 2000년 이전)까지 거슬러 올라간다
    Backfill,
    /// 앞쪽만 다시 본다: 새 책이 더 없거나 `SCAN_MAX_PAGES`에 닿으면 멈춘다
    Scan,
}

async fn walk(pool: &SqlitePool, source: &Yes24Source, category: &str, form: &str, page: i64, mode: Walk, now: i64) -> Step {
    let html = match source.list(category, page).await {
        Fetched::Blocked => return Step::Blocked,
        Fetched::Page(html) => html,
        Fetched::NotFound | Fetched::Transient => {
            if let Err(e) = query("UPDATE yes24_scan SET retry_at = ? WHERE category = ?")
                .bind(now + LIST_RETRY_SECS)
                .bind(category)
                .execute(pool)
                .await
            {
                eprintln!("[yes24] 진행 상태를 기록하지 못했습니다: {e}");
            }
            return Step::Processed;
        }
    };
    let items = parse_list(&html);
    let (new, known) = match insert_items(pool, form, &items, now).await {
        Ok(counts) => counts,
        Err(e) => {
            eprintln!("[yes24] 목록을 저장하지 못했습니다: {e}");
            return Step::Processed;
        }
    };
    let reached_oldest = items
        .iter()
        .filter_map(|item| item.pub_month.as_deref())
        .last()
        .is_some_and(|month| month < OLDEST_MONTH);
    let finished = match mode {
        Walk::Backfill => items.is_empty() || reached_oldest,
        Walk::Scan => items.is_empty() || (known > 0 && new == 0) || page >= SCAN_MAX_PAGES,
    };
    let sql = match (mode, finished) {
        (Walk::Backfill, true) => "UPDATE yes24_scan SET backfill_done = 1, last_scan_at = ?, retry_at = 0 WHERE category = ?",
        (Walk::Backfill, false) => "UPDATE yes24_scan SET backfill_page = ?, retry_at = 0 WHERE category = ?",
        (Walk::Scan, true) => "UPDATE yes24_scan SET scan_page = NULL, last_scan_at = ?, retry_at = 0 WHERE category = ?",
        (Walk::Scan, false) => "UPDATE yes24_scan SET scan_page = ?, retry_at = 0 WHERE category = ?",
    };
    let value = if finished { now } else { page + 1 };
    if let Err(e) = query(sql).bind(value).bind(category).execute(pool).await {
        eprintln!("[yes24] 진행 상태를 기록하지 못했습니다: {e}");
    }
    Step::Processed
}

pub async fn run_worker(pool: SqlitePool, source: Yes24Source, interval: Duration) {
    let interval = interval.max(MIN_INTERVAL);
    println!("[yes24] 수집 작업 시작 (간격 {}초)", interval.as_secs());
    loop {
        let wait = match process_next(&pool, &source, now()).await {
            Step::Idle => IDLE,
            Step::Processed => interval,
            Step::Blocked => {
                eprintln!("[yes24] YES24가 요청을 막았습니다(403/429). {}시간 쉽니다.", BLOCK_PAUSE.as_secs() / 3600);
                BLOCK_PAUSE
            }
        };
        tokio::time::sleep(wait).await;
    }
}

/// 환경 변수로 수집 작업을 켠다. 기본은 꺼짐.
/// - `YES24_FETCH_ENABLED=true` 일 때만 시작한다
/// - `YES24_FETCH_INTERVAL_SECS`(기본 5, 3 미만은 3으로)
/// - `User-Agent`는 알라딘 수집과 같은 `gotolib-covers/0.1 (+COVER_CONTACT)`로 식별한다.
pub fn spawn_worker_from_env(pool: SqlitePool) {
    if env::var("YES24_FETCH_ENABLED").map(|v| v != "true").unwrap_or(true) {
        println!("[yes24] 수집 작업이 꺼져 있습니다(YES24_FETCH_ENABLED).");
        return;
    }
    let user_agent = user_agent(&contact_from(env::var("COVER_CONTACT").ok().as_deref()));
    let interval = env::var("YES24_FETCH_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_INTERVAL);
    let source = match Yes24Source::new("https://www.yes24.com", &user_agent) {
        Ok(source) => source,
        Err(e) => {
            eprintln!("[yes24] HTTP 클라이언트를 만들지 못했습니다: {e}");
            return;
        }
    };
    tokio::spawn(async move {
        loop {
            let handle = tokio::spawn(run_worker(pool.clone(), source.clone(), interval));
            if let Err(e) = handle.await {
                eprintln!("[yes24] 수집 작업이 비정상 종료했습니다: {e}. 1분 뒤 다시 시작합니다.");
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
}

#[cfg(test)]
mod tests;
