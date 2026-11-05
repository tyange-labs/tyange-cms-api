//! 알라딘·YES24 상품 페이지를 정규식 없이 읽는 작은 HTML 도우미들(형제 모듈 `details`·`yes24`가 함께 쓴다).

use super::strip_tags;

/// `<tag`가 시작하는 위치(바로 뒤가 공백·`>`·`/`여야 한다 — `<li`가 `<link`에 걸리지 않게).
pub(super) fn find_tag_open(html: &str, tag: &str) -> Option<usize> {
    let needle = format!("<{tag}");
    let mut from = 0;
    while let Some(rel) = html[from..].find(&needle) {
        let at = from + rel;
        let next = html[at + needle.len()..].chars().next();
        if matches!(next, Some(c) if c.is_whitespace() || c == '>' || c == '/') {
            return Some(at);
        }
        from = at + needle.len();
    }
    None
}

/// 여는 태그에 `marker`가 든 첫 `<tag …>…</tag>`의 (안쪽 HTML, 그 뒤 나머지). 같은 이름의 태그가 안에
/// 겹쳐 있어도 짝을 맞춘다. `marker`가 빈 문자열이면 처음 만나는 `<tag>`.
pub(super) fn next_element<'a>(html: &'a str, tag: &str, marker: &str) -> Option<(&'a str, &'a str)> {
    let close = format!("</{tag}>");
    let mut from = 0;
    loop {
        let start = from + find_tag_open(&html[from..], tag)?;
        let open_end = start + html[start..].find('>')?;
        if !html[start..open_end].contains(marker) {
            from = open_end + 1;
            continue;
        }
        let content_start = open_end + 1;
        let mut depth = 1;
        let mut pos = content_start;
        while depth > 0 {
            let next_close = pos + html[pos..].find(&close)?;
            match find_tag_open(&html[pos..next_close], tag) {
                Some(rel) => {
                    depth += 1;
                    pos += rel + tag.len() + 1;
                }
                None => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((&html[content_start..next_close], &html[next_close + close.len()..]));
                    }
                    pos = next_close + close.len();
                }
            }
        }
        unreachable!()
    }
}

pub(super) fn inner_html<'a>(html: &'a str, tag: &str, marker: &str) -> Option<&'a str> {
    next_element(html, tag, marker).map(|(inner, _)| inner)
}

/// 같은 높이에 늘어선 `<tag>…</tag>`들의 안쪽 HTML.
pub(super) fn elements<'a>(html: &'a str, tag: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some((inner, after)) = next_element(rest, tag, "") {
        out.push(inner);
        rest = after;
    }
    out
}

pub(super) fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp..];
        let entity_end = after.find(';').filter(|&end| end <= 10);
        let decoded = entity_end.and_then(|end| {
            let name = &after[1..end];
            let ch = match name {
                "nbsp" => Some(' '),
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => name
                    .strip_prefix("#x")
                    .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                    .or_else(|| name.strip_prefix('#').and_then(|dec| dec.parse().ok()))
                    .and_then(char::from_u32),
            }?;
            Some((ch, end))
        });
        match decoded {
            Some((ch, end)) => {
                out.push(ch);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// 태그를 지우고 엔티티를 풀고 공백을 한 칸으로 줄인 글자.
pub(super) fn clean_text(html: &str) -> String {
    decode_entities(&strip_tags(html)).split_whitespace().collect::<Vec<_>>().join(" ")
}
