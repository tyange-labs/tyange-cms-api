// API 키(개인 액세스 토큰) 전 과정: 발급 → 키로 인증 → 폐기, 그리고 "키로는 할 수 없는 일".
// 실제로 서빙되는 openapi.rs::{PostsApi, ApiKeysApi}를 main.rs와 같은 방식으로 조립해 부른다
// (ApiKeyVerifier는 main.rs처럼 요청 데이터로 싣는다).
use std::{env, sync::Arc};

use poem::{endpoint::BoxEndpoint, http::StatusCode, test::TestClient, EndpointExt, Response, Route};
use poem_openapi::OpenApiService;
use serde_json::{json, Value};
use sqlx::{query_scalar, sqlite::SqlitePoolOptions, SqlitePool};
use tyange_cms_api::auth::{api_key::ApiKeyVerifier, jwt::Claims};

use crate::{
    db::init_db,
    models::AppState,
    openapi::{ApiKeysApi, PostsApi},
};

struct Harness {
    pool: SqlitePool,
    cli: TestClient<BoxEndpoint<'static, Response>>,
}

async fn harness() -> Harness {
    env::set_var("JWT_ACCESS_SECRET", "test-access-secret");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    init_db(&pool).await.unwrap();
    let state = Arc::new(AppState::new(pool.clone()));
    let service = OpenApiService::new(
        (PostsApi { state: state.clone() }, ApiKeysApi { state }),
        "test",
        "1.0",
    );
    Harness {
        cli: TestClient::new(
            Route::new()
                .nest("/", service)
                .data(ApiKeyVerifier::new(pool.clone()))
                .map_to_response()
                .boxed(),
        ),
        pool,
    }
}

fn token(user: &str, role: &str) -> String {
    Claims::create_access_token(user, role, b"test-access-secret").unwrap()
}

impl Harness {
    async fn create_key(&self, jwt: &str, role: &str) -> Value {
        let response = self
            .cli
            .post("/admin/api-keys")
            .header("Authorization", jwt)
            .body_json(&json!({ "name": "cli", "role": role }))
            .send()
            .await;
        response.assert_status_is_ok();
        response.json().await.value().deserialize()
    }

    async fn status_of(&self, path: &str, authorization: &str) -> StatusCode {
        self.cli
            .get(path)
            .header("Authorization", authorization)
            .send()
            .await
            .0
            .status()
    }
}

#[tokio::test]
async fn an_admin_creates_a_key_and_the_server_keeps_only_its_hash() {
    let h = harness().await;

    let created = h.create_key(&token("admin", "admin"), "admin").await;

    let key = created["data"]["key"].as_str().unwrap();
    assert!(key.starts_with("tyk_"));
    let stored: String = query_scalar("SELECT key_hash FROM api_keys WHERE key_lookup = ?")
        .bind(&key[4..16])
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_ne!(stored, key, "원문이 저장되면 안 된다");
    assert_eq!(stored, tyange_cms_api::auth::api_key::hash_key(key));
    let leaked: i64 = query_scalar("SELECT COUNT(*) FROM api_keys WHERE key_hash LIKE '%' || ? || '%' OR key_lookup = ?")
        .bind(&key[17..])
        .bind(key)
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(leaked, 0, "비밀 부분이 DB 어디에도 없어야 한다");
}

