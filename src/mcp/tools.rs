//! Claude가 부르는 CMS 도구. 글 저장 로직과 권한 확인은 HTTP 라우트와 같은 `crate::posts`를 쓴다.
//! 도구 결과는 Claude가 읽기 쉬운 JSON 텍스트로 돌려주고, 실패는 프로토콜 오류가 아니라 `is_error` 결과로
//! 돌려줘서 Claude가 이유를 보고 고쳐 다시 시도할 수 있게 한다.

use std::sync::Arc;

use poem::http::request::Parts;
use rmcp::handler::server::common::Extension;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerConfig};
use rmcp::{schemars, tool, tool_handler, tool_router, ErrorData, ServerHandler};
use serde::{Deserialize, Serialize};
use sqlx::{query, Row};
use tyange_cms_api::auth::authorization::AuthenticatedUser;

use super::images::{ImageRehoster, RehostReport};
use crate::models::{AppState, Tag, UpdatePostRequest, UploadPostRequest};
use crate::utils::parse_tags;

const DEFAULT_LIST_LIMIT: u32 = 20;
const MAX_LIST_LIMIT: u32 = 100;
const EXCERPT_CHARS: usize = 120;
const CONFIRM_PREVIEW_CHARS: usize = 30;

#[derive(Clone)]
pub struct CmsTools {
    state: Arc<AppState>,
    images: Arc<ImageRehoster>,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListPostsArgs {
    /// `published`(공개), `draft`(초안), `all`(기본).
    pub status: Option<String>,
    /// 제목·본문에 이 글자가 들어간 글만.
    pub query: Option<String>,
    /// 최대 개수(기본 20, 최대 100). 최신순.
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PostIdArgs {
    pub post_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TagArg {
    /// 태그 이름. 예: `rust`, `lovelog`.
    pub tag: String,
    /// 태그 분류. 예: `dev`, `general`. 기존 태그는 `list_tags`로 확인할 수 있다.
    pub category: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreatePostArgs {
    pub title: String,
    /// 마크다운 본문.
    pub content: String,
    /// 목록에 보이는 짧은 설명. 없으면 빈 문자열.
    pub description: Option<String>,
    /// `YYYY-MM-DD`. 없으면 오늘(한국 시간).
    pub published_at: Option<String>,
    pub tags: Option<Vec<TagArg>>,
    /// `draft`(기본) 또는 `published`. 사용자가 공개하라고 분명히 말했을 때만 `published`.
    pub status: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdatePostArgs {
    pub post_id: String,
    /// 아래 필드는 바꿀 것만 보낸다. 보내지 않은 필드는 그대로 둔다.
    pub title: Option<String>,
    pub content: Option<String>,
    pub description: Option<String>,
    pub published_at: Option<String>,
    /// 보내면 태그 전체를 이 목록으로 바꾼다.
    pub tags: Option<Vec<TagArg>>,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetStatusArgs {
    pub post_id: String,
    /// `published` 또는 `draft`.
    pub status: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DeletePostArgs {
    pub post_id: String,
    /// 지울 글의 제목을 정확히. 제목이 없는 글은 본문 앞 30자를 정확히.
    pub confirm_title: String,
}

#[tool_router(router = tool_router)]
impl CmsTools {
    pub fn new(state: Arc<AppState>, images: Arc<ImageRehoster>) -> Self {
        Self {
            state,
            images,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "블로그 글 목록(최신순). 초안 포함. 본문 대신 앞부분(excerpt)만 준다.",
        annotations(read_only_hint = true)
    )]
    async fn list_posts(
        &self,
        Parameters(args): Parameters<ListPostsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let status = args.status.unwrap_or_else(|| "all".to_string());
        if !matches!(status.as_str(), "all" | "published" | "draft") {
            return Ok(fail("status 는 all, published, draft 중 하나입니다."));
        }
        let limit = args
            .limit
            .unwrap_or(DEFAULT_LIST_LIMIT)
            .clamp(1, MAX_LIST_LIMIT);
        let pattern = args
            .query
            .filter(|q| !q.trim().is_empty())
            .map(|q| format!("%{}%", q.trim()));

        let rows = query(
            r#"
            SELECT p.post_id, p.title, p.status, p.published_at,
                   COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', p.created_at), '') AS created_at,
                   COALESCE(substr(p.content, 1, 400), '') AS head,
                   IFNULL(GROUP_CONCAT(t.category || '::' || t.name, ','), '') AS tags
            FROM posts p
            LEFT JOIN post_tags pt ON p.post_id = pt.post_id
            LEFT JOIN tags t ON pt.tag_id = t.tag_id
            WHERE (? = 'all' OR (? = 'draft' AND p.status = 'draft') OR (? = 'published' AND p.status != 'draft'))
              AND (? IS NULL OR p.title LIKE ? OR p.content LIKE ?)
            GROUP BY p.post_id
            ORDER BY p.published_at DESC, p.created_at DESC
            LIMIT ?
            "#,
        )
        .bind(&status)
        .bind(&status)
        .bind(&status)
        .bind(&pattern)
        .bind(&pattern)
        .bind(&pattern)
        .bind(limit as i64)
        .fetch_all(&self.state.db)
        .await
        .map_err(internal)?;

        let posts: Vec<serde_json::Value> = rows
            .iter()
            .map(|row| {
                serde_json::json!({
                    "post_id": row.get::<String, _>("post_id"),
                    "title": row.get::<String, _>("title"),
                    "status": row.get::<String, _>("status"),
                    "published_at": row.get::<String, _>("published_at"),
                    "created_at": row.get::<String, _>("created_at"),
                    "tags": parse_tags(row.get::<String, _>("tags").as_str())
                        .into_iter()
                        .map(|t| serde_json::json!({ "tag": t.tag, "category": t.category }))
                        .collect::<Vec<_>>(),
                    "excerpt": first_chars(row.get::<String, _>("head").as_str(), EXCERPT_CHARS),
                })
            })
            .collect();
        Ok(ok_json(serde_json::json!({ "posts": posts })))
    }

    #[tool(
        description = "글 하나를 본문까지 가져온다.",
        annotations(read_only_hint = true)
    )]
    async fn get_post(
        &self,
        Parameters(args): Parameters<PostIdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match crate::posts::find_post(&self.state.db, &args.post_id).await {
            Ok(Some(post)) => Ok(ok_json(serde_json::to_value(post).map_err(internal)?)),
            Ok(None) => Ok(fail("그 id의 글이 없습니다.")),
            Err(err) => Err(internal(err)),
        }
    }

    #[tool(
        description = "지금 쓰이는 태그를 분류별로. 새 글에 태그를 붙일 때 기존 이름·분류를 맞추려고 쓴다.",
        annotations(read_only_hint = true)
    )]
    async fn list_tags(&self) -> Result<CallToolResult, ErrorData> {
        let rows = query("SELECT category, name FROM tags ORDER BY category, name")
            .fetch_all(&self.state.db)
            .await
            .map_err(internal)?;
        let mut by_category: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        for row in rows {
            by_category
                .entry(row.get("category"))
                .or_default()
                .push(row.get("name"));
        }
        Ok(ok_json(
            serde_json::to_value(by_category).map_err(internal)?,
        ))
    }

    #[tool(
        description = "새 글을 올린다. 기본은 초안(draft). 사용자가 공개하라고 분명히 말했을 때만 status 를 published 로. 본문의 외부 이미지(![](https://…), 예: Craft 이미지)는 서버가 내려받아 블로그 이미지 주소로 바꿔 저장한다 — 결과의 images.warnings 에 실패한 이미지가 있으면 사용자에게 알린다.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false
        )
    )]
    async fn create_post(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<CreatePostArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let user = match user_of(&parts) {
            Ok(user) => user,
            Err(result) => return Ok(result),
        };
        let status = args.status.unwrap_or_else(|| "draft".to_string());
        if let Err(message) = check_status(&status) {
            return Ok(fail(message));
        }
        let published_at = args.published_at.unwrap_or_else(today_kst);
        if let Err(message) = check_date(&published_at) {
            return Ok(fail(message));
        }

        let (content, images) = self.images.rehost(&self.state.db, &args.content).await;
        let payload = UploadPostRequest {
            title: args.title,
            description: args.description.unwrap_or_default(),
            published_at,
            tags: to_tags(args.tags.unwrap_or_default()),
            content,
            status: status.clone(),
        };
        match crate::posts::create_post(&self.state, user, payload).await {
            Ok(post_id) => Ok(ok_json(serde_json::json!({
                "post_id": post_id,
                "status": status,
                "images": images_json(&images),
            }))),
            Err(err) => Ok(fail(err.to_string())),
        }
    }

