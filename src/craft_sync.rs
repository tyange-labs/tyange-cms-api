//! Craft 폴더 → 포스트 동기화.
//!
//! Craft의 지정한 폴더(기본: `CMS`)에 문서를 쓰면 lovelog 짧은 글로 올린다.
//! Craft Connect API에는 웹훅이 없어서 일정 간격으로 폴더를 읽는다(폴링). 수동으로 지금 바로
//! 돌리는 길은 `POST /admin/craft/sync`다.
//!
//! - 새 문서 → 글 생성(`CRAFT_SYNC_STATUS`, 기본 published)
//! - `lastModifiedAt`이 바뀐 문서 → **본문만** 갱신. 태그·제목·상태는 CMS에서 바꾼 대로 둔다.
//! - 폴더에서 사라진 문서(삭제·이동) → 글을 draft로 돌린다. 글 자체는 지우지 않는다.
//! - 이미지·드로잉 블록은 내려받아 이 서버에 저장하고 본문의 주소를 바꾼다(Craft 주소는 쓰지 않는다).
//!
//! `CRAFT_API_URL`(링크 자체가 접근 권한이다)은 로그·에러·응답 어디에도 내지 않는다.

use std::{
    collections::{HashMap, HashSet},
    env,
    io::Cursor,
    path::PathBuf,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};

use chrono::{DateTime, FixedOffset, Utc};
use poem_openapi::Object;
use serde::{Deserialize, Serialize};
use sqlx::{query, query_as, query_scalar, SqlitePool};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::services::image_storage::{
    store_image_from_reader, ImageFormat, ImageRecord, ImageUploadPolicy,
};

/// lovelog `src/lib/timeline.ts`의 SHORT_FORM_SINCE/SHORT_FORM_LIMIT, tyange-cms `app/utils/short-form.ts`와
/// 같은 값이다. **함께 고친다.**
const SHORT_FORM_SINCE: &str = "2026-09-23";
const SHORT_FORM_LIMIT: usize = 200;

const DEFAULT_INTERVAL_SECS: u64 = 300;
const MIN_INTERVAL_SECS: u64 = 60;
const DEFAULT_IMAGE_BASE_URL: &str = "https://tyange.com/api/cms";

/// 인증된 이미지 업로드(`openapi.rs`의 AUTHENTICATED_UPLOAD_POLICY)와 같은 값.
const IMAGE_POLICY: ImageUploadPolicy = ImageUploadPolicy {
    max_bytes: 10 * 1024 * 1024,
    allowed_formats: &[
        ImageFormat::Png,
        ImageFormat::Jpeg,
        ImageFormat::Gif,
        ImageFormat::Webp,
    ],
};

#[derive(Clone, Debug)]
pub struct CraftSyncConfig {
    /// Craft Connect API 링크. 그 자체가 비밀이다.
    pub api_url: String,
    pub folder_id: String,
    pub interval: Duration,
    /// 본문 이미지의 공개 주소 앞부분(저장된 `/images/…` 앞에 붙는다).
    pub image_base_url: String,
    pub upload_dir: PathBuf,
    /// 글 작성자. 비어 있으면 첫 admin 계정.
    pub writer_id: Option<String>,
    pub tag: String,
    pub tag_category: String,
    pub new_status: String,
}

