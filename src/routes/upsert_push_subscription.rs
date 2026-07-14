use std::sync::Arc;

use poem::{
    handler,
    web::{Data, Json},
    Error, Request,
};

use crate::{
    models::{CustomResponse, UpsertPushSubscriptionRequest, WebPushSubscriptionResponse},
    web_push::upsert_push_subscription as upsert_push_subscription_service,
    AppState,
};
use tyange_cms_api::auth::authorization::current_user;

#[handler]
pub async fn upsert_push_subscription(
    req: &Request,
    Json(payload): Json<UpsertPushSubscriptionRequest>,
    data: Data<&Arc<AppState>>,
) -> Result<Json<CustomResponse<WebPushSubscriptionResponse>>, Error> {
    let user = current_user(req)?;
    let user_agent = req
        .headers()
        .get("User-Agent")
        .and_then(|value| value.to_str().ok());
    let subscription = upsert_push_subscription_service(
        &data.db,
        &user.user_id,
        &payload.endpoint,
        &payload.keys.p256dh,
        &payload.keys.auth,
        user_agent,
    )
    .await
    .map_err(|err| Error::from_string(err.message, err.status))?;

    Ok(Json(CustomResponse {
        status: true,
        data: Some(subscription),
        message: Some("Push 구독이 저장되었습니다.".to_string()),
    }))
}
