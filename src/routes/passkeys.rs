//! 패스키(WebAuthn) 등록·로그인·관리.
//!
//! 로그인은 사용자명 없이 하는 discoverable 방식이다: `login/start`가 빈 allowCredentials로
//! 챌린지를 만들고, 브라우저가 고른 패스키의 user handle로 계정을 찾아 서명을 검증한 뒤
//! 기존 비밀번호 로그인과 같은 JWT(`issue_login_response`)를 발급한다.

use std::sync::Arc;

use poem::{
    handler,
    http::StatusCode,
    web::{Data, Json, Path},
    Error, Request,
};
use serde::{Deserialize, Serialize};
use sqlx::{query, Row};
use tyange_cms_api::auth::authorization::current_user;
use uuid::Uuid;
use webauthn_rs::prelude::{
    CreationChallengeResponse, CredentialID, DiscoverableKey, Passkey, PublicKeyCredential,
    RegisterPublicKeyCredential, RequestChallengeResponse, Webauthn,
};
use webauthn_rs_proto::ResidentKeyRequirement;

use crate::{
    models::{AppState, LoginResponse},
    passkey::Pending,
    routes::login::issue_login_response,
};

const MAX_LABEL_CHARS: usize = 40;
const DEFAULT_LABEL: &str = "패스키";
/// gotolib 전용 계정의 역할. 패스키·동기화만 쓸 수 있고 CMS 콘텐츠는 쓸 수 없다
/// (`ensure_content_writer`).
pub const GOTOLIB_ROLE: &str = "gotolib";
/// gotolib 계정의 패스키에 붙는 이름. iOS·macOS·Chrome의 패스키 선택 창은 WebAuthn user.name을 그대로
/// 보여주는데, 예전에는 계정 id(gl-8d9fd7414a28…)가 나와서 알아볼 수 없었다. 계정은 user.id(사용자
/// 핸들, UUID)로 구분하므로 이름은 표시용일 뿐이다 — 같은 이름이 여러 개여도 로그인에는 영향이 없다.
pub const GOTOLIB_PASSKEY_NAME: &str = "gotolib";

#[derive(Serialize)]
pub struct ChallengeResponse<T: Serialize> {
    challenge_id: String,
    options: T,
}

#[derive(Deserialize)]
pub struct RegisterFinishRequest {
    challenge_id: String,
    label: Option<String>,
    credential: RegisterPublicKeyCredential,
}

#[derive(Deserialize)]
pub struct SignupFinishRequest {
    challenge_id: String,
    label: Option<String>,
    credential: RegisterPublicKeyCredential,
}

#[derive(Deserialize)]
pub struct LoginFinishRequest {
    challenge_id: String,
    credential: PublicKeyCredential,
}

#[derive(Serialize)]
pub struct PasskeyItem {
    id: String,
    label: String,
    created_at: Option<String>,
    last_used_at: Option<String>,
}

fn webauthn(state: &AppState) -> Result<&Webauthn, Error> {
    state.passkeys.webauthn().ok_or_else(|| {
        Error::from_string(
            "패스키가 설정되지 않았습니다.",
            StatusCode::SERVICE_UNAVAILABLE,
        )
    })
}

fn db_error(e: sqlx::Error) -> Error {
    eprintln!("Database error: {:?}", e);
    Error::from_string("Database error", StatusCode::INTERNAL_SERVER_ERROR)
}

fn bad_request(message: &'static str) -> impl Fn(webauthn_rs::prelude::WebauthnError) -> Error {
    move |e| {
        eprintln!("WebAuthn error: {:?}", e);
        Error::from_string(message, StatusCode::BAD_REQUEST)
    }
}

fn credential_id_string(id: &CredentialID) -> String {
    match serde_json::to_value(id) {
        Ok(serde_json::Value::String(s)) => s,
        _ => String::new(),
    }
}

fn too_many_pending() -> Error {
    Error::from_string(
        "진행 중인 요청이 너무 많습니다. 잠시 후 다시 시도하세요.",
        StatusCode::TOO_MANY_REQUESTS,
    )
}

/// 사용자별 WebAuthn user handle. 이미 패스키가 있으면 그 값을 이어 쓴다.
async fn user_handle_for(state: &AppState, user_id: &str) -> Result<Uuid, Error> {
    let existing: Option<String> =
        sqlx::query_scalar("SELECT user_handle FROM passkeys WHERE user_id = ? LIMIT 1")
            .bind(user_id)
            .fetch_optional(&state.db)
            .await
            .map_err(db_error)?;
    Ok(existing
        .and_then(|value| Uuid::parse_str(&value).ok())
        .unwrap_or_else(Uuid::new_v4))
}

