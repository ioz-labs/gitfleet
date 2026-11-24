use crate::models::{
    BlameLine, CommitSummary, FileChange, LogOptions, ReflogEntry, ResetMode, SignatureStatus,
    StashEntry,
};
use crate::status::char_to_kind;
use crate::{GitService, runner};
use chrono::{DateTime, Local, TimeZone, Utc};
use gitfleet_core::AppResult;
use std::path::Path;

impl GitService {
    pub async fn get_log(
        &self,
        repo_path: &Path,
        options: LogOptions,
    ) -> AppResult<Vec<CommitSummary>> {
        let limit = if options.limit == 0 { 80 } else { options.limit };
        let limit_str = limit.to_string();
        let order_flag = if options.topo_order {
            "--topo-order"
        } else {
            "--date-order"
        };
        let file_filter = options
            .file_filter_path
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty());
        let mut args = vec!["log"];
        if file_filter.is_some() {
            args.push("--follow");
        }
        args.extend_from_slice(&[
            order_flag,
            "-n",
            &limit_str,
            "--format=%H%x1f%h%x1f%s%x1f%an%x1f%ae%x1f%ar%x1f%ct%x1f%D%x1f%P%x1f%b%x1f%G?%x1e",
        ]);
        let mut upstream_ref: Option<String> = None;
        if file_filter.is_some() {
            // Git's rename follower (`--follow`) requires a single revision (`pending.nr == 1`)
            args.push("HEAD");
        } else if !options.selected_refs.is_empty() {
            for r in &options.selected_refs {
                args.push(r.as_str());
            }
        } else if options.all_refs {
            args.push("--all");
        } else {
            let has_head = runner::run_git_ok(
                repo_path,
                &["rev-parse", "--verify", "--quiet", "HEAD"],
                None,
                None,
                false,
            )
            .await
            .is_ok();

            // Current mode: include both HEAD and its tracked upstream (e.g.
            // origin/develop) so incoming remote commits ahead of HEAD are visible in the graph.
            if let Ok(up) = runner::run_git_ok(
                repo_path,
                &["rev-parse", "--verify", "--quiet", "--abbrev-ref", "@{upstream}"],
                None,
                None,
                false,
            )
            .await
            {
                let trimmed = up.trim();
                if !trimmed.is_empty() {
                    upstream_ref = Some(trimmed.to_string());
                }
            } else if let Ok(branch) = runner::run_git_ok(
                repo_path,
                &["symbolic-ref", "--quiet", "--short", "HEAD"],
                None,
                None,
                false,
            )
            .await
            {
                let branch = branch.trim();
                if !branch.is_empty() {
                    let candidate_full = format!("refs/remotes/origin/{branch}");
                    if runner::run_git_ok(
                        repo_path,
                        &["rev-parse", "--verify", "--quiet", &candidate_full],
                        None,
                        None,
                        false,
                    )
                    .await
                    .is_ok()
                    {
                        upstream_ref = Some(format!("origin/{branch}"));
                    }
                }
            }
            if has_head {
                if let Some(ref up) = upstream_ref {
                    args.push("HEAD");
                    args.push(up.as_str());
                }
            } else if let Some(ref up) = upstream_ref {
                // Unborn local branch, but remote tracking branch has fetched commits
                args.push(up.as_str());
            } else {
                // Fresh unborn repository with zero commits yet
                return Ok(Vec::new());
            }
        }
        if let Some(fpath) = file_filter {
            args.push("--");
            args.push(fpath);
        }
        let out = match runner::run_git_ok(
            repo_path,
            &args,
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await
        {
            Ok(o) => o,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("does not have any commits yet")
                    || msg.contains("bad default revision 'HEAD'")
                {
                    return Ok(Vec::new());
                }
                return Err(e);
            }
        };

