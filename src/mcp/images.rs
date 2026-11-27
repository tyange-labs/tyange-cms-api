//! 본문 속 외부 이미지를 이 서버에 옮겨 저장한다. Craft 문서의 이미지 주소처럼 언제 만료될지 모르는
//! 주소를 글에 그대로 두지 않으려고, `![…](https://…)`를 내려받아 `store_image_from_reader`로 저장하고
//! 주소를 `<이미지 기본 주소>/images/…`로 바꾼다(지운 Craft 동기화와 같은 방식).
//!
//! 서버가 남이 준 주소로 요청을 보내는 것이라 내부망으로 새지 않게(SSRF) 막는다: https만, 사설·루프백 등으로
//! 풀리는 호스트는 거부하고, 검사한 IP로 연결을 고정한다(DNS rebinding 방지). 리다이렉트는 3번까지 https이고
//! IP 직접 주소·localhost가 아닐 때만 따라간다. 이미지 하나가 실패해도 글 저장은 계속하고 경고로 알린다.

use std::collections::HashMap;
use std::io::Cursor;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use sqlx::SqlitePool;

use crate::services::image_storage::{
    store_image_from_reader, ImageFormat, ImageRecord, ImageUploadPolicy,
};

const DEFAULT_IMAGE_BASE_URL: &str = "https://tyange.com/api/cms";
const MAX_IMAGES_PER_POST: usize = 30;

/// 인증된 이미지 업로드와 같은 값.
const IMAGE_POLICY: ImageUploadPolicy = ImageUploadPolicy {
    max_bytes: 10 * 1024 * 1024,
    allowed_formats: &[
        ImageFormat::Png,
        ImageFormat::Jpeg,
        ImageFormat::Gif,
        ImageFormat::Webp,
    ],
};

#[derive(Clone, Debug)]
pub struct ImageRehoster {
    upload_dir: PathBuf,
    /// 저장된 `/images/…` 앞에 붙는 공개 주소. 이 주소로 시작하는 이미지는 이미 우리 것이라 건너뛴다.
    base_url: String,
    /// 테스트에서 로컬 가짜 서버(http, 127.0.0.1)를 쓰려고만 켠다.
    allow_insecure_loopback: bool,
}

#[derive(Debug, Default, PartialEq)]
pub struct RehostReport {
    pub rehosted: usize,
    pub warnings: Vec<String>,
}

impl ImageRehoster {
    pub fn from_env() -> Self {
        let base_url = std::env::var("MCP_IMAGE_BASE_URL")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_IMAGE_BASE_URL.to_string());
        let upload_dir =
            std::env::var("UPLOAD_PATH").unwrap_or_else(|_| ".uploads/images".to_string());
        Self::new(PathBuf::from(upload_dir), &base_url)
    }

    pub fn new(upload_dir: PathBuf, base_url: &str) -> Self {
        Self {
            upload_dir,
            base_url: base_url.trim_end_matches('/').to_string(),
            allow_insecure_loopback: false,
        }
    }

    #[cfg(test)]
    pub fn allowing_insecure_loopback(mut self) -> Self {
        self.allow_insecure_loopback = true;
        self
    }

    /// 마크다운의 외부 이미지를 옮겨 저장하고 주소를 바꾼 본문을 돌려준다.
    pub async fn rehost(&self, db: &SqlitePool, markdown: &str) -> (String, RehostReport) {
        let mut report = RehostReport::default();
        let mut done: HashMap<String, Option<String>> = HashMap::new();
        let mut out = String::with_capacity(markdown.len());
        let mut rest = markdown;

        while let Some((before, url, after)) = next_image_url(rest) {
            out.push_str(before);
            let replacement = if url.starts_with(&self.base_url) || !url.contains("://") {
                None
            } else if let Some(cached) = done.get(url) {
                cached.clone()
            } else if done.len() >= MAX_IMAGES_PER_POST {
                report.warnings.push(format!(
                    "이미지가 {MAX_IMAGES_PER_POST}개를 넘어 나머지는 옮기지 않았습니다."
                ));
                done.insert(url.to_string(), None);
                None
            } else {
                let stored = match self.fetch_and_store(db, url).await {
                    Ok(web_path) => {
                        report.rehosted += 1;
                        Some(format!("{}{}", self.base_url, web_path))
                    }
                    Err(reason) => {
                        report
                            .warnings
                            .push(format!("{}: {reason}", short_url(url)));
                        None
                    }
                };
                done.insert(url.to_string(), stored.clone());
                stored
            };
            out.push_str(replacement.as_deref().unwrap_or(url));
            rest = after;
        }
        out.push_str(rest);
        (out, report)
    }

    async fn fetch_and_store(&self, db: &SqlitePool, url: &str) -> Result<String, String> {
        let parsed = url::Url::parse(url).map_err(|_| "주소가 올바르지 않습니다.".to_string())?;
        let insecure_ok = self.allow_insecure_loopback && parsed.scheme() == "http";
        if parsed.scheme() != "https" && !insecure_ok {
            return Err("https 주소만 옮길 수 있습니다.".to_string());
        }
        let host = parsed
            .host_str()
            .ok_or_else(|| "호스트가 없습니다.".to_string())?
            .to_string();
        let port = parsed.port_or_known_default().unwrap_or(443);
        let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
            .await
            .map_err(|_| "호스트를 찾지 못했습니다.".to_string())?
            .collect();
        // 주소 중 하나라도 내부망이면 거부한다(공인 주소와 섞어 두는 우회도 막는다).
        if !insecure_ok && addrs.iter().any(|a| !is_public_ip(a.ip())) {
            return Err("내부망 주소로는 요청하지 않습니다.".to_string());
        }
        let addr = *addrs
            .first()
            .ok_or_else(|| "호스트를 찾지 못했습니다.".to_string())?;

        let client = reqwest::Client::builder()
            .resolve(&host, addr)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                let next = attempt.url();
                let host_ok = match next.host() {
                    Some(url::Host::Domain(domain)) => !domain.eq_ignore_ascii_case("localhost"),
                    _ => false,
                };
                if attempt.previous().len() >= 3 || next.scheme() != "https" || !host_ok {
                    attempt.stop()
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|_| "HTTP 클라이언트를 만들지 못했습니다.".to_string())?;

        let response = client
            .get(parsed)
            .send()
            .await
            .map_err(|e| format!("내려받기 실패: {}", e.without_url()))?;
        if !response.status().is_success() {
            return Err(format!(
                "이미지 서버가 {}로 응답했습니다.",
                response.status()
            ));
        }
        if response
            .content_length()
            .is_some_and(|len| len > IMAGE_POLICY.max_bytes as u64)
        {
            return Err("이미지가 너무 큽니다(최대 10MB).".to_string());
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("이미지를 읽지 못함: {}", e.without_url()))?;

        let stored = store_image_from_reader(
            Some("mcp".to_string()),
            Cursor::new(bytes.to_vec()),
            &IMAGE_POLICY,
            db,
            ImageRecord {
                post_id: None,
                image_type: "in_post".to_string(),
                uploader_ip: None,
                status: "confirmed",
            },
            &self.upload_dir,
            "/images",
        )
        .await
        .map_err(|e| format!("이미지로 저장하지 못함: {e:?}"))?;
        Ok(stored.web_path)
    }
}

