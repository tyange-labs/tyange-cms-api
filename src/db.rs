use poem::{Result, error::InternalServerError};
use sqlx::{SqlitePool, query};

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
            FOREIGN KEY (post_id) REFERENCES posts(post_id)
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

    Ok(())
}


