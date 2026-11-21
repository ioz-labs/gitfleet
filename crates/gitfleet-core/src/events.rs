use crate::{CommandLogEntry, RepoId, SessionId};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// Unified application event stream (replaces Electron outbound IPC events).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AppEvent {
    RepoChanged { repo_id: RepoId },
    StatusBatchUpdated { repo_ids: Vec<RepoId> },
    WorkspaceDiscovery { found: usize, finished: bool },
    WorkspaceOwnershipChanged { owner_pid: Option<u32>, is_owner: bool },
    OperationProgress { token: String, percent: u8, message: String },
    SubmoduleAddProgress { repo_id: RepoId, path: String, stage: String },
    SearchResult { run_id: String, repo_id: RepoId, file: String, line: usize, preview: String },
    SearchFinished { run_id: String, total_matches: usize },
    TerminalData { session_id: SessionId, chunk: String },
    TerminalExit { session_id: SessionId, exit_code: Option<i32> },
    CommandLogAppended(CommandLogEntry),
    UndoStackChanged { repo_id: RepoId },
    RemoteRefresh { running: bool, updated_repos: usize },
    ViewerTargetChanged { repo_id: RepoId, file_path: String, git_ref: Option<String> },
    ViewerStateChanged { is_open: bool },
}

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<AppEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    pub fn publish(&self, event: AppEvent) {
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.tx.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(512)
    }
}
