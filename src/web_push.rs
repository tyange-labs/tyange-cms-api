use std::env;

use poem::http::StatusCode;
use sqlx::{query, query_as, FromRow, SqlitePool};
use web_push::{
    ContentEncoding, IsahcWebPushClient, SubscriptionInfo, VapidSignatureBuilder, WebPushClient,
    WebPushMessageBuilder,
};

use crate::models::WebPushSubscriptionResponse;

#[derive(Debug)]
pub struct AppError {
    pub status: StatusCode,
    pub message: String,
}

impl AppError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }

    fn service_unavailable(message: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, message)
    }
}

#[derive(Debug, FromRow)]
pub struct PushTargetRow {
    // 발송 성공/실패 상태를 web_push_subscriptions에 기록할 때 호출자가 사용한다.
    #[allow(dead_code)]
    pub push_subscription_id: i64,
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
}

#[derive(Debug)]
pub struct PushConfig {
    private_key: String,
    subject: String,
}

impl PushConfig {
    // RSS 폴링 워커 제거 후 현재 발송 경로가 없어 호출자가 없지만,
    // 새 알림 트리거가 생기면 send_push_message와 함께 재사용한다.
    #[allow(dead_code)]
    pub fn from_env() -> Option<Self> {
        let _public_key = env::var("VAPID_PUBLIC_KEY").ok()?;
        let private_key = env::var("VAPID_PRIVATE_KEY").ok()?;
        let subject = env::var("VAPID_SUBJECT").ok()?;

        Some(Self {
            private_key,
            subject,
        })
    }
}

pub fn push_public_key() -> Result<String, AppError> {
    env::var("VAPID_PUBLIC_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            AppError::service_unavailable(
                "푸시 알림이 아직 설정되지 않았습니다. VAPID_PUBLIC_KEY 환경변수를 확인해주세요.",
            )
        })
}

pub async fn list_push_subscriptions(
    db: &SqlitePool,
    user_id: &str,
) -> Result<Vec<WebPushSubscriptionResponse>, AppError> {
    let rows = query_as::<
        _,
        (
            i64,
            String,
            Option<String>,
            String,
            Option<String>,
            Option<String>,
            i64,
            Option<String>,
        ),
    >(
        r#"
        SELECT
            push_subscription_id,
            endpoint,
            user_agent,
            created_at,
            last_success_at,
            last_failure_at,
            failure_count,
            revoked_at
        FROM web_push_subscriptions
        WHERE user_id = ?
        ORDER BY created_at DESC, push_subscription_id DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(db)
    .await
    .map_err(|err| AppError::internal(format!("Push 구독 목록 조회 실패: {}", err)))?;

    Ok(rows
        .into_iter()
        .map(
            |(
                push_subscription_id,
                endpoint,
                user_agent,
                created_at,
                last_success_at,
                last_failure_at,
                failure_count,
                revoked_at,
            )| WebPushSubscriptionResponse {
                push_subscription_id,
                endpoint,
                user_agent,
                created_at,
                last_success_at,
                last_failure_at,
                failure_count,
                revoked_at,
            },
        )
        .collect())
}

pub async fn upsert_push_subscription(
    db: &SqlitePool,
    user_id: &str,
    endpoint: &str,
    p256dh: &str,
    auth: &str,
    user_agent: Option<&str>,
) -> Result<WebPushSubscriptionResponse, AppError> {
    if endpoint.trim().is_empty() || p256dh.trim().is_empty() || auth.trim().is_empty() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "endpoint와 keys 정보는 비어 있을 수 없습니다.",
        ));
    }

    query(
        r#"
        INSERT INTO web_push_subscriptions (user_id, endpoint, p256dh, auth, user_agent, revoked_at, failure_count)
        VALUES (?, ?, ?, ?, ?, NULL, 0)
        ON CONFLICT(endpoint) DO UPDATE SET
            user_id = excluded.user_id,
            p256dh = excluded.p256dh,
            auth = excluded.auth,
            user_agent = excluded.user_agent,
            revoked_at = NULL,
            failure_count = 0
        "#,
    )
    .bind(user_id)
    .bind(endpoint.trim())
    .bind(p256dh.trim())
    .bind(auth.trim())
    .bind(user_agent.map(str::trim))
    .execute(db)
    .await
    .map_err(|err| AppError::internal(format!("Push 구독 저장 실패: {}", err)))?;

    let row = query_as::<
        _,
        (
            i64,
            String,
            Option<String>,
            String,
            Option<String>,
            Option<String>,
            i64,
            Option<String>,
        ),
    >(
        r#"
        SELECT
            push_subscription_id,
            endpoint,
            user_agent,
            created_at,
            last_success_at,
            last_failure_at,
            failure_count,
            revoked_at
        FROM web_push_subscriptions
        WHERE endpoint = ?
        LIMIT 1
        "#,
    )
    .bind(endpoint.trim())
    .fetch_one(db)
    .await
    .map_err(|err| AppError::internal(format!("저장된 Push 구독 조회 실패: {}", err)))?;

    Ok(WebPushSubscriptionResponse {
        push_subscription_id: row.0,
        endpoint: row.1,
        user_agent: row.2,
        created_at: row.3,
        last_success_at: row.4,
        last_failure_at: row.5,
        failure_count: row.6,
        revoked_at: row.7,
    })
}

