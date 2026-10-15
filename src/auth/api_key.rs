//! 개인 액세스 토큰(API 키). CLI·자동화처럼 비밀번호를 들고 있고 싶지 않은 클라이언트용이다.
//!
//! 키는 `tyk_<조회용 12자>_<비밀 52자>`다. 서버에는 **SHA-256 해시만** 저장하고(키 원문은 만들 때 한 번만
//! 보여 준다), 조회용 12자로 행을 찾은 뒤 해시를 상수 시간으로 비교한다. 키는 무작위 208비트라 bcrypt처럼
//! 느린 해시가 필요 없다(느린 해시는 사람이 고른 약한 비밀번호용이다). 폐기는 행을 지우지 않고
//! `revoked_at`을 채운다.

use sha2::{Digest, Sha256};
use sqlx::{query, query_as, SqlitePool};
use uuid::Uuid;

use crate::auth::authorization::AuthenticatedUser;

pub const KEY_PREFIX: &str = "tyk_";
const LOOKUP_LEN: usize = 12;

pub struct NewKey {
    /// 사용자에게 한 번만 보여 주는 원문.
    pub plaintext: String,
    /// 행을 찾는 데 쓰는 앞부분(`tyk_` 제외). 목록에 보여 줘도 안전하다.
    pub lookup: String,
    pub hash: String,
}

pub fn hash_key(plaintext: &str) -> String {
    Sha256::digest(plaintext.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn generate() -> NewKey {
    let first = Uuid::new_v4().simple().to_string();
    let second = Uuid::new_v4().simple().to_string();
    let (lookup, rest) = first.split_at(LOOKUP_LEN);
    let plaintext = format!("{KEY_PREFIX}{lookup}_{rest}{second}");
    NewKey {
        hash: hash_key(&plaintext),
        lookup: lookup.to_string(),
        plaintext,
    }
}

/// `tyk_<lookup>_<secret>`에서 lookup만 꺼낸다. 모양이 다르면 `None`.
pub fn lookup_of(token: &str) -> Option<&str> {
    let rest = token.strip_prefix(KEY_PREFIX)?;
    let lookup = rest.get(..LOOKUP_LEN)?;
    let secret = rest.get(LOOKUP_LEN..)?.strip_prefix('_')?;
    (lookup.bytes().all(|b| b.is_ascii_hexdigit()) && !secret.is_empty()).then_some(lookup)
}

pub fn is_api_key(token: &str) -> bool {
    token.starts_with(KEY_PREFIX)
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// 요청 데이터로 실려 가서 `AuthGuard`가 키를 확인하는 데 쓴다.
#[derive(Clone)]
pub struct ApiKeyVerifier {
    pool: SqlitePool,
}

impl ApiKeyVerifier {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// 유효한 키면 그 키의 주인과 역할을 돌려준다. 모르는 키·폐기된 키·틀린 비밀은 모두 `None`.
    pub async fn verify(&self, token: &str) -> Option<AuthenticatedUser> {
        let lookup = lookup_of(token)?;
        let row: Option<(i64, String, String, String, i64)> = query_as(
            "SELECT api_key_id, user_id, user_role, key_hash, (revoked_at IS NOT NULL) AS revoked
             FROM api_keys WHERE key_lookup = ?",
        )
        .bind(lookup)
        .fetch_optional(&self.pool)
        .await
        .ok()?;
        let (id, user_id, role, hash, revoked) = row?;
        if revoked != 0 || !constant_time_eq(&hash, &hash_key(token)) {
            return None;
        }

        // 마지막 사용 시각은 1분에 한 번만 갱신한다(요청마다 쓰기를 만들지 않으려고).
        let _ = query(
            "UPDATE api_keys SET last_used_at = CURRENT_TIMESTAMP
             WHERE api_key_id = ? AND (last_used_at IS NULL OR last_used_at < datetime('now', '-1 minute'))",
        )
        .bind(id)
        .execute(&self.pool)
        .await;

        Some(AuthenticatedUser { user_id, role })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_generated_key_has_the_documented_shape() {
        let key = generate();
        assert!(key.plaintext.starts_with("tyk_"));
        assert_eq!(key.lookup.len(), LOOKUP_LEN);
        assert_eq!(key.plaintext.len(), 4 + 12 + 1 + 52);
        assert_eq!(lookup_of(&key.plaintext), Some(key.lookup.as_str()));
        assert_eq!(key.hash, hash_key(&key.plaintext));
        assert!(!key.hash.contains(&key.plaintext));
    }

    #[test]
    fn keys_are_unique_and_hashes_differ() {
        let (a, b) = (generate(), generate());
        assert_ne!(a.plaintext, b.plaintext);
        assert_ne!(a.hash, b.hash);
    }

    #[test]
    fn malformed_tokens_have_no_lookup() {
        for token in ["", "tyk_", "tyk_abc", "Bearer x", "tyk_zzzzzzzzzzzz_secret", "tyk_0123456789ab-secret", "tyk_0123456789ab_"] {
            assert_eq!(lookup_of(token), None, "{token}");
        }
        assert_eq!(lookup_of("tyk_0123456789ab_x"), Some("0123456789ab"));
    }

    #[test]
    fn constant_time_eq_compares_whole_strings() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
    }

    #[test]
    fn non_ascii_input_does_not_panic() {
        assert_eq!(lookup_of("tyk_가나다라마바사아자차카타파하_x"), None);
    }
}
