//! gotolib 찜 소장 알림의 구독 관리. 키 조회는 로그인 없이, 나머지는 로그인한 계정만.
//!
//! 알림 기능이 꺼져 있으면(`GOTOLIB_PUSH_ENABLED`) 전부 503이다. 구독 endpoint는 서버가 POST를 보내는 주소라서
//! 알려진 푸시 서비스 호스트만 받는다(`gotolib_push::validate_subscription`).

use std::sync::Arc;

use poem::{
    handler,
    http::StatusCode,
    web::{Data, Json},
    Error, Request,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tyange_cms_api::auth::authorization::current_user;

use crate::{
    covers,
    gotolib_push::{self, PushHandle, PushService, SubscriptionInput},
    models::AppState,
};

/// 구독 본문은 endpoint와 키 두 개라 작다. 이 크기를 넘는 요청은 받지 않는다(`main.rs`의 SizeLimit).
pub const PUSH_BODY_MAX_BYTES: usize = 8 * 1024;

#[derive(Deserialize)]
pub struct UnsubscribeInput {
    endpoint: String,
}

fn db_error(e: sqlx::Error) -> Error {
    eprintln!("Database error: {:?}", e);
    Error::from_string("Database error", StatusCode::INTERNAL_SERVER_ERROR)
}

fn service_of(handle: &PushHandle) -> Result<&Arc<PushService>, Error> {
    handle.0.as_ref().ok_or_else(|| {
        Error::from_string("찜 소장 알림을 지금은 쓸 수 없습니다.", StatusCode::SERVICE_UNAVAILABLE)
    })
}

/// 앱이 `pushManager.subscribe`의 applicationServerKey로 쓰는 공개 키.
#[handler]
pub async fn push_key(handle: Data<&PushHandle>) -> poem::Result<Json<Value>> {
    let service = service_of(&handle)?;
    Ok(Json(json!({ "public_key": service.keys.public_key })))
}

#[handler]
pub async fn push_subscribe(
    req: &Request,
    Json(input): Json<SubscriptionInput>,
    state: Data<&Arc<AppState>>,
    handle: Data<&PushHandle>,
) -> poem::Result<Json<Value>> {
    let service = service_of(&handle)?;
    let user = current_user(req)?;
    gotolib_push::validate_subscription(&input, service.allow_loopback())
        .map_err(|message| Error::from_string(message, StatusCode::BAD_REQUEST))?;
    let agent = req.header("user-agent");
    gotolib_push::upsert_subscription(&state.db, &user.user_id, &input, agent, covers::now())
        .await
        .map_err(db_error)?;
    Ok(Json(json!({ "ok": true })))
}

#[handler]
pub async fn push_unsubscribe(
    req: &Request,
    Json(input): Json<UnsubscribeInput>,
    state: Data<&Arc<AppState>>,
) -> poem::Result<Json<Value>> {
    let user = current_user(req)?;
    let removed = gotolib_push::delete_subscription(&state.db, &user.user_id, &input.endpoint)
        .await
        .map_err(db_error)?;
    Ok(Json(json!({ "removed": removed })))
}

/// 내 구독 기기들에 시험 알림을 보낸다(알림이 실제로 오는지 확인하는 용도).
#[handler]
pub async fn push_test(
    req: &Request,
    state: Data<&Arc<AppState>>,
    handle: Data<&PushHandle>,
) -> poem::Result<Json<Value>> {
    let service = service_of(&handle)?;
    let user = current_user(req)?;
    let payload = gotolib_push::notification_payload("gotolib 알림 테스트", "알림이 잘 오고 있어요.", "/", "gotolib-test");
    let (delivered, removed) = gotolib_push::send_to_user(&state.db, service, &user.user_id, &payload, covers::now()).await;
    Ok(Json(json!({ "delivered": delivered, "removed": removed })))
}
