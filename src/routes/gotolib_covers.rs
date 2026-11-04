//! gotolib 책 표지 조회. `GET /gotolib/covers?isbn=A,B,C`
//!
//! 이 핸들러는 DB만 본다. 알라딘에는 가지 않는다(수집은 `book_info::run_worker` 한 곳이 한다).
//! 모르는 ISBN은 수집 대기로 등록하고, 큐가 상한에 닿았으면 등록하지 않는다.

use std::{collections::BTreeMap, env, sync::Arc};

use poem::{
    handler,
    http::StatusCode,
    web::{Data, Json, Query},
    Error, IntoResponse, Response,
};
use serde::{Deserialize, Serialize};

use crate::{book_info, models::AppState};

/// 한 번에 물을 수 있는 ISBN 수. 앱은 한 페이지(20권)씩 묻는다.
pub const MAX_ISBNS_PER_REQUEST: usize = 50;

/// 서버가 이 경로에서 썸네일 파일을 내보낸다(`main.rs`의 정적 제공).
pub const IMAGE_PATH_PREFIX: &str = "/gotolib/cover-images";

#[derive(Deserialize)]
pub struct CoversQuery {
    isbn: Option<String>,
}

#[derive(Serialize)]
pub struct CoverEntry {
    /// "ok" | "pending" | "missing". 서버 내부 상태(error 등)는 앱에 새어 나가지 않는다.
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
}

#[derive(Serialize)]
pub struct CoversResponse {
    covers: BTreeMap<String, CoverEntry>,
}

fn db_error(e: sqlx::Error) -> Error {
    eprintln!("Database error: {:?}", e);
    Error::from_string("Database error", StatusCode::INTERNAL_SERVER_ERROR)
}

fn bad_request(message: &str) -> Error {
    Error::from_string(message.to_string(), StatusCode::BAD_REQUEST)
}

/// 수집 대기 큐의 상한. 누군가 실존하는 ISBN을 계속 만들어 보내도 큐가 무한정 자라지 않게 한다.
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
pub async fn get_covers(
    Query(params): Query<CoversQuery>,
    data: Data<&Arc<AppState>>,
) -> poem::Result<Response> {
    let raw = params.isbn.unwrap_or_default();
    if raw.trim().is_empty() {
        return Err(bad_request("isbn이 필요합니다."));
    }
    let isbns = parse_isbns(&raw)?;
    let now = book_info::now();
    let pool = &data.db;

    // 표지가 없다고 기록된 지 오래된 ISBN은 다시 확인하도록 대기로 돌린다.
    book_info::requeue_stale_missing(pool, &isbns, now).await.map_err(db_error)?;

    let known = book_info::statuses(pool, &isbns).await.map_err(db_error)?;

    // 모르는 ISBN은 큐에 여유가 있는 만큼만 등록한다.
    let unknown: Vec<String> = isbns.iter().filter(|isbn| !known.contains_key(*isbn)).cloned().collect();
    if !unknown.is_empty() {
        let room = (queue_max() - book_info::queue_len(pool).await.map_err(db_error)?).max(0) as usize;
        let to_register = &unknown[..unknown.len().min(room)];
        book_info::enqueue(pool, to_register, now).await.map_err(db_error)?;
    }

    let mut covers_map = BTreeMap::new();
    for isbn in isbns {
        let entry = match known.get(&isbn).map(String::as_str) {
            Some(book_info::STATUS_OK) => CoverEntry {
                status: "ok",
                url: Some(format!("{IMAGE_PATH_PREFIX}/{isbn}.jpg")),
            },
            Some(book_info::STATUS_MISSING) => CoverEntry { status: "missing", url: None },
            // pending, error, 그리고 큐가 가득 차 등록하지 못한 것은 모두 '아직'으로 보인다.
            _ => CoverEntry { status: "pending", url: None },
        };
        covers_map.insert(isbn, entry);
    }

    // 곧 바뀔 수 있는 응답이라 캐시하지 않는다(이미지 파일 쪽은 길게 캐시한다).
    Ok(Json(CoversResponse { covers: covers_map })
        .with_header("Cache-Control", "no-cache")
        .into_response())
}
