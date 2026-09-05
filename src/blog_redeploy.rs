use std::{env, sync::Arc};

use reqwest::{
    header::{AUTHORIZATION, USER_AGENT},
    Client, StatusCode,
};
use serde::Serialize;

// GitHub의 repository_dispatch(커스텀 이벤트 + 임의 워크플로 리스닝)에 대응하는 기능이
// Forgejo(v16.0.3 기준)엔 없다. 대신 "workflow_dispatch 이벤트를 API로 발생시키는" 엔드포인트를
// 쓴다 — 대상 워크플로 파일명을 URL에 직접 명시해야 하고, GitHub처럼 event_type으로
// 여러 워크플로에 브로드캐스트하는 개념이 없다.
const DEFAULT_API_BASE: &str = "https://git.tyange.com/api/v1";
const DEFAULT_REPO: &str = "tyange-labs/tyange-blog";
const DEFAULT_WORKFLOW_FILENAME: &str = "deploy.yml";
const DEFAULT_REF: &str = "master";
const SOURCE_NAME: &str = "tyange-cms-api";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlogContentEvent {
    Publish,
    Update,
    Delete,
}

impl BlogContentEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Publish => "publish",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlogVisibility {
    Visible,
    Hidden,
}

impl BlogVisibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Visible => "visible",
            Self::Hidden => "hidden",
        }
    }
}

#[derive(Clone)]
pub struct BlogRedeployService {
    client: Client,
    mode: BlogRedeployMode,
}

#[derive(Clone)]
enum BlogRedeployMode {
    Disabled {
        reason: Arc<str>,
    },
    Forgejo {
        config: Arc<BlogRedeployConfig>,
    },
    #[cfg(test)]
    Mock {
        handle: Arc<MockBlogRedeployHandle>,
    },
}

#[derive(Clone, Debug)]
struct BlogRedeployConfig {
    api_base: String,
    repo: String,
    workflow_filename: String,
    git_ref: String,
    token: String,
}

#[derive(Debug, Serialize)]
struct WorkflowDispatchRequest<'a> {
    #[serde(rename = "ref")]
    git_ref: &'a str,
    inputs: WorkflowDispatchInputs<'a>,
}

#[derive(Debug, Serialize)]
struct WorkflowDispatchInputs<'a> {
    content_event: &'a str,
    post_id: &'a str,
    visibility: &'a str,
}

#[derive(Debug)]
struct DispatchFailureLog {
    content_event: BlogContentEvent,
    post_id: String,
    visibility: BlogVisibility,
    status: Option<StatusCode>,
    message: String,
}

impl BlogRedeployService {
    pub fn from_env() -> Self {
        let client = Client::new();
        let config = BlogRedeployConfig::from_env();

        let mode = match config {
            Some(config) => BlogRedeployMode::Forgejo {
                config: Arc::new(config),
            },
            None => BlogRedeployMode::Disabled {
                reason: Arc::from(
                    "TYANGE_BLOG_REDEPLOY_TOKEN 환경변수가 비어 있어 blog rebuild trigger를 건너뜁니다.",
                ),
            },
        };

        Self { client, mode }
    }

    pub async fn dispatch_content_change(
        &self,
        content_event: BlogContentEvent,
        post_id: &str,
        visibility: BlogVisibility,
    ) {
        match &self.mode {
            BlogRedeployMode::Disabled { reason } => {
                self.log_failure(DispatchFailureLog {
                    content_event,
                    post_id: post_id.to_string(),
                    visibility,
                    status: None,
                    message: reason.to_string(),
                });
            }
            BlogRedeployMode::Forgejo { config } => {
                let payload = WorkflowDispatchRequest {
                    git_ref: &config.git_ref,
                    inputs: WorkflowDispatchInputs {
                        content_event: content_event.as_str(),
                        post_id,
                        visibility: visibility.as_str(),
                    },
                };

                let url = format!(
                    "{}/repos/{}/actions/workflows/{}/dispatches",
                    config.api_base.trim_end_matches('/'),
                    config.repo,
                    config.workflow_filename
                );

                // Forgejo 토큰 인증은 GitHub의 `Bearer`가 아니라 `token` 접두어를 쓴다.
                match self
                    .client
                    .post(url)
                    .header(AUTHORIZATION, format!("token {}", config.token))
                    .header(USER_AGENT, SOURCE_NAME)
                    .json(&payload)
                    .send()
                    .await
                {
                    // return_run_info를 안 보내면 204, 보내면 201 — 어느 쪽이든 성공이라 상태
                    // 코드를 정확히 매칭하지 않고 2xx 전체를 성공으로 취급한다(예전 GitHub
                    // 버전은 204만 성공 처리해서 201이 오면 오판할 여지가 있었다).
                    Ok(response) if response.status().is_success() => {
                        println!(
                            "blog redeploy dispatch accepted: content_event={}, post_id={}, visibility={}, forgejo_status={}",
                            content_event.as_str(),
                            post_id,
                            visibility.as_str(),
                            response.status().as_u16()
                        );
                    }
                    Ok(response) => {
                        let status = response.status();
                        let body = response.text().await.unwrap_or_default();
                        self.log_failure(DispatchFailureLog {
                            content_event,
                            post_id: post_id.to_string(),
                            visibility,
                            status: Some(status),
                            message: if body.is_empty() {
                                "Forgejo workflow dispatch 호출이 실패했습니다.".to_string()
                            } else {
                                format!(
                                    "Forgejo workflow dispatch 호출이 실패했습니다. body={}",
                                    body
                                )
                            },
                        });
                    }
                    Err(error) => {
                        self.log_failure(DispatchFailureLog {
                            content_event,
                            post_id: post_id.to_string(),
                            visibility,
                            status: None,
                            message: error.to_string(),
                        });
                    }
                }
            }
            #[cfg(test)]
            BlogRedeployMode::Mock { handle } => {
                handle
                    .dispatch(content_event, post_id.to_string(), visibility)
                    .await;
            }
        }
    }

