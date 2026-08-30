use std::sync::Arc;

use poem::{
    handler,
    http::StatusCode,
    web::{Data, Json},
    Error,
};
use sqlx::query;

use crate::{
    models::{CreateWeddingGuestRequest, CreateWeddingGuestResponse, CustomResponse},
    AppState,
};

/// lovelog(모바일 청첩장) RSVP 제출. 인증 없는 공개 쓰기라 main.rs에서
/// RateLimit + SizeLimit을 함께 건다.
const NAME_MAX_LENGTH: usize = 20;
const COMPANIONS_MAX: i64 = 10;

/// 전시는 12시에 열고 5시에 닫는다. 하객이 고르는 값은 한 시간짜리 시간대의 시작 시각이라
/// 마지막 칸(4-5시)의 시작인 16이 상한이다.
const VISIT_HOUR_MIN: i64 = 12;
const VISIT_HOUR_MAX: i64 = 16;

#[handler]
pub async fn create_wedding_guest(
    Json(payload): Json<CreateWeddingGuestRequest>,
    data: Data<&Arc<AppState>>,
) -> Result<Json<CustomResponse<CreateWeddingGuestResponse>>, Error> {
    let name = payload.name.trim();

    if name.is_empty() {
        return Err(Error::from_string(
            "이름을 입력해 주세요.",
            StatusCode::BAD_REQUEST,
        ));
    }

    // 한글이 들어오므로 바이트 길이가 아니라 문자 수로 센다
    if name.chars().count() > NAME_MAX_LENGTH {
        return Err(Error::from_string(
            format!("이름은 {}자까지 입력할 수 있어요.", NAME_MAX_LENGTH),
            StatusCode::BAD_REQUEST,
        ));
    }

    if payload.companions < 0 || payload.companions > COMPANIONS_MAX {
        return Err(Error::from_string(
            format!(
                "동반 인원은 0명에서 {}명 사이로 입력해 주세요.",
                COMPANIONS_MAX
            ),
            StatusCode::BAD_REQUEST,
        ));
    }

    let visit_hour = payload.visit_hour.ok_or_else(|| {
        Error::from_string("방문 예정 시간을 골라 주세요.", StatusCode::BAD_REQUEST)
    })?;

    if !(VISIT_HOUR_MIN..=VISIT_HOUR_MAX).contains(&visit_hour) {
        return Err(Error::from_string(
            "방문 예정 시간은 전시 시간 안에서 골라 주세요.",
            StatusCode::BAD_REQUEST,
        ));
    }

    let result = query(
        r#"
        INSERT INTO wedding_guests (name, companions, visit_hour)
        VALUES (?, ?, ?)
        "#,
    )
    .bind(name)
    .bind(payload.companions)
    .bind(visit_hour)
    .execute(&data.db)
    .await
    .map_err(|err| {
        Error::from_string(
            format!("Error saving RSVP: {}", err),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
    })?;

    Ok(Json(CustomResponse {
        status: true,
        data: Some(CreateWeddingGuestResponse {
            guest_id: result.last_insert_rowid(),
        }),
        message: Some("참석 여부가 전달되었어요.".to_string()),
    }))
}
