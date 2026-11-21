use crate::models::{AmendTarget, DiscardTarget, FileChange, FileChangeKind, RepoStatusSummary};
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
}