impl CraftSyncConfig {
    /// `CRAFT_SYNC_ENABLED=true`이고 `CRAFT_API_URL`·`CRAFT_SYNC_FOLDER_ID`가 있을 때만 켠다.
    pub fn from_env() -> Option<Self> {
        if env::var("CRAFT_SYNC_ENABLED")
            .map(|v| v != "true")
            .unwrap_or(true)
        {
            println!("[craft-sync] 꺼져 있습니다(CRAFT_SYNC_ENABLED).");
            return None;
        }
        let non_empty = |key: &str| env::var(key).ok().filter(|v| !v.trim().is_empty());
        let Some(api_url) = non_empty("CRAFT_API_URL") else {
            println!("[craft-sync] CRAFT_API_URL이 없어 꺼 둡니다.");
            return None;
        };
        let Some(folder_id) = non_empty("CRAFT_SYNC_FOLDER_ID") else {
            println!("[craft-sync] CRAFT_SYNC_FOLDER_ID가 없어 꺼 둡니다.");
            return None;
        };
        let interval_secs = non_empty("CRAFT_SYNC_INTERVAL_SECS")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(DEFAULT_INTERVAL_SECS)
            .max(MIN_INTERVAL_SECS);
        Some(Self {
            api_url: api_url.trim().trim_end_matches('/').to_string(),
            folder_id: folder_id.trim().to_string(),
            interval: Duration::from_secs(interval_secs),
            image_base_url: non_empty("CRAFT_SYNC_IMAGE_BASE_URL")
                .unwrap_or_else(|| DEFAULT_IMAGE_BASE_URL.to_string())
                .trim_end_matches('/')
                .to_string(),
            upload_dir: PathBuf::from(
                env::var("UPLOAD_PATH").unwrap_or_else(|_| ".uploads/images".to_string()),
            ),
            writer_id: non_empty("CRAFT_SYNC_WRITER_ID"),
            tag: "lovelog".to_string(),
            tag_category: "general".to_string(),
            new_status: non_empty("CRAFT_SYNC_STATUS").unwrap_or_else(|| "published".to_string()),
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Object)]
pub struct SyncedPost {
    pub craft_doc_id: String,
    pub title: String,
    pub post_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Object)]
pub struct SyncReport {
    pub started_at: String,
    pub finished_at: String,
    pub created: Vec<SyncedPost>,
    pub updated: Vec<SyncedPost>,
    /// 폴더에서 사라져 draft로 돌린 글.
    pub drafted: Vec<SyncedPost>,
    /// 바뀐 게 없어 건너뛴 문서 수.
    pub unchanged: u32,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

pub struct CraftSyncService {
    config: Option<CraftSyncConfig>,
    http: reqwest::Client,
    run_lock: Mutex<()>,
    last_report: StdMutex<Option<SyncReport>>,
}

// ── Craft 응답 ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct DocumentList {
    items: Vec<CraftDocument>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CraftDocument {
    id: String,
    #[serde(default)]
    title: String,
    last_modified_at: String,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct Block {
    #[serde(default)]
    id: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    markdown: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    content: Vec<Block>,
}

enum Part {
    Text(String),
    Image { block_id: String, url: String },
}

/// 블록 트리를 위에서 아래로 훑어 본문 조각을 모은다. 루트(페이지) 자신의 `markdown`은 제목이라 뺀다.
fn collect_parts(root: &Block, warnings: &mut Vec<String>) -> Vec<Part> {
    fn walk(block: &Block, parts: &mut Vec<Part>, warnings: &mut Vec<String>) {
        match (block.kind.as_str(), &block.url) {
            ("image" | "drawing", Some(url)) => parts.push(Part::Image {
                block_id: block.id.clone(),
                url: url.clone(),
            }),
            ("file" | "video", _) => {
                let note = format!("지원하지 않는 블록({})은 건너뜁니다.", block.kind);
                if !warnings.contains(&note) {
                    warnings.push(note);
                }
            }
            _ => {
                let text = block.markdown.trim();
                if !text.is_empty() {
                    parts.push(Part::Text(text.to_string()));
                }
            }
        }
        for child in &block.content {
            walk(child, parts, warnings);
        }
    }

    let mut parts = Vec::new();
    for child in &root.content {
        walk(child, &mut parts, warnings);
    }
    parts
}

// ── 글자 수(lovelog `plainLength`와 같은 규칙) ─────────────────────────────────

fn find_char(chars: &[char], from: usize, target: char) -> Option<usize> {
    (from..chars.len()).find(|&i| chars[i] == target)
}

/// `![alt](url)`은 통째로 지우고 `[text](url)`은 text만 남긴다.
fn strip_links(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let is_image = chars[i] == '!' && chars.get(i + 1) == Some(&'[');
        let open = if is_image { i + 1 } else { i };
        if chars[open] == '[' {
            if let Some(close) = find_char(&chars, open + 1, ']') {
                if chars.get(close + 1) == Some(&'(') {
                    if let Some(end) = find_char(&chars, close + 2, ')') {
                        if !is_image {
                            out.extend(&chars[open + 1..close]);
                        }
                        i = end + 1;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn strip_line_prefix(line: &str) -> &str {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && line[hashes..].starts_with(char::is_whitespace) {
        return line[hashes..].trim_start();
    }
    if let Some(rest) = line.strip_prefix('>') {
        return rest.strip_prefix(' ').unwrap_or(rest);
    }
    if let Some(rest) = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("+ "))
    {
        return rest.trim_start();
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && line[digits..].starts_with(". ") {
        return line[digits + 2..].trim_start();
    }
    line
}

pub fn plain_len(markdown: &str) -> usize {
    let stripped = strip_links(markdown);
    let joined = stripped
        .lines()
        .map(strip_line_prefix)
        .collect::<Vec<_>>()
        .join("\n");
    joined
        .chars()
        .filter(|c| !matches!(c, '`' | '*' | '_' | '~'))
        .collect::<String>()
        .trim()
        .chars()
        .count()
}

/// 짧은 글 규칙(200자)을 넘으면 사유를 돌려준다. 이 경우 published로 두지 않는다.
fn short_form_violation(published_at: &str, content: &str) -> Option<String> {
    if published_at < SHORT_FORM_SINCE {
        return None;
    }
    let len = plain_len(content);
    (len > SHORT_FORM_LIMIT).then(|| {
        format!("{len}자라 짧은 글 한도({SHORT_FORM_LIMIT}자)를 넘어 draft로 둡니다.")
    })
}

fn seoul_date(iso: &str) -> Result<String, String> {
    let parsed = DateTime::parse_from_rfc3339(iso).map_err(|e| format!("날짜를 읽지 못함: {e}"))?;
    let seoul = FixedOffset::east_opt(9 * 3600).expect("valid offset");
    Ok(parsed.with_timezone(&seoul).format("%Y-%m-%d").to_string())
}

// ── DB ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, sqlx::FromRow)]
struct Mapping {
    craft_doc_id: String,
    post_id: String,
    last_modified_at: String,
    removed: i64,
}

async fn load_mappings(db: &SqlitePool) -> Result<HashMap<String, Mapping>, String> {
    let rows: Vec<Mapping> = query_as(
        "SELECT craft_doc_id, post_id, last_modified_at, removed FROM craft_sync_posts",
    )
    .fetch_all(db)
    .await
    .map_err(|e| format!("연결 정보를 읽지 못함: {e}"))?;
    Ok(rows
        .into_iter()
        .map(|m| (m.craft_doc_id.clone(), m))
        .collect())
}

async fn resolve_writer(config: &CraftSyncConfig, db: &SqlitePool) -> Result<String, String> {
    if let Some(id) = &config.writer_id {
        return Ok(id.clone());
    }
    query_scalar::<_, String>(
        "SELECT user_id FROM users WHERE user_role = 'admin' ORDER BY rowid LIMIT 1",
    )
    .fetch_optional(db)
    .await
    .map_err(|e| format!("작성자를 찾지 못함: {e}"))?
    .ok_or_else(|| "admin 계정이 없어 작성자를 정할 수 없습니다(CRAFT_SYNC_WRITER_ID).".to_string())
}

async fn attach_tag(
    tx: &mut sqlx::SqliteConnection,
    post_id: &str,
    tag: &str,
    category: &str,
) -> Result<(), sqlx::Error> {
    query("INSERT OR IGNORE INTO tags (name, category) VALUES (?, ?)")
        .bind(tag)
        .bind(category)
        .execute(&mut *tx)
        .await?;
    query("INSERT INTO post_tags (post_id, tag_id) SELECT ?, tag_id FROM tags WHERE name = ?")
        .bind(post_id)
        .bind(tag)
        .execute(&mut *tx)
        .await?;
    Ok(())
}

// ── 서비스 ───────────────────────────────────────────────────────────────────

impl CraftSyncService {
    pub fn from_env() -> Arc<Self> {
        Self::new(CraftSyncConfig::from_env())
    }

    pub fn new(config: Option<CraftSyncConfig>) -> Arc<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("tyange-cms-api/craft-sync")
            .build()
            .expect("reqwest client");
        Arc::new(Self {
            config,
            http,
            run_lock: Mutex::new(()),
            last_report: StdMutex::new(None),
        })
    }

    pub fn is_enabled(&self) -> bool {
        self.config.is_some()
    }

    pub fn last_report(&self) -> Option<SyncReport> {
        self.last_report.lock().ok().and_then(|r| r.clone())
    }

    /// Craft 폴더를 한 번 훑어 포스트와 맞춘다. 동시에 두 번 돌지 않는다(뒤에 온 쪽이 기다린다).
    pub async fn sync_once(&self, db: &SqlitePool) -> Result<SyncReport, String> {
        let config = self
            .config
            .as_ref()
            .ok_or_else(|| "Craft 동기화가 꺼져 있습니다.".to_string())?;
        let _running = self.run_lock.lock().await;

        let mut report = SyncReport {
            started_at: Utc::now().to_rfc3339(),
            ..Default::default()
        };
        let outcome = self.run(config, db, &mut report).await;
        report.finished_at = Utc::now().to_rfc3339();
        if let Err(e) = &outcome {
            report.errors.push(e.clone());
        }
        if let Ok(mut slot) = self.last_report.lock() {
            *slot = Some(report.clone());
        }
        outcome.map(|()| report)
    }

    async fn run(
        &self,
        config: &CraftSyncConfig,
        db: &SqlitePool,
        report: &mut SyncReport,
    ) -> Result<(), String> {
        let docs = self.list_documents(config).await?;
        let mappings = load_mappings(db).await?;
        let writer = resolve_writer(config, db).await?;

        for doc in &docs {
            let result = match mappings.get(&doc.id) {
                None => self.create_post(config, db, &writer, doc, report).await,
                Some(m) if m.removed == 0 && m.last_modified_at == doc.last_modified_at => {
                    report.unchanged += 1;
                    Ok(())
                }
                Some(m) => self.update_post(config, db, doc, m, report).await,
            };
            if let Err(e) = result {
                // 이 문서만 건너뛴다. 연결 정보를 갱신하지 않았으니 다음 라운드에 다시 시도한다.
                report.errors.push(format!("{} ({}): {e}", doc.title, doc.id));
            }
        }

        let present: HashSet<&str> = docs.iter().map(|d| d.id.as_str()).collect();
        let gone: Vec<&Mapping> = mappings
            .values()
            .filter(|m| m.removed == 0 && !present.contains(m.craft_doc_id.as_str()))
            .collect();
        if docs.is_empty() && !gone.is_empty() {
            // API 오류나 폴더 오인으로 빈 목록이 와도 글이 한꺼번에 내려가지 않게 한다.
            report.warnings.push(format!(
                "Craft 폴더가 비어 있어 글 {}개의 draft 전환을 건너뜁니다. 정말 지운 거라면 CMS에서 직접 draft로 바꾸세요.",
                gone.len()
            ));
        } else {
            for m in gone {
                match self.draft_removed(db, m).await {
                    Ok(()) => report.drafted.push(SyncedPost {
                        craft_doc_id: m.craft_doc_id.clone(),
                        title: String::new(),
                        post_id: m.post_id.clone(),
                    }),
                    Err(e) => report.errors.push(format!("{}: {e}", m.craft_doc_id)),
                }
            }
        }
        Ok(())
    }

    // ── Craft 호출 ───────────────────────────────────────────────────────────

    /// 링크가 에러 문구에 새지 않도록 reqwest 에러에서 URL을 뺀다.
    async fn craft_get(&self, config: &CraftSyncConfig, path: &str) -> Result<reqwest::Response, String> {
        let response = self
            .http
            .get(format!("{}{}", config.api_url, path))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| format!("Craft 호출 실패: {}", e.without_url()))?;
        if !response.status().is_success() {
            return Err(format!("Craft가 {}로 응답했습니다.", response.status()));
        }
        Ok(response)
    }

    async fn list_documents(&self, config: &CraftSyncConfig) -> Result<Vec<CraftDocument>, String> {
        let path = format!("/documents?folderId={}&fetchMetadata=true", config.folder_id);
        let list: DocumentList = self
            .craft_get(config, &path)
            .await?
            .json()
            .await
            .map_err(|e| format!("문서 목록을 읽지 못함: {}", e.without_url()))?;
        Ok(list.items)
    }

    async fn fetch_blocks(&self, config: &CraftSyncConfig, doc_id: &str) -> Result<Block, String> {
        self.craft_get(config, &format!("/blocks?id={doc_id}&maxDepth=-1"))
            .await?
            .json()
            .await
            .map_err(|e| format!("문서 본문을 읽지 못함: {}", e.without_url()))
    }

    /// 문서를 마크다운으로. 이미지는 이 서버에 올리고 공개 주소로 바꾼다.
    async fn render_content(
        &self,
        config: &CraftSyncConfig,
        db: &SqlitePool,
        doc: &CraftDocument,
        report: &mut SyncReport,
    ) -> Result<String, String> {
        let root = self.fetch_blocks(config, &doc.id).await?;
        let mut notes = Vec::new();
        let parts = collect_parts(&root, &mut notes);
        for note in notes {
            let note = format!("{}: {note}", doc.title);
            if !report.warnings.contains(&note) {
                report.warnings.push(note);
            }
        }

        let mut lines = Vec::with_capacity(parts.len());
        for part in parts {
            lines.push(match part {
                Part::Text(text) => text,
                Part::Image { block_id, url } => {
                    let web_path = self.stored_image(config, db, &block_id, &url).await?;
                    format!("![]({}{})", config.image_base_url, web_path)
                }
            });
        }
        Ok(lines.join("\n\n"))
    }

    /// 같은 이미지 블록은 한 번만 내려받아 저장한다(본문만 고쳤을 때 이미지가 늘어나지 않게).
    async fn stored_image(
        &self,
        config: &CraftSyncConfig,
        db: &SqlitePool,
        block_id: &str,
        url: &str,
    ) -> Result<String, String> {
        if !block_id.is_empty() {
            let cached: Option<String> =
                query_scalar("SELECT image_path FROM craft_sync_images WHERE craft_block_id = ?")
                    .bind(block_id)
                    .fetch_optional(db)
                    .await
                    .map_err(|e| format!("이미지 캐시를 읽지 못함: {e}"))?;
            if let Some(path) = cached {
                return Ok(path);
            }
        }

        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| format!("이미지 내려받기 실패: {}", e.without_url()))?;
        if !response.status().is_success() {
            return Err(format!("이미지 서버가 {}로 응답했습니다.", response.status()));
        }
        if response
            .content_length()
            .is_some_and(|len| len > IMAGE_POLICY.max_bytes as u64)
        {
            return Err("이미지가 너무 큽니다(최대 10MB).".to_string());
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("이미지를 읽지 못함: {}", e.without_url()))?;

        let stored = store_image_from_reader(
            Some("craft".to_string()),
            Cursor::new(bytes.to_vec()),
            &IMAGE_POLICY,
            db,
            ImageRecord {
                post_id: None,
                image_type: "in_post".to_string(),
                uploader_ip: None,
                status: "confirmed",
            },
            &config.upload_dir,
            "/images",
        )
        .await
        .map_err(|e| format!("이미지를 저장하지 못함: {e:?}"))?;

        if !block_id.is_empty() {
            query("INSERT OR REPLACE INTO craft_sync_images (craft_block_id, image_path) VALUES (?, ?)")
                .bind(block_id)
                .bind(&stored.web_path)
                .execute(db)
                .await
                .map_err(|e| format!("이미지 캐시를 저장하지 못함: {e}"))?;
        }
        Ok(stored.web_path)
    }

    // ── 생성·수정·삭제 ───────────────────────────────────────────────────────

    async fn create_post(
        &self,
        config: &CraftSyncConfig,
        db: &SqlitePool,
        writer: &str,
        doc: &CraftDocument,
        report: &mut SyncReport,
    ) -> Result<(), String> {
        let content = self.render_content(config, db, doc, report).await?;
        if content.trim().is_empty() {
            // 쓰는 중인 빈 문서를 글로 올리지 않는다. 내용이 생기면 다음 라운드에 올라간다.
            return Ok(());
        }
        let published_at = seoul_date(&doc.created_at)?;
        let mut status = config.new_status.clone();
        if let Some(reason) = short_form_violation(&published_at, &content) {
            status = "draft".to_string();
            report.warnings.push(format!("{}: {reason}", doc.title));
        }

        let post_id = Uuid::new_v4().to_string();
        let mut tx = db.begin().await.map_err(|e| format!("트랜잭션 시작 실패: {e}"))?;
        query(
            "INSERT INTO posts (post_id, title, description, published_at, content, writer_id, status)
             VALUES (?, '', '', ?, ?, ?, ?)",
        )
        .bind(&post_id)
        .bind(&published_at)
        .bind(&content)
        .bind(writer)
        .bind(&status)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("포스트 저장 실패: {e}"))?;
        attach_tag(&mut tx, &post_id, &config.tag, &config.tag_category)
            .await
            .map_err(|e| format!("태그 저장 실패: {e}"))?;
        query(
            "INSERT OR REPLACE INTO craft_sync_posts (craft_doc_id, post_id, last_modified_at, removed)
             VALUES (?, ?, ?, 0)",
        )
        .bind(&doc.id)
        .bind(&post_id)
        .bind(&doc.last_modified_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("연결 정보 저장 실패: {e}"))?;
        tx.commit().await.map_err(|e| format!("트랜잭션 커밋 실패: {e}"))?;

        println!("[craft-sync] 새 글: {post_id} ({status})");
        report.created.push(SyncedPost {
            craft_doc_id: doc.id.clone(),
            title: doc.title.clone(),
            post_id,
        });
        Ok(())
    }

    async fn update_post(
        &self,
        config: &CraftSyncConfig,
        db: &SqlitePool,
        doc: &CraftDocument,
        mapping: &Mapping,
        report: &mut SyncReport,
    ) -> Result<(), String> {
        let existing: Option<(String, Option<String>)> =
            query_as("SELECT status, content FROM posts WHERE post_id = ?")
                .bind(&mapping.post_id)
                .fetch_optional(db)
                .await
                .map_err(|e| format!("기존 글을 읽지 못함: {e}"))?;
        let Some((old_status, old_content)) = existing else {
            // CMS에서 지운 글을 Craft 쪽 문서가 되살리지 않게 한다.
            report.warnings.push(format!(
                "{}: 연결된 글({})이 CMS에 없어 건너뜁니다.",
                doc.title, mapping.post_id
            ));
            return Ok(());
        };

        let content = self.render_content(config, db, doc, report).await?;
        let restored = mapping.removed != 0;
        let mut status = if restored {
            config.new_status.clone()
        } else {
            old_status.clone()
        };
        if content.trim().is_empty() {
            status = "draft".to_string();
            report
                .warnings
                .push(format!("{}: 본문이 비어 draft로 돌립니다.", doc.title));
        } else if status == "published" {
            if let Some(reason) = short_form_violation(&seoul_date(&doc.created_at)?, &content) {
                status = "draft".to_string();
                report.warnings.push(format!("{}: {reason}", doc.title));
            }
        }

        let changed = old_content.as_deref() != Some(content.as_str()) || status != old_status;
        let mut tx = db.begin().await.map_err(|e| format!("트랜잭션 시작 실패: {e}"))?;
        if changed {
            query("UPDATE posts SET content = ?, status = ? WHERE post_id = ?")
                .bind(&content)
                .bind(&status)
                .bind(&mapping.post_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| format!("포스트 갱신 실패: {e}"))?;
        }
        query("UPDATE craft_sync_posts SET last_modified_at = ?, removed = 0 WHERE craft_doc_id = ?")
            .bind(&doc.last_modified_at)
            .bind(&doc.id)
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("연결 정보 갱신 실패: {e}"))?;
        tx.commit().await.map_err(|e| format!("트랜잭션 커밋 실패: {e}"))?;

        if changed {
            println!("[craft-sync] 글 갱신: {} ({status})", mapping.post_id);
            report.updated.push(SyncedPost {
                craft_doc_id: doc.id.clone(),
                title: doc.title.clone(),
                post_id: mapping.post_id.clone(),
            });
        } else {
            report.unchanged += 1;
        }
        Ok(())
    }

    async fn draft_removed(&self, db: &SqlitePool, mapping: &Mapping) -> Result<(), String> {
        let mut tx = db.begin().await.map_err(|e| format!("트랜잭션 시작 실패: {e}"))?;
        query("UPDATE posts SET status = 'draft' WHERE post_id = ?")
            .bind(&mapping.post_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("draft 전환 실패: {e}"))?;
        query("UPDATE craft_sync_posts SET removed = 1 WHERE craft_doc_id = ?")
            .bind(&mapping.craft_doc_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("연결 정보 갱신 실패: {e}"))?;
        tx.commit().await.map_err(|e| format!("트랜잭션 커밋 실패: {e}"))?;
        println!("[craft-sync] Craft에서 사라져 draft로: {}", mapping.post_id);
        Ok(())
    }
}

/// 일정 간격으로 동기화한다. 작업이 패닉으로 죽으면 감독 루프가 잠시 뒤 다시 띄운다.
pub fn spawn_worker(pool: SqlitePool, service: Arc<CraftSyncService>) {
    let Some(interval) = service.config.as_ref().map(|c| c.interval) else {
        return;
    };
    println!(
        "[craft-sync] {}초마다 Craft 폴더를 확인합니다.",
        interval.as_secs()
    );
    tokio::spawn(async move {
        loop {
            let (pool, service) = (pool.clone(), service.clone());
            let handle = tokio::spawn(async move {
                loop {
                    if let Err(e) = service.sync_once(&pool).await {
                        eprintln!("[craft-sync] 동기화 실패: {e}");
                    }
                    tokio::time::sleep(interval).await;
                }
            });
            if let Err(e) = handle.await {
                eprintln!("[craft-sync] 작업이 비정상 종료했습니다: {e}. 1분 뒤 다시 시작합니다.");
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
}

#[cfg(test)]
mod tests;
