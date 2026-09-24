//! `#[OpenApi]`로 옮긴 라우트들. OpenAPI 스펙 자동 생성이 목적이라 poem 프레임워크
//! 자체는 그대로 쓰고, 라우트 등록 방식만 `#[handler]` 대신 `#[OpenApi]`를 쓴다.
//!
//! `src/routes/*.rs`에 원래 핸들러가 남아있는 라우트도 있다(테스트가 그쪽을 참조해서
//! 등록만 빼고 지우진 않은 것 — post/portfolio/wedding/image 계열). 포트폴리오는
//! 2026-09에 죽은 사본과 테스트를 정리해서 이 파일의 `PortfolioApi`만 남았다.
//! 아직 옮기지 않은 라우트(공개 하객 명단 접수 등)는 `main.rs`에서 여전히 plain poem
//! 라우트로 등록된다.

use std::sync::Arc;

use poem::http::StatusCode;
use poem_openapi::{
    param::{Path, Query},
    payload::Json,
    OpenApi,
};
use sqlx::{query, query_as, QueryBuilder, Row, Sqlite};
use tyange_cms_api::auth::authorization::{ensure_admin, ensure_post_owner, AuthGuard};

use crate::blog_redeploy::{is_blog_redeploy_target, BlogContentEvent, BlogVisibility};
use crate::models::{
    ApiResponse, AppState, DeletePostResponse, Post, PostItem, PostResponseDb, PostsResponse, Tag,
    TagWithCategory, UpdatePostRequest, UploadPostRequest, UploadPostResponse,
};
use crate::utils::parse_tags;

pub struct PostsApi {
    pub state: Arc<AppState>,
}

