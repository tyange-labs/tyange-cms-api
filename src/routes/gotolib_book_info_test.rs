use std::{env, sync::Arc};

use poem::{get, http::StatusCode, test::TestClient, Endpoint, EndpointExt, Route};
use serde_json::Value;
use sqlx::{query, SqlitePool};

use crate::{
    book_info::{self, tests::isbn},
    db::init_db,
    models::AppState,
    routes::gotolib_book_info::{get_book_info, MAX_ISBNS_PER_REQUEST},
};

async fn test_state() -> Arc<AppState> {
    env::set_var("JWT_ACCESS_SECRET", "test-access-secret");
    let db = SqlitePool::connect("sqlite::memory:").await.expect("sqlite");
    init_db(&db).await.expect("init db");
    Arc::new(AppState::new(db))
}

fn app(state: Arc<AppState>) -> impl Endpoint {
    Route::new().at("/gotolib/book-info", get(get_book_info)).data(state)
}

async fn json(response: poem::test::TestResponse) -> Value {
    serde_json::from_str(&response.0.into_body().into_string().await.unwrap()).unwrap()
}

async fn set_row(
    state: &AppState,
    isbn: &str,
    status: &str,
    category: Option<&str>,
    form_hint: Option<&str>,
    pages: Option<i64>,
) {
    query(
        "INSERT OR REPLACE INTO book_covers (isbn13, status, category_path, form_hint, pages, attempts, next_try_at, created_at)
         VALUES (?, ?, ?, ?, ?, 0, 0, 1)",
    )
    .bind(isbn)
    .bind(status)
    .bind(category)
    .bind(form_hint)
    .bind(pages)
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
        "/gotolib/book-info",
        "/gotolib/book-info?isbn=",
        "/gotolib/book-info?isbn=abc",
        "/gotolib/book-info?isbn=9788934972465", // 검사 숫자 틀림
    ] {
        cli.get(path).send().await.assert_status(StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn rejects_more_than_the_per_request_limit() {
    let cli = TestClient::new(app(test_state().await));
    let many: Vec<String> = (0..=MAX_ISBNS_PER_REQUEST as u32).map(|n| isbn(n + 1)).collect();
    cli.get(format!("/gotolib/book-info?isbn={}", many.join(",")))
        .send()
        .await
        .assert_status(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn unknown_isbn_is_pending_and_registered_in_the_same_queue_as_covers() {
    let state = test_state().await;
    let cli = TestClient::new(app(state.clone()));
    let id = isbn(1);

    let body = json(cli.get(format!("/gotolib/book-info?isbn={id}")).send().await).await;
    assert_eq!(body["info"][&id]["status"], "pending");
    assert!(body["info"][&id]["category"].is_null());
    // 표지 수집과 같은 행이라 book_info::statuses에서도 보인다
    assert_eq!(book_info::statuses(&state.db, std::slice::from_ref(&id)).await.unwrap()[&id], book_info::STATUS_PENDING);
    assert_eq!(row_count(&state).await, 1);
}

#[tokio::test]
async fn reports_the_fields_collected_alongside_the_cover() {
    let state = test_state().await;
    let done = isbn(2);
    set_row(&state, &done, book_info::STATUS_OK, Some("국내도서 > 소설/시/희곡 > 한국소설"), Some("장편소설"), Some(232)).await;

    let cli = TestClient::new(app(state));
    let res = cli.get(format!("/gotolib/book-info?isbn={done}")).send().await;
    res.assert_status_is_ok();
    res.assert_header("Cache-Control", "no-cache");
    let body = json(res).await;
    assert_eq!(body["info"][&done]["status"], "ok");
    assert_eq!(body["info"][&done]["category"], "국내도서 > 소설/시/희곡 > 한국소설");
    assert_eq!(body["info"][&done]["categories"], serde_json::json!(["국내도서 > 소설/시/희곡 > 한국소설"]));
    assert_eq!(body["info"][&done]["formHint"], "장편소설");
    assert_eq!(body["info"][&done]["pages"], 232);
}

#[tokio::test]
async fn yes24_form_is_reported_even_while_the_aladin_row_is_pending() {
    let state = test_state().await;
    let id = isbn(9);
    query("INSERT INTO yes24_books (goods_no, form, title, isbn13, status, created_at) VALUES (1, '장편', 't', ?, 'ok', 1)")
        .bind(&id)
        .execute(&state.db)
        .await
        .unwrap();

    let cli = TestClient::new(app(state));
    let body = json(cli.get(format!("/gotolib/book-info?isbn={id},{}", isbn(10))).send().await).await;
    assert_eq!(body["info"][&id]["status"], "pending");
    assert_eq!(body["info"][&id]["yes24Form"], "장편");
    assert!(body["info"][isbn(10)]["yes24Form"].is_null());
}

#[tokio::test]
async fn details_win_over_the_old_columns_and_carry_every_field() {
    let state = test_state().await;
    let id = isbn(8);
    set_row(&state, &id, book_info::STATUS_OK, Some("옛 분류"), None, Some(1)).await;
    query(
        r#"INSERT INTO book_details (isbn13, title, subtitle, people, publisher, pub_date, original_title, has_ebook,
             pages, size, weight, extra_info, categories, form_hint, fetched_at)
           VALUES (?, '데이터베이스 설계, 이렇게 하면 된다', '제2판', '[{"name":"미크","role":"지은이"},{"name":"윤인성","role":"옮긴이"}]',
             '제이펍', '2025-10-30', '達人に學ぶDB設計徹底指南書', 1, 356, '170*225mm', '676g', '["양장본"]',
             '["국내도서 > 컴퓨터/모바일 > 컴퓨터 공학 > 데이터베이스 개론","국내도서 > 추천도서"]', NULL, 1)"#,
    )
    .bind(&id)
    .execute(&state.db)
    .await
    .unwrap();

    let cli = TestClient::new(app(state));
    let body = json(cli.get(format!("/gotolib/book-info?isbn={id}")).send().await).await;
    let e = &body["info"][&id];
    assert_eq!(e["title"], "데이터베이스 설계, 이렇게 하면 된다");
    assert_eq!(e["subtitle"], "제2판");
    assert_eq!(e["people"], serde_json::json!([{"name":"미크","role":"지은이"},{"name":"윤인성","role":"옮긴이"}]));
    assert_eq!(e["publisher"], "제이펍");
    assert_eq!(e["pubDate"], "2025-10-30");
    assert_eq!(e["originalTitle"], "達人に學ぶDB設計徹底指南書");
    assert_eq!(e["hasEbook"], true);
    assert_eq!(e["pages"], 356);
    assert_eq!(e["size"], "170*225mm");
    assert_eq!(e["weight"], "676g");
    assert_eq!(e["extraInfo"], serde_json::json!(["양장본"]));
    assert_eq!(e["categories"][1], "국내도서 > 추천도서");
    assert_eq!(e["category"], "국내도서 > 컴퓨터/모바일 > 컴퓨터 공학 > 데이터베이스 개론");
    assert!(e["formHint"].is_null());
}

#[tokio::test]
async fn ok_status_without_any_field_omits_them_rather_than_erroring() {
    // 전자책처럼 표지는 찾았지만(ok) 쪽수가 없는 경우.
    let state = test_state().await;
    let id = isbn(3);
    set_row(&state, &id, book_info::STATUS_OK, Some("전자책 > 소설/시/희곡 > 한국소설"), None, None).await;

    let cli = TestClient::new(app(state));
    let body = json(cli.get(format!("/gotolib/book-info?isbn={id}")).send().await).await;
    assert_eq!(body["info"][&id]["status"], "ok");
    assert_eq!(body["info"][&id]["category"], "전자책 > 소설/시/희곡 > 한국소설");
    assert!(body["info"][&id]["formHint"].is_null());
    assert!(body["info"][&id]["pages"].is_null());
}

#[tokio::test]
async fn missing_status_still_reports_any_fields_it_found() {
    // 표지는 없지만(missing) 페이지 자체는 정상이어서 분류는 뽑혔을 수 있다.
    let state = test_state().await;
    let id = isbn(4);
    set_row(&state, &id, book_info::STATUS_MISSING, Some("국내도서 > 소설/시/희곡 > 한국소설"), Some("소설집"), Some(180)).await;
    query("UPDATE book_covers SET next_try_at = 9999999999 WHERE isbn13 = ?").bind(&id).execute(&state.db).await.unwrap();

    let cli = TestClient::new(app(state));
    let body = json(cli.get(format!("/gotolib/book-info?isbn={id}")).send().await).await;
    assert_eq!(body["info"][&id]["status"], "ok");
    assert_eq!(body["info"][&id]["formHint"], "소설집");
}

#[tokio::test]
async fn pending_and_error_rows_hide_internal_state_and_fields() {
    let state = test_state().await;
    let (failing, waiting) = (isbn(5), isbn(6));
    set_row(&state, &failing, book_info::STATUS_ERROR, Some("안 믿을 수도 있는 중간 값"), None, None).await;
    set_row(&state, &waiting, book_info::STATUS_PENDING, None, None, None).await;

    let cli = TestClient::new(app(state));
    let body = json(cli.get(format!("/gotolib/book-info?isbn={failing},{waiting}")).send().await).await;
    // error는 앱이 알 필요 없는 내부 사정이라 pending으로 보이고, 필드도 내보내지 않는다.
    assert_eq!(body["info"][&failing]["status"], "pending");
    assert!(body["info"][&failing]["category"].is_null());
    assert_eq!(body["info"][&waiting]["status"], "pending");
}

#[tokio::test]
async fn query_never_reaches_aladin() {
    let cli = TestClient::new(app(test_state().await));
    cli.get(format!("/gotolib/book-info?isbn={}", isbn(7))).send().await.assert_status_is_ok();
}
