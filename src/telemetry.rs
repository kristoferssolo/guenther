use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    io::{self, IsTerminal},
};
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

/// Displays an error followed by its `source()` chain, so log lines keep
/// causes such as `Connection refused` that reqwest and teloxide only expose
/// as sources.
///
/// Sources whose message the text so far already contains are skipped, since
/// `#[error("...: {0}")]` variants embed their source.
///
/// ```
/// # use guenther::telemetry::ErrorChain;
/// # let err = std::io::Error::other("boom");
/// tracing::warn!(error = %ErrorChain(&err), "Something failed");
/// ```
pub struct ErrorChain<'a>(pub &'a (dyn Error + 'static));

impl Display for ErrorChain<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut text = self.0.to_string();
        let mut source = self.0.source();
        while let Some(err) = source {
            let message = err.to_string();
            if !text.contains(&message) {
                text.push_str(": ");
                text.push_str(&message);
            }
            source = err.source();
        }
        f.write_str(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use thiserror::Error;

    #[derive(Debug, Error)]
    #[error("connection refused")]
    struct Refused;

    #[derive(Debug, Error)]
    #[error("error sending request")]
    struct Transport(#[source] Refused);

    #[derive(Debug, Error)]
    enum Outer {
        #[error("Cobalt request failed: {0}")]
        Embedded(#[source] Transport),
        #[error(transparent)]
        Transparent(Transport),
    }

    #[test]
    fn appends_causes_the_message_does_not_embed() {
        let err = Outer::Embedded(Transport(Refused));
        assert_eq!(
            ErrorChain(&err).to_string(),
            "Cobalt request failed: error sending request: connection refused"
        );
    }

    #[test]
    fn transparent_errors_are_not_repeated() {
        let err = Outer::Transparent(Transport(Refused));
        assert_eq!(
            ErrorChain(&err).to_string(),
            "error sending request: connection refused"
        );
    }
}
