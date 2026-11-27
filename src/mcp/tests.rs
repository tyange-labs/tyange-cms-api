use std::{env, sync::Arc};

use poem::{http::StatusCode, test::TestClient, EndpointExt, Response, Route};
use serde_json::{json, Value};
use sqlx::{query, query_scalar, sqlite::SqlitePoolOptions, SqlitePool};
use tyange_cms_api::auth::jwt::Claims;

use super::{add_routes_with, images::ImageRehoster, oauth::pkce_challenge, tokens, McpConfig};
use crate::{db::init_db, models::AppState};

const PUBLIC_URL: &str = "https://mcp.example.com";
const CLIENT_ID: &str = "claude-test";
const REDIRECT: &str = "https://claude.ai/api/mcp/auth_callback";
const VERIFIER: &str = "verifier-0123456789-0123456789-0123456789-0123456789";

struct Harness {
    pool: SqlitePool,
    cli: TestClient<poem::endpoint::BoxEndpoint<'static, Response>>,
}

async fn harness() -> Harness {
    harness_with(ImageRehoster::new(
        env::temp_dir(),
        "https://cms.example.com",
    ))
    .await
}

async fn harness_with(images: ImageRehoster) -> Harness {
    env::set_var("JWT_ACCESS_SECRET", "test-access-secret");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    init_db(&pool).await.unwrap();
    for (user, role) in [("admin", "admin"), ("writer", "user")] {
        query("INSERT INTO users (user_id, user_role) VALUES (?, ?)")
            .bind(user)
            .bind(role)
            .execute(&pool)
            .await
            .unwrap();
    }
    let passkeys =
        crate::passkey::PasskeyService::build("example.com", PUBLIC_URL, "test").unwrap();
    let state = Arc::new(AppState::new_with_passkeys(pool.clone(), passkeys));
    let config = Arc::new(McpConfig::new(PUBLIC_URL, CLIENT_ID).unwrap());
    let app = add_routes_with(Route::new(), state.clone(), config, Arc::new(images))
        .data(state)
        .map_to_response()
        .boxed();
    Harness {
        pool,
        cli: TestClient::new(app),
    }
}

fn jwt(user: &str, role: &str) -> String {
    Claims::create_access_token(user, role, b"test-access-secret").unwrap()
}

fn authorize_params() -> Vec<(&'static str, String)> {
    vec![
        ("response_type", "code".into()),
        ("client_id", CLIENT_ID.into()),
        ("redirect_uri", REDIRECT.into()),
        ("code_challenge", pkce_challenge(VERIFIER)),
        ("code_challenge_method", "S256".into()),
        ("state", "xyz".into()),
        ("resource", format!("{PUBLIC_URL}/mcp")),
        ("scope", "cms".into()),
    ]
}

