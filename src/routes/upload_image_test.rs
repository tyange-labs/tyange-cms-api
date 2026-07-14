use std::{env, sync::Arc};

use poem::{
    post,
    test::{TestClient, TestForm, TestFormField},
    Endpoint, EndpointExt, Route,
};
use sqlx::{query_as, query_scalar, SqlitePool};
use tokio::fs;

use crate::{
    db::init_db, middlewares::auth_middleware::Auth, models::AppState,
    routes::upload_image::upload_image,
};
use tyange_cms_api::auth::jwt::Claims;

const TEST_UPLOAD_PATH: &str = "/tmp/tyange-cms-upload-image-tests";

const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

fn png_fixture() -> Vec<u8> {
    let mut bytes = PNG_MAGIC.to_vec();
    bytes.extend_from_slice(b"test png payload");
    bytes
}

#[derive(sqlx::FromRow)]
struct StoredImageRow {
    post_id: Option<String>,
    origin_name: String,
    mime_type: String,
    image_type: String,
    file_name: String,
}

async fn create_test_state() -> Arc<AppState> {
    env::set_var("JWT_ACCESS_SECRET", "test-access-secret");
    env::set_var("UPLOAD_PATH", TEST_UPLOAD_PATH);

    // 테스트가 병렬로 실행되므로 디렉토리를 비우지 않는다.
    // (다른 테스트가 방금 쓴 파일을 지울 수 있음 — 파일명은 UUID라 충돌 없음)
    fs::create_dir_all(TEST_UPLOAD_PATH)
        .await
        .expect("failed to create upload dir");

    let db = SqlitePool::connect("sqlite::memory:")
        .await
        .expect("failed to connect sqlite");
    init_db(&db).await.expect("failed to init db");

    Arc::new(AppState::new(db))
}

fn create_upload_app(state: Arc<AppState>) -> impl Endpoint {
    Route::new()
        .at("/upload-image", post(upload_image).with(Auth))
        .at("/images/upload", post(upload_image).with(Auth))
        .data(state)
}

fn issue_access_token(user_id: &str, role: &str) -> String {
    Claims::create_access_token(user_id, role, b"test-access-secret")
        .expect("failed to create access token")
}

#[tokio::test]
async fn upload_image_accepts_image_without_post_id() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_upload_app(state.clone()));

    let png_bytes = png_fixture();

    let response = cli
        .post("/images/upload")
        .header("Authorization", issue_access_token("writer-1", "user"))
        .multipart(
            TestForm::new().field(
                TestFormField::bytes(png_bytes.clone())
                    .name("file")
                    .filename("avatar.png")
                    .content_type("image/png"),
            ),
        )
        .send()
        .await;

    response.assert_status_is_ok();
    let json = response.json().await;
    let image_path = json
        .value()
        .object()
        .get("data")
        .object()
        .get("image_path")
        .string()
        .to_string();

    assert!(image_path.starts_with("/images/"));

    let stored: StoredImageRow = query_as(
        "SELECT post_id, origin_name, mime_type, image_type, file_name FROM images LIMIT 1",
    )
    .fetch_one(&state.db)
    .await
    .expect("failed to fetch stored image");

    assert_eq!(stored.post_id, None);
    assert_eq!(stored.origin_name, "avatar.png");
    assert_eq!(stored.mime_type, "image/png");
    assert_eq!(stored.image_type, "in_post");
    assert_eq!(image_path, format!("/images/{}", stored.file_name));

    assert!(stored.file_name.ends_with(".png"));

    let saved_bytes = fs::read(format!("{}/{}", TEST_UPLOAD_PATH, stored.file_name))
        .await
        .expect("failed to read uploaded file");
    assert_eq!(saved_bytes, png_bytes);
}

