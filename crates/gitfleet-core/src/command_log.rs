use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandLogEntry {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub repo_path: Option<String>,
    pub command: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout_preview: Option<String>,
    pub stderr_preview: Option<String>,
}

#[derive(Default)]
pub struct CommandLogStore {
    entries: RwLock<Vec<CommandLogEntry>>,
}

impl CommandLogStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, entry: CommandLogEntry) {
        if let Ok(mut guard) = self.entries.write() {
            guard.push(entry);
        }
    }

    pub fn list(&self) -> Vec<CommandLogEntry> {
        self.entries
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn clear(&self) {
        if let Ok(mut guard) = self.entries.write() {
            guard.clear();
        }
    }

    pub fn export_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.list())
    }
}
