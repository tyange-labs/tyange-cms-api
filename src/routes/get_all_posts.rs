use crate::models::{CustomResponse, PostItem, PostsResponse};
use crate::utils::parse_tags;
use crate::AppState;
use poem::http::StatusCode;
use poem::web::{Data, Json};
use poem::{handler, Error};
use sqlx::{query, Row};
use std::sync::Arc;

#[handler]
pub async fn get_all_posts(
    data: Data<&Arc<AppState>>,
) -> Result<Json<CustomResponse<PostsResponse>>, Error> {
    let result = query(
        r#"
        SELECT p.post_id, p.title, p.description, p.published_at,
               COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', p.created_at), '') AS created_at, p.status,
        IFNULL(GROUP_CONCAT(t.category || '::' || t.name, ','), '') AS tags
        FROM posts p
        LEFT JOIN post_tags pt ON p.post_id = pt.post_id
        LEFT JOIN tags t ON pt.tag_id = t.tag_id
        GROUP BY p.post_id
        ORDER BY p.published_at DESC, p.created_at DESC
        "#,
    )
    .fetch_all(&data.db)
    .await;

    match result {
        Ok(db_posts) => {
            if db_posts.is_empty() {
                return Ok(Json(CustomResponse {
                    status: true,
                    data: Some(PostsResponse {
                        posts: Vec::<PostItem>::new(),
                    }),
                    message: Some(String::from("포스트가 하나도 없네요.")),
                }));
            }

            Ok(Json(CustomResponse {
                status: true,
                data: Some(PostsResponse {
                    posts: db_posts
                        .iter()
                        .map(|db_post| PostItem {
                            post_id: db_post.get("post_id"),
                            title: db_post.get("title"),
                            description: db_post.get("description"),
                            published_at: db_post.get("published_at"),
                            created_at: db_post.get("created_at"),
                            tags: parse_tags(db_post.get("tags")),
                            status: db_post.get("status"),
                            excerpt: None,
                        })
                        .collect(),
                }),
                message: None,
            }))
        }
        Err(err) => Err(Error::from_string(
            format!("Error fetching posts: {}", err),
            StatusCode::INTERNAL_SERVER_ERROR,
        )),
    }
}
