use std::sync::Arc;

use poem::{
    handler,
    http::StatusCode,
    web::{Data, Json, Path},
    Error,
};
use sqlx::{query_as, Sqlite};

use crate::AppState;
use crate::{
    models::{Post, PostResponseDb},
    utils::parse_tags,
};

#[handler]
pub async fn get_post(
    Path(post_id): Path<String>,
    data: Data<&Arc<AppState>>,
) -> Result<Json<Post>, Error> {
    let result = query_as::<Sqlite, PostResponseDb>(
        r#"
        SELECT p.post_id, p.title, p.description, p.published_at,
               COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', p.created_at), '') AS created_at,
        p.content, p.status,
        IFNULL(GROUP_CONCAT(t.category || '::' || t.name, ','), '') AS tags
        FROM posts p
        LEFT JOIN post_tags pt ON p.post_id = pt.post_id
        LEFT JOIN tags t ON pt.tag_id = t.tag_id
        WHERE p.post_id = ?
        GROUP BY p.post_id
        "#,
    )
    .bind(&post_id)
    .fetch_optional(&data.db)
    .await;

    match result {
        Ok(Some(db_post)) => {
            let post_response = Post {
                post_id: db_post.post_id,
                title: db_post.title,
                description: db_post.description,
                published_at: db_post.published_at,
                created_at: db_post.created_at,
                tags: parse_tags(&db_post.tags),
                content: db_post.content,
                status: db_post.status,
            };
            Ok(Json(post_response))
        }
        Ok(None) => Err(Error::from_string(
            "해당 id에 해당하는 포스트가 없네요.",
            StatusCode::NOT_FOUND,
        )),
        Err(err) => Err(Error::from_string(
            format!("Error fetching post: {}", err),
            StatusCode::INTERNAL_SERVER_ERROR,
        )),
    }
}
