use std::{env, sync::Arc};

use poem::{get, http::StatusCode, test::TestClient, Endpoint, EndpointExt, Route};
use serde_json::{json, Value};
use sqlx::{query, SqlitePool};

use crate::{db::init_db, models::AppState, routes::gotolib_korean_novels::get_korean_novels};

async fn test_state() -> Arc<AppState> {
    env::set_var("JWT_ACCESS_SECRET", "test-access-secret");
    let db = SqlitePool::connect("sqlite::memory:").await.expect("sqlite");
    init_db(&db).await.expect("init db");
    Arc::new(AppState::new(db))
}

fn app(state: Arc<AppState>) -> impl Endpoint {
    Route::new().at("/gotolib/korean-novels", get(get_korean_novels)).data(state)
}

async fn insert(state: &AppState, goods_no: i64, form: &str, title: &str, month: &str, isbn: Option<&str>) {
    query(
        "INSERT INTO yes24_books (goods_no, form, title, author, publisher, pub_month, isbn13, status, created_at)
         VALUES (?, ?, ?, '작가 저', '창비', ?, ?, 'ok', 1)",
    )
    .bind(goods_no)
    .bind(form)
    .bind(title)
    .bind(month)
    .bind(isbn)
    .execute(&state.db)
    .await
    .unwrap();
}

#[tokio::test]
async fn rejects_missing_or_malformed_year() {
    let cli = TestClient::new(app(test_state().await));
    for path in ["/gotolib/korean-novels", "/gotolib/korean-novels?year=26", "/gotolib/korean-novels?year=20x6"] {
        cli.get(path).send().await.assert_status(StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn returns_only_that_years_novels_with_isbn_newest_first() {
    let state = test_state().await;
    insert(&state, 1, "장편", "작년 책", "2025-12", Some("9788936431815")).await;
    insert(&state, 2, "장편", "올해 3월", "2026-03", Some("9791167903952")).await;
    insert(&state, 3, "장편", "올해 11월", "2026-11", Some("9788936440008")).await;
    insert(&state, 4, "단편", "소설집", "2026-05", Some("9788936431808")).await;
    insert(&state, 5, "장편", "ISBN 아직", "2026-06", None).await;
    // 같은 ISBN이 다른 상품 번호로 또 있으면 최근 상품 하나만
    insert(&state, 6, "장편", "올해 3월(재등록)", "2026-03", Some("9791167903952")).await;

    let cli = TestClient::new(app(state));
    let response = cli.get("/gotolib/korean-novels?year=2026").send().await;
    response.assert_status_is_ok();
    let body: Value = serde_json::from_str(&response.0.into_body().into_string().await.unwrap()).unwrap();
    assert_eq!(body["year"], "2026");
    let titles: Vec<&str> = body["books"].as_array().unwrap().iter().map(|b| b["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["올해 11월", "올해 3월(재등록)"]);
    assert_eq!(
        body["books"][0],
        json!({
            "isbn13": "9788936440008",
            "goodsNo": 3,
            "title": "올해 11월",
            "author": "작가 저",
            "publisher": "창비",
            "pubMonth": "2026-11",
        })
    );
}
