use serde::{Deserialize, Serialize};

/// Stores image metadata separately from the raw bytes held in KV.
#[derive(Serialize, Deserialize)]
pub(crate) struct ImageRecord {
    /// Original URL used to refresh the image.
    pub(crate) source: String,
    /// Refresh interval in hours; zero means upload once.
    pub(crate) frequency_hours: u64,
    /// Timestamp of the last successful upstream fetch.
    #[serde(alias = "fetched_at_ms", default)]
    pub(crate) last_success_ms: u64,
    /// Timestamp of the first failed refresh, when applicable.
    #[serde(default)]
    pub(crate) dead_since_ms: Option<u64>,
    /// MIME type used when serving the stored bytes.
    pub(crate) content_type: String,
}
