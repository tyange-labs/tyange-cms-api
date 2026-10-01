//! 패스키(WebAuthn) 서비스: 설정과 진행 중인 의식(ceremony) 상태 보관.
//!
//! - `PASSKEY_RP_ID`가 비어 있으면 기능 전체가 꺼진다(blog_redeploy와 같은 방식). 엔드포인트는
//!   503을 돌려준다.
//! - 등록·로그인 `start`에서 만든 상태는 `finish`가 올 때까지 메모리에 둔다. 단일 프로세스
//!   서버라 DB에 직렬화할 필요가 없고(`danger-allow-state-serialisation`도 불필요), 5분이 지나면
//!   버린다. 한 번 꺼낸 상태는 다시 쓸 수 없다(재전송 방지).

use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use uuid::Uuid;
use webauthn_rs::prelude::{
    DiscoverableAuthentication, PasskeyRegistration, Url, Webauthn, WebauthnBuilder,
};

const CHALLENGE_TTL: Duration = Duration::from_secs(5 * 60);
/// 인증 없이 만들 수 있는 `login/start`가 메모리를 무한히 먹지 못하게 하는 상한.
const MAX_PENDING: usize = 1000;
const DEFAULT_RP_NAME: &str = "tyange";

pub enum Pending {
    Register {
        user_id: String,
        user_handle: Uuid,
        state: PasskeyRegistration,
    },
    Login(DiscoverableAuthentication),
}

#[derive(Clone)]
pub struct PasskeyService {
    webauthn: Option<Arc<Webauthn>>,
    pending: Arc<Mutex<HashMap<String, (Instant, Pending)>>>,
}

impl PasskeyService {
    /// `PASSKEY_RP_ID`(예: tyange.com), `PASSKEY_RP_ORIGIN`(쉼표로 여러 개, 첫 번째가 기본 origin,
    /// 예: https://gotolib.tyange.com), 선택 `PASSKEY_RP_NAME`.
    pub fn from_env() -> Self {
        let Some(rp_id) = env::var("PASSKEY_RP_ID").ok().filter(|v| !v.trim().is_empty()) else {
            println!("패스키 비활성화: PASSKEY_RP_ID가 설정되지 않았습니다.");
            return Self::disabled();
        };
        let origins = env::var("PASSKEY_RP_ORIGIN").unwrap_or_default();
        let rp_name = env::var("PASSKEY_RP_NAME").unwrap_or_else(|_| DEFAULT_RP_NAME.to_string());

        match Self::build(rp_id.trim(), &origins, &rp_name) {
            Ok(service) => service,
            Err(reason) => {
                eprintln!("패스키 비활성화: {}", reason);
                Self::disabled()
            }
        }
    }

    pub fn build(rp_id: &str, origins: &str, rp_name: &str) -> Result<Self, String> {
        let mut urls = origins
            .split(',')
            .map(str::trim)
            .filter(|origin| !origin.is_empty())
            .map(|origin| Url::parse(origin).map_err(|e| format!("잘못된 origin {origin}: {e}")))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter();
        let primary = urls
            .next()
            .ok_or_else(|| "PASSKEY_RP_ORIGIN이 비어 있습니다.".to_string())?;

        let mut builder = WebauthnBuilder::new(rp_id, &primary)
            .map_err(|e| format!("WebAuthn 설정 오류: {e:?}"))?
            .rp_name(rp_name);
        for extra in urls {
            builder = builder.append_allowed_origin(&extra);
        }
        let webauthn = builder
            .build()
            .map_err(|e| format!("WebAuthn 초기화 오류: {e:?}"))?;

        Ok(Self {
            webauthn: Some(Arc::new(webauthn)),
            pending: Arc::default(),
        })
    }

    pub fn disabled() -> Self {
        Self {
            webauthn: None,
            pending: Arc::default(),
        }
    }

    pub fn webauthn(&self) -> Option<&Webauthn> {
        self.webauthn.as_deref()
    }

    /// 진행 중인 의식을 보관하고 클라이언트가 돌려줄 id를 만든다. 가득 차 있으면 None.
    pub fn store(&self, pending: Pending) -> Option<String> {
        let mut map = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        map.retain(|_, (created, _)| created.elapsed() < CHALLENGE_TTL);
        if map.len() >= MAX_PENDING {
            return None;
        }
        let id = Uuid::new_v4().to_string();
        map.insert(id.clone(), (Instant::now(), pending));
        Some(id)
    }

    /// 한 번만 꺼낼 수 있다. 만료됐거나 없으면 None.
    pub fn take(&self, id: &str) -> Option<Pending> {
        let mut map = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let (created, pending) = map.remove(id)?;
        (created.elapsed() < CHALLENGE_TTL).then_some(pending)
    }
}
