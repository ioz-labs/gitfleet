use gitfleet_core::{RepoId, SessionId};
use gitfleet_git::bulk::BulkOperation;
use gitfleet_git::{DiscardTarget, FileChange, ResetMode};
use gitfleet_terminal::TermScreenSnapshot;
use iced::Point;
use std::path::PathBuf;
pub(crate) const STATUS_BAR_HEIGHT: f32 = crate::theme::layout::STATUS_BAR_HEIGHT;
pub(crate) const TITLEBAR_HEIGHT: f32 = 36.0;
pub(crate) const TOOLBAR_HEIGHT: f32 = 40.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResizeTarget {
    SidebarWidth,
    LeftSplit,
    RightSplit,
    TopRightSplit,
    CommitFilesSplit,
    FilesStagedSplit,
    DrawerHeight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowKind {
    Host,
    DetachedViewer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffGapExpandMode {
    Top(usize),
    Bottom(usize),
    All,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DiffGapExpansion {
    pub(crate) top_shown: usize,
    pub(crate) bottom_shown: usize,
    pub(crate) show_all: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveFocusPane {
    Other,
    TextInput,
    FilesUnstaged,
    FilesStaged,
    FilesCommit,
    Diff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffSelectionSide {
    Left,
    Right,
    Unified,
    Raw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct DiffTextPos {
    pub(crate) row: usize,
    pub(crate) col: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiffTextSelection {
    pub(crate) side: DiffSelectionSide,
    pub(crate) anchor: DiffTextPos,
    pub(crate) focus: DiffTextPos,
    pub(crate) dragging: bool,
}

impl DiffTextSelection {
    pub(crate) fn ordered(&self) -> (DiffTextPos, DiffTextPos) {
        if self.anchor <= self.focus {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.anchor == self.focus
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RepoFilter {
    All,
    Dirty,
    Sync,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GraphSelection {
    WorkingTree,
    Commit(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BottomLeftTab {
    Branches,
    Stashes,
    Tags,
    Submodules,
    Worktrees,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DrawerTab {
    Terminal,
    Search,
    Secrets,
    Ignore,
    CommandLog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GraphTab {
    Graph,
    Reflog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FilesViewMode {
    Flat,
    Dir,
    AllFiles,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BottomViewMode {
    UnifiedDiff,
    SideBySideDiff,
    Preview,
    Edit,
}

#[derive(Debug, Clone)]
pub(crate) enum ConfirmAction {
    DiscardHunk { patch: String },
    ResetCommit { oid: String, mode: ResetMode },
    RevertCommit { oid: String },
    AbortGitOp,
    DropStash { index: usize },
    DeleteTag { name: String },
    DeleteBranch { name: String, force: bool },
    DeleteBranches { names: Vec<String>, force: bool },
    RemoveRepo { id: RepoId },
    RemoveBulkSelected,
    DeleteGroup { name: String },
    DeleteWorkspace { path: PathBuf },
    RemoveSubmodule { path: String },
    RemoveWorktree { path: PathBuf },
    ResetVaultKey,
    ForceCommit { push_after: bool },
    ExecuteMerge { target: String },
    FastForwardMerge { branch: String },
    ExecuteRebase { target: String },
    CheckoutCommit { oid: String },
    PushSelected,
    PushBranch { branch: String },
    PushTag { name: String },
    UnsetUpstream { branch: String },
    ApplyStash { index: usize },
    PopStash { index: usize },
    StashSelected,
    UndoLastAction,
    BulkOp { op: BulkOperation },
}

#[derive(Debug, Clone)]
pub(crate) struct PendingConfirm {
    pub(crate) title: String,
    pub(crate) message: String,
    pub(crate) detail: Option<String>,
    pub(crate) confirm_label: String,
    pub(crate) is_danger: bool,
    pub(crate) action: ConfirmAction,
}

impl PendingConfirm {
    pub(crate) fn discard_hunk(file_title: &str, header: &str, patch: String) -> Self {
        Self {
            title: "Discard Diff Hunk?".into(),
            message: "This unstaged hunk will be reverted from your working tree and cannot be undone."
                .into(),
            detail: Some(format!("{file_title}\n{header}")),
            confirm_label: "Discard Hunk".into(),
            is_danger: true,
            action: ConfirmAction::DiscardHunk { patch },
        }
    }

    pub(crate) fn reset_commit(oid: String, short_oid: &str, mode: ResetMode) -> Self {
        let mode_desc = match mode {
            ResetMode::Soft => "Soft Reset: moves HEAD back while keeping all changes staged.",
            ResetMode::Mixed => {
                "Mixed Reset: moves HEAD back and unstages changes into your working tree."
            }
            ResetMode::Hard => {
                "Hard Reset: moves HEAD back and PERMANENTLY deletes all uncommitted changes!"
            }
        };
        Self {
            title: format!("Reset Repository ({mode:?})?"),
            message: format!("{mode_desc} You can still recover the previous HEAD from Reflog / Undo."),
            detail: Some(format!("Target commit: {short_oid}")),
            confirm_label: format!("Reset {mode:?}"),
            is_danger: true,
            action: ConfirmAction::ResetCommit { oid, mode },
        }
    }

    pub(crate) fn revert_commit(oid: String, short_oid: &str, summary: &str) -> Self {
        Self {
            title: "Revert Commit?".into(),
            message: "Creates a new commit that undoes the changes introduced by this commit."
                .into(),
            detail: Some(format!("{short_oid}  {summary}")),
            confirm_label: "Revert Commit".into(),
            is_danger: true,
            action: ConfirmAction::RevertCommit { oid },
        }
    }

    pub(crate) fn abort_git_op() -> Self {
        Self {
            title: "Abort In-Progress Operation?".into(),
            message: "Aborts the current merge, rebase, or cherry-pick and discards any conflict resolutions made so far."
                .into(),
            detail: None,
            confirm_label: "Abort Operation".into(),
            is_danger: true,
            action: ConfirmAction::AbortGitOp,
        }
    }

    pub(crate) fn drop_stash(index: usize, ref_name: &str, msg: &str) -> Self {
        Self {
            title: "Drop Stash?".into(),
            message: "Permanently removes this stash entry from the stash list.".into(),
            detail: Some(format!("{ref_name}: {msg}")),
            confirm_label: "Drop Stash".into(),
            is_danger: true,
            action: ConfirmAction::DropStash { index },
        }
    }

    pub(crate) fn delete_tag(name: String) -> Self {
        Self {
            title: "Delete Git Tag?".into(),
            message: "Deletes this local Git tag (remote tags are not modified).".into(),
            detail: Some(name.clone()),
            confirm_label: "Delete Tag".into(),
            is_danger: true,
            action: ConfirmAction::DeleteTag { name },
        }
    }

    pub(crate) fn delete_branch(name: String, force: bool) -> Self {
        Self {
            title: if force {
                "Force Delete Local Branch?".into()
            } else {
                "Delete Local Branch?".into()
            },
            message: if force {
                "Force-deletes this local branch even if it contains unmerged commits (git branch -D)."
                    .into()
            } else {
                "Deletes this local branch (git branch -d). Unmerged branches are protected unless force-deleted."
                    .into()
            },
            detail: Some(name.clone()),
            confirm_label: if force {
                "Force Delete".into()
            } else {
                "Delete Branch".into()
            },
            is_danger: true,
            action: ConfirmAction::DeleteBranch { name, force },
        }
    }

    pub(crate) fn delete_branches(names: Vec<String>, force: bool) -> Self {
        let count = names.len();
        Self {
            title: if force {
                format!("Force Delete {count} Local Branch(es)?")
            } else {
                format!("Delete {count} Local Branch(es)?")
            },
            message: if force {
                "Force-deletes all selected local branches even if they have unmerged commits (git branch -D)."
                    .into()
            } else {
                "Deletes all selected local branches (git branch -d). Unmerged branches are protected unless force-deleted."
                    .into()
            },
            detail: Some(names.join("\n")),
            confirm_label: if force {
                format!("Force Delete ({count})")
            } else {
                format!("Delete ({count})")
            },
            is_danger: true,
            action: ConfirmAction::DeleteBranches { names, force },
        }
    }

    pub(crate) fn remove_repo(id: RepoId, name: &str, path: &std::path::Path) -> Self {
        Self {
            title: "Remove Repository from Workspace?".into(),
            message: "Removes this repository from your GitFleet workspace list. Your files and Git history on disk will NOT be deleted."
                .into(),
            detail: Some(format!("{name}\n{}", path.display())),
            confirm_label: "Remove Repo".into(),
            is_danger: true,
            action: ConfirmAction::RemoveRepo { id },
        }
    }

    pub(crate) fn remove_bulk(count: usize) -> Self {
        Self {
            title: format!("Remove {count} Repositories?"),
            message: "Removes all selected repositories from your GitFleet workspace list. Local folders on disk will NOT be deleted."
                .into(),
            detail: Some(format!("{count} selected repositories")),
            confirm_label: format!("Remove ({count})"),
            is_danger: true,
            action: ConfirmAction::RemoveBulkSelected,
        }
    }

    pub(crate) fn delete_group(name: String) -> Self {
        Self {
            title: "Delete Repository Group?".into(),
            message: "Deletes this group (and any subgroups inside it). Affected repositories will move to the parent group or root."
                .into(),
            detail: Some(name.clone()),
            confirm_label: "Delete Group".into(),
            is_danger: true,
            action: ConfirmAction::DeleteGroup { name },
        }
    }

    pub(crate) fn delete_workspace(name: &str, path: PathBuf) -> Self {
        Self {
            title: format!("Delete Workspace '{name}'?"),
            message: "Deletes this saved GitFleet workspace file. Your Git repositories on disk will NOT be affected."
                .into(),
            detail: Some(path.display().to_string()),
            confirm_label: "Delete Workspace".into(),
            is_danger: true,
            action: ConfirmAction::DeleteWorkspace { path },
        }
    }

    pub(crate) fn remove_submodule(path: String) -> Self {
        Self {
            title: "Remove Submodule?".into(),
            message: "Deinitializes and removes this Git submodule from the repository index and working tree."
                .into(),
            detail: Some(path.clone()),
            confirm_label: "Remove Submodule".into(),
            is_danger: true,
            action: ConfirmAction::RemoveSubmodule { path },
        }
    }

    pub(crate) fn remove_worktree(path: PathBuf) -> Self {
        Self {
            title: "Remove Git Worktree?".into(),
            message: "Removes this linked Git worktree directory from disk.".into(),
            detail: Some(path.display().to_string()),
            confirm_label: "Remove Worktree".into(),
            is_danger: true,
            action: ConfirmAction::RemoveWorktree { path },
        }
    }

    pub(crate) fn reset_vault_key() -> Self {
        Self {
            title: "Reset Age Encryption Key?".into(),
            message: "WARNING: Replaces your local Age identity key with a brand new key. Existing '.gitfleet/secrets.age' vaults encrypted with the old key will no longer decrypt unless you backed up the old private key!"
                .into(),
            detail: None,
            confirm_label: "Reset Key".into(),
            is_danger: true,
            action: ConfirmAction::ResetVaultKey,
        }
    }

    pub(crate) fn amend_pushed(short_oid: &str, push_after: bool) -> Self {
        Self {
            title: "Amend Already-Pushed Commit?".into(),
            message: "HEAD has already been pushed to upstream. Amending rewrites Git history and will require a force-push or diverge from collaborators."
                .into(),
            detail: Some(format!("Commit {short_oid}")),
            confirm_label: if push_after {
                "Amend & Push".into()
            } else {
                "Amend Commit".into()
            },
            is_danger: true,
            action: ConfirmAction::ForceCommit { push_after },
        }
    }

    pub(crate) fn commit_with_secrets(count: usize, push_after: bool) -> Self {
        Self {
            title: format!("Commit With {count} Potential Secret(s)?"),
            message: "Pre-commit hygiene detected potential credentials or private keys in staged files. Are you sure you want to commit them?"
                .into(),
            detail: Some(format!("{count} finding(s) flagged in staged diff")),
            confirm_label: if push_after {
                "Commit & Push Anyway".into()
            } else {
                "Commit Anyway".into()
            },
            is_danger: true,
            action: ConfirmAction::ForceCommit { push_after },
        }
    }

    pub(crate) fn merge_ref(target: String, label: &str) -> Self {
        Self {
            title: "Merge into Current Branch?".into(),
            message: "Merges the target commit or branch into the currently checked-out branch."
                .into(),
            detail: Some(label.to_string()),
            confirm_label: "Merge".into(),
            is_danger: false,
            action: ConfirmAction::ExecuteMerge { target },
        }
    }

    pub(crate) fn ff_merge_branch(branch: String) -> Self {
        Self {
            title: "Fast-Forward Merge?".into(),
            message: "Advances the current branch pointer directly to the target branch if possible (git merge --ff-only).".into(),
            detail: Some(branch.clone()),
            confirm_label: "Fast-Forward".into(),
            is_danger: false,
            action: ConfirmAction::FastForwardMerge { branch },
        }
    }

    pub(crate) fn rebase_onto(target: String, label: &str) -> Self {
        Self {
            title: "Rebase Current Branch?".into(),
            message: "Replays local commits from your current branch on top of the selected target. This rewrites local commit history.".into(),
            detail: Some(label.to_string()),
            confirm_label: "Rebase".into(),
            is_danger: true,
            action: ConfirmAction::ExecuteRebase { target },
        }
    }

    pub(crate) fn checkout_commit(oid: String, short_oid: &str, summary: &str) -> Self {
        Self {
            title: "Check Out Commit (Detached HEAD)?".into(),
            message: "Checks out this commit directly in Detached HEAD state. You will not be on any local branch until you check out or create one.".into(),
            detail: Some(format!("{short_oid}  {summary}")),
            confirm_label: "Check Out".into(),
            is_danger: false,
            action: ConfirmAction::CheckoutCommit { oid },
        }
    }

    pub(crate) fn push_selected(branch: &str, ahead: usize) -> Self {
        Self {
            title: "Push Commits to Remote?".into(),
            message: if ahead > 0 {
                format!("Pushes {ahead} outgoing commit(s) from '{branch}' to the remote tracking branch.")
            } else {
                format!("Pushes local branch '{branch}' to its remote tracking branch.")
            },
            detail: Some(format!("Branch: {branch}")),
            confirm_label: "Push".into(),
            is_danger: false,
            action: ConfirmAction::PushSelected,
        }
    }

    pub(crate) fn push_branch(branch: String) -> Self {
        Self {
            title: "Push Branch to Remote?".into(),
            message: "Pushes this local branch to origin and sets upstream tracking (git push -u origin).".into(),
            detail: Some(branch.clone()),
            confirm_label: "Push Branch".into(),
            is_danger: false,
            action: ConfirmAction::PushBranch { branch },
        }
    }

    pub(crate) fn push_tag(name: String) -> Self {
        Self {
            title: "Push Tag to Remote?".into(),
            message: "Publishes this Git tag to origin. Note: tags may trigger CI/CD release workflows.".into(),
            detail: Some(name.clone()),
            confirm_label: "Push Tag".into(),
            is_danger: false,
            action: ConfirmAction::PushTag { name },
        }
    }

    pub(crate) fn unset_upstream(branch: String) -> Self {
        Self {
            title: "Stop Tracking Upstream Branch?".into(),
            message: "Removes the upstream remote tracking configuration for this local branch (git branch --unset-upstream).".into(),
            detail: Some(branch.clone()),
            confirm_label: "Stop Tracking".into(),
            is_danger: false,
            action: ConfirmAction::UnsetUpstream { branch },
        }
    }

    pub(crate) fn apply_stash(index: usize, ref_name: &str, msg: &str) -> Self {
        Self {
            title: "Apply Stash?".into(),
            message: "Applies the stashed changes onto your working tree while keeping the stash entry.".into(),
            detail: Some(format!("{ref_name}: {msg}")),
            confirm_label: "Apply Stash".into(),
            is_danger: false,
            action: ConfirmAction::ApplyStash { index },
        }
    }

    pub(crate) fn pop_stash(index: usize, ref_name: &str, msg: &str) -> Self {
        Self {
            title: "Pop Stash?".into(),
            message: "Applies the stashed changes onto your working tree and removes the stash entry.".into(),
            detail: Some(format!("{ref_name}: {msg}")),
            confirm_label: "Pop Stash".into(),
            is_danger: false,
            action: ConfirmAction::PopStash { index },
        }
    }

    pub(crate) fn stash_selected(changed: usize) -> Self {
        Self {
            title: "Save Local Changes to Stash?".into(),
            message: "Stashes all modified and untracked files (git stash push -u) and cleans your working tree.".into(),
            detail: Some(format!("{changed} changed file(s) in Working Tree")),
            confirm_label: "Save Stash".into(),
            is_danger: false,
            action: ConfirmAction::StashSelected,
        }
    }

    pub(crate) fn undo_last_action(label: &str) -> Self {
        Self {
            title: "Undo Last Git Operation?".into(),
            message: "Resets HEAD (mixed) back to the commit recorded right before the last Reset / Merge / Rebase / Cherry-Pick / Revert.".into(),
            detail: Some(label.to_string()),
            confirm_label: "Undo Action".into(),
            is_danger: true,
            action: ConfirmAction::UndoLastAction,
        }
    }

    pub(crate) fn bulk_op(op: BulkOperation, count: usize) -> Self {
        Self {
            title: format!("Run Bulk {op:?} on {count} Repositories?"),
            message: format!("Executes `git {op:?}` across {count} selected repositories."),
            detail: Some(format!("{count} selected repositories")),
            confirm_label: format!("Run {op:?} ({count})"),
            is_danger: matches!(op, BulkOperation::Push),
            action: ConfirmAction::BulkOp { op },
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CherryPickModalState {
    pub(crate) revisions: Vec<String>,
    pub(crate) detail: String,
    pub(crate) append_source_id: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct DiscardFileEntry {
    pub(crate) path: String,
    pub(crate) state_label: String,
    pub(crate) is_untracked: bool,
    pub(crate) is_added_staged: bool,
    pub(crate) selected: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct DiscardModalState {
    pub(crate) target: DiscardTarget,
    pub(crate) files: Vec<DiscardFileEntry>,
}

#[derive(Debug, Clone)]
pub(crate) struct ContextMenuState {
    pub(crate) position: Point,
    pub(crate) target: ContextMenuTarget,
    pub(crate) submenu_open: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum ContextMenuTarget {
    RepositoriesPane,
    WorkingTree,
    Group {
        name: String,
    },
    Repo {
        id: RepoId,
        name: String,
        path: PathBuf,
        current_group: Option<String>,
    },
    Commit {
        oid: String,
        short_oid: String,
        summary: String,
    },
    Branch {
        name: String,
        is_head: bool,
        is_remote: bool,
    },
    ChangedFile {
        file: FileChange,
        is_working_tree: bool,
    },
}


#[derive(Debug, Clone)]
pub(crate) struct TerminalPane {
    pub(crate) session_id: SessionId,
    pub(crate) profile_label: String,
    pub(crate) short_label: String,
    pub(crate) cols: u16,
    pub(crate) rows: u16,
    pub(crate) snapshot: TermScreenSnapshot,
}

#[derive(Debug, Clone)]
pub(crate) struct TerminalTab {
    pub(crate) panes: Vec<TerminalPane>,
    pub(crate) focused_pane: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalMenuSubmenu {
    None,
    SplitWithProfile,
    SelectDefaultProfile,
}
