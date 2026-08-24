use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Pool, Sqlite};

use crate::blog_redeploy::BlogRedeployService;

pub struct AppState {
    pub db: Pool<Sqlite>,
    pub blog_redeploy: BlogRedeployService,
}

impl AppState {
    pub fn new(db: Pool<Sqlite>) -> Self {
        Self {
            db,
            blog_redeploy: BlogRedeployService::from_env(),
        }
    }

    #[cfg(test)]
    pub fn new_with_blog_redeploy(db: Pool<Sqlite>, blog_redeploy: BlogRedeployService) -> Self {
        Self { db, blog_redeploy }
    }
}

#[derive(Debug, Serialize)]
pub struct CustomResponse<T> {
    pub status: bool,
    pub data: Option<T>,
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Post {
    pub post_id: String,
    pub title: String,
    pub description: String,
    pub published_at: String,
    pub tags: Vec<TagWithCategory>,
    pub content: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct PostItem {
    pub post_id: String,
    pub title: String,
    pub description: String,
    pub published_at: String,
    pub tags: Vec<TagWithCategory>,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct UploadPostResponse {
    pub post_id: String,
}

#[derive(Debug, Deserialize)]
pub struct Tag {
    pub tag: String,
    pub category: String,
}

#[derive(Debug, Deserialize)]
pub struct UploadPostRequest {
    pub title: String,
    pub description: String,
    pub published_at: String,
    pub tags: Vec<Tag>,
    pub content: String,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LoginRequest {
    pub user_id: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct GoogleLoginRequest {
    pub id_token: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub user_role: String,
}

#[derive(Debug, Serialize, FromRow)]
pub struct MeResponse {
    pub user_id: String,
    pub user_role: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub bio: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateMyProfileRequest {
    pub display_name: String,
    pub avatar_url: String,
    pub bio: String,
}

#[derive(Debug, FromRow)]
pub struct PostResponseDb {
    pub post_id: String,
    pub title: String,
    pub description: String,
    pub published_at: String,
    pub content: String,
    pub status: String,
    pub tags: String,
}

#[derive(Debug, Serialize)]
pub struct PostsResponse {
    pub posts: Vec<PostItem>,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePostRequest {
    pub title: String,
    pub description: String,
    pub published_at: String,
    pub tags: Vec<Tag>,
    pub content: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct DeletePostResponse {
    pub post_id: String,
}

#[derive(Debug, Serialize)]
pub struct UploadImageResponse {
    pub image_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UploadImageQueryParmas {
    pub post_id: Option<String>,
    pub image_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AddUserRequest {
    pub user_id: String,
    pub password: String,
    pub user_role: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SignupRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PortfolioLink {
    pub label: String,
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PortfolioIdentity {
    pub name: String,
    pub email: String,
    pub github_url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PortfolioProject {
    pub slug: String,
    pub title: String,
    pub period: String,
    pub summary: String,
    #[serde(default)]
    pub stack: Vec<String>,
    #[serde(default)]
    pub highlights: Vec<String>,
    #[serde(default)]
    pub links: Vec<PortfolioLink>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PortfolioCareerItem {
    pub title: String,
    #[serde(default)]
    pub period: Option<String>,
    #[serde(default)]
    pub bullets: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PortfolioCareerCompany {
    pub company: String,
    pub period: String,
    pub employment_type: String,
    pub role: String,
    pub position: String,
    #[serde(default)]
    pub items: Vec<PortfolioCareerItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PortfolioCareerSection {
    pub summary_label: String,
    pub summary_value: String,
    #[serde(default)]
    pub companies: Vec<PortfolioCareerCompany>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PortfolioTechStackItem {
    pub name: String,
    pub icon_url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PortfolioIntroSection {
    pub content: String,
    #[serde(default)]
    pub tech_stack: Vec<PortfolioTechStackItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PortfolioMeta {
    pub slug: String,
    pub version: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PortfolioDocument {
    pub slug: String,
    pub version: i32,
    pub identity: PortfolioIdentity,
    #[serde(default)]
    pub featured_projects: Vec<PortfolioProject>,
    #[serde(default)]
    pub career: Option<PortfolioCareerSection>,
    #[serde(default)]
    pub intro: Option<PortfolioIntroSection>,
}

#[derive(Debug, FromRow)]
pub struct PortfolioMasterRow {
    pub portfolio_id: i32,
    pub slug: String,
    pub created_at: String,
}

#[derive(Debug, FromRow)]
pub struct PortfolioSectionRow {
    pub section_id: i32,
    pub portfolio_id: i32,
    pub section_key: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct PortfolioResponse {
    pub portfolio_id: i32,
    pub slug: String,
    pub content: PortfolioDocument,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdatePortfolioRequest {
    pub content: PortfolioDocument,
}

#[derive(Deserialize)]
pub struct SearchPostsWithTag {
    pub include: Option<String>,
    pub exclude: Option<String>,
}

#[derive(Deserialize)]
pub struct SearchParamsWithTags {
    pub category: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CountWithTag {
    pub tag: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct TagsWithCategory {
    pub category: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TagWithCategory {
    pub tag: String,
    pub category: String,
}

#[derive(Deserialize)]
pub struct SearchPostsWithWriter {
    pub writer_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PushSubscriptionKeysRequest {
    pub p256dh: String,
    pub auth: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpsertPushSubscriptionRequest {
    pub endpoint: String,
    pub keys: PushSubscriptionKeysRequest,
}

#[derive(Debug, Deserialize)]
pub struct DeletePushSubscriptionRequest {
    pub endpoint: String,
}

#[derive(Debug, Serialize)]
pub struct WebPushSubscriptionResponse {
    pub push_subscription_id: i64,
    pub endpoint: String,
    pub user_agent: Option<String>,
    pub created_at: String,
    pub last_success_at: Option<String>,
    pub last_failure_at: Option<String>,
    pub failure_count: i64,
    pub revoked_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WebPushSubscriptionListResponse {
    pub subscriptions: Vec<WebPushSubscriptionResponse>,
}

#[derive(Debug, Serialize)]
pub struct PublicPushKeyResponse {
    pub public_key: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateWeddingGuestRequest {
    pub name: String,
    #[serde(default)]
    pub companions: i64,
}

#[derive(Debug, Serialize)]
pub struct CreateWeddingGuestResponse {
    pub guest_id: i64,
}

#[derive(Debug, Serialize, FromRow)]
pub struct WeddingGuest {
    pub guest_id: i64,
    pub name: String,
    pub companions: i64,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct WeddingGuestListResponse {
    pub guests: Vec<WeddingGuest>,
    pub total_guests: i64,
    pub total_attendees: i64,
}