pub async fn revoke_push_subscription(
    db: &SqlitePool,
    user_id: &str,
    endpoint: &str,
) -> Result<bool, AppError> {
    let result = query(
        r#"
        UPDATE web_push_subscriptions
        SET revoked_at = COALESCE(revoked_at, CURRENT_TIMESTAMP)
        WHERE user_id = ? AND endpoint = ?
        "#,
    )
    .bind(user_id)
    .bind(endpoint.trim())
    .execute(db)
    .await
    .map_err(|err| AppError::internal(format!("Push 구독 해지 실패: {}", err)))?;

    Ok(result.rows_affected() > 0)
}

// RSS 폴링 워커 제거 후 현재 발송 경로가 없어 호출자가 없지만,
// 새 알림 트리거가 생기면 그대로 재사용할 수 있도록 유지한다.
#[allow(dead_code)]
pub async fn send_push_message(
    target: &PushTargetRow,
    payload: &str,
    push_config: &PushConfig,
) -> Result<(), AppError> {
    let subscription_info = SubscriptionInfo::new(
        target.endpoint.clone(),
        target.p256dh.clone(),
        target.auth.clone(),
    );
    let mut builder = WebPushMessageBuilder::new(&subscription_info);
    builder.set_payload(ContentEncoding::Aes128Gcm, payload.as_bytes());

    let mut signature_builder = VapidSignatureBuilder::from_base64(
        &push_config.private_key,
        base64::URL_SAFE_NO_PAD,
        &subscription_info,
    )
    .map_err(|err| AppError::internal(format!("VAPID 서명 생성 실패: {}", err)))?;
    signature_builder.add_claim("sub", push_config.subject.as_str());

    let signature = signature_builder
        .build()
        .map_err(|err| AppError::internal(format!("VAPID 서명 build 실패: {}", err)))?;
    builder.set_vapid_signature(signature);

    let client = IsahcWebPushClient::new()
        .map_err(|err| AppError::internal(format!("Push client 초기화 실패: {}", err)))?;
    let message = builder
        .build()
        .map_err(|err| AppError::internal(format!("Push 메시지 build 실패: {}", err)))?;

    client
        .send(message)
        .await
        .map_err(|err| AppError::internal(format!("Push 전송 실패: {}", err)))?;

    Ok(())
}

#[allow(dead_code)]
pub fn is_gone_push_error(message: &str) -> bool {
    message.contains("404") || message.contains("410")
}
