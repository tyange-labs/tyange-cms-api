//! Claude 커넥터용 원격 MCP 서버. `MCP_PUBLIC_URL`(예: `https://mcp.tyange.com`)과 `MCP_OAUTH_CLIENT_ID`가
//! 둘 다 있을 때만 켜진다. MCP 엔드포인트는 `<MCP_PUBLIC_URL>/mcp`, 인증은 이 서버가 직접 하는 OAuth
//! (`oauth` 모듈)로 받은 액세스 토큰이다. 토큰 주인이 admin일 때만 도구를 쓸 수 있다.

pub mod oauth;
pub mod tokens;
pub mod tools;

use std::sync::Arc;

use poem::endpoint::TowerCompatExt;
use poem::http::{header, StatusCode};
use poem::{Endpoint, EndpointExt, IntoResponse, Request, Response, Route};
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use sqlx::{query_scalar, SqlitePool};
use tyange_cms_api::auth::authorization::AuthenticatedUser;

use crate::models::AppState;

/// Claude가 OAuth 승인 뒤 돌아오는 주소. 지금은 claude.ai이고, 문서에 claude.com으로 바뀔 수 있다고 적혀 있어 둘 다 받는다.
pub const CLAUDE_REDIRECT_URIS: [&str; 2] = [
    "https://claude.ai/api/mcp/auth_callback",
    "https://claude.com/api/mcp/auth_callback",
];

#[derive(Clone, Debug)]
pub struct McpConfig {
    /// 끝에 `/`가 없는 공개 주소. OAuth issuer이기도 하다.
    pub public_url: String,
    /// Claude 커넥터 설정의 "Use your own OAuth client"에 넣는 공개 클라이언트 id(비밀값 없음, PKCE).
    pub client_id: String,
}

impl McpConfig {
    pub fn from_env() -> Option<Self> {
        let public_url = std::env::var("MCP_PUBLIC_URL").ok()?;
        let client_id = std::env::var("MCP_OAUTH_CLIENT_ID").ok()?;
        Self::new(&public_url, &client_id)
    }

    pub fn new(public_url: &str, client_id: &str) -> Option<Self> {
        let public_url = public_url.trim().trim_end_matches('/').to_string();
        let client_id = client_id.trim().to_string();
        if public_url.is_empty() || client_id.is_empty() {
            return None;
        }
        Some(Self {
            public_url,
            client_id,
        })
    }

    /// MCP 엔드포인트의 정식 주소. 보호 자원 메타데이터의 `resource`이자 토큰의 대상(audience)이다.
    pub fn resource(&self) -> String {
        format!("{}/mcp", self.public_url)
    }

    pub fn resource_metadata_url(&self) -> String {
        format!(
            "{}/.well-known/oauth-protected-resource/mcp",
            self.public_url
        )
    }

    /// `Host` 헤더 검사용(`rmcp`의 DNS rebinding 방지). `https://mcp.tyange.com` → `mcp.tyange.com`.
    pub fn host(&self) -> String {
        self.public_url
            .split("://")
            .nth(1)
            .unwrap_or(&self.public_url)
            .split('/')
            .next()
            .unwrap_or_default()
            .to_string()
    }
}

/// `/mcp`와 OAuth 경로들을 붙인다. 기존 `nest("/", api_service)`와 같이 쓰려고 개별 경로로 붙인다.
pub fn add_routes(route: Route, state: Arc<AppState>, config: Arc<McpConfig>) -> Route {
    let tools_state = state.clone();
    let service = StreamableHttpService::new(
        move || Ok(tools::CmsTools::new(tools_state.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .with_sse_keep_alive(None)
            .with_allowed_hosts([
                config.host(),
                "localhost".to_string(),
                "127.0.0.1".to_string(),
            ]),
    );

    let db = state.db.clone();
    let auth_config = config.clone();
    let mcp = service.compat().around(move |endpoint, req| {
        let db = db.clone();
        let config = auth_config.clone();
        async move { authorize_and_call(&db, &config, endpoint, req).await }
    });

    oauth::add_routes(route, config).at("/mcp", mcp)
}

async fn authorize_and_call<E: Endpoint>(
    db: &SqlitePool,
    config: &McpConfig,
    endpoint: Arc<E>,
    mut req: Request,
) -> poem::Result<Response> {
    let Some(token) = bearer_token(&req) else {
        return Ok(unauthorized(config, None));
    };
    let Some(user_id) = tokens::verify_access(db, &token, &config.resource()).await else {
        return Ok(unauthorized(config, Some("invalid_token")));
    };
    let role: Option<String> = query_scalar("SELECT user_role FROM users WHERE user_id = ?")
        .bind(&user_id)
        .fetch_optional(db)
        .await
        .ok()
        .flatten();
    let Some(role) = role else {
        return Ok(unauthorized(config, Some("invalid_token")));
    };
    if role != "admin" {
        return Ok(StatusCode::FORBIDDEN
            .with_body("관리자 계정만 이 커넥터를 쓸 수 있습니다.")
            .into_response());
    }

    req.extensions_mut()
        .insert(AuthenticatedUser { user_id, role });
    endpoint.call(req).await.map(IntoResponse::into_response)
}

fn bearer_token(req: &Request) -> Option<String> {
    let value = req
        .headers()
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .trim();
    let token = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))?
        .trim();
    (!token.is_empty()).then(|| token.to_string())
}

/// 401 + `WWW-Authenticate`. Claude는 이 헤더의 `resource_metadata`를 보고 로그인 흐름을 시작한다.
fn unauthorized(config: &McpConfig, error: Option<&str>) -> Response {
    let mut challenge = format!(
        "Bearer resource_metadata=\"{}\"",
        config.resource_metadata_url()
    );
    if let Some(error) = error {
        challenge.push_str(&format!(", error=\"{error}\""));
    }
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(header::WWW_AUTHENTICATE, challenge)
        .finish()
}

#[cfg(test)]
mod tests;
