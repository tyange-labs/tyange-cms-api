use std::{env, sync::Arc};

use poem::{get, test::TestClient, Endpoint, EndpointExt, Route};
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::{
    db::init_db,
    middlewares::auth_middleware::Auth,
    models::AppState,
    routes::{
        add_user::create_user,
        gotolib_sync::{get_sync, put_sync, MAX_SYNC_BYTES},
    },
};
use tyange_cms_api::auth::jwt::Claims;

async fn test_state() -> Arc<AppState> {
    env::set_var("JWT_ACCESS_SECRET", "test-access-secret");
    let db = SqlitePool::connect("sqlite::memory:")
        .await
        .expect("failed to connect sqlite");
    init_db(&db).await.expect("failed to init db");
    let state = Arc::new(AppState::new(db));
    for user in ["me@example.com", "other@example.com"] {
        create_user(&state.db, user, "password123", "user")
            .await
            .expect("create user");
    }
    state
}

fn app(state: Arc<AppState>) -> impl Endpoint {
    Route::new()
        .at(
            "/gotolib/sync",
            get(get_sync.with(Auth)).put(put_sync.with(Auth)),
        )
        .data(state)
}

fn token(user_id: &str) -> String {
    Claims::create_access_token(user_id, "user", b"test-access-secret").expect("token")
}

async fn body(response: poem::test::TestResponse) -> Value {
    serde_json::from_str(&response.0.into_body().into_string().await.unwrap()).unwrap()
}

#[tokio::test]
async fn sync_requires_authentication() {
    let cli = TestClient::new(app(test_state().await));
    cli.get("/gotolib/sync")
        .send()
        .await
        .assert_status(poem::http::StatusCode::UNAUTHORIZED);
    cli.put("/gotolib/sync")
        .body_json(&json!({ "base_version": 0, "data": {} }))
        .send()
        .await
        .assert_status(poem::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn first_get_is_empty_then_put_creates_version_1() {
    let cli = TestClient::new(app(test_state().await));
    let auth = token("me@example.com");

    let empty = body(cli.get("/gotolib/sync").header("Authorization", &auth).send().await).await;
    assert_eq!(empty["version"], 0);
    assert!(empty["data"].is_null());

    let put = cli
        .put("/gotolib/sync")
        .header("Authorization", &auth)
        .body_json(&json!({ "base_version": 0, "data": { "books": [1, 2] } }))
        .send()
        .await;
    put.assert_status_is_ok();
    let put = body(put).await;
    assert_eq!(put["version"], 1);
    assert_eq!(put["data"]["books"], json!([1, 2]));

    let got = body(cli.get("/gotolib/sync").header("Authorization", &auth).send().await).await;
    assert_eq!(got["version"], 1);
    assert_eq!(got["data"]["books"], json!([1, 2]));
    assert!(got["updated_at"].is_string());
}

#[tokio::test]
async fn put_with_matching_version_increments_and_stale_version_conflicts() {
    let cli = TestClient::new(app(test_state().await));
    let auth = token("me@example.com");
    let put = |base: i64, marker: &str| {
        cli.put("/gotolib/sync")
            .header("Authorization", &auth)
            .body_json(&json!({ "base_version": base, "data": { "marker": marker } }))
            .send()
    };

    put(0, "a").await.assert_status_is_ok();
    let second = put(1, "b").await;
    second.assert_status_is_ok();
    assert_eq!(body(second).await["version"], 2);

    // 두 번째 기기가 version 1을 기준으로 올리면(이미 2) 거부되고 최신 문서를 돌려준다.
    let stale = put(1, "c").await;
    stale.assert_status(poem::http::StatusCode::CONFLICT);
    let stale = body(stale).await;
    assert_eq!(stale["version"], 2);
    assert_eq!(stale["data"]["marker"], "b");

    // 처음 올리려는 기기(0)도 이미 문서가 있으면 충돌.
    put(0, "d")
        .await
        .assert_status(poem::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn each_user_has_an_isolated_document() {
    let cli = TestClient::new(app(test_state().await));
    cli.put("/gotolib/sync")
        .header("Authorization", token("me@example.com"))
        .body_json(&json!({ "base_version": 0, "data": { "owner": "me" } }))
        .send()
        .await
        .assert_status_is_ok();

    let other = body(
        cli.get("/gotolib/sync")
            .header("Authorization", token("other@example.com"))
            .send()
            .await,
    )
    .await;
    assert_eq!(other["version"], 0);
    assert!(other["data"].is_null());
}

#[tokio::test]
async fn put_rejects_non_object_negative_version_and_oversized_documents() {
    let cli = TestClient::new(app(test_state().await));
    let auth = token("me@example.com");

    for payload in [
        json!({ "base_version": 0, "data": [1, 2, 3] }),
        json!({ "base_version": 0, "data": "text" }),
        json!({ "base_version": -1, "data": {} }),
    ] {
        cli.put("/gotolib/sync")
            .header("Authorization", &auth)
            .body_json(&payload)
            .send()
            .await
            .assert_status(poem::http::StatusCode::BAD_REQUEST);
    }

    let huge = "x".repeat(MAX_SYNC_BYTES + 1);
    cli.put("/gotolib/sync")
        .header("Authorization", &auth)
        .body_json(&json!({ "base_version": 0, "data": { "blob": huge } }))
        .send()
        .await
        .assert_status(poem::http::StatusCode::PAYLOAD_TOO_LARGE);

    // 거부된 요청은 아무것도 저장하지 않는다.
    let got = body(cli.get("/gotolib/sync").header("Authorization", &auth).send().await).await;
    assert_eq!(got["version"], 0);
}
