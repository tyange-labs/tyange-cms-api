//! gotolib "한국문학" 탭 목록: `GET /gotolib/korean-novels?year=2026&form=novel`
//!
//! YES24 "한국 장편소설"(`form=novel`, 기본)·"한국 단편소설"(`form=short`) 분류에서 모은 17개 출판사 책
//! (종이책, ISBN 있는 것) 중 그해에 나온 것을 돌려준다.
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
    form: Option<String>,
}

#[derive(Serialize)]
pub struct KoreanNovelsResponse {
    year: String,
    form: &'static str,
    books: Vec<yes24::NovelListItem>,
}

/// 1900~2999 네 자리 연도만 받는다.
fn valid_year(raw: &str) -> bool {
    raw.len() == 4 && raw.chars().all(|c| c.is_ascii_digit()) && (raw.starts_with('1') || raw.starts_with('2'))
}

/// 주소에 한글을 넣지 않으려고 영문 이름으로 받는다. 없으면 장편(이 파라미터 이전 앱 호환).
fn parse_form(raw: Option<&str>) -> Option<(&'static str, &'static str)> {
    match raw.unwrap_or("novel") {
        "novel" => Some(("novel", yes24::FORM_NOVEL)),
        "short" => Some(("short", yes24::FORM_SHORT)),
        _ => None,
    }
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
    let Some((form, db_form)) = parse_form(params.form.as_deref()) else {
        return Err(Error::from_string("form은 novel 또는 short입니다.", StatusCode::BAD_REQUEST));
    };
    let books = yes24::novels_of_year(&data.db, db_form, &year).await.map_err(|e| {
        eprintln!("Database error: {:?}", e);
        Error::from_string("Database error", StatusCode::INTERNAL_SERVER_ERROR)
    })?;
    // 수집 작업이 계속 채우는 목록이라 오래 캐시하지 않는다.
    Ok(Json(KoreanNovelsResponse { year, form, books })
        .with_header("Cache-Control", "public, max-age=300")
        .into_response())
}