/// 사용자명 없는 로그인을 하려면 디스커버러블이어야 한다. 동기화 패스키(iCloud·Google 등)는
/// 알아서 그렇게 만들어지지만, 보안 키는 요청해야 만든다 — 가능하면 만들도록 요청한다.
fn prefer_discoverable(options: &mut CreationChallengeResponse) {
    if let Some(selection) = options.public_key.authenticator_selection.as_mut() {
        selection.resident_key = Some(ResidentKeyRequirement::Preferred);
    }
}

fn clean_label(label: Option<&str>) -> String {
    label
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .unwrap_or(DEFAULT_LABEL)
        .chars()
        .take(MAX_LABEL_CHARS)
        .collect()
}

async fn gotolib_account_count<'e, E>(executor: E) -> Result<i64, Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE user_role = ?")
        .bind(GOTOLIB_ROLE)
        .fetch_one(executor)
        .await
        .map_err(db_error)
}

fn accounts_full() -> Error {
    Error::from_string(
        "지금은 새 계정을 받지 않습니다.",
        StatusCode::SERVICE_UNAVAILABLE,
    )
}

#[handler]
pub async fn register_start(
    req: &Request,
    data: Data<&Arc<AppState>>,
) -> poem::Result<Json<ChallengeResponse<CreationChallengeResponse>>> {
    let user = current_user(req)?;
    let webauthn = webauthn(&data)?;

    let rows = query("SELECT passkey FROM passkeys WHERE user_id = ?")
        .bind(&user.user_id)
        .fetch_all(&data.db)
        .await
        .map_err(db_error)?;
    let exclude: Vec<CredentialID> = rows
        .iter()
        .filter_map(|row| {
            let json: String = row.try_get("passkey").ok()?;
            serde_json::from_str::<Passkey>(&json)
                .ok()
                .map(|passkey| passkey.cred_id().clone())
        })
        .collect();

    let (user_name, display_name) = if user.role == GOTOLIB_ROLE {
        (GOTOLIB_PASSKEY_NAME.to_string(), GOTOLIB_PASSKEY_NAME.to_string())
    } else {
        let display_name: Option<String> =
            sqlx::query_scalar("SELECT display_name FROM users WHERE user_id = ?")
                .bind(&user.user_id)
                .fetch_optional(&data.db)
                .await
                .map_err(db_error)?
                .flatten();
        let display_name = display_name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| user.user_id.clone());
        (user.user_id.clone(), display_name)
    };

    let user_handle = user_handle_for(&data, &user.user_id).await?;
    let (mut options, state) = webauthn
        .start_passkey_registration(user_handle, &user_name, &display_name, Some(exclude))
        .map_err(bad_request("패스키 등록을 시작하지 못했습니다."))?;

    prefer_discoverable(&mut options);

    let challenge_id = data
        .passkeys
        .store(Pending::Register {
            user_id: user.user_id.clone(),
            user_handle,
            state,
        })
        .ok_or_else(too_many_pending)?;

    Ok(Json(ChallengeResponse {
        challenge_id,
        options,
    }))
}

#[handler]
pub async fn register_finish(
    req: &Request,
    Json(payload): Json<RegisterFinishRequest>,
    data: Data<&Arc<AppState>>,
) -> poem::Result<Json<PasskeyItem>> {
    let user = current_user(req)?;
    let webauthn = webauthn(&data)?;

    let Some(Pending::Register {
        user_id,
        user_handle,
        state,
    }) = data.passkeys.take(&payload.challenge_id)
    else {
        return Err(Error::from_string(
            "만료되었거나 알 수 없는 요청입니다.",
            StatusCode::BAD_REQUEST,
        ));
    };
    if user_id != user.user_id {
        return Err(Error::from_string(
            "요청한 사용자와 다릅니다.",
            StatusCode::BAD_REQUEST,
        ));
    }

    let passkey = webauthn
        .finish_passkey_registration(&payload.credential, &state)
        .map_err(bad_request("패스키 등록을 확인하지 못했습니다."))?;

    let label = clean_label(payload.label.as_deref());
    let id = credential_id_string(passkey.cred_id());
    let passkey_json = serde_json::to_string(&passkey).map_err(|e| {
        Error::from_string(
            format!("serialize passkey: {e}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
    })?;

    query(
        r#"
        INSERT INTO passkeys (credential_id, user_id, user_handle, label, passkey)
        VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(&user.user_id)
    .bind(user_handle.to_string())
    .bind(&label)
    .bind(&passkey_json)
    .execute(&data.db)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            Error::from_string("이미 등록된 패스키입니다.", StatusCode::CONFLICT)
        }
        other => db_error(other),
    })?;

    Ok(Json(PasskeyItem {
        id,
        label,
        created_at: None,
        last_used_at: None,
    }))
}

