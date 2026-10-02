//! gotolib 기기 간 동기화 문서. 서버는 내용을 해석하지 않고(JSON 객체) 버전으로만 충돌을 막는다.
//!
//! 클라이언트는 `GET`으로 받아 로컬과 합친 뒤 `PUT { base_version, data }`로 올린다.
//! 그 사이 다른 기기가 먼저 올렸다면 409와 함께 최신 문서를 돌려주므로 다시 합쳐 재시도한다.

use std::sync::Arc;

use poem::{
    handler,
    http::StatusCode,
    web::{Data, Json},
    Body, Error, Request, Response,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{query, Row};
use tyange_cms_api::auth::authorization::current_user;

use crate::models::AppState;

/// 동기화 문서 크기 상한(직렬화한 JSON). 클라이언트가 책을 1,000권까지만 올리므로(권당 약
/// 200~300바이트) 정상 문서는 이보다 훨씬 작다 — 이 상한은 이중 방어다.
pub const MAX_SYNC_BYTES: usize = 512 * 1024;

#[derive(Serialize)]
pub struct SyncDocument {
    version: i64,
    data: Option<Value>,
    updated_at: Option<String>,
}

#[derive(Deserialize)]
pub struct SyncPutRequest {
    /// 클라이언트가 마지막으로 받은 버전. 처음이면 0.
    base_version: i64,
    data: Value,
}

fn db_error(e: sqlx::Error) -> Error {
    eprintln!("Database error: {:?}", e);
    Error::from_string("Database error", StatusCode::INTERNAL_SERVER_ERROR)
}

async fn load(state: &AppState, user_id: &str) -> Result<SyncDocument, Error> {
    let row = query(
        r#"
        SELECT version, data, strftime('%Y-%m-%dT%H:%M:%SZ', updated_at) AS updated_at
        FROM gotolib_sync WHERE user_id = ?
        "#,
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(db_error)?;

    Ok(match row {
        Some(row) => {
            let data: String = row.try_get("data").map_err(db_error)?;
            SyncDocument {
                version: row.try_get("version").map_err(db_error)?,
                data: serde_json::from_str(&data).ok(),
                updated_at: row.try_get("updated_at").ok(),
            }
        }
        None => SyncDocument {
            version: 0,
            data: None,
            updated_at: None,
        },
    })
}

/// 문서가 바뀌지 않았으면 본문 없이 304로 답하게 하는 ETag. version이 곧 문서의 정체성이다.
fn etag(version: i64) -> String {
    format!("\"v{version}\"")
}

#[handler]
pub async fn get_sync(req: &Request, data: Data<&Arc<AppState>>) -> poem::Result<Response> {
    let user = current_user(req)?;
    let document = load(&data, &user.user_id).await?;
    let tag = etag(document.version);

    // 토큰마다 다른 응답이라 캐시 키에 Authorization을 넣고, 매번 재검증(no-cache)하게 한다.
    let builder = Response::builder()
        .header("ETag", &tag)
        .header("Cache-Control", "private, no-cache")
        .header("Vary", "Authorization");

    let not_modified = req
        .headers()
        .get("If-None-Match")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|candidate| candidate.trim() == tag));
    if not_modified {
        return Ok(builder.status(StatusCode::NOT_MODIFIED).finish());
    }

    let body = serde_json::to_string(&document).map_err(|_| {
        Error::from_string("직렬화에 실패했습니다.", StatusCode::INTERNAL_SERVER_ERROR)
    })?;
    Ok(builder
        .status(StatusCode::OK)
        .content_type("application/json")
        .body(Body::from_string(body)))
}

#[handler]
pub async fn put_sync(
    req: &Request,
    Json(payload): Json<SyncPutRequest>,
    data: Data<&Arc<AppState>>,
) -> poem::Result<Response> {
    let user = current_user(req)?;

    if !payload.data.is_object() {
        return Err(Error::from_string(
            "data는 JSON 객체여야 합니다.",
            StatusCode::BAD_REQUEST,
        ));
    }
    if payload.base_version < 0 {
        return Err(Error::from_string(
            "base_version이 올바르지 않습니다.",
            StatusCode::BAD_REQUEST,
        ));
    }
    let serialized = serde_json::to_string(&payload.data).map_err(|_| {
        Error::from_string("직렬화에 실패했습니다.", StatusCode::BAD_REQUEST)
    })?;
    if serialized.len() > MAX_SYNC_BYTES {
        return Err(Error::from_string(
            "동기화 문서가 너무 큽니다.",
            StatusCode::PAYLOAD_TOO_LARGE,
        ));
    }

    // 버전이 맞을 때만 쓴다. 조건을 SQL 한 문장에 넣어서 동시에 두 기기가 올려도 한쪽만 이긴다.
    let written = if payload.base_version == 0 {
        query(
            r#"
            INSERT INTO gotolib_sync (user_id, version, data) VALUES (?, 1, ?)
            ON CONFLICT(user_id) DO NOTHING
            "#,
        )
        .bind(&user.user_id)
        .bind(&serialized)
        .execute(&data.db)
        .await
        .map_err(db_error)?
        .rows_affected()
    } else {
        query(
            r#"
            UPDATE gotolib_sync
            SET version = version + 1, data = ?, updated_at = CURRENT_TIMESTAMP
            WHERE user_id = ? AND version = ?
            "#,
        )
        .bind(&serialized)
        .bind(&user.user_id)
        .bind(payload.base_version)
        .execute(&data.db)
        .await
        .map_err(db_error)?
        .rows_affected()
    };

    let current = load(&data, &user.user_id).await?;
    let status = if written == 0 {
        StatusCode::CONFLICT
    } else {
        StatusCode::OK
    };
    let body = serde_json::to_string(&current).map_err(|_| {
        Error::from_string("직렬화에 실패했습니다.", StatusCode::INTERNAL_SERVER_ERROR)
    })?;
    Ok(Response::builder()
        .status(status)
        .content_type("application/json")
        .body(Body::from_string(body)))
}
