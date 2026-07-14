use std::{env, sync::Arc};

use poem::{
    get,
    http::StatusCode,
    post, put,
    test::{TestClient, TestForm, TestFormField},
    Endpoint, EndpointExt, Route,
};
use serde_json::json;
use sqlx::{query, query_scalar, SqlitePool};

use crate::{
    db::init_db,
    middlewares::auth_middleware::Auth,
    models::AppState,
    routes::{
        create_budget_plan::create_budget_plan,
        create_spending::create_spending,
        delete_all_spending::delete_all_spending,
        delete_spending::delete_spending,
        get_budget::get_budget,
        get_spending::get_spending,
        import_spending_excel::{commit_spending_import, preview_spending_import},
        update_active_budget::update_active_budget,
        update_spending::update_spending,
    },
};
use tyange_cms_api::auth::jwt::Claims;

async fn create_test_state() -> Arc<AppState> {
    let db = SqlitePool::connect("sqlite::memory:")
        .await
        .expect("failed to connect sqlite");
    init_db(&db).await.expect("failed to init db");
    Arc::new(AppState::new(db))
}

fn create_budget_app(state: Arc<AppState>) -> impl Endpoint {
    Route::new()
        .at(
            "/budget",
            get(get_budget.with(Auth)).put(update_active_budget.with(Auth)),
        )
        .at("/budget/plan", post(create_budget_plan).with(Auth))
        .at(
            "/budget/spending",
            get(get_spending.with(Auth))
                .post(create_spending.with(Auth))
                .delete(delete_all_spending.with(Auth)),
        )
        .at(
            "/budget/spending/import-preview",
            post(preview_spending_import).with(Auth),
        )
        .at(
            "/budget/spending/import-commit",
            post(commit_spending_import).with(Auth),
        )
        .at(
            "/budget/spending/:record_id",
            put(update_spending).delete(delete_spending).with(Auth),
        )
        .data(state)
}

fn fixture_excel_bytes() -> Option<Vec<u8>> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/test_samples/shinhancard_sample.xls"
    ))
    .ok()
}

fn issue_access_token(user_id: &str, role: &str) -> String {
    env::set_var("JWT_ACCESS_SECRET", "test-access-secret");
    Claims::create_access_token(user_id, role, b"test-access-secret")
        .expect("failed to create access token")
}

#[tokio::test]
async fn budget_summary_uses_latest_budget_period_and_owner_scope() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_budget_app(state.clone()));

    cli.post("/budget/plan")
        .header("Authorization", issue_access_token("user-1", "user"))
        .body_json(&json!({
            "total_budget": 1000,
            "from_date": "2026-03-01",
            "to_date": "2026-03-31",
            "alert_threshold": 0.8
        }))
        .send()
        .await
        .assert_status_is_ok();

    cli.post("/budget/plan")
        .header("Authorization", issue_access_token("user-1", "user"))
        .body_json(&json!({
            "total_budget": 2000,
            "from_date": "2026-04-01",
            "to_date": "2026-04-30",
            "alert_threshold": 0.75
        }))
        .send()
        .await
        .assert_status_is_ok();

    cli.post("/budget/plan")
        .header("Authorization", issue_access_token("user-2", "user"))
        .body_json(&json!({
            "total_budget": 3000,
            "from_date": "2026-04-01",
            "to_date": "2026-04-30",
            "alert_threshold": 0.9
        }))
        .send()
        .await
        .assert_status_is_ok();

    cli.post("/budget/spending")
        .header("Authorization", issue_access_token("user-1", "user"))
        .body_json(&json!({
            "amount": 400,
            "merchant": "lunch",
            "transacted_at": "2026-04-02T12:00:00"
        }))
        .send()
        .await
        .assert_status(StatusCode::CREATED);

    let response = cli
        .get("/budget")
        .header("Authorization", issue_access_token("user-1", "user"))
        .send()
        .await;
    response.assert_status_is_ok();

    let json = response.json().await;
    json.value().object().get("total_budget").assert_i64(2000);
    json.value()
        .object()
        .get("from_date")
        .assert_string("2026-04-01");
    json.value()
        .object()
        .get("to_date")
        .assert_string("2026-04-30");
    json.value().object().get("total_spent").assert_i64(400);
    json.value()
        .object()
        .get("remaining_budget")
        .assert_i64(1600);
    json.value().object().get("usage_rate").assert_f64(0.2);
    json.value().object().get("alert").assert_bool(false);
    json.value()
        .object()
        .get("alert_threshold")
        .assert_f64(0.75);
    json.value().object().get("is_overspent").assert_bool(false);
}

