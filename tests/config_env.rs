use claims::{assert_err, assert_ok_eq};
use guenther::{
    config::{Config, Platform},
    db::DEFAULT_DATABASE_URL,
};
use rstest::rstest;
use strum::VariantArray;
use temp_env::with_vars;

const DEFAULT_COBALT_API_URL: &str = "http://127.0.0.1:9000/";

fn with_clean_config_env<T>(f: impl FnOnce() -> T) -> T {
    with_vars(
        [
            ("CHAT_ID", None::<&str>),
            ("COBALT_API_URL", None),
            ("COBALT_API_KEY", None),
            ("ENABLED_PLATFORMS", None),
            ("F1_UTC_OFFSET", None),
            ("DATABASE_URL", None),
            ("OPENROUTER_API_KEY", None),
            ("OPENROUTER_MODEL", None),
            ("OPENROUTER_VIDEO_INPUT", None),
            ("FFMPEG_BIN", None),
            ("FFPROBE_BIN", None),
        ],
        f,
    )
}

fn config_with(vars: &[(&str, Option<&str>)]) -> Config {
    with_clean_config_env(|| with_vars(vars, Config::from_env))
}

#[rstest]
#[case::valid(Some("12345"), Some(12345))]
#[case::padded(Some(" 123 "), Some(123))]
#[case::invalid(Some("invalid"), None)]
#[case::empty(Some(""), None)]
#[case::missing(None, None)]
fn chat_id(#[case] raw: Option<&str>, #[case] expected: Option<i64>) {
    assert_eq!(config_with(&[("CHAT_ID", raw)]).chat_id, expected);
}

#[rstest]
#[case::set(
    Some("https://cobalt.example/"),
    Some("secret-key"),
    "https://cobalt.example/",
    Some("secret-key")
)]
#[case::defaults(None, None, DEFAULT_COBALT_API_URL, None)]
#[case::whitespace_key(None, Some("  "), DEFAULT_COBALT_API_URL, None)]
fn cobalt(
    #[case] url: Option<&str>,
    #[case] key: Option<&str>,
    #[case] expected_url: &str,
    #[case] expected_key: Option<&str>,
) {
    let cobalt = config_with(&[("COBALT_API_URL", url), ("COBALT_API_KEY", key)]).cobalt;
    assert_eq!(cobalt.api_url, expected_url);
    assert_eq!(cobalt.api_key.as_deref(), expected_key);
}

#[rstest]
#[case::set(Some("sqlite://custom.sqlite3"), "sqlite://custom.sqlite3")]
#[case::default(None, DEFAULT_DATABASE_URL)]
fn database_url(#[case] raw: Option<&str>, #[case] expected: &str) {
    assert_eq!(config_with(&[("DATABASE_URL", raw)]).database_url, expected);
}

#[rstest]
#[case::unset_enables_all(None, Platform::VARIANTS)]
#[case::empty_disables_all(Some(""), &[])]
#[case::selection(Some(" Instagram, X "), &[Platform::Instagram, Platform::Twitter])]
#[case::all(Some("all"), Platform::VARIANTS)]
#[case::unknown_skipped(Some("mastodon, tiktok"), &[Platform::Tiktok])]
fn enabled_platforms(#[case] raw: Option<&str>, #[case] expected: &[Platform]) {
    let platforms = config_with(&[("ENABLED_PLATFORMS", raw)]).platforms;
    let enabled = Platform::VARIANTS
        .iter()
        .copied()
        .filter(|&platform| platforms.is_enabled(platform))
        .collect::<Vec<_>>();
    assert_eq!(enabled, expected);
}

#[test]
fn platforms_parse_and_display_with_standard_traits() {
    assert_ok_eq!("Instagram".parse::<Platform>(), Platform::Instagram);
    assert_ok_eq!("x".parse::<Platform>(), Platform::Twitter);
    assert_err!("mastodon".parse::<Platform>());
    assert_eq!(Platform::Youtube.to_string(), "youtube");
}

#[rstest]
#[case::hours(Some("+3"), 10_800)]
#[case::padded_hours(Some("+03"), 10_800)]
#[case::hours_minutes(Some("+03:00"), 10_800)]
#[case::negative_minutes(Some("-5:30"), -19_800)]
#[case::invalid(Some("wat"), 0)]
#[case::empty(Some(""), 0)]
#[case::missing(None, 0)]
fn f1_utc_offset(#[case] raw: Option<&str>, #[case] expected_seconds: i32) {
    let offset = config_with(&[("F1_UTC_OFFSET", raw)]).f1.utc_offset;
    assert_eq!(offset.local_minus_utc(), expected_seconds);
}

#[cfg(feature = "llm-captions")]
#[rstest]
#[case::both_set(Some("sk-or-key"), Some(" z-ai/glm-5.3-flash "), true)]
#[case::missing_model(Some("sk-or-key"), None, false)]
#[case::missing_key(None, Some("z-ai/glm-5.3-flash"), false)]
#[case::blank_key(Some("  "), Some("z-ai/glm-5.3-flash"), false)]
#[case::unset(None, None, false)]
fn openrouter_needs_key_and_model(
    #[case] key: Option<&str>,
    #[case] model: Option<&str>,
    #[case] enabled: bool,
) {
    let openrouter =
        config_with(&[("OPENROUTER_API_KEY", key), ("OPENROUTER_MODEL", model)]).openrouter;
    assert_eq!(openrouter.is_some(), enabled);
    if let Some(openrouter) = openrouter {
        assert_eq!(openrouter.api_key, "sk-or-key");
        assert_eq!(openrouter.model, "z-ai/glm-5.3-flash");
        assert!(!openrouter.video_input);
        assert_eq!(openrouter.ffmpeg, std::path::Path::new("ffmpeg"));
        assert_eq!(openrouter.ffprobe, std::path::Path::new("ffprobe"));
    }
}

#[cfg(feature = "llm-captions")]
#[rstest]
#[case::true_word(Some("true"), true)]
#[case::one(Some("1"), true)]
#[case::upper_on(Some("ON"), true)]
#[case::false_word(Some("false"), false)]
#[case::garbage(Some("maybe"), false)]
#[case::unset(None, false)]
fn openrouter_video_input_flag(#[case] raw: Option<&str>, #[case] expected: bool) {
    let openrouter = config_with(&[
        ("OPENROUTER_API_KEY", Some("sk-or-key")),
        ("OPENROUTER_MODEL", Some("z-ai/glm-5.3-flash")),
        ("OPENROUTER_VIDEO_INPUT", raw),
    ])
    .openrouter;
    assert_eq!(openrouter.map(|config| config.video_input), Some(expected));
}

#[cfg(feature = "llm-captions")]
#[test]
fn openrouter_debug_redacts_api_key() {
    let openrouter = config_with(&[
        ("OPENROUTER_API_KEY", Some("sk-or-secret")),
        ("OPENROUTER_MODEL", Some("z-ai/glm-5.3-flash")),
    ])
    .openrouter;
    let debug = format!("{openrouter:?}");
    assert!(!debug.contains("sk-or-secret"));
    assert!(debug.contains("z-ai/glm-5.3-flash"));
}
