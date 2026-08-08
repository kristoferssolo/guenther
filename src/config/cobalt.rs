use super::get_string_from_env;

#[derive(Debug, Clone)]
pub struct CobaltConfig {
    pub api_url: String,
    pub api_key: Option<String>,
}

impl CobaltConfig {
    const DEFAULT_API_URL: &'static str = "http://127.0.0.1:9000/";

    pub(super) fn from_env() -> Self {
        Self {
            api_url: get_string_from_env("COBALT_API_URL")
                .unwrap_or_else(|| Self::DEFAULT_API_URL.to_owned()),
            api_key: get_string_from_env("COBALT_API_KEY"),
        }
    }
}

impl Default for CobaltConfig {
    fn default() -> Self {
        Self {
            api_url: Self::DEFAULT_API_URL.to_owned(),
            api_key: None,
        }
    }
}