#[tokio::test]
async fn a_key_authenticates_with_or_without_the_bearer_prefix_and_jwt_still_works() {
    let h = harness().await;
    let key = h.create_key(&token("admin", "admin"), "admin").await["data"]["key"]
        .as_str()
        .unwrap()
        .to_string();

    assert_eq!(h.status_of("/admin/posts", &key).await, StatusCode::OK);
    assert_eq!(h.status_of("/admin/posts", &format!("Bearer {key}")).await, StatusCode::OK);
    assert_eq!(h.status_of("/admin/posts", &token("admin", "admin")).await, StatusCode::OK);
    assert_eq!(
        h.status_of("/admin/posts", &format!("Bearer {}", token("admin", "admin"))).await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn unknown_tampered_and_empty_keys_are_rejected() {
    let h = harness().await;
    let key = h.create_key(&token("admin", "admin"), "admin").await["data"]["key"]
        .as_str()
        .unwrap()
        .to_string();
    let tampered = format!("{}X", &key[..key.len() - 1]);

    for bad in [tampered.as_str(), "tyk_000000000000_nothing", "tyk_", "Bearer ", "nonsense"] {
        assert_eq!(h.status_of("/admin/posts", bad).await, StatusCode::UNAUTHORIZED, "{bad}");
    }
}

#[tokio::test]
async fn a_user_role_key_cannot_reach_admin_routes() {
    let h = harness().await;
    let key = h.create_key(&token("admin", "admin"), "user").await["data"]["key"]
        .as_str()
        .unwrap()
        .to_string();

    assert_eq!(h.status_of("/admin/posts", &key).await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_key_cannot_manage_keys() {
    let h = harness().await;
    let key = h.create_key(&token("admin", "admin"), "admin").await["data"]["key"]
        .as_str()
        .unwrap()
        .to_string();

    assert_eq!(h.status_of("/admin/api-keys", &key).await, StatusCode::UNAUTHORIZED);
    let create = h
        .cli
        .post("/admin/api-keys")
        .header("Authorization", &key)
        .body_json(&json!({ "name": "child", "role": "admin" }))
        .send()
        .await;
    create.assert_status(StatusCode::UNAUTHORIZED);
    let revoke = h.cli.delete("/admin/api-keys/1").header("Authorization", &key).send().await;
    revoke.assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn only_admins_manage_keys_and_input_is_validated() {
    let h = harness().await;

    let as_user = h
        .cli
        .post("/admin/api-keys")
        .header("Authorization", token("member", "user"))
        .body_json(&json!({ "name": "x", "role": "user" }))
        .send()
        .await;
    as_user.assert_status(StatusCode::FORBIDDEN);

    for body in [json!({"name": "  ", "role": "admin"}), json!({"name": "x", "role": "root"}), json!({"name": "가".repeat(51), "role": "admin"})] {
        let response = h
            .cli
            .post("/admin/api-keys")
            .header("Authorization", token("admin", "admin"))
            .body_json(&body)
            .send()
            .await;
        response.assert_status(StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn revoking_stops_the_key_at_once_and_the_list_never_shows_the_secret() {
    let h = harness().await;
    let jwt = token("admin", "admin");
    let created = h.create_key(&jwt, "admin").await;
    let key = created["data"]["key"].as_str().unwrap().to_string();
    let id = created["data"]["api_key_id"].as_i64().unwrap();
    assert_eq!(h.status_of("/admin/posts", &key).await, StatusCode::OK);

    let listed = h.cli.get("/admin/api-keys").header("Authorization", &jwt).send().await;
    listed.assert_status_is_ok();
    let listed: Value = listed.json().await.value().deserialize();
    let item = &listed["data"]["keys"][0];
    assert_eq!(item["key_prefix"].as_str().unwrap(), &key[..16]);
    assert!(!listed.to_string().contains(&key[17..]), "목록에 비밀이 있으면 안 된다");
    assert!(item["last_used_at"].is_string(), "쓴 키는 마지막 사용 시각이 찍힌다");
    assert!(item["revoked_at"].is_null());

    let revoked = h
        .cli
        .delete(format!("/admin/api-keys/{id}"))
        .header("Authorization", &jwt)
        .send()
        .await;
    revoked.assert_status_is_ok();

    assert_eq!(h.status_of("/admin/posts", &key).await, StatusCode::UNAUTHORIZED);
    let again = h
        .cli
        .delete(format!("/admin/api-keys/{id}"))
        .header("Authorization", &jwt)
        .send()
        .await;
    again.assert_status(StatusCode::NOT_FOUND);
    let kept: i64 = query_scalar("SELECT COUNT(*) FROM api_keys WHERE revoked_at IS NOT NULL")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(kept, 1, "행은 지우지 않고 폐기 표시만 한다");
}

#[tokio::test]
async fn one_admin_cannot_list_or_revoke_anothers_keys() {
    let h = harness().await;
    let created = h.create_key(&token("alice", "admin"), "admin").await;
    let id = created["data"]["api_key_id"].as_i64().unwrap();
    let bob = token("bob", "admin");

    let listed = h.cli.get("/admin/api-keys").header("Authorization", &bob).send().await;
    let listed: Value = listed.json().await.value().deserialize();
    assert_eq!(listed["data"]["keys"].as_array().unwrap().len(), 0);
    let revoke = h
        .cli
        .delete(format!("/admin/api-keys/{id}"))
        .header("Authorization", &bob)
        .send()
        .await;
    revoke.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_number_of_active_keys_is_capped() {
    let h = harness().await;
    let jwt = token("admin", "admin");
    for _ in 0..20 {
        h.create_key(&jwt, "user").await;
    }

    let over = h
        .cli
        .post("/admin/api-keys")
        .header("Authorization", &jwt)
        .body_json(&json!({ "name": "one too many", "role": "user" }))
        .send()
        .await;
    over.assert_status(StatusCode::CONFLICT);
}
