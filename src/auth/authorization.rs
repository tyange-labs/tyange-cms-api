use std::env;

use poem::http::StatusCode;
use poem::{Error, Request};
use poem_openapi::auth::ApiKey;
use poem_openapi::SecurityScheme;
use sqlx::{query_scalar, Error as SqlxError, Pool, Sqlite};

use crate::auth::api_key::{self, ApiKeyVerifier};
use crate::auth::jwt::Claims;

#[derive(Clone, Debug)]
pub struct AuthenticatedUser {
    pub user_id: String,
    pub role: String,
}

pub fn current_user(req: &Request) -> Result<&AuthenticatedUser, Error> {
    req.extensions().get::<AuthenticatedUser>().ok_or_else(|| {
        Error::from_string("인증된 사용자 정보가 없습니다.", StatusCode::UNAUTHORIZED)
    })
}

/// poem-openapi(`#[OpenApi]`) 라우트용 인증 가드.
/// `middlewares::auth_middleware::Auth`와 검증 로직은 동일하다 — 아직 plain poem으로 남아있는
/// 라우트(공개 하객 명단 접수, 웹푸시 등)는 그 미들웨어를 계속 쓰고, `#[OpenApi]`로 옮긴
/// 라우트는 이 SecurityScheme을 파라미터로 받는다(`guard: AuthGuard`).
/// Admin 전용 라우트는 여기서 인증만 확인하고, 핸들러 본문에서 기존 `ensure_admin`을
/// 그대로 호출해 401(미인증)과 403(권한 없음)을 구분한다.
#[derive(SecurityScheme)]
#[oai(
    ty = "api_key",
    key_name = "Authorization",
    key_in = "header",
    checker = "auth_guard_checker"
)]
pub struct AuthGuard(pub AuthenticatedUser);

/// `Authorization` 헤더 값에서 토큰만 꺼낸다. 표준인 `Bearer <토큰>`과 예전 방식(토큰만)을 둘 다 받는다.
fn bearer_or_raw(value: &str) -> &str {
    let value = value.trim();
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .unwrap_or(value)
        .trim()
}

fn jwt_user(token: &str) -> Option<AuthenticatedUser> {
    let secret = env::var("JWT_ACCESS_SECRET").ok()?;
    let token_data = Claims::from_token(token, secret.as_bytes()).ok()?;
    let is_valid = Claims::validate_token(token, secret.as_bytes()).ok()?;

    if !is_valid {
        return None;
    }

    Some(AuthenticatedUser {
        user_id: token_data.claims.sub,
        role: token_data.claims.role,
    })
}

/// 로그인 JWT 또는 API 키(`tyk_…`, `auth::api_key`)로 인증한다. 키 확인에는 요청 데이터에 실린
/// `ApiKeyVerifier`가 필요하다(main.rs가 `.data(...)`로 넣는다). 없으면 키는 인증되지 않는다.
async fn auth_guard_checker(req: &Request, api_key: ApiKey) -> Option<AuthenticatedUser> {
    let token = bearer_or_raw(api_key.key.as_str());
    if api_key::is_api_key(token) {
        return req.data::<ApiKeyVerifier>()?.verify(token).await;
    }
    jwt_user(token)
}

/// **로그인 JWT만** 받는 가드. API 키 발급·폐기처럼 "키로는 할 수 없어야 하는" 일에 쓴다 —
/// 키가 새도 그 키로 새 키를 만들어 버티지 못하게 한다.
#[derive(SecurityScheme)]
#[oai(
    ty = "api_key",
    key_name = "Authorization",
    key_in = "header",
    checker = "session_guard_checker"
)]
pub struct SessionGuard(pub AuthenticatedUser);

async fn session_guard_checker(_req: &Request, api_key: ApiKey) -> Option<AuthenticatedUser> {
    let token = bearer_or_raw(api_key.key.as_str());
    if api_key::is_api_key(token) {
        return None;
    }
    jwt_user(token)
}

pub async fn ensure_post_owner(
    user: &AuthenticatedUser,
    post_id: &str,
    db: &Pool<Sqlite>,
) -> Result<(), Error> {
    let writer_id: String = query_scalar(
        r#"
        SELECT writer_id FROM posts WHERE post_id = ?
        "#,
    )
    .bind(post_id)
    .fetch_one(db)
    .await
    .map_err(|err| {
        eprintln!("Error fetching post owner: {}", err);
        match err {
            SqlxError::RowNotFound => {
                Error::from_string("게시글을 찾을 수 없습니다.", StatusCode::NOT_FOUND)
            }
            _ => Error::from_string(
                "게시글 작성자 조회에 실패했습니다.",
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        }
    })?;

    if user.role == "admin" || user.user_id == writer_id {
        Ok(())
    } else {
        Err(Error::from_string(
            "본인이 업로드한 게시글만 수정 또는 삭제할 수 있습니다.",
            StatusCode::FORBIDDEN,
        ))
    }
}

/// 글과 이미지를 쓸 수 있는 역할. gotolib 전용 계정(`gotolib`)은 패스키·동기화만 쓰는 계정이라
/// CMS 콘텐츠를 만들 수 없다. 이 서버는 블로그·CMS와 계정 테이블을 공유하므로, 새 역할이 생길 때
/// 기본값이 '쓸 수 있음'이 되지 않게 허용 목록(admin, user)으로 막는다.
pub fn ensure_content_writer(user: &AuthenticatedUser) -> Result<(), Error> {
    if matches!(user.role.as_str(), "admin" | "user") {
        Ok(())
    } else {
        Err(Error::from_string(
            "이 계정으로는 콘텐츠를 작성할 수 없습니다.",
            StatusCode::FORBIDDEN,
        ))
    }
}

pub fn ensure_admin(user: &AuthenticatedUser) -> Result<(), Error> {
    if user.role == "admin" {
        Ok(())
    } else {
        Err(Error::from_string(
            "관리자만 접근할 수 있습니다.",
            StatusCode::FORBIDDEN,
        ))
    }
}
