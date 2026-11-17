//! The editable parts of Guenther's system prompts, changed from the admin
//! chat with `/prompt`.
//!
//! Every saved version stays in `prompt_versions` and the newest of each name
//! is active, so an edit applies to the next caption or reply without a deploy
//! and can be reverted. The migration seeds generic versions. The guards
//! against instructions in chat messages stay in code, so no edit can drop
//! them.

use crate::{db, telemetry::ErrorChain, utils::truncate_with_ellipsis};
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use std::{collections::HashMap, fmt::Write};
use strum::{Display, EnumString, IntoStaticStr, VariantArray};
use teloxide::utils::html;
use thiserror::Error;
use tracing::{info, warn};

/// Longest text `/prompt <name> set` accepts, so that `/prompt <name>` can
/// still show it within Telegram's 4096 character message limit.
pub const MAX_PROMPT_CHARS: usize = 3500;
const HISTORY_LENGTH: i64 = 10;
const PREVIEW_CHARS: usize = 60;

#[derive(Debug, Error)]
pub enum PromptError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("No `{0}` prompt is saved.")]
    Missing(PromptName),

    #[error("Database contains unknown prompt name `{0}`")]
    UnknownName(String),

    #[error("{}", usage())]
    Usage,

    #[error("Usage: /prompt <name> set <text>")]
    Empty,

    #[error("The text has {0} characters; the limit is {MAX_PROMPT_CHARS}.")]
    TooLong(usize),

    #[error("Version {1} of `{0}` does not exist.")]
    UnknownVersion(PromptName, i64),
}

pub type Result<T> = std::result::Result<T, PromptError>;

/// One editable part of the system prompts.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumString, IntoStaticStr, VariantArray,
)]
#[strum(serialize_all = "snake_case")]
pub enum PromptName {
    /// Who Guenther is; shared by captions and replies.
    Persona,
    /// What a caption is, before the persona.
    CaptionTask,
    /// How to write a caption, after the persona.
    CaptionRules,
    /// What a reply is, before the persona.
    ReplyTask,
    /// How to write a reply, after the persona.
    ReplyRules,
}

impl PromptName {
    fn as_str(self) -> &'static str {
        self.into()
    }
}

/// The active version of every prompt, as the model gets them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptSet {
    pub persona: String,
    pub caption_task: String,
    pub caption_rules: String,
    pub reply_task: String,
    pub reply_rules: String,
}

/// One saved version of a prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptVersion {
    pub id: i64,
    pub text: String,
    pub created_at: String,
}

/// The active version of one prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveVersion {
    name: PromptName,
    version: PromptVersion,
}

/// Persistent store of prompt versions.
#[derive(Debug, Clone)]
pub struct PromptStore {
    pool: SqlitePool,
}

impl PromptStore {
    pub const fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// The active version of every prompt, for the next caption or reply.
    ///
    /// # Errors
    ///
    /// Returns an error when the query fails or a prompt has no version.
    pub async fn active_set(&self) -> Result<PromptSet> {
        let mut texts = self
            .active_versions()
            .await?
            .into_iter()
            .map(|active| (active.name, active.version.text))
            .collect::<HashMap<_, _>>();
        let mut take = |name| texts.remove(&name).ok_or(PromptError::Missing(name));
        Ok(PromptSet {
            persona: take(PromptName::Persona)?,
            caption_task: take(PromptName::CaptionTask)?,
            caption_rules: take(PromptName::CaptionRules)?,
            reply_task: take(PromptName::ReplyTask)?,
            reply_rules: take(PromptName::ReplyRules)?,
        })
    }

