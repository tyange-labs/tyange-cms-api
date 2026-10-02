// gotolib 전용 계정(`gotolib` 역할)은 CMS 콘텐츠를 쓸 수 없어야 한다. 이 서버는 블로그·CMS와
// 계정 테이블을 공유하므로, 누구나 만들 수 있는 가입 계정이 글·이미지를 올리면 안 된다.
// 실제로 서빙되는 openapi.rs의 PostsApi/ImagesApi를 대상으로 한다(main.rs와 같은 조립).
use std::{env, sync::Arc};

use poem::{
    http::StatusCode,
    post,
    test::{TestClient, TestForm, TestFormField},
    EndpointExt, Route,
};
use poem_openapi::OpenApiService;
use serde_json::json;
use sqlx::SqlitePool;

use crate::{
    db::init_db,
    middlewares::auth_middleware::Auth,
    models::AppState,
    openapi::{ImagesApi, PostsApi},
    routes::upload_image::upload_image,
};
use tyange_cms_api::auth::jwt::Claims;

async fn state() -> Arc<AppState> {
    env::set_var("JWT_ACCESS_SECRET", "test-access-secret");
    env::set_var("UPLOAD_PATH", "/tmp/tyange-cms-gotolib-role-tests");
    let _ = tokio::fs::create_dir_all("/tmp/tyange-cms-gotolib-role-tests").await;
    let db = SqlitePool::connect("sqlite::memory:")
        .await
        .expect("failed to connect sqlite");
    init_db(&db).await.expect("failed to init db");
    Arc::new(AppState::new(db))
}

fn app(state: Arc<AppState>) -> OpenApiService<(PostsApi, ImagesApi), ()> {
    OpenApiService::new(
        (
            PostsApi {
                state: state.clone(),
            },
            ImagesApi { state },
        ),
        "test",
        "1.0",
    )
}

fn token(user_id: &str, role: &str) -> String {
    Claims::create_access_token(user_id, role, b"test-access-secret").expect("token")
}

fn post_body() -> serde_json::Value {
    json!({
        "title": "t",
        "description": "d",
        "published_at": "2026-10-02",
        "tags": [],
        "content": "c",
        "status": "published"
    })
}

fn png_form() -> TestForm {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend_from_slice(b"png payload");
    TestForm::new().field(
        TestFormField::bytes(bytes)
            .name("file")
            .filename("a.png")
            .content_type("image/png"),
    )
}

#[tokio::test]
async fn gotolib_accounts_cannot_write_posts() {
    let cli = TestClient::new(app(state().await));
    let auth = token("gl-abc", "gotolib");

    cli.post("/post/upload")
        .header("Authorization", &auth)
        .body_json(&post_body())
        .send()
        .await
        .assert_status(StatusCode::FORBIDDEN);
    cli.put("/post/update/any-post")
        .header("Authorization", &auth)
        .body_json(&post_body())
        .send()
        .await
        .assert_status(StatusCode::FORBIDDEN);
    cli.delete("/post/delete/any-post")
        .header("Authorization", &auth)
        .send()
        .await
        .assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn gotolib_accounts_cannot_upload_images() {
    let state = state().await;
    let cli = TestClient::new(app(state.clone()));
    cli.post("/upload-image")
        .header("Authorization", token("gl-abc", "gotolib"))
        .multipart(png_form())
        .send()
        .await
        .assert_status(StatusCode::FORBIDDEN);

    // 옛 plain poem 경로(/images/upload)도 같다.
    let legacy = TestClient::new(
        Route::new()
            .at("/images/upload", post(upload_image).with(Auth))
            .data(state),
    );
    legacy
        .post("/images/upload")
        .header("Authorization", token("gl-abc", "gotolib"))
        .multipart(png_form())
        .send()
        .await
        .assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn existing_user_and_admin_roles_can_still_write_posts() {
    let cli = TestClient::new(app(state().await));
    for (id, role) in [("writer@example.com", "user"), ("admin@example.com", "admin")] {
        let response = cli
            .post("/post/upload")
            .header("Authorization", token(id, role))
            .body_json(&post_body())
            .send()
            .await;
        // 권한 확인은 통과해야 한다(이후 DB 쓰기가 어떻게 되든 403은 아니다).
        assert_ne!(response.0.status(), StatusCode::FORBIDDEN, "{role} 역할이 막혔다");
        assert_ne!(response.0.status(), StatusCode::UNAUTHORIZED, "{role}");
    }
}

// ── 비활성 gotolib 계정 정리 ────────────────────────────────────────────────

async fn insert_gotolib_account(db: &SqlitePool, user_id: &str, role: &str, active_days_ago: i64) {
    sqlx::query("INSERT INTO users (user_id, password, user_role, auth_provider) VALUES (?, NULL, ?, 'passkey')")
        .bind(user_id)
        .bind(role)
        .execute(db)
        .await
        .unwrap();
    let when = format!("-{active_days_ago} days");
    sqlx::query(
        "INSERT INTO passkeys (credential_id, user_id, user_handle, label, passkey, created_at) \
         VALUES (?, ?, ?, 'x', '{}', datetime('now', ?))",
    )
    .bind(format!("cred-{user_id}"))
    .bind(user_id)
    .bind(format!("handle-{user_id}"))
    .bind(&when)
    .execute(db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO gotolib_sync (user_id, version, data, updated_at) VALUES (?, 1, '{}', datetime('now', ?))")
        .bind(user_id)
        .bind(&when)
        .execute(db)
        .await
        .unwrap();
}

#[tokio::test]
async fn purge_removes_only_inactive_gotolib_accounts_with_their_data() {
    let state = state().await;
    let db = &state.db;
    insert_gotolib_account(db, "gl-stale", "gotolib", 400).await;
    insert_gotolib_account(db, "gl-fresh", "gotolib", 10).await;
    // 같은 조건(400일)이어도 gotolib 역할이 아니면(블로그 사용자 등) 절대 건드리지 않는다.
    insert_gotolib_account(db, "writer-old", "user", 400).await;

    // 오래된 계정이라도 최근에 동기화했다면(활동이 있으면) 남는다.
    insert_gotolib_account(db, "gl-synced-recently", "gotolib", 400).await;
    sqlx::query("UPDATE gotolib_sync SET updated_at = datetime('now', '-2 days') WHERE user_id = 'gl-synced-recently'")
        .execute(db)
        .await
        .unwrap();

    let removed = crate::db::purge_inactive_gotolib_accounts(db, 365).await.unwrap();
    assert_eq!(removed, 1);

    let users: Vec<String> = sqlx::query_scalar("SELECT user_id FROM users ORDER BY user_id")
        .fetch_all(db)
        .await
        .unwrap();
    assert_eq!(users, vec!["gl-fresh", "gl-synced-recently", "writer-old"]);
    for table in ["passkeys", "gotolib_sync"] {
        let stale: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM {table} WHERE user_id = 'gl-stale'"
        ))
        .fetch_one(db)
        .await
        .unwrap();
        assert_eq!(stale, 0, "{table}에 지운 계정의 데이터가 남았다");
    }
}