#[tokio::test]
async fn create_budget_plan_rejects_manual_total_spent_fields() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_budget_app(state));
    let token = issue_access_token("manual-total-spent-create-user", "user");

    let response = cli
        .post("/budget/plan")
        .header("Authorization", &token)
        .body_json(&json!({
            "total_budget": 1500,
            "from_date": "2026-04-01",
            "to_date": "2026-04-30",
            "total_spent": 400,
            "alert_threshold": 0.9
        }))
        .send()
        .await;
    response.assert_status(StatusCode::BAD_REQUEST);

    let alias_response = cli
        .post("/budget/plan")
        .header("Authorization", &token)
        .body_json(&json!({
            "total_budget": 1500,
            "from_date": "2026-04-01",
            "to_date": "2026-04-30",
            "spent_so_far": 400
        }))
        .send()
        .await;
    alias_response.assert_status(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn create_budget_plan_without_total_spent_uses_spending_sum() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_budget_app(state.clone()));
    let token = issue_access_token("sum-create-user", "user");

    query(
        "INSERT INTO spending_records (owner_user_id, amount, merchant, transacted_at)
         VALUES (?, ?, ?, ?)",
    )
    .bind("sum-create-user")
    .bind(250_i64)
    .bind("seeded")
    .bind("2026-04-10 09:00:00")
    .execute(&state.db)
    .await
    .expect("failed to seed spending");

    let response = cli
        .post("/budget/plan")
        .header("Authorization", &token)
        .body_json(&json!({
            "total_budget": 1000,
            "from_date": "2026-04-01",
            "to_date": "2026-04-30"
        }))
        .send()
        .await;
    response.assert_status_is_ok();

    let json = response.json().await;
    let data = json.value().object().get("data").object();
    data.get("total_spent").assert_i64(250);
    data.get("remaining_budget").assert_i64(750);
    data.get("usage_rate").assert_f64(0.25);
}

#[tokio::test]
async fn create_spending_rejects_dates_outside_active_period() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_budget_app(state));

    cli.post("/budget/plan")
        .header("Authorization", issue_access_token("period-user", "user"))
        .body_json(&json!({
            "total_budget": 1000,
            "from_date": "2026-04-01",
            "to_date": "2026-04-30"
        }))
        .send()
        .await
        .assert_status_is_ok();

    cli.post("/budget/spending")
        .header("Authorization", issue_access_token("period-user", "user"))
        .body_json(&json!({
            "amount": 100,
            "merchant": "blocked",
            "transacted_at": "2026-05-01T00:00:00"
        }))
        .send()
        .await
        .assert_status(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn update_active_budget_changes_total_budget_and_budget_summary() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_budget_app(state.clone()));
    let token = issue_access_token("budget-update-user", "user");

    cli.post("/budget/plan")
        .header("Authorization", &token)
        .body_json(&json!({
            "total_budget": 1000,
            "from_date": "2026-04-01",
            "to_date": "2026-04-30",
            "alert_threshold": 0.8
        }))
        .send()
        .await
        .assert_status_is_ok();

    cli.post("/budget/spending")
        .header("Authorization", &token)
        .body_json(&json!({
            "amount": 400,
            "merchant": "lunch",
            "transacted_at": "2026-04-02T12:00:00"
        }))
        .send()
        .await
        .assert_status(StatusCode::CREATED);

    let response = cli
        .put("/budget")
        .header("Authorization", &token)
        .body_json(&json!({
            "total_budget": 1500,
            "alert_threshold": 0.9
        }))
        .send()
        .await;
    response.assert_status_is_ok();

    let json = response.json().await;
    let data = json.value().object().get("data").object();
    data.get("total_budget").assert_i64(1500);
    data.get("total_spent").assert_i64(400);
    data.get("remaining_budget").assert_i64(1100);
    data.get("usage_rate").assert_f64(0.267);
    data.get("alert").assert_bool(false);
    data.get("alert_threshold").assert_f64(0.9);
    data.get("is_overspent").assert_bool(false);

    let budget_response = cli
        .get("/budget")
        .header("Authorization", &token)
        .send()
        .await;
    budget_response.assert_status_is_ok();
    let budget = budget_response.json().await;
    budget.value().object().get("total_budget").assert_i64(1500);
    budget.value().object().get("total_spent").assert_i64(400);
    budget
        .value()
        .object()
        .get("remaining_budget")
        .assert_i64(1100);
    budget.value().object().get("usage_rate").assert_f64(0.267);
    budget.value().object().get("alert").assert_bool(false);
    budget
        .value()
        .object()
        .get("alert_threshold")
        .assert_f64(0.9);
    budget
        .value()
        .object()
        .get("is_overspent")
        .assert_bool(false);
}

