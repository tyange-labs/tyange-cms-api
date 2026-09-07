// 이 테스트는 실제로 서빙되는 openapi.rs::PortfolioApi를 대상으로 한다.
// 예전엔 등록만 안 된 src/routes/{get,update,delete}_portfolio.rs(죽은 코드)를
// 테스트하고 있어서, cargo test가 통과해도 실서비스 라우트는 전혀 검증이 안 됐다
// (CLAUDE.md에 기록된 기술부채, 2026-09 정리). PortfolioApi는 poem-openapi
// OpenApiService로 감싸져 있어 OpenApiService 자체를 TestClient에 바로 물린다 —
// main.rs가 실제로 nest하는 것과 동일한 조립.
use std::{env, sync::Arc};

use poem::{http::StatusCode, test::TestClient};
use poem_openapi::OpenApiService;
use serde_json::json;
use sqlx::{SqlitePool, query_scalar};

use crate::{db::init_db, models::AppState, openapi::PortfolioApi};
use tyange_cms_api::auth::jwt::Claims;

async fn create_state() -> Arc<AppState> {
    env::set_var("JWT_ACCESS_SECRET", "test-access-secret");

    let db = SqlitePool::connect("sqlite::memory:")
        .await
        .expect("failed to connect sqlite");
    init_db(&db).await.expect("failed to init db");
    Arc::new(AppState::new(db))
}

fn create_portfolio_app(state: Arc<AppState>) -> OpenApiService<PortfolioApi, ()> {
    OpenApiService::new(PortfolioApi { state }, "test", "1.0")
}

fn admin_token() -> String {
    Claims::create_access_token("admin@example.com", "admin", b"test-access-secret")
        .expect("failed to create admin token")
}

fn user_token() -> String {
    Claims::create_access_token("member@example.com", "user", b"test-access-secret")
        .expect("failed to create user token")
}

fn sample_portfolio_payload() -> serde_json::Value {
    json!({
        "content": {
            "slug": "dev",
            "version": 1,
            "identity": {
                "name": "TYANGE",
                "email": "usun16@gmail.com",
                "github_url": "https://github.com/tyange"
            },
            "featured_projects": [],
            "career": {
                "summary_label": "경력",
                "summary_value": "4년",
                "companies": [
                    {
                        "company": "테스트 회사",
                        "period": "2020.01 - 2022.12",
                        "employment_type": "정규직",
                        "role": "프론트엔드 개발",
                        "position": "사원",
                        "career_items_markdown": "## 서비스 운영\n\n- React 운영\n- TypeScript 전환"
                    }
                ]
            }
        }
    })
}

#[tokio::test]
async fn get_portfolio_returns_not_found_initially_and_put_creates_and_updates_it() {
    let state = create_state().await;
    let cli = TestClient::new(create_portfolio_app(state));

    let initial = cli.get("/portfolio").send().await;
    initial.assert_status(StatusCode::NOT_FOUND);

    let created = cli
        .put("/portfolio/update")
        .header("Authorization", admin_token())
        .body_json(&sample_portfolio_payload())
        .send()
        .await;

    created.assert_status_is_ok();
    let created_json = created.json().await;
    created_json
        .value()
        .object()
        .get("data")
        .object()
        .get("content")
        .object()
        .get("identity")
        .object()
        .get("email")
        .assert_string("usun16@gmail.com");
    created_json
        .value()
        .object()
        .get("data")
        .object()
        .get("content")
        .object()
        .get("career")
        .object()
        .get("summary_value")
        .assert_string("4년");
    created_json
        .value()
        .object()
        .get("data")
        .object()
        .get("content")
        .object()
        .get("career")
        .object()
        .get("companies")
        .array()
        .get(0)
        .object()
        .get("career_items_markdown")
        .assert_string("## 서비스 운영\n\n- React 운영\n- TypeScript 전환");

    // GET should return the saved document
    let fetched = cli.get("/portfolio").send().await;
    fetched.assert_status_is_ok();
    let fetched_json = fetched.json().await;
    fetched_json
        .value()
        .object()
        .get("data")
        .object()
        .get("content")
        .object()
        .get("identity")
        .object()
        .get("email")
        .assert_string("usun16@gmail.com");
}

#[tokio::test]
async fn update_portfolio_rejects_non_admin_user() {
    let state = create_state().await;
    let cli = TestClient::new(create_portfolio_app(state));

    let response = cli
        .put("/portfolio/update")
        .header("Authorization", user_token())
        .body_json(&sample_portfolio_payload())
        .send()
        .await;

    response.assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn delete_portfolio_removes_document() {
    let state = create_state().await;
    let cli = TestClient::new(create_portfolio_app(state.clone()));

    cli.put("/portfolio/update")
        .header("Authorization", admin_token())
        .body_json(&sample_portfolio_payload())
        .send()
        .await
        .assert_status_is_ok();

    let deleted = cli
        .delete("/portfolio")
        .header("Authorization", admin_token())
        .send()
        .await;
    deleted.assert_status(StatusCode::NO_CONTENT);

    let remaining: Option<String> = query_scalar("SELECT slug FROM portfolio WHERE slug = ?")
        .bind("dev")
        .fetch_optional(&state.db)
        .await
        .expect("failed to query portfolio after delete");

    assert!(remaining.is_none());

    let section_count: i32 = query_scalar("SELECT COUNT(*) FROM portfolio_section")
        .fetch_one(&state.db)
        .await
        .expect("failed to count sections");

    assert_eq!(section_count, 0);
}

#[tokio::test]
async fn update_section_updates_individual_section() {
    let state = create_state().await;
    let cli = TestClient::new(create_portfolio_app(state));

    cli.put("/portfolio/update")
        .header("Authorization", admin_token())
        .body_json(&json!({
            "content": {
                "slug": "dev",
                "version": 1,
                "identity": {
                    "name": "Before",
                    "email": "old@example.com",
                    "github_url": ""
                },
                "featured_projects": []
            }
        }))
        .send()
        .await
        .assert_status_is_ok();

    // Update only the identity section
    let section_updated = cli
        .put("/portfolio/sections/identity")
        .header("Authorization", admin_token())
        .body_json(&json!({
            "content": {
                "name": "After",
                "email": "new@example.com",
                "github_url": "https://github.com/tyange"
            }
        }))
        .send()
        .await;

    section_updated.assert_status_is_ok();

    // GET should reflect the updated identity
    let fetched = cli.get("/portfolio").send().await;
    fetched.assert_status_is_ok();
    let fetched_json = fetched.json().await;
    fetched_json
        .value()
        .object()
        .get("data")
        .object()
        .get("content")
        .object()
        .get("identity")
        .object()
        .get("email")
        .assert_string("new@example.com");
    fetched_json
        .value()
        .object()
        .get("data")
        .object()
        .get("content")
        .object()
        .get("identity")
        .object()
        .get("name")
        .assert_string("After");
}

#[tokio::test]
async fn update_portfolio_section_rejects_unknown_section_key() {
    let state = create_state().await;
    let cli = TestClient::new(create_portfolio_app(state));

    cli.put("/portfolio/update")
        .header("Authorization", admin_token())
        .body_json(&sample_portfolio_payload())
        .send()
        .await
        .assert_status_is_ok();

    let response = cli
        .put("/portfolio/sections/unknown_section")
        .header("Authorization", admin_token())
        .body_json(&json!({ "content": {} }))
        .send()
        .await;

    response.assert_status(StatusCode::BAD_REQUEST);
}
