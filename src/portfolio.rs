//! 포트폴리오 섹션 읽기·검증·저장. HTTP 라우트(`openapi.rs::PortfolioApi`)와 MCP 도구(`mcp`)가 같이 쓴다.
//! 포트폴리오는 slug `dev` 하나이고, 섹션(`meta`, `identity`, `featured_projects`, `career`, `intro`, `portfolio_pdf`)
//! 마다 JSON을 따로 저장한다.

use sqlx::{query, Sqlite};

pub const PORTFOLIO_SLUG: &str = "dev";

#[derive(Debug)]
pub enum SaveError {
    /// 섹션 키를 모르거나 모양이 스키마와 다르다.
    Invalid(String),
    /// 포트폴리오(slug `dev`) 자체가 없다.
    NoPortfolio,
    Db(String),
}

// 파싱에 성공한 "구조체"를 재직렬화한다 — 원본 value를 그대로 재직렬화하면 스키마에
// 없는 키(예전 identity.role/location/availability처럼)가 계속 살아남는다. 구조체를
// 거쳐 다시 직렬화하면 알려진 필드만 남고, serde(default)가 채운 값도 일관되게 저장된다.
pub fn validate_portfolio_section(
    section_key: &str,
    value: &serde_json::Value,
) -> Result<String, String> {
    use crate::models::{
        PortfolioCareerSection, PortfolioIdentity, PortfolioIntroSection, PortfolioMeta,
        PortfolioProject,
    };

    match section_key {
        "meta" => {
            let parsed: PortfolioMeta = serde_json::from_value(value.clone())
                .map_err(|e| format!("meta 검증 실패: {}", e))?;
            serde_json::to_string(&parsed).map_err(|e| e.to_string())
        }
        "identity" => {
            let parsed: PortfolioIdentity = serde_json::from_value(value.clone())
                .map_err(|e| format!("identity 검증 실패: {}", e))?;
            serde_json::to_string(&parsed).map_err(|e| e.to_string())
        }
        "featured_projects" => {
            let parsed: Vec<PortfolioProject> = serde_json::from_value(value.clone())
                .map_err(|e| format!("featured_projects 검증 실패: {}", e))?;
            serde_json::to_string(&parsed).map_err(|e| e.to_string())
        }
        "career" => {
            let parsed: PortfolioCareerSection = serde_json::from_value(value.clone())
                .map_err(|e| format!("career 검증 실패: {}", e))?;
            serde_json::to_string(&parsed).map_err(|e| e.to_string())
        }
        "intro" => {
            let parsed: PortfolioIntroSection = serde_json::from_value(value.clone())
                .map_err(|e| format!("intro 검증 실패: {}", e))?;
            serde_json::to_string(&parsed).map_err(|e| e.to_string())
        }
        "portfolio_pdf" => {
            use crate::models::{PORTFOLIO_PDF_MAX_BYTES, PORTFOLIO_PDF_MAX_PAGES};
            let parsed: crate::models::PortfolioPdfSection = serde_json::from_value(value.clone())
                .map_err(|e| format!("portfolio_pdf 검증 실패: {}", e))?;
            if parsed.pages.len() > PORTFOLIO_PDF_MAX_PAGES {
                return Err(format!(
                    "portfolio_pdf 페이지는 최대 {}장입니다.",
                    PORTFOLIO_PDF_MAX_PAGES
                ));
            }
            let mut seen = std::collections::HashSet::new();
            for page in &parsed.pages {
                if page.id.trim().is_empty() || page.template.trim().is_empty() {
                    return Err(
                        "portfolio_pdf 페이지의 id와 template은 비어 있을 수 없습니다.".to_string(),
                    );
                }
                if !seen.insert(page.id.as_str()) {
                    return Err(format!("portfolio_pdf 페이지 id가 중복됩니다: {}", page.id));
                }
            }
            let serialized = serde_json::to_string(&parsed).map_err(|e| e.to_string())?;
            if serialized.len() > PORTFOLIO_PDF_MAX_BYTES {
                return Err(format!(
                    "portfolio_pdf 문서는 {}KB 이하여야 합니다.",
                    PORTFOLIO_PDF_MAX_BYTES / 1024
                ));
            }
            Ok(serialized)
        }
        _ => Err(format!("알 수 없는 섹션: {}", section_key)),
    }
}

