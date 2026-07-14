use std::path::Path;

use poem::{http::StatusCode, web::Field, Error};
use sqlx::{query, SqlitePool};
use tokio::{fs, io::AsyncReadExt};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl ImageFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
            ImageFormat::Gif => "gif",
            ImageFormat::Webp => "webp",
        }
    }

    pub fn mime_type(&self) -> &'static str {
        match self {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::Gif => "image/gif",
            ImageFormat::Webp => "image/webp",
        }
    }
}

/// 파일 앞부분의 매직 바이트로 실제 이미지 포맷을 판별한다.
/// 클라이언트가 보내는 Content-Type 헤더나 파일명 확장자는 위조 가능하므로 신뢰하지 않는다.
pub fn detect_image_format(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some(ImageFormat::Png);
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(ImageFormat::Jpeg);
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(ImageFormat::Gif);
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some(ImageFormat::Webp);
    }
    None
}

pub struct ImageUploadPolicy {
    pub max_bytes: usize,
    pub allowed_formats: &'static [ImageFormat],
}

pub struct StoredImage {
    pub image_id: String,
    pub file_name: String,
    pub web_path: String,
}

#[derive(Debug)]
pub enum ImageStoreError {
    Empty,
    TooLarge { max_bytes: usize },
    UnsupportedFormat,
    Read(std::io::Error),
    CreateDir { path: String, source: std::io::Error },
    WriteFile(std::io::Error),
    Db(sqlx::Error),
}

impl From<ImageStoreError> for Error {
    fn from(err: ImageStoreError) -> Self {
        match err {
            ImageStoreError::Empty => Error::from_string(
                "비어 있는 파일은 업로드할 수 없습니다.",
                StatusCode::BAD_REQUEST,
            ),
            ImageStoreError::TooLarge { max_bytes } => Error::from_string(
                format!(
                    "파일 크기가 제한을 초과했습니다. (최대 {}MB)",
                    max_bytes / (1024 * 1024)
                ),
                StatusCode::PAYLOAD_TOO_LARGE,
            ),
            ImageStoreError::UnsupportedFormat => Error::from_string(
                "지원하지 않는 이미지 형식입니다. (png, jpg, gif, webp만 업로드할 수 있습니다.)",
                StatusCode::BAD_REQUEST,
            ),
            ImageStoreError::Read(e) => Error::from_string(
                format!("파일 읽기 실패: {}", e),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            ImageStoreError::CreateDir { path, source } => Error::from_string(
                format!("디렉토리 생성 실패 ({}): {}", path, source),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            ImageStoreError::WriteFile(e) => Error::from_string(
                format!("파일 생성 실패: {}", e),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
            ImageStoreError::Db(e) => Error::from_string(
                format!("Error upload image: {}", e),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        }
    }
}

/// multipart 필드 하나를 읽어 정책 검증 후 디스크와 images 테이블에 저장한다.
///
/// - policy.max_bytes를 초과하면 전체를 메모리에 올리기 전에 중단
/// - 포맷은 매직 바이트로 판별하고 policy.allowed_formats 화이트리스트로 검증
/// - 저장 확장자와 mime_type은 판별된 실제 포맷에서만 도출 (유저 입력 미사용)
pub async fn store_image(
    field: Field,
    policy: &ImageUploadPolicy,
    db: &SqlitePool,
    post_id: Option<String>,
    image_type: String,
    upload_dir: &Path,
) -> Result<StoredImage, ImageStoreError> {
    let origin_filename = field
        .file_name()
        .map(|name| name.to_owned())
        .unwrap_or_default();

    let mut reader = field.into_async_read();
    let mut file_bytes: Vec<u8> = Vec::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).await.map_err(ImageStoreError::Read)?;
        if n == 0 {
            break;
        }
        if file_bytes.len() + n > policy.max_bytes {
            return Err(ImageStoreError::TooLarge {
                max_bytes: policy.max_bytes,
            });
        }
        file_bytes.extend_from_slice(&buf[..n]);
    }

    if file_bytes.is_empty() {
        return Err(ImageStoreError::Empty);
    }

    let format = detect_image_format(&file_bytes)
        .filter(|format| policy.allowed_formats.contains(format))
        .ok_or(ImageStoreError::UnsupportedFormat)?;

    let file_name = format!("{}.{}", Uuid::new_v4(), format.extension());
    let file_path = upload_dir.join(&file_name);

    fs::create_dir_all(upload_dir)
        .await
        .map_err(|source| ImageStoreError::CreateDir {
            path: upload_dir.display().to_string(),
            source,
        })?;

    fs::write(&file_path, &file_bytes)
        .await
        .map_err(ImageStoreError::WriteFile)?;

    let image_id = Uuid::new_v4().to_string();

    query(
        r#"
        INSERT INTO images (image_id, post_id, file_name, origin_name, file_path, mime_type, image_type)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&image_id)
    .bind(&post_id)
    .bind(&file_name)
    .bind(&origin_filename)
    .bind(file_path.to_str())
    .bind(format.mime_type())
    .bind(&image_type)
    .execute(db)
    .await
    .map_err(|err| {
        eprintln!("Error saving image: {}", err);
        ImageStoreError::Db(err)
    })?;

    println!("이미지 저장 완료: {}", file_path.display());

    let web_path = format!("/images/{}", file_name);

    Ok(StoredImage {
        image_id,
        file_name,
        web_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_png() {
        let bytes = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00];
        assert_eq!(detect_image_format(&bytes), Some(ImageFormat::Png));
    }

    #[test]
    fn detects_jpeg() {
        let bytes = [0xFF, 0xD8, 0xFF, 0xE0, 0x00];
        assert_eq!(detect_image_format(&bytes), Some(ImageFormat::Jpeg));
    }

    #[test]
    fn detects_gif() {
        assert_eq!(detect_image_format(b"GIF89a...."), Some(ImageFormat::Gif));
        assert_eq!(detect_image_format(b"GIF87a...."), Some(ImageFormat::Gif));
    }

    #[test]
    fn detects_webp() {
        let mut bytes = Vec::from(*b"RIFF");
        bytes.extend_from_slice(&[0x24, 0x00, 0x00, 0x00]);
        bytes.extend_from_slice(b"WEBP");
        assert_eq!(detect_image_format(&bytes), Some(ImageFormat::Webp));
    }

    #[test]
    fn rejects_non_image_bytes() {
        assert_eq!(detect_image_format(b"plain text"), None);
        assert_eq!(detect_image_format(b"<svg xmlns=\"...\"></svg>"), None);
        assert_eq!(detect_image_format(b""), None);
        // RIFF지만 WEBP가 아닌 경우 (예: WAV)
        let mut bytes = Vec::from(*b"RIFF");
        bytes.extend_from_slice(&[0x24, 0x00, 0x00, 0x00]);
        bytes.extend_from_slice(b"WAVE");
        assert_eq!(detect_image_format(&bytes), None);
    }
}
