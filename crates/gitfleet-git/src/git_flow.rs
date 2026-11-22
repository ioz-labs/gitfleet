use crate::{GitService, runner};
use gitfleet_core::AppResult;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GitFlowKind {
    Feature,
    Release,
    Hotfix,
    Bugfix,
}

impl GitFlowKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Feature => "Feature",
            Self::Release => "Release",
            Self::Hotfix => "Hotfix",
            Self::Bugfix => "Bugfix",
        }
    }

    pub fn default_prefix(self) -> &'static str {
        match self {
            Self::Feature => "feature/",
            Self::Release => "release/",
            Self::Hotfix => "hotfix/",
            Self::Bugfix => "bugfix/",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitFlowPrefixes {
    pub feature: String,
    pub release: String,
    pub hotfix: String,
    pub bugfix: String,
}

impl Default for GitFlowPrefixes {
    fn default() -> Self {
        Self {
            feature: "feature/".into(),
            release: "release/".into(),
            hotfix: "hotfix/".into(),
            bugfix: "bugfix/".into(),
        }
    }
}

pub fn normalize_flow_prefix(raw: &str, fallback: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return fallback.to_string();
    }
    if trimmed.ends_with('/') {
        trimmed.to_string()
    } else {
        format!("{trimmed}/")
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitFlowState {
    pub initialized: bool,
    pub main_branch: Option<String>,
    pub develop_branch: Option<String>,
    #[serde(default)]
    pub prefixes: GitFlowPrefixes,
}

impl GitFlowState {
    pub fn main_or_default(&self) -> &str {
        self.main_branch.as_deref().unwrap_or("main")
    }

    pub fn develop_or_default(&self) -> &str {
        self.develop_branch.as_deref().unwrap_or("develop")
    }

    pub fn prefix_for(&self, kind: GitFlowKind) -> &str {
        match kind {
            GitFlowKind::Feature => {
                if self.prefixes.feature.is_empty() {
                    "feature/"
                } else {
                    &self.prefixes.feature
                }
            }
            GitFlowKind::Release => {
                if self.prefixes.release.is_empty() {
                    "release/"
                } else {
                    &self.prefixes.release
                }
            }
            GitFlowKind::Hotfix => {
                if self.prefixes.hotfix.is_empty() {
                    "hotfix/"
                } else {
                    &self.prefixes.hotfix
                }
            }
            GitFlowKind::Bugfix => {
                if self.prefixes.bugfix.is_empty() {
                    "bugfix/"
                } else {
                    &self.prefixes.bugfix
                }
            }
        }
    }

    pub fn base_branch_for(&self, kind: GitFlowKind) -> &str {
        match kind {
            GitFlowKind::Feature | GitFlowKind::Release | GitFlowKind::Bugfix => {
                self.develop_or_default()
            }
            GitFlowKind::Hotfix => self.main_or_default(),
        }
    }

    pub fn target_branch_for(&self, kind: GitFlowKind) -> &str {
        match kind {
            GitFlowKind::Feature | GitFlowKind::Bugfix => self.develop_or_default(),
            GitFlowKind::Release | GitFlowKind::Hotfix => self.main_or_default(),
        }
    }

    /// Classifies a local branch name against the configured Git Flow prefixes.
    pub fn classify_branch<'a>(&self, branch_name: &'a str) -> Option<(GitFlowKind, &'a str)> {
        for kind in [
            GitFlowKind::Feature,
            GitFlowKind::Release,
            GitFlowKind::Hotfix,
            GitFlowKind::Bugfix,
        ] {
            let prefix = self.prefix_for(kind);
            if let Some(rest) = branch_name.strip_prefix(prefix)
                && !rest.is_empty()
            {
                return Some((kind, rest));
            }
        }
        None
    }
}

async fn read_git_config_opt(repo_path: &Path, key: &str) -> Option<String> {
    runner::run_git(repo_path, &["config", "--get", key], None, None, false)
        .await
        .ok()
        .filter(|o| o.exit_code == 0)
        .map(|o| o.stdout.trim().to_string())
        .filter(|s| !s.is_empty())
}

impl GitService {
    pub async fn get_git_flow_state(&self, repo_path: &Path) -> AppResult<GitFlowState> {
        let main_b = read_git_config_opt(repo_path, "gitflow.branch.master").await;
        let dev_b = read_git_config_opt(repo_path, "gitflow.branch.develop").await;
        let feature = read_git_config_opt(repo_path, "gitflow.prefix.feature")
            .await
            .unwrap_or_else(|| "feature/".into());
        let release = read_git_config_opt(repo_path, "gitflow.prefix.release")
            .await
            .unwrap_or_else(|| "release/".into());
        let hotfix = read_git_config_opt(repo_path, "gitflow.prefix.hotfix")
            .await
            .unwrap_or_else(|| "hotfix/".into());
        let bugfix = read_git_config_opt(repo_path, "gitflow.prefix.bugfix")
            .await
            .unwrap_or_else(|| "bugfix/".into());
        Ok(GitFlowState {
            initialized: main_b.is_some() && dev_b.is_some(),
            main_branch: main_b,
            develop_branch: dev_b,
            prefixes: GitFlowPrefixes {
                feature,
                release,
                hotfix,
                bugfix,
            },
        })
    }

    pub async fn init_git_flow(
        &self,
        repo_path: &Path,
        main_branch: &str,
        develop_branch: &str,
    ) -> AppResult<String> {
        self.init_git_flow_with_prefixes(
            repo_path,
            main_branch,
            develop_branch,
            &GitFlowPrefixes::default(),
        )
        .await
    }

    pub async fn init_git_flow_with_prefixes(
        &self,
        repo_path: &Path,
        main_branch: &str,
        develop_branch: &str,
        prefixes: &GitFlowPrefixes,
    ) -> AppResult<String> {
        let feat_p = normalize_flow_prefix(&prefixes.feature, "feature/");
        let rel_p = normalize_flow_prefix(&prefixes.release, "release/");
        let hot_p = normalize_flow_prefix(&prefixes.hotfix, "hotfix/");
        let bug_p = normalize_flow_prefix(&prefixes.bugfix, "bugfix/");

        runner::run_git_ok(
            repo_path,
            &["config", "gitflow.branch.master", main_branch],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await?;
        runner::run_git_ok(
            repo_path,
            &["config", "gitflow.branch.develop", develop_branch],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await?;
        runner::run_git_ok(
            repo_path,
            &["config", "gitflow.prefix.feature", &feat_p],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;
        runner::run_git_ok(
            repo_path,
            &["config", "gitflow.prefix.release", &rel_p],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;
        runner::run_git_ok(
            repo_path,
            &["config", "gitflow.prefix.hotfix", &hot_p],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;
        runner::run_git_ok(
            repo_path,
            &["config", "gitflow.prefix.bugfix", &bug_p],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;
        Ok(format!(
            "Git Flow configured ({main_branch} / {develop_branch})"
        ))
    }

    /// Removes Git Flow configuration sections (`gitflow.branch` and `gitflow.prefix`) from
    /// `.git/config` without deleting any Git branches or commits.
    pub async fn uninit_git_flow(&self, repo_path: &Path) -> AppResult<String> {
        for section in ["gitflow.branch", "gitflow.prefix"] {
            let _ = runner::run_git(
                repo_path,
                &["config", "--local", "--remove-section", section],
                Some(&self.command_log),
                Some(&self.events),
                true,
            )
            .await;
        }
        Ok("Git Flow configuration removed".into())
    }

    pub async fn start_git_flow(
        &self,
        repo_path: &Path,
        kind: GitFlowKind,
        name: &str,
    ) -> AppResult<String> {
        let state = self.get_git_flow_state(repo_path).await?;
        let prefix = state.prefix_for(kind);
        let base = state.base_branch_for(kind);
        let branch = if name.starts_with(prefix) {
            name.to_string()
        } else {
            format!("{prefix}{name}")
        };
        runner::run_git_ok(
            repo_path,
            &["checkout", "-b", &branch, base],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn finish_git_flow(
        &self,
        repo_path: &Path,
        kind: GitFlowKind,
        name: &str,
    ) -> AppResult<String> {
        let state = self.get_git_flow_state(repo_path).await?;
        let prefix = state.prefix_for(kind);
        let target = state.target_branch_for(kind).to_string();
        let branch = if name.starts_with(prefix) {
            name.to_string()
        } else {
            format!("{prefix}{name}")
        };
        runner::run_git_ok(
            repo_path,
            &["checkout", &target],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await?;
        runner::run_git_ok(
            repo_path,
            &["merge", "--no-ff", &branch],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await?;
        runner::run_git_ok(
            repo_path,
            &["branch", "-d", &branch],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await?;
        Ok(format!("Finished {branch} into {target}"))
    }

    /// Returns `true` if `branch` contains commits not yet merged into `base_branch`
    /// (works even while `branch` is currently checked out as `HEAD`).
    pub async fn is_branch_unmerged_into(
        &self,
        repo_path: &Path,
        branch: &str,
        base_branch: &str,
    ) -> bool {
        let range = format!("{base_branch}..{branch}");
        runner::run_git(repo_path, &["rev-list", "--count", &range], None, None, false)
            .await
            .ok()
            .filter(|o| o.exit_code == 0)
            .and_then(|o| o.stdout.trim().parse::<usize>().ok())
            .is_some_and(|cnt| cnt > 0)
    }

    /// Cancels/aborts a Git Flow branch without merging it: if currently checked out on `branch`,
    /// switches back to `base_branch` (or fallback main/develop) first, then deletes `branch`.
    pub async fn abort_git_flow(
        &self,
        repo_path: &Path,
        branch: &str,
        base_branch: &str,
        force: bool,
    ) -> AppResult<String> {
        let current_head = runner::run_git(
            repo_path,
            &["symbolic-ref", "--quiet", "--short", "HEAD"],
            None,
            None,
            false,
        )
        .await
        .ok()
        .filter(|o| o.exit_code == 0)
        .map(|o| o.stdout.trim().to_string());

        if current_head.as_deref() == Some(branch) {
            let state = self.get_git_flow_state(repo_path).await?;
            let mut candidates = vec![base_branch.to_string()];
            let dev = state.develop_or_default().to_string();
            let main = state.main_or_default().to_string();
            if !candidates.contains(&dev) {
                candidates.push(dev);
            }
            if !candidates.contains(&main) {
                candidates.push(main);
            }
            let mut switched = false;
            let mut last_err = None;
            for cand in candidates {
                if cand == branch {
                    continue;
                }
                match runner::run_git_ok(
                    repo_path,
                    &["checkout", &cand],
                    Some(&self.command_log),
                    Some(&self.events),
                    true,
                )
                .await
                {
                    Ok(_) => {
                        switched = true;
                        break;
                    }
                    Err(e) => {
                        last_err = Some(e);
                    }
                }
            }
            if !switched && let Some(err) = last_err {
                return Err(err);
            }
        }

        let flag = if force { "-D" } else { "-d" };
        runner::run_git_ok(
            repo_path,
            &["branch", flag, branch],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await?;
        Ok(format!("Cancelled branch {branch}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitfleet_core::{CommandLogStore, EventBus};
    use std::sync::Arc;

    #[tokio::test]
    async fn git_flow_init_edit_start_abort_and_uninit_lifecycle() {
        let dir = std::env::temp_dir().join(format!(
            "gitfleet_gitflow_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let svc = GitService::new(EventBus::new(16), Arc::new(CommandLogStore::new()));
        runner::run_git_ok(&dir, &["init", "-b", "main"], None, None, false)
            .await
            .unwrap();
        runner::run_git_ok(&dir, &["config", "user.email", "t@t.dev"], None, None, false)
            .await
            .unwrap();
        runner::run_git_ok(&dir, &["config", "user.name", "Test"], None, None, false)
            .await
            .unwrap();
        std::fs::write(dir.join("README.md"), "init\n").unwrap();
        runner::run_git_ok(&dir, &["add", "."], None, None, false)
            .await
            .unwrap();
        runner::run_git_ok(&dir, &["commit", "-m", "init"], None, None, false)
            .await
            .unwrap();
        runner::run_git_ok(&dir, &["branch", "develop"], None, None, false)
            .await
            .unwrap();

        // 1. Uninitialized initially
        let st0 = svc.get_git_flow_state(&dir).await.unwrap();
        assert!(!st0.initialized);

        // 2. Initialize & edit prefixes
        let custom = GitFlowPrefixes {
            feature: "feat".into(), // trailing slash normalized automatically
            release: "rel/".into(),
            hotfix: "hotfix/".into(),
            bugfix: "bugfix/".into(),
        };
        svc.init_git_flow_with_prefixes(&dir, "main", "develop", &custom)
            .await
            .unwrap();
        let st1 = svc.get_git_flow_state(&dir).await.unwrap();
        assert!(st1.initialized);
        assert_eq!(st1.main_branch.as_deref(), Some("main"));
        assert_eq!(st1.develop_branch.as_deref(), Some("develop"));
        assert_eq!(st1.prefix_for(GitFlowKind::Feature), "feat/");
        assert_eq!(
            st1.classify_branch("feat/oauth"),
            Some((GitFlowKind::Feature, "oauth"))
        );

        // 3. Start branch then abort while currently checked out on it
        svc.start_git_flow(&dir, GitFlowKind::Feature, "oauth")
            .await
            .unwrap();
        svc.abort_git_flow(&dir, "feat/oauth", "develop", false)
            .await
            .unwrap();
        let head = runner::run_git_ok(&dir, &["symbolic-ref", "--short", "HEAD"], None, None, false)
            .await
            .unwrap();
        assert_eq!(head.trim(), "develop");

        // 4. Disable / Uninit removes config without deleting branches
        svc.uninit_git_flow(&dir).await.unwrap();
        let st2 = svc.get_git_flow_state(&dir).await.unwrap();
        assert!(!st2.initialized);
        assert!(st2.main_branch.is_none());
        assert!(st2.develop_branch.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