#[tokio::test]
async fn update_active_budget_rejects_manual_total_spent_fields() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_budget_app(state.clone()));
    let token = issue_access_token("manual-total-spent-update-user", "user");

    cli.post("/budget/plan")
        .header("Authorization", &token)
        .body_json(&json!({
            "total_budget": 1000,
            "from_date": "2026-04-01",
            "to_date": "2026-04-30"
        }))
        .send()
        .await
        .assert_status_is_ok();

    let response = cli
        .put("/budget")
        .header("Authorization", &token)
        .body_json(&json!({
            "total_budget": 1800,
            "total_spent": 400,
            "alert_threshold": 0.9
        }))
        .send()
        .await;
    response.assert_status(StatusCode::BAD_REQUEST);

    let alias_response = cli
        .put("/budget")
        .header("Authorization", &token)
        .body_json(&json!({
            "total_budget": 1800,
            "spent_so_far": 400
        }))
        .send()
        .await;
    alias_response.assert_status(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn update_active_budget_without_total_spent_keeps_spending_sum() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_budget_app(state.clone()));
    let token = issue_access_token("sum-update-user", "user");

    cli.post("/budget/plan")
        .header("Authorization", &token)
        .body_json(&json!({
            "total_budget": 1000,
            "from_date": "2026-04-01",
            "to_date": "2026-04-30"
        }))
        .send()
        .await
        .assert_status_is_ok();

    cli.post("/budget/spending")
        .header("Authorization", &token)
        .body_json(&json!({
            "amount": 300,
            "merchant": "dinner",
            "transacted_at": "2026-04-03T12:00:00"
        }))
        .send()
        .await
        .assert_status(StatusCode::CREATED);

    let response = cli
        .put("/budget")
        .header("Authorization", &token)
        .body_json(&json!({
            "total_budget": 1200
        }))
        .send()
        .await;
    response.assert_status_is_ok();

    let json = response.json().await;
    let data = json.value().object().get("data").object();
    data.get("total_spent").assert_i64(300);
    data.get("remaining_budget").assert_i64(900);
}

#[tokio::test]
async fn budget_summary_marks_overspent_when_total_spent_exceeds_total_budget() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_budget_app(state.clone()));
    let token = issue_access_token("overspent-user", "user");

    query(
        "INSERT INTO spending_records (owner_user_id, amount, merchant, transacted_at)
         VALUES (?, ?, ?, ?)",
    )
    .bind("overspent-user")
    .bind(700_i64)
    .bind("seeded")
    .bind("2026-04-05 12:00:00")
    .execute(&state.db)
    .await
    .expect("failed to seed overspent spending");

    cli.post("/budget/plan")
        .header("Authorization", &token)
        .body_json(&json!({
            "total_budget": 500,
            "from_date": "2026-04-01",
            "to_date": "2026-04-30",
            "alert_threshold": 0.9
        }))
        .send()
        .await
        .assert_status_is_ok();

    let response = cli
        .get("/budget")
        .header("Authorization", &token)
        .send()
        .await;
    response.assert_status_is_ok();

    let json = response.json().await;
    json.value().object().get("total_spent").assert_i64(700);
    json.value()
        .object()
        .get("remaining_budget")
        .assert_i64(-200);
    json.value().object().get("usage_rate").assert_f64(1.4);
    json.value().object().get("alert").assert_bool(true);
    json.value().object().get("is_overspent").assert_bool(true);
}

