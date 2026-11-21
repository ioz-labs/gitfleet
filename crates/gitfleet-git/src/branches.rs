use crate::{GitService, runner};
use gitfleet_core::AppResult;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchInfo {
    pub name: String,
    pub is_head: bool,
    pub is_remote: bool,
    pub upstream: Option<String>,
    #[serde(default)]
    pub ahead: usize,
    #[serde(default)]
    pub behind: usize,
    #[serde(default)]
    pub upstream_gone: bool,
    #[serde(default)]
    pub worktree_path: Option<String>,
}

/// Parses Git's `%(upstream:track,nobracket)` output (e.g. `"ahead 2, behind 5"`, `"behind 69"`, `"gone"`).
pub fn parse_upstream_track(track: &str) -> (usize, usize, bool) {
    let trimmed = track.trim();
    if trimmed.is_empty() {
        return (0, 0, false);
    }
    if trimmed == "gone" {
        return (0, 0, true);
    }
    let mut ahead = 0usize;
    let mut behind = 0usize;
    for part in trimmed.split(',') {
        let p = part.trim();
        if let Some(n) = p.strip_prefix("ahead ") {
            ahead = n.trim().parse().unwrap_or(0);
        } else if let Some(n) = p.strip_prefix("behind ") {
            behind = n.trim().parse().unwrap_or(0);
        }
    }
    (ahead, behind, false)
}

impl GitService {
    pub async fn get_branches(&self, repo_path: &Path) -> AppResult<Vec<BranchInfo>> {
        let out = runner::run_git_ok(
            repo_path,
            &[
                "branch",
                "-a",
                "--format=%(HEAD)%09%(refname:short)%09%(upstream:short)%09%(refname)%09%(upstream:track,nobracket)%09%(worktreepath)",
            ],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;

        let mut list = Vec::new();
        for line in out.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 4 {
                // Skip symbolic remote HEAD pointers (e.g. `refs/remotes/origin/HEAD`)
                if parts[3].starts_with("refs/remotes/") && parts[3].ends_with("/HEAD") {
                    continue;
                }
                let (ahead, behind, upstream_gone) = parts
                    .get(4)
                    .map(|t| parse_upstream_track(t))
                    .unwrap_or((0, 0, false));
                let worktree_path = parts
                    .get(5)
                    .map(|p| p.trim())
                    .filter(|p| !p.is_empty())
                    .map(|p| p.to_string());

                list.push(BranchInfo {
                    is_head: parts[0].trim() == "*",
                    name: parts[1].to_string(),
                    upstream: if parts[2].is_empty() {
                        None
                    } else {
                        Some(parts[2].to_string())
                    },
                    is_remote: parts[3].starts_with("refs/remotes/"),
                    ahead,
                    behind,
                    upstream_gone,
                    worktree_path,
                });
            }
        }
        Ok(list)
    }

    pub async fn checkout(&self, repo_path: &Path, branch: &str) -> AppResult<String> {
        runner::run_git_ok(
            repo_path,
            &["checkout", branch],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn checkout_remote_branch(
        &self,
        repo_path: &Path,
        remote_ref: &str,
    ) -> AppResult<String> {
        let local_name = remote_ref
            .split_once('/')
            .map(|(_, rest)| rest)
            .unwrap_or(remote_ref);
        let track_out = runner::run_git(
            repo_path,
            &["checkout", "--track", remote_ref],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await?;
        if track_out.exit_code == 0 {
            return Ok(track_out.stdout);
        }
        // Fallback when local branch with that name already exists
        self.checkout(repo_path, local_name).await
    }

    pub async fn checkout_detached(&self, repo_path: &Path, rev: &str) -> AppResult<String> {
        runner::run_git_ok(
            repo_path,
            &["checkout", "--detach", rev],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn create_branch(&self, repo_path: &Path, branch: &str) -> AppResult<String> {
        runner::run_git_ok(
            repo_path,
            &["checkout", "-b", branch],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn push_branch(&self, repo_path: &Path, branch: &str) -> AppResult<String> {
        runner::run_git_ok(
            repo_path,
            &["push", "-u", "origin", branch],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn unset_upstream(&self, repo_path: &Path, branch: &str) -> AppResult<String> {
        runner::run_git_ok(
            repo_path,
            &["branch", "--unset-upstream", branch],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn delete_branch(
        &self,
        repo_path: &Path,
        branch: &str,
        force: bool,
    ) -> AppResult<String> {
        let flag = if force { "-D" } else { "-d" };
        runner::run_git_ok(
            repo_path,
            &["branch", flag, branch],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn delete_branches(
        &self,
        repo_path: &Path,
        branches: &[String],
        force: bool,
    ) -> AppResult<String> {
        if branches.is_empty() {
            return Ok(String::new());
        }
        let flag = if force { "-D" } else { "-d" };
        let mut args = vec!["branch", flag];
        for b in branches {
            args.push(b.as_str());
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_git_upstream_track_counts() {
        assert_eq!(parse_upstream_track(""), (0, 0, false));
        assert_eq!(parse_upstream_track("behind 69"), (0, 69, false));
        assert_eq!(parse_upstream_track("ahead 4"), (4, 0, false));
        assert_eq!(parse_upstream_track("ahead 2, behind 12"), (2, 12, false));
        assert_eq!(parse_upstream_track("gone"), (0, 0, true));
    }
}
