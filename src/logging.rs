// Logging setup. The rest of the code only uses the `tracing` macros
// (`tracing::info!(...)` etc.); this decides where the logs go: stdout, one
// JSON object per line, which log platforms parse into searchable fields.
// Shipping them elsewhere (Scaleway Cockpit collects stdout) is the platform's
// job, not the app's.

use tracing_subscriber::filter::Targets;
use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Config;

/// Installs the global logger. Must be called once, before anything logs.
///
/// # Errors
/// Returns an error if `log_filter` is invalid.
pub fn init(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    // A default level plus optional per-crate levels, e.g. `info,rmcp=warn`.
    let filter: Targets = config.log_filter.parse()?;
    let stdout = tracing_subscriber::fmt::layer()
        .json()
        // Put the event's fields at the top level, next to `message` to make search easier in Loki.
        .flatten_event(true)
        .with_filter(filter);
    tracing_subscriber::registry().with(stdout).try_init()?;
    Ok(())
}
