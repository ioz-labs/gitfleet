use crate::models::ForecastResult;
use crate::{GitService, runner};
use gitfleet_core::AppResult;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ConflictStrategy {
    Ours,
    Theirs,
    MarkResolved,
}

impl GitService {
    /// In-memory merge forecast using `git merge-tree --write-tree` (does not touch working tree).
    pub async fn merge_forecast(
        &self,
        repo_path: &Path,
        target_branch: &str,
    ) -> AppResult<ForecastResult> {
        let out = runner::run_git(
            repo_path,
            &["merge-tree", "--write-tree", "--name-only", "HEAD", target_branch],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;

        if out.exit_code == 0 {
            return Ok(ForecastResult {
                clean: true,
                conflicting_paths: Vec::new(),
            });
        }

        let mut conflicts = Vec::new();
        for line in out.stdout.lines().skip(1) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                break;
            }
            conflicts.push(trimmed.to_string());
        }
        Ok(ForecastResult {
            clean: false,
            conflicting_paths: conflicts,
        })
    }

    pub async fn merge_branch(&self, repo_path: &Path, target_branch: &str) -> AppResult<String> {
        runner::run_git_ok(
            repo_path,
            &["merge", target_branch],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn merge_ff_only(&self, repo_path: &Path, target_branch: &str) -> AppResult<String> {
        runner::run_git_ok(
            repo_path,
            &["merge", "--ff-only", target_branch],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn rebase_branch(&self, repo_path: &Path, target_branch: &str) -> AppResult<String> {
        runner::run_git_ok(
            repo_path,
            &["rebase", target_branch],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn resolve_conflict(
        &self,
        repo_path: &Path,
        file_path: &str,
        strategy: ConflictStrategy,
    ) -> AppResult<String> {
        match strategy {
            ConflictStrategy::Ours => {
                runner::run_git_ok(
                    repo_path,
                    &["checkout", "--ours", "--", file_path],
                    Some(&self.command_log),
                    Some(&self.events),
                    true,
                )
                .await?;
            }
            ConflictStrategy::Theirs => {
                runner::run_git_ok(
                    repo_path,
                    &["checkout", "--theirs", "--", file_path],
                    Some(&self.command_log),
                    Some(&self.events),
                    true,
                )
                .await?;
            }
            ConflictStrategy::MarkResolved => {}
        }
        runner::run_git_ok(
            repo_path,
            &["add", "--", file_path],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn continue_operation(&self, repo_path: &Path) -> AppResult<String> {
        let git_dir = repo_path.join(".git");
        if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
            runner::run_git_ok(
                repo_path,
                &["rebase", "--continue"],
                Some(&self.command_log),
                Some(&self.events),
                true,
            )
            .await
        } else if git_dir.join("CHERRY_PICK_HEAD").exists() {
            runner::run_git_ok(
                repo_path,
                &["cherry-pick", "--continue"],
                Some(&self.command_log),
                Some(&self.events),
                true,
            )
            .await
        } else if git_dir.join("REVERT_HEAD").exists() {
            runner::run_git_ok(
                repo_path,
                &["revert", "--continue"],
                Some(&self.command_log),
                Some(&self.events),
                true,
            )
            .await
        } else {
            runner::run_git_ok(
                repo_path,
                &["merge", "--continue"],
                Some(&self.command_log),
                Some(&self.events),
                true,
            )
            .await
        }
    }

    pub async fn abort_operation(&self, repo_path: &Path) -> AppResult<String> {
        let git_dir = repo_path.join(".git");
        if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
            runner::run_git_ok(
                repo_path,
                &["rebase", "--abort"],
                Some(&self.command_log),
                Some(&self.events),
                true,
            )
            .await
        } else if git_dir.join("CHERRY_PICK_HEAD").exists() {
            runner::run_git_ok(
                repo_path,
                &["cherry-pick", "--abort"],
                Some(&self.command_log),
                Some(&self.events),
                true,
            )
            .await
        } else if git_dir.join("REVERT_HEAD").exists() {
            runner::run_git_ok(
                repo_path,
                &["revert", "--abort"],
                Some(&self.command_log),
                Some(&self.events),
                true,
            )
            .await
        } else {
            runner::run_git_ok(
                repo_path,
                &["merge", "--abort"],
                Some(&self.command_log),
                Some(&self.events),
                true,
            )
            .await
        }
    }
}