fn authorize_url(params: impl IntoIterator<Item = (&'static str, String)>) -> String {
    let mut q = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in params {
        q.append_pair(k, &v);
    }
    format!("/oauth/authorize?{}", q.finish())
}

fn query_of(url: &str) -> std::collections::HashMap<String, String> {
    url::Url::parse(url)
        .unwrap()
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

impl Harness {
    async fn approve(&self, login_jwt: &str) -> (StatusCode, Value) {
        let mut body = json!({ "access_token": login_jwt });
        for (key, value) in authorize_params() {
            body[key] = json!(value);
        }
        let resp = self
            .cli
            .post("/oauth/authorize/approve")
            .body_json(&body)
            .send()
            .await;
        let status = resp.0.status();
        let text = resp.0.into_body().into_string().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }

    async fn token(&self, form: &[(&str, &str)]) -> (StatusCode, Value) {
        let resp = self.cli.post("/oauth/token").form(&form).send().await;
        let status = resp.0.status();
        let text = resp.0.into_body().into_string().await.unwrap();
        (status, serde_json::from_str(&text).unwrap())
    }

    /// 승인부터 토큰 발급까지 한 번에. (access, refresh)
    async fn login(&self) -> (String, String) {
        let (status, body) = self.approve(&jwt("admin", "admin")).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let code = query_of(body["redirect"].as_str().unwrap())["code"].clone();
        let (status, body) = self
            .token(&[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("redirect_uri", REDIRECT),
                ("client_id", CLIENT_ID),
                ("code_verifier", VERIFIER),
                ("resource", &format!("{PUBLIC_URL}/mcp")),
            ])
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        (
            body["access_token"].as_str().unwrap().to_string(),
            body["refresh_token"].as_str().unwrap().to_string(),
        )
    }

    async fn rpc(
        &self,
        token: Option<&str>,
        method: &str,
        params: Value,
    ) -> (StatusCode, Value, String) {
        let mut req = self
            .cli
            .post("/mcp")
            .header("Host", "mcp.example.com")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", "2025-11-25")
            .content_type("application/json")
            .body_json(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }));
        if let Some(token) = token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        let resp = req.send().await;
        let status = resp.0.status();
        let www = resp
            .0
            .headers()
            .get("WWW-Authenticate")
            .map(|v| v.to_str().unwrap().to_string())
            .unwrap_or_default();
        let text = resp.0.into_body().into_string().await.unwrap();
        let json_text = text
            .lines()
            .find_map(|l| l.strip_prefix("data: "))
            .unwrap_or(&text)
            .to_string();
        (
            status,
            serde_json::from_str(&json_text).unwrap_or(Value::String(text)),
            www,
        )
    }

    /// 도구를 불러 (is_error, 텍스트) 를 돌려준다.
    async fn call(&self, token: &str, name: &str, args: Value) -> (bool, String) {
        let (status, body, _) = self
            .rpc(
                Some(token),
                "tools/call",
                json!({ "name": name, "arguments": args }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let result = &body["result"];
        (
            result["isError"].as_bool().unwrap_or(false),
            result["content"][0]["text"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        )
    }
}

#[tokio::test]
async fn mcp_without_token_asks_for_oauth() {
    let h = harness().await;
    let (status, _, www) = h.rpc(None, "tools/list", json!({})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(www.contains(&format!(
        "resource_metadata=\"{PUBLIC_URL}/.well-known/oauth-protected-resource/mcp\""
    )));

    let (status, _, www) = h.rpc(Some("mcpa_nope"), "tools/list", json!({})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(www.contains("invalid_token"));
}

#[tokio::test]
async fn metadata_points_claude_to_this_server() {
    let h = harness().await;
    let prm: Value = h
        .cli
        .get("/.well-known/oauth-protected-resource/mcp")
        .send()
        .await
        .json()
        .await
        .value()
        .deserialize();
    assert_eq!(prm["resource"], json!(format!("{PUBLIC_URL}/mcp")));
    assert_eq!(prm["authorization_servers"], json!([PUBLIC_URL]));

    let asm: Value = h
        .cli
        .get("/.well-known/oauth-authorization-server")
        .send()
        .await
        .json()
        .await
        .value()
        .deserialize();
    assert_eq!(asm["issuer"], json!(PUBLIC_URL));
    assert_eq!(asm["code_challenge_methods_supported"], json!(["S256"]));
    assert_eq!(
        asm["token_endpoint_auth_methods_supported"],
        json!(["none"])
    );
    assert_eq!(
        asm["token_endpoint"],
        json!(format!("{PUBLIC_URL}/oauth/token"))
    );
}

#[tokio::test]
async fn authorize_page_rejects_unknown_client_or_redirect_without_redirecting() {
    let h = harness().await;
    let ok = h.cli.get(authorize_url(authorize_params())).send().await;
    ok.assert_status_is_ok();

    for (key, bad) in [
        ("client_id", "someone-else"),
        ("redirect_uri", "https://evil.example/cb"),
    ] {
        let params = authorize_params()
            .into_iter()
            .map(|(k, v)| (k, if k == key { bad.to_string() } else { v }));
        let resp = h.cli.get(authorize_url(params)).send().await;
        resp.assert_status(StatusCode::BAD_REQUEST);
        assert!(resp.0.headers().get("Location").is_none(), "{key}");
    }

    let params = authorize_params().into_iter().map(|(k, v)| {
        (
            k,
            if k == "code_challenge_method" {
                "plain".to_string()
            } else {
                v
            },
        )
    });
    let resp = h.cli.get(authorize_url(params)).send().await;
    resp.assert_status(StatusCode::FOUND);
    let location = resp.0.headers().get("Location").unwrap().to_str().unwrap();
    let q = query_of(location);
    assert_eq!(q["error"], "invalid_request");
    assert_eq!(q["state"], "xyz");
}

#[tokio::test]
async fn only_an_admin_login_can_approve() {
    let h = harness().await;
    let (status, _) = h.approve(&jwt("writer", "user")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = h.approve("not-a-jwt").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = h.approve(&jwt("admin", "admin")).await;
    assert_eq!(status, StatusCode::OK);
    let q = query_of(body["redirect"].as_str().unwrap());
    assert!(body["redirect"].as_str().unwrap().starts_with(REDIRECT));
    assert_eq!(q["state"], "xyz");
    assert_eq!(q["iss"], PUBLIC_URL);
    assert!(q["code"].starts_with("mcpc_"));
}

#[tokio::test]
async fn code_needs_the_right_verifier_and_works_only_once() {
    let h = harness().await;
    let (_, body) = h.approve(&jwt("admin", "admin")).await;
    let code = query_of(body["redirect"].as_str().unwrap())["code"].clone();

    let form = |verifier: &'static str| {
        vec![
            ("grant_type", "authorization_code"),
            ("redirect_uri", REDIRECT),
            ("client_id", CLIENT_ID),
            ("code_verifier", verifier),
        ]
    };
    let mut wrong = form("wrong-verifier");
    wrong.push(("code", &code));
    let (status, body) = h.token(&wrong).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "invalid_grant");

    let (_, body) = h.approve(&jwt("admin", "admin")).await;
    let code = query_of(body["redirect"].as_str().unwrap())["code"].clone();
    let mut right = form(VERIFIER);
    right.push(("code", &code));
    let (status, body) = h.token(&right).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["token_type"], "Bearer");
    assert_eq!(body["expires_in"], tokens::ACCESS_TTL_SECS);

    let (status, body) = h.token(&right).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "invalid_grant");
}

#[tokio::test]
async fn refresh_token_rotates() {
    let h = harness().await;
    let (_, refresh) = h.login().await;
    let pairs = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh.as_str()),
        ("client_id", CLIENT_ID),
    ];
    let (status, body) = h.token(&pairs).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let new_access = body["access_token"].as_str().unwrap().to_string();

    let (status, body) = h.token(&pairs).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "쓴 리프레시 토큰은 다시 못 쓴다"
    );
    assert_eq!(body["error"], "invalid_grant");

    let (status, _, _) = h.rpc(Some(&new_access), "tools/list", json!({})).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn token_for_another_resource_or_non_admin_is_rejected() {
    let h = harness().await;
    let other = tokens::issue_pair(&h.pool, "admin", CLIENT_ID, "https://other.example/mcp")
        .await
        .unwrap();
    let (status, _, _) = h
        .rpc(Some(&other.access_token), "tools/list", json!({}))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let writer = tokens::issue_pair(&h.pool, "writer", CLIENT_ID, &format!("{PUBLIC_URL}/mcp"))
        .await
        .unwrap();
    let (status, _, _) = h
        .rpc(Some(&writer.access_token), "tools/list", json!({}))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn tools_list_has_the_cms_tools_and_marks_delete_destructive() {
    let h = harness().await;
    let (access, _) = h.login().await;
    let (status, body, _) = h.rpc(Some(&access), "tools/list", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let tools = body["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for name in [
        "list_posts",
        "get_post",
        "list_tags",
        "create_post",
        "update_post",
        "set_post_status",
        "delete_post",
    ] {
        assert!(names.contains(&name), "{name} 없음: {names:?}");
    }
    let delete = tools.iter().find(|t| t["name"] == "delete_post").unwrap();
    assert_eq!(delete["annotations"]["destructiveHint"], json!(true));
    let list = tools.iter().find(|t| t["name"] == "list_posts").unwrap();
    assert_eq!(list["annotations"]["readOnlyHint"], json!(true));
}

#[tokio::test]
async fn create_update_publish_and_delete_through_tools() {
    let h = harness().await;
    let (access, _) = h.login().await;

    let (is_error, text) = h
        .call(
            &access,
            "create_post",
            json!({
                "title": "아이폰에서 쓴 글",
                "content": "본문입니다.",
                "tags": [{ "tag": "lovelog", "category": "general" }],
            }),
        )
        .await;
    assert!(!is_error, "{text}");
    let created: Value = serde_json::from_str(&text).unwrap();
    let post_id = created["post_id"].as_str().unwrap().to_string();
    assert_eq!(created["status"], "draft", "기본은 초안");

    let (status, writer, published_at): (String, String, String) =
        sqlx::query_as("SELECT status, writer_id, published_at FROM posts WHERE post_id = ?")
            .bind(&post_id)
            .fetch_one(&h.pool)
            .await
            .unwrap();
    assert_eq!(status, "draft");
    assert_eq!(writer, "admin");
    assert_eq!(published_at.len(), 10);

    let (is_error, text) = h
        .call(
            &access,
            "update_post",
            json!({ "post_id": post_id, "content": "고친 본문" }),
        )
        .await;
    assert!(!is_error, "{text}");
    let (title, content): (String, String) =
        sqlx::query_as("SELECT title, content FROM posts WHERE post_id = ?")
            .bind(&post_id)
            .fetch_one(&h.pool)
            .await
            .unwrap();
    assert_eq!(title, "아이폰에서 쓴 글", "보내지 않은 필드는 그대로");
    assert_eq!(content, "고친 본문");
    let tag_count: i64 = query_scalar("SELECT COUNT(*) FROM post_tags WHERE post_id = ?")
        .bind(&post_id)
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(tag_count, 1, "tags 를 안 보내면 태그도 그대로");

    let (is_error, text) = h
        .call(
            &access,
            "set_post_status",
            json!({ "post_id": post_id, "status": "published" }),
        )
        .await;
    assert!(!is_error, "{text}");

    let (is_error, text) = h
        .call(
            &access,
            "list_posts",
            json!({ "status": "published", "query": "아이폰" }),
        )
        .await;
    assert!(!is_error, "{text}");
    let listed: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(listed["posts"].as_array().unwrap().len(), 1);

    let (is_error, text) = h
        .call(
            &access,
            "delete_post",
            json!({ "post_id": post_id, "confirm_title": "다른 제목" }),
        )
        .await;
    assert!(is_error, "확인 문구가 틀리면 안 지운다: {text}");
    assert!(text.contains("아이폰에서 쓴 글"));
    let still: i64 = query_scalar("SELECT COUNT(*) FROM posts WHERE post_id = ?")
        .bind(&post_id)
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(still, 1);

    let (is_error, text) = h
        .call(
            &access,
            "delete_post",
            json!({ "post_id": post_id, "confirm_title": "아이폰에서 쓴 글" }),
        )
        .await;
    assert!(!is_error, "{text}");
    let gone: i64 = query_scalar("SELECT COUNT(*) FROM posts WHERE post_id = ?")
        .bind(&post_id)
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(gone, 0);
}

#[tokio::test]
async fn bad_tool_input_comes_back_as_a_tool_error() {
    let h = harness().await;
    let (access, _) = h.login().await;
    let (is_error, text) = h
        .call(
            &access,
            "create_post",
            json!({ "title": "t", "content": "c", "status": "public" }),
        )
        .await;
    assert!(is_error);
    assert!(text.contains("draft 또는 published"));

    let (is_error, text) = h
        .call(
            &access,
            "create_post",
            json!({ "title": "t", "content": "c", "published_at": "2026/11/27" }),
        )
        .await;
    assert!(is_error);
    assert!(text.contains("YYYY-MM-DD"));
}

#[test]
fn delete_confirmation_uses_title_or_the_start_of_the_body() {
    assert_eq!(super::tools::confirm_text("  제목 ", "본문"), "제목");
    assert_eq!(
        super::tools::confirm_text(
            "",
            "  오늘은\n\n 맑았다. 산책을 했다. 길에서 고양이를 만났다. 귀여웠다."
        ),
        "오늘은 맑았다. 산책을 했다. 길에서 고양이를 만났다."
    );
}

// ── 이미지 옮겨 저장 ─────────────────────────────────────────────────────────

const PNG: &[u8] = &[
    0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 1,
];

#[poem::handler]
fn fake_png(hits: poem::web::Data<&Arc<std::sync::atomic::AtomicUsize>>) -> Vec<u8> {
    hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    PNG.to_vec()
}

#[poem::handler]
fn fake_text() -> &'static str {
    "그림이 아님"
}

/// (포트, cat.png 요청 횟수)
async fn image_server() -> (u16, Arc<std::sync::atomic::AtomicUsize>) {
    use poem::{get, listener::TcpAcceptor, Server};
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let app = Route::new()
        .at("/cat.png", get(fake_png))
        .at("/text", get(fake_text))
        .data(hits.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(Server::new_with_acceptor(TcpAcceptor::from_tokio(listener).unwrap()).run(app));
    (port, hits)
}

#[tokio::test]
async fn create_post_moves_external_images_to_this_server() {
    let (port, hits) = image_server().await;
    let dir = env::temp_dir().join(format!("mcp-images-{}", uuid::Uuid::new_v4()));
    let h = harness_with(
        ImageRehoster::new(dir.clone(), "https://cms.example.com/").allowing_insecure_loopback(),
    )
    .await;
    let (access, _) = h.login().await;

    let cat = format!("http://127.0.0.1:{port}/cat.png");
    let text = format!("http://127.0.0.1:{port}/text");
    let ours = "https://cms.example.com/images/old.png";
    let content =
        format!("![a]({cat})\n\n![b]({cat} \"같은 그림\")\n\n![c]({text})\n\n![d]({ours})");
    let (is_error, out) = h
        .call(
            &access,
            "create_post",
            json!({ "title": "그림 글", "content": content }),
        )
        .await;
    assert!(!is_error, "{out}");
    let out: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(out["images"]["rehosted"], 1, "{out}");
    assert_eq!(
        out["images"]["warnings"].as_array().unwrap().len(),
        1,
        "{out}"
    );
    assert_eq!(
        hits.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "같은 주소는 한 번만 받는다"
    );

    let saved: String = query_scalar("SELECT content FROM posts WHERE post_id = ?")
        .bind(out["post_id"].as_str().unwrap())
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert!(!saved.contains(&cat), "{saved}");
    assert!(
        saved.contains(&text),
        "실패한 그림은 원래 주소를 둔다: {saved}"
    );
    assert!(saved.contains(ours), "이미 우리 주소면 그대로: {saved}");
    let new_url = saved
        .split("](")
        .nth(1)
        .unwrap()
        .split(')')
        .next()
        .unwrap()
        .to_string();
    assert!(
        new_url.starts_with("https://cms.example.com/images/"),
        "{new_url}"
    );
    assert_eq!(saved.matches(&new_url).count(), 2, "두 군데 모두 바뀐다");
    let file = dir.join(new_url.rsplit('/').next().unwrap());
    assert_eq!(std::fs::read(&file).unwrap(), PNG);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn production_policy_refuses_http_and_internal_addresses() {
    let (port, hits) = image_server().await;
    let h = harness().await;
    let rehoster = ImageRehoster::new(env::temp_dir(), "https://cms.example.com");
    for (url, expected) in [
        (format!("http://127.0.0.1:{port}/cat.png"), "https 주소만"),
        (format!("https://127.0.0.1:{port}/cat.png"), "내부망"),
        ("https://localhost/cat.png".to_string(), "내부망"),
    ] {
        let (out, report) = rehoster.rehost(&h.pool, &format!("![x]({url})")).await;
        assert_eq!(out, format!("![x]({url})"));
        assert_eq!(report.rehosted, 0);
        assert!(
            report.warnings[0].contains(expected),
            "{url}: {:?}",
            report.warnings
        );
    }
    assert_eq!(
        hits.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "요청 자체를 보내지 않는다"
    );
}

#[test]
fn loopback_redirects_are_allowed_for_native_clients() {
    use super::oauth::is_allowed_redirect;
    for ok in [
        "https://claude.ai/api/mcp/auth_callback",
        "https://claude.com/api/mcp/auth_callback",
        "http://localhost:8765/callback",
        "http://127.0.0.1:51234/callback",
        "http://[::1]:9000/cb",
        "http://localhost/callback",
    ] {
        assert!(is_allowed_redirect(ok), "{ok}");
    }
    for bad in [
        "https://evil.example/cb",
        "http://localhost.evil.example/callback",
        "http://evil.example/localhost",
        "https://localhost:8765/callback",
        "http://user@localhost:8765/callback",
        "http://localhost:8765/callback#x",
        "http://10.0.0.1:8765/callback",
        "javascript:alert(1)",
        "",
    ] {
        assert!(!is_allowed_redirect(bad), "{bad}");
    }
}

#[tokio::test]
async fn claude_code_can_finish_oauth_through_a_loopback_redirect() {
    let h = harness().await;
    let loopback = "http://localhost:8765/callback";
    let mut body = json!({ "access_token": jwt("admin", "admin") });
    for (key, value) in authorize_params() {
        body[key] = json!(if key == "redirect_uri" {
            loopback.to_string()
        } else {
            value
        });
    }
    let resp = h
        .cli
        .post("/oauth/authorize/approve")
        .body_json(&body)
        .send()
        .await;
    resp.assert_status_is_ok();
    let body: Value = resp.json().await.value().deserialize();
    let redirect = body["redirect"].as_str().unwrap();
    assert!(redirect.starts_with(loopback), "{redirect}");
    let code = query_of(redirect)["code"].clone();

    let (status, body) = h
        .token(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", loopback),
            ("client_id", CLIENT_ID),
            ("code_verifier", VERIFIER),
        ])
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (_, body) = h.approve(&jwt("admin", "admin")).await;
    let code = query_of(body["redirect"].as_str().unwrap())["code"].clone();
    let (status, body) = h
        .token(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", loopback),
            ("client_id", CLIENT_ID),
            ("code_verifier", VERIFIER),
        ])
        .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "승인 때와 다른 redirect_uri 로는 코드를 못 바꾼다"
    );
    assert_eq!(body["error"], "invalid_grant");
}

