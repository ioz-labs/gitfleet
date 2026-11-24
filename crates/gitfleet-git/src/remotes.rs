use crate::models::{PullOutcome, PullStrategy};
use crate::{GitService, runner};
use gitfleet_core::{AppError, AppResult};
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

    /// Executes a safe Pull / Sync workflow:
    /// 1. When `auto_stash` is `true` and the working tree has staged/unstaged/untracked changes,
    ///    stashes them (`-u`) before pulling.
    /// 2. Runs `git pull` with `--ff-only`, `--rebase`, or `--no-rebase`.
    /// 3. If `git pull` fails without entering merge/rebase (e.g., network error or diverged
    ///    `--ff-only`), immediately pops the temporary stash back so local changes are never lost.
    /// 4. If `git pull` succeeds and `auto_stashed`, pops `stash@{0}`. When re-applying local edits
    ///    conflicts with pulled upstream changes, Git keeps `stash@{0}` intact as a backup and
    ///    `stash_pop_conflicted` is returned as `true` so the UI opens the Conflict Solver.
    /// 5. When `push_after` is `true` and no conflict occurred, pushes local commits to upstream.
    pub async fn pull_smart(
        &self,
        repo_path: &Path,
        strategy: PullStrategy,
        auto_stash: bool,
        push_after: bool,
    ) -> AppResult<PullOutcome> {
        let pre_st = self.get_status(repo_path).await?;
        if pre_st.in_merge || pre_st.in_rebase || pre_st.conflict_count > 0 {
            return Err(AppError::Git(
                "Repository has an unfinished merge/rebase or unresolved conflicts. Resolve or abort before pulling."
                    .into(),
            ));
        }

        let mut did_stash = false;
        if auto_stash && (pre_st.staged_count + pre_st.unstaged_count + pre_st.untracked_count) > 0
        {
            let stash_out = self
                .stash_push_with_options(
                    repo_path,
                    Some("GitFleet auto-stash before Pull"),
                    true,
                    false,
                )
                .await?;
            did_stash = !stash_out.contains("No local changes to save");
        }

        let pull_out = runner::run_git(
            repo_path,
            &["pull", strategy.as_flag()],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await?;

        let combined = match (
            pull_out.stdout.trim().is_empty(),
            pull_out.stderr.trim().is_empty(),
        ) {
            (false, false) => format!("{}\n{}", pull_out.stdout.trim(), pull_out.stderr.trim()),
            (false, true) => pull_out.stdout.trim().to_string(),
            (true, false) => pull_out.stderr.trim().to_string(),
            (true, true) => String::new(),
        };

        if pull_out.exit_code != 0 {
            let post_st = self.get_status(repo_path).await.unwrap_or_default();
            if post_st.in_merge || post_st.in_rebase || post_st.conflict_count > 0 {
                return Ok(PullOutcome {
                    summary: combined,
                    auto_stashed: did_stash,
                    pull_conflicted: true,
                    stash_pop_conflicted: false,
                    pushed: false,
                });
            }
            if did_stash {
                let _ = self.stash_apply_smart(repo_path, 0, true).await;
            }
            return Err(AppError::Git(if combined.is_empty() {
                format!("git pull exited with code {}", pull_out.exit_code)
            } else {
                combined
            }));
        }

        if did_stash {
            let pop = self.stash_apply_smart(repo_path, 0, true).await?;
            if pop.conflicted {
                return Ok(PullOutcome {
                    summary: pop.summary,
                    auto_stashed: true,
                    pull_conflicted: false,
                    stash_pop_conflicted: true,
                    pushed: false,
                });
            }
        }

        if push_after {
            self.push(repo_path).await?;
        }

        Ok(PullOutcome {
            summary: combined,
            auto_stashed: did_stash,
            pull_conflicted: false,
            stash_pop_conflicted: false,
            pushed: push_after,
        })
    }

    pub async fn push(&self, repo_path: &Path) -> AppResult<String> {
        let st = self.get_status(repo_path).await.unwrap_or_default();
        if !st.detached
            && st.upstream.is_none()
            && let Some(branch) = st.branch.as_deref()
        {
            return self.push_set_upstream(repo_path, "origin", branch).await;
        }
        runner::run_git_ok(
            repo_path,
            &["push"],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn push_set_upstream(
        &self,
        repo_path: &Path,
        remote: &str,
        branch: &str,
    ) -> AppResult<String> {
        runner::run_git_ok(
            repo_path,
            &["push", "-u", remote, branch],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn push_force_with_lease(&self, repo_path: &Path) -> AppResult<String> {
        let st = self.get_status(repo_path).await.unwrap_or_default();
        if !st.detached
            && st.upstream.is_none()
            && let Some(branch) = st.branch.as_deref()
        {
            return runner::run_git_ok(
                repo_path,
                &["push", "--force-with-lease", "-u", "origin", branch],
                Some(&self.command_log),
                Some(&self.events),
                true,
            )
            .await;
        }
        runner::run_git_ok(
            repo_path,
            &["push", "--force-with-lease"],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn push_all_tags(&self, repo_path: &Path) -> AppResult<String> {
        runner::run_git_ok(
            repo_path,
            &["push", "origin", "--tags"],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    /// Executes a "Push, then Pull" sync order: pushes local commits first (publishing
    /// `-u origin <branch>` if needed), then pulls incoming upstream commits (with optional Auto-Stash).
    pub async fn sync_push_then_pull(
        &self,
        repo_path: &Path,
        strategy: PullStrategy,
        auto_stash: bool,
    ) -> AppResult<PullOutcome> {
        self.push(repo_path).await?;
        let mut out = self.pull_smart(repo_path, strategy, auto_stash, false).await?;
        out.pushed = true;
        Ok(out)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conflicts::has_conflict_markers;
    use gitfleet_core::{CommandLogStore, EventBus};
    use std::sync::Arc;

    async fn git_cfg(repo: &Path) {
        runner::run_git_ok(repo, &["config", "user.name", "Fleet Test"], None, None, false)
            .await
            .unwrap();
        runner::run_git_ok(
            repo,
            &["config", "user.email", "fleet@example.com"],
            None,
            None,
            false,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn smart_pull_auto_stashes_dirty_changes_and_preserves_stash_on_conflict() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "gitfleet_smart_pull_test_{}_{}",
            std::process::id(),
            nanos
        ));
        let bare = root.join("origin.git");
        let peer = root.join("peer");
        let local = root.join("local");
        std::fs::create_dir_all(&bare).unwrap();

        let svc = GitService::new(EventBus::new(16), Arc::new(CommandLogStore::new()));

        runner::run_git_ok(&bare, &["init", "--bare", "-b", "main"], None, None, false)
            .await
            .unwrap();

        let bare_str = bare.to_string_lossy().to_string();
        runner::run_git_ok(
            &root,
            &["clone", &bare_str, "peer"],
            None,
            None,
            false,
        )
        .await
        .unwrap();
        git_cfg(&peer).await;
        std::fs::write(peer.join("shared.txt"), "line 1\nline 2\n").unwrap();
        svc.stage_all(&peer).await.unwrap();
        svc.commit(&peer, "initial", false).await.unwrap();
        runner::run_git_ok(&peer, &["push", "-u", "origin", "main"], None, None, false)
            .await
            .unwrap();

        runner::run_git_ok(
            &root,
            &["clone", &bare_str, "local"],
            None,
            None,
            false,
        )
        .await
        .unwrap();
        git_cfg(&local).await;

        // 1. Clean auto-stash pull: upstream updates another file while local has dirty + untracked edits
        std::fs::write(peer.join("upstream.txt"), "from peer\n").unwrap();
        svc.stage_all(&peer).await.unwrap();
        svc.commit(&peer, "add upstream.txt", false).await.unwrap();
        svc.push(&peer).await.unwrap();

        std::fs::write(local.join("shared.txt"), "line 1\nline 2 local draft\n").unwrap();
        std::fs::write(local.join("scratch.txt"), "untracked note\n").unwrap();

        let clean_res = svc
            .pull_smart(&local, PullStrategy::FastForwardOnly, true, false)
            .await
            .unwrap();
        assert!(clean_res.auto_stashed);
        assert!(!clean_res.pull_conflicted);
        assert!(!clean_res.stash_pop_conflicted);
        assert_eq!(
            std::fs::read_to_string(local.join("upstream.txt")).unwrap(),
            "from peer\n"
        );
        assert_eq!(
            std::fs::read_to_string(local.join("shared.txt")).unwrap(),
            "line 1\nline 2 local draft\n"
        );
        assert_eq!(
            std::fs::read_to_string(local.join("scratch.txt")).unwrap(),
            "untracked note\n"
        );
        assert!(svc.list_stashes(&local).await.unwrap().is_empty());

        // 2. Diverged --ff-only failure rolls back temporary auto-stash so dirty files stay in working tree
        std::fs::write(local.join("local_commit.txt"), "ahead\n").unwrap();
        svc.stage(&local, &["local_commit.txt".to_string()])
            .await
            .unwrap();
        svc.commit(&local, "local ahead commit", false)
            .await
            .unwrap();

        std::fs::write(peer.join("shared.txt"), "line 1\nline 2 upstream conflict\n").unwrap();
        svc.stage_all(&peer).await.unwrap();
        svc.commit(&peer, "modify shared.txt upstream", false)
            .await
            .unwrap();
        svc.push(&peer).await.unwrap();

        let ff_err = svc
            .pull_smart(&local, PullStrategy::FastForwardOnly, true, false)
            .await;
        assert!(ff_err.is_err(), "diverged --ff-only should fail");
        assert_eq!(
            std::fs::read_to_string(local.join("shared.txt")).unwrap(),
            "line 1\nline 2 local draft\n",
            "auto-stash must be popped back when pull fails cleanly"
        );
        assert!(svc.list_stashes(&local).await.unwrap().is_empty());

        // 3. Rebase + Auto-Stash where local uncommitted edit conflicts with upstream commit:
        //    rebase succeeds, stash pop reports conflict, conflict markers are written, AND stash@{0} is preserved!
        let conflict_res = svc
            .pull_smart(&local, PullStrategy::Rebase, true, false)
            .await
            .unwrap();
        assert!(conflict_res.auto_stashed);
        assert!(!conflict_res.pull_conflicted);
        assert!(conflict_res.stash_pop_conflicted);

        let merged_text = std::fs::read_to_string(local.join("shared.txt")).unwrap();
        assert!(has_conflict_markers(&merged_text));
        let stashes = svc.list_stashes(&local).await.unwrap();
        assert_eq!(
            stashes.len(),
            1,
            "Git must preserve stash@{{0}} when stash pop conflicts"
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
