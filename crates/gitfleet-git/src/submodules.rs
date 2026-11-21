use crate::models::OrphanGitlink;
use crate::{GitService, runner};
use gitfleet_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::path::Path;

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmoduleInfo {
    pub name: String,
    pub path: String,
    pub url: String,
    pub head_oid: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default = "default_true")]
    pub initialized: bool,
    pub dirty: bool,
}

impl GitService {
    pub async fn get_submodules(&self, repo_path: &Path) -> AppResult<Vec<SubmoduleInfo>> {
        let out = runner::run_git(
            repo_path,
            &["submodule", "status"],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;

        let mut list = Vec::new();
        for line in out.stdout.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let initialized = !trimmed.starts_with('-');
            let dirty = trimmed.starts_with('+') || trimmed.starts_with('U');
            let clean = trimmed.trim_start_matches(['+', '-', 'U', ' ']);
            let mut parts = clean.split_whitespace();
            if let (Some(oid), Some(path)) = (parts.next(), parts.next()) {
                let url_key = format!("submodule.{path}.url");
                let url = runner::run_git(
                    repo_path,
                    &["config", "--file", ".gitmodules", "--get", &url_key],
                    None,
                    None,
                    false,
                )
                .await
                .ok()
                .filter(|o| o.exit_code == 0)
                .map(|o| o.stdout.trim().to_string())
                .unwrap_or_default();

                let sub_dir = repo_path.join(path);
                let branch = if initialized && sub_dir.exists() {
                    runner::run_git(
                        &sub_dir,
                        &["symbolic-ref", "--short", "HEAD"],
                        None,
                        None,
                        false,
                    )
                    .await
                    .ok()
                    .filter(|o| o.exit_code == 0)
                    .map(|o| o.stdout.trim().to_string())
                    .filter(|b| !b.is_empty())
                } else {
                    None
                };

                list.push(SubmoduleInfo {
                    name: path.to_string(),
                    path: path.to_string(),
                    url,
                    head_oid: Some(oid.chars().take(8).collect()),
                    branch,
                    initialized,
                    dirty,
                });
            }
        }
        Ok(list)
    }

    pub async fn submodule_add(
        &self,
        repo_path: &Path,
        url: &str,
        destination: &str,
    ) -> AppResult<String> {
        runner::run_git_ok(
            repo_path,
            &["submodule", "add", url, destination],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn submodule_update(
        &self,
        repo_path: &Path,
        sub_path: Option<&str>,
    ) -> AppResult<String> {
        let mut args = vec!["submodule", "update", "--init", "--recursive"];
        if let Some(p) = sub_path {
            args.push("--");
            args.push(p);
        }
        runner::run_git_ok(
            repo_path,
            &args,
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn submodule_sync(&self, repo_path: &Path) -> AppResult<String> {
        runner::run_git_ok(
            repo_path,
            &["submodule", "sync", "--recursive"],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn submodule_remove(&self, repo_path: &Path, sub_path: &str) -> AppResult<String> {
        let _ = runner::run_git(
            repo_path,
            &["submodule", "deinit", "-f", "--", sub_path],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await;
        runner::run_git_ok(
            repo_path,
            &["rm", "-f", sub_path],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    /// Detects orphan submodule gitlinks (pinned commits not yet reachable on any remote tracking branch).
    pub async fn push_preflight(&self, repo_path: &Path) -> AppResult<Vec<OrphanGitlink>> {
        let subs = self.get_submodules(repo_path).await?;
        let mut orphans = Vec::new();
        for sub in subs {
            let sub_dir = repo_path.join(&sub.path);
            if !sub_dir.exists() {
                continue;
            }
            if let Ok(head) = self.head_oid(&sub_dir).await {
                let remote_check = runner::run_git(
                    &sub_dir,
                    &["branch", "-r", "--contains", &head],
                    None,
                    None,
                    false,
                )
                .await;
                let on_remote = remote_check
                    .map(|o| o.exit_code == 0 && !o.stdout.trim().is_empty())
                    .unwrap_or(false);
                if !on_remote {
                    orphans.push(OrphanGitlink {
                        submodule_path: sub.path,
                        commit_oid: head.chars().take(8).collect(),
                    });
                }
            }
        }
        Ok(orphans)
    }

    /// Ordered push: pushes each listed submodule first, then the superproject; stops on first failure.
    pub async fn ordered_push(
        &self,
        super_path: &Path,
        submodule_paths: &[String],
    ) -> AppResult<String> {
        for sub_rel in submodule_paths {
            let sub_dir = super_path.join(sub_rel);
            self.push(&sub_dir).await.map_err(|e| {
                AppError::Git(format!("Submodule '{sub_rel}' push failed (stopped): {e}"))
            })?;
        }
        self.push(super_path).await?;
        Ok(format!(
            "Ordered push succeeded ({} submodules + superproject)",
            submodule_paths.len()
        ))
    }
}
