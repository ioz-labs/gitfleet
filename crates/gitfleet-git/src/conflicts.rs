use crate::models::ForecastResult;
use crate::{GitService, runner};
use gitfleet_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictStrategy {
    Ours,
    Theirs,
    MarkResolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MergeMode {
    /// Creates a merge commit (`git merge --no-ff <target>`).
    MergeCommit,
    /// Merges into working tree/index without committing (`git merge --no-commit --no-ff <target>`).
    WorkingTree,
    /// Squashes all incoming commits into staged changes without merge parents (`git merge --squash <target>`).
    Squash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HunkChoice {
    Unresolved,
    Ours,
    Theirs,
    BothOursFirst,
    BothTheirsFirst,
    Custom,
}

impl HunkChoice {
    pub fn is_resolved(self) -> bool {
        self != Self::Unresolved
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Unresolved => "Unresolved",
            Self::Ours => "Ours",
            Self::Theirs => "Theirs",
            Self::BothOursFirst => "Both (Ours → Theirs)",
            Self::BothTheirsFirst => "Both (Theirs → Ours)",
            Self::Custom => "Edited",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictHunk {
    pub id: usize,
    pub ours_label: String,
    pub base_label: Option<String>,
    pub theirs_label: String,
    pub ours_lines: Vec<String>,
    pub base_lines: Option<Vec<String>>,
    pub theirs_lines: Vec<String>,
    pub start_line: usize,
    pub choice: HunkChoice,
    pub custom_text: Option<String>,
}

impl ConflictHunk {
    /// Returns the resolved lines for this hunk, or `None` if still unresolved.
    pub fn resolved_lines(&self) -> Option<Vec<String>> {
        match self.choice {
            HunkChoice::Unresolved => None,
            HunkChoice::Ours => Some(self.ours_lines.clone()),
            HunkChoice::Theirs => Some(self.theirs_lines.clone()),
            HunkChoice::BothOursFirst => {
                let mut out = Vec::with_capacity(self.ours_lines.len() + self.theirs_lines.len());
                out.extend(self.ours_lines.iter().cloned());
                out.extend(self.theirs_lines.iter().cloned());
                Some(out)
            }
            HunkChoice::BothTheirsFirst => {
                let mut out = Vec::with_capacity(self.ours_lines.len() + self.theirs_lines.len());
                out.extend(self.theirs_lines.iter().cloned());
                out.extend(self.ours_lines.iter().cloned());
                Some(out)
            }
            HunkChoice::Custom => {
                let raw = self.custom_text.as_deref().unwrap_or("");
                if raw.is_empty() {
                    Some(Vec::new())
                } else {
                    Some(
                        raw.replace("\r\n", "\n")
                            .split('\n')
                            .map(str::to_string)
                            .collect(),
                    )
                }
            }
        }
    }

    /// Initial seed text when switching this hunk into manual custom edit mode.
    pub fn seed_text_for_editor(&self) -> String {
        if let Some(custom) = &self.custom_text {
            return custom.clone();
        }
        if let Some(lines) = self.resolved_lines() {
            return lines.join("\n");
        }
        let mut combined = Vec::with_capacity(self.ours_lines.len() + self.theirs_lines.len());
        combined.extend(self.ours_lines.iter().cloned());
        combined.extend(self.theirs_lines.iter().cloned());
        combined.join("\n")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictSegment {
    Common {
        start_line: usize,
        lines: Vec<String>,
    },
    Conflict(ConflictHunk),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedConflictFile {
    pub segments: Vec<ConflictSegment>,
    pub eol: String,
    pub trailing_newline: bool,
}

pub fn has_conflict_markers(raw: &str) -> bool {
    let mut saw_ours = false;
    let mut saw_sep = false;
    for line in raw.lines() {
        let trimmed = line.trim_end_matches('\r');
        if trimmed.starts_with("<<<<<<<") {
            saw_ours = true;
            saw_sep = false;
        } else if saw_ours && trimmed.starts_with("=======") {
            saw_sep = true;
        } else if saw_ours && saw_sep && trimmed.starts_with(">>>>>>>") {
            return true;
        }
    }
    false
}

impl ParsedConflictFile {
    /// Parses a file containing Git conflict markers (standard 2-way and `diff3`/`zdiff3` 3-way)
    /// while preserving original line endings (`\r\n` vs `\n`) and trailing newline.
    pub fn parse(raw: &str) -> Self {
        let eol = if raw.contains("\r\n") {
            "\r\n".to_string()
        } else {
            "\n".to_string()
        };
        let trailing_newline = raw.ends_with('\n');
        let normalized = raw.replace("\r\n", "\n");
        let all_lines: Vec<&str> = if normalized.is_empty() {
            Vec::new()
        } else {
            let trimmed_end = normalized.strip_suffix('\n').unwrap_or(&normalized);
            if trimmed_end.is_empty() && trailing_newline {
                vec![""]
            } else {
                trimmed_end.split('\n').collect()
            }
        };

        let mut segments: Vec<ConflictSegment> = Vec::new();
        let mut common_buf: Vec<String> = Vec::new();
        let mut common_start = 1usize;
        let mut next_hunk_id = 0usize;
        let mut i = 0usize;

        while i < all_lines.len() {
            let line = all_lines[i];
            if let Some(rest_ours) = line.strip_prefix("<<<<<<<") {
                // Attempt to scan a complete conflict block
                let block_start = i;
                let ours_label = rest_ours.trim().to_string();
                let mut ours_lines = Vec::new();
                let mut base_label: Option<String> = None;
                let mut base_lines: Option<Vec<String>> = None;
                let mut theirs_lines = Vec::new();
                let mut theirs_label = String::new();

                #[derive(PartialEq, Eq)]
                enum State {
                    Ours,
                    Base,
                    Theirs,
                }
                let mut state = State::Ours;
                let mut j = i + 1;
                let mut closed = false;

                while j < all_lines.len() {
                    let cur = all_lines[j];
                    if state == State::Ours
                        && let Some(rest_base) = cur.strip_prefix("|||||||")
                    {
                        state = State::Base;
                        base_label = Some(rest_base.trim().to_string());
                        base_lines = Some(Vec::new());
                    } else if (state == State::Ours || state == State::Base)
                        && cur.starts_with("=======")
                        && cur.chars().all(|c| c == '=')
                    {
                        state = State::Theirs;
                    } else if state == State::Theirs
                        && let Some(rest_theirs) = cur.strip_prefix(">>>>>>>")
                    {
                        theirs_label = rest_theirs.trim().to_string();
                        closed = true;
                        break;
                    } else {
                        match state {
                            State::Ours => ours_lines.push(cur.to_string()),
                            State::Base => {
                                if let Some(bl) = &mut base_lines {
                                    bl.push(cur.to_string());
                                }
                            }
                            State::Theirs => theirs_lines.push(cur.to_string()),
                        }
                    }
                    j += 1;
                }

                if closed {
                    if !common_buf.is_empty() {
                        segments.push(ConflictSegment::Common {
                            start_line: common_start,
                            lines: std::mem::take(&mut common_buf),
                        });
                    }
                    segments.push(ConflictSegment::Conflict(ConflictHunk {
                        id: next_hunk_id,
                        ours_label: if ours_label.is_empty() {
                            "Ours".to_string()
                        } else {
                            ours_label
                        },
                        base_label,
                        theirs_label: if theirs_label.is_empty() {
                            "Theirs".to_string()
                        } else {
                            theirs_label
                        },
                        ours_lines,
                        base_lines,
                        theirs_lines,
                        start_line: block_start + 1,
                        choice: HunkChoice::Unresolved,
                        custom_text: None,
                    }));
                    next_hunk_id += 1;
                    i = j + 1;
                    common_start = i + 1;
                    continue;
                }
            }

            if common_buf.is_empty() {
                common_start = i + 1;
            }
            common_buf.push(line.to_string());
            i += 1;
        }

        if !common_buf.is_empty() {
            segments.push(ConflictSegment::Common {
                start_line: common_start,
                lines: common_buf,
            });
        }

        Self {
            segments,
            eol,
            trailing_newline,
        }
    }

    pub fn conflict_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|s| matches!(s, ConflictSegment::Conflict(_)))
            .count()
    }

    pub fn unresolved_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|s| {
                matches!(
                    s,
                    ConflictSegment::Conflict(h) if !h.choice.is_resolved()
                )
            })
            .count()
    }

    pub fn hunk(&self, hunk_id: usize) -> Option<&ConflictHunk> {
        self.segments.iter().find_map(|s| match s {
            ConflictSegment::Conflict(h) if h.id == hunk_id => Some(h),
            _ => None,
        })
    }

    pub fn set_choice(&mut self, hunk_id: usize, choice: HunkChoice) {
        for seg in &mut self.segments {
            if let ConflictSegment::Conflict(h) = seg
                && h.id == hunk_id
            {
                h.choice = choice;
                if choice != HunkChoice::Custom {
                    h.custom_text = None;
                }
                break;
            }
        }
    }

    pub fn set_custom_text(&mut self, hunk_id: usize, text: String) {
        for seg in &mut self.segments {
            if let ConflictSegment::Conflict(h) = seg
                && h.id == hunk_id
            {
                h.choice = HunkChoice::Custom;
                h.custom_text = Some(text);
                break;
            }
        }
    }

    pub fn resolve_all(&mut self, choice: HunkChoice) {
        for seg in &mut self.segments {
            if let ConflictSegment::Conflict(h) = seg {
                h.choice = choice;
                if choice != HunkChoice::Custom {
                    h.custom_text = None;
                }
            }
        }
    }

    /// Reconstructs the full file text. Resolved hunks emit their chosen content cleanly;
    /// unresolved hunks retain original Git conflict markers so partial saves stay lossless.
    pub fn render_output(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        for seg in &self.segments {
            match seg {
                ConflictSegment::Common { lines: common, .. } => {
                    lines.extend(common.iter().cloned());
                }
                ConflictSegment::Conflict(h) => {
                    if let Some(resolved) = h.resolved_lines() {
                        lines.extend(resolved);
                    } else {
                        lines.push(format!("<<<<<<< {}", h.ours_label));
                        lines.extend(h.ours_lines.iter().cloned());
                        if let Some(base) = &h.base_lines {
                            let blabel = h.base_label.as_deref().unwrap_or("base");
                            lines.push(format!("||||||| {blabel}"));
                            lines.extend(base.iter().cloned());
                        }
                        lines.push("=======".to_string());
                        lines.extend(h.theirs_lines.iter().cloned());
                        lines.push(format!(">>>>>>> {}", h.theirs_label));
                    }
                }
            }
        }

        let eol = if self.eol.is_empty() {
            "\n"
        } else {
            self.eol.as_str()
        };
        let mut out = lines.join(eol);
        if self.trailing_newline && !out.is_empty() {
            out.push_str(eol);
        }
        out
    }
}

fn safe_rel_path(repo_path: &Path, rel: &str) -> AppResult<std::path::PathBuf> {
    let p = Path::new(rel);
    if p.is_absolute()
        || p.components()
            .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(AppError::Git(format!(
            "Unsafe relative path: {rel}"
        )));
    }
    Ok(repo_path.join(p))
}

impl GitService {
    /// In-memory merge forecast using `git merge-tree --write-tree` (does not touch working tree).
    pub async fn merge_forecast(
        &self,
        repo_path: &Path,
        target_branch: &str,
    ) -> AppResult<ForecastResult> {
        let out = runner::run_git(
            repo_path,
            &["merge-tree", "--write-tree", "--name-only", "HEAD", target_branch],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;

        if out.exit_code == 0 {
            return Ok(ForecastResult {
                clean: true,
                conflicting_paths: Vec::new(),
            });
        }

        let mut conflicts = Vec::new();
        for line in out.stdout.lines().skip(1) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                break;
            }
            conflicts.push(trimmed.to_string());
        }
        Ok(ForecastResult {
            clean: false,
            conflicting_paths: conflicts,
        })
    }

    pub async fn merge_branch(&self, repo_path: &Path, target_branch: &str) -> AppResult<String> {
        self.merge_branch_with_mode(repo_path, target_branch, MergeMode::MergeCommit)
            .await
    }

    pub async fn merge_branch_with_mode(
        &self,
        repo_path: &Path,
        target_branch: &str,
        mode: MergeMode,
    ) -> AppResult<String> {
        let args: Vec<&str> = match mode {
            MergeMode::MergeCommit => vec!["merge", "--no-ff", target_branch],
            MergeMode::WorkingTree => vec!["merge", "--no-commit", "--no-ff", target_branch],
            MergeMode::Squash => vec!["merge", "--squash", target_branch],
        };
        runner::run_git_ok(
            repo_path,
            &args,
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn merge_ff_only(&self, repo_path: &Path, target_branch: &str) -> AppResult<String> {
        runner::run_git_ok(
            repo_path,
            &["merge", "--ff-only", target_branch],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn rebase_branch(&self, repo_path: &Path, target_branch: &str) -> AppResult<String> {
        runner::run_git_ok(
            repo_path,
            &["rebase", target_branch],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn resolve_conflict(
        &self,
        repo_path: &Path,
        file_path: &str,
        strategy: ConflictStrategy,
    ) -> AppResult<String> {
        match strategy {
            ConflictStrategy::Ours => {
                runner::run_git_ok(
                    repo_path,
                    &["checkout", "--ours", "--", file_path],
                    Some(&self.command_log),
                    Some(&self.events),
                    true,
                )
                .await?;
            }
            ConflictStrategy::Theirs => {
                runner::run_git_ok(
                    repo_path,
                    &["checkout", "--theirs", "--", file_path],
                    Some(&self.command_log),
                    Some(&self.events),
                    true,
                )
                .await?;
            }
            ConflictStrategy::MarkResolved => {}
        }
        runner::run_git_ok(
            repo_path,
            &["add", "--", file_path],
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    /// Writes interactively resolved file content to disk and optionally stages it (`git add`)
    /// to mark the conflict resolved.
    pub async fn save_resolved_conflict(
        &self,
        repo_path: &Path,
        file_path: &str,
        content: &str,
        mark_resolved: bool,
    ) -> AppResult<String> {
        let full = safe_rel_path(repo_path, file_path)?;
        std::fs::write(&full, content)?;
        if mark_resolved {
            runner::run_git_ok(
                repo_path,
                &["add", "--", file_path],
                Some(&self.command_log),
                Some(&self.events),
                true,
            )
            .await
        } else {
            Ok("Saved resolved draft to disk".to_string())
        }
    }

    pub async fn continue_operation(&self, repo_path: &Path) -> AppResult<String> {
        let git_dir = repo_path.join(".git");
        if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
            runner::run_git_ok(
                repo_path,
                &["rebase", "--continue"],
                Some(&self.command_log),
                Some(&self.events),
                true,
            )
            .await
        } else if git_dir.join("CHERRY_PICK_HEAD").exists() {
            runner::run_git_ok(
                repo_path,
                &["cherry-pick", "--continue"],
                Some(&self.command_log),
                Some(&self.events),
                true,
            )
            .await
        } else if git_dir.join("REVERT_HEAD").exists() {
            runner::run_git_ok(
                repo_path,
                &["revert", "--continue"],
                Some(&self.command_log),
                Some(&self.events),
                true,
            )
            .await
        } else {
            runner::run_git_ok(
                repo_path,
                &["merge", "--continue"],
                Some(&self.command_log),
                Some(&self.events),
                true,
            )
            .await
        }
    }

    pub async fn abort_operation(&self, repo_path: &Path) -> AppResult<String> {
        let git_dir = repo_path.join(".git");
        if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
            runner::run_git_ok(
                repo_path,
                &["rebase", "--abort"],
                Some(&self.command_log),
                Some(&self.events),
                true,
            )
            .await
        } else if git_dir.join("CHERRY_PICK_HEAD").exists() {
            runner::run_git_ok(
                repo_path,
                &["cherry-pick", "--abort"],
                Some(&self.command_log),
                Some(&self.events),
                true,
            )
            .await
        } else if git_dir.join("REVERT_HEAD").exists() {
            runner::run_git_ok(
                repo_path,
                &["revert", "--abort"],
                Some(&self.command_log),
                Some(&self.events),
                true,
            )
            .await
        } else {
            runner::run_git_ok(
                repo_path,
                &["merge", "--abort"],
                Some(&self.command_log),
                Some(&self.events),
                true,
            )
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitfleet_core::{CommandLogStore, EventBus};
    use std::sync::Arc;

    #[test]
    fn parses_2way_and_diff3_conflicts_and_renders_all_choices_preserving_eol() {
        let raw = "header\r\n<<<<<<< HEAD\r\nours_1\r\nours_2\r\n||||||| base\r\nbase_line\r\n=======\r\ntheirs_1\r\n>>>>>>> feat/api\r\nmid\r\n<<<<<<< HEAD\r\nkeep_a\r\n=======\r\nkeep_b\r\n>>>>>>> feat/api\r\nfooter\r\n";
        assert!(has_conflict_markers(raw));

        let mut parsed = ParsedConflictFile::parse(raw);
        assert_eq!(parsed.eol, "\r\n");
        assert!(parsed.trailing_newline);
        assert_eq!(parsed.conflict_count(), 2);
        assert_eq!(parsed.unresolved_count(), 2);

        let h0 = parsed.hunk(0).unwrap();
        assert_eq!(h0.ours_label, "HEAD");
        assert_eq!(h0.base_label.as_deref(), Some("base"));
        assert_eq!(h0.theirs_label, "feat/api");
        assert_eq!(h0.ours_lines, vec!["ours_1", "ours_2"]);
        assert_eq!(h0.base_lines.as_deref(), Some(&["base_line".to_string()][..]));
        assert_eq!(h0.theirs_lines, vec!["theirs_1"]);

        // Partial resolve: hunk 0 -> BothOursFirst, hunk 1 stays unresolved
        parsed.set_choice(0, HunkChoice::BothOursFirst);
        assert_eq!(parsed.unresolved_count(), 1);
        let partial = parsed.render_output();
        assert!(has_conflict_markers(&partial));
        assert!(partial.contains("ours_1\r\nours_2\r\ntheirs_1\r\nmid\r\n"));

        // Custom edit on hunk 1
        parsed.set_custom_text(1, "merged_a_and_b".to_string());
        assert_eq!(parsed.unresolved_count(), 0);
        let final_out = parsed.render_output();
        assert!(!has_conflict_markers(&final_out));
        assert_eq!(
            final_out,
            "header\r\nours_1\r\nours_2\r\ntheirs_1\r\nmid\r\nmerged_a_and_b\r\nfooter\r\n"
        );
    }

    #[tokio::test]
    async fn merge_modes_squash_working_tree_and_interactive_conflict_save_work() {
        let dir = std::env::temp_dir().join(format!(
            "gitfleet_merge_modes_{}",
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

        std::fs::write(dir.join("app.txt"), "base\n").unwrap();
        svc.stage(&dir, &["app.txt".into()]).await.unwrap();
        svc.commit(&dir, "init", false).await.unwrap();

        // Create feature branch with conflicting change
        svc.create_branch(&dir, "feat/conflict").await.unwrap();
        std::fs::write(dir.join("app.txt"), "theirs\n").unwrap();
        svc.stage(&dir, &["app.txt".into()]).await.unwrap();
        svc.commit(&dir, "feat change", false).await.unwrap();

        // Create clean branch for squash merge
        svc.checkout(&dir, "main").await.unwrap();
        svc.create_branch(&dir, "feat/clean").await.unwrap();
        std::fs::write(dir.join("extra.txt"), "hello\n").unwrap();
        svc.stage(&dir, &["extra.txt".into()]).await.unwrap();
        svc.commit(&dir, "add extra", false).await.unwrap();

        // Squash-merge feat/clean into main -> stages extra.txt without creating a merge commit
        svc.checkout(&dir, "main").await.unwrap();
        svc.merge_branch_with_mode(&dir, "feat/clean", MergeMode::Squash)
            .await
            .unwrap();
        let st_sq = svc.get_status(&dir).await.unwrap();
        assert_eq!(st_sq.staged_count, 1);
        assert!(!st_sq.in_merge);
        svc.commit(&dir, "squash feat/clean", false).await.unwrap();

        // Modify app.txt on main so merging feat/conflict triggers a real conflict
        std::fs::write(dir.join("app.txt"), "ours\n").unwrap();
        svc.stage(&dir, &["app.txt".into()]).await.unwrap();
        svc.commit(&dir, "main change", false).await.unwrap();

        let merge_res = svc
            .merge_branch_with_mode(&dir, "feat/conflict", MergeMode::WorkingTree)
            .await;
        assert!(merge_res.is_err());

        let st_conf = svc.get_status(&dir).await.unwrap();
        assert!(st_conf.in_merge);
        assert_eq!(st_conf.conflict_count, 1);

        let raw = std::fs::read_to_string(dir.join("app.txt")).unwrap();
        let mut parsed = ParsedConflictFile::parse(&raw);
        assert_eq!(parsed.conflict_count(), 1);
        parsed.set_choice(0, HunkChoice::BothOursFirst);
        let resolved = parsed.render_output();
        svc.save_resolved_conflict(&dir, "app.txt", &resolved, true)
            .await
            .unwrap();

        let st_after = svc.get_status(&dir).await.unwrap();
        assert_eq!(st_after.conflict_count, 0);
        assert_eq!(
            std::fs::read_to_string(dir.join("app.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "ours\ntheirs\n"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
