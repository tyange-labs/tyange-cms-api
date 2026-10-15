// 글 삭제(`DELETE /post/delete/:id`)가 글을 참조하는 행(post_tags, images) 때문에 500으로 실패하던 문제를
// 고정한다. 실제로 서빙되는 openapi.rs::PostsApi를 부른다. 예전엔 `DELETE FROM posts`만 실행해서
// 태그가 달린 글(새 DB)이나 이미지가 연결된 글(운영 DB 포함)이 외래키 오류로 안 지워졌다.
use std::{env, sync::Arc};

use poem::{endpoint::BoxEndpoint, http::StatusCode, test::TestClient, EndpointExt, Response, Route};
use poem_openapi::OpenApiService;
use serde_json::json;
use sqlx::{query, query_scalar, sqlite::SqlitePoolOptions, SqlitePool};
use tyange_cms_api::auth::jwt::Claims;

use crate::{db::init_db, models::AppState, openapi::PostsApi};

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
    Harness {
        pool,
        cli: TestClient::new(
            Route::new()
                .nest("/", OpenApiService::new(PostsApi { state }, "test", "1.0"))
                .map_to_response()
                .boxed(),
        ),
    }
}

fn token(user: &str, role: &str) -> String {
    Claims::create_access_token(user, role, b"test-access-secret").unwrap()
}

impl Harness {
    async fn create_tagged_post(&self, tags: serde_json::Value) -> String {
        let response = self
            .cli
            .post("/post/upload")
            .header("Authorization", token("admin", "admin"))
            .body_json(&json!({
                "title": "", "description": "", "published_at": "2026-10-15",
                "tags": tags, "content": "본문", "status": "draft",
            }))
            .send()
            .await;
        response.assert_status_is_ok();
        let body: serde_json::Value = response.json().await.value().deserialize();
        body["data"]["post_id"].as_str().unwrap().to_string()
    }

    async fn attach_image(&self, post_id: &str) {
        query(
            "INSERT INTO images (image_id, post_id, file_name, origin_name, file_path, mime_type, image_type)
             VALUES ('img-1', ?, 'a.png', 'a.png', '/tmp/a.png', 'image/png', 'in_post')",
        )
        .bind(post_id)
        .execute(&self.pool)
        .await
        .unwrap();
    }

    async fn delete(&self, post_id: &str, jwt: &str) -> StatusCode {
        self.cli
            .delete(format!("/post/delete/{post_id}"))
            .header("Authorization", jwt)
            .send()
            .await
            .0
            .status()
    }

    async fn count(&self, sql: &str) -> i64 {
        query_scalar(sql).fetch_one(&self.pool).await.unwrap()
    }
}

#[tokio::test]
async fn a_tagged_post_can_be_deleted_and_its_tag_links_go_with_it() {
    let h = harness().await;
    let id = h
        .create_tagged_post(json!([{"tag": "lovelog", "category": "general"}, {"tag": "rust", "category": "dev"}]))
        .await;
    assert_eq!(h.count("SELECT COUNT(*) FROM post_tags").await, 2);

    assert_eq!(h.delete(&id, &token("admin", "admin")).await, StatusCode::OK);

    assert_eq!(h.count("SELECT COUNT(*) FROM posts").await, 0);
    assert_eq!(h.count("SELECT COUNT(*) FROM post_tags").await, 0);
    assert_eq!(h.count("SELECT COUNT(*) FROM tags").await, 2, "태그 자체는 다른 글이 쓸 수 있어 남긴다");
}

#[tokio::test]
async fn a_post_with_attached_images_can_be_deleted_and_the_images_are_kept_detached() {
    let h = harness().await;
    let id = h.create_tagged_post(json!([])).await;
    h.attach_image(&id).await;

    assert_eq!(h.delete(&id, &token("admin", "admin")).await, StatusCode::OK);

    assert_eq!(h.count("SELECT COUNT(*) FROM posts").await, 0);
    assert_eq!(h.count("SELECT COUNT(*) FROM images").await, 1, "이미지 행은 지우지 않는다");
    assert_eq!(
        h.count("SELECT COUNT(*) FROM images WHERE post_id IS NULL").await,
        1,
        "글과의 연결만 끊는다"
    );
}

#[tokio::test]
async fn deleting_one_post_leaves_the_others_untouched() {
    let h = harness().await;
    let keep = h.create_tagged_post(json!([{"tag": "lovelog", "category": "general"}])).await;
    let drop = h.create_tagged_post(json!([{"tag": "lovelog", "category": "general"}])).await;
    h.attach_image(&keep).await;

    assert_eq!(h.delete(&drop, &token("admin", "admin")).await, StatusCode::OK);

    assert_eq!(h.count("SELECT COUNT(*) FROM posts").await, 1);
    assert_eq!(h.count("SELECT COUNT(*) FROM post_tags").await, 1);
    let still_linked: i64 = h
        .count(&format!("SELECT COUNT(*) FROM images WHERE post_id = '{keep}'"))
        .await;
    assert_eq!(still_linked, 1, "다른 글의 이미지 연결은 그대로여야 한다");
}

#[tokio::test]
async fn a_missing_post_is_404_and_changes_nothing() {
    let h = harness().await;
    let id = h.create_tagged_post(json!([{"tag": "lovelog", "category": "general"}])).await;

    assert_eq!(h.delete("no-such-post", &token("admin", "admin")).await, StatusCode::NOT_FOUND);

    assert_eq!(h.count("SELECT COUNT(*) FROM posts").await, 1);
    assert_eq!(h.count("SELECT COUNT(*) FROM post_tags").await, 1);
    let _ = id;
}

#[tokio::test]
async fn only_the_owner_or_an_admin_can_delete() {
    let h = harness().await;
    let id = h.create_tagged_post(json!([])).await;

    assert_eq!(h.delete(&id, &token("someone-else", "user")).await, StatusCode::FORBIDDEN);
    assert_eq!(h.count("SELECT COUNT(*) FROM posts").await, 1);
    assert_eq!(h.delete(&id, "not-a-token").await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_fresh_database_cascades_tag_links_like_production() {
    let h = harness().await;
    let id = h.create_tagged_post(json!([{"tag": "lovelog", "category": "general"}])).await;

    // 핸들러를 거치지 않고 글을 직접 지워도 태그 연결이 따라 지워져야 한다(운영 스키마와 같게).
    query("DELETE FROM posts WHERE post_id = ?")
        .bind(&id)
        .execute(&h.pool)
        .await
        .expect("post_tags가 ON DELETE CASCADE여야 한다");
    assert_eq!(h.count("SELECT COUNT(*) FROM post_tags").await, 0);
}
