use std::io::{self, IsTerminal};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// Initialise tracing
pub fn setup_logger() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    let formatter = tracing_subscriber::fmt::Layer::default().with_ansi(io::stdout().is_terminal());

    tracing_subscriber::registry()
        .with(env_filter)
        .with(formatter)
        .init();
}
