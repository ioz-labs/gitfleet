use gitfleet_core::{RepoId, SessionId};
use gitfleet_git::bulk::BulkOperation;
use gitfleet_git::{DiscardTarget, FileChange, MergeMode, RepoStatusSummary, ResetMode};
use gitfleet_terminal::TermScreenSnapshot;
use gitfleet_workspace::{
    ScanFolderResult, ScanLevelInfo, ScannedRepoCandidate, normalize_group_path,
    resolve_scanned_repo_group,
};
use iced::Point;
use std::collections::HashSet;
use std::path::PathBuf;
pub(crate) const STATUS_BAR_HEIGHT: f32 = crate::theme::layout::STATUS_BAR_HEIGHT;
pub(crate) const TITLEBAR_HEIGHT: f32 = 38.0;
pub(crate) const TOOLBAR_HEIGHT: f32 = 42.0;

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
    ImageDiff,
    CsvDiff,
    Preview,
    Edit,
    ResolveConflicts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SettingsTab {
    #[default]
    General,
    Terminal,
    Automation,
    Diagnostics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum EncodingModalMode {
    #[default]
    SaveWithEncoding,
    ReopenWithEncoding,
}

#[derive(Debug, Clone)]
pub(crate) struct ActionErrorState {
    pub(crate) title: String,
    pub(crate) summary: String,
    pub(crate) detail: Option<String>,
    pub(crate) context: Option<String>,
    pub(crate) show_output_shortcut: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum ConfirmAction {
    DiscardHunk { patch: String },
    ResetCommit { oid: String, mode: ResetMode },
    RevertCommit { oid: String },
    AbortGitOp,
    DropStash { index: usize },
    DeleteTag { name: String },
    DeleteBranches { names: Vec<String>, force: bool },
    RemoveRepo { id: RepoId },
    RemoveBulkSelected,
    DeleteGroup { name: String },
    DeleteWorkspace { path: PathBuf },
    RemoveSubmodule { path: String },
    RemoveWorktree { path: PathBuf },
    DeleteVaultKey { key_id: String },
    DestroyRepoVault,
    DeleteVaultProfile { profile: String },
    ForceCommit { push_after: bool },
    CommitAllWorkingTree { push_after: bool },
    ExecuteMerge { target: String, mode: MergeMode },
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
    UninitGitFlow,
    AbortGitFlowBranch {
        branch: String,
        base_branch: String,
        force: bool,
    },
    DeleteRepoEntry {
        rel_path: String,
        is_dir: bool,
    },
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

    pub(crate) fn delete_branches(names: Vec<String>, force: bool) -> Self {
        let count = names.len();
        Self {
            title: if force {
                if count == 1 {
                    "Force Delete Unmerged Local Branch?".into()
                } else {
                    format!("Force Delete {count} Local Branches?")
                }
            } else if count == 1 {
                "Delete Local Branch?".into()
            } else {
                format!("Delete {count} Local Branches?")
            },
            message: if force {
                "Branch(es) contain commits not merged into HEAD (or were squash-merged). Force deleting runs `git branch -D` and cannot be undone easily."
                    .into()
            } else {
                "Deletes the selected local branch(es) (`git branch -d`).".into()
            },
            detail: Some(names.join("\n")),
            confirm_label: if force {
                if count == 1 {
                    "Force Delete (-D)".into()
                } else {
                    format!("Force Delete ({count})")
                }
            } else if count == 1 {
                "Delete Branch".into()
            } else {
                format!("Delete ({count})")
            },
            is_danger: true,
            action: ConfirmAction::DeleteBranches { names, force },
        }
    }

    pub(crate) fn delete_unmerged_branches(names: Vec<String>, unmerged: Vec<String>) -> Self {
        let count = names.len();
        let detail_lines: Vec<String> = names
            .iter()
            .map(|n| {
                if unmerged.contains(n) {
                    format!("{n}  (unmerged commits)")
                } else {
                    n.clone()
                }
            })
            .collect();
        Self {
            title: if count == 1 {
                format!("Force Delete Unmerged Branch '{}'?", names[0])
            } else {
                format!("Force Delete {count} Local Branches?")
            },
            message: "Not all selected branches are fully merged into HEAD (they have unmerged commits or were squash-merged). Click Force Delete to remove them with `git branch -D`."
                .into(),
            detail: Some(detail_lines.join("\n")),
            confirm_label: if count == 1 {
                "Force Delete (-D)".into()
            } else {
                format!("Force Delete ({count})")
            },
            is_danger: true,
            action: ConfirmAction::DeleteBranches { names, force: true },
        }
    }

    pub(crate) fn commit_all_working_tree(
        file_count: usize,
        preview_files: &[String],
        push_after: bool,
    ) -> Self {
        let mut lines: Vec<String> = preview_files.iter().take(6).cloned().collect();
        if file_count > lines.len() {
            lines.push(format!("… and {} more file(s)", file_count - lines.len()));
        }
        Self {
            title: "Do you want to commit all working tree changes?".into(),
            message: "No file is staged yet. To commit all working tree changes, click Commit All. To stage individual changes, click Cancel."
                .into(),
            detail: if lines.is_empty() {
                Some(format!("{file_count} changed file(s)"))
            } else {
                Some(format!("{file_count} changed file(s):\n{}", lines.join("\n")))
            },
            confirm_label: if push_after {
                "Commit All & Push".into()
            } else {
                "Commit All".into()
            },
            is_danger: false,
            action: ConfirmAction::CommitAllWorkingTree { push_after },
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

    pub(crate) fn delete_group(name: String, repo_count: usize) -> Self {
        let (title, message, detail, confirm_label) = if repo_count > 0 {
            (
                "Delete Group & Child Repositories?".into(),
                format!(
                    "Deletes this group (and any subgroups) and removes all {repo_count} child repository(ies) from your GitFleet workspace list. Local folders on disk will NOT be deleted."
                ),
                Some(format!("{name}  ({repo_count} repo(s))")),
                format!("Delete Group & {repo_count} Repo(s)"),
            )
        } else {
            (
                "Delete Repository Group?".into(),
                "Deletes this empty group (and any empty subgroups inside it).".into(),
                Some(name.clone()),
                "Delete Group".into(),
            )
        };
        Self {
            title,
            message,
            detail,
            confirm_label,
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

    pub(crate) fn delete_vault_key(key_id: String, name: &str) -> Self {
        Self {
            title: format!("Delete Age Key '{name}'?"),
            message: "Removes this Age private key from your local keyring and clears any repo/group assignments pointing to it. Vaults encrypted with this key will become locked unless you backed up the private key!"
                .into(),
            detail: Some(name.to_string()),
            confirm_label: "Delete Key".into(),
            is_danger: true,
            action: ConfirmAction::DeleteVaultKey { key_id },
        }
    }

    pub(crate) fn destroy_repo_vault(repo_name: &str) -> Self {
        Self {
            title: format!("Delete Secrets Vault in '{repo_name}'?"),
            message: "Permanently deletes '.gitfleet/secrets.age' and '.gitfleet/secrets.json' from this repository so the vault and any Locked warning are completely removed. Your local plaintext files (.env / .toml / .yaml) on disk are NOT deleted."
                .into(),
            detail: Some(".gitfleet/secrets.age & .gitfleet/secrets.json".into()),
            confirm_label: "Delete Vault".into(),
            is_danger: true,
            action: ConfirmAction::DestroyRepoVault,
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
            action: ConfirmAction::ExecuteMerge {
                target,
                mode: MergeMode::MergeCommit,
            },
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

    pub(crate) fn disable_git_flow() -> Self {
        Self {
            title: "Remove Git Flow Configuration?".into(),
            message: "Removes gitflow.branch and gitflow.prefix settings from this repository's local .git/config. Your existing Git branches and commits will NOT be deleted."
                .into(),
            detail: Some("git config --local --remove-section gitflow.branch & gitflow.prefix".into()),
            confirm_label: "Disable Git Flow".into(),
            is_danger: true,
            action: ConfirmAction::UninitGitFlow,
        }
    }

    pub(crate) fn abort_git_flow_branch(
        branch: String,
        base_branch: String,
        unmerged: bool,
    ) -> Self {
        Self {
            title: if unmerged {
                format!("Force Cancel Unmerged Branch '{branch}'?")
            } else {
                format!("Cancel Git Flow Branch '{branch}'?")
            },
            message: if unmerged {
                format!(
                    "Branch '{branch}' has commits not merged into '{base_branch}'. Cancelling switches to '{base_branch}' (if checked out) and force-deletes '{branch}' (`git branch -D`) without merging."
                )
            } else {
                format!(
                    "Switches to '{base_branch}' (if currently checked out) and deletes branch '{branch}' without merging (`git branch -d`)."
                )
            },
            detail: Some(format!("{branch}  →  return to {base_branch}")),
            confirm_label: if unmerged {
                "Force Cancel (-D)".into()
            } else {
                "Cancel Branch".into()
            },
            is_danger: true,
            action: ConfirmAction::AbortGitFlowBranch {
                branch,
                base_branch,
                force: unmerged,
            },
        }
    }

    pub(crate) fn delete_vault_profile(profile: &str) -> Self {
        let fname = gitfleet_security::vault_file_name_for_profile(profile);
        Self {
            title: format!("Delete Secrets Profile '{profile}'?"),
            message: format!(
                "Deletes '.gitfleet/{fname}' and removes profile '{profile}' from '.gitfleet/secrets.json'. Other profiles and your local .env / .toml / .yaml files on disk will NOT be deleted."
            ),
            detail: Some(format!(".gitfleet/{fname}")),
            confirm_label: "Delete Profile".into(),
            is_danger: true,
            action: ConfirmAction::DeleteVaultProfile {
                profile: profile.to_string(),
            },
        }
    }

    pub(crate) fn delete_repo_entry(rel_path: String, is_dir: bool) -> Self {
        if is_dir {
            Self {
                title: "Delete Folder Permanently?".into(),
                message: "Permanently deletes this folder and all files inside it from disk. This action cannot be undone (Git-tracked files can still be restored from Git).".into(),
                detail: Some(format!("{rel_path}/")),
                confirm_label: "Delete Folder".into(),
                is_danger: true,
                action: ConfirmAction::DeleteRepoEntry { rel_path, is_dir: true },
            }
        } else {
            Self {
                title: "Delete File Permanently?".into(),
                message: "Permanently deletes this file from disk. This action cannot be undone (Git-tracked files can still be restored from Git).".into(),
                detail: Some(rel_path.clone()),
                confirm_label: "Delete File".into(),
                is_danger: true,
                action: ConfirmAction::DeleteRepoEntry { rel_path, is_dir: false },
            }
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
pub(crate) struct RenameBranchModalState {
    pub(crate) old_name: String,
    pub(crate) new_name: String,
}

#[derive(Debug, Clone)]
pub(crate) struct RenameWorkspaceModalState {
    /// Target workspace file path (`None` means the currently active workspace).
    pub(crate) path: Option<PathBuf>,
    pub(crate) old_name: String,
    pub(crate) new_name: String,
    pub(crate) file_hint: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct RenameGroupModalState {
    pub(crate) old_name: String,
    pub(crate) new_name: String,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommitModalScope {
    Staged,
    Local,
}

#[derive(Debug, Clone)]
pub(crate) struct CommitModalFileEntry {
    pub(crate) path: String,
    pub(crate) state_label: String,
    pub(crate) selected: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct CommitModalState {
    pub(crate) repo_id: RepoId,
    pub(crate) repo_name: String,
    pub(crate) repo_path: PathBuf,
    pub(crate) scope: CommitModalScope,
    pub(crate) staged_files: Vec<CommitModalFileEntry>,
    pub(crate) local_files: Vec<CommitModalFileEntry>,
    pub(crate) amend: bool,
    pub(crate) loading: bool,
}

impl CommitModalState {
    pub(crate) fn from_status(
        repo_id: RepoId,
        repo_name: String,
        repo_path: PathBuf,
        status: Option<&RepoStatusSummary>,
        loading: bool,
    ) -> Self {
        let mut staged_files = Vec::new();
        let mut local_files = Vec::new();
        if let Some(st) = status {
            for f in &st.staged_files {
                staged_files.push(CommitModalFileEntry {
                    path: f.path.clone(),
                    state_label: f.kind.state_label(false).to_string(),
                    selected: true,
                });
            }
            let mut seen = std::collections::HashSet::new();
            for f in st
                .unstaged_files
                .iter()
                .chain(st.staged_files.iter())
                .chain(st.conflict_files.iter())
            {
                if seen.insert(f.path.clone()) {
                    local_files.push(CommitModalFileEntry {
                        path: f.path.clone(),
                        state_label: f.kind.state_label(false).to_string(),
                        selected: true,
                    });
                }
            }
            local_files.sort_by(|a, b| a.path.cmp(&b.path));
            staged_files.sort_by(|a, b| a.path.cmp(&b.path));
        }

        // SmartGit rule: if nothing is staged yet, default to Local Changes with all files checked.
        let scope = if staged_files.is_empty() {
            CommitModalScope::Local
        } else {
            CommitModalScope::Staged
        };

        Self {
            repo_id,
            repo_name,
            repo_path,
            scope,
            staged_files,
            local_files,
            amend: false,
            loading,
        }
    }

    pub(crate) fn active_files(&self) -> &[CommitModalFileEntry] {
        match self.scope {
            CommitModalScope::Staged => &self.staged_files,
            CommitModalScope::Local => &self.local_files,
        }
    }

    pub(crate) fn active_files_mut(&mut self) -> &mut Vec<CommitModalFileEntry> {
        match self.scope {
            CommitModalScope::Staged => &mut self.staged_files,
            CommitModalScope::Local => &mut self.local_files,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextMenuSubmenu {
    None,
    MoveToGroup,
    RepoSecretKey,
    GroupSecretKey,
}

#[derive(Debug, Clone)]
pub(crate) struct ContextMenuState {
    pub(crate) position: Point,
    pub(crate) target: ContextMenuTarget,
    pub(crate) submenu: ContextMenuSubmenu,
}

#[derive(Debug, Clone)]
pub(crate) enum ContextMenuTarget {
    WorkspacePill,
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
    LocalBranchesGroup {
        local_names: Vec<String>,
    },
    LocalBranchFolder {
        folder_key: String,
        prefix: String,
    },
    ChangedFile {
        file: FileChange,
        is_working_tree: bool,
    },
    RepoFile {
        path: String,
    },
    RepoDir {
        path: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScanLevelPreset {
    All,
    MaxOne,
    MaxTwo,
    Flat,
}

#[derive(Debug, Clone)]
pub(crate) struct ScanModalRepoItem {
    pub(crate) candidate: ScannedRepoCandidate,
    pub(crate) selected: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ScanModalCounts {
    pub(crate) total_repos: usize,
    pub(crate) selected_new: usize,
    pub(crate) selected_regroup: usize,
    pub(crate) existing_movable: usize,
    pub(crate) existing_same: usize,
    pub(crate) actionable_total: usize,
    pub(crate) actionable_selected: usize,
    pub(crate) new_groups_count: usize,
    pub(crate) active_groups_count: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ScanModalState {
    pub(crate) root: PathBuf,
    pub(crate) max_depth: usize,
    pub(crate) scanning: bool,
    pub(crate) has_root_level: bool,
    pub(crate) levels: Vec<ScanLevelInfo>,
    pub(crate) enabled_levels: Vec<bool>,
    pub(crate) skipped_raw_nodes: HashSet<String>,
    pub(crate) target_parent_group: String,
    pub(crate) update_existing_groups: bool,
    pub(crate) repos: Vec<ScanModalRepoItem>,
}

impl ScanModalState {
    pub(crate) fn from_result(
        result: ScanFolderResult,
        target_parent_group: Option<String>,
    ) -> Self {
        let new_count = result
            .repos
            .iter()
            .filter(|r| !r.already_in_workspace)
            .count();
        let repos = result
            .repos
            .into_iter()
            .map(|candidate| ScanModalRepoItem {
                candidate,
                selected: true,
            })
            .collect();
        Self {
            root: result.root,
            max_depth: result.max_depth,
            scanning: false,
            has_root_level: result.has_root_level,
            levels: result.levels,
            enabled_levels: result.default_enabled_levels,
            skipped_raw_nodes: HashSet::new(),
            target_parent_group: target_parent_group
                .map(|g| normalize_group_path(&g))
                .unwrap_or_default(),
            // If all discovered repos are already in the workspace, default to regrouping them
            // so re-scanning an already-imported folder organizes existing repos in one click.
            update_existing_groups: new_count == 0,
            repos,
        }
    }

    pub(crate) fn normalized_parent_group(&self) -> Option<String> {
        let norm = normalize_group_path(&self.target_parent_group);
        if norm.is_empty() {
            None
        } else {
            Some(norm)
        }
    }

    pub(crate) fn resolved_group_for(&self, item: &ScanModalRepoItem) -> Option<String> {
        let parent = self.normalized_parent_group();
        resolve_scanned_repo_group(
            &item.candidate.raw_segments,
            &self.enabled_levels,
            &self.skipped_raw_nodes,
            parent.as_deref(),
        )
    }

    pub(crate) fn is_repo_actionable(&self, item: &ScanModalRepoItem) -> bool {
        if !item.candidate.already_in_workspace {
            true
        } else {
            self.update_existing_groups
                && item.candidate.existing_group != self.resolved_group_for(item)
        }
    }

    fn preset_levels(&self, preset: ScanLevelPreset) -> Vec<bool> {
        let len = self.enabled_levels.len();
        let mut next = vec![false; len];
        if len == 0 {
            return next;
        }
        let has_direct_child_repo = self
            .repos
            .iter()
            .any(|r| r.candidate.raw_segments.len() <= 1);
        match preset {
            ScanLevelPreset::All => next.fill(true),
            ScanLevelPreset::Flat => {}
            ScanLevelPreset::MaxOne => {
                if self.has_root_level && len > 1 {
                    if has_direct_child_repo {
                        next[0] = true;
                    } else {
                        next[1] = true;
                    }
                } else {
                    next[0] = true;
                }
            }
            ScanLevelPreset::MaxTwo => {
                if self.has_root_level && len > 2 {
                    if has_direct_child_repo {
                        next[0] = true;
                        next[1] = true;
                    } else {
                        next[1] = true;
                        next[2] = true;
                    }
                } else {
                    for slot in next.iter_mut().take(2) {
                        *slot = true;
                    }
                }
            }
        }
        next
    }

    pub(crate) fn apply_preset(&mut self, preset: ScanLevelPreset) {
        self.skipped_raw_nodes.clear();
        self.enabled_levels = self.preset_levels(preset);
    }

    pub(crate) fn is_preset_active(&self, preset: ScanLevelPreset) -> bool {
        self.skipped_raw_nodes.is_empty() && self.enabled_levels == self.preset_levels(preset)
    }

    pub(crate) fn counts(&self, existing_ws_groups: &[String]) -> ScanModalCounts {
        let existing_group_set: HashSet<String> = existing_ws_groups
            .iter()
            .map(|g| normalize_group_path(g))
            .filter(|g| !g.is_empty())
            .collect();

        let mut c = ScanModalCounts {
            total_repos: self.repos.len(),
            ..Default::default()
        };
        let mut used_groups: HashSet<String> = HashSet::new();
        let mut new_groups: HashSet<String> = HashSet::new();

        for item in &self.repos {
            let target = self.resolved_group_for(item);
            if item.candidate.already_in_workspace {
                if item.candidate.existing_group != target {
                    c.existing_movable += 1;
                    if self.update_existing_groups {
                        c.actionable_total += 1;
                        if item.selected {
                            c.actionable_selected += 1;
                            c.selected_regroup += 1;
                            Self::collect_group_ancestors(
                                target.as_deref(),
                                &existing_group_set,
                                &mut used_groups,
                                &mut new_groups,
                            );
                        }
                    }
                } else {
                    c.existing_same += 1;
                }
            } else {
                c.actionable_total += 1;
                if item.selected {
                    c.actionable_selected += 1;
                    c.selected_new += 1;
                    Self::collect_group_ancestors(
                        target.as_deref(),
                        &existing_group_set,
                        &mut used_groups,
                        &mut new_groups,
                    );
                }
            }
        }

        c.active_groups_count = used_groups.len();
        c.new_groups_count = new_groups.len();
        c
    }

    fn collect_group_ancestors(
        group: Option<&str>,
        existing_ws_groups: &HashSet<String>,
        used_groups: &mut HashSet<String>,
        new_groups: &mut HashSet<String>,
    ) {
        let Some(g) = group else {
            return;
        };
        let mut acc = String::new();
        for seg in g.split('/').filter(|s| !s.is_empty()) {
            if !acc.is_empty() {
                acc.push('/');
            }
            acc.push_str(seg);
            used_groups.insert(acc.clone());
            if !existing_ws_groups.contains(&acc) {
                new_groups.insert(acc.clone());
            }
        }
    }
}

#[cfg(test)]
mod scan_modal_tests {
    use super::*;
    use gitfleet_workspace::{ScanGroupSegment, ScannedRepoCandidate};

    fn sample_modal(already_existing: bool) -> ScanModalState {
        let mk_seg = |name: &str, raw: &str| ScanGroupSegment {
            name: name.to_string(),
            raw_path: raw.to_string(),
        };
        let res = ScanFolderResult {
            root: PathBuf::from("Work"),
            max_depth: 5,
            has_root_level: true,
            levels: vec![
                ScanLevelInfo {
                    depth: 0,
                    is_root_folder: true,
                    folder_names: vec!["Work".into()],
                },
                ScanLevelInfo {
                    depth: 1,
                    is_root_folder: false,
                    folder_names: vec!["backend".into(), "frontend".into()],
                },
                ScanLevelInfo {
                    depth: 2,
                    is_root_folder: false,
                    folder_names: vec!["services".into()],
                },
            ],
            default_enabled_levels: vec![false, true, true],
            repos: vec![
                ScannedRepoCandidate {
                    name: "auth".into(),
                    path: PathBuf::from("Work/backend/services/auth"),
                    relative_path: "backend/services/auth".into(),
                    raw_segments: vec![
                        mk_seg("Work", "Work"),
                        mk_seg("backend", "Work/backend"),
                        mk_seg("services", "Work/backend/services"),
                    ],
                    already_in_workspace: already_existing,
                    existing_group: None,
                },
                ScannedRepoCandidate {
                    name: "web".into(),
                    path: PathBuf::from("Work/frontend/web"),
                    relative_path: "frontend/web".into(),
                    raw_segments: vec![
                        mk_seg("Work", "Work"),
                        mk_seg("frontend", "Work/frontend"),
                    ],
                    already_in_workspace: false,
                    existing_group: None,
                },
            ],
        };
        ScanModalState::from_result(res, None)
    }

    #[test]
    fn scan_modal_presets_and_group_counts() {
        let mut modal = sample_modal(false);
        assert!(modal.is_preset_active(ScanLevelPreset::MaxTwo));
        let counts = modal.counts(&["frontend".to_string()]);
        assert_eq!(counts.selected_new, 2);
        assert_eq!(counts.active_groups_count, 3); // backend, backend/services, frontend
        assert_eq!(counts.new_groups_count, 2); // backend, backend/services

        // Switch to 1 Level -> drops "services", keeps "backend" and "frontend"
        modal.apply_preset(ScanLevelPreset::MaxOne);
        assert!(modal.is_preset_active(ScanLevelPreset::MaxOne));
        assert_eq!(
            modal.resolved_group_for(&modal.repos[0]),
            Some("backend".to_string())
        );
        assert_eq!(
            modal.resolved_group_for(&modal.repos[1]),
            Some("frontend".to_string())
        );

        // Switch to Flat -> no auto groups
        modal.apply_preset(ScanLevelPreset::Flat);
        assert!(modal.is_preset_active(ScanLevelPreset::Flat));
        assert_eq!(modal.resolved_group_for(&modal.repos[0]), None);
        assert_eq!(modal.counts(&[]).new_groups_count, 0);

        // Switch to All -> includes root "Work"
        modal.apply_preset(ScanLevelPreset::All);
        assert!(modal.is_preset_active(ScanLevelPreset::All));
        assert_eq!(
            modal.resolved_group_for(&modal.repos[0]),
            Some("Work/backend/services".to_string())
        );
    }

    #[test]
    fn scan_modal_tracks_existing_repo_regrouping_toggle() {
        let mut modal = sample_modal(true);
        // Mixed (1 existing + 1 new) -> update_existing_groups defaults to false
        assert!(!modal.update_existing_groups);
        let c1 = modal.counts(&[]);
        assert_eq!(c1.selected_new, 1);
        assert_eq!(c1.selected_regroup, 0);
        assert_eq!(c1.existing_movable, 1);

        modal.update_existing_groups = true;
        let c2 = modal.counts(&[]);
        assert_eq!(c2.selected_new, 1);
        assert_eq!(c2.selected_regroup, 1);
        assert_eq!(c2.actionable_selected, 2);
    }
}
