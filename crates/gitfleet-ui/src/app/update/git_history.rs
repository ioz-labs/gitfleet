use crate::app::{
    ActiveFocusPane, BottomLeftTab, BottomViewMode, CommitModalScope, CommitModalState,
    GitFleetApp, GraphSelection, GraphTab, Message, PendingConfirm, RenameBranchModalState,
};
use chrono::Utc;
use gitfleet_core::UndoEntry;
use gitfleet_git::{MergeMode, ResetMode};
use iced::Task;
use iced::widget::{scrollable, text_editor, text_input};
use std::time::{Duration, Instant};

impl GitFleetApp {
    pub(crate) fn try_update_git_history(&mut self, message: Message) -> Result<Task<Message>, Message> {
        let mut unhandled: Option<Message> = None;
        let task = (|| -> Task<Message> {
            match message {
            Message::SetBottomLeftTab(tab) => {
                self.bottom_left_tab = tab;
                if let Some(repo) = self.current_repo() {
                    return match tab {
                        BottomLeftTab::Branches => self.load_branches_task(repo),
                        BottomLeftTab::Stashes => self.load_stashes_task(repo),
                        BottomLeftTab::Tags => {
                            self.release_triggers =
                                self.service.git.read_release_triggers(&repo.path);
                            self.load_tags_task(repo)
                        }
                        BottomLeftTab::Submodules => self.load_submodules_task(repo),
                        BottomLeftTab::Worktrees => self.load_worktrees_task(repo),
                    };
                }
                Task::none()
            }
            Message::BranchesLoaded(id, res) => {
                if self.active_repo_key().as_ref() == Some(&id) {
                    self.branches = res.unwrap_or_default();
                }
                Task::none()
            }
            Message::StashesLoaded(id, res) => {
                if self.active_repo_key().as_ref() == Some(&id) {
                    self.stashes = res.unwrap_or_default();
                }
                Task::none()
            }
            Message::TagsLoaded(id, res) => {
                if self.active_repo_key().as_ref() == Some(&id) {
                    self.tags = res.unwrap_or_default();
                }
                Task::none()
            }
            Message::InspectStash(index) => {
                self.diff_is_working_file = false;
                if let Some(repo) = self.current_repo() {
                    let svc = self.service.clone();
                    let title = format!("stash@{{{index}}}");
                    return Task::perform(
                        async move {
                            let res = svc
                                .git
                                .stash_show(&repo.path, index)
                                .await
                                .map_err(|e| e.to_string());
                            (title, res)
                        },
                        |(t, r)| Message::DiffLoaded(t, r),
                    );
                }
                Task::none()
            }
            Message::ApplyStash(index) => {
                self.run_git_op("Apply Stash", move |svc, repo| async move {
                    svc.git.stash_apply(&repo.path, index).await
                })
            }
            Message::PopStash(index) => self.run_git_op("Pop Stash", move |svc, repo| async move {
                svc.git.stash_pop(&repo.path, index).await
            }),
            Message::DropStash(index) => {
                self.run_git_op("Drop Stash", move |svc, repo| async move {
                    svc.git.stash_drop(&repo.path, index).await
                })
            }
            Message::NewTagNameChanged(v) => {
                self.new_tag_name = v;
                Task::none()
            }
            Message::CreateTagSubmitted => {
                let name = self.new_tag_name.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                self.new_tag_name.clear();
                self.run_git_op("Create Tag", move |svc, repo| async move {
                    svc.git.create_tag(&repo.path, &name, None, None).await
                })
            }
            Message::CreateTagAtCommit(oid) => {
                let tag_name = format!("tag-{}", &oid[..oid.len().min(7)]);
                self.run_git_op("Create Tag", move |svc, repo| async move {
                    svc.git
                        .create_tag(&repo.path, &tag_name, Some(&oid), None)
                        .await
                })
            }
            Message::PushTag(name) => self.run_git_op("Push Tag", move |svc, repo| async move {
                svc.git.push_tags(&repo.path, "origin", &[name]).await
            }),
            Message::DeleteTag(name) => {
                self.run_git_op("Delete Tag", move |svc, repo| async move {
                    svc.git.delete_tag(&repo.path, &name, None).await
                })
            }
            Message::SetGraphTab(tab) => {
                self.graph_tab = tab;
                if tab == GraphTab::Reflog
                    && let Some(repo) = self.current_repo()
                {
                    return self.load_reflog_task(repo);
                }
                Task::none()
            }
            Message::ToggleGraphAllBranches => {
                if !self.graph_checked_branches.is_empty() {
                    self.graph_checked_branches.clear();
                    self.graph_all_branches = false;
                } else {
                    self.graph_all_branches = !self.graph_all_branches;
                }
                if let Some(repo) = self.current_repo() {
                    return self.load_log_task(repo);
                }
                Task::none()
            }
            Message::ToggleGraphBranch(name) => {
                if self.graph_checked_branches.is_empty() {
                    let head_ref = self
                        .branches
                        .iter()
                        .find(|b| !b.is_remote && b.is_head)
                        .map(|b| b.name.clone())
                        .unwrap_or_else(|| "HEAD".to_string());
                    if name != head_ref && name != "HEAD" {
                        self.graph_checked_branches.insert(head_ref);
                    }
                    self.graph_checked_branches.insert(name);
                } else if !self.graph_checked_branches.remove(&name) {
                    self.graph_checked_branches.insert(name);
                }
                if let Some(repo) = self.current_repo() {
                    return self.load_log_task(repo);
                }
                Task::none()
            }
            Message::ToggleGraphBranchGroup(names) => {
                let all_checked = !names.is_empty()
                    && names
                        .iter()
                        .all(|n| self.graph_checked_branches.contains(n));
                if all_checked {
                    for n in names {
                        self.graph_checked_branches.remove(&n);
                    }
                } else {
                    for n in names {
                        self.graph_checked_branches.insert(n);
                    }
                }
                if let Some(repo) = self.current_repo() {
                    return self.load_log_task(repo);
                }
                Task::none()
            }
            Message::ClearGraphCheckedBranches => {
                self.graph_checked_branches.clear();
                self.graph_all_branches = false;
                if let Some(repo) = self.current_repo() {
                    return self.load_log_task(repo);
                }
                Task::none()
            }
            Message::ToggleGraphOrder => {
                self.graph_topo_order = !self.graph_topo_order;
                if let Some(repo) = self.current_repo() {
                    return self.load_log_task(repo);
                }
                Task::none()
            }
            Message::ReflogLoaded(id, res) => {
                if self.active_repo_key().as_ref() == Some(&id) {
                    self.reflog = res.unwrap_or_default();
                }
                Task::none()
            }
            Message::ResetCommit(oid, mode) => {
                self.run_git_op("Reset", move |svc, repo| async move {
                    if let Ok(prev) = svc.git.head_oid(&repo.path).await {
                        svc.undo.record(UndoEntry {
                            id: format!("{}", Utc::now().timestamp_millis()),
                            repo_id: repo.id.clone(),
                            label: format!("Reset {mode:?} to {}", &oid[..oid.len().min(7)]),
                            previous_head: prev,
                            created_at: Utc::now(),
                        });
                    }
                    svc.git.reset_commit(&repo.path, &oid, mode).await
                })
            }
            Message::RevertCommit(oid) => {
                self.run_git_op("Revert", move |svc, repo| async move {
                    if let Ok(prev) = svc.git.head_oid(&repo.path).await {
                        svc.undo.record(UndoEntry {
                            id: format!("{}", Utc::now().timestamp_millis()),
                            repo_id: repo.id.clone(),
                            label: format!("Revert {}", &oid[..oid.len().min(7)]),
                            previous_head: prev,
                            created_at: Utc::now(),
                        });
                    }
                    svc.git.revert_commit(&repo.path, &oid).await
                })
            }
            Message::UndoLastAction => {
                let Some(repo) = self.current_repo() else {
                    return Task::none();
                };
                let Some(entry) = self.service.undo.pop(&repo.id) else {
                    self.toast_is_error = false;
                    self.toast_message = "Nothing to undo for this repository".into();
                    return Task::none();
                };
                let prev_head = entry.previous_head;
                self.run_git_op("Undo", move |svc, r| async move {
                    svc.git
                        .reset_commit(&r.path, &prev_head, ResetMode::Mixed)
                        .await
                })
            }
            Message::ToggleMergeModal => {
                if self.selected_repo.is_none() {
                    return Task::none();
                }
                self.show_palette = false;
                self.show_branch_popover = false;
                self.show_git_flow_modal = false;
                self.show_merge_modal = !self.show_merge_modal;
                self.merge_target_branch = None;
                self.merge_forecast = None;
                Task::none()
            }
            Message::SelectMergeTarget(branch) => {
                self.merge_target_branch = Some(branch.clone());
                self.merge_forecast = None;
                if let Some(repo) = self.current_repo() {
                    let svc = self.service.clone();
                    return Task::perform(
                        async move {
                            let res = svc
                                .git
                                .merge_forecast(&repo.path, &branch)
                                .await
                                .map_err(|e| e.to_string());
                            (branch, res)
                        },
                        |(b, r)| Message::MergeForecastLoaded(b, r),
                    );
                }
                Task::none()
            }
            Message::MergeForecastLoaded(branch, res) => {
                if self.merge_target_branch.as_deref() == Some(branch.as_str())
                    && let Ok(fc) = res
                {
                    self.merge_forecast = Some(fc);
                }
                Task::none()
            }
            Message::ExecuteMerge(branch, mode) => {
                self.show_merge_modal = false;
                let op_label = match mode {
                    MergeMode::MergeCommit => "Merge",
                    MergeMode::WorkingTree => "Merge to Working Tree",
                    MergeMode::Squash => "Squash-Merge",
                };
                self.run_git_op(op_label, move |svc, repo| async move {
                    if let Ok(prev) = svc.git.head_oid(&repo.path).await {
                        svc.undo.record(UndoEntry {
                            id: format!("{}", Utc::now().timestamp_millis()),
                            repo_id: repo.id.clone(),
                            label: format!("{op_label} {branch}"),
                            previous_head: prev,
                            created_at: Utc::now(),
                        });
                    }
                    svc.git.merge_branch_with_mode(&repo.path, &branch, mode).await
                })
            }
            Message::ExecuteRebase(branch) => {
                self.show_merge_modal = false;
                self.run_git_op("Rebase", move |svc, repo| async move {
                    if let Ok(prev) = svc.git.head_oid(&repo.path).await {
                        svc.undo.record(UndoEntry {
                            id: format!("{}", Utc::now().timestamp_millis()),
                            repo_id: repo.id.clone(),
                            label: format!("Rebase onto {branch}"),
                            previous_head: prev,
                            created_at: Utc::now(),
                        });
                    }
                    svc.git.rebase_branch(&repo.path, &branch).await
                })
            }
            Message::OpenRenameBranchModal(target) => {
                if self.selected_repo.is_none() || self.has_modal_or_menu_open() && target.is_none() {
                    return Task::none();
                }
                let branch_name = target
                    .or_else(|| {
                        self.selected_branch.as_ref().and_then(|sel| {
                            self.branches
                                .iter()
                                .find(|b| !b.is_remote && &b.name == sel)
                                .map(|b| b.name.clone())
                        })
                    })
                    .or_else(|| {
                        self.branches
                            .iter()
                            .find(|b| !b.is_remote && b.is_head)
                            .map(|b| b.name.clone())
                    });
                let Some(old_name) = branch_name else {
                    return Task::none();
                };
                self.context_menu = None;
                self.show_palette = false;
                self.show_branch_popover = false;
                self.rename_branch_modal = Some(RenameBranchModalState {
                    new_name: old_name.clone(),
                    old_name,
                });
                text_input::focus(self.rename_branch_input_id.clone())
            }
            Message::RenameBranchInputChanged(val) => {
                if let Some(modal) = &mut self.rename_branch_modal {
                    modal.new_name = val;
                }
                Task::none()
            }
            Message::SubmitRenameBranch => {
                let Some(modal) = self.rename_branch_modal.take() else {
                    return Task::none();
                };
                let new_name = modal.new_name.trim().to_string();
                if new_name.is_empty() || new_name == modal.old_name {
                    return Task::none();
                }
                let old_name = modal.old_name;
                if self.selected_branch.as_deref() == Some(old_name.as_str()) {
                    self.selected_branch = Some(new_name.clone());
                }
                if self.graph_checked_branches.remove(&old_name) {
                    self.graph_checked_branches.insert(new_name.clone());
                }
                self.run_git_op("Rename Branch", move |svc, repo| async move {
                    svc.git.rename_branch(&repo.path, &old_name, &new_name).await
                })
            }
            Message::ShortcutMergeSelectedBranch => {
                if self.selected_repo.is_none() || self.has_modal_or_menu_open() {
                    return Task::none();
                }
                let target = self.selected_branch.as_ref().and_then(|sel| {
                    self.branches
                        .iter()
                        .find(|b| !b.is_head && &b.name == sel)
                        .map(|b| b.name.clone())
                });
                if let Some(branch) = target {
                    self.update(Message::OpenMergeWithTarget(branch))
                } else {
                    self.update(Message::ToggleMergeModal)
                }
            }
            Message::ShortcutRebaseSelectedBranch => {
                if self.selected_repo.is_none() || self.has_modal_or_menu_open() {
                    return Task::none();
                }
                let target = self.selected_branch.as_ref().and_then(|sel| {
                    self.branches
                        .iter()
                        .find(|b| !b.is_head && &b.name == sel)
                        .map(|b| b.name.clone())
                });
                if let Some(branch) = target {
                    self.update(Message::OpenMergeWithTarget(branch))
                } else {
                    self.update(Message::ToggleMergeModal)
                }
            }
            Message::ResolveConflict(path, strategy) => {
                if self.conflict_file_path.as_deref() == Some(path.as_str()) {
                    self.clear_conflict_state();
                    if self.bottom_mode == BottomViewMode::ResolveConflicts {
                        self.bottom_mode = BottomViewMode::SideBySideDiff;
                    }
                }
                self.run_git_op("Resolve Conflict", move |svc, repo| async move {
                    svc.git.resolve_conflict(&repo.path, &path, strategy).await
                })
            }
            Message::OpenConflictSolver(rel_path) => {
                self.active_file_path = Some(rel_path.clone());
                self.diff_title = format!("Resolve Conflicts · {rel_path}");
                self.load_conflict_state_for_file(&rel_path);
                self.bottom_mode = BottomViewMode::ResolveConflicts;
                Task::none()
            }
            Message::ConflictSetHunkChoice(hunk_id, choice) => {
                if self.conflict_editing_hunk == Some(hunk_id) {
                    self.conflict_editing_hunk = None;
                }
                self.conflict_parsed.set_choice(hunk_id, choice);
                self.conflict_active_hunk = Some(hunk_id);
                Task::none()
            }
            Message::ConflictToggleCustomEdit(hunk_id) => {
                if self.conflict_editing_hunk == Some(hunk_id) {
                    let edited = self.conflict_hunk_editor.text();
                    self.conflict_parsed.set_custom_text(hunk_id, edited);
                    self.conflict_editing_hunk = None;
                    self.focus_pane = ActiveFocusPane::Other;
                } else {
                    if let Some(prev_id) = self.conflict_editing_hunk {
                        let edited = self.conflict_hunk_editor.text();
                        self.conflict_parsed.set_custom_text(prev_id, edited);
                    }
                    let seed = self
                        .conflict_parsed
                        .hunk(hunk_id)
                        .map(|h| h.seed_text_for_editor())
                        .unwrap_or_default();
                    self.conflict_hunk_editor = text_editor::Content::with_text(&seed);
                    self.conflict_parsed.set_custom_text(hunk_id, seed);
                    self.conflict_editing_hunk = Some(hunk_id);
                    self.conflict_active_hunk = Some(hunk_id);
                    self.focus_pane = ActiveFocusPane::TextInput;
                }
                Task::none()
            }
            Message::ConflictCustomEditAction(action) => {
                self.focus_pane = ActiveFocusPane::TextInput;
                self.conflict_hunk_editor.perform(action);
                if let Some(hunk_id) = self.conflict_editing_hunk {
                    let edited = self.conflict_hunk_editor.text();
                    self.conflict_parsed.set_custom_text(hunk_id, edited);
                }
                Task::none()
            }
            Message::ConflictResolveAll(choice) => {
                self.conflict_editing_hunk = None;
                self.conflict_parsed.resolve_all(choice);
                Task::none()
            }
            Message::ConflictJump(delta) => {
                let total = self.conflict_parsed.conflict_count();
                if total == 0 {
                    return Task::none();
                }
                let unresolved: Vec<usize> = self
                    .conflict_parsed
                    .segments
                    .iter()
                    .filter_map(|s| match s {
                        gitfleet_git::ConflictSegment::Conflict(h) if !h.choice.is_resolved() => {
                            Some(h.id)
                        }
                        _ => None,
                    })
                    .collect();
                let candidates: Vec<usize> = if !unresolved.is_empty() {
                    unresolved
                } else {
                    (0..total).collect()
                };
                let cur = self.conflict_active_hunk.unwrap_or(0);
                let next_id = if delta >= 0 {
                    candidates
                        .iter()
                        .copied()
                        .find(|&id| id > cur)
                        .unwrap_or(candidates[0])
                } else {
                    candidates
                        .iter()
                        .rev()
                        .copied()
                        .find(|&id| id < cur)
                        .unwrap_or(*candidates.last().unwrap_or(&0))
                };
                self.conflict_active_hunk = Some(next_id);
                let rel_y = if total <= 1 {
                    0.0
                } else {
                    (next_id as f32 / (total - 1) as f32).clamp(0.0, 1.0)
                };
                scrollable::snap_to(
                    self.conflict_scroll_id.clone(),
                    scrollable::RelativeOffset { x: 0.0, y: rel_y },
                )
            }
            Message::ConflictToggleContextSegment(seg_idx) => {
                if !self.conflict_expanded_segments.remove(&seg_idx) {
                    self.conflict_expanded_segments.insert(seg_idx);
                }
                Task::none()
            }
            Message::SaveResolvedConflict { mark_resolved } => {
                let Some(rel_path) = self.conflict_file_path.clone() else {
                    return Task::none();
                };
                if let Some(hunk_id) = self.conflict_editing_hunk.take() {
                    let edited = self.conflict_hunk_editor.text();
                    self.conflict_parsed.set_custom_text(hunk_id, edited);
                }
                let remaining = self.conflict_parsed.unresolved_count();
                if mark_resolved && remaining > 0 {
                    self.toast_is_error = true;
                    self.toast_message = format!(
                        "{remaining} conflict(s) still unresolved — resolve all hunks before marking resolved"
                    );
                    return Task::none();
                }
                let rendered = self.conflict_parsed.render_output();
                let label = if mark_resolved {
                    "Save & Mark Resolved"
                } else {
                    "Save Conflict Draft"
                };
                if mark_resolved {
                    self.clear_conflict_state();
                    self.bottom_mode = BottomViewMode::SideBySideDiff;
                }
                self.run_git_op(label, move |svc, repo| async move {
                    svc.git
                        .save_resolved_conflict(&repo.path, &rel_path, &rendered, mark_resolved)
                        .await
                })
            }
            Message::ContinueGitOp => {
                self.run_git_op("Continue", move |svc, repo| async move {
                    svc.git.continue_operation(&repo.path).await
                })
            }
            Message::AbortGitOp => self.run_git_op("Abort", move |svc, repo| async move {
                svc.git.abort_operation(&repo.path).await
            }),
            Message::ToggleGitFlowModal => {
                if self.selected_repo.is_none() {
                    return Task::none();
                }
                self.show_palette = false;
                self.show_branch_popover = false;
                self.show_merge_modal = false;
                self.show_git_flow_modal = !self.show_git_flow_modal;
                if self.show_git_flow_modal
                    && let Some(repo) = self.current_repo()
                {
                    let svc = self.service.clone();
                    return Task::perform(
                        async move {
                            svc.git
                                .get_git_flow_state(&repo.path)
                                .await
                                .map_err(|e| e.to_string())
                        },
                        Message::GitFlowStateLoaded,
                    );
                }
                Task::none()
            }
            Message::GitFlowStateLoaded(res) => {
                if let Ok(st) = res {
                    if let Some(m) = &st.main_branch {
                        self.git_flow_main_input = m.clone();
                    }
                    if let Some(d) = &st.develop_branch {
                        self.git_flow_develop_input = d.clone();
                    }
                    self.git_flow_state = st;
                }
                Task::none()
            }
            Message::GitFlowMainChanged(v) => {
                self.git_flow_main_input = v;
                Task::none()
            }
            Message::GitFlowDevelopChanged(v) => {
                self.git_flow_develop_input = v;
                Task::none()
            }
            Message::GitFlowNameChanged(v) => {
                self.git_flow_name_input = v;
                Task::none()
            }
            Message::GitFlowKindChanged(k) => {
                self.git_flow_kind = k;
                Task::none()
            }
            Message::InitGitFlow => {
                let m = self.git_flow_main_input.trim().to_string();
                let d = self.git_flow_develop_input.trim().to_string();
                if m.is_empty() || d.is_empty() {
                    return Task::none();
                }
                self.show_git_flow_modal = false;
                self.run_git_op("Init Git Flow", move |svc, repo| async move {
                    svc.git.init_git_flow(&repo.path, &m, &d).await
                })
            }
            Message::StartGitFlow => {
                let name = self.git_flow_name_input.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                let kind = self.git_flow_kind;
                self.git_flow_name_input.clear();
                self.show_git_flow_modal = false;
                self.run_git_op("Start Git Flow", move |svc, repo| async move {
                    svc.git.start_git_flow(&repo.path, kind, &name).await
                })
            }
            Message::FinishGitFlow => {
                let name = self.git_flow_name_input.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                let kind = self.git_flow_kind;
                self.git_flow_name_input.clear();
                self.show_git_flow_modal = false;
                self.run_git_op("Finish Git Flow", move |svc, repo| async move {
                    svc.git.finish_git_flow(&repo.path, kind, &name).await
                })
            }
            Message::BranchClicked(branch) => {
                const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(450);
                let now = Instant::now();
                let is_double = self.last_branch_click.as_ref().is_some_and(|(prev, t)| {
                    prev == &branch && now.duration_since(*t) <= DOUBLE_CLICK_WINDOW
                });
                self.selected_branch = Some(branch.clone());
                if is_double {
                    self.last_branch_click = None;
                    let is_already_head = self
                        .branches
                        .iter()
                        .any(|b| !b.is_remote && b.name == branch && b.is_head);
                    if is_already_head {
                        return Task::none();
                    }
                    let linked_wt_path = self
                        .worktrees
                        .iter()
                        .find(|w| !w.is_main && w.branch.as_deref() == Some(branch.as_str()))
                        .map(|w| w.path.display().to_string())
                        .or_else(|| {
                            self.branches
                                .iter()
                                .find(|b| !b.is_remote && !b.is_head && b.name == branch)
                                .and_then(|b| b.worktree_path.clone())
                        });
                    if let Some(wt_path) = linked_wt_path {
                        self.toast_is_error = false;
                        self.toast_message = format!(
                            "Branch '{branch}' is already checked out in worktree: {wt_path}"
                        );
                        return Task::none();
                    }
                    return self.update(Message::CheckoutBranch(branch));
                }
                self.last_branch_click = Some((branch, now));
                Task::none()
            }
            Message::CheckoutBranch(branch) => {
                self.show_branch_popover = false;
                self.selected_branch = Some(branch.clone());
                self.run_git_op("Checkout", move |svc, repo| async move {
                    svc.git.checkout(&repo.path, &branch).await
                })
            }
            Message::CheckoutRemoteBranch(remote_ref) => {
                self.show_branch_popover = false;
                self.run_git_op("Checkout Remote", move |svc, repo| async move {
                    svc.git.checkout_remote_branch(&repo.path, &remote_ref).await
                })
            }
            Message::CheckoutCommit(oid) => {
                self.run_git_op("Checkout Commit", move |svc, repo| async move {
                    svc.git.checkout_detached(&repo.path, &oid).await
                })
            }
            Message::FastForwardMergeBranch(branch) => {
                self.run_git_op("Fast-Forward Merge", move |svc, repo| async move {
                    if let Ok(prev) = svc.git.head_oid(&repo.path).await {
                        svc.undo.record(UndoEntry {
                            id: format!("{}", Utc::now().timestamp_millis()),
                            repo_id: repo.id.clone(),
                            label: format!("Fast-forward merge {branch}"),
                            previous_head: prev,
                            created_at: Utc::now(),
                        });
                    }
                    svc.git.merge_ff_only(&repo.path, &branch).await
                })
            }
            Message::PushBranch(branch) => {
                self.run_git_op("Push Branch", move |svc, repo| async move {
                    svc.git.push_branch(&repo.path, &branch).await
                })
            }
            Message::UnsetBranchUpstream(branch) => {
                self.run_git_op("Stop Tracking", move |svc, repo| async move {
                    svc.git.unset_upstream(&repo.path, &branch).await
                })
            }
            Message::CreateBranchSubmitted => {
                let name = self.branch_filter_query.trim().to_string();
                if name.is_empty() {
                    return Task::none();
                }
                let base_ref = self.branch_create_base.take();
                let checkout = self.branch_create_checkout;
                self.branch_filter_query.clear();
                self.show_branch_popover = false;
                self.run_git_op("Create Branch", move |svc, repo| async move {
                    svc.git
                        .create_branch_from(&repo.path, &name, base_ref.as_deref(), checkout)
                        .await
                })
            }
            Message::RequestDeleteBranches(names) => {
                if names.is_empty() {
                    return Task::none();
                }
                let Some(repo) = self.current_repo() else {
                    return Task::none();
                };
                self.context_menu = None;
                let svc = self.service.clone();
                Task::perform(
                    async move {
                        let unmerged = svc
                            .git
                            .unmerged_branches(&repo.path, &names)
                            .await
                            .unwrap_or_default();
                        (names, unmerged)
                    },
                    |(names, unmerged)| Message::DeleteBranchesChecked { names, unmerged },
                )
            }
            Message::DeleteBranchesChecked { names, unmerged } => {
                if unmerged.is_empty() {
                    self.pending_confirm = Some(PendingConfirm::delete_branches(names, false));
                } else {
                    self.pending_confirm =
                        Some(PendingConfirm::delete_unmerged_branches(names, unmerged));
                }
                Task::none()
            }
            Message::DeleteBranches(names, force) => {
                if names.is_empty() {
                    return Task::none();
                }
                let Some(repo) = self.current_repo() else {
                    return Task::none();
                };
                for n in &names {
                    self.graph_checked_branches.remove(n);
                }
                self.busy_label = Some("Delete Branch…".into());
                let svc = self.service.clone();
                Task::perform(
                    async move {
                        let result = svc
                            .git
                            .delete_branches(&repo.path, &names, force)
                            .await
                            .map_err(|e| e.to_string());
                        (names, force, result)
                    },
                    |(names, force, result)| Message::DeleteBranchesDone {
                        names,
                        force,
                        result,
                    },
                )
            }
            Message::DeleteBranchesDone {
                names,
                force,
                result,
            } => {
                self.busy_label = None;
                match result {
                    Ok(_) => {
                        self.toast_is_error = false;
                        self.toast_message = if names.len() == 1 {
                            format!("Deleted branch '{}'", names[0])
                        } else {
                            format!("Deleted {} branch(es)", names.len())
                        };
                        self.update(Message::RefreshSelected)
                    }
                    Err(e) => {
                        if !force && e.to_lowercase().contains("not fully merged") {
                            self.pending_confirm = Some(PendingConfirm::delete_unmerged_branches(
                                names.clone(),
                                names,
                            ));
                            return Task::none();
                        }
                        self.toast_is_error = true;
                        self.toast_message = format!("Delete Branch failed: {e}");
                        self.update(Message::RefreshSelected)
                    }
                }
            }
            Message::OpenCommitModal(repo_id) => {
                let ws = self.service.workspace.current();
                let Some(repo) = ws.repos.into_iter().find(|r| r.id == repo_id) else {
                    return Task::none();
                };
                self.context_menu = None;
                self.show_palette = false;
                let was_same_repo = self.selected_repo.as_ref() == Some(&repo_id);
                let select_task = if !was_same_repo {
                    self.update(Message::SelectRepo(repo_id.clone()))
                } else {
                    Task::none()
                };
                let cached = self.statuses.get(&repo_id);
                let state = CommitModalState::from_status(
                    repo.id.clone(),
                    repo.name.clone(),
                    repo.path.clone(),
                    cached,
                    true,
                );
                let seed_msg = if was_same_repo {
                    self.commit_editor.text()
                } else {
                    String::new()
                };
                self.commit_modal_editor = text_editor::Content::with_text(&seed_msg);
                self.commit_modal = Some(state);
                self.focus_pane = ActiveFocusPane::TextInput;

                let svc = self.service.clone();
                let fetch_status = Task::perform(
                    async move {
                        let res = svc
                            .git
                            .get_status(&repo.path)
                            .await
                            .map_err(|e| e.to_string());
                        (repo.id, res)
                    },
                    |(id, res)| Message::CommitModalStatusLoaded(id, res),
                );
                Task::batch([select_task, fetch_status])
            }
            Message::CommitModalStatusLoaded(repo_id, res) => {
                if let Ok(st) = res {
                    self.statuses.insert(repo_id.clone(), st.clone());
                    if let Some(modal) = &mut self.commit_modal
                        && modal.repo_id == repo_id
                    {
                        let amend = modal.amend;
                        let refreshed = CommitModalState::from_status(
                            modal.repo_id.clone(),
                            modal.repo_name.clone(),
                            modal.repo_path.clone(),
                            Some(&st),
                            false,
                        );
                        *modal = refreshed;
                        modal.amend = amend;
                    }
                } else if let Some(modal) = &mut self.commit_modal
                    && modal.repo_id == repo_id
                {
                    modal.loading = false;
                }
                Task::none()
            }
            Message::CommitModalSetScope(scope) => {
                if let Some(modal) = &mut self.commit_modal {
                    modal.scope = scope;
                }
                Task::none()
            }
            Message::CommitModalToggleFile(idx, checked) => {
                if let Some(modal) = &mut self.commit_modal
                    && let Some(entry) = modal.active_files_mut().get_mut(idx)
                {
                    entry.selected = checked;
                }
                Task::none()
            }
            Message::CommitModalToggleAll(checked) => {
                if let Some(modal) = &mut self.commit_modal {
                    for entry in modal.active_files_mut().iter_mut() {
                        entry.selected = checked;
                    }
                }
                Task::none()
            }
            Message::CommitModalMessageAction(action) => {
                self.focus_pane = ActiveFocusPane::TextInput;
                self.commit_modal_editor.perform(action);
                Task::none()
            }
            Message::CommitModalAmendToggled(checked) => {
                let mut repo_path = None;
                if let Some(modal) = &mut self.commit_modal {
                    modal.amend = checked;
                    if checked && self.commit_modal_editor.text().trim().is_empty() {
                        repo_path = Some(modal.repo_path.clone());
                    }
                }
                if let Some(path) = repo_path {
                    let svc = self.service.clone();
                    return Task::perform(
                        async move { svc.git.amend_target(&path).await.map_err(|e| e.to_string()) },
                        Message::CommitModalAmendTargetLoaded,
                    );
                }
                Task::none()
            }
            Message::CommitModalAmendTargetLoaded(res) => {
                if let Ok(target) = res
                    && self.commit_modal.as_ref().is_some_and(|m| m.amend)
                    && self.commit_modal_editor.text().trim().is_empty()
                {
                    self.commit_modal_editor = text_editor::Content::with_text(&target.message);
                }
                Task::none()
            }
            Message::SubmitCommitModal { push_after } => {
                let raw = self.commit_modal_editor.text();
                let msg = raw.trim().replace("\r\n", "\n");
                if msg.is_empty() {
                    self.toast_is_error = true;
                    self.toast_message = "Commit message cannot be empty".into();
                    return Task::none();
                }
                let Some(modal) = self.commit_modal.take() else {
                    return Task::none();
                };
                self.commit_modal_editor = text_editor::Content::new();
                if self.selected_repo.as_ref() == Some(&modal.repo_id) {
                    self.commit_editor = text_editor::Content::new();
                    self.amend_commit = false;
                }

                let mut stage_paths = Vec::new();
                let mut unstage_paths = Vec::new();
                match modal.scope {
                    CommitModalScope::Staged => {
                        for f in &modal.staged_files {
                            if !f.selected {
                                unstage_paths.push(f.path.clone());
                            }
                        }
                    }
                    CommitModalScope::Local => {
                        let staged_set: std::collections::HashSet<&str> = modal
                            .staged_files
                            .iter()
                            .map(|f| f.path.as_str())
                            .collect();
                        for f in &modal.local_files {
                            if f.selected {
                                stage_paths.push(f.path.clone());
                            } else if staged_set.contains(f.path.as_str()) {
                                unstage_paths.push(f.path.clone());
                            }
                        }
                    }
                }

                let repo_path = modal.repo_path;
                let amend = modal.amend;
                let label = if push_after {
                    "Commit & Push"
                } else {
                    "Commit"
                };
                self.busy_label = Some(format!("{label}…"));
                let svc = self.service.clone();
                Task::perform(
                    async move {
                        let res = svc
                            .git
                            .commit_with_selection(
                                &repo_path,
                                &stage_paths,
                                &unstage_paths,
                                &msg,
                                amend,
                                push_after,
                            )
                            .await
                            .map_err(|e| e.to_string());
                        (label.to_string(), res)
                    },
                    |(l, r)| Message::GitActionDone(l, r),
                )
            }
            Message::CompareRefs(base_ref, target_ref) => {
                self.diff_is_working_file = false;
                if matches!(self.bottom_mode, BottomViewMode::Preview | BottomViewMode::Edit) {
                    self.bottom_mode = BottomViewMode::SideBySideDiff;
                }
                if let Some(repo) = self.current_repo() {
                    let svc = self.service.clone();
                    let title = format!("Compare {base_ref} ↔ {target_ref}");
                    return Task::perform(
                        async move {
                            let res = svc
                                .git
                                .diff_refs(&repo.path, &base_ref, &target_ref)
                                .await
                                .map_err(|e| e.to_string());
                            (title, res)
                        },
                        |(t, r)| Message::DiffLoaded(t, r),
                    );
                }
                Task::none()
            }
            Message::RevealBranchCommit(branch) => {
                if let Some(repo) = self.current_repo() {
                    let mut tasks = Vec::new();
                    if !self.graph_checked_branches.is_empty()
                        && self.graph_checked_branches.insert(branch.clone())
                    {
                        tasks.push(self.load_log_task(repo.clone()));
                    }
                    let svc = self.service.clone();
                    tasks.push(Task::perform(
                        async move {
                            svc.git
                                .rev_parse(&repo.path, &branch)
                                .await
                                .map_err(|e| e.to_string())
                        },
                        Message::BranchCommitResolved,
                    ));
                    return Task::batch(tasks);
                }
                Task::none()
            }
            Message::BranchCommitResolved(res) => match res {
                Ok(oid) => self.update(Message::SelectCommit(oid)),
                Err(e) => {
                    self.toast_is_error = true;
                    self.toast_message = format!("Could not resolve branch commit: {e}");
                    Task::none()
                }
            },
            Message::UseCommitMessage(oid) => {
                if let Some(c) = self.commits.iter().find(|c| c.oid == oid) {
                    let full = if c.body.trim().is_empty() {
                        c.summary.clone()
                    } else {
                        format!("{}\n\n{}", c.summary, c.body)
                    };
                    self.commit_editor = text_editor::Content::with_text(&full);
                    self.graph_selection = GraphSelection::WorkingTree;
                    self.toast_is_error = false;
                    self.toast_message =
                        format!("Loaded message from {} into Commit box", c.short_oid);
                }
                Task::none()
            }
                other => {
                    unhandled = Some(other);
                    Task::none()
                }
            }
        })();
        match unhandled {
            Some(msg) => Err(msg),
            None => Ok(task),
        }
    }
}
