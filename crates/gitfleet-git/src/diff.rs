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

const MAX_FULL_CONTEXT_BYTES: usize = 4 * 1024 * 1024;
const UNTRACKED_MAX_LINES: usize = 10_000;

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
}
