use std::{
    collections::HashMap,
    io::Cursor,
    sync::{Arc, Mutex},
};

use image::{DynamicImage, ImageFormat as ImgFormat, RgbImage};
use poem::{
    get, handler,
    http::StatusCode,
    listener::TcpAcceptor,
    web::{Data, Path, Query},
    EndpointExt, Response, Route, Server,
};
use sqlx::{query, query_scalar, sqlite::SqlitePoolOptions, SqlitePool};

use super::*;
use crate::db::init_db;

// ── 순수 함수 ────────────────────────────────────────────────────────────────

#[test]
fn plain_len_matches_lovelog_rules() {
    // lovelog `timeline.test.ts`와 같은 기대값
    assert_eq!(plain_len("**해니**가 [여기](https://x.com)서 웃었다."), "해니가 여기서 웃었다.".chars().count());
    assert_eq!(plain_len("![](https://tyange.com/api/cms/images/a.png)"), 0);
    assert_eq!(plain_len("# 제목\n\n- 하나\n- 둘\n\n> 인용\n\n1. 첫째"), "제목\n\n하나\n둘\n\n인용\n\n첫째".chars().count());
    assert_eq!(plain_len("  가나다  "), 3);
}

#[test]
fn short_form_applies_only_since_the_cutoff_and_over_the_limit() {
    let long = "가".repeat(SHORT_FORM_LIMIT + 1);
    let ok = "가".repeat(SHORT_FORM_LIMIT);
    assert!(short_form_violation("2026-10-15", &long).is_some());
    assert!(short_form_violation("2026-10-15", &ok).is_none());
    // 기준일 전의 긴 글은 규칙 밖이다.
    assert!(short_form_violation("2026-09-22", &long).is_none());
}

#[test]
fn seoul_date_rolls_over_at_utc_15() {
    assert_eq!(seoul_date("2026-10-15T14:59:59.000Z").unwrap(), "2026-10-15");
    assert_eq!(seoul_date("2026-10-15T15:00:00.000Z").unwrap(), "2026-10-16");
}

// ── 가짜 Craft ───────────────────────────────────────────────────────────────

