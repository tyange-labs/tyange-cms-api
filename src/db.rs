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
            FOREIGN KEY (post_id) REFERENCES posts(post_id) ON DELETE CASCADE,
            FOREIGN KEY (tag_id) REFERENCES tags(tag_id) ON DELETE CASCADE
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
            display_name TEXT,
            avatar_url TEXT,
            bio TEXT
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    // Google 로그인 기능 제거(2026-09) — 실사용 안 해서 로그인 경로만 걷어냄.
    // auth_provider 컬럼은 범용이라 유지(항상 'local'이 됨). google_sub는 그 로그인
    // 전용 컬럼이라 같이 제거. 이미 배포된 DB 정리용 DROP — 반복 실행해도 안전.
    let _ = query("DROP INDEX IF EXISTS idx_users_google_sub")
        .execute(pool)
        .await;
    let _ = query("ALTER TABLE users DROP COLUMN google_sub")
        .execute(pool)
        .await;

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

    // Web Push 기능 제거(2026-09) — 실사용 안 해서 라우트/구독 테이블 전부 걷어냄.
    // 이미 배포된 DB에 남아있을 테이블/인덱스를 정리한다. DROP은 실패해도(이미
    // 지워졌으면) 무시 — 반복 실행해도 안전.
    let _ = query("DROP INDEX IF EXISTS idx_web_push_subscriptions_user_revoked")
        .execute(pool)
        .await;
    let _ = query("DROP TABLE IF EXISTS web_push_subscriptions")
        .execute(pool)
        .await;

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

    // passkeys — WebAuthn 자격 증명. passkey 칼럼은 webauthn-rs `Passkey`를 JSON으로 직렬화한 값
    // (공개키·서명 카운터 포함)이고, 로그인할 때마다 카운터가 바뀌면 갱신한다. user_handle은
    // 사용자별로 고정된 WebAuthn user.id(UUID)로, 사용자명 없는 로그인에서 계정을 찾는 키다.
    query(
        r#"
        CREATE TABLE IF NOT EXISTS passkeys (
            credential_id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL,
            user_handle TEXT NOT NULL,
            label TEXT NOT NULL,
            passkey TEXT NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            last_used_at DATETIME,
            FOREIGN KEY (user_id) REFERENCES users(user_id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    query("CREATE INDEX IF NOT EXISTS idx_passkeys_user_id ON passkeys(user_id)")
        .execute(pool)
        .await
        .map_err(InternalServerError)?;
    query("CREATE INDEX IF NOT EXISTS idx_passkeys_user_handle ON passkeys(user_handle)")
        .execute(pool)
        .await
        .map_err(InternalServerError)?;

    // gotolib_sync — gotolib(도서관 찾기 앱)의 기기 간 동기화 문서. 사용자당 한 행.
    // 서버는 내용을 해석하지 않고(JSON 객체) 버전만 관리한다: version이 맞을 때만 쓸 수 있어서
    // 두 기기가 동시에 올려도 한쪽이 다른 쪽을 조용히 덮어쓰지 못한다.
    query(
        r#"
        CREATE TABLE IF NOT EXISTS gotolib_sync (
            user_id TEXT PRIMARY KEY,
            version INTEGER NOT NULL,
            data TEXT NOT NULL,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (user_id) REFERENCES users(user_id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    // book_covers — gotolib 책 표지 썸네일의 수집 상태. 키는 ISBN-13.
    // status: pending(수집 대기) / ok(저장됨) / missing(알라딘에 표지 없음) / error(일시 실패).
    // 시각은 유닉스 초. next_try_at은 error·missing을 다시 시도해도 되는 시각이다.
    query(
        r#"
        CREATE TABLE IF NOT EXISTS book_covers (
            isbn13 TEXT PRIMARY KEY,
            status TEXT NOT NULL,
            source_url TEXT,
            attempts INTEGER NOT NULL DEFAULT 0,
            next_try_at INTEGER NOT NULL DEFAULT 0,
            fetched_at INTEGER,
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;
    // 수집 작업이 '지금 해도 되는 다음 건'을 고르는 질의(status + next_try_at)를 받친다.
    query("CREATE INDEX IF NOT EXISTS idx_book_covers_queue ON book_covers(status, next_try_at)")
        .execute(pool)
        .await
        .map_err(InternalServerError)?;

    // gotolib 찜한 책 소장 알림(Web Push). 구독은 기기(endpoint)마다 한 행이고, 상태는 (계정, 책, 도서관)마다
    // '지난번 확인에서 소장이었나'를 기억해 '미소장 → 소장'으로 바뀔 때만 알리게 한다.
    // 이름에 gotolib_를 붙인 건 이 DB를 쓰는 다른 앱의 web_push_* 표와 겹치지 않게 하려는 것이다
    // (옛 web_push_subscriptions는 위에서 지운다).
    query(
        r#"
        CREATE TABLE IF NOT EXISTS gotolib_push_subscriptions (
            sub_id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id TEXT NOT NULL,
            endpoint TEXT NOT NULL UNIQUE,
            p256dh TEXT NOT NULL,
            auth TEXT NOT NULL,
            user_agent TEXT,
            created_at INTEGER NOT NULL,
            last_success_at INTEGER,
            failure_count INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (user_id) REFERENCES users(user_id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;
    query("CREATE INDEX IF NOT EXISTS idx_gotolib_push_subs_user ON gotolib_push_subscriptions(user_id)")
        .execute(pool)
        .await
        .map_err(InternalServerError)?;
    query(
        r#"
        CREATE TABLE IF NOT EXISTS gotolib_watch_state (
            user_id TEXT NOT NULL,
            isbn13 TEXT NOT NULL,
            lib_code TEXT NOT NULL,
            has_book INTEGER NOT NULL,
            checked_at INTEGER NOT NULL,
            PRIMARY KEY (user_id, isbn13, lib_code)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;
    // 확인 라운드를 마지막으로 돈 시각(한 행). 서버가 자주 재시작돼도 주기를 지키려고 DB에 둔다.
    query(
        r#"
        CREATE TABLE IF NOT EXISTS gotolib_push_runs (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            last_run_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    // API 키(개인 액세스 토큰, src/auth/api_key.rs). 운영 DB에는 예전에 만든 같은 모양의 표가 이미 있어
    // IF NOT EXISTS라 건드리지 않는다. 키 원문이 아니라 해시(key_hash)만 저장한다.
    query(
        r#"
        CREATE TABLE IF NOT EXISTS api_keys (
            api_key_id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id TEXT NOT NULL,
            name TEXT NOT NULL,
            key_lookup TEXT NOT NULL,
            key_hash TEXT NOT NULL,
            user_role TEXT NOT NULL DEFAULT 'user',
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            last_used_at DATETIME,
            revoked_at DATETIME
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;
    query("CREATE UNIQUE INDEX IF NOT EXISTS idx_api_keys_lookup ON api_keys(key_lookup)")
        .execute(pool)
        .await
        .map_err(InternalServerError)?;
    query("CREATE INDEX IF NOT EXISTS idx_api_keys_user_id ON api_keys(user_id)")
        .execute(pool)
        .await
        .map_err(InternalServerError)?;

    // Craft 폴더 동기화(src/craft_sync.rs): Craft 문서 ↔ 포스트 연결. removed=1이면 Craft에서 사라져 draft로 돌린 글.
    query(
        r#"
        CREATE TABLE IF NOT EXISTS craft_sync_posts (
            craft_doc_id TEXT PRIMARY KEY,
            post_id TEXT NOT NULL,
            last_modified_at TEXT NOT NULL,
            removed INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    // 같은 Craft 이미지 블록을 다시 내려받지 않으려고 저장 경로를 기억한다.
    query(
        r#"
        CREATE TABLE IF NOT EXISTS craft_sync_images (
            craft_block_id TEXT PRIMARY KEY,
            image_path TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(InternalServerError)?;

    Ok(())
}

/// 오래 쓰지 않은 gotolib 전용 계정을 지운다(계정·패스키·동기화 문서). 가입에 이메일이 없어서
/// 연락할 방법이 없고 복구도 불가능하므로, 쓰지 않는 계정이 저장소를 계속 차지하지 않게 한다.
/// '마지막 활동' = 패스키 등록·사용, 동기화 문서 갱신 중 가장 최근. 지운 계정 수를 돌려준다.
pub async fn purge_inactive_gotolib_accounts(
    pool: &sqlx::SqlitePool,
    inactive_days: u32,
) -> Result<u64> {
    let cutoff = format!("-{inactive_days} days");
    let inactive = r#"
        SELECT u.user_id FROM users u
        WHERE u.user_role = 'gotolib'
          AND COALESCE((
                SELECT MAX(ts) FROM (
                    SELECT created_at AS ts FROM passkeys WHERE user_id = u.user_id
                    UNION ALL
                    SELECT last_used_at FROM passkeys WHERE user_id = u.user_id
                    UNION ALL
                    SELECT updated_at FROM gotolib_sync WHERE user_id = u.user_id
                )
              ), '1970-01-01') < datetime('now', ?1)
    "#;

    let mut tx = pool.begin().await.map_err(InternalServerError)?;
    for table in [
        "gotolib_sync",
        "passkeys",
        "gotolib_push_subscriptions",
        "gotolib_watch_state",
    ] {
        query(&format!("DELETE FROM {table} WHERE user_id IN ({inactive})"))
            .bind(&cutoff)
            .execute(&mut *tx)
            .await
            .map_err(InternalServerError)?;
    }
    let removed = query(&format!("DELETE FROM users WHERE user_id IN ({inactive})"))
        .bind(&cutoff)
        .execute(&mut *tx)
        .await
        .map_err(InternalServerError)?
        .rows_affected();
    tx.commit().await.map_err(InternalServerError)?;
    Ok(removed)
}
