use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use poem::{
    get, handler,
    http::StatusCode,
    listener::TcpAcceptor,
    web::{Data, Path},
    EndpointExt, Request, Response, Route, Server,
};
use sqlx::{query, query_scalar, SqlitePool};

use super::*;
use crate::db::init_db;

/// 9자리 번호로 검사 숫자까지 맞는 ISBN-13을 만든다. `cover/tests.rs`도 이걸 쓴다.
pub(crate) fn isbn(n: u32) -> String {
    let body = format!("978{n:09}");
    let sum: u32 = body
        .bytes()
        .enumerate()
        .map(|(i, d)| u32::from(d - b'0') * if i % 2 == 0 { 1 } else { 3 })
        .sum();
    format!("{body}{}", (10 - sum % 10) % 10)
}

fn jpeg(width: u32, height: u32) -> Vec<u8> {
    use image::{DynamicImage, ImageFormat, RgbImage};
    let img = RgbImage::from_fn(width, height, |x, y| image::Rgb([(x % 255) as u8, (y % 255) as u8, 90]));
    let mut out = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(img).write_to(&mut out, ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

fn temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("covers-test-{}", uuid::Uuid::new_v4()))
}

// ── 순수 함수: ISBN ────────────────────────────────────────────────────────────

#[test]
fn accepts_real_isbn13s() {
    for value in ["9788934972464", "9788936434267", "9791189799847", "9788962624601"] {
        assert!(is_valid_isbn13(value), "{value}");
    }
}

#[test]
fn rejects_malformed_isbns() {
    let cases = [
        "",
        "978893497246",      // 12자리
        "97889349724644",    // 14자리
        "9788934972465",     // 검사 숫자 틀림
        "978893497246x",     // 문자
        " 9788934972464",    // 공백
        "9788934972464 ",
        "../etc/passwd",
        "9788934972464%00",
        "1288934972464",     // 접두 978/979 아님
        "９７８８９３４９７２４６４", // 전각 숫자
    ];
    for value in cases {
        assert!(!is_valid_isbn13(value), "{value:?}");
    }
}

#[test]
fn generated_isbns_pass_the_validator() {
    for n in [1, 2, 99, 123456789] {
        assert!(is_valid_isbn13(&isbn(n)));
    }
}

// ── 순수 함수: 상품 페이지 파싱 ─────────────────────────────────────────────────

#[test]
fn extracts_og_image_regardless_of_attribute_order_and_quotes() {
    let a = r#"<html><head><meta charset="utf-8"><meta property="og:image" content="https://image.aladin.co.kr/product/1/2/cover500/x.jpg" /></head>"#;
    let b = r#"<meta content='https://image.aladin.co.kr/product/1/2/cover500/y.jpg' property='og:image'>"#;
    let c = r#"<meta  property="og:title" content="x"><meta property="og:image"   content="https://image.aladin.co.kr/product/3/cover500/z.jpg">"#;
    assert_eq!(extract_og_image(a).as_deref(), Some("https://image.aladin.co.kr/product/1/2/cover500/x.jpg"));
    assert_eq!(extract_og_image(b).as_deref(), Some("https://image.aladin.co.kr/product/1/2/cover500/y.jpg"));
    assert_eq!(extract_og_image(c).as_deref(), Some("https://image.aladin.co.kr/product/3/cover500/z.jpg"));
}

