//! MCP OAuth의 승인 코드와 토큰 저장. 원문은 발급할 때 한 번만 돌려주고 DB에는 SHA-256 해시만 둔다
//! (API 키와 같은 방식). 모두 무작위 값이라 해시를 그대로 기본 키로 찾는다.

use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use sqlx::{query, query_as, SqlitePool};
use uuid::Uuid;

pub const CODE_TTL_SECS: i64 = 5 * 60;
pub const ACCESS_TTL_SECS: i64 = 60 * 60;
pub const REFRESH_TTL_SECS: i64 = 60 * 24 * 60 * 60;

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

fn random_secret(prefix: &str) -> String {
    format!(
        "{prefix}{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

pub fn hash(secret: &str) -> String {
    Sha256::digest(secret.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AuthCode {
    pub user_id: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub resource: String,
}

pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
}

pub async fn store_code(db: &SqlitePool, code: &AuthCode) -> Result<String, sqlx::Error> {
    let secret = random_secret("mcpc_");
    query(
        r#"
        INSERT INTO mcp_oauth_codes
            (code_hash, user_id, client_id, redirect_uri, code_challenge, resource, expires_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(hash(&secret))
    .bind(&code.user_id)
    .bind(&code.client_id)
    .bind(&code.redirect_uri)
    .bind(&code.code_challenge)
    .bind(&code.resource)
    .bind(now() + CODE_TTL_SECS)
    .execute(db)
    .await?;
    Ok(secret)
}

/// 코드를 꺼내면서 지운다(일회용). 만료된 코드는 `None`.
pub async fn take_code(db: &SqlitePool, code: &str) -> Option<AuthCode> {
    let found = query_as::<_, AuthCode>(
        r#"
        DELETE FROM mcp_oauth_codes WHERE code_hash = ? AND expires_at > ?
        RETURNING user_id, client_id, redirect_uri, code_challenge, resource
        "#,
    )
    .bind(hash(code))
    .bind(now())
    .fetch_optional(db)
    .await
    .ok()
    .flatten();
    let _ = query("DELETE FROM mcp_oauth_codes WHERE expires_at <= ?")
        .bind(now())
        .execute(db)
        .await;
    found
}

pub async fn issue_pair(
    db: &SqlitePool,
    user_id: &str,
    client_id: &str,
    resource: &str,
) -> Result<TokenPair, sqlx::Error> {
    let access_token = random_secret("mcpa_");
    let refresh_token = random_secret("mcpr_");
    let now = now();
    let mut tx = db.begin().await?;
    for (secret, kind, ttl) in [
        (&access_token, "access", ACCESS_TTL_SECS),
        (&refresh_token, "refresh", REFRESH_TTL_SECS),
    ] {
        query(
            r#"
            INSERT INTO mcp_oauth_tokens (token_hash, kind, user_id, client_id, resource, expires_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(hash(secret))
        .bind(kind)
        .bind(user_id)
        .bind(client_id)
        .bind(resource)
        .bind(now + ttl)
        .execute(&mut *tx)
        .await?;
    }
    query("DELETE FROM mcp_oauth_tokens WHERE expires_at <= ?")
        .bind(now)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(TokenPair {
        access_token,
        refresh_token,
        expires_in: ACCESS_TTL_SECS,
    })
}

/// 이 MCP 주소(`resource`)용으로 발급된, 만료되지 않은 액세스 토큰이면 그 사용자 id.
pub async fn verify_access(db: &SqlitePool, token: &str, resource: &str) -> Option<String> {
    query_as::<_, (String,)>(
        r#"
        SELECT user_id FROM mcp_oauth_tokens
        WHERE token_hash = ? AND kind = 'access' AND resource = ? AND expires_at > ?
        "#,
    )
    .bind(hash(token))
    .bind(resource)
    .bind(now())
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
    .map(|(user_id,)| user_id)
}

/// 리프레시 토큰을 쓰면 그 토큰은 지우고 새 쌍을 준다(회전). 같은 클라이언트가 발급받은 토큰이어야 한다.
pub async fn rotate_refresh(
    db: &SqlitePool,
    refresh_token: &str,
    client_id: &str,
) -> Option<(TokenPair, String)> {
    let (user_id, resource) = query_as::<_, (String, String)>(
        r#"
        DELETE FROM mcp_oauth_tokens
        WHERE token_hash = ? AND kind = 'refresh' AND client_id = ? AND expires_at > ?
        RETURNING user_id, resource
        "#,
    )
    .bind(hash(refresh_token))
    .bind(client_id)
    .bind(now())
    .fetch_optional(db)
    .await
    .ok()
    .flatten()?;
    let pair = issue_pair(db, &user_id, client_id, &resource).await.ok()?;
    Some((pair, resource))
}
