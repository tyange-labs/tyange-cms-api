mod blog_redeploy;
mod covers;
mod db;
mod gotolib_push;
mod middlewares;
mod models;
mod openapi;
mod passkey;
mod routes;
mod services;
mod utils;

use dotenv::dotenv;
use middlewares::admin_middleware::AdminOnly;
use middlewares::auth_middleware::Auth;
use middlewares::rate_limit::RateLimit;
use std::{env, fs, sync::Arc, time::Duration};

use crate::routes::create_wedding_guest::create_wedding_guest;
use crate::routes::get_count_with_tags::get_count_with_tags;
use crate::routes::get_posts_with_tags::get_posts_with_tags;
use crate::routes::gotolib_covers::{get_covers, IMAGE_PATH_PREFIX};
use crate::routes::gotolib_push::{
    push_key, push_subscribe, push_test, push_unsubscribe, PUSH_BODY_MAX_BYTES,
};
use crate::routes::gotolib_sync::{get_sync, put_sync, MAX_SYNC_BYTES};
use crate::routes::me::me;
use crate::routes::passkeys::{
    delete_passkey, list_passkeys, login_finish, login_start, register_finish, register_start,
    signup_finish, signup_start,
};
use crate::routes::update_my_profile::update_my_profile;
use crate::routes::upload_image::upload_image;
use crate::{models::AppState, routes::add_user::add_user};
use db::init_db;
use openapi::{ImagesApi, PortfolioApi, PostsApi, TagsApi, WeddingApi};
use poem::{
    delete,
    endpoint::StaticFilesEndpoint,
    get, handler,
    http::StatusCode,
    listener::TcpListener,
    middleware::{Cors, SetHeader, SizeLimit},
    post, put, EndpointExt, Response, Route, Server,
};
use poem_openapi::OpenApiService;
use routes::login::login;
use sqlx::SqlitePool;

#[handler]
fn return_str() -> &'static str {
    "hello"
}

#[handler]
async fn options_handler() -> Response {
    Response::builder().status(StatusCode::OK).finish()
}

const DEFAULT_UPLOAD_MAX_BYTES: usize = 20 * 1024 * 1024;

/// 이 기간 동안 아무 활동이 없는 gotolib 전용 계정은 정리한다.
const GOTOLIB_INACTIVE_DAYS: u32 = 365;

/// 패스키 등록·로그인 본문은 WebAuthn 응답 하나(수 KB)라 넉넉히 잡아도 16KB면 충분하다.
const PASSKEY_MAX_BYTES: usize = 16 * 1024;

/// 동기화 PUT 본문 = 문서(MAX_SYNC_BYTES 이하) + base_version 등 봉투.
const SYNC_ENVELOPE_BYTES: usize = 4 * 1024;

/// RSVP 본문은 name + companions 두 필드뿐이라 넉넉히 잡아도 4KB면 충분하다.
const RSVP_MAX_BYTES: usize = 4 * 1024;

