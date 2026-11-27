//! 글 쓰기·고치기·지우기 로직. HTTP 라우트(`openapi.rs::PostsApi`)와 MCP 도구(`mcp`)가 같이 쓴다.
//! 권한 확인(쓰기 역할, 작성자 본인·관리자)과 블로그 재배포 트리거까지 여기서 한다.

use poem::http::StatusCode;
use sqlx::{query, query_as, Sqlite};
use tyange_cms_api::auth::authorization::{
    ensure_content_writer, ensure_post_owner, AuthenticatedUser,
};

use crate::blog_redeploy::{is_blog_redeploy_target, BlogContentEvent, BlogVisibility};
use crate::models::{
    AppState, Post, PostResponseDb, Tag, TagWithCategory, UpdatePostRequest, UploadPostRequest,
};
use crate::utils::parse_tags;

pub async fn create_post(
    state: &AppState,
    user: &AuthenticatedUser,
    payload: UploadPostRequest,
) -> poem::Result<String> {
    ensure_content_writer(user)?;
    let post_id = uuid::Uuid::new_v4().to_string();

    let mut tx = state.db.begin().await.map_err(|e| {
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
        state
            .blog_redeploy
            .dispatch_content_change(BlogContentEvent::Publish, &post_id, BlogVisibility::Visible)
            .await;
    }

    Ok(post_id)
}

pub async fn update_post(
    state: &AppState,
    user: &AuthenticatedUser,
    post_id: &str,
    payload: UpdatePostRequest,
) -> poem::Result<Post> {
    let post_id = post_id.to_string();
    ensure_content_writer(user)?;
    ensure_post_owner(user, &post_id, &state.db).await?;
    let existing_post = fetch_existing_post(&state.db, &post_id).await?;
    let redeploy_event = determine_redeploy_event(&existing_post, &payload);

    let mut tx = state.db.begin().await.map_err(|e| {
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
        state
            .blog_redeploy
            .dispatch_content_change(content_event, &post_id, visibility)
            .await;
    }

    Ok(Post {
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
    })
}

pub async fn delete_post(
    state: &AppState,
    user: &AuthenticatedUser,
    post_id: &str,
) -> poem::Result<()> {
    let post_id = post_id.to_string();
    ensure_content_writer(user)?;
    ensure_post_owner(user, &post_id, &state.db).await?;

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
    .fetch_one(&state.db)
    .await
    .map_err(|err| {
        poem::Error::from_string(
            format!("게시글 조회에 실패했습니다: {}", err),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
    })?;

    // posts를 참조하는 행을 먼저 정리한다. post_tags는 지우고, images는 파일과 행을 남긴 채 글과의
    // 연결만 끊는다(외래키(images.post_id)가 cascade가 아니라서 그냥 지우면 500이 난다).
    // 한 트랜잭션이라 글이 없어서 404가 나면 아무것도 바뀌지 않는다.
    let internal = |what: &str, err: sqlx::Error| {
        poem::Error::from_string(format!("{what}: {err}"), StatusCode::INTERNAL_SERVER_ERROR)
    };
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|err| internal("트랜잭션 시작 실패", err))?;
    query("DELETE FROM post_tags WHERE post_id = ?")
        .bind(&post_id)
        .execute(&mut *tx)
        .await
        .map_err(|err| internal("태그 연결 삭제 실패", err))?;
    query("UPDATE images SET post_id = NULL WHERE post_id = ?")
        .bind(&post_id)
        .execute(&mut *tx)
        .await
        .map_err(|err| internal("이미지 연결 해제 실패", err))?;
    let result = query("DELETE FROM posts WHERE post_id = ?")
        .bind(&post_id)
        .execute(&mut *tx)
        .await
        .map_err(|err| internal("게시글 삭제 실패", err))?;

    if result.rows_affected() == 0 {
        return Err(poem::Error::from_string(
            "게시글을 찾을 수 없습니다.",
            StatusCode::NOT_FOUND,
        ));
    }
    tx.commit()
        .await
        .map_err(|err| internal("트랜잭션 커밋 실패", err))?;

    if is_blog_redeploy_target(
        &existing_post.status,
        parse_tags(&existing_post.tags)
            .iter()
            .map(|tag| tag.tag.as_str()),
    ) {
        state
            .blog_redeploy
            .dispatch_content_change(BlogContentEvent::Delete, &post_id, BlogVisibility::Hidden)
            .await;
    }

    Ok(())
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
