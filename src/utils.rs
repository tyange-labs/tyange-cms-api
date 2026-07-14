use crate::models::TagWithCategory;

pub fn parse_tags(tags_str: &str) -> Vec<TagWithCategory> {
    if tags_str.is_empty() {
        return vec![];
    }

    tags_str
        .split(',')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, "::");
            Some(TagWithCategory {
                category: parts.next()?.trim().to_string(),
                tag: parts.next()?.trim().to_string(),
            })
        })
        .collect()
}