    fn log_failure(&self, failure: DispatchFailureLog) {
        eprintln!(
            "blog redeploy dispatch failed: content_event={}, post_id={}, visibility={}, forgejo_status={}, error={}",
            failure.content_event.as_str(),
            failure.post_id,
            failure.visibility.as_str(),
            failure
                .status
                .map(|status| status.as_u16().to_string())
                .unwrap_or_else(|| "none".to_string()),
            failure.message
        );
    }

    #[cfg(test)]
    pub fn mock() -> (Self, Arc<MockBlogRedeployHandle>) {
        let handle = Arc::new(MockBlogRedeployHandle::default());
        let service = Self {
            client: Client::new(),
            mode: BlogRedeployMode::Mock {
                handle: handle.clone(),
            },
        };

        (service, handle)
    }
}

impl BlogRedeployConfig {
    fn from_env() -> Option<Self> {
        let token = env::var("TYANGE_BLOG_REDEPLOY_TOKEN").ok()?;
        let token = token.trim().to_string();
        if token.is_empty() {
            return None;
        }

        let repo = env::var("TYANGE_BLOG_REDEPLOY_REPO")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_REPO.to_string());

        let workflow_filename = env::var("TYANGE_BLOG_REDEPLOY_WORKFLOW_FILENAME")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_WORKFLOW_FILENAME.to_string());

        let git_ref = env::var("TYANGE_BLOG_REDEPLOY_REF")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_REF.to_string());

        let api_base = env::var("TYANGE_BLOG_REDEPLOY_API_BASE")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_API_BASE.to_string());

        Some(Self {
            api_base,
            repo,
            workflow_filename,
            git_ref,
            token,
        })
    }
}

pub fn is_publicly_visible(status: &str) -> bool {
    !status.trim().eq_ignore_ascii_case("draft")
}

pub fn is_blog_redeploy_target<'a, I>(status: &str, tags: I) -> bool
where
    I: IntoIterator<Item = &'a str>,
{
    is_publicly_visible(status) && !tags.into_iter().any(is_dev_tag)
}

fn is_dev_tag(tag: &str) -> bool {
    tag.trim().eq_ignore_ascii_case("dev")
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MockDispatchCall {
    pub content_event: BlogContentEvent,
    pub post_id: String,
    pub visibility: BlogVisibility,
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MockDispatchFailure {
    pub content_event: BlogContentEvent,
    pub post_id: String,
    pub visibility: BlogVisibility,
    pub status: Option<u16>,
    pub message: String,
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
struct MockFailurePlan {
    status: Option<u16>,
    message: String,
}

#[cfg(test)]
#[derive(Default)]
pub struct MockBlogRedeployHandle {
    calls: tokio::sync::Mutex<Vec<MockDispatchCall>>,
    failures: tokio::sync::Mutex<Vec<MockDispatchFailure>>,
    next_failure: tokio::sync::Mutex<Option<MockFailurePlan>>,
}

#[cfg(test)]
impl MockBlogRedeployHandle {
    async fn dispatch(
        &self,
        content_event: BlogContentEvent,
        post_id: String,
        visibility: BlogVisibility,
    ) {
        self.calls.lock().await.push(MockDispatchCall {
            content_event,
            post_id: post_id.clone(),
            visibility,
        });

        if let Some(failure_plan) = self.next_failure.lock().await.take() {
            let failure = MockDispatchFailure {
                content_event,
                post_id: post_id.clone(),
                visibility,
                status: failure_plan.status,
                message: failure_plan.message,
            };
            let status_text = failure
                .status
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string());
            let message = failure.message.clone();
            self.failures.lock().await.push(failure);
            eprintln!(
                "blog redeploy dispatch failed: content_event={}, post_id={}, visibility={}, forgejo_status={}, error={}",
                content_event.as_str(),
                post_id,
                visibility.as_str(),
                status_text,
                message
            );
        }
    }

    pub async fn take_calls(&self) -> Vec<MockDispatchCall> {
        std::mem::take(&mut *self.calls.lock().await)
    }

    pub async fn take_failures(&self) -> Vec<MockDispatchFailure> {
        std::mem::take(&mut *self.failures.lock().await)
    }

    pub async fn fail_next(&self, failure: MockDispatchFailure) {
        *self.next_failure.lock().await = Some(MockFailurePlan {
            status: failure.status,
            message: failure.message,
        });
    }
}
