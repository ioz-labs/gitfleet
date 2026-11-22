use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub theme: String,
    pub background_image: Option<PathBuf>,
    pub external_diff_tool: Option<String>,
    pub external_merge_tool: Option<String>,
    pub external_terminal: Option<String>,
    #[serde(default)]
    pub default_terminal_profile: Option<String>,
    #[serde(default)]
    pub terminal_font_family: Option<String>,
    #[serde(default = "default_terminal_font_size")]
    pub terminal_font_size: u16,
    #[serde(default)]
    pub scheduled_fetch_enabled: bool,
    pub auto_fetch_interval_secs: Option<u64>,
    pub max_parallel_bulk_ops: usize,
    #[serde(default = "default_oversized_mb")]
    pub oversized_threshold_mb: u64,
    #[serde(default = "default_true")]
    pub watch_filesystem: bool,
    #[serde(default = "default_true")]
    pub auto_check_updates: bool,
    #[serde(default)]
    pub skipped_update_version: Option<String>,
    #[serde(default)]
    pub last_update_check_epoch_secs: Option<i64>,
    #[serde(default)]
    pub last_update_etag: Option<String>,
    #[serde(default)]
    pub last_update_etag_version: Option<String>,
}

fn default_oversized_mb() -> u64 {
    5
}

fn default_terminal_font_size() -> u16 {
    13
}

fn default_true() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            background_image: None,
            external_diff_tool: None,
            external_merge_tool: None,
            external_terminal: None,
            default_terminal_profile: None,
            terminal_font_family: None,
            terminal_font_size: default_terminal_font_size(),
            scheduled_fetch_enabled: false,
            auto_fetch_interval_secs: Some(300),
            max_parallel_bulk_ops: 6,
            oversized_threshold_mb: 5,
            watch_filesystem: true,
            auto_check_updates: true,
            skipped_update_version: None,
            last_update_check_epoch_secs: None,
            last_update_etag: None,
            last_update_etag_version: None,
        }
    }
}
