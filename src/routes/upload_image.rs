use std::{env, path::PathBuf, sync::Arc};

use poem::{
    handler,
    http::StatusCode,
    web::{Data, Json, Multipart, Query},
    Error, Request,
};
use tyange_cms_api::auth::authorization::current_user;

use crate::models::{AppState, CustomResponse, UploadImageQueryParmas, UploadImageResponse};
use crate::services::image_storage::{store_image, ImageFormat, ImageUploadPolicy};

const AUTHENTICATED_UPLOAD_POLICY: ImageUploadPolicy = ImageUploadPolicy {
    max_bytes: 10 * 1024 * 1024,
    allowed_formats: &[
        ImageFormat::Png,
        ImageFormat::Jpeg,
        ImageFormat::Gif,
        ImageFormat::Webp,
    ],
};

#[handler]
pub async fn upload_image(
    req: &Request,
    mut multipart: Multipart,
    Query(params): Query<UploadImageQueryParmas>,
    data: Data<&Arc<AppState>>,
) -> Result<Json<CustomResponse<UploadImageResponse>>, Error> {
    let _user = current_user(req)?;

    while let Some(field) = multipart.next_field().await? {
        if field.file_name().is_none() {
            continue;
        }

        let upload_dir = PathBuf::from(
            env::var("UPLOAD_PATH").unwrap_or_else(|_| ".uploads/images".to_string()),
        );

        let image_type = params.image_type.clone().unwrap_or(String::from("in_post"));

        let stored = store_image(
            field,
            &AUTHENTICATED_UPLOAD_POLICY,
            &data.db,
            params.post_id.clone(),
            image_type,
            &upload_dir,
        )
        .await?;

        return Ok(Json(CustomResponse {
            status: true,
            data: Some(UploadImageResponse {
                image_path: stored.web_path,
            }),
            message: Some(String::from("이미지 업로드에 성공했습니다.")),
        }));
    }

    Err(Error::from_string(
        "업로드할 이미지 파일이 없습니다.",
        StatusCode::BAD_REQUEST,
    ))
}
