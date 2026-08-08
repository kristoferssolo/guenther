use claims::{assert_none, assert_some};
use guenther_core::config::{Config, Platform};
use temp_env::with_vars;

fn with_clean_config_env<T>(f: impl FnOnce() -> T) -> T {
    with_vars(
        [
            ("CHAT_ID", None::<&str>),
            ("COBALT_API_URL", None),
            ("COBALT_API_KEY", None),
            ("ENABLED_PLATFORMS", None),
            ("F1_UTC_OFFSET", None),
        ],
        f,
    )
}

#[test]
fn from_env_sets_chat_id_when_valid() {
    with_clean_config_env(|| {
        with_vars([("CHAT_ID", Some("12345"))], || {
            let cfg = Config::from_env();
            let chat_id = assert_some!(cfg.chat_id);
            assert_eq!(chat_id, 12345);
        });
    });
}

#[test]
fn from_env_uses_none_when_chat_id_invalid() {
    with_clean_config_env(|| {
        with_vars([("CHAT_ID", Some("invalid"))], || {
            let cfg = Config::from_env();
            assert_none!(cfg.chat_id);
        });
    });
}

#[test]
fn from_env_uses_none_when_chat_id_missing() {
    with_clean_config_env(|| {
        let cfg = Config::from_env();
        assert_none!(cfg.chat_id);
    });
}

#[test]
fn from_env_sets_cobalt_configuration() {
    with_clean_config_env(|| {
        with_vars(
            [
                ("COBALT_API_URL", Some("https://cobalt.example/")),
                ("COBALT_API_KEY", Some("secret-key")),
            ],
            || {
                let cfg = Config::from_env();
                assert_eq!(cfg.cobalt.api_url, "https://cobalt.example/");
                assert_eq!(cfg.cobalt.api_key.as_deref(), Some("secret-key"));
            },
        );
    });
}

#[test]
fn from_env_uses_default_cobalt_configuration() {
    with_clean_config_env(|| {
        let cfg = Config::from_env();
        assert_eq!(cfg.cobalt.api_url, "http://127.0.0.1:9000/");
        assert_none!(cfg.cobalt.api_key);
    });
}

#[test]
fn from_env_ignores_empty_cobalt_api_key() {
    with_clean_config_env(|| {
        with_vars([("COBALT_API_KEY", Some("  "))], || {
            let cfg = Config::from_env();
            assert_none!(cfg.cobalt.api_key);
        });
    });
}

#[test]
fn from_env_enables_all_platforms_by_default() {
    with_clean_config_env(|| {
        let platforms = Config::from_env().platforms;
        for platform in Platform::ALL {
            assert!(platforms.is_enabled(platform));
        }
    });
}

#[test]
fn from_env_enables_selected_platforms() {
    with_clean_config_env(|| {
        with_vars([("ENABLED_PLATFORMS", Some(" Instagram, X "))], || {
            let platforms = Config::from_env().platforms;
            assert!(platforms.is_enabled(Platform::Instagram));
            assert!(platforms.is_enabled(Platform::Twitter));
            assert!(!platforms.is_enabled(Platform::Tiktok));
            assert!(!platforms.is_enabled(Platform::Youtube));
        });
    });
}

#[test]
fn from_env_disables_all_platforms_when_empty() {
    with_clean_config_env(|| {
        with_vars([("ENABLED_PLATFORMS", Some(""))], || {
            let platforms = Config::from_env().platforms;
            for platform in Platform::ALL {
                assert!(!platforms.is_enabled(platform));
            }
        });
    });
}

#[test]
fn from_env_sets_f1_utc_offset_when_valid() {
    with_clean_config_env(|| {
        with_vars([("F1_UTC_OFFSET", Some("+3"))], || {
            let cfg = Config::from_env();
            assert_eq!(cfg.f1.utc_offset.whole_seconds(), 10_800);
        });
    });
}

#[test]
fn from_env_uses_utc_when_f1_utc_offset_invalid() {
    with_clean_config_env(|| {
        with_vars([("F1_UTC_OFFSET", Some("wat"))], || {
            let cfg = Config::from_env();
            assert_eq!(cfg.f1.utc_offset.whole_seconds(), 0);
        });
    });
}
