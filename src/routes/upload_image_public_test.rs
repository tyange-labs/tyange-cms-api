use std::{env, sync::Arc, time::Duration};

use poem::{
    post,
    test::{TestClient, TestForm, TestFormField},
    Endpoint, EndpointExt, Route,
};
use sqlx::{query, query_as, query_scalar, SqlitePool};
use tokio::fs;

use crate::{
    db::init_db,
    middlewares::rate_limit::RateLimit,
    models::AppState,
    routes::upload_image_public::upload_image_public,
    services::image_storage::purge_stale_pending_images,
};

const TEST_UPLOAD_PATH: &str = "/tmp/tyange-cms-upload-image-tests";

const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

fn png_fixture() -> Vec<u8> {
    let mut bytes = PNG_MAGIC.to_vec();
    bytes.extend_from_slice(b"test png payload");
    bytes
}

async fn create_test_state(anonymous_upload_enabled: bool) -> Arc<AppState> {
    env::set_var("UPLOAD_PATH", TEST_UPLOAD_PATH);

    fs::create_dir_all(TEST_UPLOAD_PATH)
        .await
        .expect("failed to create upload dir");

    let db = SqlitePool::connect("sqlite::memory:")
        .await
        .expect("failed to connect sqlite");
    init_db(&db).await.expect("failed to init db");

    let mut state = AppState::new(db);
    state.anonymous_upload_enabled = anonymous_upload_enabled;
    Arc::new(state)
}

fn create_public_upload_app(state: Arc<AppState>) -> impl Endpoint {
    Route::new()
        .at("/public/images/upload", post(upload_image_public))
        .data(state)
}

fn png_form() -> TestForm {
    TestForm::new().field(
        TestFormField::bytes(png_fixture())
            .name("file")
            .filename("anon.png")
            .content_type("image/png"),
    )
}

#[derive(sqlx::FromRow)]
struct StoredPublicImageRow {
    post_id: Option<String>,
    image_type: String,
    status: String,
    uploader_ip: Option<String>,
    file_name: String,
    file_path: String,
}

#[tokio::test]
async fn public_upload_rejected_when_disabled() {
    let state = create_test_state(false).await;
    let cli = TestClient::new(create_public_upload_app(state.clone()));

    let response = cli
        .post("/public/images/upload")
        .multipart(png_form())
        .send()
        .await;

    response.assert_status(poem::http::StatusCode::SERVICE_UNAVAILABLE);

    let saved_count: i64 = query_scalar("SELECT COUNT(*) FROM images")
        .fetch_one(&state.db)
        .await
        .expect("failed to count images");
    assert_eq!(saved_count, 0);
}

#[tokio::test]
async fn public_upload_stores_pending_image_with_uploader_ip() {
    let state = create_test_state(true).await;
    let cli = TestClient::new(create_public_upload_app(state.clone()));

    let response = cli
        .post("/public/images/upload")
        .multipart(png_form())
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

    assert!(image_path.starts_with("/images/public/"));

    let stored: StoredPublicImageRow = query_as(
        "SELECT post_id, image_type, status, uploader_ip, file_name, file_path FROM images LIMIT 1",
    )
    .fetch_one(&state.db)
    .await
    .expect("failed to fetch stored image");

    assert_eq!(stored.post_id, None);
    assert_eq!(stored.image_type, "anonymous");
    assert_eq!(stored.status, "pending");
    assert!(stored.uploader_ip.is_some());
    assert!(stored.file_name.ends_with(".png"));
    assert_eq!(image_path, format!("/images/public/{}", stored.file_name));

    let saved_bytes = fs::read(format!("{}/public/{}", TEST_UPLOAD_PATH, stored.file_name))
        .await
        .expect("failed to read uploaded file");
    assert_eq!(saved_bytes, png_fixture());
}

#[tokio::test]
async fn public_upload_is_rate_limited_per_ip() {
    let state = create_test_state(true).await;
    let app = Route::new()
        .at(
            "/public/images/upload",
            post(upload_image_public).with(RateLimit::new(2, Duration::from_secs(60))),
        )
        .data(state.clone());
    let cli = TestClient::new(app);

    for _ in 0..2 {
        let response = cli
            .post("/public/images/upload")
            .multipart(png_form())
            .send()
            .await;
        response.assert_status_is_ok();
    }

    let response = cli
        .post("/public/images/upload")
        .multipart(png_form())
        .send()
        .await;
    response.assert_status(poem::http::StatusCode::TOO_MANY_REQUESTS);

    let saved_count: i64 = query_scalar("SELECT COUNT(*) FROM images")
        .fetch_one(&state.db)
        .await
        .expect("failed to count images");
    assert_eq!(saved_count, 2);
}

#[tokio::test]
async fn purge_removes_only_stale_pending_images() {
    let state = create_test_state(true).await;

    let stale_pending_path = format!("{}/purge-test-stale-pending.png", TEST_UPLOAD_PATH);
    let fresh_pending_path = format!("{}/purge-test-fresh-pending.png", TEST_UPLOAD_PATH);
    let stale_confirmed_path = format!("{}/purge-test-stale-confirmed.png", TEST_UPLOAD_PATH);
    for path in [
        &stale_pending_path,
        &fresh_pending_path,
        &stale_confirmed_path,
    ] {
        fs::write(path, b"png").await.expect("failed to write file");
    }

    let insert = |image_id: &'static str, file_path: String, status: &'static str, age: &'static str| {
        let db = state.db.clone();
        async move {
            query(
                r#"
                INSERT INTO images
                    (image_id, file_name, origin_name, file_path, mime_type, image_type, status, uploaded_at)
                VALUES (?, ?, ?, ?, 'image/png', 'anonymous', ?, datetime('now', ?))
                "#,
            )
            .bind(image_id)
            .bind(format!("{}.png", image_id))
            .bind("origin.png")
            .bind(file_path)
            .bind(status)
            .bind(age)
            .execute(&db)
            .await
            .expect("failed to insert image row");
        }
    };

    insert("stale-pending", stale_pending_path.clone(), "pending", "-25 hours").await;
    insert("fresh-pending", fresh_pending_path.clone(), "pending", "-1 hours").await;
    insert(
        "stale-confirmed",
        stale_confirmed_path.clone(),
        "confirmed",
        "-25 hours",
    )
    .await;

    let removed = purge_stale_pending_images(&state.db)
        .await
        .expect("purge failed");
    assert_eq!(removed, 1);

    let remaining: Vec<(String,)> = query_as("SELECT image_id FROM images ORDER BY image_id")
        .fetch_all(&state.db)
        .await
        .expect("failed to fetch remaining");
    let remaining_ids: Vec<&str> = remaining.iter().map(|(id,)| id.as_str()).collect();
    assert_eq!(remaining_ids, vec!["fresh-pending", "stale-confirmed"]);

    assert!(fs::metadata(&stale_pending_path).await.is_err());
    assert!(fs::metadata(&fresh_pending_path).await.is_ok());
    assert!(fs::metadata(&stale_confirmed_path).await.is_ok());
}
