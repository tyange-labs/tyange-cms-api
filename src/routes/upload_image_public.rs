use std::{env, path::PathBuf, sync::Arc};

use poem::{
    handler,
    http::StatusCode,
    web::{Data, Json, Multipart},
    Error, Request,
};

use crate::middlewares::rate_limit::client_ip;
use crate::models::{AppState, CustomResponse, UploadImageResponse};
use crate::services::image_storage::{store_image, ImageFormat, ImageRecord, ImageUploadPolicy};

const ANONYMOUS_UPLOAD_POLICY: ImageUploadPolicy = ImageUploadPolicy {
    max_bytes: 3 * 1024 * 1024,
    allowed_formats: &[
        ImageFormat::Png,
        ImageFormat::Jpeg,
        ImageFormat::Gif,
        ImageFormat::Webp,
    ],
};

/// 익명 이미지 업로드.
/// - 인증 없음: 대신 rate limit(라우트 등록부)과 더 작은 정책(3MB)이 적용된다
/// - post_id 등 쿼리 파라미터를 받지 않는다 (익명 유저가 임의 연결 불가)
/// - status='pending'으로 저장되어 확정되지 않으면 24시간 후 정리된다
#[handler]
pub async fn upload_image_public(
    req: &Request,
    mut multipart: Multipart,
    data: Data<&Arc<AppState>>,
) -> Result<Json<CustomResponse<UploadImageResponse>>, Error> {
    if !data.anonymous_upload_enabled {
        return Err(Error::from_string(
            "익명 이미지 업로드가 비활성화되어 있습니다.",
            StatusCode::SERVICE_UNAVAILABLE,
        ));
    }

    while let Some(field) = multipart.next_field().await? {
        if field.file_name().is_none() {
            continue;
        }

        let upload_dir = PathBuf::from(
            env::var("UPLOAD_PATH").unwrap_or_else(|_| ".uploads/images".to_string()),
        )
        .join("public");

        let stored = store_image(
            field,
            &ANONYMOUS_UPLOAD_POLICY,
            &data.db,
            ImageRecord {
                post_id: None,
                image_type: String::from("anonymous"),
                uploader_ip: Some(client_ip(req)),
                status: "pending",
            },
            &upload_dir,
            "/images/public",
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
