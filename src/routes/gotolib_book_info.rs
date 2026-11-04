//! gotolib "한국문학" 탭: 장편/소설집 판정을 돕는 책 정보 조회. `GET /gotolib/book-info?isbn=A,B,C`
//!
//! 이 핸들러는 DB만 본다. 알라딘에는 가지 않는다 — 수집은 `book_info::run_worker` 한 곳이, 표지를 모을 때
//! 같은 페이지에서 함께 한다(covers.rs 참고). 모르는 ISBN은 **표지 수집과 같은 큐**에 등록한다: 두 기능이
//! 같은 행을 보므로, 이미 표지를 조회한 책은 바로 책 정보도 같이 나온다.

use std::{collections::BTreeMap, env, sync::Arc};

use poem::{
    handler,
    http::StatusCode,
    web::{Data, Json, Query},
    Error, IntoResponse, Response,
};
use serde::{Deserialize, Serialize};

use crate::{book_info, models::AppState};

/// 한 번에 물을 수 있는 ISBN 수. gotolib의 "한국문학" 탭도 한 페이지(20권)씩 묻는다.
pub const MAX_ISBNS_PER_REQUEST: usize = 50;

#[derive(Deserialize)]
pub struct BookInfoQuery {
    isbn: Option<String>,
}

#[derive(Serialize)]
pub struct BookInfoEntry {
    /// "ok"(수집 끝, 필드가 있을 수 있음) | "pending"(대기·수집 중 — 아직 믿을 값이 없다)
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    category: Option<String>,
    #[serde(rename = "formHint", skip_serializing_if = "Option::is_none")]
    form_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pages: Option<i64>,
}

#[derive(Serialize)]
pub struct BookInfoResponse {
    info: BTreeMap<String, BookInfoEntry>,
}

fn db_error(e: sqlx::Error) -> Error {
    eprintln!("Database error: {:?}", e);
    Error::from_string("Database error", StatusCode::INTERNAL_SERVER_ERROR)
}

fn bad_request(message: &str) -> Error {
    Error::from_string(message.to_string(), StatusCode::BAD_REQUEST)
}

fn queue_max() -> i64 {
    env::var("COVER_QUEUE_MAX")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|v| *v >= 0)
        .unwrap_or(book_info::DEFAULT_QUEUE_MAX)
}

/// 쉼표로 이어진 ISBN을 검증하고 중복을 없앤다(처음 나온 순서 유지). 하나라도 잘못되면 요청 전체를 거부한다.
fn parse_isbns(raw: &str) -> Result<Vec<String>, Error> {
    let mut seen = std::collections::HashSet::new();
    let mut isbns = Vec::new();
    for part in raw.split(',') {
        if !book_info::is_valid_isbn13(part) {
            return Err(bad_request("올바르지 않은 ISBN-13이 있습니다."));
        }
        if seen.insert(part) {
            isbns.push(part.to_string());
        }
    }
    if isbns.len() > MAX_ISBNS_PER_REQUEST {
        return Err(bad_request("한 번에 물을 수 있는 ISBN 수를 넘었습니다."));
    }
    Ok(isbns)
}

#[handler]
pub async fn get_book_info(
    Query(params): Query<BookInfoQuery>,
    data: Data<&Arc<AppState>>,
) -> poem::Result<Response> {
    let raw = params.isbn.unwrap_or_default();
    if raw.trim().is_empty() {
        return Err(bad_request("isbn이 필요합니다."));
    }
    let isbns = parse_isbns(&raw)?;
    let now = book_info::now();
    let pool = &data.db;

    // 표지와 같은 규칙: 오래된 "표지 없음" 기록은 다시 확인하도록 대기로 돌린다.
    book_info::requeue_stale_missing(pool, &isbns, now).await.map_err(db_error)?;

    let known = book_info::book_infos(pool, &isbns).await.map_err(db_error)?;

    let unknown: Vec<String> = isbns.iter().filter(|isbn| !known.contains_key(*isbn)).cloned().collect();
    if !unknown.is_empty() {
        let room = (queue_max() - book_info::queue_len(pool).await.map_err(db_error)?).max(0) as usize;
        let to_register = &unknown[..unknown.len().min(room)];
        book_info::enqueue(pool, to_register, now).await.map_err(db_error)?;
    }

    let mut info = BTreeMap::new();
    for isbn in isbns {
        let entry = match known.get(&isbn) {
            Some(row) if row.status == book_info::STATUS_OK || row.status == book_info::STATUS_MISSING => BookInfoEntry {
                status: "ok",
                category: row.category_path.clone(),
                form_hint: row.form_hint.clone(),
                pages: row.pages,
            },
            // pending, error, 그리고 큐가 가득 차 등록하지 못한 것은 모두 '아직'으로 보인다.
            _ => BookInfoEntry { status: "pending", category: None, form_hint: None, pages: None },
        };
        info.insert(isbn, entry);
    }

    // 곧 바뀔 수 있는 응답이라 캐시하지 않는다.
    Ok(Json(BookInfoResponse { info }).with_header("Cache-Control", "no-cache").into_response())
}
