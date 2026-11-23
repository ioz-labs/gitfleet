use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscardTarget {
    Head,
    Index,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileChangeKind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    Untracked,
    Conflicted,
}

impl FileChangeKind {
    pub fn short_code(self) -> &'static str {
        match self {
            Self::Modified => "M",
            Self::Added => "A",
            Self::Deleted => "D",
            Self::Renamed => "R",
            Self::Copied => "C",
            Self::Untracked => "?",
            Self::Conflicted => "U",
        }
    }

    pub fn state_label(self, staged: bool) -> &'static str {
        if staged {
            match self {
                Self::Added => "Staged (+)",
                Self::Deleted => "Staged (-)",
                Self::Renamed => "Renamed",
                _ => "Staged",
            }
        } else {
            match self {
                Self::Modified => "Modified",
                Self::Added => "Added",
                Self::Deleted => "Deleted",
                Self::Renamed => "Renamed",
                Self::Copied => "Copied",
                Self::Untracked => "Untracked",
                Self::Conflicted => "Conflict",
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub kind: FileChangeKind,
    pub staged: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IgnoredEntries {
    /// Gitignored regular files (relative forward-slash paths, sorted), e.g. `.env`, `apps/web/.env`.
    pub files: Vec<String>,
    /// Collapsed gitignored directories (relative forward-slash paths without trailing `/`, sorted),
    /// e.g. `.vscode`, `target`.
    pub dirs: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IgnoredDirListing {
    /// Direct child regular files relative to repo root (sorted, forward-slash).
    pub files: Vec<String>,
    /// Direct child directories relative to repo root (sorted, forward-slash, no trailing `/`).
    pub dirs: Vec<String>,
    /// True if the directory contained more entries than the listing cap.
    pub truncated: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepoStatusSummary {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub head_oid: Option<String>,
    pub detached: bool,
    pub ahead: usize,
    pub behind: usize,
    pub staged_count: usize,
    pub unstaged_count: usize,
    pub untracked_count: usize,
    pub conflict_count: usize,
    pub is_clean: bool,
    pub in_merge: bool,
    pub in_rebase: bool,
    pub staged_files: Vec<FileChange>,
    pub unstaged_files: Vec<FileChange>,
    pub conflict_files: Vec<FileChange>,
}

impl RepoStatusSummary {
    pub fn total_changes(&self) -> usize {
        self.staged_count + self.unstaged_count + self.untracked_count + self.conflict_count
    }

    /// Mirrors ElectroGit's Suggested Next Action (`tb-next`) logic.
    pub fn suggested_next(&self) -> String {
        if self.conflict_count > 0 {
            format!("Resolve {} conflicts", self.conflict_count)
        } else if self.behind > 0 && self.ahead > 0 {
            format!("Diverged (-{} / +{})", self.behind, self.ahead)
        } else if self.behind > 0 {
            format!("Pull {} incoming", self.behind)
        } else if self.staged_count > 0 {
            format!("Commit {} staged", self.staged_count)
        } else if self.unstaged_count + self.untracked_count > 0 {
            format!("Stage {} changes", self.unstaged_count + self.untracked_count)
        } else if self.ahead > 0 {
            format!("Push {} outgoing", self.ahead)
        } else {
            "Clean — up to date".to_string()
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LogOptions {
    pub limit: usize,
    pub all_refs: bool,
    pub topo_order: bool,
    pub unpushed_only: bool,
    pub incoming_only: bool,
    #[serde(default)]
    pub selected_refs: Vec<String>,
    #[serde(default)]
    pub file_filter_path: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignatureStatus {
    #[default]
    None,
    Verified,
    Unverified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitSummary {
    pub oid: String,
    pub short_oid: String,
    pub summary: String,
    #[serde(default)]
    pub body: String,
    pub author: String,
    pub author_email: String,
    pub date_relative: String,
    pub date_display: String,
    pub refs: String,
    pub timestamp: DateTime<Utc>,
    pub parents: Vec<String>,
    #[serde(default)]
    pub signature: SignatureStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlameLine {
    pub final_line: usize,
    pub oid: String,
    pub short_oid: String,
    pub author: String,
    pub date_short: String,
    pub summary: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RebaseAction {
    Pick,
    Reword(String),
    Squash,
    Fixup,
    Drop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RebaseTodoItem {
    pub action: RebaseAction,
    pub oid: String,
    pub short_oid: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastResult {
    pub clean: bool,
    pub conflicting_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrphanGitlink {
    pub submodule_path: String,
    pub commit_oid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmendTarget {
    pub short_oid: String,
    pub message: String,
    pub pushed_to_upstream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StashEntry {
    pub index: usize,
    pub ref_name: String,
    pub message: String,
    pub date_relative: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflogEntry {
    pub short_oid: String,
    pub selector: String,
    pub action: String,
    pub message: String,
    pub date_relative: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}

impl ResetMode {
    pub fn as_flag(self) -> &'static str {
        match self {
            Self::Soft => "--soft",
            Self::Mixed => "--mixed",
            Self::Hard => "--hard",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmartActionKind {
    Clean,
    ResolveConflicts,
    Pull,
    StageAll,
    FocusCommit,
    Push,
}

impl RepoStatusSummary {
    pub fn smart_action_kind(&self) -> SmartActionKind {
        if self.conflict_count > 0 {
            SmartActionKind::ResolveConflicts
        } else if self.behind > 0 {
            SmartActionKind::Pull
        } else if self.staged_count > 0 {
            SmartActionKind::FocusCommit
        } else if self.unstaged_count + self.untracked_count > 0 {
            SmartActionKind::StageAll
        } else if self.ahead > 0 {
            SmartActionKind::Push
        } else {
            SmartActionKind::Clean
        }
    }
}