    /// The newest version of each prompt that has one.
    async fn active_versions(&self) -> Result<Vec<ActiveVersion>> {
        let rows = sqlx::query!(
            r#"
            SELECT
                id,
                name,
                text,
                created_at
            FROM
                prompt_versions
            WHERE
                id IN (
                    SELECT
                        MAX(id)
                    FROM
                        prompt_versions
                    GROUP BY
                        name
                )
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let name = row
                    .name
                    .parse()
                    .map_err(|_| PromptError::UnknownName(row.name))?;
                Ok(ActiveVersion {
                    name,
                    version: PromptVersion {
                        id: row.id,
                        text: row.text,
                        created_at: row.created_at,
                    },
                })
            })
            .collect()
    }

    /// The newest version of `name`.
    ///
    /// # Errors
    ///
    /// Returns an error when the query fails or `name` has no version.
    pub async fn active(&self, name: PromptName) -> Result<PromptVersion> {
        let name_text = name.as_str();
        sqlx::query_as!(
            PromptVersion,
            r#"
            SELECT
                id,
                text,
                created_at
            FROM
                prompt_versions
            WHERE
                name = ?1
            ORDER BY
                id DESC
            LIMIT
                1
            "#,
            name_text,
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(PromptError::Missing(name))
    }

    /// Version `id` of `name`.
    ///
    /// # Errors
    ///
    /// Returns an error when the query fails or `name` has no version `id`.
    pub async fn version(&self, name: PromptName, id: i64) -> Result<PromptVersion> {
        let name_text = name.as_str();
        sqlx::query_as!(
            PromptVersion,
            r#"
            SELECT
                id,
                text,
                created_at
            FROM
                prompt_versions
            WHERE
                name = ?1
                AND id = ?2
            "#,
            name_text,
            id,
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(PromptError::UnknownVersion(name, id))
    }

    /// Up to `limit` versions of `name`, newest first.
    ///
    /// # Errors
    ///
    /// Returns an error when the query fails.
    pub async fn history(&self, name: PromptName, limit: i64) -> Result<Vec<PromptVersion>> {
        let name_text = name.as_str();
        let versions = sqlx::query_as!(
            PromptVersion,
            r#"
            SELECT
                id,
                text,
                created_at
            FROM
                prompt_versions
            WHERE
                name = ?1
            ORDER BY
                id DESC
            LIMIT
                ?2
            "#,
            name_text,
            limit,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(versions)
    }

    /// Save `text` as the active version of `name` and return its id.
    ///
    /// # Errors
    ///
    /// Returns an error when the insert fails.
    pub async fn save(&self, name: PromptName, text: &str, at: DateTime<Utc>) -> Result<i64> {
        let name_text = name.as_str();
        let created_at = db::timestamp(at);
        let id = sqlx::query!(
            r#"
            INSERT INTO
                prompt_versions (name, text, created_at)
            VALUES
                (?1, ?2, ?3)
            "#,
            name_text,
            text,
            created_at,
        )
        .execute(&self.pool)
        .await?
        .last_insert_rowid();
        info!(prompt = %name, version = id, "Saved prompt");
        Ok(id)
    }
}

/// A parsed `/prompt` argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptCommand {
    /// List every prompt with its active version.
    Overview,
    /// Show the active version, or the one with this id.
    Show(PromptName, Option<i64>),
    Set(PromptName, String),
    History(PromptName),
    /// Save a copy of the version with this id.
    Revert(PromptName, i64),
}

impl PromptCommand {
    /// Parse the text after `/prompt`. The text of `set` keeps its line
    /// breaks.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown prompt name or subcommand, a missing or
    /// malformed argument, or a text over [`MAX_PROMPT_CHARS`].
    pub fn parse(input: &str) -> Result<Self> {
        let (name, rest) = split_word(input);
        if name.is_empty() {
            return Ok(Self::Overview);
        }
        let name = name.parse().map_err(|_| PromptError::Usage)?;
        let (verb, rest) = split_word(rest);
        match verb {
            "" => Ok(Self::Show(name, None)),
            "set" => {
                if rest.is_empty() {
                    return Err(PromptError::Empty);
                }
                let length = rest.chars().count();
                if length > MAX_PROMPT_CHARS {
                    return Err(PromptError::TooLong(length));
                }
                Ok(Self::Set(name, rest.to_owned()))
            }
            "history" if rest.is_empty() => Ok(Self::History(name)),
            "revert" => parse_id(rest).map(|id| Self::Revert(name, id)),
            id if rest.is_empty() => parse_id(id).map(|id| Self::Show(name, Some(id))),
            _ => Err(PromptError::Usage),
        }
    }
}

/// The first word of `input` and the trimmed rest, which keeps its inner
/// line breaks.
fn split_word(input: &str) -> (&str, &str) {
    let input = input.trim();
    input
        .split_once(char::is_whitespace)
        .map_or((input, ""), |(word, rest)| (word, rest.trim()))
}

fn parse_id(input: &str) -> Result<i64> {
    input.parse().map_err(|_| PromptError::Usage)
}

fn usage() -> String {
    let names = PromptName::VARIANTS
        .iter()
        .map(|name| name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Usage:
/prompt: list the prompts and their active versions
/prompt <name> [id]: show the active version or version <id>
/prompt <name> set <text>: save a new version
/prompt <name> history: list recent versions
/prompt <name> revert <id>: save a copy of version <id>
Names: {names}"
    )
}

/// Run a `/prompt` command and render the reply as Telegram HTML.
pub async fn answer_prompt(input: &str, store: &PromptStore, now: DateTime<Utc>) -> String {
    let answered = match PromptCommand::parse(input) {
        Ok(command) => run(command, store, now).await,
        Err(err) => Err(err),
    };
    answered.unwrap_or_else(|err| {
        if matches!(err, PromptError::Database(_) | PromptError::UnknownName(_)) {
            warn!(error = %ErrorChain(&err), "Failed to answer /prompt");
        }
        html::escape(&err.to_string())
    })
}

async fn run(command: PromptCommand, store: &PromptStore, now: DateTime<Utc>) -> Result<String> {
    match command {
        PromptCommand::Overview => Ok(render_overview(&store.active_versions().await?)),
        PromptCommand::Show(name, None) => Ok(render_version(
            &format!("{name}: active version"),
            &store.active(name).await?,
        )),
        PromptCommand::Show(name, Some(id)) => Ok(render_version(
            &format!("{name}: version"),
            &store.version(name, id).await?,
        )),
        PromptCommand::Set(name, text) => {
            let previous = store.active(name).await?;
            let id = store.save(name, &text, now).await?;
            Ok(format!(
                "Saved version {id} of {name}. It applies from the next message.\nUndo: /prompt {name} revert {}",
                previous.id
            ))
        }
        PromptCommand::History(name) => {
            Ok(render_history(&store.history(name, HISTORY_LENGTH).await?))
        }
        PromptCommand::Revert(name, source) => {
            let version = store.version(name, source).await?;
            let id = store.save(name, &version.text, now).await?;
            Ok(format!(
                "Saved version {id} of {name} as a copy of version {source}."
            ))
        }
    }
}

fn render_overview(active: &[ActiveVersion]) -> String {
    let mut text = String::new();
    for name in PromptName::VARIANTS {
        let _ = match active.iter().find(|active| active.name == *name) {
            Some(active) => writeln!(
                text,
                "{name} · version {} · {}",
                active.version.id, active.version.created_at
            ),
            None => writeln!(text, "{name} · missing"),
        };
    }
    text.push_str("Show one with /prompt &lt;name&gt;.");
    text
}

fn render_version(label: &str, version: &PromptVersion) -> String {
    format!(
        "{label} {}, saved {} UTC\n{}",
        version.id,
        version.created_at,
        html::code_block(&version.text)
    )
}

fn render_history(versions: &[PromptVersion]) -> String {
    let mut text = String::new();
    for (index, version) in versions.iter().enumerate() {
        let first_line = version.text.lines().next().unwrap_or_default();
        let preview = html::escape(&truncate_with_ellipsis(first_line, PREVIEW_CHARS));
        let active = if index == 0 { " (active)" } else { "" };
        let _ = writeln!(
            text,
            "{}{active} · {} · {preview}",
            version.id, version.created_at
        );
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use claims::{assert_ok, assert_ok_eq, assert_some};
    use rstest::rstest;
    use std::assert_matches;

    /// A store holding only the seeded versions 1 to 5.
    async fn store() -> PromptStore {
        PromptStore::new(assert_ok!(db::connect("sqlite::memory:").await))
    }

    fn at(hour: u32) -> DateTime<Utc> {
        assert_some!(Utc.with_ymd_and_hms(2026, 11, 17, hour, 0, 0).single())
    }

    #[rstest]
    #[case::overview("", PromptCommand::Overview)]
    #[case::show_active("persona", PromptCommand::Show(PromptName::Persona, None))]
    #[case::show_version(
        " reply_rules 3 ",
        PromptCommand::Show(PromptName::ReplyRules, Some(3))
    )]
    #[case::history(
        "caption_task history",
        PromptCommand::History(PromptName::CaptionTask)
    )]
    #[case::revert("persona revert 2", PromptCommand::Revert(PromptName::Persona, 2))]
    #[case::set_keeps_line_breaks(
        "caption_rules set - Be calm.\n\n- Never swear.\n",
        PromptCommand::Set(PromptName::CaptionRules, "- Be calm.\n\n- Never swear.".to_owned())
    )]
    fn parses_subcommands(#[case] input: &str, #[case] expected: PromptCommand) {
        assert_ok_eq!(PromptCommand::parse(input), expected);
    }

    #[rstest]
    #[case::unknown_name("rules")]
    #[case::unknown_verb("persona reset")]
    #[case::bad_id("persona abc")]
    #[case::revert_without_target("persona revert")]
    #[case::history_with_argument("persona history 3")]
    fn rejects_malformed_arguments(#[case] input: &str) {
        assert_matches!(PromptCommand::parse(input), Err(PromptError::Usage));
    }

    #[test]
    fn rejects_empty_and_oversized_texts() {
        assert_matches!(
            PromptCommand::parse("persona set  "),
            Err(PromptError::Empty)
        );
        let long = format!("persona set {}", "a".repeat(MAX_PROMPT_CHARS + 1));
        assert_matches!(PromptCommand::parse(&long), Err(PromptError::TooLong(_)));
    }

    #[tokio::test]
    async fn the_migration_seeds_every_prompt() {
        let prompts = assert_ok!(store().await.active_set().await);
        assert!(
            prompts.persona.starts_with("Guenther is"),
            "{}",
            prompts.persona
        );
        assert!(
            prompts.caption_rules.starts_with("- "),
            "{}",
            prompts.caption_rules
        );
    }

    #[tokio::test]
    async fn set_activates_the_text_for_its_name_only() {
        let store = store().await;
        let seeded = assert_ok!(store.active_set().await);

        let reply = answer_prompt("persona set Calm Guenther", &store, at(10)).await;

        assert_eq!(
            reply,
            "Saved version 6 of persona. It applies from the next message.\nUndo: /prompt persona revert 3"
        );
        let prompts = assert_ok!(store.active_set().await);
        assert_eq!(prompts.persona, "Calm Guenther");
        assert_eq!(prompts.caption_rules, seeded.caption_rules);
    }

    #[tokio::test]
    async fn revert_saves_a_copy_of_the_old_version() {
        let store = store().await;
        let calm = assert_ok!(
            store
                .save(PromptName::Persona, "Calm Guenther", at(10))
                .await
        );
        assert_ok!(
            store
                .save(PromptName::Persona, "Angry Guenther", at(11))
                .await
        );

        let reply = answer_prompt(&format!("persona revert {calm}"), &store, at(12)).await;

        assert_eq!(reply, "Saved version 8 of persona as a copy of version 6.");
        assert_ok_eq!(
            store
                .active(PromptName::Persona)
                .await
                .map(|version| version.text),
            "Calm Guenther"
        );
    }

    #[tokio::test]
    async fn versions_of_another_prompt_are_not_found() {
        let store = store().await;
        let rules = assert_ok!(
            store
                .save(PromptName::ReplyRules, "- Be calm.", at(10))
                .await
        );

        assert_eq!(
            answer_prompt(&format!("persona revert {rules}"), &store, at(11)).await,
            "Version 6 of `persona` does not exist."
        );
        assert_eq!(
            answer_prompt("persona 99", &store, at(11)).await,
            "Version 99 of `persona` does not exist."
        );
    }

    #[tokio::test]
    async fn show_escapes_the_text_for_telegram_html() {
        let store = store().await;
        assert_ok!(
            store
                .save(PromptName::Persona, "He hates <b> & </b>", at(10))
                .await
        );

        assert_eq!(
            answer_prompt("persona", &store, at(11)).await,
            "persona: active version 6, saved 2026-11-17 10:00:00 UTC\n<pre>He hates &lt;b&gt; &amp; &lt;/b&gt;</pre>"
        );
    }

    #[tokio::test]
    async fn history_lists_newest_first_and_marks_the_active_version() {
        let store = store().await;
        assert_ok!(
            store
                .save(PromptName::Persona, "Calm Guenther\nSecond line", at(10))
                .await
        );
        assert_ok!(
            store
                .save(PromptName::ReplyRules, "- Be calm.", at(10))
                .await
        );
        assert_ok!(
            store
                .save(PromptName::Persona, "Angry Guenther", at(11))
                .await
        );

        let history = answer_prompt("persona history", &store, at(12)).await;

        assert_eq!(
            history.lines().take(2).collect::<Vec<_>>(),
            [
                "8 (active) · 2026-11-17 11:00:00 · Angry Guenther",
                "6 · 2026-11-17 10:00:00 · Calm Guenther",
            ]
        );
        assert_eq!(history.lines().count(), 3);
    }

    #[tokio::test]
    async fn overview_lists_the_active_version_of_every_prompt() {
        let store = store().await;
        assert_ok!(
            store
                .save(PromptName::ReplyRules, "- Be calm.", at(10))
                .await
        );

        let overview = answer_prompt("", &store, at(11)).await;

        assert!(
            overview.contains("reply_rules · version 6 · 2026-11-17 10:00:00\n"),
            "{overview}"
        );
        assert_eq!(overview.lines().count(), PromptName::VARIANTS.len() + 1);
        assert!(
            overview.ends_with("Show one with /prompt &lt;name&gt;."),
            "{overview}"
        );
    }
}
