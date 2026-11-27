//! 글 쓰기·고치기·지우기 로직. HTTP 라우트(`openapi.rs::PostsApi`)와 MCP 도구(`mcp`)가 같이 쓴다.
//! 권한 확인(쓰기 역할, 작성자 본인·관리자)까지 여기서 한다.

use poem::http::StatusCode;
use sqlx::{query, query_as, Sqlite};
use tyange_cms_api::auth::authorization::{
    ensure_content_writer, ensure_post_owner, AuthenticatedUser,
};

use crate::models::{
    AppState, Post, PostResponseDb, TagWithCategory, UpdatePostRequest, UploadPostRequest,
};
use crate::utils::parse_tags;

/// 글 하나(본문·태그 포함). 없으면 `None`.
pub async fn find_post(db: &sqlx::Pool<Sqlite>, post_id: &str) -> Result<Option<Post>, sqlx::Error> {
    let row = query_as::<Sqlite, PostResponseDb>(
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
    .await?;
    Ok(row.map(|p| Post {
        post_id: p.post_id,
        title: p.title,
        description: p.description,
        published_at: p.published_at,
        created_at: p.created_at,
        tags: parse_tags(&p.tags),
        content: p.content,
        status: p.status,
    }))
}

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
    let created_at = fetch_created_at(&state.db, &post_id).await?;

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

    Ok(Post {
        post_id,
        title: payload.title,
        description: payload.description,
        published_at: payload.published_at,
        created_at,
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

    Ok(())
}

async fn fetch_created_at(db: &sqlx::Pool<Sqlite>, post_id: &str) -> poem::Result<String> {
    query_as::<Sqlite, (String,)>(
        "SELECT COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', created_at), '') FROM posts WHERE post_id = ?",
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
    .map(|(created_at,)| created_at)
    .ok_or_else(|| poem::Error::from_string("게시글을 찾을 수 없습니다.", StatusCode::NOT_FOUND))
}
