use crate::{GitService, runner};
use gitfleet_core::AppResult;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeInfo {
    pub name: String,
    pub path: PathBuf,
    pub branch: Option<String>,
    pub is_main: bool,
    pub dirty: bool,
    pub ahead: usize,
    pub behind: usize,
}

impl GitService {
    pub async fn get_worktrees(&self, repo_path: &Path) -> AppResult<Vec<WorktreeInfo>> {
        let out = runner::run_git_ok(
            repo_path,
            &["worktree", "list", "--porcelain"],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;

        let mut list = Vec::new();
        let mut cur_path: Option<PathBuf> = None;
        let mut cur_branch: Option<String> = None;

        for line in out.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                if let Some(prev) = cur_path.take() {
                    let name = prev
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("worktree")
                        .to_string();
                    list.push(WorktreeInfo {
                        name,
                        is_main: list.is_empty(),
                        path: prev,
                        branch: cur_branch.take(),
                        dirty: false,
                        ahead: 0,
                        behind: 0,
                    });
                }
                cur_path = Some(PathBuf::from(p));
            } else if let Some(b) = line.strip_prefix("branch ") {
                cur_branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
            }
        }
        if let Some(prev) = cur_path {
            let name = prev
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("worktree")
                .to_string();
            list.push(WorktreeInfo {
                name,
                is_main: list.is_empty(),
                path: prev,
                branch: cur_branch,
                dirty: false,
                ahead: 0,
                behind: 0,
            });
        }

        for wt in &mut list {
            if wt.path.exists()
                && let Ok(st) = self.get_status(&wt.path).await
            {
                wt.dirty = !st.is_clean;
                wt.ahead = st.ahead;
                wt.behind = st.behind;
            }
        }
        Ok(list)
    }

    pub async fn add_worktree(
        &self,
        repo_path: &Path,
        dest_path: &Path,
        branch: &str,
        create_new: bool,
    ) -> AppResult<String> {
        let dest_str = dest_path.to_string_lossy().to_string();
        let args: Vec<&str> = if create_new {
            vec!["worktree", "add", "-b", branch, &dest_str]
        } else {
            vec!["worktree", "add", &dest_str, branch]
        };
        runner::run_git_ok(
            repo_path,
            &args,
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn remove_worktree(
        &self,
        repo_path: &Path,
        worktree_path: &Path,
        force: bool,
    ) -> AppResult<String> {
        let wt_str = worktree_path.to_string_lossy().to_string();
        let args: Vec<&str> = if force {
            vec!["worktree", "remove", "--force", &wt_str]
        } else {
            vec!["worktree", "remove", &wt_str]
        };
        runner::run_git_ok(
            repo_path,
            &args,
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }
}