    #[tool(
        description = "글을 고친다. 바꿀 필드만 보낸다(tags 를 보내면 태그 전체를 바꾼다).",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    async fn update_post(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<UpdatePostArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let user = match user_of(&parts) {
            Ok(user) => user,
            Err(result) => return Ok(result),
        };
        let Some(current) = crate::posts::find_post(&self.state.db, &args.post_id)
            .await
            .map_err(internal)?
        else {
            return Ok(fail("그 id의 글이 없습니다."));
        };

        let status = args.status.unwrap_or(current.status);
        if let Err(message) = check_status(&status) {
            return Ok(fail(message));
        }
        let published_at = args.published_at.unwrap_or(current.published_at);
        if let Err(message) = check_date(&published_at) {
            return Ok(fail(message));
        }
        let tags = match args.tags {
            Some(tags) => to_tags(tags),
            None => current
                .tags
                .into_iter()
                .map(|t| Tag {
                    tag: t.tag,
                    category: t.category,
                })
                .collect(),
        };

        let (content, images) = match args.content {
            Some(content) => self.images.rehost(&self.state.db, &content).await,
            None => (current.content, RehostReport::default()),
        };
        let payload = UpdatePostRequest {
            title: args.title.unwrap_or(current.title),
            description: args.description.unwrap_or(current.description),
            published_at,
            tags,
            content,
            status,
        };
        match crate::posts::update_post(&self.state, user, &args.post_id, payload).await {
            Ok(post) => Ok(ok_json(serde_json::json!({
                "post_id": post.post_id,
                "title": post.title,
                "status": post.status,
                "published_at": post.published_at,
                "images": images_json(&images),
            }))),
            Err(err) => Ok(fail(err.to_string())),
        }
    }

