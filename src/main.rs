mod blog_redeploy;
mod db;
mod middlewares;
mod models;
mod routes;
mod services;
mod utils;
mod web_push;

use dotenv::dotenv;
use middlewares::admin_middleware::AdminOnly;
use middlewares::auth_middleware::Auth;
use std::{env, fs, sync::Arc};

use crate::routes::delete_portfolio::delete_portfolio;
use crate::routes::delete_post::delete_post;
use crate::routes::delete_push_subscription::delete_push_subscription;
use crate::routes::get_all_posts::get_all_posts;
use crate::routes::get_count_with_tags::get_count_with_tags;
use crate::routes::get_portfolio::get_portfolio;
use crate::routes::get_posts_with_tags::get_posts_with_tags;
use crate::routes::get_push_public_key::get_push_public_key;
use crate::routes::get_push_subscriptions::get_push_subscriptions;
use crate::routes::get_tags_with_category::get_tags_with_category;
use crate::routes::me::me;
use crate::routes::signup::signup;
use crate::routes::update_my_profile::update_my_profile;
use crate::routes::update_portfolio::update_portfolio;
use crate::routes::update_portfolio_section::update_portfolio_section;
use crate::routes::update_post::update_post;
use crate::routes::upload_image::upload_image;
use crate::routes::upload_image_public::upload_image_public;
use crate::routes::upsert_push_subscription::upsert_push_subscription;
use crate::services::image_storage::purge_stale_pending_images;
use middlewares::rate_limit::RateLimit;
use std::time::Duration;
use crate::{models::AppState, routes::add_user::add_user};
use db::init_db;
use poem::{
    delete,
    endpoint::StaticFilesEndpoint,
    get, handler,
    http::StatusCode,
    listener::TcpListener,
    middleware::{Cors, SizeLimit},
    options, post, put, EndpointExt, Response, Route, Server,
};
use routes::{
    get_post::get_post, get_posts::get_posts, login::login, login_google::login_google,
    upload_post::upload_post,
};
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

    let state = Arc::new(AppState::new(db));

    // 24시간 넘게 pending 상태인 익명 업로드를 1시간 주기로 정리
    let cleanup_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(3600));
        loop {
            interval.tick().await;
            match purge_stale_pending_images(&cleanup_state.db).await {
                Ok(removed) if removed > 0 => {
                    println!("pending 이미지 {}건 정리 완료", removed);
                }
                Ok(_) => {}
                Err(e) => eprintln!("pending 이미지 정리 실패: {}", e),
            }
        }
    });

    fn configure_routes() -> Route {
        let upload_base_path = env::var("UPLOAD_PATH").unwrap_or(String::from(".uploads/images"));
        let upload_max_bytes = upload_size_limit();

        Route::new()
            .at("/health", get(return_str))
            .at("/health-check", get(return_str))
            .at("/posts", get(get_posts))
            .at("/posts/search-with-tags", get(get_posts_with_tags))
            .at("/post/:post_id", get(get_post))
            .at("/post/upload", post(upload_post).with(Auth))
            .at("/post/update/:post_id", put(update_post).with(Auth))
            .at("/post/delete/:post_id", delete(delete_post).with(Auth))
            .at("/tags", get(get_count_with_tags))
            .at("/tags-with-category", get(get_tags_with_category))
            .at(
                "/portfolio",
                get(get_portfolio)
                    .put(update_portfolio.with(AdminOnly).with(Auth))
                    .delete(delete_portfolio.with(AdminOnly).with(Auth)),
            )
            .at(
                "/portfolio/update",
                put(update_portfolio).with(AdminOnly).with(Auth),
            )
            .at(
                "/portfolio/sections/:section_key",
                put(update_portfolio_section)
                    .with(AdminOnly)
                    .with(Auth),
            )
            .at(
                "/upload-image",
                post(upload_image)
                    .with(SizeLimit::new(upload_max_bytes))
                    .with(Auth),
            )
            .at(
                "/images/upload",
                post(upload_image)
                    .with(SizeLimit::new(upload_max_bytes))
                    .with(Auth),
            )
            .at(
                // 익명 업로드: rate limit이 가장 바깥에서 모든 시도를 카운트
                "/public/images/upload",
                post(upload_image_public)
                    .with(SizeLimit::new(4 * 1024 * 1024))
                    .with(RateLimit::new(30, Duration::from_secs(3600)))
                    .with(RateLimit::new(5, Duration::from_secs(60))),
            )
            .at("/login", post(login))
            .at("/login/google", post(login_google))
            .at("/signup", post(signup))
            .at("/me", get(me).with(Auth))
            .at("/me/profile", put(update_my_profile).with(Auth))
            .at(
                "/push/subscriptions",
                get(get_push_subscriptions)
                    .post(upsert_push_subscription)
                    .delete(delete_push_subscription)
                    .with(Auth),
            )
            .at("/push/public-key", get(get_push_public_key))
            .at("/admin/add-user", post(add_user).with(AdminOnly).with(Auth))
            .at(
                "/admin/posts",
                get(get_all_posts).with(AdminOnly).with(Auth),
            )
            .nest("/images", StaticFilesEndpoint::new(upload_base_path))
            .at("/*path", options(options_handler))
    }

    let app = configure_routes().data(state).with(
        Cors::new()
            .allow_origin("http://localhost:3001")
            .allow_origin("http://localhost:3000")
            .allow_origin("http://localhost:4000")
            .allow_origin("https://cms.tyange.com")
            .allow_origin("https://blog.tyange.com")
            .allow_origin("https://dashboard.tyange.com")
            .allow_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"])
            .allow_credentials(true)
            .allow_headers(vec!["authorization", "content-type", "accept"]),
    );

    Server::new(TcpListener::bind("0.0.0.0:8080"))
        .run(app)
        .await
}