#[tokio::test]
async fn upload_image_rejects_forged_content_type_header() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_upload_app(state.clone()));

    // Content-Type 헤더는 image/png지만 실제 바이트는 이미지가 아닌 경우
    let response = cli
        .post("/images/upload")
        .header("Authorization", issue_access_token("writer-1", "user"))
        .multipart(
            TestForm::new().field(
                TestFormField::bytes(b"<html><script>alert(1)</script></html>")
                    .name("file")
                    .filename("evil.png")
                    .content_type("image/png"),
            ),
        )
        .send()
        .await;

    response.assert_status(poem::http::StatusCode::BAD_REQUEST);

    let saved_count: i64 = query_scalar("SELECT COUNT(*) FROM images")
        .fetch_one(&state.db)
        .await
        .expect("failed to count images");
    assert_eq!(saved_count, 0);
}

#[tokio::test]
async fn upload_image_derives_extension_from_detected_format_not_filename() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_upload_app(state.clone()));

    // 파일명이 evil.html이어도 실제 PNG 바이트면 .png로 저장되어야 한다
    let response = cli
        .post("/images/upload")
        .header("Authorization", issue_access_token("writer-1", "user"))
        .multipart(
            TestForm::new().field(
                TestFormField::bytes(png_fixture())
                    .name("file")
                    .filename("evil.html")
                    .content_type("image/png"),
            ),
        )
        .send()
        .await;

    response.assert_status_is_ok();

    let stored: StoredImageRow = query_as(
        "SELECT post_id, origin_name, mime_type, image_type, file_name FROM images LIMIT 1",
    )
    .fetch_one(&state.db)
    .await
    .expect("failed to fetch stored image");

    assert!(stored.file_name.ends_with(".png"));
    assert!(!stored.file_name.contains("html"));
    assert_eq!(stored.mime_type, "image/png");
    assert_eq!(stored.origin_name, "evil.html");
}

#[tokio::test]
async fn upload_image_rejects_svg() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_upload_app(state.clone()));

    let response = cli
        .post("/images/upload")
        .header("Authorization", issue_access_token("writer-1", "user"))
        .multipart(
            TestForm::new().field(
                TestFormField::bytes(b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>")
                    .name("file")
                    .filename("image.svg")
                    .content_type("image/svg+xml"),
            ),
        )
        .send()
        .await;

    response.assert_status(poem::http::StatusCode::BAD_REQUEST);

    let saved_count: i64 = query_scalar("SELECT COUNT(*) FROM images")
        .fetch_one(&state.db)
        .await
        .expect("failed to count images");
    assert_eq!(saved_count, 0);
}

#[tokio::test]
async fn upload_image_rejects_file_over_max_bytes() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_upload_app(state.clone()));

    // 정책 상한(10MB)을 1바이트 초과하는 PNG
    let mut big_bytes = PNG_MAGIC.to_vec();
    big_bytes.resize(10 * 1024 * 1024 + 1, 0);

    let response = cli
        .post("/images/upload")
        .header("Authorization", issue_access_token("writer-1", "user"))
        .multipart(
            TestForm::new().field(
                TestFormField::bytes(big_bytes)
                    .name("file")
                    .filename("big.png")
                    .content_type("image/png"),
            ),
        )
        .send()
        .await;

    response.assert_status(poem::http::StatusCode::PAYLOAD_TOO_LARGE);

    let saved_count: i64 = query_scalar("SELECT COUNT(*) FROM images")
        .fetch_one(&state.db)
        .await
        .expect("failed to count images");
    assert_eq!(saved_count, 0);
}

#[tokio::test]
async fn upload_image_rejects_non_image_content_type() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_upload_app(state.clone()));

    let response = cli
        .post("/upload-image")
        .header("Authorization", issue_access_token("writer-1", "user"))
        .multipart(
            TestForm::new().field(
                TestFormField::bytes(b"plain text")
                    .name("file")
                    .filename("note.txt")
                    .content_type("text/plain"),
            ),
        )
        .send()
        .await;

    response.assert_status(poem::http::StatusCode::BAD_REQUEST);

    let saved_count: i64 = query_scalar("SELECT COUNT(*) FROM images")
        .fetch_one(&state.db)
        .await
        .expect("failed to count images");
    assert_eq!(saved_count, 0);
}
