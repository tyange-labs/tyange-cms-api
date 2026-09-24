use crate::blog_redeploy::{is_blog_redeploy_target, BlogContentEvent, BlogVisibility};
use crate::models::{
    CustomResponse, Post, PostResponseDb, Tag, TagWithCategory, UpdatePostRequest,
};
use crate::utils::parse_tags;
use crate::AppState;
use poem::http::StatusCode;
use poem::web::{Data, Json, Path};
use poem::{handler, Error, Request};
use sqlx::{query, query_as, Sqlite};
use std::sync::Arc;
use tyange_cms_api::auth::authorization::{current_user, ensure_post_owner};

#[handler]
pub async fn update_post(
    req: &Request,
    Path(post_id): Path<String>,
    Json(payload): Json<UpdatePostRequest>,
    data: Data<&Arc<AppState>>,
) -> Result<Json<CustomResponse<Post>>, Error> {
    let user = current_user(req)?;
    ensure_post_owner(user, &post_id, &data.db).await?;
    let existing_post = fetch_existing_post(&data.db, &post_id).await?;
    let redeploy_event = determine_redeploy_event(&existing_post, &payload);

    let mut tx = data.db.begin().await.map_err(|e| {
        Error::from_string(
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
        Error::from_string(
            format!("포스트 업데이트 실패: {}", e),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
    })?;

    if updated.rows_affected() == 0 {
        return Err(Error::from_string(
            "게시글을 찾을 수 없습니다.",
            StatusCode::NOT_FOUND,
        ));
    }

    query("DELETE FROM post_tags WHERE post_id = ?")
        .bind(&post_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            Error::from_string(
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
                Error::from_string(
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
            Error::from_string(
                format!("포스트-태그 관계 저장 실패: {}", e),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;
    }

    tx.commit().await.map_err(|e| {
        Error::from_string(
            format!("트랜잭션 커밋 실패: {}", e),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
    })?;

    if let Some((content_event, visibility)) = redeploy_event {
        data.blog_redeploy
            .dispatch_content_change(content_event, &post_id, visibility)
            .await;
    }

    Ok(Json(CustomResponse {
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
) -> Result<ExistingPostSnapshot, Error> {
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
        Error::from_string(
            format!("기존 포스트 조회 실패: {}", e),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
    })?
    .ok_or_else(|| Error::from_string("게시글을 찾을 수 없습니다.", StatusCode::NOT_FOUND))?;

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
