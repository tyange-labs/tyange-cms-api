//! gotolib "한국문학" 탭 목록: `GET /gotolib/korean-novels?year=2026`
//!
//! YES24 "한국 장편소설" 분류에서 모은 17개 출판사 책(종이책, ISBN 있는 것) 중 그해에 나온 것을 돌려준다.
//! DB만 본다 — YES24에는 가지 않는다(수집은 `book_info::yes24::run_worker` 한 곳에서 한다).

use std::sync::Arc;

use poem::{
    handler,
    http::StatusCode,
    web::{Data, Json, Query},
    Error, IntoResponse, Response,
};
use serde::{Deserialize, Serialize};

use crate::{book_info::yes24, models::AppState};

#[derive(Deserialize)]
pub struct KoreanNovelsQuery {
    year: Option<String>,
}

#[derive(Serialize)]
pub struct KoreanNovelsResponse {
    year: String,
    books: Vec<yes24::NovelListItem>,
}

/// 1900~2999 네 자리 연도만 받는다.
fn valid_year(raw: &str) -> bool {
    raw.len() == 4 && raw.chars().all(|c| c.is_ascii_digit()) && (raw.starts_with('1') || raw.starts_with('2'))
}

#[handler]
pub async fn get_korean_novels(
    Query(params): Query<KoreanNovelsQuery>,
    data: Data<&Arc<AppState>>,
) -> poem::Result<Response> {
    let year = params.year.unwrap_or_default();
    if !valid_year(&year) {
        return Err(Error::from_string("year(네 자리 연도)가 필요합니다.", StatusCode::BAD_REQUEST));
    }
    let books = yes24::novels_of_year(&data.db, &year).await.map_err(|e| {
        eprintln!("Database error: {:?}", e);
        Error::from_string("Database error", StatusCode::INTERNAL_SERVER_ERROR)
    })?;
    // 수집 작업이 계속 채우는 목록이라 오래 캐시하지 않는다.
    Ok(Json(KoreanNovelsResponse { year, books })
        .with_header("Cache-Control", "public, max-age=300")
        .into_response())
}
