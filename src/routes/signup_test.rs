use std::{env, sync::Arc};

use bcrypt::verify;
use poem::{get, http::StatusCode, post, put, test::TestClient, Endpoint, EndpointExt, Route};
use serde_json::json;
use sqlx::{query, query_scalar, Row, SqlitePool};

use crate::{
    db::init_db,
    middlewares::admin_middleware::AdminOnly,
    middlewares::auth_middleware::Auth,
    models::AppState,
    routes::{
        add_user::add_user, login::login, me::me, signup::signup,
        update_my_profile::update_my_profile,
    },
};
use tyange_cms_api::auth::jwt::Claims;

async fn create_test_state() -> Arc<AppState> {
    env::set_var("JWT_ACCESS_SECRET", "test-access-secret");

    let db = SqlitePool::connect("sqlite::memory:")
        .await
        .expect("failed to connect sqlite");
    init_db(&db).await.expect("failed to init db");
    Arc::new(AppState::new(db))
}

fn create_auth_app(state: Arc<AppState>) -> impl Endpoint {
    Route::new()
        .at("/signup", post(signup))
        .at("/login", post(login))
        .at("/me", get(me).with(Auth))
        .at("/me/profile", put(update_my_profile).with(Auth))
        .at("/admin/add-user", post(add_user).with(AdminOnly).with(Auth))
        .data(state)
}

fn issue_access_token(user_id: &str, role: &str) -> String {
    Claims::create_access_token(user_id, role, b"test-access-secret")
        .expect("failed to create access token")
}

#[tokio::test]
async fn signup_creates_user_role_account_with_hashed_password() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_auth_app(state.clone()));

    let response = cli
        .post("/signup")
        .body_json(&json!({
            "email": "user@example.com",
            "password": "password123"
        }))
        .send()
        .await;

    response.assert_status_is_ok();

    let stored_hash: String = query_scalar("SELECT password FROM users WHERE user_id = ?")
        .bind("user@example.com")
        .fetch_one(&state.db)
        .await
        .expect("failed to fetch stored password");
    let stored_role: String = query_scalar("SELECT user_role FROM users WHERE user_id = ?")
        .bind("user@example.com")
        .fetch_one(&state.db)
        .await
        .expect("failed to fetch stored role");

    assert_ne!(stored_hash, "password123");
    assert!(verify("password123", &stored_hash).expect("password verify should work"));
    assert_eq!(stored_role, "user");
}

#[tokio::test]
async fn signup_rejects_duplicate_email() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_auth_app(state));

    cli.post("/signup")
        .body_json(&json!({
            "email": "duplicate@example.com",
            "password": "password123"
        }))
        .send()
        .await
        .assert_status_is_ok();

    cli.post("/signup")
        .body_json(&json!({
            "email": "duplicate@example.com",
            "password": "password123"
        }))
        .send()
        .await
        .assert_status(StatusCode::CONFLICT);
}

#[tokio::test]
async fn signup_rejects_invalid_email_and_short_password() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_auth_app(state));

    cli.post("/signup")
        .body_json(&json!({
            "email": "invalid-email",
            "password": "password123"
        }))
        .send()
        .await
        .assert_status(StatusCode::BAD_REQUEST);

    cli.post("/signup")
        .body_json(&json!({
            "email": "valid@example.com",
            "password": "short"
        }))
        .send()
        .await
        .assert_status(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn signup_user_can_login_with_email_as_user_id() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_auth_app(state));

    cli.post("/signup")
        .body_json(&json!({
            "email": "login@example.com",
            "password": "password123"
        }))
        .send()
        .await
        .assert_status_is_ok();

    let response = cli
        .post("/login")
        .body_json(&json!({
            "user_id": "login@example.com",
            "password": "password123"
        }))
        .send()
        .await;

    response.assert_status_is_ok();
    let json = response.json().await;
    json.value().object().get("user_role").assert_string("user");
    json.value().object().get("access_token").assert_not_null();
}

#[tokio::test]
async fn login_and_me_return_current_user() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_auth_app(state));

    cli.post("/signup")
        .body_json(&json!({
            "email": "me@example.com",
            "password": "password123"
        }))
        .send()
        .await
        .assert_status_is_ok();

    let login_response = cli
        .post("/login")
        .body_json(&json!({
            "user_id": "me@example.com",
            "password": "password123"
        }))
        .send()
        .await;

    login_response.assert_status_is_ok();
    let login_json = login_response.json().await;
    let access_token = login_json
        .value()
        .object()
        .get("access_token")
        .string()
        .to_string();

    let me_response = cli
        .get("/me")
        .header("Authorization", access_token)
        .send()
        .await;

    me_response.assert_status_is_ok();
    let me_json = me_response.json().await;
    me_json
        .value()
        .object()
        .get("user_id")
        .assert_string("me@example.com");
    me_json
        .value()
        .object()
        .get("user_role")
        .assert_string("user");
    me_json.value().object().get("display_name").assert_null();
    me_json.value().object().get("avatar_url").assert_null();
    me_json.value().object().get("bio").assert_null();
}

