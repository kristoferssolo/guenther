const MAX_INLINE_RESULTS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VoiceLine {
    pub id: &'static str,
    pub title: &'static str,
    pub file_id: &'static str,
    pub tags: &'static [&'static str],
}

pub const VOICE_LINES: &[VoiceLine] = &[
    // VoiceLine {
    //     id: "not_for_guenther",
    //     title: "This is not for Guenther",
    //     file_id: "AwACAgIAAxkBAAIB...",
    //     tags: &["radio", "angry", "classic"],
    // },
];

#[must_use]
pub fn search_voice_lines(query: &str) -> Vec<&'static VoiceLine> {
    let needle = normalize(query);

    if needle.is_empty() {
        return VOICE_LINES.iter().take(MAX_INLINE_RESULTS).collect();
    }

    VOICE_LINES
        .iter()
        .filter(|line| matches_query(line, &needle))
        .take(MAX_INLINE_RESULTS)
        .collect()
}

#[inline]
fn matches_query(line: &VoiceLine, needle: &str) -> bool {
    contains_ignore_ascii_case(line.title, needle)
        || contains_ignore_ascii_case(line.id, needle)
        || line
            .tags
            .iter()
            .any(|tag| contains_ignore_ascii_case(tag, needle))
}

#[inline]
fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    if needle.len() > haystack.len() {
        return false;
    }
    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

#[inline]
fn normalize(text: &str) -> String {
    text.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{VoiceLine, normalize};

    const SAMPLE_LINES: &[VoiceLine] = &[
        VoiceLine {
            id: "line_1",
            title: "This is not acceptable",
            file_id: "file-1",
            tags: &["angry", "radio"],
        },
        VoiceLine {
            id: "line_2",
            title: "We look like amateurs",
            file_id: "file-2",
            tags: &["team", "mess"],
        },
    ];

    fn search(lines: &'static [VoiceLine], query: &str) -> Vec<&'static VoiceLine> {
        let normalized_query = normalize(query);

        lines
            .iter()
            .filter(|line| {
                normalized_query.is_empty()
                    || normalize(line.title).contains(&normalized_query)
                    || normalize(line.id).contains(&normalized_query)
                    || line
                        .tags
                        .iter()
                        .any(|tag| normalize(tag).contains(&normalized_query))
            })
            .collect()
    }

    #[test]
    fn matches_by_title() {
        let results = search(SAMPLE_LINES, "acceptable");
        assert_eq!(results, vec![&SAMPLE_LINES[0]]);
    }

    #[test]
    fn matches_by_tag() {
        let results = search(SAMPLE_LINES, "mess");
        assert_eq!(results, vec![&SAMPLE_LINES[1]]);
    }

    #[test]
    fn empty_query_returns_all() {
        let results = search(SAMPLE_LINES, "");
        assert_eq!(results, SAMPLE_LINES.iter().collect::<Vec<_>>());
    }
}