    #[tool(
        description = "글을 공개(published)하거나 초안(draft)으로 돌린다.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    async fn set_post_status(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<SetStatusArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.update_post(
            Extension(parts),
            Parameters(UpdatePostArgs {
                post_id: args.post_id,
                title: None,
                content: None,
                description: None,
                published_at: None,
                tags: None,
                status: Some(args.status),
            }),
        )
        .await
    }

    #[tool(
        description = "글을 지운다. 되돌릴 수 없다. confirm_title 에 지울 글의 제목(제목이 없으면 본문 앞 30자)을 정확히 넣어야 지워진다. 사용자가 지우라고 분명히 말했을 때만 쓴다.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false
        )
    )]
    async fn delete_post(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(args): Parameters<DeletePostArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let user = match user_of(&parts) {
            Ok(user) => user,
            Err(result) => return Ok(result),
        };
        let Some(current) = crate::posts::find_post(&self.state.db, &args.post_id)
            .await
            .map_err(internal)?
        else {
            return Ok(fail("그 id의 글이 없습니다."));
        };

        let expected = confirm_text(&current.title, &current.content);
        if args.confirm_title.trim() != expected {
            return Ok(fail(format!(
                "confirm_title 이 맞지 않아 지우지 않았습니다. 이 글의 확인 문구는 \"{expected}\" 입니다."
            )));
        }

        match crate::posts::delete_post(&self.state, user, &args.post_id).await {
            Ok(()) => Ok(ok_json(
                serde_json::json!({ "deleted": args.post_id, "title": current.title }),
            )),
            Err(err) => Ok(fail(err.to_string())),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for CmsTools {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "tyange 블로그 CMS. 글 목록·조회·작성·수정·공개 전환·삭제를 한다. 새 글은 사용자가 공개하라고 하지 않는 한 draft 로 올린다.",
        )
    }
}

fn user_of(parts: &Parts) -> Result<&AuthenticatedUser, CallToolResult> {
    parts
        .extensions
        .get::<AuthenticatedUser>()
        .ok_or_else(|| fail("인증된 사용자 정보가 없습니다."))
}

/// 삭제 확인 문구: 제목, 제목이 비었으면 본문 앞 30자(공백 정리).
pub fn confirm_text(title: &str, content: &str) -> String {
    let title = title.trim();
    if !title.is_empty() {
        return title.to_string();
    }
    first_chars(content.trim(), CONFIRM_PREVIEW_CHARS)
}

fn first_chars(text: &str, n: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(n)
        .collect()
}

fn check_status(status: &str) -> Result<(), &'static str> {
    if matches!(status, "draft" | "published") {
        Ok(())
    } else {
        Err("status 는 draft 또는 published 입니다.")
    }
}

fn check_date(date: &str) -> Result<(), &'static str> {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map(|_| ())
        .map_err(|_| "published_at 은 YYYY-MM-DD 모양이어야 합니다.")
}

fn today_kst() -> String {
    (chrono::Utc::now() + chrono::Duration::hours(9))
        .format("%Y-%m-%d")
        .to_string()
}

fn to_tags(tags: Vec<TagArg>) -> Vec<Tag> {
    tags.into_iter()
        .filter(|t| !t.tag.trim().is_empty())
        .map(|t| Tag {
            tag: t.tag.trim().to_string(),
            category: t.category.trim().to_string(),
        })
        .collect()
}

fn images_json(report: &RehostReport) -> serde_json::Value {
    serde_json::json!({ "rehosted": report.rehosted, "warnings": report.warnings })
}

fn ok_json(value: serde_json::Value) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(
        serde_json::to_string_pretty(&value).unwrap_or_default(),
    )])
}

fn fail(message: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message.into())])
}

fn internal(err: impl std::fmt::Display) -> ErrorData {
    ErrorData::internal_error(err.to_string(), None)
}
