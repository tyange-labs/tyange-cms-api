use std::{env, sync::Arc};

use bcrypt::{hash, verify, DEFAULT_COST};
use poem::{
    handler,
    http::StatusCode,
    web::{Data, Json},
    Body, Response,
};

use sqlx::Row;
use tyange_cms_api::auth::jwt::Claims;

use crate::{
    models::{LoginRequest, LoginResponse},
    AppState,
};

pub fn issue_login_response(user_id: &str, user_role: &str) -> Result<LoginResponse, poem::Error> {
    let access_token_secret = match env::var("JWT_ACCESS_SECRET") {
        Ok(value) => value,
        Err(e) => {
            eprintln!("Server configuration error: {:?}", e);
            return Err(poem::Error::from_string(
                "Server configuration error.",
                StatusCode::INTERNAL_SERVER_ERROR,
            ));
        }
    };

    let access_token_secret_bytes = access_token_secret.as_bytes();
    let access_token = Claims::create_access_token(user_id, user_role, access_token_secret_bytes)
        .map_err(|e| {
        eprintln!("Server configuration error: {:?}", e);
        poem::Error::from_string(
            "Can not create access token.",
            StatusCode::INTERNAL_SERVER_ERROR,
        )
    })?;

    Ok(LoginResponse {
        access_token,
        user_role: user_role.to_string(),
    })
}

#[handler]
pub async fn login(
    Json(payload): Json<LoginRequest>,
    data: Data<&Arc<AppState>>,
) -> poem::Result<Response> {
    let user = sqlx::query(
        r#"
        SELECT user_id, password, user_role FROM users WHERE user_id = ?
        "#,
    )
    .bind(&payload.user_id)
    .fetch_optional(&data.db)
    .await
    .map_err(|e| {
        eprintln!("Database error: {:?}", e);
        poem::Error::from_string("Database error", StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    let row = match user {
        Some(row) => row,
        None => {
            return Err(poem::Error::from_string(
                "Invalid credentials",
                StatusCode::UNAUTHORIZED,
            ))
        }
    };

    let user_id: String = row.try_get("user_id").unwrap_or_default();
    let stored_hash: Option<String> = row.try_get("password").unwrap_or(None);
    let user_role: String = row.try_get("user_role").unwrap_or_default();

    let password_matches = matches_password(&payload.password, stored_hash.as_deref());

    if password_matches {
        upgrade_legacy_password_if_needed(
            &data.db,
            &user_id,
            &payload.password,
            stored_hash.as_deref(),
        )
        .await?;

        let login_response = issue_login_response(&user_id, &user_role)?;

        let json_body = serde_json::to_string(&login_response).map_err(|_| {
            poem::Error::from_string(
                "JSON serialization error",
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;

        println!("로그인 성공: {}", user_id);

        Ok(Response::builder()
            .status(StatusCode::OK)
            .content_type("application/json")
            .body(Body::from(json_body)))
    } else {
        Ok(Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .content_type("application/json")
            .body(Body::from_string(String::from(
                "로그인 실패: 잘못된 비밀번호",
            ))))
    }
}

fn matches_password(candidate_password: &str, stored_password: Option<&str>) -> bool {
    let Some(stored_password) = stored_password else {
        return false;
    };

    if is_bcrypt_hash(stored_password) {
        verify(candidate_password, stored_password).unwrap_or(false)
    } else {
        candidate_password == stored_password
    }
}

async fn upgrade_legacy_password_if_needed(
    db: &sqlx::SqlitePool,
    user_id: &str,
    candidate_password: &str,
    stored_password: Option<&str>,
) -> Result<(), poem::Error> {
    let Some(stored_password) = stored_password else {
        return Ok(());
    };

    if is_bcrypt_hash(stored_password) || candidate_password != stored_password {
        return Ok(());
    }

    let upgraded_hash = hash(candidate_password, DEFAULT_COST).map_err(|e| {
        poem::Error::from_string(
            format!("Password hashing failed: {}", e),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
    })?;

    sqlx::query(
        r#"
        UPDATE users
        SET password = ?, auth_provider = COALESCE(NULLIF(auth_provider, ''), 'local')
        WHERE user_id = ?
        "#,
    )
    .bind(upgraded_hash)
    .bind(user_id)
    .execute(db)
    .await
    .map_err(|e| {
        eprintln!("Database error while upgrading legacy password: {:?}", e);
        poem::Error::from_string("Database error", StatusCode::INTERNAL_SERVER_ERROR)
    })?;

    Ok(())
}

fn is_bcrypt_hash(value: &str) -> bool {
    value.starts_with("$2a$") || value.starts_with("$2b$") || value.starts_with("$2y$")
}
