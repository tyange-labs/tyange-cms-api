use std::{env, sync::Arc};

use poem::{
    delete, get, post,
    test::{TestClient, TestResponse},
    Endpoint, EndpointExt, Route,
};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use webauthn_authenticator_rs::{softpasskey::SoftPasskey, WebauthnAuthenticator};
use webauthn_rs::prelude::{PublicKeyCredential, RegisterPublicKeyCredential, Url};
use webauthn_rs_proto::{CreationChallengeResponse, RequestChallengeResponse};

use crate::{
    db::init_db,
    middlewares::auth_middleware::Auth,
    models::AppState,
    passkey::PasskeyService,
    routes::{
        add_user::create_user,
        passkeys::{
            delete_passkey, list_passkeys, login_finish, login_start, register_finish,
            register_start, signup_finish, signup_start,
        },
    },
};
use tyange_cms_api::auth::jwt::Claims;

const ORIGIN: &str = "https://gotolib.example.com";

async fn state_with(passkeys: PasskeyService) -> Arc<AppState> {
    env::set_var("JWT_ACCESS_SECRET", "test-access-secret");
    let db = SqlitePool::connect("sqlite::memory:")
        .await
        .expect("failed to connect sqlite");
    init_db(&db).await.expect("failed to init db");
    let state = Arc::new(AppState::new_with_passkeys(db, passkeys));
    create_user(&state.db, "me@example.com", "password123", "user")
        .await
        .expect("create user");
    create_user(&state.db, "other@example.com", "password123", "user")
        .await
        .expect("create other user");
    state
}

async fn enabled_state() -> Arc<AppState> {
    let service = PasskeyService::build("example.com", ORIGIN, "test").expect("build service");
    state_with(service).await
}

fn app(state: Arc<AppState>) -> impl Endpoint {
    Route::new()
        .at("/passkeys/signup/start", post(signup_start))
        .at("/passkeys/signup/finish", post(signup_finish))
        .at("/passkeys/login/start", post(login_start))
        .at("/passkeys/login/finish", post(login_finish))
        .at("/passkeys/register/start", post(register_start).with(Auth))
        .at("/passkeys/register/finish", post(register_finish).with(Auth))
        .at("/passkeys", get(list_passkeys).with(Auth))
        .at("/passkeys/:id", delete(delete_passkey).with(Auth))
        .data(state)
}

fn token(user_id: &str) -> String {
    Claims::create_access_token(user_id, "user", b"test-access-secret").expect("token")
}

async fn json_of(response: TestResponse) -> Value {
    serde_json::from_str(&response.0.into_body().into_string().await.expect("body"))
        .expect("json body")
}

/// 등록 전체 절차를 소프트웨어 인증기로 수행하고 (자격 증명 id, 인증기)를 돌려준다.
async fn register<E: Endpoint>(
    cli: &TestClient<E>,
    user_id: &str,
    label: &str,
    authenticator: &mut WebauthnAuthenticator<SoftPasskey>,
) -> String {
    let start = cli
        .post("/passkeys/register/start")
        .header("Authorization", token(user_id))
        .send()
        .await;
    start.assert_status_is_ok();
    let start = json_of(start).await;
    let options: CreationChallengeResponse =
        serde_json::from_value(start["options"].clone()).expect("creation options");

    let credential: RegisterPublicKeyCredential = authenticator
        .do_registration(Url::parse(ORIGIN).unwrap(), options)
        .expect("soft registration");

    let finish = cli
        .post("/passkeys/register/finish")
        .header("Authorization", token(user_id))
        .body_json(&json!({
            "challenge_id": start["challenge_id"],
            "label": label,
            "credential": credential,
        }))
        .send()
        .await;
    finish.assert_status_is_ok();
    json_of(finish).await["id"].as_str().unwrap().to_string()
}

