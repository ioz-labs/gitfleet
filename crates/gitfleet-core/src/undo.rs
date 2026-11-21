use crate::RepoId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoEntry {
    pub id: String,
    pub repo_id: RepoId,
    pub label: String,
    pub previous_head: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Default)]
pub struct UndoStack {
    by_repo: RwLock<HashMap<RepoId, Vec<UndoEntry>>>,
}

impl UndoStack {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&self, entry: UndoEntry) {
        if let Ok(mut guard) = self.by_repo.write() {
            guard.entry(entry.repo_id.clone()).or_default().push(entry);
        }
    }

    pub fn list(&self, repo_id: &RepoId) -> Vec<UndoEntry> {
        self.by_repo
            .read()
            .ok()
            .and_then(|g| g.get(repo_id).cloned())
            .unwrap_or_default()
    }

    pub fn pop(&self, repo_id: &RepoId) -> Option<UndoEntry> {
        self.by_repo
            .write()
            .ok()
            .and_then(|mut g| g.get_mut(repo_id).and_then(|v| v.pop()))
    }
}
