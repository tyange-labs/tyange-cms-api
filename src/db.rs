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

    // budget_periods
    query(
        r#"
        CREATE TABLE IF NOT EXISTS budget_periods (
            budget_id INTEGER PRIMARY KEY AUTOINCREMENT,
            owner_user_id TEXT NOT NULL,
            total_budget INTEGER NOT NULL,
            from_date DATE NOT NULL,
            to_date DATE NOT NULL,
            alert_threshold REAL NOT NULL DEFAULT 0.85,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_budget_periods_owner_updated_at
        ON budget_periods(owner_user_id, updated_at DESC, budget_id DESC)
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    // spending_records
    query(
        r#"
        CREATE TABLE IF NOT EXISTS spending_records (
            record_id INTEGER PRIMARY KEY AUTOINCREMENT,
            owner_user_id TEXT NOT NULL,
            amount INTEGER NOT NULL,
            merchant TEXT,
            transacted_at DATETIME NOT NULL,
            source_type TEXT,
            source_fingerprint TEXT,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_spending_records_owner_transacted_at
        ON spending_records(owner_user_id, transacted_at)
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    query(
        r#"
        CREATE UNIQUE INDEX IF NOT EXISTS idx_spending_records_import_fingerprint
        ON spending_records(owner_user_id, source_type, source_fingerprint)
        WHERE source_type IS NOT NULL AND source_fingerprint IS NOT NULL
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    // user_matches
    query(
        r#"
        CREATE TABLE IF NOT EXISTS user_matches (
            match_id INTEGER PRIMARY KEY AUTOINCREMENT,
            requester_user_id TEXT NOT NULL,
            target_user_id TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            responded_at DATETIME,
            closed_at DATETIME
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_user_matches_requester_status
        ON user_matches(requester_user_id, status, created_at DESC, match_id DESC)
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_user_matches_target_status
        ON user_matches(target_user_id, status, created_at DESC, match_id DESC)
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    // match_messages
    query(
        r#"
        CREATE TABLE IF NOT EXISTS match_messages (
            message_id INTEGER PRIMARY KEY AUTOINCREMENT,
            match_id INTEGER NOT NULL,
            sender_user_id TEXT NOT NULL,
            receiver_user_id TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (match_id) REFERENCES user_matches(match_id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_match_messages_match_created_at
        ON match_messages(match_id, created_at ASC, message_id ASC)
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    // rss_sources
    query(
        r#"
        CREATE TABLE IF NOT EXISTS rss_sources (
            source_id TEXT PRIMARY KEY,
            feed_url TEXT NOT NULL,
            normalized_feed_url TEXT NOT NULL UNIQUE,
            title TEXT,
            site_url TEXT,
            etag TEXT,
            last_modified TEXT,
            last_polled_at DATETIME,
            last_success_at DATETIME,
            last_error TEXT,
            consecutive_failures INTEGER NOT NULL DEFAULT 0,
            is_active BOOLEAN NOT NULL DEFAULT true,
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_rss_sources_active
        ON rss_sources(is_active, updated_at DESC)
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    // rss_feed_items
    query(
        r#"
        CREATE TABLE IF NOT EXISTS rss_feed_items (
            item_id INTEGER PRIMARY KEY AUTOINCREMENT,
            source_id TEXT NOT NULL,
            item_guid_hash TEXT NOT NULL,
            guid_or_link TEXT,
            title TEXT NOT NULL,
            link TEXT,
            published_at TEXT,
            detected_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (source_id) REFERENCES rss_sources(source_id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    query(
        r#"
        CREATE UNIQUE INDEX IF NOT EXISTS idx_rss_feed_items_source_guid_hash
        ON rss_feed_items(source_id, item_guid_hash)
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    // user_rss_subscriptions
    query(
        r#"
        CREATE TABLE IF NOT EXISTS user_rss_subscriptions (
            subscription_id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id TEXT NOT NULL,
            source_id TEXT NOT NULL,
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (source_id) REFERENCES rss_sources(source_id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    query(
        r#"
        CREATE UNIQUE INDEX IF NOT EXISTS idx_user_rss_subscriptions_user_source
        ON user_rss_subscriptions(user_id, source_id)
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_user_rss_subscriptions_source
        ON user_rss_subscriptions(source_id)
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

    // push_delivery_logs
    query(
        r#"
        CREATE TABLE IF NOT EXISTS push_delivery_logs (
            delivery_id INTEGER PRIMARY KEY AUTOINCREMENT,
            push_subscription_id INTEGER NOT NULL,
            item_id INTEGER NOT NULL,
            status TEXT NOT NULL,
            error_message TEXT,
            sent_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (push_subscription_id) REFERENCES web_push_subscriptions(push_subscription_id),
            FOREIGN KEY (item_id) REFERENCES rss_feed_items(item_id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    query(
        r#"
        CREATE UNIQUE INDEX IF NOT EXISTS idx_push_delivery_logs_subscription_item
        ON push_delivery_logs(push_subscription_id, item_id)
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    Ok(())
}


