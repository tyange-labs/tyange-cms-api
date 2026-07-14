use std::sync::Arc;

use poem::{
    handler,
    web::{Data, Json},
    Error, Request,
};

use crate::{
    models::{CustomResponse, WebPushSubscriptionListResponse},
    web_push::list_push_subscriptions,
    AppState,
};
use tyange_cms_api::auth::authorization::current_user;

#[handler]
pub async fn get_push_subscriptions(
    req: &Request,
    data: Data<&Arc<AppState>>,
) -> Result<Json<CustomResponse<WebPushSubscriptionListResponse>>, Error> {
    let user = current_user(req)?;
    let subscriptions = list_push_subscriptions(&data.db, &user.user_id)
        .await
        .map_err(|err| Error::from_string(err.message, err.status))?;

    Ok(Json(CustomResponse {
        status: true,
        data: Some(WebPushSubscriptionListResponse { subscriptions }),
        message: None,
    }))
}
