use std::sync::Arc;

use poem::{
    handler,
    http::StatusCode,
    web::{Data, Json},
    Error, Request,
};
use sqlx::query_as;
use tyange_cms_api::auth::authorization::current_user;

use crate::models::{AppState, MeResponse};

#[handler]
pub async fn me(req: &Request, data: Data<&Arc<AppState>>) -> Result<Json<MeResponse>, Error> {
    let user = current_user(req)?;

    let me = query_as::<_, MeResponse>(
        "SELECT user_id, user_role, display_name, avatar_url, bio FROM users WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_optional(&data.db)
    .await
    .map_err(|e| {
        Error::from_string(
            format!("사용자 정보 조회 실패: {}", e),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
    })?
    .ok_or_else(|| Error::from_string("사용자를 찾을 수 없습니다.", StatusCode::NOT_FOUND))?;

    Ok(Json(me))
}