#[tokio::test]
async fn update_active_budget_returns_not_found_without_active_period() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_budget_app(state));

    cli.put("/budget")
        .header(
            "Authorization",
            issue_access_token("no-budget-user", "user"),
        )
        .body_json(&json!({
            "total_budget": 1500
        }))
        .send()
        .await
        .assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn spending_groups_records_by_iso_week_and_matches_totals() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_budget_app(state.clone()));
    let token = issue_access_token("group-user", "user");

    cli.post("/budget/plan")
        .header("Authorization", &token)
        .body_json(&json!({
            "total_budget": 1000,
            "from_date": "2026-03-30",
            "to_date": "2026-04-12"
        }))
        .send()
        .await
        .assert_status_is_ok();

    for (amount, merchant, transacted_at) in [
        (100, "a", "2026-03-30T12:00:00"),
        (200, "b", "2026-04-01T09:00:00"),
        (50, "c", "2026-04-08T10:00:00"),
    ] {
        cli.post("/budget/spending")
            .header("Authorization", &token)
            .body_json(&json!({
                "amount": amount,
                "merchant": merchant,
                "transacted_at": transacted_at
            }))
            .send()
            .await
            .assert_status(StatusCode::CREATED);
    }

    let response = cli
        .get("/budget/spending")
        .header("Authorization", &token)
        .send()
        .await;
    response.assert_status_is_ok();

    let json = response.json().await;
    json.value().object().get("total_spent").assert_i64(350);
    json.value().object().get("remaining").assert_i64(650);
    let weeks = json.value().object().get("weeks").array();
    assert_eq!(weeks.len(), 2);
    weeks
        .get(0)
        .object()
        .get("week_key")
        .assert_string("2026-W15");
    weeks.get(0).object().get("weekly_total").assert_i64(50);
    weeks.get(0).object().get("record_count").assert_i64(1);
    weeks
        .get(1)
        .object()
        .get("week_key")
        .assert_string("2026-W14");
    weeks.get(1).object().get("weekly_total").assert_i64(300);
    weeks.get(1).object().get("record_count").assert_i64(2);
}

