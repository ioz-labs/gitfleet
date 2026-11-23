use crate::models::{RebaseAction, RebaseTodoItem};
use crate::{GitService, runner};
use gitfleet_core::{AppError, AppResult};
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
        self.create_branch_from(repo_path, branch, None, true).await
    }

    pub async fn create_branch_from(
        &self,
        repo_path: &Path,
        branch: &str,
        base_ref: Option<&str>,
        checkout: bool,
    ) -> AppResult<String> {
        let mut args: Vec<&str> = if checkout {
            vec!["checkout", "-b", branch]
        } else {
            vec!["branch", branch]
        };
        if let Some(base) = base_ref.map(str::trim).filter(|s| !s.is_empty()) {
            args.push(base);
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

    pub async fn rename_branch(
        &self,
        repo_path: &Path,
        old_name: &str,
        new_name: &str,
    ) -> AppResult<String> {
        runner::run_git_ok(
            repo_path,
            &["branch", "-m", "--", old_name, new_name],
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

    /// Returns the subset of `candidates` that contain commits not yet merged into `HEAD`
    /// (i.e. branches for which `git branch -d` would fail and `git branch -D` is required).
    pub async fn unmerged_branches(
        &self,
        repo_path: &Path,
        candidates: &[String],
    ) -> AppResult<Vec<String>> {
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        let out = runner::run_git(
            repo_path,
            &["branch", "--no-merged", "HEAD", "--format=%(refname:short)"],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;
        if out.exit_code != 0 {
            return Ok(Vec::new());
        }
        let unmerged_set: std::collections::HashSet<&str> = out
            .stdout
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        Ok(candidates
            .iter()
            .filter(|b| unmerged_set.contains(b.as_str()))
            .cloned()
            .collect())
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

    /// Lists commits in `<base_ref>..HEAD` ordered oldest-first (matching `git rebase -i` todo order).
    pub async fn list_rebase_commits(
        &self,
        repo_path: &Path,
        base_ref: &str,
    ) -> AppResult<Vec<RebaseTodoItem>> {
        let range = format!("{base_ref}..HEAD");
        let out = runner::run_git_ok(
            repo_path,
            &["log", "--reverse", "--format=%H%x1f%h%x1f%s", &range],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;
        let mut items = Vec::new();
        for line in out.lines() {
            let parts: Vec<&str> = line.split('\x1f').collect();
            if parts.len() >= 3 {
                items.push(RebaseTodoItem {
                    action: RebaseAction::Pick,
                    oid: parts[0].trim().to_string(),
                    short_oid: parts[1].trim().to_string(),
                    summary: parts[2].trim().to_string(),
                });
            }
        }
        Ok(items)
    }

    /// Executes an interactive rebase (`git rebase -i --autostash <base_ref>`) headlessly without
    /// spawning any external editor window, using environment-injected POSIX `cp` + `:` sequence
    /// helpers safe across Windows, macOS, and Linux paths containing spaces or Unicode.
    pub async fn interactive_rebase(
        &self,
        repo_path: &Path,
        base_ref: &str,
        plan: &[RebaseTodoItem],
    ) -> AppResult<String> {
        if plan.is_empty() {
            return Err(AppError::Git(
                "Interactive rebase plan has no commits to apply.".into(),
            ));
        }
        if let Some(first_kept) = plan.iter().find(|it| it.action != RebaseAction::Drop)
            && matches!(first_kept.action, RebaseAction::Squash | RebaseAction::Fixup)
        {
            return Err(AppError::Git(
                "The first kept commit in an interactive rebase cannot be Squash or Fixup."
                    .into(),
            ));
        }

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let temp_dir = std::env::temp_dir().join(format!(
            "gitfleet_irebase_{}_{}",
            std::process::id(),
            nanos
        ));
        std::fs::create_dir_all(&temp_dir)?;

        let mut todo_buf = String::new();
        let mut env_owned: Vec<(String, String)> = Vec::new();

        for (idx, item) in plan.iter().enumerate() {
            match &item.action {
                RebaseAction::Pick => {
                    todo_buf.push_str(&format!("pick {}\n", item.oid));
                }
                RebaseAction::Reword(new_msg) => {
                    let msg_path = temp_dir.join(format!("msg_{idx}.txt"));
                    let normalized = new_msg.trim().replace("\r\n", "\n");
                    let body = if normalized.is_empty() {
                        item.summary.clone()
                    } else {
                        normalized
                    };
                    std::fs::write(&msg_path, format!("{body}\n"))?;
                    let env_key = format!("GITFLEET_MSG_{idx}");
                    env_owned.push((env_key.clone(), msg_path.to_string_lossy().to_string()));
                    todo_buf.push_str(&format!(
                        "pick {}\nexec git commit --amend -F \"${env_key}\"\n",
                        item.oid
                    ));
                }
                RebaseAction::Squash => {
                    todo_buf.push_str(&format!("squash {}\n", item.oid));
                }
                RebaseAction::Fixup => {
                    todo_buf.push_str(&format!("fixup {}\n", item.oid));
                }
                RebaseAction::Drop => {
                    todo_buf.push_str(&format!("drop {}\n", item.oid));
                }
            }
        }

        let todo_path = temp_dir.join("git-rebase-todo");
        std::fs::write(&todo_path, todo_buf.as_bytes())?;
        env_owned.push((
            "GITFLEET_TODO_FILE".to_string(),
            todo_path.to_string_lossy().to_string(),
        ));
        env_owned.push((
            "GIT_SEQUENCE_EDITOR".to_string(),
            "cp \"$GITFLEET_TODO_FILE\"".to_string(),
        ));
        env_owned.push(("GIT_EDITOR".to_string(), ":".to_string()));

        let env_refs: Vec<(&str, &str)> = env_owned
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        let res = runner::run_git_with_envs(
            repo_path,
            &["rebase", "-i", "--autostash", base_ref],
            &env_refs,
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await;

        let _ = std::fs::remove_dir_all(&temp_dir);

        let out = res?;
        if out.exit_code != 0 {
            let msg = if out.stderr.trim().is_empty() {
                out.stdout.trim().to_string()
            } else {
                out.stderr.trim().to_string()
            };
            return Err(AppError::Git(msg));
        }
        Ok(out.stdout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitfleet_core::{CommandLogStore, EventBus};
    use std::sync::Arc;

    #[test]
    fn parses_git_upstream_track_counts() {
        assert_eq!(parse_upstream_track(""), (0, 0, false));
        assert_eq!(parse_upstream_track("behind 69"), (0, 69, false));
        assert_eq!(parse_upstream_track("ahead 4"), (4, 0, false));
        assert_eq!(parse_upstream_track("ahead 2, behind 12"), (2, 12, false));
        assert_eq!(parse_upstream_track("gone"), (0, 0, true));
    }

    #[tokio::test]
    async fn detects_unmerged_branches_and_deletes_with_force() {
        let dir = std::env::temp_dir().join(format!(
            "gitfleet_branch_delete_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let svc = GitService::new(EventBus::new(16), Arc::new(CommandLogStore::new()));
        runner::run_git_ok(&dir, &["init", "-b", "main"], None, None, false)
            .await
            .unwrap();
        runner::run_git_ok(&dir, &["config", "user.name", "Tester"], None, None, false)
            .await
            .unwrap();
        runner::run_git_ok(&dir, &["config", "user.email", "t@local"], None, None, false)
            .await
            .unwrap();

        std::fs::write(dir.join("base.txt"), "base\n").unwrap();
        svc.stage(&dir, &["base.txt".into()]).await.unwrap();
        svc.commit(&dir, "init", false).await.unwrap();

        // Merged branch (points at HEAD)
        svc.create_branch_from(&dir, "merged-ok", None, false)
            .await
            .unwrap();

        // Unmerged branch (has a commit ahead of main)
        svc.create_branch(&dir, "test-2").await.unwrap();
        std::fs::write(dir.join("unmerged.txt"), "wip\n").unwrap();
        svc.stage(&dir, &["unmerged.txt".into()]).await.unwrap();
        svc.commit(&dir, "unmerged commit", false).await.unwrap();
        svc.checkout(&dir, "main").await.unwrap();

        let unmerged = svc
            .unmerged_branches(&dir, &["merged-ok".into(), "test-2".into()])
            .await
            .unwrap();
        assert_eq!(unmerged, vec!["test-2".to_string()]);

        // Safe delete works for merged branch, rejects unmerged branch
        svc.delete_branch(&dir, "merged-ok", false).await.unwrap();
        assert!(svc.delete_branch(&dir, "test-2", false).await.is_err());

        // Force delete succeeds for unmerged branch
        svc.delete_branch(&dir, "test-2", true).await.unwrap();
        let remaining = svc.get_branches(&dir).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].name, "main");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn interactive_rebase_reorders_rewords_squashes_fixups_and_drops_headlessly() {
        let dir = std::env::temp_dir().join(format!(
            "gitfleet_irebase_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let svc = GitService::new(EventBus::new(16), Arc::new(CommandLogStore::new()));
        runner::run_git_ok(&dir, &["init", "-b", "main"], None, None, false)
            .await
            .unwrap();
        runner::run_git_ok(&dir, &["config", "user.name", "Tester"], None, None, false)
            .await
            .unwrap();
        runner::run_git_ok(&dir, &["config", "user.email", "t@local"], None, None, false)
            .await
            .unwrap();

        std::fs::write(dir.join("base.txt"), "root\n").unwrap();
        svc.stage(&dir, &["base.txt".into()]).await.unwrap();
        svc.commit(&dir, "root commit", false).await.unwrap();
        let base_oid = svc.head_oid(&dir).await.unwrap();

        for (fname, msg) in [
            ("c1.txt", "first commit"),
            ("c2.txt", "second fixup"),
            ("c3.txt", "third drop me"),
            ("c4.txt", "fourth reorder"),
        ] {
            std::fs::write(dir.join(fname), fname).unwrap();
            svc.stage(&dir, &[fname.into()]).await.unwrap();
            svc.commit(&dir, msg, false).await.unwrap();
        }

        let mut plan = svc.list_rebase_commits(&dir, &base_oid).await.unwrap();
        assert_eq!(plan.len(), 4);
        assert_eq!(plan[0].summary, "first commit");
        assert_eq!(plan[3].summary, "fourth reorder");

        // Reorder c4 to the front with a Vietnamese UTF-8 Reword, keep c1 + Fixup c2, and Drop c3!
        let mut c4 = plan.remove(3);
        c4.action = RebaseAction::Reword("feat: đưa commit 4 lên đầu".into());
        plan[0].action = RebaseAction::Pick;
        plan[1].action = RebaseAction::Fixup;
        plan[2].action = RebaseAction::Drop;
        plan.insert(0, c4);

        svc.interactive_rebase(&dir, &base_oid, &plan).await.unwrap();

        let log = svc
            .get_log(
                &dir,
                crate::models::LogOptions {
                    limit: 10,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let summaries: Vec<&str> = log.iter().map(|c| c.summary.as_str()).collect();
        assert_eq!(
            summaries,
            vec!["first commit", "feat: đưa commit 4 lên đầu", "root commit"]
        );
        assert!(dir.join("c1.txt").exists());
        assert!(dir.join("c2.txt").exists());
        assert!(!dir.join("c3.txt").exists());
        assert!(dir.join("c4.txt").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