#[tokio::test]
async fn login_accepts_legacy_plaintext_password_and_upgrades_hash() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_auth_app(state.clone()));

    query(
        r#"
        INSERT INTO users (user_id, password, user_role, auth_provider)
        VALUES (?, ?, 'admin', 'local')
        "#,
    )
    .bind("admin-legacy")
    .bind("plaintext-password")
    .execute(&state.db)
    .await
    .expect("failed to insert legacy admin");

    let response = cli
        .post("/login")
        .body_json(&json!({
            "user_id": "admin-legacy",
            "password": "plaintext-password"
        }))
        .send()
        .await;

    response.assert_status_is_ok();

    let stored_password: String = query_scalar("SELECT password FROM users WHERE user_id = ?")
        .bind("admin-legacy")
        .fetch_one(&state.db)
        .await
        .expect("failed to fetch upgraded password");

    assert_ne!(stored_password, "plaintext-password");
    assert!(stored_password.starts_with("$2"));
    assert!(verify("plaintext-password", &stored_password).expect("bcrypt verify should work"));
}

#[tokio::test]
async fn update_my_profile_persists_and_returns_profile_fields() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_auth_app(state.clone()));

    cli.post("/signup")
        .body_json(&json!({
            "email": "profile@example.com",
            "password": "password123"
        }))
        .send()
        .await
        .assert_status_is_ok();

    let response = cli
        .put("/me/profile")
        .header(
            "Authorization",
            issue_access_token("profile@example.com", "user"),
        )
        .body_json(&json!({
            "display_name": "프로필 이름",
            "avatar_url": "https://example.com/profile.png",
            "bio": "소개 문장"
        }))
        .send()
        .await;

    response.assert_status_is_ok();
    let json = response.json().await;
    json.value()
        .object()
        .get("display_name")
        .assert_string("프로필 이름");
    json.value()
        .object()
        .get("avatar_url")
        .assert_string("https://example.com/profile.png");
    json.value().object().get("bio").assert_string("소개 문장");

    let row = query("SELECT display_name, avatar_url, bio FROM users WHERE user_id = ?")
        .bind("profile@example.com")
        .fetch_one(&state.db)
        .await
        .expect("failed to fetch updated profile");

    let display_name: Option<String> = row
        .try_get("display_name")
        .expect("display_name should exist");
    let avatar_url: Option<String> = row.try_get("avatar_url").expect("avatar_url should exist");
    let bio: Option<String> = row.try_get("bio").expect("bio should exist");

    assert_eq!(display_name.as_deref(), Some("프로필 이름"));
    assert_eq!(
        avatar_url.as_deref(),
        Some("https://example.com/profile.png")
    );
    assert_eq!(bio.as_deref(), Some("소개 문장"));
}

#[tokio::test]
async fn update_my_profile_clears_fields_when_blank() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_auth_app(state.clone()));

    query(
        r#"
        INSERT INTO users (user_id, password, user_role, auth_provider, display_name, avatar_url, bio)
        VALUES (?, NULL, 'user', 'local', ?, ?, ?)
        "#,
    )
    .bind("clear@example.com")
    .bind("Before")
    .bind("https://example.com/before.png")
    .bind("before bio")
    .execute(&state.db)
    .await
    .expect("failed to seed profile");

    cli.put("/me/profile")
        .header(
            "Authorization",
            issue_access_token("clear@example.com", "user"),
        )
        .body_json(&json!({
            "display_name": "   ",
            "avatar_url": "",
            "bio": " "
        }))
        .send()
        .await
        .assert_status_is_ok();

    let row = query("SELECT display_name, avatar_url, bio FROM users WHERE user_id = ?")
        .bind("clear@example.com")
        .fetch_one(&state.db)
        .await
        .expect("failed to fetch cleared profile");

    let display_name: Option<String> = row
        .try_get("display_name")
        .expect("display_name should exist");
    let avatar_url: Option<String> = row.try_get("avatar_url").expect("avatar_url should exist");
    let bio: Option<String> = row.try_get("bio").expect("bio should exist");

    assert_eq!(display_name, None);
    assert_eq!(avatar_url, None);
    assert_eq!(bio, None);
}

#[tokio::test]
async fn non_admin_still_cannot_use_admin_add_user() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_auth_app(state));

    let response = cli
        .post("/admin/add-user")
        .header(
            "Authorization",
            issue_access_token("member@example.com", "user"),
        )
        .body_json(&json!({
            "user_id": "created-by-user@example.com",
            "password": "password123",
            "user_role": "user"
        }))
        .send()
        .await;

    response.assert_status(StatusCode::FORBIDDEN);
}
