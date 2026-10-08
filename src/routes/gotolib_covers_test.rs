use std::{env, fs, sync::Arc};

use poem::{
    endpoint::StaticFilesEndpoint, get, http::StatusCode, middleware::SetHeader, test::TestClient, Endpoint,
    EndpointExt, Route,
};
use serde_json::Value;
use sqlx::{query, SqlitePool};

use crate::{
    covers::{self, tests::isbn},
    db::init_db,
    models::AppState,
    routes::gotolib_covers::{get_covers, IMAGE_PATH_PREFIX, MAX_ISBNS_PER_REQUEST},
};

async fn test_state() -> Arc<AppState> {
    env::set_var("JWT_ACCESS_SECRET", "test-access-secret");
    let db = SqlitePool::connect("sqlite::memory:").await.expect("sqlite");
    init_db(&db).await.expect("init db");
    Arc::new(AppState::new(db))
}

fn app(state: Arc<AppState>) -> impl Endpoint {
    Route::new().at("/gotolib/covers", get(get_covers)).data(state)
}

async fn json(response: poem::test::TestResponse) -> Value {
    serde_json::from_str(&response.0.into_body().into_string().await.unwrap()).unwrap()
}

async fn set_row(state: &AppState, isbn: &str, status: &str) {
    query(
        "INSERT OR REPLACE INTO book_covers (isbn13, status, attempts, next_try_at, created_at) VALUES (?, ?, 0, 0, 1)",
    )
    .bind(isbn)
    .bind(status)
    .execute(&state.db)
    .await
    .unwrap();
}

async fn row_count(state: &AppState) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM book_covers").fetch_one(&state.db).await.unwrap()
}

#[tokio::test]
async fn rejects_missing_empty_and_malformed_isbns() {
    let cli = TestClient::new(app(test_state().await));
    for path in [
        "/gotolib/covers",
        "/gotolib/covers?isbn=",
        "/gotolib/covers?isbn=abc",
        "/gotolib/covers?isbn=9788934972465", // 검사 숫자 틀림
        "/gotolib/covers?isbn=../etc/passwd",
    ] {
        cli.get(path).send().await.assert_status(StatusCode::BAD_REQUEST);
    }
    // 하나라도 잘못되면 요청 전체를 거부하고 아무것도 등록하지 않는다
    let state = test_state().await;
    let cli = TestClient::new(app(state.clone()));
    let good = isbn(1);
    cli.get(format!("/gotolib/covers?isbn={good},bad"))
        .send()
        .await
        .assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(row_count(&state).await, 0);
}

#[tokio::test]
async fn rejects_more_than_the_per_request_limit() {
    let cli = TestClient::new(app(test_state().await));
    let many: Vec<String> = (0..=MAX_ISBNS_PER_REQUEST as u32).map(|n| isbn(n + 1)).collect();
    cli.get(format!("/gotolib/covers?isbn={}", many.join(",")))
        .send()
        .await
        .assert_status(StatusCode::BAD_REQUEST);
    let ok: Vec<String> = (0..MAX_ISBNS_PER_REQUEST as u32).map(|n| isbn(n + 1)).collect();
    cli.get(format!("/gotolib/covers?isbn={}", ok.join(",")))
        .send()
        .await
        .assert_status_is_ok();
}

#[tokio::test]
async fn unknown_isbn_is_pending_and_registered_once_even_when_repeated() {
    let state = test_state().await;
    let cli = TestClient::new(app(state.clone()));
    let id = isbn(2);

    let res = cli.get(format!("/gotolib/covers?isbn={id},{id},{id}")).send().await;
    res.assert_status_is_ok();
    let body = json(res).await;
    assert_eq!(body["covers"][&id]["status"], "pending");
    assert!(body["covers"][&id]["url"].is_null());
    assert_eq!(body["covers"].as_object().unwrap().len(), 1);
    assert_eq!(row_count(&state).await, 1);

    // 다시 물어도 행이 늘지 않는다
    cli.get(format!("/gotolib/covers?isbn={id}")).send().await.assert_status_is_ok();
    assert_eq!(row_count(&state).await, 1);
}

#[tokio::test]
async fn reports_ok_with_a_path_and_hides_internal_states() {
    let state = test_state().await;
    let (done, none, failing, waiting) = (isbn(3), isbn(4), isbn(5), isbn(6));
    set_row(&state, &done, covers::STATUS_OK).await;
    set_row(&state, &none, covers::STATUS_MISSING).await;
    query("UPDATE book_covers SET next_try_at = 9999999999 WHERE isbn13 = ?")
        .bind(&none)
        .execute(&state.db)
        .await
        .unwrap();
    set_row(&state, &failing, covers::STATUS_ERROR).await;
    set_row(&state, &waiting, covers::STATUS_PENDING).await;

    let cli = TestClient::new(app(state));
    let res = cli
        .get(format!("/gotolib/covers?isbn={done},{none},{failing},{waiting}"))
        .send()
        .await;
    res.assert_status_is_ok();
    res.assert_header("Cache-Control", "no-cache");
    let body = json(res).await;
    assert_eq!(body["covers"][&done]["status"], "ok");
    assert_eq!(body["covers"][&done]["url"], format!("{IMAGE_PATH_PREFIX}/{done}.jpg"));
    assert_eq!(body["covers"][&none]["status"], "missing");
    // error는 앱이 알 필요 없는 내부 사정이라 pending으로 보인다
    assert_eq!(body["covers"][&failing]["status"], "pending");
    assert_eq!(body["covers"][&waiting]["status"], "pending");
}