#[tokio::test]
async fn update_and_delete_require_record_owner() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_budget_app(state.clone()));

    cli.post("/budget/plan")
        .header("Authorization", issue_access_token("owner-1", "user"))
        .body_json(&json!({
            "total_budget": 1000,
            "from_date": "2026-04-01",
            "to_date": "2026-04-30"
        }))
        .send()
        .await
        .assert_status_is_ok();

    cli.post("/budget/spending")
        .header("Authorization", issue_access_token("owner-1", "user"))
        .body_json(&json!({
            "amount": 100,
            "merchant": "groceries",
            "transacted_at": "2026-04-05T12:00:00"
        }))
        .send()
        .await
        .assert_status(StatusCode::CREATED);

    let record_id: i64 = query_scalar(
        "SELECT record_id FROM spending_records WHERE owner_user_id = ? ORDER BY record_id DESC LIMIT 1",
    )
    .bind("owner-1")
    .fetch_one(&state.db)
    .await
    .expect("failed to fetch record_id");

    cli.put(&format!("/budget/spending/{}", record_id))
        .header("Authorization", issue_access_token("other-user", "user"))
        .body_json(&json!({
            "amount": 300,
            "merchant": "blocked",
            "transacted_at": "2026-04-06T12:00:00"
        }))
        .send()
        .await
        .assert_status(StatusCode::NOT_FOUND);

    cli.delete(&format!("/budget/spending/{}", record_id))
        .header("Authorization", issue_access_token("other-user", "user"))
        .send()
        .await
        .assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_all_spending_clears_only_current_user_records() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_budget_app(state.clone()));
    let token = issue_access_token("reset-owner", "user");

    for user_id in ["reset-owner", "other-owner"] {
        cli.post("/budget/plan")
            .header("Authorization", issue_access_token(user_id, "user"))
            .body_json(&json!({
                "total_budget": 1000,
                "from_date": "2026-04-01",
                "to_date": "2026-04-30"
            }))
            .send()
            .await
            .assert_status_is_ok();
    }

    for (user_id, amount) in [
        ("reset-owner", 120_i64),
        ("reset-owner", 80_i64),
        ("other-owner", 300_i64),
    ] {
        cli.post("/budget/spending")
            .header("Authorization", issue_access_token(user_id, "user"))
            .body_json(&json!({
                "amount": amount,
                "merchant": "seeded",
                "transacted_at": "2026-04-05T12:00:00"
            }))
            .send()
            .await
            .assert_status(StatusCode::CREATED);
    }

    cli.delete("/budget/spending")
        .header("Authorization", &token)
        .send()
        .await
        .assert_status(StatusCode::NO_CONTENT);

    let remaining_for_owner: i64 =
        query_scalar("SELECT COUNT(*) FROM spending_records WHERE owner_user_id = ?")
            .bind("reset-owner")
            .fetch_one(&state.db)
            .await
            .expect("failed to count reset-owner records");
    let remaining_for_other: i64 =
        query_scalar("SELECT COUNT(*) FROM spending_records WHERE owner_user_id = ?")
            .bind("other-owner")
            .fetch_one(&state.db)
            .await
            .expect("failed to count other-owner records");
    assert_eq!(remaining_for_owner, 0);
    assert_eq!(remaining_for_other, 1);

    let budget = cli
        .get("/budget")
        .header("Authorization", &token)
        .send()
        .await;
    budget.assert_status_is_ok();
    let budget_json = budget.json().await;
    budget_json
        .value()
        .object()
        .get("total_spent")
        .assert_i64(0);
    budget_json
        .value()
        .object()
        .get("remaining_budget")
        .assert_i64(1000);

    let spending = cli
        .get("/budget/spending")
        .header("Authorization", &token)
        .send()
        .await;
    spending.assert_status_is_ok();
    let spending_json = spending.json().await;
    spending_json
        .value()
        .object()
        .get("total_spent")
        .assert_i64(0);
    spending_json
        .value()
        .object()
        .get("remaining")
        .assert_i64(1000);
    assert_eq!(spending_json.value().object().get("weeks").array().len(), 0);
}

#[tokio::test]
async fn delete_all_spending_returns_no_content_without_active_budget() {
    let state = create_test_state().await;
    let cli = TestClient::new(create_budget_app(state.clone()));
    let token = issue_access_token("reset-no-budget", "user");

    query(
        "INSERT INTO spending_records (owner_user_id, amount, merchant, transacted_at)
         VALUES (?, ?, ?, ?)",
    )
    .bind("reset-no-budget")
    .bind(100_i64)
    .bind("seeded")
    .bind("2026-04-05 12:00:00")
    .execute(&state.db)
    .await
    .expect("failed to seed spending without budget");

    cli.delete("/budget/spending")
        .header("Authorization", &token)
        .send()
        .await
        .assert_status(StatusCode::NO_CONTENT);

    let remaining_for_owner: i64 =
        query_scalar("SELECT COUNT(*) FROM spending_records WHERE owner_user_id = ?")
            .bind("reset-no-budget")
            .fetch_one(&state.db)
            .await
            .expect("failed to count reset-no-budget records");
    assert_eq!(remaining_for_owner, 0);
}