#[OpenApi]
impl PostsApi {
    /// 발행된 포스트 목록. `writer_id`로 필터링 가능.
    #[oai(path = "/posts", method = "get")]
    async fn get_posts(
        &self,
        Query(writer_id): Query<Option<String>>,
    ) -> poem::Result<Json<ApiResponse<PostsResponse>>> {
        let mut builder = QueryBuilder::new(
            r#"
            SELECT p.post_id, p.title, p.description, p.published_at,
               COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', p.created_at), '') AS created_at, p.status,
            IFNULL(GROUP_CONCAT(t.category || '::' || t.name, ','), '') AS tags
            FROM posts p
            LEFT JOIN post_tags pt ON p.post_id = pt.post_id
            LEFT JOIN tags t ON pt.tag_id = t.tag_id
            "#,
        );

        builder.push("WHERE p.status != 'draft' ");

        if let Some(id) = writer_id {
            builder.push("AND p.writer_id = ");
            builder.push_bind(id);
            builder.push(" ");
        }

        builder.push("GROUP BY p.post_id ORDER BY p.published_at DESC, p.created_at DESC");

        let db_posts = builder
            .build()
            .fetch_all(&self.state.db)
            .await
            .map_err(|err| {
                poem::Error::from_string(
                    format!("Error fetching posts: {}", err),
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
            })?;

        if db_posts.is_empty() {
            return Ok(Json(ApiResponse {
                status: true,
                data: Some(PostsResponse {
                    posts: Vec::<PostItem>::new(),
                }),
                message: Some(String::from("포스트가 하나도 없네요.")),
            }));
        }

        Ok(Json(ApiResponse {
            status: true,
            data: Some(PostsResponse {
                posts: db_posts
                    .iter()
                    .map(|db_post| PostItem {
                        post_id: db_post.get("post_id"),
                        title: db_post.get("title"),
                        description: db_post.get("description"),
                        published_at: db_post.get("published_at"),
                        created_at: db_post.get("created_at"),
                        tags: parse_tags(db_post.get("tags")),
                        status: db_post.get("status"),
                    })
                    .collect(),
            }),
            message: None,
        }))
    }

    /// draft 포함 전체 포스트 목록. 관리자만 조회 가능(tyange-cms 관리 화면이 씀).
    #[oai(path = "/admin/posts", method = "get")]
    async fn get_all_posts(
        &self,
        guard: AuthGuard,
    ) -> poem::Result<Json<ApiResponse<PostsResponse>>> {
        ensure_admin(&guard.0)?;

        let db_posts = query(
            r#"
            SELECT p.post_id, p.title, p.description, p.published_at,
               COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', p.created_at), '') AS created_at, p.status,
            IFNULL(GROUP_CONCAT(t.category || '::' || t.name, ','), '') AS tags
            FROM posts p
            LEFT JOIN post_tags pt ON p.post_id = pt.post_id
            LEFT JOIN tags t ON pt.tag_id = t.tag_id
            GROUP BY p.post_id
            ORDER BY p.published_at DESC, p.created_at DESC
            "#,
        )
        .fetch_all(&self.state.db)
        .await
        .map_err(|err| {
            poem::Error::from_string(
                format!("Error fetching posts: {}", err),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;

        if db_posts.is_empty() {
            return Ok(Json(ApiResponse {
                status: true,
                data: Some(PostsResponse {
                    posts: Vec::<PostItem>::new(),
                }),
                message: Some(String::from("포스트가 하나도 없네요.")),
            }));
        }

        Ok(Json(ApiResponse {
            status: true,
            data: Some(PostsResponse {
                posts: db_posts
                    .iter()
                    .map(|db_post| PostItem {
                        post_id: db_post.get("post_id"),
                        title: db_post.get("title"),
                        description: db_post.get("description"),
                        published_at: db_post.get("published_at"),
                        created_at: db_post.get("created_at"),
                        tags: parse_tags(db_post.get("tags")),
                        status: db_post.get("status"),
                    })
                    .collect(),
            }),
            message: None,
        }))
    }

    /// 단일 포스트 조회 (본문 포함). 비인증.
    #[oai(path = "/post/:post_id", method = "get")]
    async fn get_post(&self, Path(post_id): Path<String>) -> poem::Result<Json<Post>> {
        let db_post = query_as::<Sqlite, PostResponseDb>(
            r#"
            SELECT p.post_id, p.title, p.description, p.published_at,
               COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', p.created_at), '') AS created_at,
            p.content, p.status,
            IFNULL(GROUP_CONCAT(t.category || '::' || t.name, ','), '') AS tags
            FROM posts p
            LEFT JOIN post_tags pt ON p.post_id = pt.post_id
            LEFT JOIN tags t ON pt.tag_id = t.tag_id
            WHERE p.post_id = ?
            GROUP BY p.post_id
            "#,
        )
        .bind(&post_id)
        .fetch_optional(&self.state.db)
        .await
        .map_err(|err| {
            poem::Error::from_string(
                format!("Error fetching post: {}", err),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;

        match db_post {
            Some(db_post) => Ok(Json(Post {
                post_id: db_post.post_id,
                title: db_post.title,
                description: db_post.description,
                published_at: db_post.published_at,
                created_at: db_post.created_at,
                tags: parse_tags(&db_post.tags),
                content: db_post.content,
                status: db_post.status,
            })),
            None => Err(poem::Error::from_string(
                "해당 id에 해당하는 포스트가 없네요.",
                StatusCode::NOT_FOUND,
            )),
        }
    }

    /// 새 포스트 업로드. 인증 필요.
    #[oai(path = "/post/upload", method = "post")]
    async fn upload_post(
        &self,
        guard: AuthGuard,
        payload: Json<UploadPostRequest>,
    ) -> poem::Result<Json<ApiResponse<UploadPostResponse>>> {
        let user = &guard.0;
        let payload = payload.0;
        let post_id = uuid::Uuid::new_v4().to_string();

        let mut tx = self.state.db.begin().await.map_err(|e| {
            poem::Error::from_string(
                format!("트랜잭션 시작 실패: {}", e),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;

        query(
            r#"
            INSERT INTO posts (post_id, title, description, published_at, content, writer_id, status)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&post_id)
        .bind(&payload.title)
        .bind(&payload.description)
        .bind(&payload.published_at)
        .bind(&payload.content)
        .bind(&user.user_id)
        .bind(&payload.status)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            poem::Error::from_string(
                format!("포스트 저장 실패: {}", e),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;

        for tag in &payload.tags {
            let tag_name = &tag.tag;
            if tag_name.is_empty() {
                continue;
            }

            query("INSERT OR IGNORE INTO tags (name, category) VALUES (?, ?)")
                .bind(tag_name)
                .bind(&tag.category)
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    poem::Error::from_string(
                        format!("태그 저장 실패: {}", e),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )
                })?;

            query(
                r#"
                INSERT INTO post_tags (post_id, tag_id)
                SELECT ?, tag_id FROM tags WHERE name = ?
                "#,
            )
            .bind(&post_id)
            .bind(tag_name)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                poem::Error::from_string(
                    format!("포스트-태그 관계 저장 실패: {}", e),
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
            })?;
        }

        tx.commit().await.map_err(|e| {
            poem::Error::from_string(
                format!("트랜잭션 커밋 실패: {}", e),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;

        if is_blog_redeploy_target(
            &payload.status,
            payload.tags.iter().map(|tag| tag.tag.as_str()),
        ) {
            self.state
                .blog_redeploy
                .dispatch_content_change(
                    BlogContentEvent::Publish,
                    &post_id,
                    BlogVisibility::Visible,
                )
                .await;
        }

        Ok(Json(ApiResponse {
            status: true,
            data: Some(UploadPostResponse { post_id }),
            message: Some(String::from("포스트를 업로드 했습니다.")),
        }))
    }

    /// 포스트 수정. 작성자 본인 또는 관리자만.
    #[oai(path = "/post/update/:post_id", method = "put")]
    async fn update_post(
        &self,
        guard: AuthGuard,
        Path(post_id): Path<String>,
        payload: Json<UpdatePostRequest>,
    ) -> poem::Result<Json<ApiResponse<Post>>> {
        let user = &guard.0;
        let payload = payload.0;
        ensure_post_owner(user, &post_id, &self.state.db).await?;
        let existing_post = fetch_existing_post(&self.state.db, &post_id).await?;
        let redeploy_event = determine_redeploy_event(&existing_post, &payload);

        let mut tx = self.state.db.begin().await.map_err(|e| {
            poem::Error::from_string(
                format!("트랜잭션 시작 실패: {}", e),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;

        let updated = query(
            r#"
            UPDATE posts SET title = ?, description = ?, published_at = ?,
            content = ?, status = ? WHERE post_id = ?
            "#,
        )
        .bind(&payload.title)
        .bind(&payload.description)
        .bind(&payload.published_at)
        .bind(&payload.content)
        .bind(&payload.status)
        .bind(&post_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            poem::Error::from_string(
                format!("포스트 업데이트 실패: {}", e),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;

        if updated.rows_affected() == 0 {
            return Err(poem::Error::from_string(
                "게시글을 찾을 수 없습니다.",
                StatusCode::NOT_FOUND,
            ));
        }

        query("DELETE FROM post_tags WHERE post_id = ?")
            .bind(&post_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                poem::Error::from_string(
                    format!("태그 관계 삭제 실패: {}", e),
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
            })?;

        for tag in &payload.tags {
            let tag_name = &tag.tag;
            if tag_name.is_empty() {
                continue;
            }

            query("INSERT OR IGNORE INTO tags (name, category) VALUES (?, ?)")
                .bind(tag_name)
                .bind(&tag.category)
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    poem::Error::from_string(
                        format!("태그 저장 실패: {}", e),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )
                })?;

            query(
                r#"
                INSERT INTO post_tags (post_id, tag_id)
                SELECT ?, tag_id FROM tags WHERE name = ?
                "#,
            )
            .bind(&post_id)
            .bind(tag_name)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                poem::Error::from_string(
                    format!("포스트-태그 관계 저장 실패: {}", e),
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
            })?;
        }

        tx.commit().await.map_err(|e| {
            poem::Error::from_string(
                format!("트랜잭션 커밋 실패: {}", e),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;

        if let Some((content_event, visibility)) = redeploy_event {
            self.state
                .blog_redeploy
                .dispatch_content_change(content_event, &post_id, visibility)
                .await;
        }

        Ok(Json(ApiResponse {
            status: true,
            data: Some(Post {
                post_id,
                title: payload.title,
                description: payload.description,
                published_at: payload.published_at,
                created_at: existing_post.created_at,
                tags: payload
                    .tags
                    .iter()
                    .map(|tag| TagWithCategory {
                        tag: String::from(&tag.tag),
                        category: String::from(&tag.category),
                    })
                    .collect(),
                content: payload.content,
                status: payload.status,
            }),
            message: Some(String::from("포스트를 업데이트 했습니다.")),
        }))
    }

    /// 포스트 삭제. 작성자 본인 또는 관리자만.
    #[oai(path = "/post/delete/:post_id", method = "delete")]
    async fn delete_post(
        &self,
        guard: AuthGuard,
        Path(post_id): Path<String>,
    ) -> poem::Result<Json<ApiResponse<DeletePostResponse>>> {
        ensure_post_owner(&guard.0, &post_id, &self.state.db).await?;

        let existing_post = query_as::<Sqlite, PostResponseDb>(
            r#"
            SELECT p.post_id, p.title, p.description, p.published_at,
               COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', p.created_at), '') AS created_at,
            p.content, p.status,
            IFNULL(GROUP_CONCAT(t.category || '::' || t.name, ','), '') AS tags
            FROM posts p
            LEFT JOIN post_tags pt ON p.post_id = pt.post_id
            LEFT JOIN tags t ON pt.tag_id = t.tag_id
            WHERE p.post_id = ?
            GROUP BY p.post_id
            "#,
        )
        .bind(&post_id)
        .fetch_one(&self.state.db)
        .await
        .map_err(|err| {
            poem::Error::from_string(
                format!("게시글 조회에 실패했습니다: {}", err),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;

        let result = query("DELETE FROM posts WHERE post_id = ?")
            .bind(&post_id)
            .execute(&self.state.db)
            .await
            .map_err(|err| {
                poem::Error::from_string(err.to_string(), StatusCode::INTERNAL_SERVER_ERROR)
            })?;

        if result.rows_affected() == 0 {
            return Err(poem::Error::from_string(
                "게시글을 찾을 수 없습니다.",
                StatusCode::NOT_FOUND,
            ));
        }

        if is_blog_redeploy_target(
            &existing_post.status,
            parse_tags(&existing_post.tags)
                .iter()
                .map(|tag| tag.tag.as_str()),
        ) {
            self.state
                .blog_redeploy
                .dispatch_content_change(BlogContentEvent::Delete, &post_id, BlogVisibility::Hidden)
                .await;
        }

        Ok(Json(ApiResponse {
            status: true,
            data: Some(DeletePostResponse { post_id }),
            message: Some(String::from("포스트가 삭제되었습니다.")),
        }))
    }
}

/// `routes::update_post`에 있던 것과 동일한 로직. 상태/태그가 실제로 바뀌었을 때만
/// 블로그 재배포를 트리거하려고 기존 포스트 스냅샷을 남긴다.
#[derive(Debug)]
struct ExistingPostSnapshot {
    title: String,
    description: String,
    published_at: String,
    created_at: String,
    content: String,
    status: String,
    tags: Vec<(String, String)>,
}

async fn fetch_existing_post(
    db: &sqlx::Pool<Sqlite>,
    post_id: &str,
) -> poem::Result<ExistingPostSnapshot> {
    let existing = query_as::<Sqlite, PostResponseDb>(
        r#"
        SELECT p.post_id, p.title, p.description, p.published_at,
               COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', p.created_at), '') AS created_at,
        p.content, p.status,
        IFNULL(GROUP_CONCAT(t.category || '::' || t.name, ','), '') AS tags
        FROM posts p
        LEFT JOIN post_tags pt ON p.post_id = pt.post_id
        LEFT JOIN tags t ON pt.tag_id = t.tag_id
        WHERE p.post_id = ?
        GROUP BY p.post_id
        "#,
    )
    .bind(post_id)
    .fetch_optional(db)
    .await
    .map_err(|e| {
        poem::Error::from_string(
            format!("기존 포스트 조회 실패: {}", e),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
    })?
    .ok_or_else(|| poem::Error::from_string("게시글을 찾을 수 없습니다.", StatusCode::NOT_FOUND))?;

    Ok(ExistingPostSnapshot {
        title: existing.title,
        description: existing.description,
        published_at: existing.published_at,
        created_at: existing.created_at,
        content: existing.content,
        status: existing.status,
        tags: normalize_tags(
            &parse_tags(&existing.tags)
                .into_iter()
                .map(|tag| Tag {
                    tag: tag.tag,
                    category: tag.category,
                })
                .collect::<Vec<_>>(),
        ),
    })
}

fn determine_redeploy_event(
    existing_post: &ExistingPostSnapshot,
    payload: &UpdatePostRequest,
) -> Option<(BlogContentEvent, BlogVisibility)> {
    let was_visible = is_blog_redeploy_target(
        &existing_post.status,
        existing_post.tags.iter().map(|(_, tag)| tag.as_str()),
    );
    let is_visible = is_blog_redeploy_target(
        &payload.status,
        payload.tags.iter().map(|tag| tag.tag.as_str()),
    );

    match (was_visible, is_visible) {
        (false, true) => Some((BlogContentEvent::Publish, BlogVisibility::Visible)),
        (true, false) => Some((BlogContentEvent::Delete, BlogVisibility::Hidden)),
        (true, true) if public_content_changed(existing_post, payload) => {
            Some((BlogContentEvent::Update, BlogVisibility::Visible))
        }
        _ => None,
    }
}

fn public_content_changed(
    existing_post: &ExistingPostSnapshot,
    payload: &UpdatePostRequest,
) -> bool {
    existing_post.title != payload.title
        || existing_post.description != payload.description
        || existing_post.published_at != payload.published_at
        || existing_post.content != payload.content
        || existing_post.status != payload.status
        || existing_post.tags != normalize_tags(&payload.tags)
}

fn normalize_tags(tags: &[Tag]) -> Vec<(String, String)> {
    let mut normalized = tags
        .iter()
        .map(|tag| (tag.category.clone(), tag.tag.clone()))
        .collect::<Vec<_>>();
    normalized.sort();
    normalized
}

pub struct TagsApi {
    pub state: Arc<AppState>,
}

#[OpenApi]
impl TagsApi {
    /// 카테고리별 태그 목록.
    #[oai(path = "/tags-with-category", method = "get")]
    async fn get_tags_with_category(
        &self,
    ) -> poem::Result<Json<ApiResponse<Vec<crate::models::TagsWithCategory>>>> {
        use std::collections::BTreeMap;

        let db_tags = query("SELECT category, name FROM tags ORDER BY category")
            .fetch_all(&self.state.db)
            .await
            .map_err(|err| {
                poem::Error::from_string(
                    format!("Error fetching tags: {}", err),
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
            })?;

        if db_tags.is_empty() {
            return Ok(Json(ApiResponse {
                status: true,
                data: Some(Vec::new()),
                message: Some(String::from("조회된 태그-카테고리가 하나도 없어요.")),
            }));
        }

        let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for row in &db_tags {
            let category: String = row.get("category");
            let name: String = row.get("name");
            map.entry(category).or_default().push(name);
        }

        let tags_with_category = map
            .into_iter()
            .map(|(category, tags)| crate::models::TagsWithCategory { category, tags })
            .collect();

        Ok(Json(ApiResponse {
            status: true,
            data: Some(tags_with_category),
            message: None,
        }))
    }
}

/// 인증된 이미지 업로드 하나에만 쓰는 정책. `routes::upload_image`와 동일한 값.
const AUTHENTICATED_UPLOAD_POLICY: crate::services::image_storage::ImageUploadPolicy =
    crate::services::image_storage::ImageUploadPolicy {
        max_bytes: 10 * 1024 * 1024,
        allowed_formats: &[
            crate::services::image_storage::ImageFormat::Png,
            crate::services::image_storage::ImageFormat::Jpeg,
            crate::services::image_storage::ImageFormat::Gif,
            crate::services::image_storage::ImageFormat::Webp,
        ],
    };

#[derive(Debug, poem_openapi::Multipart)]
struct UploadImagePayload {
    file: poem_openapi::types::multipart::Upload,
}

pub struct ImagesApi {
    pub state: Arc<AppState>,
}

#[OpenApi]
impl ImagesApi {
    /// 포스트 본문 이미지와 포트폴리오 기술 스택 아이콘이 공유하는 업로드 엔드포인트.
    /// `image_type` 쿼리로 종류를 구분한다 (기본값 "in_post").
    #[oai(path = "/upload-image", method = "post")]
    async fn upload_image(
        &self,
        guard: AuthGuard,
        Query(post_id): Query<Option<String>>,
        Query(image_type): Query<Option<String>>,
        payload: UploadImagePayload,
    ) -> poem::Result<Json<ApiResponse<crate::models::UploadImageResponse>>> {
        let _user = &guard.0;

        let upload_dir = std::path::PathBuf::from(
            std::env::var("UPLOAD_PATH").unwrap_or_else(|_| ".uploads/images".to_string()),
        );
        let image_type = image_type.unwrap_or_else(|| String::from("in_post"));

        let stored = crate::services::image_storage::store_image(
            payload.file,
            &AUTHENTICATED_UPLOAD_POLICY,
            &self.state.db,
            crate::services::image_storage::ImageRecord {
                post_id,
                image_type,
                uploader_ip: None,
                status: "confirmed",
            },
            &upload_dir,
            "/images",
        )
        .await?;

        Ok(Json(ApiResponse {
            status: true,
            data: Some(crate::models::UploadImageResponse {
                image_path: stored.web_path,
            }),
            message: Some(String::from("이미지 업로드에 성공했습니다.")),
        }))
    }
}

pub struct PortfolioApi {
    pub state: Arc<AppState>,
}

#[derive(poem_openapi::ApiResponse)]
enum DeletePortfolioResponse {
    #[oai(status = 204)]
    NoContent,
}

#[derive(Debug, serde::Deserialize, poem_openapi::Object)]
struct UpdateSectionRequest {
    content: poem_openapi::types::Any<serde_json::Value>,
}

#[derive(Debug, serde::Serialize, poem_openapi::Object)]
struct UpdateSectionResponse {
    section_key: String,
    updated_at: String,
}

// 파싱에 성공한 "구조체"를 재직렬화한다 — 원본 value를 그대로 재직렬화하면 스키마에
// 없는 키(예전 identity.role/location/availability처럼)가 계속 살아남는다. 구조체를
// 거쳐 다시 직렬화하면 알려진 필드만 남고, serde(default)가 채운 값도 일관되게 저장된다.
fn validate_portfolio_section(
    section_key: &str,
    value: &serde_json::Value,
) -> Result<String, String> {
    use crate::models::{
        PortfolioCareerSection, PortfolioIdentity, PortfolioIntroSection, PortfolioMeta,
        PortfolioProject,
    };

    match section_key {
        "meta" => {
            let parsed: PortfolioMeta = serde_json::from_value(value.clone())
                .map_err(|e| format!("meta 검증 실패: {}", e))?;
            serde_json::to_string(&parsed).map_err(|e| e.to_string())
        }
        "identity" => {
            let parsed: PortfolioIdentity = serde_json::from_value(value.clone())
                .map_err(|e| format!("identity 검증 실패: {}", e))?;
            serde_json::to_string(&parsed).map_err(|e| e.to_string())
        }
        "featured_projects" => {
            let parsed: Vec<PortfolioProject> = serde_json::from_value(value.clone())
                .map_err(|e| format!("featured_projects 검증 실패: {}", e))?;
            serde_json::to_string(&parsed).map_err(|e| e.to_string())
        }
        "career" => {
            let parsed: PortfolioCareerSection = serde_json::from_value(value.clone())
                .map_err(|e| format!("career 검증 실패: {}", e))?;
            serde_json::to_string(&parsed).map_err(|e| e.to_string())
        }
        "intro" => {
            let parsed: PortfolioIntroSection = serde_json::from_value(value.clone())
                .map_err(|e| format!("intro 검증 실패: {}", e))?;
            serde_json::to_string(&parsed).map_err(|e| e.to_string())
        }
        _ => Err(format!("알 수 없는 섹션: {}", section_key)),
    }
}

// pool과 트랜잭션(&mut *tx) 양쪽에서 다 쓸 수 있도록 Executor로 제네릭화했다 —
// update_portfolio/delete_portfolio가 여러 statement를 트랜잭션 하나로 묶으면서
// 이 헬퍼도 트랜잭션 안에서 호출해야 했다.
async fn upsert_portfolio_section<'e, E>(
    executor: E,
    portfolio_id: i32,
    section_key: &str,
    content: &str,
) -> Result<(), sqlx::Error>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    query(
        r#"
        INSERT INTO portfolio_section (portfolio_id, section_key, content, created_at, updated_at)
        VALUES (?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
        ON CONFLICT(portfolio_id, section_key) DO UPDATE SET
            content = excluded.content,
            updated_at = CURRENT_TIMESTAMP
        "#,
    )
    .bind(portfolio_id)
    .bind(section_key)
    .bind(content)
    .execute(executor)
    .await?;
    Ok(())
}

#[OpenApi]
impl PortfolioApi {
    /// 포트폴리오 문서 조회. 섹션 테이블을 조립해서 반환한다.
    #[oai(path = "/portfolio", method = "get")]
    async fn get_portfolio(
        &self,
    ) -> poem::Result<Json<ApiResponse<crate::models::PortfolioResponse>>> {
        use crate::models::{
            PortfolioCareerSection, PortfolioDocument, PortfolioIdentity, PortfolioIntroSection,
            PortfolioMasterRow, PortfolioMeta, PortfolioProject, PortfolioSectionRow,
        };

        let master = query_as::<Sqlite, PortfolioMasterRow>(
            "SELECT portfolio_id, slug, created_at FROM portfolio WHERE slug = ?",
        )
        .bind("dev")
        .fetch_optional(&self.state.db)
        .await
        .map_err(|err| {
            poem::Error::from_string(
                format!("Error fetching portfolio: {}", err),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?
        .ok_or_else(|| {
            poem::Error::from_string(
                "포트폴리오 데이터를 찾지 못했습니다.",
                StatusCode::NOT_FOUND,
            )
        })?;

        let sections = query_as::<Sqlite, PortfolioSectionRow>(
            r#"
            SELECT section_id, portfolio_id, section_key, content, created_at, updated_at
            FROM portfolio_section
            WHERE portfolio_id = ?
            "#,
        )
        .bind(master.portfolio_id)
        .fetch_all(&self.state.db)
        .await
        .map_err(|err| {
            poem::Error::from_string(
                format!("Error fetching portfolio sections: {}", err),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;

        if sections.is_empty() {
            return Err(poem::Error::from_string(
                "포트폴리오 데이터를 찾지 못했습니다.",
                StatusCode::NOT_FOUND,
            ));
        }

        let mut meta: Option<PortfolioMeta> = None;
        let mut identity: Option<PortfolioIdentity> = None;
        let mut featured_projects: Vec<PortfolioProject> = Vec::new();
        let mut career: Option<PortfolioCareerSection> = None;
        let mut intro: Option<PortfolioIntroSection> = None;
        let mut latest_updated_at = String::new();

        for section in &sections {
            if section.updated_at > latest_updated_at {
                latest_updated_at = section.updated_at.clone();
            }

            match section.section_key.as_str() {
                "meta" => {
                    meta = Some(serde_json::from_str(&section.content).map_err(|err| {
                        poem::Error::from_string(
                            format!("Error parsing meta section: {}", err),
                            StatusCode::INTERNAL_SERVER_ERROR,
                        )
                    })?);
                }
                "identity" => {
                    identity = Some(serde_json::from_str(&section.content).map_err(|err| {
                        poem::Error::from_string(
                            format!("Error parsing identity section: {}", err),
                            StatusCode::INTERNAL_SERVER_ERROR,
                        )
                    })?);
                }
                "featured_projects" => {
                    featured_projects = serde_json::from_str(&section.content).map_err(|err| {
                        poem::Error::from_string(
                            format!("Error parsing featured_projects section: {}", err),
                            StatusCode::INTERNAL_SERVER_ERROR,
                        )
                    })?;
                }
                "career" => {
                    career = Some(serde_json::from_str(&section.content).map_err(|err| {
                        poem::Error::from_string(
                            format!("Error parsing career section: {}", err),
                            StatusCode::INTERNAL_SERVER_ERROR,
                        )
                    })?);
                }
                "intro" => {
                    intro = Some(serde_json::from_str(&section.content).map_err(|err| {
                        poem::Error::from_string(
                            format!("Error parsing intro section: {}", err),
                            StatusCode::INTERNAL_SERVER_ERROR,
                        )
                    })?);
                }
                _ => {}
            }
        }

        let meta = meta.ok_or_else(|| {
            poem::Error::from_string(
                "포트폴리오 meta 섹션을 찾지 못했습니다.",
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;
        let identity = identity.ok_or_else(|| {
            poem::Error::from_string(
                "포트폴리오 identity 섹션을 찾지 못했습니다.",
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;

        let document = PortfolioDocument {
            slug: meta.slug,
            version: meta.version,
            identity,
            featured_projects,
            career,
            intro,
        };

        Ok(Json(ApiResponse {
            status: true,
            data: Some(crate::models::PortfolioResponse {
                portfolio_id: master.portfolio_id,
                slug: master.slug,
                content: document,
                created_at: master.created_at,
                updated_at: latest_updated_at,
            }),
            message: None,
        }))
    }

    /// 포트폴리오 전체 문서를 업서트한다. 최초 생성 시에만 쓰고 이후는 섹션별 저장을 쓴다.
    #[oai(path = "/portfolio/update", method = "put")]
    async fn update_portfolio(
        &self,
        guard: AuthGuard,
        payload: Json<crate::models::UpdatePortfolioRequest>,
    ) -> poem::Result<Json<ApiResponse<crate::models::PortfolioResponse>>> {
        use crate::models::PortfolioMeta;

        ensure_admin(&guard.0)?;
        let content = payload.0.content;
        let slug = content.slug.trim();
        let slug = if slug.is_empty() { "dev" } else { slug };

        // 아래 여러 INSERT/UPDATE/DELETE가 이 트랜잭션 하나로 묶인다 — 예전엔 각각
        // 독립적으로 커밋돼서 중간에 실패하면(예: featured_projects 저장 성공,
        // career 저장 실패) 문서가 반쪽짜리 상태로 남을 수 있었다.
        let mut tx = self.state.db.begin().await.map_err(|err| {
            poem::Error::from_string(
                format!("트랜잭션 시작 실패: {}", err),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;

        query(
            r#"
            INSERT INTO portfolio (slug, created_at)
            VALUES (?, CURRENT_TIMESTAMP)
            ON CONFLICT(slug) DO UPDATE SET slug = excluded.slug
            "#,
        )
        .bind(slug)
        .execute(&mut *tx)
        .await
        .map_err(|err| {
            poem::Error::from_string(
                format!("포트폴리오 마스터 행 생성 실패: {}", err),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;

        let portfolio_id: i32 =
            sqlx::query_scalar("SELECT portfolio_id FROM portfolio WHERE slug = ?")
                .bind(slug)
                .fetch_one(&mut *tx)
                .await
                .map_err(|err| {
                    poem::Error::from_string(
                        format!("포트폴리오 조회 실패: {}", err),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )
                })?;

        let meta = PortfolioMeta {
            slug: slug.to_string(),
            version: content.version,
        };
        let meta_json = serde_json::to_string(&meta).map_err(|err| {
            poem::Error::from_string(
                format!("meta 직렬화 실패: {}", err),
                StatusCode::BAD_REQUEST,
            )
        })?;
        let identity_json = serde_json::to_string(&content.identity).map_err(|err| {
            poem::Error::from_string(
                format!("identity 직렬화 실패: {}", err),
                StatusCode::BAD_REQUEST,
            )
        })?;
        let projects_json = serde_json::to_string(&content.featured_projects).map_err(|err| {
            poem::Error::from_string(
                format!("featured_projects 직렬화 실패: {}", err),
                StatusCode::BAD_REQUEST,
            )
        })?;
        let career_json = content
            .career
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|err| {
                poem::Error::from_string(
                    format!("career 직렬화 실패: {}", err),
                    StatusCode::BAD_REQUEST,
                )
            })?;
        let intro_json = content
            .intro
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|err| {
                poem::Error::from_string(
                    format!("intro 직렬화 실패: {}", err),
                    StatusCode::BAD_REQUEST,
                )
            })?;

        upsert_portfolio_section(&mut *tx, portfolio_id, "meta", &meta_json)
            .await
            .map_err(|err| {
                poem::Error::from_string(
                    format!("meta 섹션 저장 실패: {}", err),
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
            })?;
        upsert_portfolio_section(&mut *tx, portfolio_id, "identity", &identity_json)
            .await
            .map_err(|err| {
                poem::Error::from_string(
                    format!("identity 섹션 저장 실패: {}", err),
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
            })?;
        upsert_portfolio_section(&mut *tx, portfolio_id, "featured_projects", &projects_json)
            .await
            .map_err(|err| {
                poem::Error::from_string(
                    format!("featured_projects 섹션 저장 실패: {}", err),
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
            })?;

        if let Some(career_str) = &career_json {
            upsert_portfolio_section(&mut *tx, portfolio_id, "career", career_str)
                .await
                .map_err(|err| {
                    poem::Error::from_string(
                        format!("career 섹션 저장 실패: {}", err),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )
                })?;
        } else {
            query(
                "DELETE FROM portfolio_section WHERE portfolio_id = ? AND section_key = 'career'",
            )
            .bind(portfolio_id)
            .execute(&mut *tx)
            .await
            .map_err(|err| {
                poem::Error::from_string(
                    format!("career 섹션 삭제 실패: {}", err),
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
            })?;
        }

        if let Some(intro_str) = &intro_json {
            upsert_portfolio_section(&mut *tx, portfolio_id, "intro", intro_str)
                .await
                .map_err(|err| {
                    poem::Error::from_string(
                        format!("intro 섹션 저장 실패: {}", err),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )
                })?;
        } else {
            query("DELETE FROM portfolio_section WHERE portfolio_id = ? AND section_key = 'intro'")
                .bind(portfolio_id)
                .execute(&mut *tx)
                .await
                .map_err(|err| {
                    poem::Error::from_string(
                        format!("intro 섹션 삭제 실패: {}", err),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )
                })?;
        }

        let updated_at: String = sqlx::query_scalar(
            "SELECT MAX(updated_at) FROM portfolio_section WHERE portfolio_id = ?",
        )
        .bind(portfolio_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|err| {
            poem::Error::from_string(
                format!("updated_at 조회 실패: {}", err),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;
        let created_at: String =
            sqlx::query_scalar("SELECT created_at FROM portfolio WHERE portfolio_id = ?")
                .bind(portfolio_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|err| {
                    poem::Error::from_string(
                        format!("created_at 조회 실패: {}", err),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )
                })?;

        tx.commit().await.map_err(|err| {
            poem::Error::from_string(
                format!("트랜잭션 커밋 실패: {}", err),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;

        Ok(Json(ApiResponse {
            status: true,
            data: Some(crate::models::PortfolioResponse {
                portfolio_id,
                slug: slug.to_string(),
                content,
                created_at,
                updated_at,
            }),
            message: Some(String::from("포트폴리오를 업데이트 했습니다.")),
        }))
    }

    /// 포트폴리오 섹션별 저장. 섹션 키에 따라 다른 스키마로 런타임 검증한다 —
    /// 이 다형성은 OpenAPI 스키마로 정적 표현이 안 돼서 `content`는 스펙상 "any"로 노출된다.
    #[oai(path = "/portfolio/sections/:section_key", method = "put")]
    async fn update_portfolio_section(
        &self,
        guard: AuthGuard,
        Path(section_key): Path<String>,
        payload: Json<UpdateSectionRequest>,
    ) -> poem::Result<Json<ApiResponse<UpdateSectionResponse>>> {
        ensure_admin(&guard.0)?;

        let serialized = validate_portfolio_section(&section_key, &payload.0.content.0)
            .map_err(|err| poem::Error::from_string(err, StatusCode::BAD_REQUEST))?;

        let portfolio_id: Option<i32> =
            sqlx::query_scalar("SELECT portfolio_id FROM portfolio WHERE slug = ?")
                .bind("dev")
                .fetch_optional(&self.state.db)
                .await
                .map_err(|err| {
                    poem::Error::from_string(
                        format!("포트폴리오 조회 실패: {}", err),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )
                })?;
        let portfolio_id = portfolio_id.ok_or_else(|| {
            poem::Error::from_string(
                "포트폴리오 데이터를 찾지 못했습니다.",
                StatusCode::NOT_FOUND,
            )
        })?;

        query(
            r#"
            INSERT INTO portfolio_section (portfolio_id, section_key, content, created_at, updated_at)
            VALUES (?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
            ON CONFLICT(portfolio_id, section_key) DO UPDATE SET
                content = excluded.content,
                updated_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(portfolio_id)
        .bind(&section_key)
        .bind(&serialized)
        .execute(&self.state.db)
        .await
        .map_err(|err| {
            poem::Error::from_string(
                format!("{} 섹션 저장 실패: {}", section_key, err),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;

        let updated_at: String = sqlx::query_scalar(
            "SELECT updated_at FROM portfolio_section WHERE portfolio_id = ? AND section_key = ?",
        )
        .bind(portfolio_id)
        .bind(&section_key)
        .fetch_one(&self.state.db)
        .await
        .map_err(|err| {
            poem::Error::from_string(
                format!("updated_at 조회 실패: {}", err),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;

        Ok(Json(ApiResponse {
            status: true,
            data: Some(UpdateSectionResponse {
                section_key,
                updated_at,
            }),
            message: Some(String::from("섹션을 업데이트 했습니다.")),
        }))
    }

    /// 포트폴리오 전체 삭제.
    #[oai(path = "/portfolio", method = "delete")]
    async fn delete_portfolio(&self, guard: AuthGuard) -> poem::Result<DeletePortfolioResponse> {
        ensure_admin(&guard.0)?;

        let portfolio_id: Option<i32> =
            sqlx::query_scalar("SELECT portfolio_id FROM portfolio WHERE slug = ?")
                .bind("dev")
                .fetch_optional(&self.state.db)
                .await
                .map_err(|err| {
                    poem::Error::from_string(
                        format!("포트폴리오 조회 실패: {}", err),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )
                })?;
        let portfolio_id = portfolio_id.ok_or_else(|| {
            poem::Error::from_string(
                "포트폴리오 데이터를 찾지 못했습니다.",
                StatusCode::NOT_FOUND,
            )
        })?;

        let mut tx = self.state.db.begin().await.map_err(|err| {
            poem::Error::from_string(
                format!("트랜잭션 시작 실패: {}", err),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;

        query("DELETE FROM portfolio_section WHERE portfolio_id = ?")
            .bind(portfolio_id)
            .execute(&mut *tx)
            .await
            .map_err(|err| {
                poem::Error::from_string(
                    format!("포트폴리오 섹션 삭제 실패: {}", err),
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
            })?;
        query("DELETE FROM portfolio WHERE portfolio_id = ?")
            .bind(portfolio_id)
            .execute(&mut *tx)
            .await
            .map_err(|err| {
                poem::Error::from_string(
                    format!("포트폴리오 삭제 실패: {}", err),
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
            })?;

        tx.commit().await.map_err(|err| {
            poem::Error::from_string(
                format!("트랜잭션 커밋 실패: {}", err),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;

        Ok(DeletePortfolioResponse::NoContent)
    }
}

pub struct WeddingApi {
    pub state: Arc<AppState>,
}

#[OpenApi]
impl WeddingApi {
    /// 하객 실명이 담긴 목록이라 AdminOnly.
    #[oai(path = "/admin/wedding/guests", method = "get")]
    async fn get_wedding_guests(
        &self,
        guard: AuthGuard,
    ) -> poem::Result<Json<ApiResponse<crate::models::WeddingGuestListResponse>>> {
        ensure_admin(&guard.0)?;

        let guests = query_as::<_, crate::models::WeddingGuest>(
            r#"
            SELECT guest_id, name, companions, visit_hour, created_at
            FROM wedding_guests
            ORDER BY created_at DESC, guest_id DESC
            "#,
        )
        .fetch_all(&self.state.db)
        .await
        .map_err(|err| {
            poem::Error::from_string(
                format!("Error fetching wedding guests: {}", err),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;

        let total_guests = guests.len() as i64;
        let total_attendees = guests.iter().map(|g| g.companions + 1).sum();

        Ok(Json(ApiResponse {
            status: true,
            data: Some(crate::models::WeddingGuestListResponse {
                guests,
                total_guests,
                total_attendees,
            }),
            message: None,
        }))
    }
}
