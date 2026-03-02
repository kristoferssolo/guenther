use claims::{assert_none, assert_some};
use guenther::config::Config;
use std::{
    fs,
    path::{Path, PathBuf},
};
use teloxide::types::ChatId;
use temp_env::with_vars;
use tempfile::tempdir;

fn with_clean_config_env<T>(f: impl FnOnce() -> T) -> T {
    with_vars(
        [
            ("CHAT_ID", None::<&str>),
            ("YOUTUBE_SESSION_COOKIE_PATH", None),
            ("IG_SESSION_COOKIE_PATH", None),
            ("TIKTOK_SESSION_COOKIE_PATH", None),
            ("TWITTER_SESSION_COOKIE_PATH", None),
            ("YOUTUBE_POSTPROCESSOR_ARGS", None),
        ],
        f,
    )
}

fn write_temp_cookie_file(dir: &Path, name: &str) -> PathBuf {
    let file_path = dir.join(name);
    fs::write(&file_path, "session=example\n").expect("write cookie fixture");
    file_path
}

#[test]
fn from_env_sets_chat_id_when_valid() {
    with_clean_config_env(|| {
        with_vars([("CHAT_ID", Some("12345"))], || {
            let cfg = Config::from_env();
            let chat_id = assert_some!(cfg.chat_id);
            assert_eq!(chat_id, ChatId(12345));
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
fn from_env_sets_youtube_cookie_path_when_file_exists() {
    let tmp = tempdir().expect("create temp dir");
    let cookie = write_temp_cookie_file(tmp.path(), "yt.cookies");
    let cookie_str = cookie.to_string_lossy().into_owned();

    with_clean_config_env(|| {
        with_vars(
            [("YOUTUBE_SESSION_COOKIE_PATH", Some(cookie_str.as_str()))],
            || {
                let cfg = Config::from_env();
                let yt_cookie = assert_some!(cfg.youtube.cookies_path);
                assert_eq!(yt_cookie, cookie);
            },
        );
    });
}

#[test]
fn from_env_uses_none_when_cookie_path_missing() {
    with_clean_config_env(|| {
        let cfg = Config::from_env();
        assert_none!(cfg.youtube.cookies_path);
    });
}

#[test]
fn from_env_uses_none_when_cookie_path_is_not_a_file() {
    let tmp = tempdir().expect("create temp dir");
    let dir_path = tmp.path().join("not-a-file");
    fs::create_dir(&dir_path).expect("create directory fixture");
    let dir_str = dir_path.to_string_lossy().into_owned();

    with_clean_config_env(|| {
        with_vars(
            [("YOUTUBE_SESSION_COOKIE_PATH", Some(dir_str.as_str()))],
            || {
                let cfg = Config::from_env();
                assert_none!(cfg.youtube.cookies_path);
            },
        );
    });
}
