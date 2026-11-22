pub mod branches;
pub mod bulk;
pub mod conflicts;
pub mod diff;
pub mod git_flow;
pub mod history;
pub mod models;
pub mod remotes;
pub mod runner;
pub mod status;
pub mod submodules;
pub mod tags;
pub mod worktrees;

use gitfleet_core::{AppResult, CommandLogStore, EventBus};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

pub use branches::BranchInfo;
pub use conflicts::{
    ConflictHunk, ConflictSegment, ConflictStrategy, HunkChoice, MergeMode, ParsedConflictFile,
    has_conflict_markers,
};
pub use diff::{
    DiffBlock, DiffChangeBlock, DiffChangeKind, DiffContextLine, DiffHunk, DiffStats,
    FileVersionPair, ParsedDiff, diff_two_texts, parse_unified_diff,
};
pub use git_flow::{GitFlowKind, GitFlowPrefixes, GitFlowState, normalize_flow_prefix};
pub use models::*;
pub use remotes::RemoteInfo;
pub use status::{delete_workspace_entry, list_ignored_dir_children, rename_workspace_entry};
pub use submodules::SubmoduleInfo;
pub use tags::{ReleaseTrigger, TagInfo};
pub use worktrees::WorktreeInfo;

/// Core Git engine facade. Feature implementations live in their respective domain modules.
pub struct GitService {
    pub(crate) status_cache: RwLock<HashMap<PathBuf, RepoStatusSummary>>,
    pub(crate) events: EventBus,
    pub(crate) command_log: Arc<CommandLogStore>,
}

impl GitService {
    pub fn new(events: EventBus, command_log: Arc<CommandLogStore>) -> Self {
        Self {
            status_cache: RwLock::new(HashMap::new()),
            events,
            command_log,
        }
    }

    pub fn cached_statuses(&self) -> HashMap<PathBuf, RepoStatusSummary> {
        self.status_cache
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub async fn init_repo(&self, dest: &Path) -> AppResult<String> {
        std::fs::create_dir_all(dest)?;
        runner::run_git_ok(
            dest,
            &["init"],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn clone_repo(
        &self,
        url: &str,
        parent_dir: &Path,
        folder_name: &str,
        recurse_submodules: bool,
    ) -> AppResult<PathBuf> {
        let dest = parent_dir.join(folder_name);
        let dest_str = dest.to_string_lossy().to_string();
        let mut args = vec!["clone"];
        if recurse_submodules {
            args.push("--recurse-submodules");
        }
        args.push(url);
        args.push(&dest_str);

        runner::run_git_ok(
            parent_dir,
            &args,
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await?;
        Ok(dest)
    }

    pub async fn git_version(&self) -> String {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        runner::run_git(&cwd, &["--version"], None, None, false)
            .await
            .map(|o| o.stdout.trim().to_string())
            .unwrap_or_else(|_| "git not found".to_string())
    }
}
