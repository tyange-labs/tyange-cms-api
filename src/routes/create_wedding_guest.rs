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
            format!("동반 인원은 0명에서 {}명 사이로 입력해 주세요.", COMPANIONS_MAX),
            StatusCode::BAD_REQUEST,
        ));
    }

    let result = query(
        r#"
        INSERT INTO wedding_guests (name, companions)
        VALUES (?, ?)
        "#,
    )
    .bind(name)
    .bind(payload.companions)
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
