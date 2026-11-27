use crate::models::{CustomResponse, DeletePostResponse};
use crate::AppState;
use poem::http::StatusCode;
use poem::web::{Data, Json, Path};
use poem::{handler, Error, Request};
use sqlx::query;
use std::sync::Arc;
use tyange_cms_api::auth::authorization::{current_user, ensure_post_owner};

#[handler]
pub async fn delete_post(
    req: &Request,
    Path(post_id): Path<String>,
    data: Data<&Arc<AppState>>,
) -> Result<Json<CustomResponse<DeletePostResponse>>, Error> {
    let user = current_user(req)?;
    ensure_post_owner(user, &post_id, &data.db).await?;
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