/// 사용자명 없는 로그인의 클라이언트 쪽을 흉내 낸다. 소프트웨어 인증기는 레지던트 키가 없고
/// 응답에 user handle도 안 채우므로, 등록한 자격 증명을 allowCredentials로 넘겨 서명을 받고
/// user handle만 직접 채운다 — 서명 검증과 챌린지·origin 확인은 실제 그대로 거친다.
fn authenticate(
    authenticator: &mut WebauthnAuthenticator<SoftPasskey>,
    start: &Value,
    credential_id: &str,
    user_handle: &[u8],
) -> PublicKeyCredential {
    let mut options = start["options"].clone();
    options["publicKey"]["allowCredentials"] =
        json!([{ "type": "public-key", "id": credential_id }]);
    let options: RequestChallengeResponse =
        serde_json::from_value(options).expect("request options");
    let mut credential = authenticator
        .do_authentication(Url::parse(ORIGIN).unwrap(), options)
        .expect("soft authentication");
    credential.response.user_handle = Some(user_handle.to_vec().into());
    credential
}

async fn user_handle_of(state: &AppState, user_id: &str) -> Vec<u8> {
    let handle: String = sqlx::query_scalar("SELECT user_handle FROM passkeys WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(&state.db)
        .await
        .expect("user handle");
    uuid::Uuid::parse_str(&handle).unwrap().as_bytes().to_vec()
}

#[tokio::test]
async fn passkey_endpoints_return_503_when_not_configured() {
    let state = state_with(PasskeyService::disabled()).await;
    let cli = TestClient::new(app(state));

    cli.post("/passkeys/login/start")
        .send()
        .await
        .assert_status(poem::http::StatusCode::SERVICE_UNAVAILABLE);
    cli.post("/passkeys/register/start")
        .header("Authorization", token("me@example.com"))
        .send()
        .await
        .assert_status(poem::http::StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn register_and_list_and_delete_require_authentication() {
    let cli = TestClient::new(app(enabled_state().await));

    cli.post("/passkeys/register/start")
        .send()
        .await
        .assert_status(poem::http::StatusCode::UNAUTHORIZED);
    cli.get("/passkeys")
        .send()
        .await
        .assert_status(poem::http::StatusCode::UNAUTHORIZED);
    cli.delete("/passkeys/abc")
        .send()
        .await
        .assert_status(poem::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn register_asks_for_discoverable_credentials() {
    let cli = TestClient::new(app(enabled_state().await));
    let start = cli
        .post("/passkeys/register/start")
        .header("Authorization", token("me@example.com"))
        .send()
        .await;
    start.assert_status_is_ok();
    let start = json_of(start).await;
    assert_eq!(
        start["options"]["publicKey"]["authenticatorSelection"]["residentKey"],
        "preferred"
    );
    assert_eq!(start["options"]["publicKey"]["rp"]["id"], "example.com");
}

#[tokio::test]
async fn passkey_registration_then_login_issues_same_jwt_as_password_login() {
    let state = enabled_state().await;
    let cli = TestClient::new(app(state.clone()));
    let mut authenticator = WebauthnAuthenticator::new(SoftPasskey::new(true));

    let credential_id = register(&cli, "me@example.com", "내 맥북", &mut authenticator).await;

    let list = json_of(
        cli.get("/passkeys")
            .header("Authorization", token("me@example.com"))
            .send()
            .await,
    )
    .await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["label"], "내 맥북");
    assert_eq!(list[0]["id"], credential_id);

    // 로그인: 인증 없이 start → 서명 → finish
    let start = cli.post("/passkeys/login/start").send().await;
    start.assert_status_is_ok();
    let start = json_of(start).await;
    let handle = user_handle_of(&state, "me@example.com").await;
    let credential = authenticate(&mut authenticator, &start, &credential_id, &handle);

    let finish = cli
        .post("/passkeys/login/finish")
        .body_json(&json!({
            "challenge_id": start["challenge_id"],
            "credential": credential,
        }))
        .send()
        .await;
    finish.assert_status_is_ok();
    let finish = json_of(finish).await;
    assert_eq!(finish["user_role"], "user");

    let claims = Claims::from_token(finish["access_token"].as_str().unwrap(), b"test-access-secret")
        .expect("valid jwt");
    assert_eq!(claims.claims.sub, "me@example.com");

    // 사용 시각이 남았다
    let last_used: Option<String> =
        sqlx::query_scalar("SELECT last_used_at FROM passkeys WHERE credential_id = ?")
            .bind(&credential_id)
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert!(last_used.is_some());
}

#[tokio::test]
async fn login_challenge_can_only_be_used_once() {
    let state = enabled_state().await;
    let cli = TestClient::new(app(state.clone()));
    let mut authenticator = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let credential_id = register(&cli, "me@example.com", "x", &mut authenticator).await;

    let start = json_of(cli.post("/passkeys/login/start").send().await).await;
    let handle = user_handle_of(&state, "me@example.com").await;
    let credential = authenticate(&mut authenticator, &start, &credential_id, &handle);
    let body = json!({ "challenge_id": start["challenge_id"], "credential": credential });

    cli.post("/passkeys/login/finish")
        .body_json(&body)
        .send()
        .await
        .assert_status_is_ok();
    // 같은 응답을 다시 보내면(재전송 공격) 거부된다.
    cli.post("/passkeys/login/finish")
        .body_json(&body)
        .send()
        .await
        .assert_status(poem::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn login_rejects_unknown_challenge_and_unknown_passkey() {
    let state = enabled_state().await;
    let cli = TestClient::new(app(state.clone()));
    let mut authenticator = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let credential_id = register(&cli, "me@example.com", "x", &mut authenticator).await;

    // 서버가 만든 적 없는 challenge_id
    let start = json_of(cli.post("/passkeys/login/start").send().await).await;
    let handle = user_handle_of(&state, "me@example.com").await;
    let credential = authenticate(&mut authenticator, &start, &credential_id, &handle);
    cli.post("/passkeys/login/finish")
        .body_json(&json!({ "challenge_id": "nope", "credential": credential }))
        .send()
        .await
        .assert_status(poem::http::StatusCode::BAD_REQUEST);

    // 등록된 적 없는 user handle
    let start = json_of(cli.post("/passkeys/login/start").send().await).await;
    let credential = authenticate(&mut authenticator, &start, &credential_id, b"0123456789abcdef");
    cli.post("/passkeys/login/finish")
        .body_json(&json!({ "challenge_id": start["challenge_id"], "credential": credential }))
        .send()
        .await
        .assert_status(poem::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn deleted_passkey_can_no_longer_log_in() {
    let state = enabled_state().await;
    let cli = TestClient::new(app(state.clone()));
    let mut authenticator = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let credential_id = register(&cli, "me@example.com", "x", &mut authenticator).await;
    let handle = user_handle_of(&state, "me@example.com").await;

    cli.delete(format!("/passkeys/{credential_id}"))
        .header("Authorization", token("me@example.com"))
        .send()
        .await
        .assert_status_is_ok();

    let start = json_of(cli.post("/passkeys/login/start").send().await).await;
    let credential = authenticate(&mut authenticator, &start, &credential_id, &handle);
    cli.post("/passkeys/login/finish")
        .body_json(&json!({ "challenge_id": start["challenge_id"], "credential": credential }))
        .send()
        .await
        .assert_status(poem::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn users_cannot_delete_or_see_each_others_passkeys() {
    let state = enabled_state().await;
    let cli = TestClient::new(app(state));
    let mut authenticator = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let credential_id = register(&cli, "me@example.com", "x", &mut authenticator).await;

    cli.delete(format!("/passkeys/{credential_id}"))
        .header("Authorization", token("other@example.com"))
        .send()
        .await
        .assert_status(poem::http::StatusCode::NOT_FOUND);

    let list = json_of(
        cli.get("/passkeys")
            .header("Authorization", token("other@example.com"))
            .send()
            .await,
    )
    .await;
    assert!(list.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn register_finish_rejects_challenge_started_by_another_user() {
    let cli = TestClient::new(app(enabled_state().await));
    let mut authenticator = WebauthnAuthenticator::new(SoftPasskey::new(true));

    let start = cli
        .post("/passkeys/register/start")
        .header("Authorization", token("me@example.com"))
        .send()
        .await;
    let start = json_of(start).await;
    let options: CreationChallengeResponse =
        serde_json::from_value(start["options"].clone()).unwrap();
    let credential = authenticator
        .do_registration(Url::parse(ORIGIN).unwrap(), options)
        .unwrap();

    cli.post("/passkeys/register/finish")
        .header("Authorization", token("other@example.com"))
        .body_json(&json!({
            "challenge_id": start["challenge_id"],
            "label": "x",
            "credential": credential,
        }))
        .send()
        .await
        .assert_status(poem::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn already_registered_authenticator_is_excluded_from_registration() {
    let cli = TestClient::new(app(enabled_state().await));
    let mut authenticator = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let credential_id = register(&cli, "me@example.com", "x", &mut authenticator).await;

    let start = json_of(
        cli.post("/passkeys/register/start")
            .header("Authorization", token("me@example.com"))
            .send()
            .await,
    )
    .await;
    let excluded = start["options"]["publicKey"]["excludeCredentials"]
        .as_array()
        .expect("excludeCredentials");
    assert_eq!(excluded.len(), 1);
    assert_eq!(excluded[0]["id"], credential_id);
}

#[tokio::test]
async fn login_signed_for_a_different_origin_is_rejected() {
    let state = enabled_state().await;
    let cli = TestClient::new(app(state.clone()));
    let mut authenticator = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let credential_id = register(&cli, "me@example.com", "x", &mut authenticator).await;
    let handle = user_handle_of(&state, "me@example.com").await;

    // RP ID(example.com)는 유효하지만 허용 목록에 없는 서브도메인(탈취된 서브도메인·피싱)에서
    // 받은 서명은 챌린지가 맞아도 서버가 거부해야 한다.
    let start = json_of(cli.post("/passkeys/login/start").send().await).await;
    let mut options = start["options"].clone();
    options["publicKey"]["allowCredentials"] =
        json!([{ "type": "public-key", "id": credential_id }]);
    let options: RequestChallengeResponse = serde_json::from_value(options).unwrap();
    let mut credential = authenticator
        .do_authentication(Url::parse("https://evil.example.com").unwrap(), options)
        .expect("soft authentication");
    credential.response.user_handle = Some(handle.into());

    cli.post("/passkeys/login/finish")
        .body_json(&json!({ "challenge_id": start["challenge_id"], "credential": credential }))
        .send()
        .await
        .assert_status(poem::http::StatusCode::UNAUTHORIZED);
}

/// 브라우저(`PublicKeyCredential.toJSON()`, @simplewebauthn/browser)가 실제로 보내는 모양이
/// 서버 타입으로 읽히는지. 소프트웨어 인증기 테스트는 서버 자신의 직렬화를 쓰므로 이 모양은
/// 따로 확인해야 한다(camelCase 필드, clientExtensionResults, 서버가 모르는 필드).
#[test]
fn browser_tojson_shapes_deserialize_into_server_types() {
    let registration: RegisterPublicKeyCredential = serde_json::from_value(json!({
        "id": "AAAA",
        "rawId": "AAAA",
        "type": "public-key",
        "authenticatorAttachment": "platform",
        "response": {
            "attestationObject": "AAAA",
            "clientDataJSON": "AAAA",
            "transports": ["internal", "hybrid", "usb", "nfc", "ble"],
            "publicKeyAlgorithm": -7,
            "authenticatorData": "AAAA"
        },
        "clientExtensionResults": {}
    }))
    .expect("registration JSON from a browser");
    assert_eq!(registration.id, "AAAA");

    for user_handle in [json!("AAAA"), json!(null)] {
        let assertion: PublicKeyCredential = serde_json::from_value(json!({
            "id": "AAAA",
            "rawId": "AAAA",
            "type": "public-key",
            "authenticatorAttachment": "platform",
            "response": {
                "authenticatorData": "AAAA",
                "clientDataJSON": "AAAA",
                "signature": "AAAA",
                "userHandle": user_handle
            },
            "clientExtensionResults": {}
        }))
        .expect("assertion JSON from a browser");
        assert_eq!(assertion.id, "AAAA");
    }
}

// ── 가입 (이메일·비밀번호 없이 패스키로 계정 생성) ─────────────────────────────

/// 가입 전체 절차. (응답 JSON, 인증기)를 돌려준다.
async fn sign_up<E: Endpoint>(
    cli: &TestClient<E>,
    authenticator: &mut WebauthnAuthenticator<SoftPasskey>,
) -> TestResponse {
    let start = cli.post("/passkeys/signup/start").send().await;
    start.assert_status_is_ok();
    let start = json_of(start).await;
    let options: CreationChallengeResponse =
        serde_json::from_value(start["options"].clone()).expect("creation options");
    let credential = authenticator
        .do_registration(Url::parse(ORIGIN).unwrap(), options)
        .expect("soft registration");
    cli.post("/passkeys/signup/finish")
        .body_json(&json!({
            "challenge_id": start["challenge_id"],
            "label": "내 폰",
            "credential": credential,
        }))
        .send()
        .await
}

async fn count_gotolib_accounts(state: &AppState) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE user_role = 'gotolib'")
        .fetch_one(&state.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn signup_creates_a_gotolib_account_and_returns_a_jwt() {
    let state = enabled_state().await;
    let cli = TestClient::new(app(state.clone()));
    let mut authenticator = WebauthnAuthenticator::new(SoftPasskey::new(true));

    let response = sign_up(&cli, &mut authenticator).await;
    response.assert_status_is_ok();
    let body = json_of(response).await;
    assert_eq!(body["user_role"], "gotolib");

    let claims = Claims::from_token(body["access_token"].as_str().unwrap(), b"test-access-secret")
        .expect("valid jwt");
    assert!(claims.claims.sub.starts_with("gl-"));
    assert_eq!(claims.claims.role, "gotolib");

    // 계정은 비밀번호가 없다(비밀번호 로그인 경로로 들어올 수 없다).
    let password: Option<String> =
        sqlx::query_scalar("SELECT password FROM users WHERE user_id = ?")
            .bind(&claims.claims.sub)
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert!(password.is_none());

    // 만든 패스키로 사용자명 없이 다시 로그인하면 같은 계정이다.
    let credential_id: String =
        sqlx::query_scalar("SELECT credential_id FROM passkeys WHERE user_id = ?")
            .bind(&claims.claims.sub)
            .fetch_one(&state.db)
            .await
            .unwrap();
    let handle = user_handle_of(&state, &claims.claims.sub).await;
    let start = json_of(cli.post("/passkeys/login/start").send().await).await;
    let credential = authenticate(&mut authenticator, &start, &credential_id, &handle);
    let login = cli
        .post("/passkeys/login/finish")
        .body_json(&json!({ "challenge_id": start["challenge_id"], "credential": credential }))
        .send()
        .await;
    login.assert_status_is_ok();
    let login = json_of(login).await;
    assert_eq!(login["user_role"], "gotolib");
    let again = Claims::from_token(login["access_token"].as_str().unwrap(), b"test-access-secret")
        .unwrap();
    assert_eq!(again.claims.sub, claims.claims.sub);
}

#[tokio::test]
async fn signup_start_alone_creates_no_account() {
    let state = enabled_state().await;
    let cli = TestClient::new(app(state.clone()));
    let before = count_gotolib_accounts(&state).await;

    for _ in 0..3 {
        cli.post("/passkeys/signup/start")
            .send()
            .await
            .assert_status_is_ok();
    }
    assert_eq!(count_gotolib_accounts(&state).await, before);
}

#[tokio::test]
async fn signup_is_unavailable_when_passkeys_are_not_configured() {
    let state = state_with(PasskeyService::disabled()).await;
    let cli = TestClient::new(app(state));
    cli.post("/passkeys/signup/start")
        .send()
        .await
        .assert_status(poem::http::StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn signup_stops_at_the_account_cap_even_if_challenges_were_started_earlier() {
    let service = PasskeyService::build("example.com", ORIGIN, "test")
        .expect("build service")
        .with_max_accounts(1);
    let state = state_with(service).await;
    let cli = TestClient::new(app(state.clone()));

    // 상한 1: 두 가입이 동시에 시작돼도 한 명만 만들어진다.
    let mut first = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let mut second = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let start_a = json_of(cli.post("/passkeys/signup/start").send().await).await;
    let start_b = json_of(cli.post("/passkeys/signup/start").send().await).await;
    let register = |auth: &mut WebauthnAuthenticator<SoftPasskey>, start: &Value| {
        let options: CreationChallengeResponse =
            serde_json::from_value(start["options"].clone()).unwrap();
        auth.do_registration(Url::parse(ORIGIN).unwrap(), options).unwrap()
    };
    let cred_a = register(&mut first, &start_a);
    let cred_b = register(&mut second, &start_b);

    cli.post("/passkeys/signup/finish")
        .body_json(&json!({ "challenge_id": start_a["challenge_id"], "credential": cred_a }))
        .send()
        .await
        .assert_status_is_ok();
    cli.post("/passkeys/signup/finish")
        .body_json(&json!({ "challenge_id": start_b["challenge_id"], "credential": cred_b }))
        .send()
        .await
        .assert_status(poem::http::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(count_gotolib_accounts(&state).await, 1);

    // 상한에 닿은 뒤의 새 시작도 거절한다.
    cli.post("/passkeys/signup/start")
        .send()
        .await
        .assert_status(poem::http::StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn signup_challenges_and_register_challenges_cannot_be_swapped() {
    let state = enabled_state().await;
    let cli = TestClient::new(app(state.clone()));
    let mut authenticator = WebauthnAuthenticator::new(SoftPasskey::new(true));

    // 가입 챌린지로 로그인한 사용자의 '패스키 추가'를 완료할 수 없다.
    let start = json_of(cli.post("/passkeys/signup/start").send().await).await;
    let options: CreationChallengeResponse =
        serde_json::from_value(start["options"].clone()).unwrap();
    let credential = authenticator
        .do_registration(Url::parse(ORIGIN).unwrap(), options)
        .unwrap();
    cli.post("/passkeys/register/finish")
        .header("Authorization", token("me@example.com"))
        .body_json(&json!({ "challenge_id": start["challenge_id"], "credential": credential }))
        .send()
        .await
        .assert_status(poem::http::StatusCode::BAD_REQUEST);

    // 반대로 '패스키 추가' 챌린지로 가입을 완료해 새 계정을 만들 수도 없다.
    let reg = cli
        .post("/passkeys/register/start")
        .header("Authorization", token("me@example.com"))
        .send()
        .await;
    let reg = json_of(reg).await;
    let options: CreationChallengeResponse =
        serde_json::from_value(reg["options"].clone()).unwrap();
    let mut other = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let credential = other
        .do_registration(Url::parse(ORIGIN).unwrap(), options)
        .unwrap();
    let before = count_gotolib_accounts(&state).await;
    cli.post("/passkeys/signup/finish")
        .body_json(&json!({ "challenge_id": reg["challenge_id"], "credential": credential }))
        .send()
        .await
        .assert_status(poem::http::StatusCode::BAD_REQUEST);
    assert_eq!(count_gotolib_accounts(&state).await, before);
}
