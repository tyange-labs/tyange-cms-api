use poem::{error::InternalServerError, Result};
use sqlx::{query, SqlitePool};

pub async fn init_db(pool: &SqlitePool) -> Result<()> {
    // posts
    query(
        r#"
        CREATE TABLE IF NOT EXISTS posts (
            post_id TEXT PRIMARY KEY,
            title TEXT,
            description TEXT,
            published_at DATETIME,
            tags TEXT,
            content TEXT,
            writer_id TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    // images
    query(
        r#"
        CREATE TABLE IF NOT EXISTS images (
            image_id TEXT PRIMARY KEY,
            post_id TEXT,
            file_name TEXT NOT NULL,
            origin_name TEXT NOT NULL,
            file_path TEXT NOT NULL,
            mime_type TEXT NOT NULL,
            image_type TEXT NOT NULL,
            uploaded_at TEXT DEFAULT CURRENT_TIMESTAMP,
            uploader_ip TEXT,
            status TEXT NOT NULL DEFAULT 'confirmed',
            FOREIGN KEY (post_id) REFERENCES posts(post_id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    // 기존 DB 마이그레이션: 컬럼이 이미 있으면 duplicate column 에러가 나므로 무시
    for stmt in [
        "ALTER TABLE images ADD COLUMN uploader_ip TEXT",
        "ALTER TABLE images ADD COLUMN status TEXT NOT NULL DEFAULT 'confirmed'",
    ] {
        let _ = query(stmt).execute(pool).await;
    }

    // tags / post_tags
    // 예전엔 posts.tags(TEXT)에 태그를 직접 박아 쓰다가 이 정규화 테이블로 옮겼는데,
    // 그 마이그레이션 SQL이 여기 init_db()에는 반영이 안 돼 있었다. 운영 DB는 이미
    // 수동으로 테이블이 있어서 안 겪었지만, 새 DB(로컬 개발 등)로 시작하면 태그가
    // 걸린 쿼리가 전부 "no such table" 500을 낸다.
    // name UNIQUE인 이유: update_post/upload_post의 `INSERT OR IGNORE INTO tags
    // (name, category) VALUES (?, ?)`가 같은 이름이면 새로 안 만들고 기존 걸 재사용하길
    // 기대하는데, name에 유니크 제약이 없으면 INSERT OR IGNORE가 아무것도 걸러내지 못하고
    // 매번 중복 행을 쌓는다.
    query(
        r#"
        CREATE TABLE IF NOT EXISTS tags (
            tag_id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            category TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    query(
        r#"
        CREATE TABLE IF NOT EXISTS post_tags (
            post_id TEXT NOT NULL,
            tag_id INTEGER NOT NULL,
            PRIMARY KEY (post_id, tag_id),
            FOREIGN KEY (post_id) REFERENCES posts(post_id),
            FOREIGN KEY (tag_id) REFERENCES tags(tag_id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    // users
    query(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            user_id TEXT PRIMARY KEY,
            password TEXT,
            user_role TEXT NOT NULL,
            auth_provider TEXT NOT NULL DEFAULT 'local',
            google_sub TEXT,
            display_name TEXT,
            avatar_url TEXT,
            bio TEXT
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    query(
        r#"
        CREATE UNIQUE INDEX IF NOT EXISTS idx_users_google_sub
        ON users(google_sub)
        WHERE google_sub IS NOT NULL
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    // portfolio (master)
    query(
        r#"
        CREATE TABLE IF NOT EXISTS portfolio (
            portfolio_id INTEGER PRIMARY KEY AUTOINCREMENT,
            slug TEXT NOT NULL UNIQUE,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    // portfolio_section
    query(
        r#"
        CREATE TABLE IF NOT EXISTS portfolio_section (
            section_id INTEGER PRIMARY KEY AUTOINCREMENT,
            portfolio_id INTEGER NOT NULL,
            section_key TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(portfolio_id, section_key)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    // web_push_subscriptions
    query(
        r#"
        CREATE TABLE IF NOT EXISTS web_push_subscriptions (
            push_subscription_id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id TEXT NOT NULL,
            endpoint TEXT NOT NULL UNIQUE,
            p256dh TEXT NOT NULL,
            auth TEXT NOT NULL,
            user_agent TEXT,
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            last_success_at DATETIME,
            last_failure_at DATETIME,
            failure_count INTEGER NOT NULL DEFAULT 0,
            revoked_at DATETIME
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_web_push_subscriptions_user_revoked
        ON web_push_subscriptions(user_id, revoked_at)
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    // wedding_guests (lovelog RSVP)
    query(
        r#"
        CREATE TABLE IF NOT EXISTS wedding_guests (
            guest_id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            companions INTEGER NOT NULL DEFAULT 0,
            visit_hour INTEGER,
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    // visit_hour는 테이블을 만든 뒤에 추가한 컬럼이다. 이미 돌고 있는 배포본의 테이블에는
    // CREATE TABLE IF NOT EXISTS가 아무 일도 하지 않으므로 없을 때만 따로 붙여준다.
    // 기존 행은 NULL로 남고, 그건 "시간을 받기 전에 접수된 하객"을 뜻한다.
    let has_visit_hour = query(
        r#"
        SELECT 1 FROM pragma_table_info('wedding_guests') WHERE name = 'visit_hour'
        "#,
    )
    .fetch_optional(pool)
    .await
    .map_err(InternalServerError)?
    .is_some();

    if !has_visit_hour {
        query(
            r#"
            ALTER TABLE wedding_guests ADD COLUMN visit_hour INTEGER
            "#,
        )
        .execute(pool)
        .await
        .map_err(InternalServerError)?;
    }

    Ok(())
}
