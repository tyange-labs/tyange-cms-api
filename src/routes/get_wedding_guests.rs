use std::sync::Arc;

use poem::{
    handler,
    http::StatusCode,
    web::{Data, Json},
    Error,
};
use sqlx::query_as;

use crate::{
    models::{CustomResponse, WeddingGuest, WeddingGuestListResponse},
    AppState,
};

/// 하객 실명이 담긴 목록이라 main.rs에서 AdminOnly + Auth를 건다.
#[handler]
pub async fn get_wedding_guests(
    data: Data<&Arc<AppState>>,
) -> Result<Json<CustomResponse<WeddingGuestListResponse>>, Error> {
    let guests = query_as::<_, WeddingGuest>(
        r#"
        SELECT guest_id, name, companions, visit_hour, created_at
        FROM wedding_guests
        ORDER BY created_at DESC, guest_id DESC
        "#,
    )
    .fetch_all(&data.db)
    .await
    .map_err(|err| {
        Error::from_string(
            format!("Error fetching wedding guests: {}", err),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
    })?;

    let total_guests = guests.len() as i64;
    // 본인 + 동반 인원
    let total_attendees = guests.iter().map(|g| g.companions + 1).sum();

    Ok(Json(CustomResponse {
        status: true,
        data: Some(WeddingGuestListResponse {
            guests,
            total_guests,
            total_attendees,
        }),
        message: None,
    }))
}
