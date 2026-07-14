use std::sync::Arc;

use poem::{
    handler,
    http::StatusCode,
    web::{Data, Json},
    Error, Request,
};

use crate::{models::DeletePushSubscriptionRequest, web_push::revoke_push_subscription, AppState};
use tyange_cms_api::auth::authorization::current_user;

#[handler]
pub async fn delete_push_subscription(
    req: &Request,
    Json(payload): Json<DeletePushSubscriptionRequest>,
    data: Data<&Arc<AppState>>,
) -> Result<StatusCode, Error> {
    let user = current_user(req)?;
    let deleted = revoke_push_subscription(&data.db, &user.user_id, &payload.endpoint)
        .await
        .map_err(|err| Error::from_string(err.message, err.status))?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(Error::from_string(
            "해당 Push 구독을 찾을 수 없습니다.",
            StatusCode::NOT_FOUND,
        ))
    }
}