#[derive(Clone)]
enum TestBlock {
    Text(&'static str),
    Image(&'static str),
}

#[derive(Clone)]
struct FakeDoc {
    id: &'static str,
    title: &'static str,
    modified: &'static str,
    blocks: Vec<TestBlock>,
}

struct Fake {
    base: String,
    docs: Vec<FakeDoc>,
    fail: bool,
    image: Vec<u8>,
    image_hits: usize,
}

type Shared = Arc<Mutex<Fake>>;

fn png() -> Vec<u8> {
    let img = RgbImage::from_fn(4, 4, |x, y| image::Rgb([x as u8 * 40, y as u8 * 40, 90]));
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(img)
        .write_to(&mut out, ImgFormat::Png)
        .unwrap();
    out.into_inner()
}

fn json(value: serde_json::Value) -> Response {
    Response::builder()
        .content_type("application/json")
        .body(value.to_string())
}

#[handler]
fn fake_documents(fake: Data<&Shared>, Query(q): Query<HashMap<String, String>>) -> Response {
    let fake = fake.lock().unwrap();
    if fake.fail {
        return Response::builder().status(StatusCode::INTERNAL_SERVER_ERROR).finish();
    }
    assert_eq!(q.get("folderId").map(String::as_str), Some("FOLDER"));
    let items: Vec<_> = fake
        .docs
        .iter()
        .map(|d| {
            serde_json::json!({
                "id": d.id, "title": d.title,
                "lastModifiedAt": d.modified, "createdAt": "2026-10-15T03:00:00.000Z",
            })
        })
        .collect();
    json(serde_json::json!({ "items": items }))
}

#[handler]
fn fake_blocks(fake: Data<&Shared>, Query(q): Query<HashMap<String, String>>) -> Response {
    let fake = fake.lock().unwrap();
    let Some(doc) = fake.docs.iter().find(|d| Some(d.id) == q.get("id").map(String::as_str)) else {
        return Response::builder().status(StatusCode::NOT_FOUND).finish();
    };
    let content: Vec<_> = doc
        .blocks
        .iter()
        .enumerate()
        .map(|(i, b)| match b {
            TestBlock::Text(t) => serde_json::json!({ "id": format!("{}-t{i}", doc.id), "type": "text", "markdown": t }),
            TestBlock::Image(name) => serde_json::json!({
                "id": format!("{}-{name}", doc.id), "type": "image",
                "url": format!("{}/img/{name}", fake.base), "markdown": "![Image](x)",
            }),
        })
        .collect();
    json(serde_json::json!({ "id": doc.id, "type": "page", "markdown": doc.title, "content": content }))
}

#[handler]
fn fake_image(fake: Data<&Shared>, Path(_name): Path<String>) -> Response {
    let mut fake = fake.lock().unwrap();
    fake.image_hits += 1;
    Response::builder().content_type("image/png").body(fake.image.clone())
}

struct Harness {
    pool: SqlitePool,
    service: Arc<CraftSyncService>,
    fake: Shared,
}

async fn harness() -> Harness {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let fake: Shared = Arc::new(Mutex::new(Fake {
        base: base.clone(),
        docs: Vec::new(),
        fail: false,
        image: png(),
        image_hits: 0,
    }));
    let app = Route::new()
        .at("/documents", get(fake_documents))
        .at("/blocks", get(fake_blocks))
        .at("/img/:name", get(fake_image))
        .data(fake.clone());
    tokio::spawn(Server::new_with_acceptor(TcpAcceptor::from_tokio(listener).unwrap()).run(app));

    // 인메모리 SQLite는 연결마다 따로라 연결 하나로 고정한다.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    init_db(&pool).await.unwrap();

    let upload_dir = std::env::temp_dir().join(format!("craft-sync-test-{}", Uuid::new_v4()));
    let service = CraftSyncService::new(Some(CraftSyncConfig {
        api_url: base,
        folder_id: "FOLDER".to_string(),
        interval: Duration::from_secs(300),
        image_base_url: "https://example.test/api/cms".to_string(),
        upload_dir,
        writer_id: Some("tester".to_string()),
        tag: "lovelog".to_string(),
        tag_category: "general".to_string(),
        new_status: "published".to_string(),
    }));
    Harness { pool, service, fake }
}

impl Harness {
    fn set_docs(&self, docs: Vec<FakeDoc>) {
        self.fake.lock().unwrap().docs = docs;
    }

    async fn post(&self, post_id: &str) -> (String, String, String) {
        sqlx::query_as("SELECT status, content, published_at FROM posts WHERE post_id = ?")
            .bind(post_id)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    async fn count(&self, table: &str) -> i64 {
        query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }
}

fn handwriting(modified: &'static str) -> FakeDoc {
    FakeDoc {
        id: "D1",
        title: "연보",
        modified,
        blocks: vec![TestBlock::Text(""), TestBlock::Image("ink.png")],
    }
}

// ── 동기화 ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn creates_a_published_lovelog_post_with_the_image_rehosted() {
    let h = harness().await;
    h.set_docs(vec![handwriting("2026-10-15T03:05:00.000Z")]);

    let report = h.service.sync_once(&h.pool).await.unwrap();

    assert_eq!(report.created.len(), 1, "{report:?}");
    assert!(report.errors.is_empty(), "{report:?}");
    let (status, content, published_at) = h.post(&report.created[0].post_id).await;
    assert_eq!(status, "published");
    assert_eq!(published_at, "2026-10-15");
    // Craft 주소가 아니라 이 서버가 저장한 이미지 주소여야 한다.
    assert!(content.starts_with("![](https://example.test/api/cms/images/"), "{content}");
    assert!(content.ends_with(".png)"), "{content}");
    assert!(!content.contains("127.0.0.1"), "{content}");
    let tag: String = query_scalar(
        "SELECT t.name || '/' || t.category FROM post_tags pt JOIN tags t ON t.tag_id = pt.tag_id WHERE pt.post_id = ?",
    )
    .bind(&report.created[0].post_id)
    .fetch_one(&h.pool)
    .await
    .unwrap();
    assert_eq!(tag, "lovelog/general");
    assert_eq!(h.count("images").await, 1);
}

#[tokio::test]
async fn a_second_sync_changes_nothing() {
    let h = harness().await;
    h.set_docs(vec![handwriting("2026-10-15T03:05:00.000Z")]);
    h.service.sync_once(&h.pool).await.unwrap();

    let report = h.service.sync_once(&h.pool).await.unwrap();

    assert!(report.created.is_empty() && report.updated.is_empty(), "{report:?}");
    assert_eq!(report.unchanged, 1);
    assert_eq!(h.count("posts").await, 1);
    assert_eq!(h.count("images").await, 1);
}

#[tokio::test]
async fn editing_updates_only_the_body_and_reuses_the_stored_image() {
    let h = harness().await;
    h.set_docs(vec![handwriting("2026-10-15T03:05:00.000Z")]);
    let first = h.service.sync_once(&h.pool).await.unwrap();
    let post_id = first.created[0].post_id.clone();
    // CMS에서 사람이 draft로 돌려 둔 상태.
    query("UPDATE posts SET status = 'draft' WHERE post_id = ?")
        .bind(&post_id)
        .execute(&h.pool)
        .await
        .unwrap();
    let hits_before = h.fake.lock().unwrap().image_hits;

    h.set_docs(vec![FakeDoc {
        blocks: vec![TestBlock::Text("덧붙인 글"), TestBlock::Image("ink.png")],
        ..handwriting("2026-10-15T04:00:00.000Z")
    }]);
    let report = h.service.sync_once(&h.pool).await.unwrap();

    assert_eq!(report.updated.len(), 1, "{report:?}");
    let (status, content, _) = h.post(&post_id).await;
    assert_eq!(status, "draft", "수정이 CMS에서 정한 상태를 덮어쓰면 안 된다");
    assert!(content.starts_with("덧붙인 글\n\n![](https://example.test/api/cms/images/"), "{content}");
    assert_eq!(h.count("images").await, 1, "같은 이미지를 다시 저장하면 안 된다");
    assert_eq!(h.fake.lock().unwrap().image_hits, hits_before);
}

#[tokio::test]
async fn a_document_that_leaves_the_folder_goes_back_to_draft_and_returns_when_it_reappears() {
    let h = harness().await;
    let keep = FakeDoc { id: "D2", title: "남는 글", modified: "m1", blocks: vec![TestBlock::Text("남는다")] };
    h.set_docs(vec![handwriting("m1"), keep.clone()]);
    let first = h.service.sync_once(&h.pool).await.unwrap();
    let post_id = first.created.iter().find(|p| p.craft_doc_id == "D1").unwrap().post_id.clone();

    h.set_docs(vec![keep.clone()]);
    let report = h.service.sync_once(&h.pool).await.unwrap();
    assert_eq!(report.drafted.len(), 1, "{report:?}");
    assert_eq!(h.post(&post_id).await.0, "draft");
    assert_eq!(h.count("posts").await, 2, "글을 지우면 안 된다");

    // 다시 돌아오면(같은 수정 시각이어도) 되살아난다.
    h.set_docs(vec![handwriting("m1"), keep]);
    let report = h.service.sync_once(&h.pool).await.unwrap();
    assert_eq!(report.updated.len(), 1, "{report:?}");
    assert_eq!(h.post(&post_id).await.0, "published");
}

#[tokio::test]
async fn an_empty_listing_does_not_draft_everything() {
    let h = harness().await;
    h.set_docs(vec![handwriting("m1")]);
    let first = h.service.sync_once(&h.pool).await.unwrap();
    let post_id = first.created[0].post_id.clone();

    h.set_docs(vec![]);
    let report = h.service.sync_once(&h.pool).await.unwrap();

    assert!(report.drafted.is_empty(), "{report:?}");
    assert_eq!(report.warnings.len(), 1);
    assert_eq!(h.post(&post_id).await.0, "published");
}

#[tokio::test]
async fn a_blank_document_is_not_published() {
    let h = harness().await;
    h.set_docs(vec![FakeDoc { id: "D3", title: "쓰는 중", modified: "m1", blocks: vec![TestBlock::Text("  ")] }]);

    let report = h.service.sync_once(&h.pool).await.unwrap();

    assert!(report.created.is_empty(), "{report:?}");
    assert_eq!(h.count("posts").await, 0);
}

#[tokio::test]
async fn text_over_the_short_form_limit_is_saved_as_a_draft() {
    let h = harness().await;
    let long: &'static str = Box::leak("가".repeat(SHORT_FORM_LIMIT + 1).into_boxed_str());
    h.set_docs(vec![FakeDoc { id: "D4", title: "긴 글", modified: "m1", blocks: vec![TestBlock::Text(long)] }]);

    let report = h.service.sync_once(&h.pool).await.unwrap();

    assert_eq!(h.post(&report.created[0].post_id).await.0, "draft");
    assert_eq!(report.warnings.len(), 1, "{report:?}");
}

#[tokio::test]
async fn a_post_deleted_in_the_cms_is_not_resurrected() {
    let h = harness().await;
    h.set_docs(vec![handwriting("m1")]);
    let first = h.service.sync_once(&h.pool).await.unwrap();
    for sql in ["DELETE FROM post_tags WHERE post_id = ?", "DELETE FROM posts WHERE post_id = ?"] {
        query(sql)
            .bind(&first.created[0].post_id)
            .execute(&h.pool)
            .await
            .unwrap();
    }

    h.set_docs(vec![handwriting("m2")]);
    let report = h.service.sync_once(&h.pool).await.unwrap();

    assert!(report.created.is_empty() && report.updated.is_empty(), "{report:?}");
    assert_eq!(report.warnings.len(), 1);
    assert_eq!(h.count("posts").await, 0);
}

#[tokio::test]
async fn a_craft_failure_leaves_everything_untouched_and_does_not_leak_the_link() {
    let h = harness().await;
    h.set_docs(vec![handwriting("m1")]);
    h.service.sync_once(&h.pool).await.unwrap();

    h.fake.lock().unwrap().fail = true;
    let err = h.service.sync_once(&h.pool).await.unwrap_err();

    assert!(err.contains("500"), "{err}");
    assert!(!err.contains("127.0.0.1"), "링크가 에러에 새면 안 된다: {err}");
    assert_eq!(h.count("posts").await, 1);
    assert!(h.service.last_report().unwrap().errors.len() == 1);
}

#[tokio::test]
async fn a_bad_image_skips_only_that_document_and_retries_next_round() {
    let h = harness().await;
    h.fake.lock().unwrap().image = b"not an image".to_vec();
    h.set_docs(vec![
        handwriting("m1"),
        FakeDoc { id: "D2", title: "글", modified: "m1", blocks: vec![TestBlock::Text("정상")] },
    ]);

    let report = h.service.sync_once(&h.pool).await.unwrap();
    assert_eq!(report.created.len(), 1, "{report:?}");
    assert_eq!(report.errors.len(), 1, "{report:?}");

    h.fake.lock().unwrap().image = png();
    let report = h.service.sync_once(&h.pool).await.unwrap();
    assert_eq!(report.created.len(), 1, "{report:?}");
    assert!(report.errors.is_empty(), "{report:?}");
    assert_eq!(h.count("posts").await, 2);
}

#[tokio::test]
async fn a_disabled_service_refuses_to_sync() {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    let service = CraftSyncService::new(None);

    assert!(!service.is_enabled());
    assert!(service.sync_once(&pool).await.is_err());
}