/// 섹션 내용과 마지막 수정 시각. 섹션이 아직 없으면 `None`.
pub async fn get_section(
    db: &sqlx::Pool<Sqlite>,
    section_key: &str,
) -> Result<Option<(serde_json::Value, String)>, String> {
    let row: Option<(String, String)> = sqlx::query_as(
        r#"
        SELECT s.content, s.updated_at
        FROM portfolio_section s
        JOIN portfolio p ON p.portfolio_id = s.portfolio_id
        WHERE p.slug = ? AND s.section_key = ?
        "#,
    )
    .bind(PORTFOLIO_SLUG)
    .bind(section_key)
    .fetch_optional(db)
    .await
    .map_err(|err| format!("{} 섹션 조회 실패: {}", section_key, err))?;
    let Some((content, updated_at)) = row else {
        return Ok(None);
    };
    let value = serde_json::from_str(&content)
        .map_err(|err| format!("{} 섹션 파싱 실패: {}", section_key, err))?;
    Ok(Some((value, updated_at)))
}

/// 섹션 키와 마지막 수정 시각, 저장된 JSON 크기(바이트).
pub async fn list_sections(db: &sqlx::Pool<Sqlite>) -> Result<Vec<(String, String, i64)>, String> {
    sqlx::query_as(
        r#"
        SELECT s.section_key, s.updated_at, length(s.content)
        FROM portfolio_section s
        JOIN portfolio p ON p.portfolio_id = s.portfolio_id
        WHERE p.slug = ?
        ORDER BY s.section_key
        "#,
    )
    .bind(PORTFOLIO_SLUG)
    .fetch_all(db)
    .await
    .map_err(|err| format!("포트폴리오 섹션 목록 조회 실패: {}", err))
}

/// 섹션을 검증하고 통째로 저장한다. 저장 뒤의 `updated_at`을 돌려준다.
pub async fn save_section(
    db: &sqlx::Pool<Sqlite>,
    section_key: &str,
    value: &serde_json::Value,
) -> Result<String, SaveError> {
    let serialized = validate_portfolio_section(section_key, value).map_err(SaveError::Invalid)?;

    let portfolio_id: Option<i32> =
        sqlx::query_scalar("SELECT portfolio_id FROM portfolio WHERE slug = ?")
            .bind(PORTFOLIO_SLUG)
            .fetch_optional(db)
            .await
            .map_err(|err| SaveError::Db(format!("포트폴리오 조회 실패: {}", err)))?;
    let portfolio_id = portfolio_id.ok_or(SaveError::NoPortfolio)?;

    query(
        r#"
        INSERT INTO portfolio_section (portfolio_id, section_key, content, created_at, updated_at)
        VALUES (?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
        ON CONFLICT(portfolio_id, section_key) DO UPDATE SET
            content = excluded.content,
            updated_at = CURRENT_TIMESTAMP
        "#,
    )
    .bind(portfolio_id)
    .bind(section_key)
    .bind(&serialized)
    .execute(db)
    .await
    .map_err(|err| SaveError::Db(format!("{} 섹션 저장 실패: {}", section_key, err)))?;

    sqlx::query_scalar(
        "SELECT updated_at FROM portfolio_section WHERE portfolio_id = ? AND section_key = ?",
    )
    .bind(portfolio_id)
    .bind(section_key)
    .fetch_one(db)
    .await
    .map_err(|err| SaveError::Db(format!("updated_at 조회 실패: {}", err)))
}
