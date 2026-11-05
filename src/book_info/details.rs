//! 알라딘 상품 페이지에서 책 상세를 뽑는다: 제목·부제·사람(지은이/옮긴이 등, 역할 표기 그대로)·출판사·
//! 출간일·원제·판본(종이책/전자책)·기본정보(쪽수·판형·무게)·주제 분류(여러 줄)·형식어.
//!
//! 전자책 상품 페이지에서는 상세를 쌓지 않는다(`ProductPage::is_ebook`으로 가려 부모 모듈이 저장하지 않는다) —
//! 같은 책의 종이책 페이지가 더 많은 정보를 갖고 있고, 같은 책을 두 번 쌓을 이유가 없다.

use serde::{Deserialize, Serialize};

use super::html::{clean_text, decode_entities, elements, find_tag_open, inner_html, next_element};
use super::{attribute, extract_form_hint};

/// 사람 한 명. `role`은 페이지에 적힌 그대로다("지은이", "옮긴이", "글", "그림" …). 여러 사람이 한 역할을
/// 공유하면("A, B (지은이)") 각자에게 같은 역할이 붙는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Person {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

/// 종이책 상품 페이지 한 장에서 뽑은 상세.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BookDetails {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub people: Vec<Person>,
    pub publisher: Option<String>,
    /// "2025-10-30"
    pub pub_date: Option<String>,
    /// 번역서의 원제("원제 : …"의 뒷부분)
    pub original_title: Option<String>,
    /// 판본 탭에 전자책이 있는가
    pub has_ebook: bool,
    pub pages: Option<i64>,
    /// "170*225mm", "128*188mm (B6)"
    pub size: Option<String>,
    /// "676g"
    pub weight: Option<String>,
    /// 기본정보 목록에서 쪽수·판형·무게·ISBN이 아닌 나머지(예: "양장본")
    pub extra_info: Vec<String>,
    /// 주제 분류 경로들("국내도서 > 소설/시/희곡 > 한국소설 > …"). 한 책이 여러 분류에 걸릴 수 있다.
    pub categories: Vec<String>,
    /// 책 소개의 형식어("장편소설"·"소설집" …) — `extract_form_hint` 참고
    pub form_hint: Option<String>,
}

/// 상품 페이지 한 장을 읽은 결과.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ProductPage {
    /// 판본 탭이 "전자책"에 켜져 있거나 제목이 "[eBook]"으로 시작한다
    pub is_ebook: bool,
    pub details: BookDetails,
}

