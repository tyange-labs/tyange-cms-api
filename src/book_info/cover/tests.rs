use std::path::PathBuf;

use image::{DynamicImage, ImageFormat, RgbImage};

use super::*;
use crate::book_info::tests::isbn;

fn jpeg(width: u32, height: u32) -> Vec<u8> {
    let img = RgbImage::from_fn(width, height, |x, y| image::Rgb([(x % 255) as u8, (y % 255) as u8, 90]));
    let mut out = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(img).write_to(&mut out, ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

fn temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("covers-test-{}", uuid::Uuid::new_v4()))
}

#[test]
fn thumbnail_shrinks_large_images_and_never_enlarges_small_ones() {
    let big = make_thumbnail(&jpeg(800, 1200)).unwrap();
    let decoded = image::load_from_memory(&big).unwrap();
    assert!(decoded.width() <= THUMB_WIDTH && decoded.height() <= THUMB_MAX_HEIGHT);
    assert!(decoded.width() == THUMB_WIDTH || decoded.height() == THUMB_MAX_HEIGHT);

    let small = make_thumbnail(&jpeg(60, 90)).unwrap();
    let decoded = image::load_from_memory(&small).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (60, 90));
}

#[test]
fn thumbnail_rejects_garbage_without_panicking() {
    assert!(make_thumbnail(b"this is not an image").is_err());
    assert!(make_thumbnail(b"").is_err());
    assert!(make_thumbnail(&jpeg(40, 40)[..50]).is_err()); // 잘린 파일
}

#[test]
fn store_writes_final_file_and_leaves_no_temp_file() {
    let dir = temp_dir();
    let id = isbn(7);
    store_thumbnail(&dir, &id, &jpeg(500, 700)).unwrap();
    let path = cover_path(&dir, &id).unwrap();
    assert!(path.exists());
    assert!(!path.with_extension("jpg.tmp").exists());
    let written = std::fs::read(&path).unwrap();
    assert_eq!(&written[..2], &[0xFF, 0xD8]); // JPEG 시작 표지
    assert!(written.len() < 40 * 1024, "썸네일이 너무 크다: {}", written.len());
    // 같은 ISBN을 다시 저장해도 덮어쓴다
    store_thumbnail(&dir, &id, &jpeg(300, 400)).unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn store_refuses_bad_isbn_and_bad_image() {
    let dir = temp_dir();
    assert!(store_thumbnail(&dir, "../evil", &jpeg(50, 50)).is_err());
    assert!(store_thumbnail(&dir, &isbn(8), b"nope").is_err());
    assert!(cover_path(&dir, "../evil").is_none());
    assert!(!dir.join("../evil.jpg").exists());
    let _ = std::fs::remove_dir_all(dir);
}