/// `![alt](url "title")`에서 url을 찾는다. (앞부분, url, 뒷부분). 제목·꺾쇠(`<url>`)도 처리한다.
fn next_image_url(text: &str) -> Option<(&str, &str, &str)> {
    let mut from = 0;
    while let Some(rel) = text[from..].find("![") {
        let start = from + rel;
        let Some(close_rel) = text[start..].find("](") else {
            return None;
        };
        let url_start = start + close_rel + 2;
        let mut s = url_start;
        let angled = text[s..].starts_with('<');
        if angled {
            s += 1;
        }
        let end = text[s..]
            .find(|c: char| {
                if angled {
                    c == '>'
                } else {
                    c == ')' || c.is_whitespace()
                }
            })
            .map(|i| s + i);
        match end {
            Some(e) if e > s => return Some((&text[..s], &text[s..e], &text[e..])),
            _ => from = url_start,
        }
    }
    None
}

fn short_url(url: &str) -> String {
    let base = url.split(['?', '#']).next().unwrap_or(url);
    base.chars().take(80).collect()
}

/// 공인 IP인가. 사설·루프백·링크 로컬·CGNAT·멀티캐스트·문서용 등은 아니다.
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
                || o[0] == 0
                || (o[0] == 100 && (64..=127).contains(&o[1]))
                || (o[0] == 192 && o[1] == 0 && o[2] == 0)
                || (o[0] == 198 && (18..=19).contains(&o[1]))
                || o[0] >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(v4));
            }
            let s = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
                || s[0] == 0x2001 && s[1] == 0x0db8)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_image_urls_with_titles_and_angle_brackets() {
        let (before, url, after) =
            next_image_url("앞 ![고양이](https://a.example/c.png \"제목\") 뒤").unwrap();
        assert_eq!(before, "앞 ![고양이](");
        assert_eq!(url, "https://a.example/c.png");
        assert_eq!(after, " \"제목\") 뒤");

        let (_, url, _) = next_image_url("![x](<https://a.example/a b.png>)").unwrap();
        assert_eq!(url, "https://a.example/a b.png");

        assert!(next_image_url("[링크](https://a.example) 그림 없음").is_none());
        assert!(next_image_url("![빈]()").is_none());
    }

    #[test]
    fn private_and_special_addresses_are_not_public() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.0.1",
            "169.254.169.254",
            "100.96.219.103",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!is_public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "52.78.251.17", "2606:4700:4700::1111"] {
            assert!(is_public_ip(ip.parse().unwrap()), "{ip}");
        }
    }
}
