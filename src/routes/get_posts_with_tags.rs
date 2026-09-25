use crate::models::{CustomResponse, PostItem, PostsResponse, SearchPostsWithTag};
use crate::utils::parse_tags;
use crate::AppState;
use poem::http::StatusCode;
use poem::web::{Data, Json, Query};
use poem::{handler, Error};
use sqlx::{QueryBuilder, Row};
use std::sync::Arc;

#[handler]
pub async fn get_posts_with_tags(
    Query(search_params): Query<SearchPostsWithTag>,
    data: Data<&Arc<AppState>>,
) -> Result<Json<CustomResponse<PostsResponse>>, Error> {
    let mut builder = QueryBuilder::new(
        r#"
        SELECT p.post_id, p.title, p.description, p.published_at,
               COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', p.created_at), '') AS created_at, p.status,
        IFNULL(GROUP_CONCAT(t2.category || '::' || t2.name, ','), '') AS tags
        FROM posts p
        LEFT JOIN post_tags pt2 ON p.post_id = pt2.post_id
        LEFT JOIN tags t2 ON pt2.tag_id = t2.tag_id
        WHERE p.status != 'draft'
        "#,
    );

    if let Some(tag) = search_params.include {
        builder.push(
            " AND p.post_id IN (
                SELECT pt.post_id FROM post_tags pt
                JOIN tags t ON pt.tag_id = t.tag_id
                WHERE t.name = ",
        );
        builder.push_bind(tag);
        builder.push(")");
    }

    if let Some(exclude_tag) = search_params.exclude {
        builder.push(
            " AND p.post_id NOT IN (
            SELECT pt.post_id FROM post_tags pt
            JOIN tags t ON pt.tag_id = t.tag_id
            WHERE t.name = ",
        );
        builder.push_bind(exclude_tag);
        builder.push(")");
    }

    builder.push(" GROUP BY p.post_id ORDER BY p.published_at DESC, p.created_at DESC");

    let result = builder.build().fetch_all(&data.db).await;

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
