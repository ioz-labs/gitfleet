use crate::models::{
    AmendTarget, DiscardTarget, FileChange, FileChangeKind, IgnoredDirListing, IgnoredEntries,
    RepoStatusSummary,
};
use crate::{GitService, runner};
use gitfleet_core::{AppError, AppResult};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path};

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
            &[
                "-c",
                "core.quotePath=false",
                "status",
                "--porcelain=v2",
                "--branch",
                "-uall",
                "-z",
            ],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;

        let (mut summary, _) = parse_porcelain_v2_z(&out);
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
            &[
                "-c",
                "core.quotePath=false",
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;
        let mut files: Vec<String> = out
            .split('\0')
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect();
        files.sort();
        Ok(files)
    }

    /// Evaluates `git check-ignore -v --no-index -- <test_path>` so Ignore Studio can preview
    /// the exact `<source>:<line>:<pattern>` rule matching a candidate path in real time.
    pub async fn check_ignore_path(
        &self,
        repo_path: &Path,
        test_path: &str,
    ) -> AppResult<Option<String>> {
        let cleaned = test_path.trim().trim_start_matches(['/', '\\']);
        if cleaned.is_empty() {
            return Ok(None);
        }
        let out = runner::run_git(
            repo_path,
            &[
                "-c",
                "core.quotePath=false",
                "check-ignore",
                "-v",
                "--no-index",
                "--",
                cleaned,
            ],
            None,
            None,
            false,
        )
        .await?;
        if out.exit_code == 0 {
            let line = out.stdout.lines().next().unwrap_or("").trim();
            let rule_part = line.split('\t').next().unwrap_or(line).trim();
            Ok(Some(if rule_part.is_empty() {
                "Matched ignore rule".to_string()
            } else {
                rule_part.to_string()
            }))
        } else {
            Ok(None)
        }
    }

    /// Lists gitignored files and collapsed gitignored directories (`--ignored=matching`) without
    /// eagerly recursing into massive ignored build/dependency trees like `target/` or `node_modules/`.
    /// Works both at a repository root and inside any subdirectory of a Git repository.
    pub async fn list_ignored_entries(&self, repo_path: &Path) -> AppResult<IgnoredEntries> {
        let subdir_prefix = if repo_path.join(".git").exists() {
            String::new()
        } else {
            runner::run_git_ok(
                repo_path,
                &["rev-parse", "--show-prefix"],
                None,
                None,
                false,
            )
            .await?
            .trim()
            .replace('\\', "/")
        };

        let out = runner::run_git(
            repo_path,
            &[
                "-c",
                "core.quotePath=false",
                "-c",
                "status.relativePaths=false",
                "status",
                "--porcelain=v2",
                "--ignored=matching",
                "-uall",
                "-z",
                "--",
                ".",
            ],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;
        if out.exit_code == 0 {
            let mut entries = parse_ignored_entries_v2_z(&out.stdout);
            if !subdir_prefix.is_empty() {
                entries = strip_ignored_prefix(entries, &subdir_prefix);
            }
            return Ok(entries);
        }

        // Fallback for older Git (< 2.16) that lacks `--ignored=matching`.
        // `ls-files` already outputs paths relative to `repo_path`.
        let fallback = runner::run_git_ok(
            repo_path,
            &[
                "-c",
                "core.quotePath=false",
                "ls-files",
                "-z",
                "--others",
                "--ignored",
                "--exclude-standard",
                "--directory",
            ],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;
        Ok(parse_ls_files_ignored_directory_z(&fallback))
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
    /// Each entry is `(relative_path, is_untracked_hint, is_added_in_index_hint)`.
    /// Live Git status (`--porcelain=v2 -z`) and `HEAD` tree are inspected directly so renames,
    /// copies, intent-to-add, unmerged conflicts, unborn repos, and Unicode paths all discard cleanly.
    pub async fn discard_changes(
        &self,
        repo_path: &Path,
        items: &[(String, bool, bool)],
        target: DiscardTarget,
    ) -> AppResult<()> {
        if items.is_empty() {
            return Ok(());
        }

        let mut requested: Vec<(&str, bool, bool)> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();
        for (path, is_untracked_hint, is_added_hint) in items {
            if !is_safe_rel_path(path) || !seen.insert(path.as_str()) {
                continue;
            }
            requested.push((path.as_str(), *is_untracked_hint, *is_added_hint));
        }
        if requested.is_empty() {
            return Ok(());
        }

        let live_raw = runner::run_git_ok(
            repo_path,
            &[
                "-c",
                "core.quotePath=false",
                "status",
                "--porcelain=v2",
                "-uall",
                "-z",
            ],
            None,
            None,
            false,
        )
        .await
        .unwrap_or_default();
        let (_, live_map) = parse_porcelain_v2_z(&live_raw);

        match target {
            DiscardTarget::Index => {
                let mut delete_disk: Vec<String> = Vec::new();
                let mut rm_cached_and_disk: Vec<String> = Vec::new();
                let mut unmerged_paths: Vec<String> = Vec::new();
                let mut restore_worktree: Vec<String> = Vec::new();

                for (path, is_untracked_hint, _) in requested {
                    if let Some(live) = live_map.get(path) {
                        if live.is_untracked {
                            delete_disk.push(path.to_string());
                        } else if live.is_unmerged {
                            unmerged_paths.push(path.to_string());
                        } else if live.staged_char == 'D' {
                            // Path is deleted in Index — matching Index means removing any recreated copy on disk
                            delete_disk.push(path.to_string());
                        } else if live.staged_char == '.' && live.worktree_char == 'A' {
                            // Intent-to-add (`git add -N`): remove placeholder from index + delete file
                            rm_cached_and_disk.push(path.to_string());
                        } else if live.worktree_char != '.' {
                            restore_worktree.push(path.to_string());
                        } else {
                            // Staged-only change with clean worktree relative to Index: restoring worktree is a safe no-op
                            restore_worktree.push(path.to_string());
                        }
                    } else if is_untracked_hint {
                        delete_disk.push(path.to_string());
                    } else {
                        restore_worktree.push(path.to_string());
                    }
                }

                for path in &delete_disk {
                    remove_path_on_disk(repo_path, path)?;
                }

                if !rm_cached_and_disk.is_empty() {
                    let mut args = vec!["rm", "-f", "--cached", "--ignore-unmatch", "-r", "--"];
                    args.extend(rm_cached_and_disk.iter().map(String::as_str));
                    runner::run_git_ok(
                        repo_path,
                        &args,
                        Some(&self.command_log),
                        Some(&self.events),
                        true,
                    )
                    .await?;
                    for path in &rm_cached_and_disk {
                        remove_path_on_disk(repo_path, path)?;
                    }
                }

                for path in &unmerged_paths {
                    let out = runner::run_git(
                        repo_path,
                        &["checkout", "--ours", "--", path.as_str()],
                        Some(&self.command_log),
                        Some(&self.events),
                        true,
                    )
                    .await?;
                    if out.exit_code != 0 {
                        // Deleted-by-us / absent in stage 2 (e.g. DU, DD, UA) -> remove from worktree
                        remove_path_on_disk(repo_path, path)?;
                    }
                }

                if !restore_worktree.is_empty() {
                    let mut args = vec!["restore", "--worktree", "--"];
                    args.extend(restore_worktree.iter().map(String::as_str));
                    runner::run_git_ok(
                        repo_path,
                        &args,
                        Some(&self.command_log),
                        Some(&self.events),
                        true,
                    )
                    .await?;
                }
            }
            DiscardTarget::Head => {
                let has_head = runner::run_git_ok(
                    repo_path,
                    &["rev-parse", "--verify", "--quiet", "HEAD"],
                    None,
                    None,
                    false,
                )
                .await
                .is_ok();

                let mut head_lookup: Vec<String> = Vec::new();
                for (path, _, _) in &requested {
                    head_lookup.push((*path).to_string());
                    if let Some(orig) = live_map.get(*path).and_then(|e| e.rename_orig_path.as_ref())
                    {
                        head_lookup.push(orig.clone());
                    }
                }

                let mut in_head: HashSet<String> = HashSet::new();
                if has_head && !head_lookup.is_empty() {
                    let mut args = vec![
                        "-c",
                        "core.quotePath=false",
                        "ls-tree",
                        "-r",
                        "-z",
                        "--full-tree",
                        "--name-only",
                        "HEAD",
                        "--",
                    ];
                    args.extend(head_lookup.iter().map(String::as_str));
                    if let Ok(out) = runner::run_git_ok(repo_path, &args, None, None, false).await {
                        for p in out.split('\0').filter(|s| !s.is_empty()) {
                            in_head.insert(p.to_string());
                        }
                    }
                }

                // Fallback case-insensitive lookup in HEAD (handles Windows case mismatches like PCB vs pcb)
                let needs_ci_lookup = has_head
                    && requested.iter().any(|(path, is_u, is_a)| {
                        if *is_u || *is_a || in_head.contains(*path) {
                            return false;
                        }
                        live_map
                            .get(*path)
                            .is_none_or(|e| !e.is_untracked && e.staged_char != 'A' && e.staged_char != 'C')
                    });
                let mut head_ci_map: HashMap<String, String> = HashMap::new();
                if needs_ci_lookup
                    && let Ok(out) = runner::run_git_ok(
                        repo_path,
                        &[
                            "-c",
                            "core.quotePath=false",
                            "ls-tree",
                            "-r",
                            "-z",
                            "--full-tree",
                            "--name-only",
                            "HEAD",
                        ],
                        None,
                        None,
                        false,
                    )
                    .await
                {
                    for p in out.split('\0').filter(|s| !s.is_empty()) {
                        head_ci_map.insert(p.to_ascii_lowercase(), p.to_string());
                    }
                }

                let mut delete_disk_only: Vec<String> = Vec::new();
                let mut rm_index_and_disk: Vec<String> = Vec::new();
                let mut rm_cached_then_disk: Vec<String> = Vec::new();
                let mut unmerged_reset_head: Vec<String> = Vec::new();
                let mut restore_from_head: Vec<String> = Vec::new();
                let mut restore_seen: HashSet<String> = HashSet::new();

                for (path, is_untracked_hint, is_added_hint) in requested {
                    let live = live_map.get(path);

                    // 1. Staged rename (`2 R... new\0orig`): remove destination & restore original path from HEAD
                    if let Some(entry) = live
                        && entry.staged_char == 'R'
                        && let Some(orig) = &entry.rename_orig_path
                    {
                        if !in_head.contains(path) {
                            // Use --cached + manual disk delete so Windows case-only renames don't collide
                            rm_cached_then_disk.push(path.to_string());
                        } else if restore_seen.insert(path.to_string()) {
                            restore_from_head.push(path.to_string());
                        }
                        if has_head && in_head.contains(orig) && restore_seen.insert(orig.clone()) {
                            restore_from_head.push(orig.clone());
                        }
                        continue;
                    }

                    // 2. Untracked file
                    if live.is_some_and(|e| e.is_untracked) || (live.is_none() && is_untracked_hint)
                    {
                        delete_disk_only.push(path.to_string());
                        continue;
                    }

                    // 3. No HEAD (unborn repo) or path absent from HEAD (Added, Copied, intent-to-add, DU/DD/UA)
                    let canonical_head_path = if has_head {
                        if in_head.contains(path) {
                            Some(path.to_string())
                        } else {
                            head_ci_map.get(&path.to_ascii_lowercase()).cloned()
                        }
                    } else {
                        None
                    };

                    let Some(head_path) = canonical_head_path else {
                        if is_added_hint || live.is_some() || !has_head {
                            rm_index_and_disk.push(path.to_string());
                        } else {
                            delete_disk_only.push(path.to_string());
                        }
                        continue;
                    };

                    // 4. Path exists in HEAD (possibly with different casing on Windows)
                    if head_path != path {
                        rm_cached_then_disk.push(path.to_string());
                    }
                    if live.is_some_and(|e| e.is_unmerged) {
                        unmerged_reset_head.push(head_path.clone());
                    }
                    if restore_seen.insert(head_path.clone()) {
                        restore_from_head.push(head_path);
                    }
                }

                for path in &delete_disk_only {
                    remove_path_on_disk(repo_path, path)?;
                }

                if !rm_index_and_disk.is_empty() {
                    let mut args = vec!["rm", "-f", "--ignore-unmatch", "-r", "--"];
                    args.extend(rm_index_and_disk.iter().map(String::as_str));
                    runner::run_git_ok(
                        repo_path,
                        &args,
                        Some(&self.command_log),
                        Some(&self.events),
                        true,
                    )
                    .await?;
                    // Ensure files removed from index (e.g. intent-to-add or already missing from index) are also cleaned on disk
                    for path in &rm_index_and_disk {
                        remove_path_on_disk(repo_path, path)?;
                    }
                }

                if !rm_cached_then_disk.is_empty() {
                    let mut args = vec!["rm", "-f", "--cached", "--ignore-unmatch", "-r", "--"];
                    args.extend(rm_cached_then_disk.iter().map(String::as_str));
                    runner::run_git_ok(
                        repo_path,
                        &args,
                        Some(&self.command_log),
                        Some(&self.events),
                        true,
                    )
                    .await?;
                    for path in &rm_cached_then_disk {
                        remove_path_on_disk(repo_path, path)?;
                    }
                }

                if !unmerged_reset_head.is_empty() {
                    let mut args = vec!["reset", "-q", "HEAD", "--"];
                    args.extend(unmerged_reset_head.iter().map(String::as_str));
                    runner::run_git_ok(
                        repo_path,
                        &args,
                        Some(&self.command_log),
                        Some(&self.events),
                        true,
                    )
                    .await?;
                }

                if !restore_from_head.is_empty() {
                    let mut args = vec!["restore", "--source=HEAD", "--staged", "--worktree", "--"];
                    args.extend(restore_from_head.iter().map(String::as_str));
                    runner::run_git_ok(
                        repo_path,
                        &args,
                        Some(&self.command_log),
                        Some(&self.events),
                        true,
                    )
                    .await?;
                }
            }
        }

        Ok(())
    }

    pub async fn commit(
        &self,
        repo_path: &Path,
        message: &str,
        amend: bool,
    ) -> AppResult<String> {
        self.commit_signed(repo_path, message, amend, false).await
    }

    pub async fn commit_signed(
        &self,
        repo_path: &Path,
        message: &str,
        amend: bool,
        sign: bool,
    ) -> AppResult<String> {
        let mut args = vec!["commit", "-F", "-"];
        if amend {
            args.push("--amend");
        }
        if sign {
            args.push("-S");
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
        self.commit_and_push_signed(repo_path, message, amend, false)
            .await
    }

    pub async fn commit_and_push_signed(
        &self,
        repo_path: &Path,
        message: &str,
        amend: bool,
        sign: bool,
    ) -> AppResult<String> {
        self.commit_signed(repo_path, message, amend, sign).await?;
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
        self.commit_all_changes_signed(repo_path, message, amend, push_after, false)
            .await
    }

    pub async fn commit_all_changes_signed(
        &self,
        repo_path: &Path,
        message: &str,
        amend: bool,
        push_after: bool,
        sign: bool,
    ) -> AppResult<String> {
        self.stage_all(repo_path).await?;
        if push_after {
            self.commit_and_push_signed(repo_path, message, amend, sign)
                .await
        } else {
            self.commit_signed(repo_path, message, amend, sign).await
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
        self.commit_with_selection_signed(
            repo_path,
            stage_paths,
            unstage_paths,
            message,
            amend,
            push_after,
            false,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn commit_with_selection_signed(
        &self,
        repo_path: &Path,
        stage_paths: &[String],
        unstage_paths: &[String],
        message: &str,
        amend: bool,
        push_after: bool,
        sign: bool,
    ) -> AppResult<String> {
        if !unstage_paths.is_empty() {
            self.unstage(repo_path, unstage_paths).await?;
        }
        if !stage_paths.is_empty() {
            self.stage(repo_path, stage_paths).await?;
        }
        if push_after {
            self.commit_and_push_signed(repo_path, message, amend, sign)
                .await
        } else {
            self.commit_signed(repo_path, message, amend, sign).await
        }
    }

    /// Reads the configured commit message template (`git config commit.template`, `.gitmessage`,
    /// or `.github/COMMIT_TEMPLATE`), stripping `#` comment lines.
    pub async fn read_commit_template(&self, repo_path: &Path) -> Option<String> {
        let mut candidates: Vec<std::path::PathBuf> = Vec::new();
        if let Ok(cfg) = runner::run_git_ok(
            repo_path,
            &["config", "--get", "commit.template"],
            None,
            None,
            false,
        )
        .await
        {
            let trimmed = cfg.trim();
            if !trimmed.is_empty() {
                if let Some(rest) = trimmed.strip_prefix("~/").or_else(|| trimmed.strip_prefix("~\\"))
                    && let Some(home) = std::env::var_os("USERPROFILE")
                        .or_else(|| std::env::var_os("HOME"))
                {
                    candidates.push(std::path::PathBuf::from(home).join(rest));
                } else {
                    let p = Path::new(trimmed);
                    if p.is_absolute() {
                        candidates.push(p.to_path_buf());
                    } else {
                        candidates.push(repo_path.join(p));
                    }
                }
            }
        }
        candidates.push(repo_path.join(".gitmessage"));
        candidates.push(repo_path.join(".github").join("COMMIT_TEMPLATE"));

        for path in candidates {
            if let Ok(raw) = std::fs::read_to_string(&path) {
                let cleaned: Vec<&str> = raw
                    .lines()
                    .filter(|l| !l.trim_start().starts_with('#'))
                    .collect();
                let joined = cleaned.join("\n").trim().to_string();
                if !joined.is_empty() {
                    return Some(joined);
                }
            }
        }
        None
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

#[derive(Debug, Clone)]
struct LiveStatusEntry {
    staged_char: char,
    worktree_char: char,
    is_untracked: bool,
    is_unmerged: bool,
    rename_orig_path: Option<String>,
}

fn is_safe_rel_path(path: &str) -> bool {
    !path.is_empty()
        && !Path::new(path).components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
}

fn remove_path_on_disk(repo_path: &Path, rel_path: &str) -> AppResult<()> {
    let full = repo_path.join(rel_path);
    match std::fs::symlink_metadata(&full) {
        Ok(meta) => {
            if meta.is_dir() && !meta.file_type().is_symlink() {
                std::fs::remove_dir_all(&full)?;
            } else {
                std::fs::remove_file(&full)?;
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(())
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

fn parse_porcelain_v2_z(output: &str) -> (RepoStatusSummary, HashMap<String, LiveStatusEntry>) {
    let mut s = RepoStatusSummary::default();
    let mut live: HashMap<String, LiveStatusEntry> = HashMap::new();
    let mut records = output.split('\0');

    while let Some(rec) = records.next() {
        if rec.is_empty() {
            continue;
        }
        if let Some(rest) = rec.strip_prefix("# branch.oid ") {
            if rest != "(initial)" {
                s.head_oid = Some(rest.chars().take(8).collect());
            }
        } else if let Some(rest) = rec.strip_prefix("# branch.head ") {
            if rest == "(detached)" {
                s.detached = true;
                s.branch = None;
            } else {
                s.detached = false;
                s.branch = Some(rest.to_string());
            }
        } else if let Some(rest) = rec.strip_prefix("# branch.upstream ") {
            s.upstream = Some(rest.to_string());
        } else if let Some(rest) = rec.strip_prefix("# branch.ab ") {
            for tok in rest.split_whitespace() {
                if let Some(a) = tok.strip_prefix('+') {
                    s.ahead = a.parse().unwrap_or(0);
                } else if let Some(b) = tok.strip_prefix('-') {
                    s.behind = b.parse().unwrap_or(0);
                }
            }
        } else if let Some(rest) = rec.strip_prefix("1 ") {
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
                    path: path.clone(),
                    kind: char_to_kind(y),
                    staged: false,
                });
            }
            live.insert(
                path,
                LiveStatusEntry {
                    staged_char: x,
                    worktree_char: y,
                    is_untracked: false,
                    is_unmerged: false,
                    rename_orig_path: None,
                },
            );
        } else if let Some(rest) = rec.strip_prefix("2 ") {
            let mut parts = rest.splitn(9, ' ');
            let xy = parts.next().unwrap_or("..");
            let path = parts.nth(7).unwrap_or("").to_string();
            // In `-z` mode, `<origPath>` is the next NUL-delimited record!
            let orig_path = records.next().filter(|p| !p.is_empty()).map(str::to_string);
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
                    path: path.clone(),
                    kind: char_to_kind(y),
                    staged: false,
                });
            }
            live.insert(
                path,
                LiveStatusEntry {
                    staged_char: x,
                    worktree_char: y,
                    is_untracked: false,
                    is_unmerged: false,
                    rename_orig_path: orig_path,
                },
            );
        } else if let Some(rest) = rec.strip_prefix("u ") {
            let mut parts = rest.splitn(10, ' ');
            let xy = parts.next().unwrap_or("UU");
            let path = parts.nth(8).unwrap_or("").to_string();
            let mut chars = xy.chars();
            let x = chars.next().unwrap_or('U');
            let y = chars.next().unwrap_or('U');
            s.conflict_files.push(FileChange {
                path: path.clone(),
                kind: FileChangeKind::Conflicted,
                staged: false,
            });
            live.insert(
                path,
                LiveStatusEntry {
                    staged_char: x,
                    worktree_char: y,
                    is_untracked: false,
                    is_unmerged: true,
                    rename_orig_path: None,
                },
            );
        } else if let Some(path) = rec.strip_prefix("? ") {
            let path_s = path.to_string();
            s.unstaged_files.push(FileChange {
                path: path_s.clone(),
                kind: FileChangeKind::Untracked,
                staged: false,
            });
            s.untracked_count += 1;
            live.entry(path_s).or_insert(LiveStatusEntry {
                staged_char: '?',
                worktree_char: '?',
                is_untracked: true,
                is_unmerged: false,
                rename_orig_path: None,
            });
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
    (s, live)
}

/// Parses `git status --porcelain=v2 --ignored=matching -uall -z` output for `! ` ignored entries.
pub(crate) fn parse_ignored_entries_v2_z(output: &str) -> IgnoredEntries {
    let mut entries = IgnoredEntries::default();
    let mut records = output.split('\0');

    while let Some(rec) = records.next() {
        if rec.is_empty() {
            continue;
        }
        if rec.starts_with("2 ") {
            // In `-z` mode, rename/copy records are immediately followed by `<origPath>\0`.
            let _ = records.next();
            continue;
        }
        if let Some(raw) = rec.strip_prefix("! ") {
            let norm = raw.replace('\\', "/");
            if let Some(dir_raw) = norm.strip_suffix('/') {
                let dir = dir_raw.trim_end_matches('/');
                if !dir.is_empty() && dir != ".git" && !dir.starts_with(".git/") {
                    entries.dirs.push(dir.to_string());
                }
            } else if !norm.is_empty() && norm != ".git" && !norm.starts_with(".git/") {
                entries.files.push(norm);
            }
        }
    }

    entries.files.sort();
    entries.files.dedup();
    entries.dirs.sort();
    entries.dirs.dedup();
    entries
}

fn strip_ignored_prefix(entries: IgnoredEntries, prefix: &str) -> IgnoredEntries {
    let norm_prefix = if prefix.ends_with('/') {
        prefix.to_string()
    } else {
        format!("{prefix}/")
    };
    let files = entries
        .files
        .into_iter()
        .filter_map(|p| p.strip_prefix(&norm_prefix).map(str::to_string))
        .filter(|p| !p.is_empty())
        .collect();
    let dirs = entries
        .dirs
        .into_iter()
        .filter_map(|p| p.strip_prefix(&norm_prefix).map(str::to_string))
        .filter(|p| !p.is_empty())
        .collect();
    IgnoredEntries { files, dirs }
}

/// Fallback parser for `git ls-files -z --others --ignored --exclude-standard --directory`.
fn parse_ls_files_ignored_directory_z(output: &str) -> IgnoredEntries {
    let mut files = Vec::new();
    let mut raw_dirs = Vec::new();

    for rec in output.split('\0') {
        if rec.is_empty() {
            continue;
        }
        let norm = rec.replace('\\', "/");
        if let Some(dir_raw) = norm.strip_suffix('/') {
            let dir = dir_raw.trim_end_matches('/');
            if !dir.is_empty() && dir != ".git" && !dir.starts_with(".git/") {
                raw_dirs.push(dir.to_string());
            }
        } else if norm != ".git" && !norm.starts_with(".git/") {
            files.push(norm);
        }
    }

    files.sort();
    files.dedup();
    let mut dirs: Vec<String> = raw_dirs
        .into_iter()
        .filter(|d| {
            let prefix = format!("{d}/");
            !files.iter().any(|f| f.starts_with(&prefix))
        })
        .collect();
    dirs.sort();
    dirs.dedup();
    IgnoredEntries { files, dirs }
}

/// Lists direct children (one directory level, non-recursive) inside a gitignored directory
/// so `All Files` can lazily explore folders like `.vscode/` or `target/` without walking
/// tens of thousands of build/dependency files upfront.
pub fn list_ignored_dir_children(
    repo_path: &Path,
    rel_dir: &str,
    max_entries: usize,
) -> AppResult<IgnoredDirListing> {
    let norm = rel_dir.replace('\\', "/");
    let trimmed = norm.trim_matches('/');
    if !is_safe_rel_path(trimmed) || trimmed == ".git" || trimmed.starts_with(".git/") {
        return Err(AppError::Git(format!(
            "Invalid relative directory path: {rel_dir}"
        )));
    }

    let full_dir = repo_path.join(trimmed);
    let read_dir = match std::fs::read_dir(&full_dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(IgnoredDirListing::default());
        }
        Err(e) => return Err(e.into()),
    };

    let limit = max_entries.max(1);
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    let mut truncated = false;

    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.is_empty() || name == ".git" {
            continue;
        }
        if files.len() + dirs.len() >= limit {
            truncated = true;
            break;
        }
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        let child_rel = format!("{trimmed}/{name}");
        if ft.is_dir() {
            dirs.push(child_rel);
        } else if ft.is_file() || (ft.is_symlink() && entry.path().is_file()) {
            files.push(child_rel);
        }
    }

    files.sort();
    dirs.sort();
    Ok(IgnoredDirListing {
        files,
        dirs,
        truncated,
    })
}

/// Safely deletes a file or directory (recursively) inside `root` by relative path.
/// Rejects empty paths, path traversal (`..`), absolute paths, and `.git` metadata.
/// Returns `Ok(true)` if an entry was removed, or `Ok(false)` if it was already absent.
pub fn delete_workspace_entry(root: &Path, rel_path: &str) -> AppResult<bool> {
    let norm = rel_path.replace('\\', "/");
    let trimmed = norm.trim_matches('/');
    if trimmed.is_empty()
        || !is_safe_rel_path(trimmed)
        || trimmed == ".git"
        || trimmed.starts_with(".git/")
    {
        return Err(AppError::Git(format!(
            "Refusing to delete unsafe or protected path: '{rel_path}'"
        )));
    }
    let full = root.join(trimmed);
    match std::fs::symlink_metadata(&full) {
        Ok(meta) => {
            if meta.is_dir() && !meta.file_type().is_symlink() {
                std::fs::remove_dir_all(&full)?;
            } else {
                std::fs::remove_file(&full)?;
            }
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// Safely renames a file or directory in-place inside `root` (keeping the same parent directory).
/// Rejects empty/unsafe source paths, invalid target names, path separators in `new_name`,
/// `.git` metadata, and accidental overwrites of distinct existing entries.
/// Supports case-only renames on case-insensitive filesystems (e.g. Windows NTFS).
/// Returns the new normalized relative path (using `/` separators).
pub fn rename_workspace_entry(root: &Path, rel_path: &str, new_name: &str) -> AppResult<String> {
    let norm = rel_path.replace('\\', "/");
    let trimmed = norm.trim_matches('/');
    if trimmed.is_empty()
        || !is_safe_rel_path(trimmed)
        || trimmed == ".git"
        || trimmed.starts_with(".git/")
    {
        return Err(AppError::Git(format!(
            "Refusing to rename unsafe or protected path: '{rel_path}'"
        )));
    }

    let cleaned_name = new_name.trim();
    if cleaned_name.is_empty()
        || cleaned_name == "."
        || cleaned_name == ".."
        || cleaned_name == ".git"
        || cleaned_name.contains('/')
        || cleaned_name.contains('\\')
        || cleaned_name.contains('\0')
    {
        return Err(AppError::Git(format!(
            "Invalid name: '{new_name}'. Provide a single file or folder name without path separators."
        )));
    }

    #[cfg(target_os = "windows")]
    if cleaned_name
        .chars()
        .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        || cleaned_name.ends_with('.')
    {
        return Err(AppError::Git(format!(
            "Invalid Windows file or folder name: '{cleaned_name}'"
        )));
    }

    let (parent, old_name) = match trimmed.rsplit_once('/') {
        Some((p, n)) => (p, n),
        None => ("", trimmed),
    };
    let new_rel = if parent.is_empty() {
        cleaned_name.to_string()
    } else {
        format!("{parent}/{cleaned_name}")
    };

    let src = root.join(trimmed);
    if std::fs::symlink_metadata(&src).is_err() {
        return Err(AppError::Git(format!(
            "Source path does not exist: '{trimmed}'"
        )));
    }

    if new_rel == trimmed {
        return Ok(new_rel);
    }

    let dst = root.join(&new_rel);
    let case_only = old_name.eq_ignore_ascii_case(cleaned_name);
    if !case_only && std::fs::symlink_metadata(&dst).is_ok() {
        return Err(AppError::Git(format!(
            "An entry named '{cleaned_name}' already exists here"
        )));
    }

    std::fs::rename(&src, &dst)?;
    Ok(new_rel)
}

/// Safely creates a new empty file or directory inside `root` under `parent_rel`.
/// `name_or_rel` may be a single name (`"mod.rs"`) or a nested relative path (`"ui/button.rs"`).
/// Rejects path traversal (`..`), `.git` segments, invalid Windows characters, and existing files.
/// Returns the created normalized relative path (using `/` separators).
pub fn create_workspace_entry(
    root: &Path,
    parent_rel: &str,
    name_or_rel: &str,
    is_dir: bool,
) -> AppResult<String> {
    let parent_norm = parent_rel.replace('\\', "/");
    let parent_trimmed = parent_norm.trim_matches('/');
    if !parent_trimmed.is_empty()
        && (!is_safe_rel_path(parent_trimmed)
            || parent_trimmed == ".git"
            || parent_trimmed.starts_with(".git/"))
    {
        return Err(AppError::Git(format!(
            "Invalid parent folder path: '{parent_rel}'"
        )));
    }

    let child_norm = name_or_rel.trim().replace('\\', "/");
    let child_trimmed = child_norm.trim_matches('/');
    if child_trimmed.is_empty() || !is_safe_rel_path(child_trimmed) {
        return Err(AppError::Git(format!(
            "Invalid path: '{name_or_rel}'"
        )));
    }

    for seg in child_trimmed.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." || seg == ".git" || seg.contains('\0') {
            return Err(AppError::Git(format!(
                "Invalid path segment '{seg}' in '{name_or_rel}'"
            )));
        }
        #[cfg(target_os = "windows")]
        if seg
            .chars()
            .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
            || seg.ends_with('.')
            || seg.ends_with(' ')
        {
            return Err(AppError::Git(format!(
                "Invalid Windows path segment: '{seg}'"
            )));
        }
    }

    let full_rel = if parent_trimmed.is_empty() {
        child_trimmed.to_string()
    } else {
        format!("{parent_trimmed}/{child_trimmed}")
    };

    let dst = root.join(&full_rel);
    if std::fs::symlink_metadata(&dst).is_ok() {
        return Err(AppError::Git(format!(
            "Entry '{full_rel}' already exists"
        )));
    }

    if is_dir {
        std::fs::create_dir_all(&dst)?;
    } else {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dst)?;
    }

    Ok(full_rel)
}

/// Safely duplicates a file inside `root` into the same parent directory as `new_name`.
/// Rejects directories, `.git` paths, path separators in `new_name`, and existing targets.
/// Returns the duplicated file's normalized relative path.
pub fn duplicate_workspace_file(root: &Path, src_rel: &str, new_name: &str) -> AppResult<String> {
    let norm = src_rel.replace('\\', "/");
    let trimmed = norm.trim_matches('/');
    if trimmed.is_empty()
        || !is_safe_rel_path(trimmed)
        || trimmed == ".git"
        || trimmed.starts_with(".git/")
    {
        return Err(AppError::Git(format!(
            "Refusing to duplicate unsafe path: '{src_rel}'"
        )));
    }

    let cleaned_name = new_name.trim();
    if cleaned_name.is_empty()
        || cleaned_name == "."
        || cleaned_name == ".."
        || cleaned_name == ".git"
        || cleaned_name.contains('/')
        || cleaned_name.contains('\\')
        || cleaned_name.contains('\0')
    {
        return Err(AppError::Git(format!(
            "Invalid target file name: '{new_name}'"
        )));
    }

    #[cfg(target_os = "windows")]
    if cleaned_name
        .chars()
        .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        || cleaned_name.ends_with('.')
    {
        return Err(AppError::Git(format!(
            "Invalid Windows file name: '{cleaned_name}'"
        )));
    }

    let src = root.join(trimmed);
    let meta = std::fs::metadata(&src).map_err(|_| {
        AppError::Git(format!("Source file does not exist: '{trimmed}'"))
    })?;
    if !meta.is_file() {
        return Err(AppError::Git(format!(
            "Only regular files can be duplicated: '{trimmed}'"
        )));
    }

    let parent = trimmed.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
    let dst_rel = if parent.is_empty() {
        cleaned_name.to_string()
    } else {
        format!("{parent}/{cleaned_name}")
    };
    let dst = root.join(&dst_rel);
    if std::fs::symlink_metadata(&dst).is_ok() {
        return Err(AppError::Git(format!(
            "An entry named '{cleaned_name}' already exists here"
        )));
    }

    std::fs::copy(&src, &dst)?;
    Ok(dst_rel)
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

    #[tokio::test]
    async fn discards_renames_utf8_double_dots_unborn_and_conflicts_cleanly() {
        let dir = std::env::temp_dir().join(format!(
            "gitfleet_discard_edge_cases_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("hardware/PCB")).unwrap();

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

        // 1. Unborn repo: discard staged file to HEAD (no HEAD exists yet)
        std::fs::write(dir.join("hardware/PCB/unborn.csv"), "u1\n").unwrap();
        svc.stage(&dir, &["hardware/PCB/unborn.csv".into()])
            .await
            .unwrap();
        svc.discard_changes(
            &dir,
            &[("hardware/PCB/unborn.csv".into(), false, false)],
            DiscardTarget::Head,
        )
        .await
        .unwrap();
        assert!(!dir.join("hardware/PCB/unborn.csv").exists());
        assert!(svc.get_status(&dir).await.unwrap().is_clean);

        // 2. UTF-8 & double-dot filenames + staged rename
        std::fs::create_dir_all(dir.join("hardware/PCB")).unwrap();
        std::fs::write(dir.join("hardware/PCB/tên tệp.csv"), "a,b\n1,2\n").unwrap();
        std::fs::write(dir.join("hardware/PCB/pcb..v2.csv"), "x,y\n").unwrap();
        std::fs::write(dir.join("hardware/PCB/old-bom.csv"), "orig\n").unwrap();
        svc.stage_all(&dir).await.unwrap();
        svc.commit(&dir, "init", false).await.unwrap();

        // Modify UTF-8 and double-dot files -> get_status must return unquoted exact paths & discard cleanly
        std::fs::write(dir.join("hardware/PCB/tên tệp.csv"), "a,b\n1,9\n").unwrap();
        std::fs::write(dir.join("hardware/PCB/pcb..v2.csv"), "x,z\n").unwrap();
        let st_utf8 = svc.get_status(&dir).await.unwrap();
        assert!(st_utf8
            .unstaged_files
            .iter()
            .any(|f| f.path == "hardware/PCB/tên tệp.csv"));
        assert!(st_utf8
            .unstaged_files
            .iter()
            .any(|f| f.path == "hardware/PCB/pcb..v2.csv"));
        svc.discard_changes(
            &dir,
            &[
                ("hardware/PCB/tên tệp.csv".into(), false, false),
                ("hardware/PCB/pcb..v2.csv".into(), false, false),
            ],
            DiscardTarget::Index,
        )
        .await
        .unwrap();
        assert!(svc.get_status(&dir).await.unwrap().is_clean);

        // 3. Staged Rename (`git mv`) + further edit:
        //    - Discard to Index keeps rename staged and restores renamed file to staged content
        //    - Discard to HEAD restores original path and removes renamed path cleanly
        runner::run_git_ok(
            &dir,
            &[
                "mv",
                "hardware/PCB/old-bom.csv",
                "hardware/PCB/redreen-fren-bom-plan.csv",
            ],
            None,
            None,
            false,
        )
        .await
        .unwrap();
        std::fs::write(dir.join("hardware/PCB/redreen-fren-bom-plan.csv"), "edited\n").unwrap();
        svc.discard_changes(
            &dir,
            &[("hardware/PCB/redreen-fren-bom-plan.csv".into(), false, false)],
            DiscardTarget::Index,
        )
        .await
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("hardware/PCB/redreen-fren-bom-plan.csv"))
                .unwrap()
                .replace("\r\n", "\n"),
            "orig\n"
        );
        svc.discard_changes(
            &dir,
            &[("hardware/PCB/redreen-fren-bom-plan.csv".into(), false, false)],
            DiscardTarget::Head,
        )
        .await
        .unwrap();
        assert!(dir.join("hardware/PCB/old-bom.csv").exists());
        assert!(!dir.join("hardware/PCB/redreen-fren-bom-plan.csv").exists());
        assert!(svc.get_status(&dir).await.unwrap().is_clean);

        // 4. Staged Deleted (`D `) discarded to Index must not error
        runner::run_git_ok(&dir, &["rm", "hardware/PCB/old-bom.csv"], None, None, false)
            .await
            .unwrap();
        svc.discard_changes(
            &dir,
            &[("hardware/PCB/old-bom.csv".into(), false, false)],
            DiscardTarget::Index,
        )
        .await
        .unwrap();
        assert!(!dir.join("hardware/PCB/old-bom.csv").exists());
        svc.discard_changes(
            &dir,
            &[("hardware/PCB/old-bom.csv".into(), false, false)],
            DiscardTarget::Head,
        )
        .await
        .unwrap();
        assert!(dir.join("hardware/PCB/old-bom.csv").exists());

        // 5. Unmerged DU conflict (deleted by us, modified by them) discarded to HEAD cleanly
        runner::run_git_ok(&dir, &["checkout", "-b", "other"], None, None, false)
            .await
            .unwrap();
        std::fs::write(dir.join("hardware/PCB/old-bom.csv"), "them\n").unwrap();
        svc.stage_all(&dir).await.unwrap();
        svc.commit(&dir, "them", false).await.unwrap();
        runner::run_git_ok(&dir, &["checkout", "main"], None, None, false)
            .await
            .unwrap();
        runner::run_git_ok(&dir, &["rm", "hardware/PCB/old-bom.csv"], None, None, false)
            .await
            .unwrap();
        svc.commit(&dir, "del us", false).await.unwrap();
        let _ = runner::run_git(&dir, &["merge", "other"], None, None, false).await;
        let st_du = svc.get_status(&dir).await.unwrap();
        assert_eq!(st_du.conflict_count, 1);
        svc.discard_changes(
            &dir,
            &[("hardware/PCB/old-bom.csv".into(), false, false)],
            DiscardTarget::Head,
        )
        .await
        .unwrap();
        assert!(!dir.join("hardware/PCB/old-bom.csv").exists());
        assert!(svc.get_status(&dir).await.unwrap().is_clean);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_porcelain_v2_ignored_entries_and_skips_rename_orig_paths() {
        let raw = concat!(
            "2 R. N... 100644 100644 100644 aaa bbb R100 new.txt\0",
            "! fake_orig_starting_with_bang.txt\0",
            "? untracked.rs\0",
            "! .env\0",
            "! apps/web/.env.local\0",
            "! .vscode/\0",
            "! target/\0",
            "! .git/\0",
        );
        let entries = parse_ignored_entries_v2_z(raw);
        assert_eq!(entries.files, vec![".env", "apps/web/.env.local"]);
        assert_eq!(entries.dirs, vec![".vscode", "target"]);
    }

    #[tokio::test]
    async fn lists_gitignored_files_collapsed_dirs_and_lazy_dir_children() {
        let dir = std::env::temp_dir().join(format!(
            "gitfleet_ignored_entries_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("apps/web")).unwrap();
        std::fs::create_dir_all(dir.join("local_only/nested")).unwrap();
        std::fs::create_dir_all(dir.join(".vscode")).unwrap();
        std::fs::create_dir_all(dir.join("target/debug/deps")).unwrap();

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

        std::fs::write(
            dir.join(".gitignore"),
            ".env\n.env.*\n!.env.example\n.vscode/\n/target/\n*.log\n",
        )
        .unwrap();
        std::fs::write(dir.join("apps/web/index.ts"), "export {};\n").unwrap();
        std::fs::write(dir.join(".env.example"), "API_KEY=\n").unwrap();
        svc.stage_all(&dir).await.unwrap();
        svc.commit(&dir, "init", false).await.unwrap();

        std::fs::write(dir.join(".env"), "API_KEY=secret\n").unwrap();
        std::fs::write(dir.join("apps/web/.env.local"), "PORT=3000\n").unwrap();
        std::fs::write(dir.join("local_only/nested/app.log"), "ok\n").unwrap();
        std::fs::write(dir.join(".vscode/settings.json"), "{}\n").unwrap();
        std::fs::write(dir.join("target/CACHEDIR.TAG"), "sig\n").unwrap();
        std::fs::write(dir.join("target/debug/deps/huge.o"), "bin\n").unwrap();

        let ignored = svc.list_ignored_entries(&dir).await.unwrap();
        assert_eq!(
            ignored.files,
            vec![".env", "apps/web/.env.local", "local_only/nested/app.log"]
        );
        assert_eq!(ignored.dirs, vec![".vscode", "target"]);

        // Subdirectory inside a git repo should return paths relative to that subdirectory
        let sub_ignored = svc.list_ignored_entries(&dir.join("apps/web")).await.unwrap();
        assert_eq!(sub_ignored.files, vec![".env.local"]);
        assert!(sub_ignored.dirs.is_empty());

        // Lazy single-level inspection of ignored directories
        let vscode = list_ignored_dir_children(&dir, ".vscode", 500).unwrap();
        assert_eq!(vscode.files, vec![".vscode/settings.json"]);
        assert!(vscode.dirs.is_empty());
        assert!(!vscode.truncated);

        let target_lvl1 = list_ignored_dir_children(&dir, "target", 500).unwrap();
        assert_eq!(target_lvl1.files, vec!["target/CACHEDIR.TAG"]);
        assert_eq!(target_lvl1.dirs, vec!["target/debug"]);
        assert!(!target_lvl1.truncated);

        // Cap truncation & path traversal protection
        let capped = list_ignored_dir_children(&dir, "target", 1).unwrap();
        assert_eq!(capped.files.len() + capped.dirs.len(), 1);
        assert!(capped.truncated);
        assert!(list_ignored_dir_children(&dir, "../outside", 10).is_err());
        assert!(list_ignored_dir_children(&dir, ".git", 10).is_err());

        // Safe file and folder deletion + protection of .git / traversal / root
        assert!(delete_workspace_entry(&dir, ".env").unwrap());
        assert!(!dir.join(".env").exists());
        assert!(!delete_workspace_entry(&dir, ".env").unwrap()); // already absent
        assert!(delete_workspace_entry(&dir, "local_only").unwrap());
        assert!(!dir.join("local_only").exists());
        assert!(delete_workspace_entry(&dir, "").is_err());
        assert!(delete_workspace_entry(&dir, ".git").is_err());
        assert!(delete_workspace_entry(&dir, ".git/HEAD").is_err());
        assert!(delete_workspace_entry(&dir, "../outside").is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn renames_files_and_folders_safely() {
        let dir = std::env::temp_dir().join(format!(
            "gitfleet_rename_entry_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/nested")).unwrap();
        std::fs::write(dir.join("src/nested/Alpha.rs"), "pub fn a() {}\n").unwrap();
        std::fs::write(dir.join("src/nested/existing.rs"), "pub fn b() {}\n").unwrap();

        // File rename inside nested dir
        let renamed_file =
            rename_workspace_entry(&dir, "src/nested/Alpha.rs", "alpha_new.rs").unwrap();
        assert_eq!(renamed_file, "src/nested/alpha_new.rs");
        assert!(dir.join("src/nested/alpha_new.rs").is_file());
        assert!(!dir.join("src/nested/Alpha.rs").exists());

        // Case-only rename (especially important on Windows NTFS)
        let cased =
            rename_workspace_entry(&dir, "src/nested/alpha_new.rs", "Alpha_New.rs").unwrap();
        assert_eq!(cased, "src/nested/Alpha_New.rs");
        let entries: Vec<String> = std::fs::read_dir(dir.join("src/nested"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(entries.iter().any(|n| n == "Alpha_New.rs"));

        // Folder rename preserves children
        let renamed_dir = rename_workspace_entry(&dir, "src/nested", "sub").unwrap();
        assert_eq!(renamed_dir, "src/sub");
        assert!(dir.join("src/sub/Alpha_New.rs").is_file());

        // Collision & unsafe name/path protections
        assert!(rename_workspace_entry(&dir, "src/sub/Alpha_New.rs", "existing.rs").is_err());
        assert!(rename_workspace_entry(&dir, "src/sub/Alpha_New.rs", "bad/name.rs").is_err());
        assert!(rename_workspace_entry(&dir, "src/sub/Alpha_New.rs", "..").is_err());
        assert!(rename_workspace_entry(&dir, "src/sub/Alpha_New.rs", ".git").is_err());
        assert!(rename_workspace_entry(&dir, ".git", "nope").is_err());
        assert!(rename_workspace_entry(&dir, "../outside", "x").is_err());

        // Safe create & duplicate helpers
        let created_file =
            create_workspace_entry(&dir, "src/sub", "nested/widget.rs", false).unwrap();
        assert_eq!(created_file, "src/sub/nested/widget.rs");
        assert!(dir.join("src/sub/nested/widget.rs").is_file());
        std::fs::write(dir.join("src/sub/nested/widget.rs"), "pub struct W;\n").unwrap();

        let created_dir = create_workspace_entry(&dir, "src/sub", "assets", true).unwrap();
        assert_eq!(created_dir, "src/sub/assets");
        assert!(dir.join("src/sub/assets").is_dir());

        let dup_file =
            duplicate_workspace_file(&dir, "src/sub/nested/widget.rs", "widget_copy.rs").unwrap();
        assert_eq!(dup_file, "src/sub/nested/widget_copy.rs");
        assert_eq!(
            std::fs::read_to_string(dir.join("src/sub/nested/widget_copy.rs")).unwrap(),
            "pub struct W;\n"
        );

        assert!(create_workspace_entry(&dir, "src/sub", "../escape.rs", false).is_err());
        assert!(create_workspace_entry(&dir, "", ".git/hooks/x", false).is_err());
        assert!(duplicate_workspace_file(&dir, "src/sub/nested/widget.rs", "widget.rs").is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
