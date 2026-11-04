//! 표지 썸네일 만들기·저장하기. `book_info`가 알라딘에서 가져온 원본 이미지 바이트를 받아 줄이고
//! 디스크에 쓰는 일만 한다 — 알라딘에 요청을 보내지 않고, 어떤 책의 표지인지도 모른다(ISBN은 파일 이름일 뿐).

use std::{env, fs, path::{Path, PathBuf}};

use image::{codecs::jpeg::JpegEncoder, imageops::FilterType, ImageReader, Limits};

use super::is_valid_isbn13;

/// 앱 카드에 보이는 크기의 2배쯤(고해상도 화면). 원본이 더 작으면 키우지 않는다.
pub const THUMB_WIDTH: u32 = 200;
pub const THUMB_MAX_HEIGHT: u32 = 320;
pub const THUMB_QUALITY: u8 = 80;
/// 디코딩 전에 거르는 한도: 비정상적으로 큰 그림(압축 폭탄)을 메모리에 풀지 않는다.
const MAX_DECODE_DIMENSION: u32 = 6000;
const MAX_DECODE_ALLOC_BYTES: u64 = 256 * 1024 * 1024;

/// 이미지 바이트 → 줄인 JPEG 바이트. 순수 변환이라 네트워크·파일을 쓰지 않는다.
/// 이미지가 아니거나 깨졌으면 `Err`(패닉하지 않는다).
pub fn make_thumbnail(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut reader = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| format!("형식을 알아내지 못했습니다: {e}"))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODE_DIMENSION);
    limits.max_image_height = Some(MAX_DECODE_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOC_BYTES);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|e| format!("이미지를 읽지 못했습니다: {e}"))?;

    // 원본이 상자보다 작으면 키우지 않는다.
    let fitted = if decoded.width() > THUMB_WIDTH || decoded.height() > THUMB_MAX_HEIGHT {
        decoded.resize(THUMB_WIDTH, THUMB_MAX_HEIGHT, FilterType::Lanczos3)
    } else {
        decoded
    };

    let rgb = fitted.to_rgb8();
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, THUMB_QUALITY)
        .encode_image(&rgb)
        .map_err(|e| format!("JPEG로 저장하지 못했습니다: {e}"))?;
    Ok(out)
}

/// 썸네일 폴더. 배포가 지우지 않는 곳(DB와 같은 `data` 아래)을 기본으로 쓴다.
pub fn covers_dir() -> PathBuf {
    env::var("COVERS_DIR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./data/covers"))
}

/// `<폴더>/<isbn>.jpg`. 이름은 검증을 통과한 ISBN과 고정 확장자로만 만든다(외부 문자열을 섞지 않는다).
pub fn cover_path(dir: &Path, isbn: &str) -> Option<PathBuf> {
    is_valid_isbn13(isbn).then(|| dir.join(format!("{isbn}.jpg")))
}

/// 원본 바이트를 줄여 임시 파일로 쓴 뒤 이름을 바꾼다. 쓰다가 죽어도 반쪽 파일이 최종 이름으로 남지 않는다.
/// 블로킹 작업이라 `spawn_blocking` 안에서 부른다.
pub fn store_thumbnail(dir: &Path, isbn: &str, source: &[u8]) -> Result<(), String> {
    let path = cover_path(dir, isbn).ok_or("ISBN이 올바르지 않습니다.")?;
    let thumbnail = make_thumbnail(source)?;
    fs::create_dir_all(dir).map_err(|e| format!("폴더를 만들지 못했습니다: {e}"))?;
    let temp = path.with_extension("jpg.tmp");
    fs::write(&temp, &thumbnail).map_err(|e| format!("파일을 쓰지 못했습니다: {e}"))?;
    fs::rename(&temp, &path).map_err(|e| {
        let _ = fs::remove_file(&temp);
        format!("파일 이름을 바꾸지 못했습니다: {e}")
    })
}

#[cfg(test)]
mod tests;
