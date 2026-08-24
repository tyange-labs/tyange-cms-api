use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use poem::{http::StatusCode, Endpoint, Error, Middleware, Request};

/// 요청자의 IP를 문자열 키로 얻는다.
/// 리버스 프록시 뒤에서 운영할 때는 TRUST_PROXY_FORWARDED_FOR=true로 설정해야
/// X-Forwarded-For의 첫 IP를 사용한다. 프록시 없이 이 값을 켜면 헤더 위조로
/// rate limit을 우회할 수 있으므로 기본값은 꺼짐.
pub fn client_ip(req: &Request) -> String {
    let trust_proxy = env::var("TRUST_PROXY_FORWARDED_FOR")
        .map(|value| value == "true")
        .unwrap_or(false);

    if trust_proxy {
        if let Some(forwarded) = req
            .headers()
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
        {
            if let Some(first) = forwarded.split(',').next() {
                let ip = first.trim();
                if !ip.is_empty() {
                    return ip.to_string();
                }
            }
        }
    }

    req.remote_addr()
        .as_socket_addr()
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|| req.remote_addr().to_string())
}

/// IP별 슬라이딩 윈도우 rate limit.
/// 인스턴스마다 독립된 카운터를 가지므로 분당·시간당 제한을 체이닝할 수 있다.
pub struct RateLimit {
    max_requests: usize,
    window: Duration,
}

impl RateLimit {
    pub fn new(max_requests: usize, window: Duration) -> Self {
        Self {
            max_requests,
            window,
        }
    }
}

impl<E: Endpoint> Middleware<E> for RateLimit {
    type Output = RateLimitImpl<E>;

    fn transform(&self, ep: E) -> Self::Output {
        RateLimitImpl {
            ep,
            max_requests: self.max_requests,
            window: self.window,
            hits: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

pub struct RateLimitImpl<E> {
    ep: E,
    max_requests: usize,
    window: Duration,
    hits: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
}

impl<E: Endpoint> Endpoint for RateLimitImpl<E> {
    type Output = E::Output;

    async fn call(&self, req: Request) -> Result<Self::Output, Error> {
        let key = client_ip(&req);
        let now = Instant::now();

        {
            let mut hits = self.hits.lock().unwrap();

            // 빈 엔트리가 쌓이지 않도록 주기적으로 정리
            if hits.len() > 10_000 {
                let window = self.window;
                hits.retain(|_, timestamps| {
                    timestamps.iter().any(|t| now.duration_since(*t) < window)
                });
            }

            let timestamps = hits.entry(key).or_default();
            timestamps.retain(|t| now.duration_since(*t) < self.window);

            if timestamps.len() >= self.max_requests {
                return Err(Error::from_string(
                    "요청이 너무 많습니다. 잠시 후 다시 시도해주세요.",
                    StatusCode::TOO_MANY_REQUESTS,
                ));
            }

            timestamps.push(now);
        }

        self.ep.call(req).await
    }
}
