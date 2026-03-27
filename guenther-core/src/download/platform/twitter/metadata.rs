use serde_json::Value;
use std::path::Path;
use tokio::fs;

pub async fn extract_post_text(root: &Path) -> Option<String> {
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let mut read_dir = fs::read_dir(&dir).await.ok()?;
        while let Some(entry) = read_dir.next_entry().await.ok()? {
            let path = entry.path();
            let file_type = entry.file_type().await.ok()?;

            if file_type.is_dir() {
                stack.push(path);
                continue;
            }

            if !is_info_json(&path) {
                continue;
            }

            let content = fs::read_to_string(&path).await.ok()?;
            if let Some(text) = parse_post_text(&content) {
                return Some(text);
            }
        }
    }

    None
}

fn is_info_json(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".info.json"))
}

pub fn parse_post_text(content: &str) -> Option<String> {
    let json = serde_json::from_str::<Value>(content).ok()?;
    parse_post_text_from_value(&json)
}

pub fn parse_post_text_from_value(json: &Value) -> Option<String> {
    let text = json
        .get("full_text")
        .and_then(Value::as_str)
        .or_else(|| json.get("text").and_then(Value::as_str))
        .or_else(|| json.get("description").and_then(Value::as_str))
        .or_else(|| json.get("title").and_then(Value::as_str))?
        .trim();

    (!text.is_empty()).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::parse_post_text;

    #[test]
    fn parse_prefers_full_text() {
        let content = r#"{"full_text":"hello world","description":"fallback"}"#;
        assert_eq!(parse_post_text(content).as_deref(), Some("hello world"));
    }

    #[test]
    fn parse_falls_back_to_text() {
        let content = r#"{"text":"tweet body"}"#;
        assert_eq!(parse_post_text(content).as_deref(), Some("tweet body"));
    }

    #[test]
    fn parse_falls_back_to_description() {
        let content = r#"{"description":"fallback"}"#;
        assert_eq!(parse_post_text(content).as_deref(), Some("fallback"));
    }

    #[test]
    fn parse_rejects_empty_text() {
        let content = r#"{"full_text":"   "}"#;
        assert!(parse_post_text(content).is_none());
    }
}
