use crate::{media_posts::period::StatsPeriod, utils::truncate_with_ellipsis};
use std::fmt;

/// Longest sender name or URL shown. With at most 10 senders, 5 links, and
/// the platforms, this keeps the reply under Telegram's 4096-character limit.
const MAX_LABEL_CHARS: usize = 200;

/// How often one sender, platform, or link appears in a chat's media posts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareCount {
    pub label: String,
    pub count: i64,
}

/// Aggregated media posts of one chat, rendered as the `/stats` reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkStats {
    pub period: StatsPeriod,
    /// Most active senders first.
    pub senders: Vec<ShareCount>,
    /// Every platform with at least one post, most used first.
    pub platforms: Vec<ShareCount>,
    /// Links shared more than once, labeled with their latest URL.
    pub links: Vec<ShareCount>,
}

impl LinkStats {
    pub fn total(&self) -> i64 {
        self.platforms.iter().map(|platform| platform.count).sum()
    }
}

impl fmt::Display for LinkStats {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Links, {}: {}",
            self.period.label(),
            self.total()
        )?;
        write_inline(formatter, "Senders", &self.senders)?;
        write_inline(formatter, "Platforms", &self.platforms)?;
        if !self.links.is_empty() {
            formatter.write_str("\nMost shared:")?;
            for link in &self.links {
                write!(formatter, "\n{} {}", link.count, label(&link.label))?;
            }
        }
        Ok(())
    }
}

fn write_inline(
    formatter: &mut fmt::Formatter<'_>,
    title: &str,
    counts: &[ShareCount],
) -> fmt::Result {
    let mut counts = counts.iter();
    let Some(first) = counts.next() else {
        return Ok(());
    };
    write!(
        formatter,
        "\n{title}: {} {}",
        label(&first.label),
        first.count
    )?;
    for count in counts {
        write!(formatter, ", {} {}", label(&count.label), count.count)?;
    }
    Ok(())
}

fn label(text: &str) -> String {
    truncate_with_ellipsis(text, MAX_LABEL_CHARS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(label: &str, count: i64) -> ShareCount {
        ShareCount {
            label: label.to_owned(),
            count,
        }
    }

    #[test]
    fn renders_sections_densely() {
        let stats = LinkStats {
            period: StatsPeriod::Month,
            senders: vec![count("Alice", 3), count("Bob", 1)],
            platforms: vec![count("instagram", 2), count("tiktok", 2)],
            links: vec![count("https://x.com/a/status/1", 2)],
        };
        assert_eq!(
            stats.to_string(),
            "Links, past month: 4\n\
             Senders: Alice 3, Bob 1\n\
             Platforms: instagram 2, tiktok 2\n\
             Most shared:\n\
             2 https://x.com/a/status/1"
        );
    }

    #[test]
    fn long_labels_stay_within_the_telegram_message_limit() {
        let name = "N".repeat(500);
        let url = format!("https://instagram.com/reel/abc/?igsh={}", "x".repeat(3000));
        let stats = LinkStats {
            period: StatsPeriod::All,
            senders: vec![count(&name, 2); 10],
            platforms: vec![
                count("instagram", 5),
                count("tiktok", 5),
                count("twitter", 5),
                count("youtube", 5),
            ],
            links: vec![count(&url, 2); 5],
        };

        let rendered = stats.to_string();

        assert!(rendered.chars().count() <= 4096);
        assert!(!rendered.contains(&url));
        assert!(rendered.contains(&truncate_with_ellipsis(&url, MAX_LABEL_CHARS)));
    }

    #[test]
    fn renders_only_the_total_without_posts() {
        let stats = LinkStats {
            period: StatsPeriod::All,
            senders: Vec::new(),
            platforms: Vec::new(),
            links: Vec::new(),
        };
        assert_eq!(stats.to_string(), "Links, all time: 0");
    }
}
