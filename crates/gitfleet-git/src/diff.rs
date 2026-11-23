use crate::{GitService, runner};
use gitfleet_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffChangeKind {
    Added,
    Removed,
    Modified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffContextLine {
    pub old_line: usize,
    pub new_line: usize,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffChangeBlock {
    pub index: usize,
    pub kind: DiffChangeKind,
    pub old_start: usize,
    pub old_lines: Vec<String>,
    pub new_start: usize,
    pub new_lines: Vec<String>,
    pub header: String,
    pub patch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffBlock {
    Equal(Vec<DiffContextLine>),
    Change(DiffChangeBlock),
    Separator(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffStats {
    pub added_blocks: usize,
    pub removed_blocks: usize,
    pub modified_blocks: usize,
    pub added_lines: usize,
    pub removed_lines: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    pub index: usize,
    pub header: String,
    pub lines: Vec<String>,
    pub patch: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParsedDiff {
    pub file_header: String,
    pub hunks: Vec<DiffHunk>,
    #[serde(default)]
    pub blocks: Vec<DiffBlock>,
    #[serde(default)]
    pub stats: DiffStats,
    #[serde(default)]
    pub is_full_file: bool,
    pub raw: String,
}

#[derive(Debug, Clone)]
pub struct FileVersionPair {
    pub file_path: String,
    pub before_label: String,
    pub after_label: String,
    pub before_bytes: Option<Vec<u8>>,
    pub after_bytes: Option<Vec<u8>>,
}

const MAX_FULL_CONTEXT_BYTES: usize = 4 * 1024 * 1024;
const MAX_MEDIA_DIFF_BYTES: u64 = 24 * 1024 * 1024;
const UNTRACKED_MAX_LINES: usize = 10_000;

fn read_disk_capped(path: &Path) -> AppResult<Option<Vec<u8>>> {
    let Ok(meta) = std::fs::metadata(path) else {
        return Ok(None);
    };
    if !meta.is_file() {
        return Ok(None);
    }
    if meta.len() > MAX_MEDIA_DIFF_BYTES {
        return Err(AppError::Git(format!(
            "File exceeds {} MB diff limit ({:.1} MB)",
            MAX_MEDIA_DIFF_BYTES / (1024 * 1024),
            meta.len() as f64 / (1024.0 * 1024.0)
        )));
    }
    Ok(Some(std::fs::read(path)?))
}

async fn read_git_blob_capped(repo_path: &Path, spec: &str) -> AppResult<Option<Vec<u8>>> {
    // Check blob size first so huge LFS/binary blobs never blow up memory
    if let Ok(size_str) =
        runner::run_git_ok(repo_path, &["cat-file", "-s", spec], None, None, false).await
        && let Ok(size) = size_str.trim().parse::<u64>()
        && size > MAX_MEDIA_DIFF_BYTES
    {
        return Err(AppError::Git(format!(
            "Git object '{spec}' exceeds {} MB diff limit ({:.1} MB)",
            MAX_MEDIA_DIFF_BYTES / (1024 * 1024),
            size as f64 / (1024.0 * 1024.0)
        )));
    }
    match runner::run_git_bytes(repo_path, &["show", spec]).await {
        Ok(bytes) => Ok(Some(bytes)),
        Err(_) => Ok(None),
    }
}

impl GitService {
    pub async fn get_diff(
        &self,
        repo_path: &Path,
        file_path: &str,
        staged: bool,
        untracked: bool,
    ) -> AppResult<String> {
        if untracked {
            let full = repo_path.join(file_path);
            let content = std::fs::read_to_string(&full)
                .unwrap_or_else(|_| "[Binary or unreadable file]".to_string());
            let all_lines: Vec<&str> = content.lines().take(UNTRACKED_MAX_LINES).collect();
            let mut preview = format!(
                "diff --git a/{file_path} b/{file_path}\n--- /dev/null\n+++ b/{file_path}\n@@ -0,0 +1,{} @@\n",
                all_lines.len()
            );
            for line in all_lines {
                preview.push('+');
                preview.push_str(line);
                preview.push('\n');
            }
            return Ok(preview);
        }

        // Request full-file context (-U999999) so the viewer supports line numbers, expandable
        // hidden regions in Compact mode, and full-file viewing when Compact is toggled off.
        let full_args: Vec<&str> = if staged {
            vec!["diff", "--cached", "-U999999", "--", file_path]
        } else {
            vec!["diff", "-U999999", "--", file_path]
        };

        let out = runner::run_git_ok(
            repo_path,
            &full_args,
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;

        let out = if out.len() > MAX_FULL_CONTEXT_BYTES {
            let fallback_args: Vec<&str> = if staged {
                vec!["diff", "--cached", "--", file_path]
            } else {
                vec!["diff", "--", file_path]
            };
            runner::run_git_ok(
                repo_path,
                &fallback_args,
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

    pub async fn apply_hunk(
        &self,
        repo_path: &Path,
        patch: &str,
        cached: bool,
        reverse: bool,
    ) -> AppResult<()> {
        let mut args = vec!["apply", "--unidiff-zero", "--recount"];
        if cached {
            args.push("--cached");
        }
        if reverse {
            args.push("--reverse");
        }
        args.push("-");

        let out = runner::run_git_with_stdin(
            repo_path,
            &args,
            Some(patch.as_bytes()),
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await?;

        if out.exit_code != 0 {
            return Err(AppError::Git(out.stderr.trim().to_string()));
        }
        Ok(())
    }

    pub async fn staged_diff(&self, repo_path: &Path) -> AppResult<String> {
        runner::run_git_ok(repo_path, &["diff", "--cached"], None, None, false).await
    }

    pub async fn staged_file_paths(&self, repo_path: &Path) -> AppResult<Vec<String>> {
        let out = runner::run_git_ok(
            repo_path,
            &["diff", "--cached", "--name-only", "--diff-filter=AM"],
            None,
            None,
            false,
        )
        .await?;
        Ok(out
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(String::from)
            .collect())
    }

    /// Loads raw `(before, after)` byte payloads for a working-tree or staged file so rich visual
    /// diff viewers (Image Diff & CSV Table Diff) have exact unmodified contents for both sides.
    pub async fn get_working_file_versions(
        &self,
        repo_path: &Path,
        file_path: &str,
        staged: bool,
        untracked: bool,
    ) -> AppResult<FileVersionPair> {
        let git_path = file_path.replace('\\', "/");
        if untracked {
            let after_bytes = read_disk_capped(&repo_path.join(file_path))?;
            return Ok(FileVersionPair {
                file_path: file_path.to_string(),
                before_label: "New file".into(),
                after_label: "Working Tree".into(),
                before_bytes: None,
                after_bytes,
            });
        }

        if staged {
            let head_spec = format!("HEAD:{git_path}");
            let index_spec = format!(":{git_path}");
            let before_bytes = read_git_blob_capped(repo_path, &head_spec).await?;
            let after_bytes = read_git_blob_capped(repo_path, &index_spec).await?;
            return Ok(FileVersionPair {
                file_path: file_path.to_string(),
                before_label: "HEAD".into(),
                after_label: "Index (Staged)".into(),
                before_bytes,
                after_bytes,
            });
        }

        // Unstaged working-tree change: compare Index (or HEAD fallback) vs disk
        let index_spec = format!(":{git_path}");
        let mut before_label = "Index".to_string();
        let mut before_bytes = read_git_blob_capped(repo_path, &index_spec).await?;
        if before_bytes.is_none() {
            let head_spec = format!("HEAD:{git_path}");
            before_bytes = read_git_blob_capped(repo_path, &head_spec).await?;
            if before_bytes.is_some() {
                before_label = "HEAD".into();
            }
        }
        let after_bytes = read_disk_capped(&repo_path.join(file_path))?;
        Ok(FileVersionPair {
            file_path: file_path.to_string(),
            before_label,
            after_label: "Working Tree".into(),
            before_bytes,
            after_bytes,
        })
    }

    /// Loads raw `(before, after)` byte payloads for a file inside a historical commit `oid`
    /// (`<oid>^:<path>` vs `<oid>:<path>`), following renames automatically.
    pub async fn get_commit_file_versions(
        &self,
        repo_path: &Path,
        oid: &str,
        file_path: &str,
    ) -> AppResult<FileVersionPair> {
        let git_path = file_path.replace('\\', "/");
        let short_oid = &oid[..oid.len().min(7)];

        let after_spec = format!("{oid}:{git_path}");
        let after_bytes = read_git_blob_capped(repo_path, &after_spec).await?;

        let parent_spec = format!("{oid}^:{git_path}");
        let mut before_bytes = read_git_blob_capped(repo_path, &parent_spec).await?;
        let mut before_label = format!("{short_oid}^");

        // Follow commit rename if `<oid>^:<file_path>` didn't exist under the new path
        if before_bytes.is_none()
            && let Ok(tree_out) = runner::run_git_ok(
                repo_path,
                &[
                    "diff-tree",
                    "--no-commit-id",
                    "--name-status",
                    "-r",
                    "-M",
                    "--root",
                    oid,
                ],
                None,
                None,
                false,
            )
            .await
        {
            for line in tree_out.lines() {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() >= 3
                    && (parts[0].starts_with('R') || parts[0].starts_with('C'))
                    && parts[2] == git_path
                {
                    let prev_path = parts[1];
                    let prev_spec = format!("{oid}^:{prev_path}");
                    if let Ok(Some(bytes)) = read_git_blob_capped(repo_path, &prev_spec).await {
                        before_bytes = Some(bytes);
                        before_label = format!("{short_oid}^ ({prev_path})");
                    }
                    break;
                }
            }
        }

        Ok(FileVersionPair {
            file_path: file_path.to_string(),
            before_label,
            after_label: short_oid.to_string(),
            before_bytes,
            after_bytes,
        })
    }

    /// Loads raw `(before, after)` byte payloads for a file across `<base>..<head>`.
    pub async fn get_range_file_versions(
        &self,
        repo_path: &Path,
        base: &str,
        head: &str,
        file_path: &str,
    ) -> AppResult<FileVersionPair> {
        let git_path = file_path.replace('\\', "/");
        let base_short = &base[..base.len().min(12)];
        let head_short = &head[..head.len().min(12)];

        let before_spec = format!("{base}:{git_path}");
        let after_spec = format!("{head}:{git_path}");
        let before_bytes = read_git_blob_capped(repo_path, &before_spec).await?;
        let after_bytes = read_git_blob_capped(repo_path, &after_spec).await?;

        Ok(FileVersionPair {
            file_path: file_path.to_string(),
            before_label: base_short.to_string(),
            after_label: head_short.to_string(),
            before_bytes,
            after_bytes,
        })
    }
}

/// Builds a valid unified diff patch containing only the selected changed lines (`sel_old` 1-based
/// old line numbers for `'-'` lines, `sel_new` 1-based new line numbers for `'+'` lines).
///
/// - When `reverse == false` (forward apply, e.g. **Stage Selected Lines** on unstaged diff):
///   - Selected `'-'` stay `'-'`; unselected `'-'` become `' '` context (kept in Index).
///   - Selected `'+'` stay `'+'`; unselected `'+'` are omitted (not added yet).
/// - When `reverse == true` (`git apply --reverse`, e.g. **Unstage Selected Lines** or **Discard Selected Lines**):
///   - Selected `'+'` stay `'+'` (removed by `--reverse`); unselected `'+'` become `' '` context (kept).
///   - Selected `'-'` stay `'-'` (restored by `--reverse`); unselected `'-'` are omitted.
pub fn build_partial_patch(
    parsed: &ParsedDiff,
    sel_old: &std::collections::HashSet<usize>,
    sel_new: &std::collections::HashSet<usize>,
    reverse: bool,
) -> Option<String> {
    if parsed.hunks.is_empty() || (sel_old.is_empty() && sel_new.is_empty()) {
        return None;
    }

    let mut out_hunks: Vec<String> = Vec::new();
    let mut any_old_lines = false;
    let mut any_new_lines = false;

    for hunk in &parsed.hunks {
        let (old_start_raw, _, new_start_raw, _) =
            parse_hunk_range(&hunk.header).unwrap_or((1, 0, 1, 0));
        let mut old_ln = old_start_raw.max(1);
        let mut new_ln = new_start_raw.max(1);

        let mut hunk_out: Vec<String> = Vec::new();
        let mut old_cnt = 0usize;
        let mut new_cnt = 0usize;
        let mut hunk_has_selected = false;

        let mut i = 0usize;
        while i < hunk.lines.len() {
            let line = &hunk.lines[i];
            if line.starts_with('-') || line.starts_with('+') {
                // Collect one contiguous change run of '-' followed by '+' lines
                let mut olds: Vec<(usize, String, Option<String>)> = Vec::new();
                let mut news: Vec<(usize, String, Option<String>)> = Vec::new();

                while i < hunk.lines.len() {
                    let cur = &hunk.lines[i];
                    if let Some(rem) = cur.strip_prefix('-') {
                        if !news.is_empty() {
                            break;
                        }
                        let ln = old_ln;
                        old_ln += 1;
                        i += 1;
                        let mut no_nl = None;
                        if i < hunk.lines.len() && hunk.lines[i].starts_with('\\') {
                            no_nl = Some(hunk.lines[i].clone());
                            i += 1;
                        }
                        olds.push((ln, rem.to_string(), no_nl));
                    } else if let Some(add) = cur.strip_prefix('+') {
                        let ln = new_ln;
                        new_ln += 1;
                        i += 1;
                        let mut no_nl = None;
                        if i < hunk.lines.len() && hunk.lines[i].starts_with('\\') {
                            no_nl = Some(hunk.lines[i].clone());
                            i += 1;
                        }
                        news.push((ln, add.to_string(), no_nl));
                    } else {
                        break;
                    }
                }

                if !reverse {
                    // Forward apply: place selected '+' lines immediately after the last selected
                    // '-' line (or after all unselected '-' context lines if no '-' is selected)
                    // so paired line modifications replace in-place cleanly.
                    let last_sel_old = olds.iter().rposition(|(ln, _, _)| sel_old.contains(ln));
                    let emit_plus_at = last_sel_old.map(|idx| idx + 1).unwrap_or(olds.len());

                    for (idx, (ln, txt, no_nl)) in olds.iter().enumerate() {
                        if sel_old.contains(ln) {
                            hunk_has_selected = true;
                            old_cnt += 1;
                            hunk_out.push(format!("-{txt}"));
                            if let Some(nn) = no_nl {
                                hunk_out.push(nn.clone());
                            }
                        } else {
                            old_cnt += 1;
                            new_cnt += 1;
                            hunk_out.push(format!(" {txt}"));
                            if let Some(nn) = no_nl {
                                hunk_out.push(nn.clone());
                            }
                        }
                        if idx + 1 == emit_plus_at {
                            for (nln, ntxt, n_no_nl) in &news {
                                if sel_new.contains(nln) {
                                    hunk_has_selected = true;
                                    new_cnt += 1;
                                    hunk_out.push(format!("+{ntxt}"));
                                    if let Some(nn) = n_no_nl {
                                        hunk_out.push(nn.clone());
                                    }
                                }
                            }
                        }
                    }
                    if olds.is_empty() {
                        for (nln, ntxt, n_no_nl) in &news {
                            if sel_new.contains(nln) {
                                hunk_has_selected = true;
                                new_cnt += 1;
                                hunk_out.push(format!("+{ntxt}"));
                                if let Some(nn) = n_no_nl {
                                    hunk_out.push(nn.clone());
                                }
                            }
                        }
                    }
                } else {
                    // Reverse apply (`git apply --reverse`): place selected '-' lines immediately
                    // before the first selected '+' line so in-place modifications revert cleanly.
                    let first_sel_new = news.iter().position(|(ln, _, _)| sel_new.contains(ln));
                    let emit_minus_at = first_sel_new.unwrap_or(news.len());

                    for (idx, (nln, ntxt, n_no_nl)) in news.iter().enumerate() {
                        if idx == emit_minus_at {
                            for (oln, otxt, o_no_nl) in &olds {
                                if sel_old.contains(oln) {
                                    hunk_has_selected = true;
                                    old_cnt += 1;
                                    hunk_out.push(format!("-{otxt}"));
                                    if let Some(nn) = o_no_nl {
                                        hunk_out.push(nn.clone());
                                    }
                                }
                            }
                        }
                        if sel_new.contains(nln) {
                            hunk_has_selected = true;
                            new_cnt += 1;
                            hunk_out.push(format!("+{ntxt}"));
                            if let Some(nn) = n_no_nl {
                                hunk_out.push(nn.clone());
                            }
                        } else {
                            old_cnt += 1;
                            new_cnt += 1;
                            hunk_out.push(format!(" {ntxt}"));
                            if let Some(nn) = n_no_nl {
                                hunk_out.push(nn.clone());
                            }
                        }
                    }
                    if emit_minus_at == news.len() {
                        for (oln, otxt, o_no_nl) in &olds {
                            if sel_old.contains(oln) {
                                hunk_has_selected = true;
                                old_cnt += 1;
                                hunk_out.push(format!("-{otxt}"));
                                if let Some(nn) = o_no_nl {
                                    hunk_out.push(nn.clone());
                                }
                            }
                        }
                    }
                }
            } else if line.starts_with('\\') {
                hunk_out.push(line.clone());
                i += 1;
            } else {
                let txt = line.strip_prefix(' ').unwrap_or(line);
                old_ln += 1;
                new_ln += 1;
                old_cnt += 1;
                new_cnt += 1;
                hunk_out.push(format!(" {txt}"));
                i += 1;
            }
        }

        if hunk_has_selected {
            if old_cnt > 0 {
                any_old_lines = true;
            }
            if new_cnt > 0 {
                any_new_lines = true;
            }
            let hdr_old_start = if old_cnt == 0 {
                old_start_raw.saturating_sub(1)
            } else {
                old_start_raw.max(1)
            };
            let hdr_new_start = if new_cnt == 0 {
                new_start_raw.saturating_sub(1)
            } else {
                new_start_raw.max(1)
            };
            let mut block = format!("@@ -{hdr_old_start},{old_cnt} +{hdr_new_start},{new_cnt} @@\n");
            for l in hunk_out {
                block.push_str(&l);
                block.push('\n');
            }
            out_hunks.push(block);
        }
    }

    if out_hunks.is_empty() {
        return None;
    }

    let rel_path = parsed
        .file_header
        .lines()
        .find_map(|l| l.strip_prefix("+++ b/").or_else(|| l.strip_prefix("--- a/")))
        .unwrap_or("file");

    let rewrite_old_dev_null = parsed.file_header.contains("--- /dev/null") && any_old_lines;
    let rewrite_new_dev_null = parsed.file_header.contains("+++ /dev/null") && any_new_lines;

    let mut patch = String::new();
    if rewrite_old_dev_null || rewrite_new_dev_null {
        patch.push_str(&format!(
            "diff --git a/{rel_path} b/{rel_path}\n--- a/{rel_path}\n+++ b/{rel_path}\n"
        ));
    } else if !parsed.file_header.is_empty() {
        for hdr_line in parsed.file_header.lines() {
            if !hdr_line.starts_with("index ") {
                patch.push_str(hdr_line);
                patch.push('\n');
            }
        }
    }

    for h in out_hunks {
        patch.push_str(&h);
    }
    Some(patch)
}

/// Parses a hunk header `@@ -old_start[,old_count] +new_start[,new_count] @@ ...`
/// into `(old_start, old_count, new_start, new_count)`.
pub fn parse_hunk_range(header: &str) -> Option<(usize, usize, usize, usize)> {
    let rest = header.strip_prefix("@@ ")?;
    let end = rest.find(" @@")?;
    let range_part = &rest[..end];
    let mut parts = range_part.split_whitespace();
    let old_part = parts.next()?.strip_prefix('-')?;
    let new_part = parts.next()?.strip_prefix('+')?;

    let parse_pair = |s: &str| -> Option<(usize, usize)> {
        if let Some((a, b)) = s.split_once(',') {
            Some((a.parse().ok()?, b.parse().ok()?))
        } else {
            Some((s.parse().ok()?, 1))
        }
    };
    let (os, oc) = parse_pair(old_part)?;
    let (ns, nc) = parse_pair(new_part)?;
    Some((os, oc, ns, nc))
}

/// Parses a unified diff output into file header, individual apply-ready hunks, and structured
/// `DiffBlock`s (`Equal` / `Change::{Added, Removed, Modified}`) with exact 1-based line numbers.
pub fn parse_unified_diff(raw: &str) -> ParsedDiff {
    let mut header_lines: Vec<&str> = Vec::new();
    let mut raw_hunks: Vec<(String, Vec<String>)> = Vec::new();

    let mut in_hunk = false;
    let mut cur_header = String::new();
    let mut cur_lines: Vec<String> = Vec::new();

    for line in raw.lines() {
        if line.starts_with("@@") {
            if in_hunk {
                raw_hunks.push((cur_header.clone(), std::mem::take(&mut cur_lines)));
            }
            in_hunk = true;
            cur_header = line.to_string();
        } else if in_hunk {
            // Stop hunk body if a multi-file diff begins a new file header
            if line.starts_with("diff --git ") {
                raw_hunks.push((cur_header.clone(), std::mem::take(&mut cur_lines)));
                in_hunk = false;
            } else {
                cur_lines.push(line.to_string());
            }
        } else {
            header_lines.push(line);
        }
    }

    let file_header = header_lines.join("\n");
    if in_hunk {
        raw_hunks.push((cur_header, cur_lines));
    }

    let mut hunks: Vec<DiffHunk> = Vec::new();
    let mut blocks: Vec<DiffBlock> = Vec::new();
    let mut stats = DiffStats::default();
    let mut change_idx = 0usize;

    let single_file_start = raw_hunks.len() == 1
        && raw_hunks
            .first()
            .and_then(|(h, _)| parse_hunk_range(h))
            .is_some_and(|(os, _, ns, _)| os <= 1 && ns <= 1);

    for (h_idx, (h_header, h_lines)) in raw_hunks.iter().enumerate() {
        let patch = build_hunk_patch(&file_header, h_header, h_lines);
        hunks.push(DiffHunk {
            index: h_idx,
            header: h_header.clone(),
            lines: h_lines.clone(),
            patch,
        });

        let (old_start_raw, _, new_start_raw, _) =
            parse_hunk_range(h_header).unwrap_or((1, 0, 1, 0));
        if h_idx > 0 {
            blocks.push(DiffBlock::Separator(h_header.clone()));
        }

        let mut old_ln = old_start_raw.max(1);
        let mut new_ln = new_start_raw.max(1);
        let mut equal_buf: Vec<DiffContextLine> = Vec::new();

        let flush_equal = |buf: &mut Vec<DiffContextLine>, blocks: &mut Vec<DiffBlock>| {
            if !buf.is_empty() {
                blocks.push(DiffBlock::Equal(std::mem::take(buf)));
            }
        };

        let mut i = 0usize;
        while i < h_lines.len() {
            let line = &h_lines[i];
            if line.starts_with('\\') {
                // `\ No newline at end of file`
                i += 1;
                continue;
            }

            if line.starts_with('-') || line.starts_with('+') {
                flush_equal(&mut equal_buf, &mut blocks);
                let block_old_start = old_ln;
                let block_new_start = new_ln;
                let mut old_chunk: Vec<String> = Vec::new();
                let mut new_chunk: Vec<String> = Vec::new();
                let mut raw_chunk_lines: Vec<String> = Vec::new();

                while i < h_lines.len() {
                    let cur = &h_lines[i];
                    if let Some(removed) = cur.strip_prefix('-') {
                        // If we already collected `+` lines and now see another `-`, start a new change block
                        if !new_chunk.is_empty() {
                            break;
                        }
                        old_chunk.push(removed.to_string());
                        raw_chunk_lines.push(cur.clone());
                        old_ln += 1;
                        i += 1;
                    } else if let Some(added) = cur.strip_prefix('+') {
                        new_chunk.push(added.to_string());
                        raw_chunk_lines.push(cur.clone());
                        new_ln += 1;
                        i += 1;
                    } else if cur.starts_with('\\') {
                        raw_chunk_lines.push(cur.clone());
                        i += 1;
                    } else {
                        break;
                    }
                }

                let kind = match (!old_chunk.is_empty(), !new_chunk.is_empty()) {
                    (true, false) => {
                        stats.removed_blocks += 1;
                        DiffChangeKind::Removed
                    }
                    (false, true) => {
                        stats.added_blocks += 1;
                        DiffChangeKind::Added
                    }
                    _ => {
                        stats.modified_blocks += 1;
                        DiffChangeKind::Modified
                    }
                };
                stats.removed_lines += old_chunk.len();
                stats.added_lines += new_chunk.len();

                let hdr_old_start = if old_chunk.is_empty() {
                    block_old_start.saturating_sub(1)
                } else {
                    block_old_start
                };
                let hdr_new_start = if new_chunk.is_empty() {
                    block_new_start.saturating_sub(1)
                } else {
                    block_new_start
                };
                let block_header = format!(
                    "@@ -{},{} +{},{} @@",
                    hdr_old_start,
                    old_chunk.len(),
                    hdr_new_start,
                    new_chunk.len()
                );
                let block_patch =
                    build_hunk_patch(&file_header, &block_header, &raw_chunk_lines);

                blocks.push(DiffBlock::Change(DiffChangeBlock {
                    index: change_idx,
                    kind,
                    old_start: block_old_start,
                    old_lines: old_chunk,
                    new_start: block_new_start,
                    new_lines: new_chunk,
                    header: block_header,
                    patch: block_patch,
                }));
                change_idx += 1;
            } else {
                let text = line.strip_prefix(' ').unwrap_or(line).to_string();
                equal_buf.push(DiffContextLine {
                    old_line: old_ln,
                    new_line: new_ln,
                    text,
                });
                old_ln += 1;
                new_ln += 1;
                i += 1;
            }
        }
        flush_equal(&mut equal_buf, &mut blocks);
    }

    ParsedDiff {
        file_header,
        hunks,
        blocks,
        stats,
        is_full_file: single_file_start,
        raw: raw.to_string(),
    }
}

fn build_hunk_patch(file_header: &str, hunk_header: &str, lines: &[String]) -> String {
    let mut patch = String::new();
    if !file_header.is_empty() {
        patch.push_str(file_header);
        patch.push('\n');
    }
    patch.push_str(hunk_header);
    patch.push('\n');
    for line in lines {
        patch.push_str(line);
        patch.push('\n');
    }
    patch
}

/// Computes a full-file `ParsedDiff` between two in-memory text strings without requiring a Git
/// repository (used by the standalone Detached Diff / Preview window when comparing arbitrary files).
pub fn diff_two_texts(left_label: &str, left: &str, right_label: &str, right: &str) -> ParsedDiff {
    let a: Vec<&str> = left.lines().collect();
    let b: Vec<&str> = right.lines().collect();

    let file_header = format!("--- {left_label}\n+++ {right_label}");
    let mut prefix = 0usize;
    while prefix < a.len() && prefix < b.len() && a[prefix] == b[prefix] {
        prefix += 1;
    }
    let mut suffix = 0usize;
    while suffix < a.len() - prefix
        && suffix < b.len() - prefix
        && a[a.len() - 1 - suffix] == b[b.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let mut raw = format!(
        "{file_header}\n@@ -1,{} +1,{} @@\n",
        a.len(),
        b.len()
    );

    for line in &a[..prefix] {
        raw.push(' ');
        raw.push_str(line);
        raw.push('\n');
    }

    let a_mid = &a[prefix..a.len() - suffix];
    let b_mid = &b[prefix..b.len() - suffix];
    let m = a_mid.len();
    let n = b_mid.len();

    if m == 0 || n == 0 || m.saturating_mul(n) > 400_000 {
        for line in a_mid {
            raw.push('-');
            raw.push_str(line);
            raw.push('\n');
        }
        for line in b_mid {
            raw.push('+');
            raw.push_str(line);
            raw.push('\n');
        }
    } else {
        let cols = n + 1;
        let mut dp = vec![0u32; (m + 1) * cols];
        for i in (0..m).rev() {
            for j in (0..n).rev() {
                dp[i * cols + j] = if a_mid[i] == b_mid[j] {
                    1 + dp[(i + 1) * cols + (j + 1)]
                } else {
                    dp[(i + 1) * cols + j].max(dp[i * cols + (j + 1)])
                };
            }
        }
        let mut i = 0usize;
        let mut j = 0usize;
        while i < m && j < n {
            if a_mid[i] == b_mid[j] {
                raw.push(' ');
                raw.push_str(a_mid[i]);
                raw.push('\n');
                i += 1;
                j += 1;
            } else if dp[(i + 1) * cols + j] >= dp[i * cols + (j + 1)] {
                raw.push('-');
                raw.push_str(a_mid[i]);
                raw.push('\n');
                i += 1;
            } else {
                raw.push('+');
                raw.push_str(b_mid[j]);
                raw.push('\n');
                j += 1;
            }
        }
        while i < m {
            raw.push('-');
            raw.push_str(a_mid[i]);
            raw.push('\n');
            i += 1;
        }
        while j < n {
            raw.push('+');
            raw.push_str(b_mid[j]);
            raw.push('\n');
            j += 1;
        }
    }

    for line in &a[a.len() - suffix..] {
        raw.push(' ');
        raw.push_str(line);
        raw.push('\n');
    }

    parse_unified_diff(&raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiple_hunks_with_valid_headers() {
        let raw = "\
diff --git a/foo.rs b/foo.rs
--- a/foo.rs
+++ b/foo.rs
@@ -1,3 +1,4 @@
 fn a() {}
+fn b() {}
 fn c() {}
@@ -10,2 +11,2 @@
-let x = 1;
+let x = 2;
";
        let parsed = parse_unified_diff(raw);
        assert_eq!(parsed.hunks.len(), 2);
        assert!(parsed.hunks[0].patch.contains("+++ b/foo.rs"));
        assert!(parsed.hunks[0].patch.contains("+fn b() {}"));
        assert!(parsed.hunks[1].patch.contains("-let x = 1;"));
        assert_eq!(parsed.stats.added_blocks, 1);
        assert_eq!(parsed.stats.modified_blocks, 1);
        assert_eq!(parsed.stats.removed_blocks, 0);
        assert_eq!(parsed.stats.added_lines, 2);
        assert_eq!(parsed.stats.removed_lines, 1);
    }

    #[test]
    fn classifies_added_removed_modified_blocks_with_exact_line_numbers() {
        let raw = "\
diff --git a/main.go b/main.go
--- a/main.go
+++ b/main.go
@@ -1,8 +1,8 @@
 line1
-del1
-del2
 line4
+add1
+add2
 line5
-old6
+new6
 line7
";
        let parsed = parse_unified_diff(raw);
        assert!(parsed.is_full_file);
        assert_eq!(parsed.stats.removed_blocks, 1);
        assert_eq!(parsed.stats.added_blocks, 1);
        assert_eq!(parsed.stats.modified_blocks, 1);
        assert_eq!(parsed.blocks.len(), 7);

        // Equal(1) -> Removed(2..3) -> Equal(4/2) -> Added(3..4) -> Equal(5/5) -> Modified(6/6) -> Equal(7/7)
        match &parsed.blocks[1] {
            DiffBlock::Change(b) => {
                assert_eq!(b.kind, DiffChangeKind::Removed);
                assert_eq!(b.old_start, 2);
                assert_eq!(b.old_lines, vec!["del1", "del2"]);
                assert_eq!(b.new_start, 2);
                assert!(b.new_lines.is_empty());
                assert!(b.patch.contains("@@ -2,2 +1,0 @@"));
            }
            other => panic!("expected removed block, got {other:?}"),
        }
        match &parsed.blocks[3] {
            DiffBlock::Change(b) => {
                assert_eq!(b.kind, DiffChangeKind::Added);
                assert_eq!(b.old_start, 5);
                assert!(b.old_lines.is_empty());
                assert_eq!(b.new_start, 3);
                assert_eq!(b.new_lines, vec!["add1", "add2"]);
                assert!(b.patch.contains("@@ -4,0 +3,2 @@"));
            }
            other => panic!("expected added block, got {other:?}"),
        }
        match &parsed.blocks[5] {
            DiffBlock::Change(b) => {
                assert_eq!(b.kind, DiffChangeKind::Modified);
                assert_eq!(b.old_start, 6);
                assert_eq!(b.new_start, 6);
                assert_eq!(b.old_lines, vec!["old6"]);
                assert_eq!(b.new_lines, vec!["new6"]);
            }
            other => panic!("expected modified block, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn loads_binary_and_csv_file_versions_for_working_staged_and_commits() {
        use gitfleet_core::{CommandLogStore, EventBus};
        use std::sync::Arc;

        let dir = std::env::temp_dir().join(format!(
            "gitfleet_file_versions_{}",
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

        // Commit v1 binary bytes (contains non-UTF8 0x89 PNG magic) + CSV
        let bin_v1: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x00, 0xFF, 0x01];
        let bin_v2: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x00, 0xFE, 0x02];
        let bin_v3: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x00, 0xFD, 0x03];
        std::fs::write(dir.join("hero.png"), &bin_v1).unwrap();
        std::fs::write(dir.join("data.csv"), "sku,stock\nSF-1,10\n").unwrap();
        svc.stage(&dir, &["hero.png".into(), "data.csv".into()])
            .await
            .unwrap();
        svc.commit(&dir, "initial", false).await.unwrap();

        // Stage v2 and modify working tree to v3
        std::fs::write(dir.join("hero.png"), &bin_v2).unwrap();
        svc.stage(&dir, &["hero.png".into()]).await.unwrap();
        std::fs::write(dir.join("hero.png"), &bin_v3).unwrap();

        let staged_pair = svc
            .get_working_file_versions(&dir, "hero.png", true, false)
            .await
            .unwrap();
        assert_eq!(staged_pair.before_bytes.as_deref(), Some(bin_v1.as_slice()));
        assert_eq!(staged_pair.after_bytes.as_deref(), Some(bin_v2.as_slice()));

        let unstaged_pair = svc
            .get_working_file_versions(&dir, "hero.png", false, false)
            .await
            .unwrap();
        assert_eq!(
            unstaged_pair.before_bytes.as_deref(),
            Some(bin_v2.as_slice())
        );
        assert_eq!(
            unstaged_pair.after_bytes.as_deref(),
            Some(bin_v3.as_slice())
        );

        // Commit v3 and verify commit version loading
        svc.stage(&dir, &["hero.png".into()]).await.unwrap();
        svc.commit(&dir, "update hero", false).await.unwrap();
        let head = svc.head_oid(&dir).await.unwrap();
        let commit_pair = svc
            .get_commit_file_versions(&dir, &head, "hero.png")
            .await
            .unwrap();
        assert_eq!(commit_pair.before_bytes.as_deref(), Some(bin_v1.as_slice()));
        assert_eq!(commit_pair.after_bytes.as_deref(), Some(bin_v3.as_slice()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn diffs_two_arbitrary_texts_into_structured_blocks() {
        let left = "alpha\nbeta\ngamma\ndelta\n";
        let right = "alpha\nbeta_2\ngamma\nepsilon\ndelta\n";
        let parsed = diff_two_texts("a.txt", left, "b.txt", right);
        assert!(parsed.is_full_file);
        assert_eq!(parsed.stats.modified_blocks, 1);
        assert_eq!(parsed.stats.added_blocks, 1);
        assert_eq!(parsed.stats.removed_blocks, 0);
    }

    #[tokio::test]
    async fn partial_line_stage_unstage_and_discard_apply_cleanly_via_git() {
        use gitfleet_core::{CommandLogStore, EventBus};
        use std::collections::HashSet;
        use std::sync::Arc;

        let dir = std::env::temp_dir().join(format!(
            "gitfleet_partial_lines_{}",
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

        std::fs::write(dir.join("app.rs"), "line1\nold2\nold3\nline4\n").unwrap();
        svc.stage(&dir, &["app.rs".into()]).await.unwrap();
        svc.commit(&dir, "init", false).await.unwrap();

        // Modify old2->new2, old3->new3, and add extra5
        std::fs::write(dir.join("app.rs"), "line1\nnew2\nnew3\nline4\nextra5\n").unwrap();
        let raw = svc.get_diff(&dir, "app.rs", false, false).await.unwrap();
        let parsed = parse_unified_diff(&raw);

        // 1. Stage ONLY old2 -> new2 (old line 2, new line 2)
        let sel_old: HashSet<usize> = [2usize].into_iter().collect();
        let sel_new: HashSet<usize> = [2usize].into_iter().collect();
        let stage_patch = build_partial_patch(&parsed, &sel_old, &sel_new, false).unwrap();
        svc.apply_hunk(&dir, &stage_patch, true, false).await.unwrap();

        let index_blob = runner::run_git_ok(&dir, &["show", ":app.rs"], None, None, false)
            .await
            .unwrap();
        assert_eq!(index_blob, "line1\nnew2\nold3\nline4\n");

        // 2. Stage extra5 too, then Unstage ONLY extra5 from staged diff!
        let unstaged_raw = svc.get_diff(&dir, "app.rs", false, false).await.unwrap();
        let unstaged_parsed = parse_unified_diff(&unstaged_raw);
        let stage_extra = build_partial_patch(
            &unstaged_parsed,
            &HashSet::new(),
            &[5usize].into_iter().collect(),
            false,
        )
        .unwrap();
        svc.apply_hunk(&dir, &stage_extra, true, false).await.unwrap();

        let staged_raw = svc.get_diff(&dir, "app.rs", true, false).await.unwrap();
        let staged_parsed = parse_unified_diff(&staged_raw);
        let unstage_extra = build_partial_patch(
            &staged_parsed,
            &HashSet::new(),
            &[5usize].into_iter().collect(),
            true,
        )
        .unwrap();
        svc.apply_hunk(&dir, &unstage_extra, true, true).await.unwrap();
        let index_after_unstage = runner::run_git_ok(&dir, &["show", ":app.rs"], None, None, false)
            .await
            .unwrap();
        assert_eq!(index_after_unstage, "line1\nnew2\nold3\nline4\n");

        // 3. Discard ONLY extra5 from working tree while keeping new3 unstaged!
        let wt_raw = svc.get_diff(&dir, "app.rs", false, false).await.unwrap();
        let wt_parsed = parse_unified_diff(&wt_raw);
        let discard_extra = build_partial_patch(
            &wt_parsed,
            &HashSet::new(),
            &[5usize].into_iter().collect(),
            true,
        )
        .unwrap();
        svc.apply_hunk(&dir, &discard_extra, false, true).await.unwrap();
        let disk_after = std::fs::read_to_string(dir.join("app.rs")).unwrap();
        assert_eq!(disk_after, "line1\nnew2\nnew3\nline4\n");

        // 4. Newly added file: stage all 3 lines, then partially unstage line 2!
        std::fs::write(dir.join("fresh.txt"), "a\nb\nc\n").unwrap();
        svc.stage(&dir, &["fresh.txt".into()]).await.unwrap();
        let fresh_staged_raw = svc.get_diff(&dir, "fresh.txt", true, false).await.unwrap();
        let fresh_staged_parsed = parse_unified_diff(&fresh_staged_raw);
        let unstage_b = build_partial_patch(
            &fresh_staged_parsed,
            &HashSet::new(),
            &[2usize].into_iter().collect(),
            true,
        )
        .unwrap();
        svc.apply_hunk(&dir, &unstage_b, true, true).await.unwrap();
        let fresh_index = runner::run_git_ok(&dir, &["show", ":fresh.txt"], None, None, false)
            .await
            .unwrap();
        assert_eq!(fresh_index, "a\nc\n");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