/// 가입 시작: 이메일·비밀번호 없이 패스키 하나로 계정을 만든다. 계정 id는 서버가 정한다.
#[handler]
pub async fn signup_start(
    data: Data<&Arc<AppState>>,
) -> poem::Result<Json<ChallengeResponse<CreationChallengeResponse>>> {
    let webauthn = webauthn(&data)?;
    if gotolib_account_count(&data.db).await? >= data.passkeys.max_accounts() {
        return Err(accounts_full());
    }

    let user_handle = Uuid::new_v4();
    let user_id = format!("gl-{}", user_handle.simple());
    let (mut options, state) = webauthn
        .start_passkey_registration(user_handle, GOTOLIB_PASSKEY_NAME, GOTOLIB_PASSKEY_NAME, None)
        .map_err(bad_request("가입을 시작하지 못했습니다."))?;
    prefer_discoverable(&mut options);

    let challenge_id = data
        .passkeys
        .store(Pending::Signup {
            user_id,
            user_handle,
            state,
        })
        .ok_or_else(too_many_pending)?;
    Ok(Json(ChallengeResponse {
        challenge_id,
        options,
    }))
}

/// 가입 완료: 패스키를 검증한 뒤에야 계정과 패스키를 한 트랜잭션으로 만들고 JWT를 돌려준다.
#[handler]
pub async fn signup_finish(
    Json(payload): Json<SignupFinishRequest>,
    data: Data<&Arc<AppState>>,
) -> poem::Result<Json<LoginResponse>> {
    let webauthn = webauthn(&data)?;

    let Some(Pending::Signup {
        user_id,
        user_handle,
        state,
    }) = data.passkeys.take(&payload.challenge_id)
    else {
        return Err(Error::from_string(
            "만료되었거나 알 수 없는 요청입니다.",
            StatusCode::BAD_REQUEST,
        ));
    };

    let passkey = webauthn
        .finish_passkey_registration(&payload.credential, &state)
        .map_err(bad_request("가입을 확인하지 못했습니다."))?;
    let passkey_json = serde_json::to_string(&passkey).map_err(|e| {
        Error::from_string(
            format!("serialize passkey: {e}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
    })?;

    let mut tx = data.db.begin().await.map_err(db_error)?;
    // 상한 확인과 INSERT를 같은 트랜잭션에서 한다(동시에 여러 가입이 몰려도 상한을 크게 넘지 않게).
    if gotolib_account_count(&mut *tx).await? >= data.passkeys.max_accounts() {
        return Err(accounts_full());
    }
    query(
        r#"
        INSERT INTO users (user_id, password, user_role, auth_provider)
        VALUES (?, NULL, ?, 'passkey')
        "#,
    )
    .bind(&user_id)
    .bind(GOTOLIB_ROLE)
    .execute(&mut *tx)
    .await
    .map_err(db_error)?;
    query(
        r#"
        INSERT INTO passkeys (credential_id, user_id, user_handle, label, passkey)
        VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(credential_id_string(passkey.cred_id()))
    .bind(&user_id)
    .bind(user_handle.to_string())
    .bind(clean_label(payload.label.as_deref()))
    .bind(&passkey_json)
    .execute(&mut *tx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            Error::from_string("이미 등록된 패스키입니다.", StatusCode::CONFLICT)
        }
        other => db_error(other),
    })?;
    tx.commit().await.map_err(db_error)?;

    println!("gotolib 계정 생성: {}", user_id);
    Ok(Json(issue_login_response(&user_id, GOTOLIB_ROLE)?))
}

#[handler]
pub async fn login_start(
    data: Data<&Arc<AppState>>,
) -> poem::Result<Json<ChallengeResponse<RequestChallengeResponse>>> {
    let webauthn = webauthn(&data)?;
    let (options, state) = webauthn
        .start_discoverable_authentication()
        .map_err(bad_request("패스키 로그인을 시작하지 못했습니다."))?;
    let challenge_id = data
        .passkeys
        .store(Pending::Login(state))
        .ok_or_else(too_many_pending)?;
    Ok(Json(ChallengeResponse {
        challenge_id,
        options,
    }))
}

fn unauthorized() -> Error {
    Error::from_string("Invalid credentials", StatusCode::UNAUTHORIZED)
}

#[handler]
pub async fn login_finish(
    Json(payload): Json<LoginFinishRequest>,
    data: Data<&Arc<AppState>>,
) -> poem::Result<Json<LoginResponse>> {
    let webauthn = webauthn(&data)?;

    let Some(Pending::Login(state)) = data.passkeys.take(&payload.challenge_id) else {
        return Err(Error::from_string(
            "만료되었거나 알 수 없는 요청입니다.",
            StatusCode::BAD_REQUEST,
        ));
    };

    let (user_handle, _) = webauthn
        .identify_discoverable_authentication(&payload.credential)
        .map_err(|_| unauthorized())?;

    let rows = query("SELECT credential_id, user_id, passkey FROM passkeys WHERE user_handle = ?")
        .bind(user_handle.to_string())
        .fetch_all(&data.db)
        .await
        .map_err(db_error)?;
    let mut owner: Option<String> = None;
    let mut stored: Vec<(String, Passkey)> = Vec::new();
    for row in &rows {
        let credential_id: String = row.try_get("credential_id").map_err(db_error)?;
        let user_id: String = row.try_get("user_id").map_err(db_error)?;
        let json: String = row.try_get("passkey").map_err(db_error)?;
        let Ok(passkey) = serde_json::from_str::<Passkey>(&json) else {
            continue;
        };
        owner = Some(user_id);
        stored.push((credential_id, passkey));
    }
    let Some(user_id) = owner else {
        return Err(unauthorized());
    };

    let keys: Vec<DiscoverableKey> = stored.iter().map(|(_, p)| DiscoverableKey::from(p)).collect();
    let result = webauthn
        .finish_discoverable_authentication(&payload.credential, state, &keys)
        .map_err(|e| {
            eprintln!("WebAuthn login failed: {:?}", e);
            unauthorized()
        })?;

    // 서명 카운터가 바뀌었으면 저장하고, 쓴 자격 증명의 사용 시각을 남긴다.
    for (credential_id, mut passkey) in stored {
        if passkey.cred_id() != result.cred_id() {
            continue;
        }
        let updated = passkey.update_credential(&result).unwrap_or(false);
        let passkey_json = updated.then(|| serde_json::to_string(&passkey).ok()).flatten();
        query(
            r#"
            UPDATE passkeys
            SET last_used_at = CURRENT_TIMESTAMP, passkey = COALESCE(?, passkey)
            WHERE credential_id = ?
            "#,
        )
        .bind(passkey_json)
        .bind(&credential_id)
        .execute(&data.db)
        .await
        .map_err(db_error)?;
    }

    let role: String = sqlx::query_scalar("SELECT user_role FROM users WHERE user_id = ?")
        .bind(&user_id)
        .fetch_optional(&data.db)
        .await
        .map_err(db_error)?
        .ok_or_else(unauthorized)?;

    println!("패스키 로그인 성공: {}", user_id);
    Ok(Json(issue_login_response(&user_id, &role)?))
}

#[handler]
pub async fn list_passkeys(
    req: &Request,
    data: Data<&Arc<AppState>>,
) -> poem::Result<Json<Vec<PasskeyItem>>> {
    let user = current_user(req)?;
    let rows = query(
        r#"
        SELECT credential_id, label,
               strftime('%Y-%m-%dT%H:%M:%SZ', created_at) AS created_at,
               strftime('%Y-%m-%dT%H:%M:%SZ', last_used_at) AS last_used_at
        FROM passkeys WHERE user_id = ? ORDER BY created_at, credential_id
        "#,
    )
    .bind(&user.user_id)
    .fetch_all(&data.db)
    .await
    .map_err(db_error)?;

    let items = rows
        .iter()
        .map(|row| PasskeyItem {
            id: row.try_get("credential_id").unwrap_or_default(),
            label: row.try_get("label").unwrap_or_default(),
            created_at: row.try_get("created_at").ok(),
            last_used_at: row.try_get("last_used_at").ok(),
        })
        .collect();
    Ok(Json(items))
}

#[handler]
pub async fn delete_passkey(
    req: &Request,
    Path(id): Path<String>,
    data: Data<&Arc<AppState>>,
) -> poem::Result<Json<serde_json::Value>> {
    let user = current_user(req)?;
    let result = query("DELETE FROM passkeys WHERE credential_id = ? AND user_id = ?")
        .bind(&id)
        .bind(&user.user_id)
        .execute(&data.db)
        .await
        .map_err(db_error)?;
    if result.rows_affected() == 0 {
        return Err(Error::from_string(
            "패스키를 찾을 수 없습니다.",
            StatusCode::NOT_FOUND,
        ));
    }
    Ok(Json(serde_json::json!({ "status": true })))
}