impl ProductPage {
    pub fn from_html(html: &str) -> Self {
        let raw_title = inner_html(html, "span", r#"class="Ere_bo_title""#).map(clean_text);
        let title_says_ebook = raw_title.as_deref().is_some_and(|t| t.starts_with("[eBook]"));
        let editions = Editions::from_html(html);
        let credits = Credits::from_html(html);
        let info = BasicInfo::from_html(html);
        Self {
            is_ebook: title_says_ebook || editions.current_is_ebook,
            details: BookDetails {
                title: raw_title
                    .map(|t| t.trim_start_matches("[eBook]").trim().to_string())
                    .filter(|t| !t.is_empty()),
                subtitle: inner_html(html, "span", r#"class="Ere_sub1_title""#)
                    .map(clean_text)
                    .map(|s| s.trim_start_matches('-').trim().to_string())
                    .filter(|s| !s.is_empty()),
                people: credits.people,
                publisher: credits.publisher,
                pub_date: credits.pub_date,
                original_title: credits.original_title,
                has_ebook: editions.other_is_ebook,
                pages: info.pages,
                size: info.size,
                weight: info.weight,
                extra_info: info.extra,
                categories: extract_categories(html),
                form_hint: extract_form_hint(html).map(str::to_string),
            },
        }
    }
}

/// 대표 표지 이미지(`<img id="CoverMainImage" src="…">`). 주소를 믿어도 되는지는 여기서 판단하지 않는다.
pub fn extract_cover_main_image(html: &str) -> Option<String> {
    let mut rest = html;
    while let Some(start) = find_tag_open(rest, "img") {
        let tag_and_after = &rest[start..];
        let end = tag_and_after.find('>')?;
        let tag = &tag_and_after[..end];
        if attribute(tag, "id") == Some("CoverMainImage") {
            return attribute(tag, "src").map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        }
        rest = &tag_and_after[end + 1..];
    }
    None
}

/// 주제 분류(`<ul id="ulCategory">`)의 줄마다 경로 하나. 각 줄 끝의 "접기" 토글(`…Fold…` class)은 뺀다.
pub fn extract_categories(html: &str) -> Vec<String> {
    let Some(list) = inner_html(html, "ul", r#"id="ulCategory""#) else {
        return Vec::new();
    };
    elements(list, "li").into_iter().filter_map(category_from_line).collect()
}

fn category_from_line(line: &str) -> Option<String> {
    let mut parts = Vec::new();
    let mut rest = line;
    while let Some(a_start) = find_tag_open(rest, "a") {
        let after_a = &rest[a_start..];
        let Some(tag_end) = after_a.find('>') else { break };
        let open_tag = &after_a[..tag_end];
        let text_start = tag_end + 1;
        let Some(close_rel) = after_a[text_start..].find("</a>") else { break };
        if open_tag.contains("Fold") {
            break;
        }
        let text = clean_text(&after_a[text_start..text_start + close_rel]);
        if !text.is_empty() {
            parts.push(text);
        }
        rest = &after_a[text_start + close_rel + 4..];
    }
    (!parts.is_empty()).then(|| parts.join(" > "))
}

// ── 사람·출판사·출간일·원제 (`<li class="Ere_sub2_title">`) ─────────────────────────────────
#[derive(Default)]
struct Credits {
    people: Vec<Person>,
    publisher: Option<String>,
    pub_date: Option<String>,
    original_title: Option<String>,
}

impl Credits {
    fn from_html(html: &str) -> Self {
        let Some(line) = inner_html(html, "li", r#"class="Ere_sub2_title""#) else {
            return Self::default();
        };
        let mut credits = Self::default();
        // 역할이 아직 안 붙은 사람들: "A, B (지은이)"처럼 쉼표로 이어지다가 괄호가 나오면 다 같이 그 역할이다.
        let mut waiting: Vec<Person> = Vec::new();
        let mut rest = line;
        while let Some(a_start) = find_tag_open(rest, "a") {
            let after_a = &rest[a_start..];
            let Some(tag_end) = after_a.find('>') else { break };
            let open_tag = &after_a[..tag_end];
            let text_start = tag_end + 1;
            let Some(close_rel) = after_a[text_start..].find("</a>") else { break };
            let text = clean_text(&after_a[text_start..text_start + close_rel]);
            let after_close = &after_a[text_start + close_rel + 4..];
            let trailing = decode_entities(&after_close[..after_close.find('<').unwrap_or(after_close.len())]);

            if open_tag.contains("AuthorSearch") {
                waiting.push(Person { name: text, role: None });
                if let Some(role) = parenthesized(&trailing) {
                    for mut person in waiting.drain(..) {
                        person.role = Some(role.clone());
                        credits.people.push(person);
                    }
                }
            } else if open_tag.contains("PublisherSearch") {
                credits.publisher = Some(text).filter(|t| !t.is_empty());
            } else if let Some(original) = text.strip_prefix("원제") {
                let original = original.trim_start().trim_start_matches(':').trim();
                if !original.is_empty() {
                    credits.original_title = Some(original.to_string());
                }
            }
            rest = after_close;
        }
        credits.people.extend(waiting);
        credits.pub_date = find_date(&clean_text(line));
        credits
    }
}

fn parenthesized(text: &str) -> Option<String> {
    let open = text.find('(')?;
    let close = text[open..].find(')')? + open;
    let inner = text[open + 1..close].trim();
    (!inner.is_empty()).then(|| inner.to_string())
}

/// "2025-10-30" 꼴의 첫 날짜.
fn find_date(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    (0..bytes.len().saturating_sub(9)).find_map(|i| {
        let candidate = &bytes[i..i + 10];
        let shape_ok = candidate.iter().enumerate().all(|(j, b)| match j {
            4 | 7 => *b == b'-',
            _ => b.is_ascii_digit(),
        });
        shape_ok.then(|| String::from_utf8_lossy(candidate).into_owned())
    })
}

// ── 판본 탭 (`<div class="book_tab_box">`) ─────────────────────────────────────────────
#[derive(Default)]
struct Editions {
    current_is_ebook: bool,
    other_is_ebook: bool,
}

impl Editions {
    fn from_html(html: &str) -> Self {
        let Some(tabs) = inner_html(html, "div", r#"class="book_tab_box""#) else {
            return Self::default();
        };
        let current = inner_html(tabs, "span", r#"class="tab_on""#).map(clean_text).unwrap_or_default();
        let mut other_is_ebook = false;
        let mut rest = tabs;
        while let Some((inner, after)) = next_element(rest, "span", r#"class="tab_off""#) {
            other_is_ebook |= clean_text(inner).starts_with("전자책");
            rest = after;
        }
        Self { current_is_ebook: current.starts_with("전자책"), other_is_ebook }
    }
}

// ── 기본정보 (`<div class="conts_info_list1">`) ────────────────────────────────────────
#[derive(Default)]
struct BasicInfo {
    pages: Option<i64>,
    size: Option<String>,
    weight: Option<String>,
    extra: Vec<String>,
}

impl BasicInfo {
    fn from_html(html: &str) -> Self {
        let Some(block) = inner_html(html, "div", r#"class="conts_info_list1""#) else {
            return Self::default();
        };
        let mut info = Self::default();
        for item in elements(block, "li").into_iter().map(clean_text).filter(|t| !t.is_empty()) {
            if let Some(pages) = item.strip_suffix('쪽').and_then(|n| n.trim().parse().ok()) {
                info.pages = Some(pages);
            } else if item.starts_with(|c: char| c.is_ascii_digit()) && item.contains('*') && item.contains("mm") {
                info.size = Some(item);
            } else if item.strip_suffix('g').is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())) {
                info.weight = Some(item);
            } else if item.starts_with("ISBN") {
                // 요청한 ISBN과 같다(행의 키) — 따로 두지 않는다.
            } else {
                info.extra.push(item);
            }
        }
        info
    }
}

#[cfg(test)]
mod tests;