#[tokio::test]
async fn old_missing_rows_are_queued_again() {
    let state = test_state().await;
    let id = isbn(7);
    set_row(&state, &id, covers::STATUS_MISSING).await; // next_try_at = 0 → 이미 오래됨
    let cli = TestClient::new(app(state.clone()));
    let body = json(cli.get(format!("/gotolib/covers?isbn={id}")).send().await).await;
    assert_eq!(body["covers"][&id]["status"], "pending");
    assert_eq!(covers::statuses(&state.db, std::slice::from_ref(&id)).await.unwrap()[&id], covers::STATUS_PENDING);
}

#[tokio::test]
async fn full_queue_stops_accepting_new_isbns_but_still_answers() {
    let state = test_state().await;
    for n in 0..covers::DEFAULT_QUEUE_MAX as u32 {
        set_row(&state, &isbn(100 + n), covers::STATUS_PENDING).await;
    }
    let cli = TestClient::new(app(state.clone()));
    let fresh = isbn(9);
    let body = json(cli.get(format!("/gotolib/covers?isbn={fresh}")).send().await).await;
    assert_eq!(body["covers"][&fresh]["status"], "pending");
    assert_eq!(row_count(&state).await, covers::DEFAULT_QUEUE_MAX); // 등록되지 않았다
    // 이미 아는 ISBN은 큐가 가득 차도 답한다
    let known = isbn(100);
    let body = json(cli.get(format!("/gotolib/covers?isbn={known}")).send().await).await;
    assert_eq!(body["covers"][&known]["status"], "pending");
}

#[tokio::test]
async fn query_never_reaches_aladin() {
    // 핸들러는 HTTP 클라이언트를 갖지 않는다: 알라딘이 없어도(여기서는 아무것도 띄우지 않았다) 정상 응답한다.
    let cli = TestClient::new(app(test_state().await));
    cli.get(format!("/gotolib/covers?isbn={}", isbn(10))).send().await.assert_status_is_ok();
}

// ── 이미지 제공 ──────────────────────────────────────────────────────────────

fn image_app(dir: &std::path::Path) -> impl Endpoint {
    Route::new().nest(
        IMAGE_PATH_PREFIX,
        StaticFilesEndpoint::new(dir).with(SetHeader::new().overriding("Cache-Control", "public, max-age=2592000")),
    )
}

#[tokio::test]
async fn serves_stored_thumbnails_with_long_cache_and_blocks_path_escape() {
    let dir = std::env::temp_dir().join(format!("covers-serve-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    let id = isbn(11);
    fs::write(dir.join(format!("{id}.jpg")), b"\xFF\xD8fake-jpeg").unwrap();
    // 폴더 바깥에 둔 비밀 파일: 어떤 경로로도 나오면 안 된다
    let secret = dir.parent().unwrap().join(format!("secret-{}.txt", uuid::Uuid::new_v4()));
    fs::write(&secret, "top secret").unwrap();
    let secret_name = secret.file_name().unwrap().to_string_lossy().to_string();

    let cli = TestClient::new(image_app(&dir));

    let ok = cli.get(format!("{IMAGE_PATH_PREFIX}/{id}.jpg")).send().await;
    ok.assert_status_is_ok();
    ok.assert_header("Cache-Control", "public, max-age=2592000");

    cli.get(format!("{IMAGE_PATH_PREFIX}/{}.jpg", isbn(12)))
        .send()
        .await
        .assert_status(StatusCode::NOT_FOUND);

    for attempt in [
        format!("{IMAGE_PATH_PREFIX}/../{secret_name}"),
        format!("{IMAGE_PATH_PREFIX}/%2e%2e/{secret_name}"),
        format!("{IMAGE_PATH_PREFIX}/..%2f{secret_name}"),
        format!("{IMAGE_PATH_PREFIX}/%2e%2e%2f{secret_name}"),
        format!("{IMAGE_PATH_PREFIX}//{}", secret.display()),
    ] {
        let res = cli.get(&attempt).send().await;
        let status = res.0.status();
        let body = res.0.into_body().into_string().await.unwrap_or_default();
        assert!(!body.contains("top secret"), "{attempt} 가 폴더 밖 파일을 내보냈다 ({status})");
    }

    // 폴더 목록은 보이지 않는다
    let listing = cli.get(format!("{IMAGE_PATH_PREFIX}/")).send().await;
    let status = listing.0.status();
    let body = listing.0.into_body().into_string().await.unwrap_or_default();
    assert!(!body.contains(&id), "폴더 목록이 노출됐다 ({status})");

    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_file(&secret);
}
