use std::{env, sync::Arc, time::Duration};

use base64::{encode_config, URL_SAFE_NO_PAD};
use poem::{
    get, handler, listener::TcpAcceptor, post, test::TestClient, web::Data, Endpoint, EndpointExt, Response, Route,
    Server,
};
use serde_json::{json, Value};
use sqlx::{query_scalar, SqlitePool};
use tyange_cms_api::auth::jwt::Claims;
use web_push_native::{jwt_simple::algorithms::ES256KeyPair, p256::elliptic_curve::sec1::ToEncodedPoint, p256::SecretKey};

use crate::{
    db::init_db,
    gotolib_push::{PushHandle, PushService, VapidKeys},
    middlewares::auth_middleware::Auth,
    models::AppState,
    routes::{
        add_user::create_user,
        gotolib_push::{push_key, push_subscribe, push_test, push_unsubscribe},
    },
};

#[handler]
fn fake_push_ok(_hits: Data<&Arc<std::sync::atomic::AtomicUsize>>) -> Response {
    _hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    Response::builder().status(poem::http::StatusCode::CREATED).finish()
}

struct Setup {
    state: Arc<AppState>,
    hits: Arc<std::sync::atomic::AtomicUsize>,
    push_base: String,
}

async fn setup() -> Setup {
    env::set_var("JWT_ACCESS_SECRET", "test-access-secret");
    let db = SqlitePool::connect("sqlite::memory:").await.expect("sqlite");
    init_db(&db).await.expect("init db");
    let state = Arc::new(AppState::new(db));
    for user in ["me@example.com", "other@example.com"] {
        create_user(&state.db, user, "password123", "user").await.expect("user");
    }
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let fake = Route::new().at("/push/ok", post(fake_push_ok)).data(hits.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(Server::new_with_acceptor(TcpAcceptor::from_tokio(listener).unwrap()).run(fake));
    Setup { state, hits, push_base: format!("http://127.0.0.1:{port}") }
}

fn enabled(loopback: bool) -> PushHandle {
    let keys = VapidKeys::generate("https://gotolib.example");
    PushHandle(Some(Arc::new(PushService::new(keys, "http://127.0.0.1:1", loopback, 10, Duration::ZERO).unwrap())))
}

fn app(setup: &Setup, handle: PushHandle) -> impl Endpoint {
    Route::new()
        .at("/gotolib/push/key", get(push_key))
        .at("/gotolib/push/subscribe", post(push_subscribe.with(Auth)))
        .at("/gotolib/push/unsubscribe", post(push_unsubscribe.with(Auth)))
        .at("/gotolib/push/test", post(push_test.with(Auth)))
        .data(handle)
        .data(setup.state.clone())
}

fn token(user_id: &str) -> String {
    Claims::create_access_token(user_id, "user", b"test-access-secret").expect("token")
}

fn subscription(endpoint: &str) -> Value {
    let key_pair = ES256KeyPair::generate();
    let secret = SecretKey::from_slice(&key_pair.to_bytes()).unwrap();
    json!({
        "endpoint": endpoint,
        "keys": {
            "p256dh": encode_config(secret.public_key().to_encoded_point(false).as_bytes(), URL_SAFE_NO_PAD),
            "auth": encode_config([9u8; 16], URL_SAFE_NO_PAD),
        }
    })
}

async fn count(setup: &Setup, user_id: &str) -> i64 {
    query_scalar("SELECT COUNT(*) FROM gotolib_push_subscriptions WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(&setup.state.db)
        .await
        .unwrap()
}

async fn json_of(response: poem::test::TestResponse) -> Value {
    serde_json::from_str(&response.0.into_body().into_string().await.unwrap()).unwrap()
}

#[tokio::test]
async fn everything_is_unavailable_while_notifications_are_off() {
    let setup = setup().await;
    let cli = TestClient::new(app(&setup, PushHandle(None)));
    cli.get("/gotolib/push/key").send().await.assert_status(poem::http::StatusCode::SERVICE_UNAVAILABLE);
    cli.post("/gotolib/push/subscribe")
        .header("Authorization", token("me@example.com"))
        .body_json(&subscription("https://fcm.googleapis.com/fcm/send/abc"))
        .send()
        .await
        .assert_status(poem::http::StatusCode::SERVICE_UNAVAILABLE);
    cli.post("/gotolib/push/test")
        .header("Authorization", token("me@example.com"))
        .send()
        .await
        .assert_status(poem::http::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(count(&setup, "me@example.com").await, 0);
}

#[tokio::test]
async fn the_public_key_needs_no_login_and_is_an_uncompressed_p256_point() {
    let setup = setup().await;
    let cli = TestClient::new(app(&setup, enabled(false)));
    let res = cli.get("/gotolib/push/key").send().await;
    res.assert_status_is_ok();
    let key = json_of(res).await["public_key"].as_str().unwrap().to_string();
    let raw = base64::decode_config(&key, URL_SAFE_NO_PAD).unwrap();
    assert_eq!((raw.len(), raw[0]), (65, 4));
}

#[tokio::test]
async fn subscribing_requires_login() {
    let setup = setup().await;
    let cli = TestClient::new(app(&setup, enabled(false)));
    for path in ["/gotolib/push/subscribe", "/gotolib/push/unsubscribe", "/gotolib/push/test"] {
        cli.post(path)
            .body_json(&subscription("https://fcm.googleapis.com/fcm/send/abc"))
            .send()
            .await
            .assert_status(poem::http::StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn a_valid_subscription_is_stored_and_a_bad_one_is_refused() {
    let setup = setup().await;
    let cli = TestClient::new(app(&setup, enabled(false)));
    let auth = token("me@example.com");

    let ok = cli
        .post("/gotolib/push/subscribe")
        .header("Authorization", &auth)
        .header("User-Agent", "Mozilla/5.0 iPhone")
        .body_json(&subscription("https://web.push.apple.com/QGabc"))
        .send()
        .await;
    ok.assert_status_is_ok();
    assert_eq!(count(&setup, "me@example.com").await, 1);
    let agent: String = query_scalar("SELECT user_agent FROM gotolib_push_subscriptions WHERE user_id = 'me@example.com'")
        .fetch_one(&setup.state.db)
        .await
        .unwrap();
    assert_eq!(agent, "Mozilla/5.0 iPhone");

    // 서버가 요청을 보내는 주소라서 알려진 푸시 서비스가 아니면 거부한다
    for endpoint in ["https://evil.example/x", "http://fcm.googleapis.com/x", "http://127.0.0.1:1/push/ok", "not a url"] {
        cli.post("/gotolib/push/subscribe")
            .header("Authorization", &auth)
            .body_json(&subscription(endpoint))
            .send()
            .await
            .assert_status(poem::http::StatusCode::BAD_REQUEST);
    }
    // 키가 이상해도 거부한다
    let mut broken = subscription("https://fcm.googleapis.com/fcm/send/abc");
    broken["keys"]["p256dh"] = json!("AAAA");
    cli.post("/gotolib/push/subscribe")
        .header("Authorization", &auth)
        .body_json(&broken)
        .send()
        .await
        .assert_status(poem::http::StatusCode::BAD_REQUEST);
    assert_eq!(count(&setup, "me@example.com").await, 1, "거부된 요청은 저장되지 않는다");
}

#[tokio::test]
async fn unsubscribing_removes_only_the_callers_subscription() {
    let setup = setup().await;
    let cli = TestClient::new(app(&setup, enabled(false)));
    let endpoint = "https://fcm.googleapis.com/fcm/send/mine";
    cli.post("/gotolib/push/subscribe")
        .header("Authorization", token("me@example.com"))
        .body_json(&subscription(endpoint))
        .send()
        .await
        .assert_status_is_ok();

    let other = cli
        .post("/gotolib/push/unsubscribe")
        .header("Authorization", token("other@example.com"))
        .body_json(&json!({ "endpoint": endpoint }))
        .send()
        .await;
    other.assert_status_is_ok();
    assert_eq!(json_of(other).await["removed"], 0);
    assert_eq!(count(&setup, "me@example.com").await, 1);

    let mine = cli
        .post("/gotolib/push/unsubscribe")
        .header("Authorization", token("me@example.com"))
        .body_json(&json!({ "endpoint": endpoint }))
        .send()
        .await;
    assert_eq!(json_of(mine).await["removed"], 1);
    assert_eq!(count(&setup, "me@example.com").await, 0);
}

#[tokio::test]
async fn the_test_notification_reaches_only_my_devices() {
    let setup = setup().await;
    let cli = TestClient::new(app(&setup, enabled(true)));
    // 시험 환경에서만 루프백 주소를 받는다
    cli.post("/gotolib/push/subscribe")
        .header("Authorization", token("me@example.com"))
        .body_json(&subscription(&format!("{}/push/ok", setup.push_base)))
        .send()
        .await
        .assert_status_is_ok();

    let mine = cli.post("/gotolib/push/test").header("Authorization", token("me@example.com")).send().await;
    mine.assert_status_is_ok();
    let body = json_of(mine).await;
    assert_eq!((body["delivered"].as_u64(), body["removed"].as_u64()), (Some(1), Some(0)));
    assert_eq!(setup.hits.load(std::sync::atomic::Ordering::SeqCst), 1);

    // 구독이 없는 다른 계정이 눌러도 내 기기로는 가지 않는다
    let theirs = cli.post("/gotolib/push/test").header("Authorization", token("other@example.com")).send().await;
    assert_eq!(json_of(theirs).await["delivered"], 0);
    assert_eq!(setup.hits.load(std::sync::atomic::Ordering::SeqCst), 1);
}