#[tokio::test]
async fn initialize_names_the_server() {
    let h = harness().await;
    let (access, _) = h.login().await;
    let (status, body, _) = h
        .rpc(
            Some(&access),
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1" },
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["result"]["serverInfo"]["name"], "tyange-cms");
}

// ── 비밀번호 없는 admin 패스키 등록 ─────────────────────────────────────────

async fn setup_post(h: &Harness, path: &str, body: Value) -> (StatusCode, Value) {
    let resp = h.cli.post(path).body_json(&body).send().await;
    let status = resp.0.status();
    let text = resp.0.into_body().into_string().await.unwrap();
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

#[tokio::test]
async fn setup_code_lets_the_admin_register_a_passkey_once() {
    use webauthn_authenticator_rs::{softpasskey::SoftPasskey, WebauthnAuthenticator};
    use webauthn_rs::prelude::{RegisterPublicKeyCredential, Url};
    use webauthn_rs_proto::CreationChallengeResponse;

    let h = harness().await;
    let code = tokens::store_setup_code(&h.pool, "admin").await.unwrap();

    let (status, start) =
        setup_post(&h, "/oauth/passkey-setup/start", json!({ "code": code })).await;
    assert_eq!(status, StatusCode::OK, "{start}");
    let options: CreationChallengeResponse =
        serde_json::from_value(start["options"].clone()).unwrap();
    let mut authenticator = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let credential: RegisterPublicKeyCredential = authenticator
        .do_registration(Url::parse(PUBLIC_URL).unwrap(), options)
        .unwrap();

    let (status, body) = setup_post(
        &h,
        "/oauth/passkey-setup/finish",
        json!({ "code": code, "challenge_id": start["challenge_id"], "label": "Claude 커넥터", "credential": credential }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (owner, label): (String, String) = sqlx::query_as("SELECT user_id, label FROM passkeys")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(owner, "admin");
    assert_eq!(label, "Claude 커넥터");

    let (status, _) = setup_post(&h, "/oauth/passkey-setup/start", json!({ "code": code })).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "쓴 코드는 다시 못 쓴다");
}

#[tokio::test]
async fn setup_code_must_be_known_unexpired_and_belong_to_an_admin() {
    let h = harness().await;
    let (status, _) = setup_post(
        &h,
        "/oauth/passkey-setup/start",
        json!({ "code": "mcps_unknown" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    query("INSERT INTO mcp_passkey_setup_codes (code_hash, user_id, expires_at) VALUES (?, 'admin', ?)")
        .bind(tokens::hash("mcps_expired"))
        .bind(tokens::now() - 1)
        .execute(&h.pool)
        .await
        .unwrap();
    let (status, _) = setup_post(
        &h,
        "/oauth/passkey-setup/start",
        json!({ "code": "mcps_expired" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "만료된 코드");

    let writer_code = tokens::store_setup_code(&h.pool, "writer").await.unwrap();
    let (status, _) = setup_post(
        &h,
        "/oauth/passkey-setup/start",
        json!({ "code": writer_code }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "admin 이 아닌 계정");

    let (status, _) = setup_post(
        &h,
        "/oauth/passkey-setup/finish",
        json!({ "code": writer_code, "challenge_id": "x", "credential": {} }),
    )
    .await;
    assert!(
        status.is_client_error(),
        "finish 도 코드 주인을 다시 확인한다: {status}"
    );
    let count: i64 = query_scalar("SELECT COUNT(*) FROM passkeys")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn approval_is_passkey_only() {
    let page = super::oauth::AUTHORIZE_HTML;
    assert!(page.contains("/passkeys/login/start"));
    assert!(!page.contains("'/login'"), "비밀번호 로그인은 없다");
    assert!(!page.contains("type=\"password\""));
    let setup = super::oauth::SETUP_HTML;
    assert!(setup.contains("/oauth/passkey-setup/start"));
    assert!(setup.contains("/oauth/passkey-setup/finish"));
}
