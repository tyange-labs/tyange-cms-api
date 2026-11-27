//! MCP 커넥터용 OAuth 2.1 인가 서버. Claude 문서·MCP 인가 규격에 맞춘 최소 구성이다.
//!
//! - 보호 자원 메타데이터(RFC 9728): `/.well-known/oauth-protected-resource[/mcp]`
//! - 인가 서버 메타데이터(RFC 8414): `/.well-known/oauth-authorization-server` (+ `openid-configuration`)
//! - `/oauth/authorize`: 승인 화면. 기존 패스키 로그인(`/passkeys/login/*`)으로 받은 로그인 JWT를
//!   `/oauth/authorize/approve`에 넘기면, admin 계정일 때 승인 코드를 만들어 Claude로 돌려보낸다.
//! - `/oauth/token`: 코드 + PKCE(S256) → 토큰 쌍, 리프레시 토큰 → 새 쌍(회전).
//!
//! 클라이언트는 미리 정한 공개 클라이언트 하나(`MCP_OAUTH_CLIENT_ID`)만 받는다. 동적 등록(DCR)·CIMD는 없다 —
//! 쓰는 사람이 한 명이라 Claude 커넥터 설정에서 "Use your own OAuth client"로 이 id를 넣는다.
//! 리다이렉트는 Claude 앱 콜백과 루프백(Claude Code 등)만 받는다(`is_allowed_redirect`).

use std::sync::Arc;

use poem::http::{header, StatusCode};
use poem::web::{Data, Form, Html, Json, Query};
use poem::{get, handler, post, EndpointExt, IntoResponse, Response, Route};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tyange_cms_api::auth::jwt::Claims;

use super::{tokens, McpConfig, CLAUDE_REDIRECT_URIS};
use crate::models::AppState;

pub const SCOPE: &str = "cms";

/// `AppState`는 main.rs가 앱 전체에 싣는다. 설정만 엔드포인트마다 싣는다.
pub fn add_routes(route: Route, config: Arc<McpConfig>) -> Route {
    let c = || config.clone();
    route
        .at(
            "/.well-known/oauth-protected-resource",
            get(protected_resource_metadata).data(c()),
        )
        .at(
            "/.well-known/oauth-protected-resource/mcp",
            get(protected_resource_metadata).data(c()),
        )
        .at(
            "/.well-known/oauth-authorization-server",
            get(authorization_server_metadata).data(c()),
        )
        .at(
            "/.well-known/openid-configuration",
            get(authorization_server_metadata).data(c()),
        )
        .at("/oauth/authorize", get(authorize_page).data(c()))
        .at("/oauth/authorize/approve", post(approve).data(c()))
        .at("/oauth/token", post(token).data(c()))
}

#[handler]
fn protected_resource_metadata(config: Data<&Arc<McpConfig>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "resource": config.resource(),
        "authorization_servers": [config.public_url],
        "scopes_supported": [SCOPE],
        "bearer_methods_supported": ["header"],
        "resource_name": "tyange CMS",
    }))
}

#[handler]
fn authorization_server_metadata(config: Data<&Arc<McpConfig>>) -> Json<serde_json::Value> {
    let base = &config.public_url;
    Json(serde_json::json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/oauth/authorize"),
        "token_endpoint": format!("{base}/oauth/token"),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none"],
        "scopes_supported": [SCOPE],
        "authorization_response_iss_parameter_supported": true,
    }))
}

/// `/oauth/authorize`와 `/oauth/authorize/approve`가 같이 쓰는 인가 요청 값.
#[derive(Debug, Clone, Deserialize)]
pub struct AuthorizeParams {
    pub response_type: Option<String>,
    pub client_id: Option<String>,
    pub redirect_uri: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub state: Option<String>,
    pub resource: Option<String>,
    pub scope: Option<String>,
}

/// 검증을 통과한 인가 요청.
struct ValidRequest {
    client_id: String,
    redirect_uri: String,
    code_challenge: String,
    state: Option<String>,
    resource: String,
}

