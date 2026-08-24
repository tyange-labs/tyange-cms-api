use std::{env, sync::Arc};

use poem::{get, http::StatusCode, post, test::TestClient, Endpoint, EndpointExt, Route};
use serde_json::json;
use sqlx::SqlitePool;

use crate::{
    db::init_db,
    middlewares::admin_middleware::AdminOnly,
    middlewares::auth_middleware::Auth,
    models::AppState,
    routes::{create_wedding_guest::create_wedding_guest, get_wedding_guests::get_wedding_guests},
};
use tyange_cms_api::auth::jwt::Claims;

async fn create_test_state() -> Arc<AppState> {
    env::set_var("JWT_ACCESS_SECRET", "test-access-secret");
    env::set_var("JWT_REFRESH_SECRET", "test-refresh-secret");

    let db = SqlitePool::connect("sqlite::memory:")
        .await
        .expect("failed to connect sqlite");
    init_db(&db).await.expect("failed to init db");
    Arc::new(AppState::new(db))
}

fn create_wedding_app(state: Arc<AppState>) -> impl Endpoint {
    Route::new()
        .at("/public/wedding/guests", post(create_wedding_guest))
        .at(
            "/admin/wedding/guests",
            get(get_wedding_guests).with(AdminOnly).with(Auth),
        )
        .data(state)
}

fn issue_access_token(user_id: &str, role: &str) -> String {
    Claims::create_access_token(user_id, role, b"test-access-secret")
        .expect("failed to create access token")
}

#[tokio::test]
async fn rsvp_submission_succeeds_without_auth() {
    let client = TestClient::new(create_wedding_app(create_test_state().await));

    let response = client
        .post("/public/wedding/guests")
        .body_json(&json!({ "name": "  김하객  ", "companions": 2 }))
        .send()
        .await;

    response.assert_status_is_ok();
    let body = response.json().await;
    let value = body.value().object();
    assert!(value.get("status").bool());
    assert!(value.get("data").object().get("guest_id").i64() > 0);
}

#[tokio::test]
async fn rsvp_defaults_companions_to_zero_when_omitted() {
    let client = TestClient::new(create_wedding_app(create_test_state().await));

    client
        .post("/public/wedding/guests")
        .body_json(&json!({ "name": "김하객" }))
        .send()
        .await
        .assert_status_is_ok();
}

#[tokio::test]
async fn rsvp_rejects_invalid_payloads() {
    let client = TestClient::new(create_wedding_app(create_test_state().await));

    // 공백만 있는 이름
    client
        .post("/public/wedding/guests")
        .body_json(&json!({ "name": "   ", "companions": 0 }))
        .send()
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    // 21자 이름 (바이트가 아니라 문자 수로 세는지 확인)
    client
        .post("/public/wedding/guests")
        .body_json(&json!({ "name": "가".repeat(21), "companions": 0 }))
        .send()
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    // 20자 이름은 통과해야 한다
    client
        .post("/public/wedding/guests")
        .body_json(&json!({ "name": "가".repeat(20), "companions": 0 }))
        .send()
        .await
        .assert_status_is_ok();

    for companions in [-1, 11] {
        client
            .post("/public/wedding/guests")
            .body_json(&json!({ "name": "김하객", "companions": companions }))
            .send()
            .await
            .assert_status(StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn guest_list_requires_admin() {
    let client = TestClient::new(create_wedding_app(create_test_state().await));

    client
        .get("/admin/wedding/guests")
        .send()
        .await
        .assert_status(StatusCode::UNAUTHORIZED);

    client
        .get("/admin/wedding/guests")
        .header("Authorization", issue_access_token("guest-user", "user"))
        .send()
        .await
        .assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn guest_list_returns_totals_for_admin() {
    let client = TestClient::new(create_wedding_app(create_test_state().await));

    for (name, companions) in [("김하객", 2), ("이하객", 0)] {
        client
            .post("/public/wedding/guests")
            .body_json(&json!({ "name": name, "companions": companions }))
            .send()
            .await
            .assert_status_is_ok();
    }

    let response = client
        .get("/admin/wedding/guests")
        .header("Authorization", issue_access_token("admin-user", "admin"))
        .send()
        .await;

    response.assert_status_is_ok();
    let body = response.json().await;
    let data = body.value().object().get("data").object();

    assert_eq!(data.get("total_guests").i64(), 2);
    // 본인 2명 + 동반 2명
    assert_eq!(data.get("total_attendees").i64(), 4);

    let guests = data.get("guests").array();
    assert_eq!(guests.len(), 2);
    // 이름이 trim되어 저장되는지
    assert!(guests
        .iter()
        .any(|guest| guest.object().get("name").string() == "김하객"));
}
