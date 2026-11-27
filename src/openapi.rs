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
use tyange_cms_api::auth::authorization::{
    ensure_admin, ensure_content_writer, AuthGuard, SessionGuard,
};

use crate::models::{
    ApiResponse, AppState, DeletePostResponse, Post, PostItem, PostResponseDb, PostsResponse,
    UpdatePostRequest, UploadPostRequest, UploadPostResponse,
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
                        excerpt: None,
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
               COALESCE(substr(p.content, 1, 300), '') AS excerpt,
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
                        excerpt: Some(db_post.get("excerpt")),
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
        let post_id = crate::posts::create_post(&self.state, &guard.0, payload.0).await?;

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
        let post = crate::posts::update_post(&self.state, &guard.0, &post_id, payload.0).await?;

        Ok(Json(ApiResponse {
            status: true,
            data: Some(post),
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
        crate::posts::delete_post(&self.state, &guard.0, &post_id).await?;

        Ok(Json(ApiResponse {
            status: true,
            data: Some(DeletePostResponse { post_id }),
            message: Some(String::from("포스트가 삭제되었습니다.")),
        }))
    }
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
        ensure_content_writer(&guard.0)?;

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
struct SectionContentResponse {
    section_key: String,
    content: poem_openapi::types::Any<serde_json::Value>,
    updated_at: String,
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
        "portfolio_pdf" => {
            use crate::models::{PORTFOLIO_PDF_MAX_BYTES, PORTFOLIO_PDF_MAX_PAGES};
            let parsed: crate::models::PortfolioPdfSection = serde_json::from_value(value.clone())
                .map_err(|e| format!("portfolio_pdf 검증 실패: {}", e))?;
            if parsed.pages.len() > PORTFOLIO_PDF_MAX_PAGES {
                return Err(format!("portfolio_pdf 페이지는 최대 {}장입니다.", PORTFOLIO_PDF_MAX_PAGES));
            }
            let mut seen = std::collections::HashSet::new();
            for page in &parsed.pages {
                if page.id.trim().is_empty() || page.template.trim().is_empty() {
                    return Err("portfolio_pdf 페이지의 id와 template은 비어 있을 수 없습니다.".to_string());
                }
                if !seen.insert(page.id.as_str()) {
                    return Err(format!("portfolio_pdf 페이지 id가 중복됩니다: {}", page.id));
                }
            }
            let serialized = serde_json::to_string(&parsed).map_err(|e| e.to_string())?;
            if serialized.len() > PORTFOLIO_PDF_MAX_BYTES {
                return Err(format!("portfolio_pdf 문서는 {}KB 이하여야 합니다.", PORTFOLIO_PDF_MAX_BYTES / 1024));
            }
            Ok(serialized)
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

    /// 섹션 하나의 저장된 JSON을 그대로 돌려준다(관리자 전용). 공개 `GET /portfolio`에
    /// 섞지 않는 `portfolio_pdf`처럼 편집기만 읽는 섹션용이다.
    #[oai(path = "/portfolio/sections/:section_key", method = "get")]
    async fn get_portfolio_section(
        &self,
        guard: AuthGuard,
        Path(section_key): Path<String>,
    ) -> poem::Result<Json<ApiResponse<SectionContentResponse>>> {
        ensure_admin(&guard.0)?;

        let row: Option<(String, String)> = sqlx::query_as(
            r#"
            SELECT s.content, s.updated_at
            FROM portfolio_section s
            JOIN portfolio p ON p.portfolio_id = s.portfolio_id
            WHERE p.slug = ? AND s.section_key = ?
            "#,
        )
        .bind("dev")
        .bind(&section_key)
        .fetch_optional(&self.state.db)
        .await
        .map_err(|err| {
            poem::Error::from_string(
                format!("{} 섹션 조회 실패: {}", section_key, err),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;
        let (content, updated_at) = row.ok_or_else(|| {
            poem::Error::from_string(
                format!("{} 섹션이 아직 없습니다.", section_key),
                StatusCode::NOT_FOUND,
            )
        })?;
        let value: serde_json::Value = serde_json::from_str(&content).map_err(|err| {
            poem::Error::from_string(
                format!("{} 섹션 파싱 실패: {}", section_key, err),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;

        Ok(Json(ApiResponse {
            status: true,
            data: Some(SectionContentResponse {
                section_key,
                content: poem_openapi::types::Any(value),
                updated_at,
            }),
            message: None,
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

#[derive(Debug, serde::Deserialize, poem_openapi::Object)]
pub struct CreateApiKeyRequest {
    pub name: String,
    /// `admin`(관리 API까지) 또는 `user`(글·이미지 쓰기만).
    pub role: String,
}

#[derive(Debug, serde::Serialize, poem_openapi::Object)]
pub struct CreatedApiKey {
    pub api_key_id: i64,
    pub name: String,
    pub role: String,
    /// 키 원문. **이 응답에서만** 볼 수 있다(서버는 해시만 가진다).
    pub key: String,
    pub created_at: String,
}

#[derive(Debug, serde::Serialize, poem_openapi::Object)]
pub struct ApiKeyItem {
    pub api_key_id: i64,
    pub name: String,
    pub role: String,
    /// 키 앞부분(`tyk_` + 12자). 어떤 키인지 알아보는 용도이고 이것만으론 인증되지 않는다.
    pub key_prefix: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub revoked_at: Option<String>,
}

#[derive(Debug, serde::Serialize, poem_openapi::Object)]
pub struct ApiKeyListResponse {
    pub keys: Vec<ApiKeyItem>,
}

/// 사용자당 살아 있는(폐기하지 않은) 키 상한.
const MAX_ACTIVE_API_KEYS: i64 = 20;
const MAX_API_KEY_NAME_CHARS: usize = 50;

pub struct ApiKeysApi {
    pub state: Arc<AppState>,
}

/// 키 발급·조회·폐기는 **로그인 JWT로만** 된다(`SessionGuard`) — API 키로는 키를 관리할 수 없다.
#[OpenApi]
impl ApiKeysApi {
    /// API 키를 만든다. 원문은 이 응답에서 한 번만 보여 준다. 관리자 전용.
    #[oai(path = "/admin/api-keys", method = "post")]
    async fn create_api_key(
        &self,
        guard: SessionGuard,
        payload: Json<CreateApiKeyRequest>,
    ) -> poem::Result<Json<ApiResponse<CreatedApiKey>>> {
        ensure_admin(&guard.0)?;
        let payload = payload.0;

        let name = payload.name.trim().to_string();
        if name.is_empty() || name.chars().count() > MAX_API_KEY_NAME_CHARS {
            return Err(poem::Error::from_string(
                format!("이름은 1~{MAX_API_KEY_NAME_CHARS}자여야 합니다."),
                StatusCode::BAD_REQUEST,
            ));
        }
        if !matches!(payload.role.as_str(), "admin" | "user") {
            return Err(poem::Error::from_string(
                "role은 admin 또는 user여야 합니다.",
                StatusCode::BAD_REQUEST,
            ));
        }

        let active: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM api_keys WHERE user_id = ? AND revoked_at IS NULL",
        )
        .bind(&guard.0.user_id)
        .fetch_one(&self.state.db)
        .await
        .map_err(|e| {
            poem::Error::from_string(format!("키 조회 실패: {e}"), StatusCode::INTERNAL_SERVER_ERROR)
        })?;
        if active >= MAX_ACTIVE_API_KEYS {
            return Err(poem::Error::from_string(
                format!("키는 최대 {MAX_ACTIVE_API_KEYS}개까지 만들 수 있습니다. 안 쓰는 키를 폐기하세요."),
                StatusCode::CONFLICT,
            ));
        }

        let key = tyange_cms_api::auth::api_key::generate();
        let row: (i64, String) = sqlx::query_as(
            "INSERT INTO api_keys (user_id, name, key_lookup, key_hash, user_role)
             VALUES (?, ?, ?, ?, ?)
             RETURNING api_key_id, COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', created_at), '')",
        )
        .bind(&guard.0.user_id)
        .bind(&name)
        .bind(&key.lookup)
        .bind(&key.hash)
        .bind(&payload.role)
        .fetch_one(&self.state.db)
        .await
        .map_err(|e| {
            poem::Error::from_string(format!("키 저장 실패: {e}"), StatusCode::INTERNAL_SERVER_ERROR)
        })?;

        Ok(Json(ApiResponse {
            status: true,
            data: Some(CreatedApiKey {
                api_key_id: row.0,
                name,
                role: payload.role,
                key: key.plaintext,
                created_at: row.1,
            }),
            message: Some(String::from("키 원문은 지금만 볼 수 있습니다. 안전한 곳에 저장하세요.")),
        }))
    }

    /// 내 API 키 목록(폐기한 것 포함, 최신순). 키 원문은 나오지 않는다. 관리자 전용.
    #[oai(path = "/admin/api-keys", method = "get")]
    async fn list_api_keys(
        &self,
        guard: SessionGuard,
    ) -> poem::Result<Json<ApiResponse<ApiKeyListResponse>>> {
        ensure_admin(&guard.0)?;

        let rows = query(
            "SELECT api_key_id, name, user_role, key_lookup,
                    COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', created_at), '') AS created_at,
                    strftime('%Y-%m-%dT%H:%M:%SZ', last_used_at) AS last_used_at,
                    strftime('%Y-%m-%dT%H:%M:%SZ', revoked_at) AS revoked_at
             FROM api_keys WHERE user_id = ? ORDER BY api_key_id DESC",
        )
        .bind(&guard.0.user_id)
        .fetch_all(&self.state.db)
        .await
        .map_err(|e| {
            poem::Error::from_string(format!("키 조회 실패: {e}"), StatusCode::INTERNAL_SERVER_ERROR)
        })?;

        let keys = rows
            .iter()
            .map(|row| ApiKeyItem {
                api_key_id: row.get("api_key_id"),
                name: row.get("name"),
                role: row.get("user_role"),
                key_prefix: format!("{}{}", tyange_cms_api::auth::api_key::KEY_PREFIX, row.get::<String, _>("key_lookup")),
                created_at: row.get("created_at"),
                last_used_at: row.get("last_used_at"),
                revoked_at: row.get("revoked_at"),
            })
            .collect();

        Ok(Json(ApiResponse {
            status: true,
            data: Some(ApiKeyListResponse { keys }),
            message: None,
        }))
    }

    /// 키를 폐기한다(행은 남기고 `revoked_at`만 채운다). 이미 폐기했으면 404. 관리자 전용.
    #[oai(path = "/admin/api-keys/:api_key_id", method = "delete")]
    async fn revoke_api_key(
        &self,
        guard: SessionGuard,
        Path(api_key_id): Path<i64>,
    ) -> poem::Result<Json<ApiResponse<ApiKeyRevoked>>> {
        ensure_admin(&guard.0)?;

        let updated = query(
            "UPDATE api_keys SET revoked_at = CURRENT_TIMESTAMP
             WHERE api_key_id = ? AND user_id = ? AND revoked_at IS NULL",
        )
        .bind(api_key_id)
        .bind(&guard.0.user_id)
        .execute(&self.state.db)
        .await
        .map_err(|e| {
            poem::Error::from_string(format!("키 폐기 실패: {e}"), StatusCode::INTERNAL_SERVER_ERROR)
        })?;
        if updated.rows_affected() == 0 {
            return Err(poem::Error::from_string(
                "폐기할 키를 찾을 수 없습니다.",
                StatusCode::NOT_FOUND,
            ));
        }

        Ok(Json(ApiResponse {
            status: true,
            data: Some(ApiKeyRevoked { api_key_id }),
            message: None,
        }))
    }
}

#[derive(Debug, serde::Serialize, poem_openapi::Object)]
pub struct ApiKeyRevoked {
    pub api_key_id: i64,
}