#[tokio::test]
async fn init_db_leaves_stale_spending_temp_table_alone_when_schema_is_current() {
    let db = SqlitePool::connect("sqlite::memory:")
        .await
        .expect("failed to connect sqlite");

    query(
        r#"
        CREATE TABLE spending_records (
            record_id INTEGER PRIMARY KEY AUTOINCREMENT,
            owner_user_id TEXT NOT NULL,
            amount INTEGER NOT NULL,
            merchant TEXT,
            transacted_at DATETIME NOT NULL,
            source_type TEXT,
            source_fingerprint TEXT,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(&db)
    .await
    .expect("failed to create spending_records");

    query(
        r#"
        CREATE TABLE spending_records_new (
            record_id INTEGER PRIMARY KEY AUTOINCREMENT,
            owner_user_id TEXT NOT NULL,
            amount INTEGER NOT NULL,
            merchant TEXT,
            transacted_at DATETIME NOT NULL,
            source_type TEXT,
            source_fingerprint TEXT,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(&db)
    .await
    .expect("failed to create spending_records_new");

    init_db(&db).await.expect("failed to init db");

    let temp_exists: i64 = query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'spending_records_new'",
    )
    .fetch_one(&db)
    .await
    .expect("failed to inspect spending_records_new");
    assert_eq!(temp_exists, 1);
}

#[tokio::test]
async fn init_db_leaves_stale_budget_temp_table_alone_when_schema_is_current() {
    let db = SqlitePool::connect("sqlite::memory:")
        .await
        .expect("failed to connect sqlite");

    query(
        r#"
        CREATE TABLE budget_periods (
            budget_id INTEGER PRIMARY KEY AUTOINCREMENT,
            owner_user_id TEXT NOT NULL,
            total_budget INTEGER NOT NULL,
            from_date DATE NOT NULL,
            to_date DATE NOT NULL,
            alert_threshold REAL NOT NULL DEFAULT 0.85,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(&db)
    .await
    .expect("failed to create budget_periods");

    query(
        r#"
        CREATE TABLE budget_periods_new (
            budget_id INTEGER PRIMARY KEY AUTOINCREMENT,
            owner_user_id TEXT NOT NULL,
            total_budget INTEGER NOT NULL,
            from_date DATE NOT NULL,
            to_date DATE NOT NULL,
            alert_threshold REAL NOT NULL DEFAULT 0.85,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(&db)
    .await
    .expect("failed to create budget_periods_new");

    init_db(&db).await.expect("failed to init db");

    let temp_exists: i64 = query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'budget_periods_new'",
    )
    .fetch_one(&db)
    .await
    .expect("failed to inspect budget_periods_new");
    assert_eq!(temp_exists, 1);
}

#[tokio::test]
async fn spending_import_preview_reports_new_and_out_of_period_rows() {
    let Some(fixture_bytes) = fixture_excel_bytes() else {
        return;
    };
    let state = create_test_state().await;
    let cli = TestClient::new(create_budget_app(state));
    let token = issue_access_token("excel-preview-user", "user");

    cli.post("/budget/plan")
        .header("Authorization", &token)
        .body_json(&json!({
            "total_budget": 300000,
            "from_date": "2026-03-01",
            "to_date": "2026-03-05"
        }))
        .send()
        .await
        .assert_status_is_ok();

    let response = cli
        .post("/budget/spending/import-preview")
        .header("Authorization", &token)
        .multipart(
            TestForm::new().field(
                TestFormField::bytes(fixture_bytes)
                    .name("file")
                    .filename("shinhancard_sample.xls")
                    .content_type("application/vnd.ms-excel"),
            ),
        )
        .send()
        .await;
    response.assert_status_is_ok();

    let json = response.json().await;
    let data = json.value().object().get("data").object();
    data.get("detected_source").assert_string("shinhancard_xls");
    let summary = data.get("summary").object();
    assert!(summary.get("parsed_count").i64() > 0);
    assert!(summary.get("new_count").i64() > 0);
    assert!(summary.get("out_of_period_count").i64() > 0);
    let rows = data.get("rows").array();
    rows.assert_contains(|value| {
        let object = value.object();
        object.get("transacted_at").string() == "2026-03-03T12:20:00"
            && object.get("amount").i64() == 8800
            && object.get("merchant").string() == "씨유 역삼신웅점"
            && object.get("fingerprint").string().contains("본인996*")
    });
    assert!(!rows.iter().any(|value| {
        let object = value.object();
        object.get("transacted_at").string() == "2026-03-02T19:09:00"
            && object.get("merchant").string() == "㈜우아한형제들"
    }));
    rows.assert_contains(|value| value.object().get("status").string() == "new");
    rows.assert_contains(|value| value.object().get("status").string() == "out_of_period");
}

#[tokio::test]
async fn spending_import_commit_inserts_selected_rows_and_deduplicates_reuploads() {
    let Some(fixture_bytes) = fixture_excel_bytes() else {
        return;
    };
    let state = create_test_state().await;
    let cli = TestClient::new(create_budget_app(state.clone()));
    let token = issue_access_token("excel-commit-user", "user");

    cli.post("/budget/plan")
        .header("Authorization", &token)
        .body_json(&json!({
            "total_budget": 300000,
            "from_date": "2026-03-01",
            "to_date": "2026-03-05"
        }))
        .send()
        .await
        .assert_status_is_ok();

    let preview = cli
        .post("/budget/spending/import-preview")
        .header("Authorization", &token)
        .multipart(
            TestForm::new().field(
                TestFormField::bytes(fixture_bytes.clone())
                    .name("file")
                    .filename("shinhancard_sample.xls")
                    .content_type("application/vnd.ms-excel"),
            ),
        )
        .send()
        .await;
    preview.assert_status_is_ok();
    let preview_json = preview.json().await;
    let rows = preview_json
        .value()
        .object()
        .get("data")
        .object()
        .get("rows")
        .array();
    let fingerprint = rows
        .iter()
        .find(|value| value.object().get("status").string() == "new")
        .expect("expected at least one new row")
        .object()
        .get("fingerprint")
        .string()
        .to_string();

    let commit = cli
        .post("/budget/spending/import-commit")
        .header("Authorization", &token)
        .multipart(
            TestForm::new()
                .field(
                    TestFormField::bytes(fixture_bytes.clone())
                        .name("file")
                        .filename("shinhancard_sample.xls")
                        .content_type("application/vnd.ms-excel"),
                )
                .text("selected_fingerprints", &fingerprint),
        )
        .send()
        .await;
    commit.assert_status_is_ok();
    let commit_json = commit.json().await;
    let commit_data = commit_json.value().object().get("data").object();
    commit_data.get("inserted_count").assert_i64(1);
    commit_data.get("skipped_duplicate_count").assert_i64(0);
    let period_total_spent_from_records = commit_data.get("period_total_spent_from_records").i64();
    commit_data
        .get("remaining")
        .assert_i64(300000 - period_total_spent_from_records);
    let imported: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT source_type, source_fingerprint
         FROM spending_records
         WHERE owner_user_id = ?
         ORDER BY record_id DESC
         LIMIT 1",
    )
    .bind("excel-commit-user")
    .fetch_one(&state.db)
    .await
    .expect("failed to fetch imported row");
    assert_eq!(imported.0.as_deref(), Some("shinhancard_xls"));
    assert_eq!(imported.1.as_deref(), Some(fingerprint.as_str()));

    let second_commit = cli
        .post("/budget/spending/import-commit")
        .header("Authorization", &token)
        .multipart(
            TestForm::new()
                .field(
                    TestFormField::bytes(fixture_bytes.clone())
                        .name("file")
                        .filename("shinhancard_sample.xls")
                        .content_type("application/vnd.ms-excel"),
                )
                .text("selected_fingerprints", &fingerprint),
        )
        .send()
        .await;
    second_commit.assert_status(StatusCode::BAD_REQUEST);

    let second_preview = cli
        .post("/budget/spending/import-preview")
        .header("Authorization", &token)
        .multipart(
            TestForm::new().field(
                TestFormField::bytes(fixture_bytes)
                    .name("file")
                    .filename("shinhancard_sample.xls")
                    .content_type("application/vnd.ms-excel"),
            ),
        )
        .send()
        .await;
    second_preview.assert_status_is_ok();
    let second_preview_json = second_preview.json().await;
    let second_rows = second_preview_json
        .value()
        .object()
        .get("data")
        .object()
        .get("rows")
        .array();
    second_rows.assert_contains_exactly_one(|value| {
        let object = value.object();
        object.get("fingerprint").string() == fingerprint
            && object.get("status").string() == "duplicate"
    });
}

#[tokio::test]
async fn spending_import_updates_budget_summary_from_records() {
    let Some(fixture_bytes) = fixture_excel_bytes() else {
        return;
    };
    let state = create_test_state().await;
    let cli = TestClient::new(create_budget_app(state));
    let token = issue_access_token("excel-summary-user", "user");

    cli.post("/budget/plan")
        .header("Authorization", &token)
        .body_json(&json!({
            "total_budget": 300000,
            "from_date": "2026-03-01",
            "to_date": "2026-03-05"
        }))
        .send()
        .await
        .assert_status_is_ok();

    let preview = cli
        .post("/budget/spending/import-preview")
        .header("Authorization", &token)
        .multipart(
            TestForm::new().field(
                TestFormField::bytes(fixture_bytes.clone())
                    .name("file")
                    .filename("shinhancard_sample.xls")
                    .content_type("application/vnd.ms-excel"),
            ),
        )
        .send()
        .await;
    preview.assert_status_is_ok();
    let preview_json = preview.json().await;
    let rows = preview_json
        .value()
        .object()
        .get("data")
        .object()
        .get("rows")
        .array();
    let fingerprint = rows
        .iter()
        .find(|value| value.object().get("status").string() == "new")
        .expect("expected at least one new row")
        .object()
        .get("fingerprint")
        .string()
        .to_string();

    let commit = cli
        .post("/budget/spending/import-commit")
        .header("Authorization", &token)
        .multipart(
            TestForm::new()
                .field(
                    TestFormField::bytes(fixture_bytes)
                        .name("file")
                        .filename("shinhancard_sample.xls")
                        .content_type("application/vnd.ms-excel"),
                )
                .text("selected_fingerprints", &fingerprint),
        )
        .send()
        .await;
    commit.assert_status_is_ok();
    let commit_json = commit.json().await;
    let commit_data = commit_json.value().object().get("data").object();
    let period_total_spent_from_records = commit_data.get("period_total_spent_from_records").i64();
    commit_data
        .get("remaining")
        .assert_i64(300000 - period_total_spent_from_records);

    let budget = cli
        .get("/budget")
        .header("Authorization", &token)
        .send()
        .await;
    budget.assert_status_is_ok();
    let budget_json = budget.json().await;
    budget_json
        .value()
        .object()
        .get("total_spent")
        .assert_i64(period_total_spent_from_records);

    let spending = cli
        .get("/budget/spending")
        .header("Authorization", &token)
        .send()
        .await;
    spending.assert_status_is_ok();
    let spending_json = spending.json().await;
    spending_json
        .value()
        .object()
        .get("total_spent")
        .assert_i64(period_total_spent_from_records);
}

#[tokio::test]
async fn spending_import_commit_rejects_unknown_fingerprint() {
    let Some(fixture_bytes) = fixture_excel_bytes() else {
        return;
    };
    let state = create_test_state().await;
    let cli = TestClient::new(create_budget_app(state));
    let token = issue_access_token("excel-invalid-selection-user", "user");

    cli.post("/budget/plan")
        .header("Authorization", &token)
        .body_json(&json!({
            "total_budget": 300000,
            "from_date": "2026-03-01",
            "to_date": "2026-03-05"
        }))
        .send()
        .await
        .assert_status_is_ok();

    cli.post("/budget/spending/import-commit")
        .header("Authorization", &token)
        .multipart(
            TestForm::new()
                .field(
                    TestFormField::bytes(fixture_bytes)
                        .name("file")
                        .filename("shinhancard_sample.xls")
                        .content_type("application/vnd.ms-excel"),
                )
                .text("selected_fingerprints", "does-not-exist"),
        )
        .send()
        .await
        .assert_status(StatusCode::BAD_REQUEST);
}