/// 클라이언트·리다이렉트 주소가 틀리면 Claude로 돌려보내지 않고 이 화면에서 멈춘다(열린 리다이렉트 방지).
/// 그 밖의 오류는 RFC 6749대로 리다이렉트 주소에 `error`를 붙여 돌려보낸다.
enum Invalid {
    Stop(&'static str),
    Redirect {
        redirect_uri: String,
        error: &'static str,
        state: Option<String>,
    },
}

fn validate(config: &McpConfig, params: &AuthorizeParams) -> Result<ValidRequest, Invalid> {
    let client_id = params.client_id.clone().unwrap_or_default();
    if client_id != config.client_id {
        return Err(Invalid::Stop("알 수 없는 클라이언트입니다."));
    }
    let redirect_uri = params.redirect_uri.clone().unwrap_or_default();
    if !is_allowed_redirect(&redirect_uri) {
        return Err(Invalid::Stop("허용되지 않은 리다이렉트 주소입니다."));
    }
    let redirect = |error| Invalid::Redirect {
        redirect_uri: redirect_uri.clone(),
        error,
        state: params.state.clone(),
    };
    if params.response_type.as_deref() != Some("code") {
        return Err(redirect("unsupported_response_type"));
    }
    let code_challenge = params.code_challenge.clone().unwrap_or_default();
    if code_challenge.is_empty() || params.code_challenge_method.as_deref() != Some("S256") {
        return Err(redirect("invalid_request"));
    }
    let resource = config.resource();
    if let Some(requested) = &params.resource {
        if requested.trim_end_matches('/') != resource {
            return Err(redirect("invalid_target"));
        }
    }
    if let Some(scope) = &params.scope {
        if scope.split_whitespace().any(|s| s != SCOPE) {
            return Err(redirect("invalid_scope"));
        }
    }
    Ok(ValidRequest {
        client_id,
        redirect_uri,
        code_challenge,
        state: params.state.clone(),
        resource,
    })
}

/// Claude 앱(웹·데스크톱·모바일)의 콜백, 또는 루프백 주소(`http://localhost|127.0.0.1|[::1]:<포트>/…`).
/// 루프백은 맥의 Claude Code 같은 네이티브 클라이언트용이다(RFC 8252 7.3 — 포트는 매번 달라서 보지 않는다).
/// 코드가 사용자 자기 컴퓨터로만 가고 PKCE가 필수라 안전하다.
pub fn is_allowed_redirect(redirect_uri: &str) -> bool {
    if CLAUDE_REDIRECT_URIS.contains(&redirect_uri) {
        return true;
    }
    let Ok(url) = url::Url::parse(redirect_uri) else {
        return false;
    };
    let loopback = match url.host() {
        Some(url::Host::Domain(domain)) => domain == "localhost",
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    url.scheme() == "http"
        && loopback
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
}

fn redirect_with(
    config: &McpConfig,
    redirect_uri: &str,
    pairs: &[(&str, &str)],
    state: Option<&str>,
) -> String {
    let mut url = url::Url::parse(redirect_uri).expect("허용된 리다이렉트 주소는 올바른 URL이다");
    {
        let mut query = url.query_pairs_mut();
        for (key, value) in pairs {
            query.append_pair(key, value);
        }
        if let Some(state) = state {
            query.append_pair("state", state);
        }
        query.append_pair("iss", &config.public_url);
    }
    url.to_string()
}

fn no_frame(resp: impl IntoResponse) -> Response {
    resp.with_header("X-Frame-Options", "DENY")
        .with_header("Content-Security-Policy", "frame-ancestors 'none'")
        .with_header(header::CACHE_CONTROL, "no-store")
        .into_response()
}

#[handler]
fn authorize_page(
    config: Data<&Arc<McpConfig>>,
    Query(params): Query<AuthorizeParams>,
) -> Response {
    match validate(&config, &params) {
        Ok(_) => no_frame(Html(AUTHORIZE_HTML)),
        Err(Invalid::Stop(message)) => no_frame(
            Html(format!(
                "<!doctype html><meta charset=utf-8><title>연결할 수 없음</title><p>{message}</p>"
            ))
            .with_status(StatusCode::BAD_REQUEST),
        ),
        Err(Invalid::Redirect {
            redirect_uri,
            error,
            state,
        }) => no_frame(
            Response::builder()
                .status(StatusCode::FOUND)
                .header(
                    header::LOCATION,
                    redirect_with(
                        &config,
                        &redirect_uri,
                        &[("error", error)],
                        state.as_deref(),
                    ),
                )
                .finish(),
        ),
    }
}

#[derive(Debug, Deserialize)]
pub struct ApproveRequest {
    /// `/passkeys/login/finish`가 준 로그인 JWT.
    pub access_token: String,
    #[serde(flatten)]
    pub params: AuthorizeParams,
}

/// 패스키 로그인으로 받은 JWT를 확인하고, admin이면 승인 코드를 만들어 돌아갈 주소를 준다.
#[handler]
async fn approve(
    config: Data<&Arc<McpConfig>>,
    state: Data<&Arc<AppState>>,
    Json(body): Json<ApproveRequest>,
) -> poem::Result<Response> {
    let request = match validate(&config, &body.params) {
        Ok(request) => request,
        Err(Invalid::Stop(message)) | Err(Invalid::Redirect { error: message, .. }) => {
            return Err(poem::Error::from_string(message, StatusCode::BAD_REQUEST));
        }
    };

    let user = std::env::var("JWT_ACCESS_SECRET")
        .ok()
        .and_then(|secret| {
            let valid = Claims::validate_token(&body.access_token, secret.as_bytes()).ok()?;
            let data = Claims::from_token(&body.access_token, secret.as_bytes()).ok()?;
            valid.then_some(data.claims)
        })
        .ok_or_else(|| {
            poem::Error::from_string("로그인이 필요합니다.", StatusCode::UNAUTHORIZED)
        })?;
    if user.role != "admin" {
        return Err(poem::Error::from_string(
            "관리자 계정만 이 커넥터를 연결할 수 있습니다.",
            StatusCode::FORBIDDEN,
        ));
    }

    let code = tokens::store_code(
        &state.db,
        &tokens::AuthCode {
            user_id: user.sub,
            client_id: request.client_id,
            redirect_uri: request.redirect_uri.clone(),
            code_challenge: request.code_challenge,
            resource: request.resource,
        },
    )
    .await
    .map_err(|e| poem::Error::from_string(e.to_string(), StatusCode::INTERNAL_SERVER_ERROR))?;

    let redirect = redirect_with(
        &config,
        &request.redirect_uri,
        &[("code", &code)],
        request.state.as_deref(),
    );
    Ok(no_frame(Json(serde_json::json!({ "redirect": redirect }))))
}

#[derive(Debug, Deserialize)]
pub struct TokenRequest {
    pub grant_type: String,
    pub code: Option<String>,
    pub redirect_uri: Option<String>,
    pub client_id: Option<String>,
    pub code_verifier: Option<String>,
    pub refresh_token: Option<String>,
    pub resource: Option<String>,
}

fn token_error(error: &str, description: &str) -> Response {
    Json(serde_json::json!({ "error": error, "error_description": description }))
        .with_status(StatusCode::BAD_REQUEST)
        .with_header(header::CACHE_CONTROL, "no-store")
        .into_response()
}

fn token_ok(pair: tokens::TokenPair) -> Response {
    Json(serde_json::json!({
        "access_token": pair.access_token,
        "token_type": "Bearer",
        "expires_in": pair.expires_in,
        "refresh_token": pair.refresh_token,
        "scope": SCOPE,
    }))
    .with_header(header::CACHE_CONTROL, "no-store")
    .into_response()
}

pub fn pkce_challenge(verifier: &str) -> String {
    base64::encode_config(Sha256::digest(verifier.as_bytes()), base64::URL_SAFE_NO_PAD)
}

#[handler]
async fn token(
    config: Data<&Arc<McpConfig>>,
    state: Data<&Arc<AppState>>,
    Form(req): Form<TokenRequest>,
) -> Response {
    let client_id = req.client_id.clone().unwrap_or_default();
    if client_id != config.client_id {
        return token_error("invalid_client", "알 수 없는 클라이언트입니다.");
    }
    if let Some(resource) = &req.resource {
        if resource.trim_end_matches('/') != config.resource() {
            return token_error("invalid_target", "이 서버의 MCP 주소가 아닙니다.");
        }
    }

    match req.grant_type.as_str() {
        "authorization_code" => {
            let (Some(code), Some(verifier)) = (&req.code, &req.code_verifier) else {
                return token_error("invalid_request", "code 와 code_verifier 가 필요합니다.");
            };
            let Some(stored) = tokens::take_code(&state.db, code).await else {
                return token_error("invalid_grant", "만료되었거나 이미 쓴 코드입니다.");
            };
            if stored.client_id != client_id
                || req.redirect_uri.as_deref() != Some(stored.redirect_uri.as_str())
                || pkce_challenge(verifier) != stored.code_challenge
            {
                return token_error("invalid_grant", "코드 검증에 실패했습니다.");
            }
            match tokens::issue_pair(&state.db, &stored.user_id, &client_id, &stored.resource).await
            {
                Ok(pair) => token_ok(pair),
                Err(_) => token_error("server_error", "토큰을 만들지 못했습니다."),
            }
        }
        "refresh_token" => {
            let Some(refresh) = &req.refresh_token else {
                return token_error("invalid_request", "refresh_token 이 필요합니다.");
            };
            match tokens::rotate_refresh(&state.db, refresh, &client_id).await {
                Some((pair, _)) => token_ok(pair),
                None => token_error(
                    "invalid_grant",
                    "만료되었거나 알 수 없는 리프레시 토큰입니다.",
                ),
            }
        }
        _ => token_error("unsupported_grant_type", "지원하지 않는 grant_type 입니다."),
    }
}

/// 승인 화면. 패스키로 로그인 → 로그인 JWT로 승인 → Claude로 돌아간다. 인가 요청 값은 이 화면의 주소(쿼리)에서 읽는다.
const AUTHORIZE_HTML: &str = r#"<!doctype html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>tyange CMS 연결</title>
<style>
  :root { color-scheme: light dark; }
  body { font-family: -apple-system, system-ui, sans-serif; max-width: 420px; margin: 0 auto; padding: 48px 16px; line-height: 1.5; }
  h1 { font-size: 1.3rem; }
  button { width: 100%; padding: 14px; font-size: 1rem; border-radius: 10px; border: 0; background: #2563eb; color: #fff; }
  button:disabled { opacity: .5; }
  #msg { margin-top: 16px; min-height: 1.5em; }
</style>
</head>
<body>
<h1>Claude를 tyange CMS에 연결</h1>
<p>Claude가 블로그 글을 조회·작성·수정·공개·삭제할 수 있게 됩니다. 관리자 패스키로 승인하세요.</p>
<button id="go">패스키로 승인</button>
<p id="msg"></p>
<script>
const b64u = {
  toBuf: (s) => Uint8Array.from(atob(s.replace(/-/g, '+').replace(/_/g, '/') + '==='.slice((s.length + 3) % 4)), c => c.charCodeAt(0)).buffer,
  fromBuf: (buf) => btoa(String.fromCharCode(...new Uint8Array(buf))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, ''),
};
const msg = (t) => { document.getElementById('msg').textContent = t; };
async function postJson(url, body) {
  const res = await fetch(url, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
  const text = await res.text();
  if (!res.ok) throw new Error(text || res.status);
  return text ? JSON.parse(text) : {};
}
document.getElementById('go').addEventListener('click', async () => {
  const btn = document.getElementById('go');
  btn.disabled = true; msg('패스키를 확인하는 중…');
  try {
    const start = await postJson('/passkeys/login/start', {});
    const pk = start.options.publicKey;
    pk.challenge = b64u.toBuf(pk.challenge);
    (pk.allowCredentials || []).forEach(c => { c.id = b64u.toBuf(c.id); });
    const cred = await navigator.credentials.get({ publicKey: pk });
    const r = cred.response;
    const credential = {
      id: cred.id, rawId: b64u.fromBuf(cred.rawId), type: cred.type, extensions: {},
      response: {
        authenticatorData: b64u.fromBuf(r.authenticatorData),
        clientDataJSON: b64u.fromBuf(r.clientDataJSON),
        signature: b64u.fromBuf(r.signature),
        userHandle: r.userHandle ? b64u.fromBuf(r.userHandle) : null,
      },
    };
    const login = await postJson('/passkeys/login/finish', { challenge_id: start.challenge_id, credential });
    const params = Object.fromEntries(new URLSearchParams(location.search));
    const done = await postJson('/oauth/authorize/approve', { access_token: login.access_token, ...params });
    msg('승인했습니다. Claude로 돌아갑니다…');
    location.href = done.redirect;
  } catch (e) {
    msg('승인하지 못했습니다: ' + e.message);
    btn.disabled = false;
  }
});
</script>
</body>
</html>
"#;
