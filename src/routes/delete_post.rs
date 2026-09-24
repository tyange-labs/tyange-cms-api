use crate::blog_redeploy::{is_blog_redeploy_target, BlogContentEvent, BlogVisibility};
use crate::models::{CustomResponse, DeletePostResponse};
use crate::utils::parse_tags;
use crate::AppState;
use poem::http::StatusCode;
use poem::web::{Data, Json, Path};
use poem::{handler, Error, Request};
use sqlx::{query, query_as, Sqlite};
use std::sync::Arc;
use tyange_cms_api::auth::authorization::{current_user, ensure_post_owner};
use tyange_cms_api::models::PostResponseDb;

#[handler]
pub async fn delete_post(
    req: &Request,
    Path(post_id): Path<String>,
    data: Data<&Arc<AppState>>,
) -> Result<Json<CustomResponse<DeletePostResponse>>, Error> {
    let user = current_user(req)?;
    ensure_post_owner(user, &post_id, &data.db).await?;
    let existing_post = query_as::<Sqlite, PostResponseDb>(
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
    .fetch_one(&data.db)
    .await
    .map_err(|err| {
        eprintln!("Error fetch post before delete: {}", err);
        Error::from_string(
            "게시글 조회에 실패했습니다.",
            StatusCode::INTERNAL_SERVER_ERROR,
        )
    })?;

    let result = query(
        r#"
            DELETE FROM posts WHERE post_id = ?
            "#,
    )
    .bind(&post_id)
    .execute(&data.db)
    .await;

    match result {
        Ok(result) => {
            if result.rows_affected() == 0 {
                return Err(Error::from_string(
                    "게시글을 찾을 수 없습니다.",
                    StatusCode::NOT_FOUND,
                ));
            }

            if is_blog_redeploy_target(
                &existing_post.status,
                parse_tags(&existing_post.tags)
                    .iter()
                    .map(|tag| tag.tag.as_str()),
            ) {
                data.blog_redeploy
                    .dispatch_content_change(
                        BlogContentEvent::Delete,
                        &post_id,
                        BlogVisibility::Hidden,
                    )
                    .await;
            }

            Ok(Json(CustomResponse {
                status: true,
                data: Some(DeletePostResponse { post_id }),
                message: Some(String::from("포스트가 삭제되었습니다.")),
            }))
        }
        Err(err) => {
            eprintln!("Error delete post: {}", err);
            Err(Error::from_string(
                err.to_string(),
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}
