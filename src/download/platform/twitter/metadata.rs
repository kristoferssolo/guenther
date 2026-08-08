use serde_json::Value;

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
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_prefers_full_text() {
        let content = json!({"full_text": "hello world", "description": "fallback"});
        assert_eq!(
            parse_post_text_from_value(&content).as_deref(),
            Some("hello world")
        );
    }

    #[test]
    fn parse_falls_back_to_text() {
        let content = json!({"text": "tweet body"});
        assert_eq!(
            parse_post_text_from_value(&content).as_deref(),
            Some("tweet body")
        );
    }

    #[test]
    fn parse_falls_back_to_description() {
        let content = json!({"description": "fallback"});
        assert_eq!(
            parse_post_text_from_value(&content).as_deref(),
            Some("fallback")
        );
    }

    #[test]
    fn parse_rejects_empty_text() {
        let content = json!({"full_text": "   "});
        assert!(parse_post_text_from_value(&content).is_none());
    }
}
