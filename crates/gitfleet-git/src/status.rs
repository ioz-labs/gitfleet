use crate::models::{AmendTarget, DiscardTarget, FileChange, FileChangeKind, RepoStatusSummary};
use crate::{GitService, runner};
use gitfleet_core::{AppError, AppResult};
use std::path::Path;

/// Lightweight, zero-spawn HEAD reader for initial bootstrap.
pub fn inspect_basic_status(repo_path: &Path) -> AppResult<RepoStatusSummary> {
    let head_file = repo_path.join(".git").join("HEAD");
    if !head_file.exists() {
        return Ok(RepoStatusSummary::default());
    }
    let content = std::fs::read_to_string(&head_file)?;
    let trimmed = content.trim();
    if let Some(ref_path) = trimmed.strip_prefix("ref: ") {
        let branch = ref_path
            .strip_prefix("refs/heads/")
            .unwrap_or(ref_path)
            .to_string();
        Ok(RepoStatusSummary {
            branch: Some(branch),
            detached: false,
            is_clean: true,
            ..Default::default()
        })
    } else {
        Ok(RepoStatusSummary {
            branch: None,
            head_oid: Some(trimmed.to_string()),
            detached: true,
            is_clean: true,
            ..Default::default()
        })
    }
}

impl GitService {
    pub async fn get_status(&self, repo_path: &Path) -> AppResult<RepoStatusSummary> {
        let out = runner::run_git_ok(
            repo_path,
            &["status", "--porcelain=v2", "--branch", "-uall"],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;

        let mut summary = parse_porcelain_v2(&out);
        let git_dir = repo_path.join(".git");
        summary.in_merge = git_dir.join("MERGE_HEAD").exists();
        summary.in_rebase =
            git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists();
        if let Ok(mut cache) = self.status_cache.write() {
            cache.insert(repo_path.to_path_buf(), summary.clone());
        }
        Ok(summary)
    }

    /// Lists all tracked + non-ignored untracked files in the repository (for All Files explorer).
    pub async fn list_repo_files(&self, repo_path: &Path) -> AppResult<Vec<String>> {
        let out = runner::run_git_ok(
            repo_path,
            &["ls-files", "--cached", "--others", "--exclude-standard"],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;
        let mut files: Vec<String> = out
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.to_string())
            .collect();
        files.sort();
        Ok(files)
    }

    pub async fn stage(&self, repo_path: &Path, paths: &[String]) -> AppResult<()> {
        let mut args = vec!["add", "--"];
        let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        args.extend(refs);
        runner::run_git_ok(
            repo_path,
            &args,
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await?;
        Ok(())
    }

    pub async fn stage_all(&self, repo_path: &Path) -> AppResult<()> {
        runner::run_git_ok(
            repo_path,
            &["add", "-A"],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await?;
        Ok(())
    }

    pub async fn unstage(&self, repo_path: &Path, paths: &[String]) -> AppResult<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let has_head = runner::run_git_ok(
            repo_path,
            &["rev-parse", "--verify", "--quiet", "HEAD"],
            None,
            None,
            false,
        )
        .await
        .is_ok();
        let mut args = if has_head {
            vec!["restore", "--staged", "--"]
        } else {
            vec!["rm", "--cached", "-r", "--"]
        };
        let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        args.extend(refs);
        runner::run_git_ok(
            repo_path,
            &args,
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await?;
        Ok(())
    }

    pub async fn unstage_all(&self, repo_path: &Path) -> AppResult<()> {
        let has_head = runner::run_git_ok(
            repo_path,
            &["rev-parse", "--verify", "--quiet", "HEAD"],
            None,
            None,
            false,
        )
        .await
        .is_ok();
        let args: &[&str] = if has_head {
            &["reset", "HEAD"]
        } else {
            &["rm", "--cached", "-r", "."]
        };
        runner::run_git_ok(
            repo_path,
            args,
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await?;
        Ok(())
    }

    pub async fn discard(&self, repo_path: &Path, path: &str, untracked: bool) -> AppResult<()> {
        if untracked {
            let full = repo_path.join(path);
            if full.is_dir() {
                std::fs::remove_dir_all(&full)?;
            } else if full.exists() {
                std::fs::remove_file(&full)?;
            }
            return Ok(());
        }
        runner::run_git_ok(
            repo_path,
            &["checkout", "--", path],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await?;
        Ok(())
    }

    /// Discards selected local or staged file changes back to `HEAD` or `Index`.
    /// Each entry is `(relative_path, is_untracked, is_added_in_index)`.
    pub async fn discard_changes(
        &self,
        repo_path: &Path,
        items: &[(String, bool, bool)],
        target: DiscardTarget,
    ) -> AppResult<()> {
        if items.is_empty() {
            return Ok(());
        }

        let mut untracked_paths: Vec<&str> = Vec::new();
        let mut added_staged_paths: Vec<&str> = Vec::new();
        let mut tracked_paths: Vec<&str> = Vec::new();

        for (path, is_untracked, is_added_staged) in items {
            if path.is_empty() || path.contains("..") {
                continue;
            }
            if *is_untracked {
                untracked_paths.push(path.as_str());
            } else if *is_added_staged && target == DiscardTarget::Head {
                added_staged_paths.push(path.as_str());
            } else {
                tracked_paths.push(path.as_str());
            }
        }

        for path in untracked_paths {
            let full = repo_path.join(path);
            if full.is_dir() {
                std::fs::remove_dir_all(&full)?;
            } else if full.exists() {
                std::fs::remove_file(&full)?;
            }
        }

        if !added_staged_paths.is_empty() {
            let mut args = vec!["rm", "-f", "--"];
            args.extend(added_staged_paths.iter().copied());
            runner::run_git_ok(
                repo_path,
                &args,
                Some(&self.command_log),
                Some(&self.events),
                true,
            )
            .await?;
        }

        if !tracked_paths.is_empty() {
            let mut args = match target {
                DiscardTarget::Head => vec!["restore", "--source=HEAD", "--staged", "--worktree", "--"],
                DiscardTarget::Index => vec!["restore", "--worktree", "--"],
            };
            args.extend(tracked_paths.iter().copied());
            runner::run_git_ok(
                repo_path,
                &args,
                Some(&self.command_log),
                Some(&self.events),
                true,
            )
            .await?;
        }

        Ok(())
    }

    pub async fn commit(
        &self,
        repo_path: &Path,
        message: &str,
        amend: bool,
    ) -> AppResult<String> {
        let mut args = vec!["commit", "-F", "-"];
        if amend {
            args.push("--amend");
        }
        let normalized = message.replace("\r\n", "\n");
        let out = runner::run_git_with_stdin(
            repo_path,
            &args,
            Some(normalized.as_bytes()),
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await?;
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

    pub async fn commit_and_push(
        &self,
        repo_path: &Path,
        message: &str,
        amend: bool,
    ) -> AppResult<String> {
        self.commit(repo_path, message, amend).await?;
        self.push(repo_path).await
    }

    /// Stages all working tree changes (`git add -A`) and commits (optionally pushing afterwards).
    pub async fn commit_all_changes(
        &self,
        repo_path: &Path,
        message: &str,
        amend: bool,
        push_after: bool,
    ) -> AppResult<String> {
        self.stage_all(repo_path).await?;
        if push_after {
            self.commit_and_push(repo_path, message, amend).await
        } else {
            self.commit(repo_path, message, amend).await
        }
    }

    /// Applies explicit file stage/unstage selections from the Commit popup and commits.
    pub async fn commit_with_selection(
        &self,
        repo_path: &Path,
        stage_paths: &[String],
        unstage_paths: &[String],
        message: &str,
        amend: bool,
        push_after: bool,
    ) -> AppResult<String> {
        if !unstage_paths.is_empty() {
            self.unstage(repo_path, unstage_paths).await?;
        }
        if !stage_paths.is_empty() {
            self.stage(repo_path, stage_paths).await?;
        }
        if push_after {
            self.commit_and_push(repo_path, message, amend).await
        } else {
            self.commit(repo_path, message, amend).await
        }
    }

    pub async fn amend_target(&self, repo_path: &Path) -> AppResult<AmendTarget> {
        let out = runner::run_git_ok(
            repo_path,
            &["log", "-1", "--format=%h%x1f%B"],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;
        let mut parts = out.splitn(2, '\x1f');
        let short_oid = parts.next().unwrap_or("").trim().to_string();
        let message = parts.next().unwrap_or("").trim().to_string();

        let pushed = runner::run_git(
            repo_path,
            &["merge-base", "--is-ancestor", "HEAD", "@{u}"],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await
        .map(|o| o.exit_code == 0)
        .unwrap_or(false);

        Ok(AmendTarget {
            short_oid,
            message,
            pushed_to_upstream: pushed,
        })
    }
}

pub(crate) fn char_to_kind(c: char) -> FileChangeKind {
    match c {
        'A' => FileChangeKind::Added,
        'D' => FileChangeKind::Deleted,
        'R' => FileChangeKind::Renamed,
        'C' => FileChangeKind::Copied,
        _ => FileChangeKind::Modified,
    }
}

fn parse_porcelain_v2(output: &str) -> RepoStatusSummary {
    let mut s = RepoStatusSummary::default();
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("# branch.oid ") {
            if rest != "(initial)" {
                s.head_oid = Some(rest.chars().take(8).collect());
            }
        } else if let Some(rest) = line.strip_prefix("# branch.head ") {
            if rest == "(detached)" {
                s.detached = true;
                s.branch = None;
            } else {
                s.detached = false;
                s.branch = Some(rest.to_string());
            }
        } else if let Some(rest) = line.strip_prefix("# branch.upstream ") {
            s.upstream = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            for tok in rest.split_whitespace() {
                if let Some(a) = tok.strip_prefix('+') {
                    s.ahead = a.parse().unwrap_or(0);
                } else if let Some(b) = tok.strip_prefix('-') {
                    s.behind = b.parse().unwrap_or(0);
                }
            }
        } else if let Some(rest) = line.strip_prefix("1 ") {
            let mut parts = rest.splitn(8, ' ');
            let xy = parts.next().unwrap_or("..");
            let path = parts.nth(6).unwrap_or("").to_string();
            let mut chars = xy.chars();
            let x = chars.next().unwrap_or('.');
            let y = chars.next().unwrap_or('.');
            if x != '.' {
                s.staged_files.push(FileChange {
                    path: path.clone(),
                    kind: char_to_kind(x),
                    staged: true,
                });
            }
            if y != '.' {
                s.unstaged_files.push(FileChange {
                    path,
                    kind: char_to_kind(y),
                    staged: false,
                });
            }
        } else if let Some(rest) = line.strip_prefix("2 ") {
            let mut parts = rest.splitn(9, ' ');
            let xy = parts.next().unwrap_or("..");
            let path_field = parts.nth(7).unwrap_or("");
            let path = path_field.split('\t').next().unwrap_or(path_field).to_string();
            let mut chars = xy.chars();
            let x = chars.next().unwrap_or('.');
            let y = chars.next().unwrap_or('.');
            if x != '.' {
                s.staged_files.push(FileChange {
                    path: path.clone(),
                    kind: char_to_kind(x),
                    staged: true,
                });
            }
            if y != '.' {
                s.unstaged_files.push(FileChange {
                    path,
                    kind: char_to_kind(y),
                    staged: false,
                });
            }
        } else if let Some(rest) = line.strip_prefix("u ") {
            let mut parts = rest.splitn(10, ' ');
            let path = parts.nth(9).unwrap_or("").to_string();
            s.conflict_files.push(FileChange {
                path,
                kind: FileChangeKind::Conflicted,
                staged: false,
            });
        } else if let Some(path) = line.strip_prefix("? ") {
            s.unstaged_files.push(FileChange {
                path: path.to_string(),
                kind: FileChangeKind::Untracked,
                staged: false,
            });
            s.untracked_count += 1;
        }
    }

    s.staged_count = s.staged_files.len();
    s.unstaged_count = s
        .unstaged_files
        .iter()
        .filter(|f| f.kind != FileChangeKind::Untracked)
        .count();
    s.conflict_count = s.conflict_files.len();
    s.is_clean = s.staged_count == 0
        && s.unstaged_count == 0
        && s.untracked_count == 0
        && s.conflict_count == 0;
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitfleet_core::{CommandLogStore, EventBus};
    use std::sync::Arc;

    #[tokio::test]
    async fn commits_and_amends_multiline_message_intact() {
        let dir = std::env::temp_dir().join(format!(
            "gitfleet_multiline_commit_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let svc = GitService::new(EventBus::new(16), Arc::new(CommandLogStore::new()));
        runner::run_git_ok(&dir, &["init"], None, None, false)
            .await
            .unwrap();
        runner::run_git_ok(&dir, &["config", "user.name", "GitFleet Test"], None, None, false)
            .await
            .unwrap();
        runner::run_git_ok(
            &dir,
            &["config", "user.email", "test@gitfleet.local"],
            None,
            None,
            false,
        )
        .await
        .unwrap();

        std::fs::write(dir.join("hello.txt"), "hello\n").unwrap();
        svc.stage(&dir, &["hello.txt".to_string()]).await.unwrap();

        let msg = "feat: support multi-line commit\n\n- Line 1 with \"quotes\"\n- Line 2 with -leading-dash\n\nSigned-off-by: Dev <dev@example.com>";
        svc.commit(&dir, msg, false).await.unwrap();

        let target = svc.amend_target(&dir).await.unwrap();
        assert_eq!(target.message, msg);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn discards_local_and_staged_changes_to_index_or_head() {
        let dir = std::env::temp_dir().join(format!(
            "gitfleet_discard_changes_{}",
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
        runner::run_git_ok(&dir, &["config", "core.autocrlf", "false"], None, None, false)
            .await
            .unwrap();

        std::fs::write(dir.join("a.txt"), "v1\n").unwrap();
        svc.stage(&dir, &["a.txt".into()]).await.unwrap();
        svc.commit(&dir, "init", false).await.unwrap();

        // Stage v2, then modify working tree to v3
        std::fs::write(dir.join("a.txt"), "v2\n").unwrap();
        svc.stage(&dir, &["a.txt".into()]).await.unwrap();
        std::fs::write(dir.join("a.txt"), "v3\n").unwrap();

        // Discard to Index: working tree becomes v2, staged v2 stays!
        svc.discard_changes(
            &dir,
            &[("a.txt".into(), false, false)],
            DiscardTarget::Index,
        )
        .await
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "v2\n"
        );

        // Add newly staged file + untracked file
        std::fs::write(dir.join("new_staged.txt"), "ns\n").unwrap();
        svc.stage(&dir, &["new_staged.txt".into()]).await.unwrap();
        std::fs::write(dir.join("untracked.txt"), "u\n").unwrap();

        // Discard all to HEAD: a.txt -> v1, new_staged.txt removed, untracked.txt removed!
        svc.discard_changes(
            &dir,
            &[
                ("a.txt".into(), false, false),
                ("new_staged.txt".into(), false, true),
                ("untracked.txt".into(), true, false),
            ],
            DiscardTarget::Head,
        )
        .await
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("a.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "v1\n"
        );
        assert!(!dir.join("new_staged.txt").exists());
        assert!(!dir.join("untracked.txt").exists());
        let st = svc.get_status(&dir).await.unwrap();
        assert!(st.is_clean);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn stage_and_unstage_multiple_files_work_on_unborn_repo() {
        let dir = std::env::temp_dir().join(format!(
            "gitfleet_unborn_stage_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let svc = GitService::new(EventBus::new(16), Arc::new(CommandLogStore::new()));
        runner::run_git_ok(&dir, &["init", "-b", "main"], None, None, false)
            .await
            .unwrap();
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        std::fs::write(dir.join("b.txt"), "b").unwrap();
        std::fs::write(dir.join("c.txt"), "c").unwrap();

        svc.stage(&dir, &["a.txt".into(), "b.txt".into(), "c.txt".into()])
            .await
            .unwrap();
        let st1 = svc.get_status(&dir).await.unwrap();
        assert_eq!(st1.staged_count, 3);

        svc.unstage(&dir, &["a.txt".into(), "c.txt".into()])
            .await
            .unwrap();
        let st2 = svc.get_status(&dir).await.unwrap();
        assert_eq!(st2.staged_count, 1);
        assert_eq!(st2.staged_files[0].path, "b.txt");

        svc.unstage_all(&dir).await.unwrap();
        let st3 = svc.get_status(&dir).await.unwrap();
        assert_eq!(st3.staged_count, 0);
        assert_eq!(st3.untracked_count, 3);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