#[test]
fn og_image_is_none_when_absent_or_empty() {
    assert_eq!(extract_og_image("<html></html>"), None);
    assert_eq!(extract_og_image(r#"<meta property="og:image" content="">"#), None);
    assert_eq!(extract_og_image(r#"<meta property="og:title" content="x">"#), None);
    assert_eq!(extract_og_image(r#"<meta property="og:image""#), None); // 닫히지 않은 태그
}

// ── 실제 알라딘 페이지에서 받은 조각으로 검증 ────────────────────────────────────
// 2026-11-04에 두 상품 페이지에서 그대로 떠온 조각이다:
//   - 재이(조남주, 장편소설, 종이책, ItemId=401662202)
//   - [전자책] 태풍의 이름을 잊은 것처럼(정세랑, 소설집, ItemId=406072524)
// 실제 마크업을 그대로 테스트에 박아 두면 알라딘이 마크업을 바꿨을 때 이 테스트가 먼저 깨진다.

pub(crate) const JAEI_CATEGORY: &str = r#"<div class="conts_info_list2">
    <ul id="ulCategory">
        <li><a href="https://www.aladin.co.kr/home/wbookmain.aspx">국내도서</a>&nbsp;&gt;&nbsp;<a href="https://www.aladin.co.kr/shop/wbrowse.aspx?CID=1">소설/시/희곡</a>&nbsp;&gt;&nbsp;<a href="https://www.aladin.co.kr/shop/wbrowse.aspx?CID=50917">한국소설</a>&nbsp;&gt;&nbsp;<a href="https://www.aladin.co.kr/shop/wbrowse.aspx?CID=50993&amp;BrowseTarget=List"><b>2000년대 이후 한국소설</b></a> <a class="Ere_sub_gray8 Ere_fs13 foreignCategoryFold" style="display:none;">접기<img src="https://image.aladin.co.kr/img/shop/2018/icon_arrow_fold.png" style="margin-bottom:-1px;"></a></li>
    </ul>
</div>"#;

pub(crate) const JAEI_TITLE: &str = r#"<div><span class="Ere_bo_title">재이</span></div>"#;

pub(crate) const JAEI_BASIC_INFO: &str = r#"<div class="conts_info_list1"><ul><li>양장본</li><li>232쪽</li><li>128*188mm (B6)</li><li>325g</li><li>ISBN : 9791141603649</li></ul></div>"#;

pub(crate) const JAEI_DESCRIPTION_META: &str = r#"<meta name="description" content="소설가 조남주의 신작 장편소설 『재이』를 문학동네에서 펴낸다. 한국 문학과 사회의 바로미터라 불러도 좋을 법한 감각으로 시대의 기분을 정확하게 포착하고, 또 그것을 소설로 지어 독자에게 건네는 그의 네번째...">"#;

// 소설집 쪽은 "장편소설"이라는 낱말이 (다른 맥락으로) 같이 들어 있다는 게 핵심 함정이다:
// "그의 출간작 절반 이상이 장편소설이기에" — 이 책 얘기가 아니라 작가의 이력 얘기다.
const TAEPUNG_DESCRIPTION_META: &str = r#"<meta name="description" content="동시대 한국문학을 대표하는 정세랑 작가의 6년 만의 소설집 『태풍의 이름을 잊은 것처럼』을 문학동네에서 펴낸다. 그의 출간작 절반 이상이 장편소설이기에 또다른 매력이 만개하는 소설집은 귀한 동시에 반갑기 ...">"#;

const TAEPUNG_CATEGORY: &str = r#"<ul id="ulCategory">
                    <li><a href="https://www.aladin.co.kr/home/webookmain.aspx">전자책</a>&nbsp;&gt;&nbsp;<a href="https://www.aladin.co.kr/shop/wbrowse.aspx?CID=38396">소설/시/희곡</a>&nbsp;&gt;&nbsp;<a href="https://www.aladin.co.kr/shop/wbrowse.aspx?CID=56538">한국소설</a>&nbsp;&gt;&nbsp;<a href="https://www.aladin.co.kr/shop/wbrowse.aspx?CID=57637&amp;BrowseTarget=List"><b>2000년대 이후</b></a> <a class="Ere_sub_gray8 Ere_fs13 foreignCategoryFold" style="display:none;">접기<img src="https://image.aladin.co.kr/img/shop/2018/icon_arrow_fold.png" style="margin-bottom:-1px;"></a></li>




                </ul>"#;

#[test]
fn categories_join_hrefed_links_and_skip_the_fold_toggle() {
    assert_eq!(details::extract_categories(JAEI_CATEGORY), vec!["국내도서 > 소설/시/희곡 > 한국소설 > 2000년대 이후 한국소설"]);
    assert_eq!(details::extract_categories(TAEPUNG_CATEGORY), vec!["전자책 > 소설/시/희곡 > 한국소설 > 2000년대 이후"]);
}

#[test]
fn categories_are_empty_when_the_list_is_absent() {
    assert!(details::extract_categories("<html></html>").is_empty());
}

#[test]
fn categories_stop_at_the_fold_toggle_even_when_it_has_an_href() {
    // 운영에서 실제로 겪은 버그: 테스트 조각을 뜰 때는 "접기" 토글에 href가 없었는데, 다른 책 페이지에서는
    // href가 붙어 나와서 href 유무로만 거르면 "...한국소설 > 접기"처럼 분류 경로 끝에 섞여 들어갔다.
    let html = r#"<ul id="ulCategory">
        <li><a href="/a">국내도서</a>&gt;<a href="/b">한국소설</a>
            <a href="javascript:void(0)" class="Ere_sub_gray8 Ere_fs13 foreignCategoryFold">접기<img src="x.png"></a>
        </li>
    </ul>"#;
    assert_eq!(details::extract_categories(html), vec!["국내도서 > 한국소설"]);
}

#[test]
fn form_hint_reads_the_word_directly_before_the_titles_quote() {
    assert_eq!(extract_form_hint(JAEI_DESCRIPTION_META), Some("장편소설"));
    // 함정: 이 설명문에 "장편소설"도 나오지만(작가의 과거작 얘기) 제목 바로 앞은 "소설집"이다.
    assert_eq!(extract_form_hint(TAEPUNG_DESCRIPTION_META), Some("소설집"));
}

#[test]
fn form_hint_is_none_without_a_recognized_word_right_before_the_title() {
    assert_eq!(
        extract_form_hint(r#"<meta name="description" content="어느 작가의 신작 『제목』을 펴낸다.">"#),
        None,
    );
    assert_eq!(extract_form_hint("<html></html>"), None);
}

#[test]
fn product_page_combines_form_hint_categories_and_pages() {
    let html = format!("{JAEI_TITLE}{JAEI_DESCRIPTION_META}{JAEI_CATEGORY}{JAEI_BASIC_INFO}");
    let d = ProductPage::from_html(&html).details;
    assert_eq!(d.title.as_deref(), Some("재이"));
    assert_eq!(d.form_hint.as_deref(), Some("장편소설"));
    assert_eq!(d.categories, vec!["국내도서 > 소설/시/희곡 > 한국소설 > 2000년대 이후 한국소설"]);
    assert_eq!(d.pages, Some(232));
}

#[test]
fn config_only_allows_https_aladin_cover_paths() {
    let config = SourceConfig::aladin();
    assert!(config.allows("https://image.aladin.co.kr/product/31424/4/cover500/k482832219_1.jpg"));
    assert!(!config.allows("http://image.aladin.co.kr/product/1/cover500/a.jpg")); // http
    assert!(!config.allows("https://evil.example/product/1/cover500/a.jpg")); // 다른 호스트
    assert!(!config.allows("https://image.aladin.co.kr.evil.example/product/1/a.jpg")); // 호스트 위장
    assert!(!config.allows("https://image.aladin.co.kr/img/common/logo.png")); // 표지 경로 아님
    assert!(!config.allows("not a url"));
}

#[test]
fn user_agent_names_the_service_and_the_contact() {
    assert_eq!(user_agent("https://gotolib.tyange.com"), "gotolib-covers/0.1 (+https://gotolib.tyange.com)");
}

#[test]
fn contact_falls_back_to_the_public_site_when_unset_or_blank() {
    assert_eq!(contact_from(None), DEFAULT_CONTACT);
    assert_eq!(contact_from(Some("")), DEFAULT_CONTACT);
    assert_eq!(contact_from(Some("   ")), DEFAULT_CONTACT);
    assert_eq!(contact_from(Some(" me@example.com ")), "me@example.com");
}

// ── 수집 작업 (가짜 알라딘 서버) ───────────────────────────────────────────────
// 썸네일 변환·저장 자체의 단위 테스트는 `cover/tests.rs`에 있다. 여기서는 큐 → 페치 → 파싱 → 저장이
// 끝까지 맞물리는지를 가짜 알라딘 서버로 본다.

#[derive(Clone, Copy)]
enum Scenario {
    Ok,
    /// 표지 + 제목·분류·형식어·쪽수(실제 알라딘 조각)까지 있는 종이책 페이지.
    OkWithGenre,
    /// 표지가 있는 전자책 상품 페이지(제목 "[eBook] …", 판본 탭이 전자책에 켜져 있다).
    Ebook,
    NoCover,
    Forbidden,
    ServerError,
    NotAnImage,
    ForeignImage,
}

type Scenarios = Arc<Mutex<HashMap<String, Scenario>>>;

fn scenario_of(scenarios: &Scenarios, isbn: &str) -> Scenario {
    scenarios.lock().unwrap().get(isbn).copied().unwrap_or(Scenario::NoCover)
}

#[handler]
fn fake_page(req: &Request, scenarios: Data<&Scenarios>) -> Response {
    let isbn = req
        .uri()
        .query()
        .and_then(|q| q.strip_prefix("ISBN="))
        .unwrap_or_default()
        .to_string();
    let host = req.header("host").unwrap_or("127.0.0.1").to_string();
    let og = |url: String| format!(r#"<html><head><meta property="og:image" content="{url}"></head></html>"#);
    match scenario_of(&scenarios, &isbn) {
        Scenario::Forbidden => Response::builder().status(StatusCode::FORBIDDEN).finish(),
        Scenario::ServerError => Response::builder().status(StatusCode::INTERNAL_SERVER_ERROR).finish(),
        Scenario::NoCover => Response::builder().body("<html><head></head></html>"),
        Scenario::ForeignImage => Response::builder().body(og("http://evil.example/product/1/x.jpg".to_string())),
        Scenario::Ok | Scenario::NotAnImage => {
            Response::builder().body(og(format!("http://{host}/product/1/cover500/{isbn}.jpg")))
        }
        Scenario::OkWithGenre => Response::builder().body(format!(
            r#"<html><head>
                <meta property="og:image" content="http://{host}/product/1/cover500/{isbn}.jpg">
                {JAEI_DESCRIPTION_META}
            </head><body>{JAEI_TITLE}{JAEI_CATEGORY}{JAEI_BASIC_INFO}</body></html>"#
        )),
        Scenario::Ebook => Response::builder().body(format!(
            r#"<html><head><meta property="og:image" content="http://{host}/product/1/cover500/{isbn}.jpg"></head>
            <body><span class="Ere_bo_title">[eBook] 재이</span>
            <div class="book_tab_box"><span class="tab_on">전자책</span><span class="tab_off"><a href="/x">종이책</a></span></div>
            {TAEPUNG_CATEGORY}</body></html>"#
        )),
    }
}

#[handler]
fn fake_image(Path(name): Path<String>, scenarios: Data<&Scenarios>) -> Response {
    let isbn = name.trim_end_matches(".jpg");
    match scenario_of(&scenarios, isbn) {
        Scenario::NotAnImage => Response::builder().content_type("text/html").body("nope"),
        _ => Response::builder().content_type("image/jpeg").body(jpeg(600, 900)),
    }
}

struct Fixture {
    pool: SqlitePool,
    source: CoverSource,
    dir: PathBuf,
    scenarios: Scenarios,
}

async fn fixture() -> Fixture {
    let scenarios: Scenarios = Arc::default();
    let app = Route::new()
        .at("/shop/wproduct.aspx", get(fake_page))
        .at("/product/1/cover500/:name", get(fake_image))
        .data(scenarios.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(Server::new_with_acceptor(TcpAcceptor::from_tokio(listener).unwrap()).run(app));

    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    init_db(&pool).await.unwrap();
    let config = SourceConfig {
        page_base: format!("http://127.0.0.1:{port}"),
        image_hosts: vec!["127.0.0.1".to_string()],
        allow_http: true,
    };
    Fixture {
        pool,
        source: CoverSource::new(config, "gotolib-test").unwrap(),
        dir: temp_dir(),
        scenarios,
    }
}

impl Fixture {
    async fn add(&self, isbn: &str, scenario: Scenario) {
        self.scenarios.lock().unwrap().insert(isbn.to_string(), scenario);
        enqueue(&self.pool, &[isbn.to_string()], 1_000).await.unwrap();
    }

    async fn step(&self, now: i64) -> Step {
        process_next(&self.pool, &self.source, &self.dir, now).await
    }

    async fn status(&self, isbn: &str) -> String {
        query_scalar("SELECT status FROM book_covers WHERE isbn13 = ?")
            .bind(isbn)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    async fn field(&self, isbn: &str, column: &str) -> i64 {
        query_scalar(&format!("SELECT {column} FROM book_covers WHERE isbn13 = ?"))
            .bind(isbn)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    async fn text_field(&self, isbn: &str, column: &str) -> Option<String> {
        query_scalar(&format!("SELECT {column} FROM book_covers WHERE isbn13 = ?"))
            .bind(isbn)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }

    async fn info(&self, isbn: &str) -> BookInfoRow {
        book_infos(&self.pool, &[isbn.to_string()]).await.unwrap().remove(isbn).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn idle_when_queue_is_empty() {
    let f = fixture().await;
    assert_eq!(f.step(2_000).await, Step::Idle);
}

#[tokio::test]
async fn found_cover_is_downloaded_shrunk_and_recorded() {
    let f = fixture().await;
    let id = isbn(1);
    f.add(&id, Scenario::Ok).await;

    assert_eq!(f.step(2_000).await, Step::Processed);
    assert_eq!(f.status(&id).await, STATUS_OK);
    let written = std::fs::read(cover_path(&f.dir, &id).unwrap()).unwrap();
    let decoded = image::load_from_memory(&written).unwrap();
    assert!(decoded.width() <= THUMB_WIDTH);
    assert_eq!(f.field(&id, "fetched_at").await, 2_000);
    // 처리한 건은 큐에서 빠진다
    assert_eq!(f.step(3_000).await, Step::Idle);
}

#[tokio::test]
async fn paper_book_page_records_its_details() {
    let f = fixture().await;
    let id = isbn(20);
    f.add(&id, Scenario::OkWithGenre).await;

    assert_eq!(f.step(2_000).await, Step::Processed);
    assert_eq!(f.status(&id).await, STATUS_OK);
    assert_eq!(f.field(&id, "is_ebook").await, 0);
    let row = f.info(&id).await;
    let d = row.details.expect("종이책이면 상세가 쌓인다");
    assert_eq!(d.title.as_deref(), Some("재이"));
    assert_eq!(d.categories, vec!["국내도서 > 소설/시/희곡 > 한국소설 > 2000년대 이후 한국소설"]);
    assert_eq!(d.form_hint.as_deref(), Some("장편소설"));
    assert_eq!(d.pages, Some(232));
    assert_eq!(d.extra_info, vec!["양장본"]);
    // 예전 열에는 더 쓰지 않는다
    assert_eq!(row.legacy_category_path, None);
}

#[tokio::test]
async fn ebook_page_keeps_its_cover_but_not_its_details() {
    let f = fixture().await;
    let id = isbn(21);
    f.add(&id, Scenario::Ebook).await;

    assert_eq!(f.step(2_000).await, Step::Processed);
    assert_eq!(f.status(&id).await, STATUS_OK, "서재의 전자책 표지는 계속 쓴다");
    assert!(cover_path(&f.dir, &id).unwrap().exists());
    assert_eq!(f.field(&id, "is_ebook").await, 1);
    assert!(f.info(&id).await.details.is_none(), "전자책 상세는 쌓지 않는다");
}

/// 상세 테이블을 만들기 전에 `ok`가 된 행을 흉내 낸다(표지는 이미 있고, 예전 열에 분류가 있고, is_ebook은 비어 있다).
async fn legacy_ok_row(f: &Fixture, id: &str, scenario: Scenario) {
    f.scenarios.lock().unwrap().insert(id.to_string(), scenario);
    query(
        "INSERT INTO book_covers (isbn13, status, source_url, category_path, attempts, next_try_at, fetched_at, created_at)
         VALUES (?, ?, 'https://image.aladin.co.kr/product/old.jpg', '옛 분류', 0, 0, 1, 1)",
    )
    .bind(id)
    .bind(STATUS_OK)
    .execute(&f.pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn old_ok_rows_get_details_filled_without_touching_status_or_cover() {
    let f = fixture().await;
    let id = isbn(22);
    legacy_ok_row(&f, &id, Scenario::OkWithGenre).await;

    assert_eq!(f.step(2_000).await, Step::Processed);
    assert_eq!(f.status(&id).await, STATUS_OK);
    assert_eq!(
        f.text_field(&id, "source_url").await.as_deref(),
        Some("https://image.aladin.co.kr/product/old.jpg"),
        "표지를 다시 받지 않는다",
    );
    assert!(!cover_path(&f.dir, &id).unwrap().exists(), "이미지를 내려받지 않았다");
    assert_eq!(f.info(&id).await.details.unwrap().title.as_deref(), Some("재이"));
    // 한 번 채우면 다시 보충 대상이 아니다
    assert_eq!(f.step(3_000).await, Step::Idle);
}

#[tokio::test]
async fn old_ebook_rows_are_marked_but_get_no_details() {
    let f = fixture().await;
    let id = isbn(23);
    legacy_ok_row(&f, &id, Scenario::Ebook).await;

    assert_eq!(f.step(2_000).await, Step::Processed);
    assert_eq!(f.field(&id, "is_ebook").await, 1);
    let row = f.info(&id).await;
    assert!(row.details.is_none());
    assert_eq!(row.legacy_category_path.as_deref(), Some("옛 분류"), "예전에 모은 값은 그대로 남는다");
    assert_eq!(f.step(3_000).await, Step::Idle);
}

#[tokio::test]
async fn filling_old_rows_waits_for_the_regular_queue() {
    let f = fixture().await;
    let old = isbn(24);
    let new = isbn(25);
    legacy_ok_row(&f, &old, Scenario::OkWithGenre).await;
    f.add(&new, Scenario::Ok).await;

    assert_eq!(f.step(2_000).await, Step::Processed);
    assert_eq!(f.status(&new).await, STATUS_OK, "새 대기 행이 먼저다");
    assert!(f.info(&old).await.details.is_none());
}

#[tokio::test]
async fn failing_to_fill_an_old_row_postpones_it_but_keeps_it_ok() {
    let f = fixture().await;
    let id = isbn(26);
    legacy_ok_row(&f, &id, Scenario::ServerError).await;

    assert_eq!(f.step(2_000).await, Step::Processed);
    assert_eq!(f.status(&id).await, STATUS_OK, "일시 오류로 서재 표지가 사라지면 안 된다");
    assert_eq!(f.field(&id, "next_try_at").await, 2_000 + REFRESH_RETRY_SECS);
    assert_eq!(f.step(2_001).await, Step::Idle);
}

#[tokio::test]
async fn page_without_cover_becomes_missing_with_long_retry() {
    let f = fixture().await;
    let id = isbn(2);
    f.add(&id, Scenario::NoCover).await;

    assert_eq!(f.step(2_000).await, Step::Processed);
    assert_eq!(f.status(&id).await, STATUS_MISSING);
    assert_eq!(f.field(&id, "next_try_at").await, 2_000 + MISSING_RETRY_SECS);
    assert!(!cover_path(&f.dir, &id).unwrap().exists());
}

#[tokio::test]
async fn forbidden_blocks_the_whole_worker_and_leaves_the_row_untouched() {
    let f = fixture().await;
    let blocked = isbn(3);
    let other = isbn(4);
    f.add(&blocked, Scenario::Forbidden).await;
    f.add(&other, Scenario::Ok).await;

    assert_eq!(f.step(2_000).await, Step::Blocked);
    assert_eq!(f.status(&blocked).await, STATUS_PENDING);
    assert_eq!(f.field(&blocked, "attempts").await, 0);
    // 막힘은 행 하나의 실패가 아니라서 아무것도 기록하지 않는다(다음 행도 그대로)
    assert_eq!(f.status(&other).await, STATUS_PENDING);
}

#[tokio::test]
async fn server_error_backs_off_and_eventually_gives_up() {
    let f = fixture().await;
    let id = isbn(5);
    f.add(&id, Scenario::ServerError).await;

    let mut now = 2_000;
    assert_eq!(f.step(now).await, Step::Processed);
    assert_eq!(f.status(&id).await, STATUS_ERROR);
    assert_eq!(f.field(&id, "attempts").await, 1);
    let first_retry = f.field(&id, "next_try_at").await;
    assert!(first_retry > now);

    // 아직 때가 아니면 건드리지 않는다
    assert_eq!(f.step(now + 1).await, Step::Idle);

    // 간격은 점점 늘어난다
    now = first_retry;
    assert_eq!(f.step(now).await, Step::Processed);
    let second_retry = f.field(&id, "next_try_at").await;
    assert!(second_retry - now > first_retry - 2_000);

    // 반복 실패하면 missing으로 두고 한참 뒤에 본다
    for _ in 0..(MAX_ATTEMPTS - 2) {
        now = f.field(&id, "next_try_at").await;
        assert_eq!(f.step(now).await, Step::Processed);
    }
    assert_eq!(f.status(&id).await, STATUS_MISSING);
    assert_eq!(f.field(&id, "next_try_at").await, now + EXHAUSTED_RETRY_SECS);
}

#[tokio::test]
async fn non_image_response_is_treated_as_missing() {
    let f = fixture().await;
    let id = isbn(6);
    f.add(&id, Scenario::NotAnImage).await;
    assert_eq!(f.step(2_000).await, Step::Processed);
    assert_eq!(f.status(&id).await, STATUS_MISSING);
    assert!(!cover_path(&f.dir, &id).unwrap().exists());
}

#[tokio::test]
async fn cover_url_on_a_foreign_host_is_never_fetched() {
    let f = fixture().await;
    let id = isbn(9);
    f.add(&id, Scenario::ForeignImage).await;
    assert_eq!(f.step(2_000).await, Step::Processed);
    assert_eq!(f.status(&id).await, STATUS_MISSING);
}

#[tokio::test]
async fn queue_is_processed_oldest_first_one_at_a_time() {
    let f = fixture().await;
    let (a, b) = (isbn(10), isbn(11));
    f.scenarios.lock().unwrap().insert(a.clone(), Scenario::Ok);
    f.scenarios.lock().unwrap().insert(b.clone(), Scenario::Ok);
    enqueue(&f.pool, std::slice::from_ref(&a), 100).await.unwrap();
    enqueue(&f.pool, std::slice::from_ref(&b), 200).await.unwrap();

    assert_eq!(f.step(2_000).await, Step::Processed);
    assert_eq!(f.status(&a).await, STATUS_OK);
    assert_eq!(f.status(&b).await, STATUS_PENDING);
}

// ── DB 도우미 ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn enqueue_does_not_touch_existing_rows() {
    let f = fixture().await;
    let id = isbn(12);
    f.add(&id, Scenario::Ok).await;
    f.step(2_000).await;
    assert_eq!(enqueue(&f.pool, std::slice::from_ref(&id), 9_999).await.unwrap(), 0);
    assert_eq!(f.status(&id).await, STATUS_OK);
}

#[tokio::test]
async fn stale_missing_rows_return_to_the_queue_but_fresh_ones_do_not() {
    let f = fixture().await;
    let (stale, fresh) = (isbn(13), isbn(14));
    f.add(&stale, Scenario::NoCover).await;
    f.add(&fresh, Scenario::NoCover).await;
    f.step(2_000).await;
    f.step(2_000).await;
    let retry_at = 2_000 + MISSING_RETRY_SECS;
    assert_eq!(f.status(&stale).await, STATUS_MISSING);
    assert_eq!(f.status(&fresh).await, STATUS_MISSING);

    // 다시 볼 때가 되기 전에는 건드리지 않는다
    requeue_stale_missing(&f.pool, &[stale.clone(), fresh.clone()], retry_at - 1).await.unwrap();
    assert_eq!(f.status(&stale).await, STATUS_MISSING);

    // fresh는 더 최근에 확인한 것으로 만들고, 때가 된 시각에 부르면 stale만 돌아온다
    query("UPDATE book_covers SET next_try_at = ? WHERE isbn13 = ?")
        .bind(retry_at + 10_000)
        .bind(&fresh)
        .execute(&f.pool)
        .await
        .unwrap();
    requeue_stale_missing(&f.pool, &[stale.clone(), fresh.clone()], retry_at).await.unwrap();
    assert_eq!(f.status(&stale).await, STATUS_PENDING);
    assert_eq!(f.status(&fresh).await, STATUS_MISSING);
}
