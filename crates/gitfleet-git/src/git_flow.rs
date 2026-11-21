use crate::{GitService, runner};
use gitfleet_core::AppResult;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum GitFlowKind {
    Feature,
    Release,
    Hotfix,
    Bugfix,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitFlowState {
    pub initialized: bool,
    pub main_branch: Option<String>,
    pub develop_branch: Option<String>,
}

impl GitService {
    pub async fn get_git_flow_state(&self, repo_path: &Path) -> AppResult<GitFlowState> {
        let main_b = runner::run_git(
            repo_path,
            &["config", "--get", "gitflow.branch.master"],
            None,
            None,
            false,
        )
        .await
        .ok()
        .filter(|o| o.exit_code == 0)
        .map(|o| o.stdout.trim().to_string())
        .filter(|s| !s.is_empty());
        let dev_b = runner::run_git(
            repo_path,
            &["config", "--get", "gitflow.branch.develop"],
            None,
            None,
            false,
        )
        .await
        .ok()
        .filter(|o| o.exit_code == 0)
        .map(|o| o.stdout.trim().to_string())
        .filter(|s| !s.is_empty());
        Ok(GitFlowState {
            initialized: main_b.is_some() && dev_b.is_some(),
            main_branch: main_b,
            develop_branch: dev_b,
        })
    }

    pub async fn init_git_flow(
        &self,
        repo_path: &Path,
        main_branch: &str,
        develop_branch: &str,
    ) -> AppResult<String> {
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
            &["config", "gitflow.prefix.feature", "feature/"],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;
        runner::run_git_ok(
            repo_path,
            &["config", "gitflow.prefix.release", "release/"],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;
        runner::run_git_ok(
            repo_path,
            &["config", "gitflow.prefix.hotfix", "hotfix/"],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;
        Ok("Git Flow initialized".into())
    }

    pub async fn start_git_flow(
        &self,
        repo_path: &Path,
        kind: GitFlowKind,
        name: &str,
    ) -> AppResult<String> {
        let state = self.get_git_flow_state(repo_path).await?;
        let (prefix, base) = match kind {
            GitFlowKind::Feature => (
                "feature/",
                state.develop_branch.as_deref().unwrap_or("develop"),
            ),
            GitFlowKind::Release => (
                "release/",
                state.develop_branch.as_deref().unwrap_or("develop"),
            ),
            GitFlowKind::Hotfix => ("hotfix/", state.main_branch.as_deref().unwrap_or("main")),
            GitFlowKind::Bugfix => (
                "bugfix/",
                state.develop_branch.as_deref().unwrap_or("develop"),
            ),
        };
        let branch = format!("{prefix}{name}");
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
        let main_b = state.main_branch.unwrap_or_else(|| "main".into());
        let dev_b = state.develop_branch.unwrap_or_else(|| "develop".into());
        let prefix = match kind {
            GitFlowKind::Feature => "feature/",
            GitFlowKind::Release => "release/",
            GitFlowKind::Hotfix => "hotfix/",
            GitFlowKind::Bugfix => "bugfix/",
        };
        let branch = if name.starts_with(prefix) {
            name.to_string()
        } else {
            format!("{prefix}{name}")
        };
        let target = match kind {
            GitFlowKind::Feature | GitFlowKind::Bugfix => dev_b,
            GitFlowKind::Release | GitFlowKind::Hotfix => main_b,
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
}