fn upload_size_limit() -> usize {
    env::var("UPLOAD_MAX_BYTES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_UPLOAD_MAX_BYTES)
}

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    dotenv().ok();

    let db_path = env::var("DATABASE_PATH").unwrap_or_else(|_| "./data/database.db".to_string());
    if let Some(parent) = std::path::Path::new(&db_path).parent() {
        fs::create_dir_all(parent)?;
    }

    let db_url = format!("sqlite:{}?mode=rwc", db_path);
    println!("Database URL: {}", db_url);

    let db = SqlitePool::connect(&db_url).await.map_err(|e| {
        eprintln!("Connection with Database: {:?}", e);
        std::io::Error::new(std::io::ErrorKind::Other, e)
    })?;

    init_db(&db)
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    // 쓰지 않는 gotolib 전용 계정 정리: 시작할 때와 하루에 한 번.
    {
        let db = db.clone();
        tokio::spawn(async move {
            loop {
                match db::purge_inactive_gotolib_accounts(&db, GOTOLIB_INACTIVE_DAYS).await {
                    Ok(0) => {}
                    Ok(removed) => println!("비활성 gotolib 계정 {removed}개 정리"),
                    Err(_) => eprintln!("비활성 gotolib 계정 정리 실패"),
                }
                tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
            }
        });
    }

    // gotolib 책 표지 수집(알라딘 상품 페이지 → 썸네일). COVER_FETCH_ENABLED=true일 때만 돈다.
    let covers_dir = covers::covers_dir();
    if let Err(e) = fs::create_dir_all(&covers_dir) {
        eprintln!("[covers] 썸네일 폴더를 만들지 못했습니다: {e}");
    }
    covers::spawn_worker_from_env(db.clone());

    // gotolib 찜한 책 소장 알림(Web Push). GOTOLIB_PUSH_ENABLED=true일 때만 켜진다.
    let push = gotolib_push::handle_from_env();
    if let Some(service) = &push.0 {
        gotolib_push::spawn_worker(db.clone(), service.clone());
    }

    let state = Arc::new(AppState::new(db));

    fn configure_routes(state: &Arc<AppState>) -> Route {
        let upload_base_path = env::var("UPLOAD_PATH").unwrap_or(String::from(".uploads/images"));
        let upload_max_bytes = upload_size_limit();

        // #[OpenApi]로 옮긴 라우트. 오퍼레이션 path(`#[oai(path = "/posts")]`)가 그대로
        // 서빙 경로가 되도록 root("/")에 nest한다 — tyange-cms/lovelog가 부르는 URL이
        // 바뀌면 안 되기 때문. 스펙/문서는 별도 경로(/api-docs)에서만 노출한다.
        //
        // ImagesApi::upload_image에는 원래 /upload-image에 걸려있던 SizeLimit이 없다.
        // poem::middleware::SizeLimit은 Content-Length 헤더가 없는 요청을 전부 411로
        // 거부해서 OpenApiService 전체(GET 포함)에 걸면 다른 오퍼레이션이 깨지고,
        // OpenApiService는 오퍼레이션 단위로 미들웨어를 걸 수도 없다. 게다가
        // poem_openapi::types::multipart::Upload는 크기 제한 없이 통째로 임시 파일에
        // 먼저 써버려서, 핸들러 안에서 policy.max_bytes를 체크해도 그 시점엔 이미 늦다.
        // 이 엔드포인트는 인증(AuthGuard) 필수라 무제한 업로드 노출 위험은 제한적이지만,
        // 완전한 하위 호환은 아니다 — 크기 제한이 꼭 필요하면 옛 `/images/upload`
        // (plain poem, SizeLimit 그대로 유지)를 쓴다.
        let api_service = OpenApiService::new(
            (
                PostsApi {
                    state: state.clone(),
                },
                TagsApi {
                    state: state.clone(),
                },
                PortfolioApi {
                    state: state.clone(),
                },
                ImagesApi {
                    state: state.clone(),
                },
                WeddingApi {
                    state: state.clone(),
                },
            ),
            "tyange-cms-api",
            "1.0",
        );
        let api_docs = api_service.swagger_ui();
        let api_spec = api_service.spec_endpoint();

        Route::new()
            .at("/health", get(return_str))
            .at("/health-check", get(return_str))
            .nest("/", api_service)
            .nest("/api-docs", api_docs)
            .at("/api-docs/openapi.json", api_spec)
            .at("/posts/search-with-tags", get(get_posts_with_tags))
            .at("/tags", get(get_count_with_tags))
            .at(
                "/images/upload",
                post(upload_image)
                    .with(SizeLimit::new(upload_max_bytes))
                    .with(Auth),
            )
            .at("/login", post(login))
            // 패스키: 등록은 로그인한 상태에서, 로그인은 사용자명 없이. 로그인 쪽은 인터넷에 열려
            // 있으므로 IP별 rate limit을 건다. 본문은 WebAuthn 응답 하나라 작게 제한한다.
            .at(
                "/passkeys/login/start",
                post(login_start).with(RateLimit::new(20, Duration::from_secs(60))),
            )
            .at(
                "/passkeys/login/finish",
                post(login_finish)
                    .with(SizeLimit::new(PASSKEY_MAX_BYTES))
                    .with(RateLimit::new(20, Duration::from_secs(60))),
            )
            // 가입(계정 생성)은 누구나 부를 수 있어서 더 촘촘하게 제한한다. 계정 총량 상한은 별도.
            .at(
                "/passkeys/signup/start",
                post(signup_start).with(RateLimit::new(10, Duration::from_secs(60 * 60))),
            )
            .at(
                "/passkeys/signup/finish",
                post(signup_finish)
                    .with(SizeLimit::new(PASSKEY_MAX_BYTES))
                    .with(RateLimit::new(5, Duration::from_secs(60 * 60))),
            )
            .at("/passkeys/register/start", post(register_start).with(Auth))
            .at(
                "/passkeys/register/finish",
                post(register_finish)
                    .with(SizeLimit::new(PASSKEY_MAX_BYTES))
                    .with(Auth),
            )
            .at("/passkeys", get(list_passkeys).with(Auth))
            .at("/passkeys/:id", delete(delete_passkey).with(Auth))
            // SizeLimit은 Content-Length 없는 요청(GET)을 411로 거부하므로 PUT에만 건다.
            .at(
                "/gotolib/sync",
                get(get_sync.with(Auth)).put(
                    put_sync
                        .with(SizeLimit::new(MAX_SYNC_BYTES + SYNC_ENVELOPE_BYTES))
                        .with(Auth),
                ),
            )
            // 표지 조회는 로그인 없이 쓴다(표지는 계정과 무관한 기능). 대신 IP별로 제한한다.
            // 이 핸들러는 DB만 보고, 알라딘 호출은 수집 작업 한 곳에서만 일어난다.
            .at(
                "/gotolib/covers",
                get(get_covers)
                    .with(RateLimit::new(60, Duration::from_secs(60)))
                    .with(RateLimit::new(600, Duration::from_secs(60 * 60))),
            )
            .nest(
                IMAGE_PATH_PREFIX,
                StaticFilesEndpoint::new(covers::covers_dir())
                    .with(SetHeader::new().overriding("Cache-Control", "public, max-age=2592000")),
            )
            // 찜 소장 알림: 구독 관리. 키 조회만 로그인 없이, 나머지는 로그인한 계정만.
            .at("/gotolib/push/key", get(push_key).with(RateLimit::new(60, Duration::from_secs(60))))
            .at(
                "/gotolib/push/subscribe",
                post(
                    push_subscribe
                        .with(SizeLimit::new(PUSH_BODY_MAX_BYTES))
                        .with(Auth),
                )
                .with(RateLimit::new(30, Duration::from_secs(60 * 60))),
            )
            .at(
                "/gotolib/push/unsubscribe",
                post(
                    push_unsubscribe
                        .with(SizeLimit::new(PUSH_BODY_MAX_BYTES))
                        .with(Auth),
                )
                .with(RateLimit::new(30, Duration::from_secs(60 * 60))),
            )
            .at(
                "/gotolib/push/test",
                post(push_test.with(Auth)).with(RateLimit::new(5, Duration::from_secs(60 * 60))),
            )
            .at("/me", get(me).with(Auth))
            .at("/me/profile", put(update_my_profile).with(Auth))
            .at(
                "/public/wedding/guests",
                post(create_wedding_guest)
                    .with(SizeLimit::new(RSVP_MAX_BYTES))
                    .with(RateLimit::new(5, Duration::from_secs(60)))
                    .with(RateLimit::new(30, Duration::from_secs(60 * 60))),
            )
            .at("/admin/add-user", post(add_user).with(AdminOnly).with(Auth))
            .nest("/images", StaticFilesEndpoint::new(upload_base_path))
    }

    let app = configure_routes(&state).data(push).data(state).with(
        Cors::new()
            .allow_origin("http://localhost:3001")
            .allow_origin("http://localhost:3000")
            .allow_origin("http://localhost:4000")
            .allow_origin("https://cms.tyange.com")
            .allow_origin("https://blog.tyange.com")
            .allow_origin("https://dashboard.tyange.com")
            .allow_origin("https://lovelog.tyange.com")
            // gotolib은 nginx가 /api/cms/를 같은 출처로 프록시하지만, POST/PUT에는 브라우저가
            // Origin 헤더를 붙이므로 허용 목록에 없으면 거부된다.
            .allow_origin("https://gotolib.tyange.com")
            .allow_origin("http://localhost:5199")
            .allow_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"])
            .allow_credentials(true)
            .allow_headers(vec!["authorization", "content-type", "accept"]),
    );

    Server::new(TcpListener::bind("0.0.0.0:8080"))
        .run(app)
        .await
}
