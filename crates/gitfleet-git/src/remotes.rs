use crate::{GitService, runner};
use gitfleet_core::AppResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteInfo {
    pub name: String,
    pub fetch_url: String,
    pub push_url: Option<String>,
}

impl GitService {
    pub async fn fetch(&self, repo_path: &Path) -> AppResult<String> {
        runner::run_git_ok(
            repo_path,
            &["fetch", "--all", "--prune"],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn pull(&self, repo_path: &Path) -> AppResult<String> {
        runner::run_git_ok(
            repo_path,
            &["pull", "--ff-only"],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn push(&self, repo_path: &Path) -> AppResult<String> {
        runner::run_git_ok(
            repo_path,
            &["push"],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn get_remotes(&self, repo_path: &Path) -> AppResult<Vec<RemoteInfo>> {
        let out = runner::run_git_ok(
            repo_path,
            &["remote", "-v"],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;

        let mut map: HashMap<String, RemoteInfo> = HashMap::new();
        for line in out.lines() {
            let mut parts = line.split_whitespace();
            if let (Some(name), Some(url), Some(kind)) = (parts.next(), parts.next(), parts.next())
            {
                let entry = map.entry(name.to_string()).or_insert_with(|| RemoteInfo {
                    name: name.to_string(),
                    fetch_url: url.to_string(),
                    push_url: None,
                });
                if kind.contains("push") {
                    entry.push_url = Some(url.to_string());
                } else {
                    entry.fetch_url = url.to_string();
                }
            }
        }
        let mut remotes: Vec<_> = map.into_values().collect();
        remotes.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(remotes)
    }

    pub async fn ensure_remote(
        &self,
        repo_path: &Path,
        name: &str,
        fetch_url: &str,
        push_url: Option<&str>,
    ) -> AppResult<()> {
        let existing = self.get_remotes(repo_path).await.unwrap_or_default();
        if existing.iter().any(|r| r.name == name) {
            let _ = runner::run_git_ok(
                repo_path,
                &["remote", "set-url", name, fetch_url],
                Some(&self.command_log),
                Some(&self.events),
                false,
            )
            .await?;
        } else {
            let _ = runner::run_git_ok(
                repo_path,
                &["remote", "add", name, fetch_url],
                Some(&self.command_log),
                Some(&self.events),
                false,
            )
            .await?;
        }
        if let Some(purl) = push_url
            && !purl.trim().is_empty()
            && purl != fetch_url
        {
            let _ = runner::run_git_ok(
                repo_path,
                &["remote", "set-url", "--push", name, purl],
                Some(&self.command_log),
                Some(&self.events),
                false,
            )
            .await;
        }
        Ok(())
    }
}
