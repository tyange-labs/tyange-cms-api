//! 터미널 출력용 정렬 도우미. (Claude가 만든 뼈대)
//!
//! `format!("{:<6}", s)` 는 "글자 수"로 칸을 채우는데,
//! 한글은 터미널에서 한 글자가 2칸을 차지해서 열이 어긋납니다.
//! 그래서 "화면 폭"을 직접 세서 공백을 채웁니다.

/// 터미널에서 이 글자가 차지하는 칸 수. 한글은 2, 나머지는 1로 봅니다.
fn char_width(c: char) -> usize {
    match c as u32 {
        0x1100..=0x115F | 0x3130..=0x318F | 0xAC00..=0xD7A3 => 2,
        _ => 1,
    }
}

/// 문자열이 터미널에서 차지하는 칸 수.
pub fn display_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// 왼쪽 정렬: 오른쪽에 공백을 붙여 `width` 칸으로 맞춥니다.
pub fn pad_right(s: &str, width: usize) -> String {
    let fill = width.saturating_sub(display_width(s));
    format!("{s}{}", " ".repeat(fill))
}

/// 오른쪽 정렬: 왼쪽에 공백을 붙여 `width` 칸으로 맞춥니다.
pub fn pad_left(s: &str, width: usize) -> String {
    let fill = width.saturating_sub(display_width(s));
    format!("{}{s}", " ".repeat(fill))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 한글은_두_칸() {
        assert_eq!(display_width("에스퍼"), 6);
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width("물/비행"), 7);
    }

    #[test]
    fn 폭을_맞춰_채운다() {
        assert_eq!(pad_right("물", 6), "물    ");
        assert_eq!(pad_left("물", 6), "    물");
        // 이미 넘치면 자르지 않고 그대로 둡니다.
        assert_eq!(pad_right("드래곤드래곤", 4), "드래곤드래곤");
    }
}