        let now_local = Local::now();
        let mut commits = Vec::new();
        for record in out.split('\x1e') {
            let trimmed = record.trim_matches(['\r', '\n']);
            if trimmed.is_empty() {
                continue;
            }
            let parts: Vec<&str> = trimmed.split('\x1f').collect();
            if parts.len() >= 9 {
                let unix_secs: i64 = parts[6].trim().parse().unwrap_or(0);
                let utc_dt = DateTime::<Utc>::from_timestamp(unix_secs, 0).unwrap_or_else(Utc::now);
                let local_dt = Local.from_utc_datetime(&utc_dt.naive_utc());
                let date_display = format_commit_date(local_dt, now_local);
                let body = parts.get(9).map(|b| b.trim().to_string()).unwrap_or_default();
                let signature = match parts.get(10).and_then(|s| s.trim().chars().next()) {
                    Some('G' | 'U') => SignatureStatus::Verified,
                    Some('B' | 'E' | 'X' | 'Y' | 'R') => SignatureStatus::Unverified,
                    _ => SignatureStatus::None,
                };

                commits.push(CommitSummary {
                    oid: parts[0].trim().to_string(),
                    short_oid: parts[1].trim().to_string(),
                    summary: parts[2].to_string(),
                    body,
                    author: parts[3].to_string(),
                    author_email: parts[4].to_string(),
                    date_relative: parts[5].to_string(),
                    date_display,
                    refs: parts[7].to_string(),
                    timestamp: utc_dt,
                    parents: parts[8]
                        .split_whitespace()
                        .map(|s| s.to_string())
                        .collect(),
                    signature,
                });
            }
        }
        Ok(commits)
    }

    pub async fn show_commit(&self, repo_path: &Path, oid: &str) -> AppResult<String> {
        runner::run_git_ok(
            repo_path,
            &["show", "--stat", "--patch", oid],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await
    }

    pub async fn get_commit_files(&self, repo_path: &Path, oid: &str) -> AppResult<Vec<FileChange>> {
        let out = runner::run_git_ok(
            repo_path,
            &["diff-tree", "--no-commit-id", "--name-status", "-r", "--root", oid],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;
        let mut files = Vec::new();
        for line in out.lines() {
            let mut parts = line.split('\t');
            if let (Some(status), Some(path)) = (parts.next(), parts.next()) {
                let c = status.chars().next().unwrap_or('M');
                let actual_path = parts.next().unwrap_or(path);
                files.push(FileChange {
                    path: actual_path.to_string(),
                    kind: char_to_kind(c),
                    staged: false,
                });
            }
        }
        Ok(files)
    }

    pub async fn get_commit_file_diff(
        &self,
        repo_path: &Path,
        oid: &str,
        file_path: &str,
    ) -> AppResult<String> {
        let out = runner::run_git_ok(
            repo_path,
            &["show", "--format=", "--patch", "-U999999", oid, "--", file_path],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;
        if out.len() > 4 * 1024 * 1024 {
            runner::run_git_ok(
                repo_path,
                &["show", "--format=", "--patch", oid, "--", file_path],
                Some(&self.command_log),
                Some(&self.events),
                false,
            )
            .await
        } else {
            Ok(out)
        }
    }

    pub async fn stash_push(&self, repo_path: &Path, message: Option<&str>) -> AppResult<String> {
        self.stash_push_with_options(repo_path, message, true, false)
            .await
    }

    pub async fn stash_push_with_options(
        &self,
        repo_path: &Path,
        message: Option<&str>,
        include_untracked: bool,
        keep_index: bool,
    ) -> AppResult<String> {
        let mut args: Vec<&str> = vec!["stash", "push"];
        if include_untracked {
            args.push("-u");
        }
        if keep_index {
            args.push("--keep-index");
        }
        let trimmed = message.map(str::trim).filter(|m| !m.is_empty());
        if let Some(m) = trimmed {
            args.push("-m");
            args.push(m);
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

    pub async fn stash_push_paths(
        &self,
        repo_path: &Path,
        message: Option<&str>,
        paths: &[String],
    ) -> AppResult<String> {
        if paths.is_empty() {
            return Ok(String::new());
        }
        let mut args: Vec<&str> = vec!["stash", "push", "-u"];
        if let Some(m) = message
            && !m.trim().is_empty()
        {
            args.push("-m");
            args.push(m);
        }
        args.push("--");
        for p in paths {
            args.push(p.as_str());
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

    pub async fn get_stash_files(
        &self,
        repo_path: &Path,
        index: usize,
    ) -> AppResult<Vec<FileChange>> {
        let base = format!("stash@{{{index}}}^1");
        let head = format!("stash@{{{index}}}");
        let mut files = self.get_range_files(repo_path, &base, &head).await.unwrap_or_default();
        let mut seen: std::collections::HashSet<String> =
            files.iter().map(|f| f.path.clone()).collect();

        // Untracked files saved with `git stash push -u` live in the 3rd parent (`stash@{n}^3`)
        let untracked_parent = format!("stash@{{{index}}}^3");
        if let Ok(untracked_out) = runner::run_git_ok(
            repo_path,
            &["ls-tree", "-r", "--name-only", &untracked_parent],
            None,
            None,
            false,
        )
        .await
        {
            for line in untracked_out.lines() {
                let p = line.trim();
                if !p.is_empty() && seen.insert(p.to_string()) {
                    files.push(FileChange {
                        path: p.to_string(),
                        kind: crate::models::FileChangeKind::Untracked,
                        staged: false,
                    });
                }
            }
        }
        Ok(files)
    }

    pub async fn get_stash_file_diff(
        &self,
        repo_path: &Path,
        index: usize,
        file_path: &str,
    ) -> AppResult<String> {
        let base = format!("stash@{{{index}}}^1");
        let head = format!("stash@{{{index}}}");
        let tracked_diff = self
            .get_range_file_diff(repo_path, &base, &head, file_path)
            .await
            .unwrap_or_default();
        if !tracked_diff.trim().is_empty()
            && !tracked_diff.starts_with("(No diff output for ")
        {
            return Ok(tracked_diff);
        }

        let git_path = file_path.replace('\\', "/");
        let untracked_spec = format!("stash@{{{index}}}^3:{git_path}");
        if let Ok(content) =
            runner::run_git_ok(repo_path, &["show", &untracked_spec], None, None, false).await
        {
            let lines: Vec<&str> = content.lines().take(10_000).collect();
            let mut preview = format!(
                "diff --git a/{git_path} b/{git_path}\n--- /dev/null\n+++ b/{git_path}\n@@ -0,0 +1,{} @@\n",
                lines.len()
            );
            for l in lines {
                preview.push('+');
                preview.push_str(l);
                preview.push('\n');
            }
            return Ok(preview);
        }
        Ok(format!("(No diff output for {file_path})"))
    }

    pub async fn list_stashes(&self, repo_path: &Path) -> AppResult<Vec<StashEntry>> {
        let out = runner::run_git_ok(
            repo_path,
            &["stash", "list", "--format=%gd%x1f%gs%x1f%ar"],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;
        let mut list = Vec::new();
        for (index, line) in out.lines().enumerate() {
            let parts: Vec<&str> = line.split('\x1f').collect();
            if parts.len() >= 3 {
                list.push(StashEntry {
                    index,
                    ref_name: parts[0].to_string(),
                    message: parts[1].to_string(),
                    date_relative: parts[2].to_string(),
                });
            }
        }
        Ok(list)
    }

    pub async fn stash_show(&self, repo_path: &Path, index: usize) -> AppResult<String> {
        let target = format!("stash@{{{index}}}");
        runner::run_git_ok(
            repo_path,
            &["stash", "show", "-p", &target],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await
    }

    pub async fn stash_apply(&self, repo_path: &Path, index: usize) -> AppResult<String> {
        let target = format!("stash@{{{index}}}");
        runner::run_git_ok(
            repo_path,
            &["stash", "apply", &target],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn stash_pop(&self, repo_path: &Path, index: usize) -> AppResult<String> {
        let target = format!("stash@{{{index}}}");
        runner::run_git_ok(
            repo_path,
            &["stash", "pop", &target],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    /// Applies or pops `stash@{index}` while distinguishing merge conflicts (`conflicted: true`,
    /// where Git safely retains the stash entry on `pop`) from command execution errors.
    pub async fn stash_apply_smart(
        &self,
        repo_path: &Path,
        index: usize,
        pop: bool,
    ) -> AppResult<crate::models::StashApplyOutcome> {
        let target = format!("stash@{{{index}}}");
        let subcmd = if pop { "pop" } else { "apply" };
        let out = runner::run_git(
            repo_path,
            &["stash", subcmd, &target],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await?;
        let combined = match (out.stdout.trim().is_empty(), out.stderr.trim().is_empty()) {
            (false, false) => format!("{}\n{}", out.stdout.trim(), out.stderr.trim()),
            (false, true) => out.stdout.trim().to_string(),
            (true, false) => out.stderr.trim().to_string(),
            (true, true) => String::new(),
        };
        if out.exit_code == 0 {
            return Ok(crate::models::StashApplyOutcome {
                summary: combined,
                conflicted: false,
            });
        }
        let has_conflict = combined.contains("CONFLICT")
            || combined.contains("Merge conflict")
            || self
                .get_status(repo_path)
                .await
                .is_ok_and(|st| st.conflict_count > 0);
        if has_conflict {
            Ok(crate::models::StashApplyOutcome {
                summary: combined,
                conflicted: true,
            })
        } else {
            Err(gitfleet_core::AppError::Git(if combined.is_empty() {
                format!("git stash {subcmd} exited with code {}", out.exit_code)
            } else {
                combined
            }))
        }
    }

    pub async fn stash_drop(&self, repo_path: &Path, index: usize) -> AppResult<String> {
        let target = format!("stash@{{{index}}}");
        runner::run_git_ok(
            repo_path,
            &["stash", "drop", &target],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn head_oid(&self, repo_path: &Path) -> AppResult<String> {
        self.rev_parse(repo_path, "HEAD").await
    }

    pub async fn rev_parse(&self, repo_path: &Path, rev: &str) -> AppResult<String> {
        let out = runner::run_git_ok(
            repo_path,
            &["rev-parse", rev],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;
        Ok(out.trim().to_string())
    }

    pub async fn diff_refs(
        &self,
        repo_path: &Path,
        base_ref: &str,
        target_ref: &str,
    ) -> AppResult<String> {
        runner::run_git_ok(
            repo_path,
            &["diff", "--patch", base_ref, target_ref],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await
    }

    pub async fn get_range_files(
        &self,
        repo_path: &Path,
        base_ref: &str,
        head_ref: &str,
    ) -> AppResult<Vec<FileChange>> {
        let out = runner::run_git_ok(
            repo_path,
            &["diff", "--name-status", "-r", base_ref, head_ref],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;
        let mut files = Vec::new();
        for line in out.lines() {
            let mut parts = line.split('\t');
            if let (Some(status), Some(path)) = (parts.next(), parts.next()) {
                let c = status.chars().next().unwrap_or('M');
                let actual_path = parts.next().unwrap_or(path);
                files.push(FileChange {
                    path: actual_path.to_string(),
                    kind: char_to_kind(c),
                    staged: false,
                });
            }
        }
        Ok(files)
    }

    pub async fn get_range_file_diff(
        &self,
        repo_path: &Path,
        base_ref: &str,
        head_ref: &str,
        file_path: &str,
    ) -> AppResult<String> {
        let out = runner::run_git_ok(
            repo_path,
            &["diff", "-U999999", base_ref, head_ref, "--", file_path],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;
        let out = if out.len() > 4 * 1024 * 1024 {
            runner::run_git_ok(
                repo_path,
                &["diff", base_ref, head_ref, "--", file_path],
                Some(&self.command_log),
                Some(&self.events),
                false,
            )
            .await?
        } else {
            out
        };
        if out.trim().is_empty() {
            Ok(format!("(No diff output for {file_path})"))
        } else {
            Ok(out)
        }
    }

    pub async fn blame_file(
        &self,
        repo_path: &Path,
        rev: Option<&str>,
        file_path: &str,
    ) -> AppResult<Vec<BlameLine>> {
        let mut args: Vec<&str> = vec!["blame", "--line-porcelain", "-M"];
        if let Some(r) = rev.map(str::trim).filter(|s| !s.is_empty()) {
            args.push(r);
        }
        args.push("--");
        args.push(file_path);
        let out = runner::run_git_ok(
            repo_path,
            &args,
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;
        Ok(parse_blame_porcelain(&out))
    }

    pub async fn reset_commit(
        &self,
        repo_path: &Path,
        revision: &str,
        mode: ResetMode,
    ) -> AppResult<String> {
        runner::run_git_ok(
            repo_path,
            &["reset", mode.as_flag(), revision],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn revert_commit(&self, repo_path: &Path, revision: &str) -> AppResult<String> {
        runner::run_git_ok(
            repo_path,
            &["revert", "--no-edit", revision],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn cherry_pick(&self, repo_path: &Path, revision: &str) -> AppResult<String> {
        self.cherry_pick_with_options(repo_path, &[revision.to_string()], false, false)
            .await
    }

    pub async fn cherry_pick_multiple(
        &self,
        repo_path: &Path,
        revisions: &[String],
    ) -> AppResult<String> {
        self.cherry_pick_with_options(repo_path, revisions, false, false)
            .await
    }

    pub async fn cherry_pick_with_options(
        &self,
        repo_path: &Path,
        revisions: &[String],
        no_commit: bool,
        append_source_id: bool,
    ) -> AppResult<String> {
        if revisions.is_empty() {
            return Ok(String::new());
        }
        let mut args = vec!["cherry-pick"];
        if no_commit {
            args.push("--no-commit");
        } else if append_source_id {
            args.push("-x");
        }
        for rev in revisions {
            args.push(rev.as_str());
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

    pub async fn get_reflog(&self, repo_path: &Path, limit: usize) -> AppResult<Vec<ReflogEntry>> {
        let lim = limit.max(20).to_string();
        let out = match runner::run_git_ok(
            repo_path,
            &["reflog", "-n", &lim, "--format=%h%x1f%gd%x1f%gs%x1f%ar"],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await
        {
            Ok(o) => o,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("does not have any commits yet")
                    || msg.contains("bad default revision")
                    || msg.contains("unknown revision")
                {
                    return Ok(Vec::new());
                }
                return Err(e);
            }
        };
        let mut entries = Vec::new();
        for line in out.lines() {
            let parts: Vec<&str> = line.split('\x1f').collect();
            if parts.len() >= 4 {
                let gs = parts[2];
                let (action, msg) = gs
                    .split_once(": ")
                    .map(|(a, m)| (a.to_string(), m.to_string()))
                    .unwrap_or_else(|| (gs.to_string(), String::new()));
                entries.push(ReflogEntry {
                    short_oid: parts[0].to_string(),
                    selector: parts[1].to_string(),
                    action,
                    message: msg,
                    date_relative: parts[3].to_string(),
                });
            }
        }
        Ok(entries)
    }
}

fn format_commit_date(dt: DateTime<Local>, now: DateTime<Local>) -> String {
    let days_diff = (now.date_naive() - dt.date_naive()).num_days();
    if days_diff == 0 {
        dt.format("Today %I:%M %p").to_string()
    } else if days_diff == 1 {
        dt.format("Yesterday %I:%M %p").to_string()
    } else if (2..=6).contains(&days_diff) {
        dt.format("%A %I:%M %p").to_string()
    } else {
        dt.format("%m/%d/%Y %I:%M %p").to_string()
    }
}

pub fn parse_blame_porcelain(raw: &str) -> Vec<BlameLine> {
    let mut result = Vec::new();
    let mut cur_oid = String::new();
    let mut cur_final_line = 0usize;
    let mut cur_author = String::new();
    let mut cur_time: i64 = 0;
    let mut cur_summary = String::new();
    let mut in_entry = false;

    for line in raw.lines() {
        if let Some(code) = line.strip_prefix('\t') {
            if in_entry {
                let uncommitted = cur_oid.chars().all(|c| c == '0');
                let short_oid = if uncommitted {
                    "Working".to_string()
                } else {
                    cur_oid[..cur_oid.len().min(7)].to_string()
                };
                let date_short = DateTime::<Utc>::from_timestamp(cur_time, 0)
                    .map(|dt| dt.format("%Y-%m-%d").to_string())
                    .unwrap_or_default();
                let summary = if uncommitted && cur_summary.is_empty() {
                    "Uncommitted changes".to_string()
                } else {
                    cur_summary.clone()
                };
                result.push(BlameLine {
                    final_line: cur_final_line,
                    oid: cur_oid.clone(),
                    short_oid,
                    author: cur_author.clone(),
                    date_short,
                    summary,
                    content: code.trim_end_matches('\r').to_string(),
                });
                in_entry = false;
            }
            continue;
        }

        if !in_entry {
            let mut parts = line.split_whitespace();
            if let (Some(oid), Some(_orig), Some(fin)) = (parts.next(), parts.next(), parts.next())
                && oid.len() >= 40
                && oid.chars().all(|c| c.is_ascii_hexdigit())
                && let Ok(fin_ln) = fin.parse::<usize>()
            {
                cur_oid = oid.to_string();
                cur_final_line = fin_ln;
                cur_author.clear();
                cur_time = 0;
                cur_summary.clear();
                in_entry = true;
            }
        } else if let Some(a) = line.strip_prefix("author ") {
            cur_author = a.trim().to_string();
        } else if let Some(t) = line.strip_prefix("author-time ") {
            cur_time = t.trim().parse().unwrap_or(0);
        } else if let Some(s) = line.strip_prefix("summary ") {
            cur_summary = s.trim().to_string();
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitfleet_core::{CommandLogStore, EventBus};
    use std::sync::Arc;

    #[tokio::test]
    async fn filters_graph_log_to_selected_branches() {
        let dir = std::env::temp_dir().join(format!(
            "gitfleet_multibranch_log_{}",
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

        std::fs::write(dir.join("base.txt"), "base").unwrap();
        svc.stage(&dir, &["base.txt".into()]).await.unwrap();
        svc.commit(&dir, "base commit", false).await.unwrap();

        svc.create_branch(&dir, "feat-a").await.unwrap();
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        svc.stage(&dir, &["a.txt".into()]).await.unwrap();
        svc.commit(&dir, "commit on feat-a", false).await.unwrap();

        svc.checkout(&dir, "main").await.unwrap();
        svc.create_branch(&dir, "feat-b").await.unwrap();
        std::fs::write(dir.join("b.txt"), "b").unwrap();
        svc.stage(&dir, &["b.txt".into()]).await.unwrap();
        svc.commit(&dir, "commit on feat-b", false).await.unwrap();

        svc.checkout(&dir, "main").await.unwrap();
        svc.create_branch(&dir, "hidden-c").await.unwrap();
        std::fs::write(dir.join("c.txt"), "c").unwrap();
        svc.stage(&dir, &["c.txt".into()]).await.unwrap();
        svc.commit(&dir, "commit on hidden-c", false).await.unwrap();

        svc.checkout(&dir, "main").await.unwrap();

        let filtered = svc
            .get_log(
                &dir,
                LogOptions {
                    limit: 50,
                    all_refs: true,
                    selected_refs: vec!["feat-a".into(), "feat-b".into()],
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let summaries: Vec<&str> = filtered.iter().map(|c| c.summary.as_str()).collect();
        assert!(summaries.contains(&"commit on feat-a"));
        assert!(summaries.contains(&"commit on feat-b"));
        assert!(summaries.contains(&"base commit"));
        assert!(!summaries.contains(&"commit on hidden-c"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn current_mode_log_includes_upstream_origin_commits() {
        let base = std::env::temp_dir().join(format!(
            "gitfleet_upstream_log_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let remote_dir = base.join("remote.git");
        let work_dir = base.join("work");
        std::fs::create_dir_all(&remote_dir).unwrap();
        std::fs::create_dir_all(&work_dir).unwrap();

        let svc = GitService::new(EventBus::new(16), Arc::new(CommandLogStore::new()));
        runner::run_git_ok(&remote_dir, &["init", "--bare", "-b", "develop"], None, None, false)
            .await
            .unwrap();
        runner::run_git_ok(&work_dir, &["init", "-b", "develop"], None, None, false)
            .await
            .unwrap();
        runner::run_git_ok(&work_dir, &["config", "user.name", "Tester"], None, None, false)
            .await
            .unwrap();
        runner::run_git_ok(&work_dir, &["config", "user.email", "t@local"], None, None, false)
            .await
            .unwrap();
        let remote_str = remote_dir.to_string_lossy().to_string();
        runner::run_git_ok(&work_dir, &["remote", "add", "origin", &remote_str], None, None, false)
            .await
            .unwrap();

        std::fs::write(work_dir.join("a.txt"), "1").unwrap();
        svc.stage(&work_dir, &["a.txt".into()]).await.unwrap();
        svc.commit(&work_dir, "local head commit", false).await.unwrap();

        std::fs::write(work_dir.join("b.txt"), "2").unwrap();
        svc.stage(&work_dir, &["b.txt".into()]).await.unwrap();
        svc.commit(&work_dir, "incoming origin commit 1", false).await.unwrap();

        std::fs::write(work_dir.join("c.txt"), "3").unwrap();
        svc.stage(&work_dir, &["c.txt".into()]).await.unwrap();
        svc.commit(&work_dir, "incoming origin commit 2", false).await.unwrap();

        runner::run_git_ok(&work_dir, &["push", "-u", "origin", "develop"], None, None, false)
            .await
            .unwrap();
        // Move local develop 2 commits behind origin/develop
        runner::run_git_ok(&work_dir, &["reset", "--hard", "HEAD~2"], None, None, false)
            .await
            .unwrap();

        let commits = svc
            .get_log(
                &work_dir,
                LogOptions {
                    limit: 50,
                    all_refs: false,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let summaries: Vec<&str> = commits.iter().map(|c| c.summary.as_str()).collect();
        assert_eq!(summaries.len(), 3);
        assert_eq!(summaries[0], "incoming origin commit 2");
        assert!(commits[0].refs.contains("origin/develop"));
        assert_eq!(summaries[1], "incoming origin commit 1");
        assert_eq!(summaries[2], "local head commit");
        assert!(commits[2].refs.contains("HEAD -> develop"));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn unborn_repo_log_and_reflog_return_empty_instead_of_error() {
        let dir = std::env::temp_dir().join(format!(
            "gitfleet_unborn_log_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let svc = GitService::new(EventBus::new(16), Arc::new(CommandLogStore::new()));
        runner::run_git_ok(&dir, &["init", "-b", "main"], None, None, false)
            .await
            .unwrap();
        std::fs::write(dir.join("README.md"), "# fresh repo").unwrap();

        let cur = svc
            .get_log(&dir, LogOptions { limit: 50, all_refs: false, ..Default::default() })
            .await
            .unwrap();
        assert!(cur.is_empty());

        let all = svc
            .get_log(&dir, LogOptions { limit: 50, all_refs: true, ..Default::default() })
            .await
            .unwrap();
        assert!(all.is_empty());

        let reflog = svc.get_reflog(&dir, 50).await.unwrap();
        assert!(reflog.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn range_compare_and_stash_inspector_list_and_diff_files() {
        let dir = std::env::temp_dir().join(format!(
            "gitfleet_range_stash_{}",
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

        std::fs::write(dir.join("alpha.txt"), "v1\n").unwrap();
        svc.stage(&dir, &["alpha.txt".into()]).await.unwrap();
        svc.commit(&dir, "c1", false).await.unwrap();
        let c1 = svc.head_oid(&dir).await.unwrap();

        std::fs::write(dir.join("alpha.txt"), "v2\n").unwrap();
        std::fs::write(dir.join("beta.txt"), "added\n").unwrap();
        svc.stage(&dir, &["alpha.txt".into(), "beta.txt".into()])
            .await
            .unwrap();
        svc.commit(&dir, "c2", false).await.unwrap();
        let c2 = svc.head_oid(&dir).await.unwrap();

        let range_files = svc.get_range_files(&dir, &c1, &c2).await.unwrap();
        assert_eq!(range_files.len(), 2);
        let alpha_diff = svc
            .get_range_file_diff(&dir, &c1, &c2, "alpha.txt")
            .await
            .unwrap();
        assert!(alpha_diff.contains("-v1"));
        assert!(alpha_diff.contains("+v2"));

        // Partial stash of one tracked + one untracked file while leaving beta.txt dirty
        std::fs::write(dir.join("alpha.txt"), "stashed_v3\n").unwrap();
        std::fs::write(dir.join("beta.txt"), "keep_in_wt\n").unwrap();
        std::fs::write(dir.join("untracked_note.md"), "hello stash\n").unwrap();
        svc.stash_push_paths(
            &dir,
            Some("selected-stash"),
            &["alpha.txt".into(), "untracked_note.md".into()],
        )
        .await
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(dir.join("beta.txt")).unwrap(),
            "keep_in_wt\n"
        );
        let stash_files = svc.get_stash_files(&dir, 0).await.unwrap();
        let stash_paths: Vec<&str> = stash_files.iter().map(|f| f.path.as_str()).collect();
        assert!(stash_paths.contains(&"alpha.txt"));
        assert!(stash_paths.contains(&"untracked_note.md"));
        assert!(!stash_paths.contains(&"beta.txt"));

        let untracked_diff = svc
            .get_stash_file_diff(&dir, 0, "untracked_note.md")
            .await
            .unwrap();
        assert!(untracked_diff.contains("+hello stash"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn blame_file_and_follow_history_across_renames_work() {
        let dir = std::env::temp_dir().join(format!(
            "gitfleet_blame_follow_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let svc = GitService::new(EventBus::new(16), Arc::new(CommandLogStore::new()));
        runner::run_git_ok(&dir, &["init", "-b", "main"], None, None, false)
            .await
            .unwrap();
        runner::run_git_ok(&dir, &["config", "user.name", "Alice"], None, None, false)
            .await
            .unwrap();
        runner::run_git_ok(&dir, &["config", "user.email", "a@local"], None, None, false)
            .await
            .unwrap();

        std::fs::write(
            dir.join("legacy.rs"),
            "fn alpha() {}\nfn keep_one() {}\nfn keep_two() {}\nfn beta() {}\n",
        )
        .unwrap();
        svc.stage(&dir, &["legacy.rs".into()]).await.unwrap();
        svc.commit(&dir, "create legacy", false).await.unwrap();
        let c1 = svc.head_oid(&dir).await.unwrap();

        runner::run_git_ok(&dir, &["mv", "legacy.rs", "modern.rs"], None, None, false)
            .await
            .unwrap();
        std::fs::write(
            dir.join("modern.rs"),
            "fn alpha() {}\nfn keep_one() {}\nfn keep_two() {}\nfn beta_v2() {}\nfn gamma() {}\n",
        )
        .unwrap();
        svc.stage(&dir, &["modern.rs".into()]).await.unwrap();
        svc.commit(&dir, "rename and extend", false).await.unwrap();
        let c2 = svc.head_oid(&dir).await.unwrap();

        let blame = svc.blame_file(&dir, None, "modern.rs").await.unwrap();
        assert_eq!(blame.len(), 5);
        assert_eq!(blame[0].oid, c1);
        assert_eq!(blame[0].content, "fn alpha() {}");
        assert_eq!(blame[3].oid, c2);
        assert_eq!(blame[3].summary, "rename and extend");
        assert_eq!(blame[4].final_line, 5);

        let followed = svc
            .get_log(
                &dir,
                LogOptions {
                    limit: 50,
                    file_filter_path: Some("modern.rs".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let summaries: Vec<&str> = followed.iter().map(|c| c.summary.as_str()).collect();
        assert_eq!(summaries, vec!["rename and extend", "create legacy"]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
