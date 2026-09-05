use std::env;

use poem::http::StatusCode;
use poem::{Error, Request};
use poem_openapi::auth::ApiKey;
use poem_openapi::SecurityScheme;
use sqlx::{query_scalar, Error as SqlxError, Pool, Sqlite};

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

async fn auth_guard_checker(_req: &Request, api_key: ApiKey) -> Option<AuthenticatedUser> {
    let secret = env::var("JWT_ACCESS_SECRET").ok()?;
    let token = api_key.key.as_str();
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
