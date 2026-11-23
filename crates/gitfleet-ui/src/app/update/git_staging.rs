use crate::app::{ActiveFocusPane, GitFleetApp, GraphSelection, GraphTab, Message, PendingConfirm};
use crate::views::diff_viewer::build_selected_lines_patch;
use crate::views::overlays::stash_pull_modal::{
    ApplyStashModalState, PullSyncModalState, SaveStashModalState,
};
use chrono::Utc;
use gitfleet_core::UndoEntry;
use gitfleet_git::PullStrategy;
use iced::Task;

impl GitFleetApp {
    pub(crate) fn try_update_git_staging(&mut self, message: Message) -> Result<Task<Message>, Message> {
        let mut unhandled: Option<Message> = None;
        let task = (|| -> Task<Message> {
            match message {
            Message::RefreshSelected => {
                if let Some(repo) = self.current_repo() {
                    self.busy_label = Some("Refreshing…".into());
                    self.release_triggers = self.service.git.read_release_triggers(&repo.path);
                    self.refresh_vault_summary();
                    let mut tasks = vec![
                        self.load_status_task(repo.clone()),
                        self.load_log_task(repo.clone()),
                        self.load_branches_task(repo.clone()),
                        self.load_stashes_task(repo.clone()),
                        self.load_tags_task(repo.clone()),
                        self.load_submodules_task(repo.clone()),
                        self.load_worktrees_task(repo.clone()),
                        self.load_all_files_task(repo.clone()),
                    ];
                    if self.show_ignored_files {
                        self.ignored_entries_stale = false;
                        tasks.push(self.load_ignored_entries_task(repo.clone()));
                    }
                    if self.graph_tab == GraphTab::Reflog {
                        tasks.push(self.load_reflog_task(repo.clone()));
                    }
                    if self.show_git_flow_modal {
                        let svc = self.service.clone();
                        let rpath = repo.path.clone();
                        tasks.push(Task::perform(
                            async move {
                                svc.git
                                    .get_git_flow_state(&rpath)
                                    .await
                                    .map_err(|e| e.to_string())
                            },
                            Message::GitFlowStateLoaded,
                        ));
                    }
                    if let Some((parent_id, _)) = &self.active_submodule
                        && let Some(parent) = self
                            .service
                            .workspace
                            .current()
                            .repos
                            .into_iter()
                            .find(|r| &r.id == parent_id)
                    {
                        tasks.push(self.load_status_task(parent.clone()));
                        tasks.push(self.load_submodules_task(parent));
                    }
                    return Task::batch(tasks);
                }
                Task::none()
            }
            Message::RefreshAll => {
                let repos = self.service.workspace.current().repos;
                let count = repos.len();
                let mut tasks = Vec::with_capacity(count * 2);
                for r in repos {
                    tasks.push(self.load_status_task(r.clone()));
                    tasks.push(self.load_submodules_task(r));
                }
                self.toast_is_error = false;
                self.toast_message = format!("Refreshing {count} repositories…");
                Task::batch(tasks)
            }
            Message::StatusLoaded(id, res) => {
                self.busy_label = None;
                match res {
                    Ok(summary) => {
                        let has_staged = summary.staged_count > 0;
                        let is_selected = self.active_repo_key().as_ref() == Some(&id);
                        let refs_or_tracking_changed = self.statuses.get(&id).is_some_and(|old| {
                            old.head_oid != summary.head_oid
                                || old.branch != summary.branch
                                || old.upstream != summary.upstream
                                || old.detached != summary.detached
                                || old.ahead != summary.ahead
                                || old.behind != summary.behind
                                || old.in_merge != summary.in_merge
                                || old.in_rebase != summary.in_rebase
                        });
                        let first_conflict = summary.conflict_files.first().cloned();
                        self.statuses.insert(id, summary);
                        if is_selected {
                            self.prune_file_selection();
                            let mut follow_up = Vec::new();
                            if self.auto_open_first_conflict {
                                self.auto_open_first_conflict = false;
                                if let Some(cf) = first_conflict {
                                    self.graph_selection = GraphSelection::WorkingTree;
                                    self.selected_changed_file = Some(cf.clone());
                                    follow_up.push(self.update(Message::OpenConflictSolver(cf.path)));
                                } else {
                                    follow_up.push(self.sync_active_working_file_after_status());
                                }
                            } else {
                                follow_up.push(self.sync_active_working_file_after_status());
                            }
                            if let Some(repo) = self.current_repo() {
                                if refs_or_tracking_changed {
                                    follow_up.push(self.load_log_task(repo.clone()));
                                    follow_up.push(self.load_branches_task(repo.clone()));
                                    follow_up.push(self.load_tags_task(repo.clone()));
                                    follow_up.push(self.load_stashes_task(repo.clone()));
                                    follow_up.push(self.load_worktrees_task(repo.clone()));
                                    follow_up.push(self.load_all_files_task(repo.clone()));
                                    if self.graph_tab == GraphTab::Reflog {
                                        follow_up.push(self.load_reflog_task(repo.clone()));
                                    }
                                }
                                if has_staged {
                                    follow_up.push(self.scan_hygiene_task(repo));
                                } else {
                                    self.staged_secrets.clear();
                                    self.oversized_files.clear();
                                }
                            } else {
                                self.staged_secrets.clear();
                                self.oversized_files.clear();
                            }
                            return Task::batch(follow_up);
                        }
                    }
                    Err(e) => {
                        self.record_background_error("Repository Status Error", e, true);
                    }
                }
                Task::none()
            }
            Message::StageFile(path) => self.run_git_op("Stage", move |svc, repo| async move {
                svc.git.stage(&repo.path, &[path]).await.map(|_| "Staged".into())
            }),
            Message::UnstageFile(path) => self.run_git_op("Unstage", move |svc, repo| async move {
                svc.git
                    .unstage(&repo.path, &[path])
                    .await
                    .map(|_| "Unstaged".into())
            }),
            Message::StageSelectedFiles => {
                if self.has_modal_or_menu_open()
                    || self.graph_selection != GraphSelection::WorkingTree
                {
                    return Task::none();
                }
                let paths = self.selected_unstaged_paths();
                if paths.is_empty() {
                    self.toast_is_error = false;
                    self.toast_message = "No unstaged files selected".into();
                    return Task::none();
                }
                let count = paths.len();
                for p in &paths {
                    self.selected_changed_files.remove(&(p.clone(), false));
                }
                self.run_git_op("Stage", move |svc, repo| async move {
                    svc.git
                        .stage(&repo.path, &paths)
                        .await
                        .map(|_| format!("Staged {count} file(s)"))
                })
            }
            Message::UnstageSelectedFiles => {
                if self.has_modal_or_menu_open()
                    || self.graph_selection != GraphSelection::WorkingTree
                {
                    return Task::none();
                }
                let paths = self.selected_staged_paths();
                if paths.is_empty() {
                    self.toast_is_error = false;
                    self.toast_message = "No staged files selected".into();
                    return Task::none();
                }
                let count = paths.len();
                for p in &paths {
                    self.selected_changed_files.remove(&(p.clone(), true));
                }
                self.run_git_op("Unstage", move |svc, repo| async move {
                    svc.git
                        .unstage(&repo.path, &paths)
                        .await
                        .map(|_| format!("Unstaged {count} file(s)"))
                })
            }
            Message::DiscardSelectedFiles => {
                if self.has_modal_or_menu_open()
                    || self.graph_selection != GraphSelection::WorkingTree
                    || self.focus_pane == ActiveFocusPane::TextInput
                {
                    return Task::none();
                }
                let paths = self.selected_discard_paths();
                if paths.is_empty() {
                    self.toast_is_error = false;
                    self.toast_message = "Select file(s) to discard".into();
                    return Task::none();
                }
                self.update(Message::OpenDiscardModal(Some(paths)))
            }
            Message::StageAll => self.run_git_op("Stage All", move |svc, repo| async move {
                svc.git.stage_all(&repo.path).await.map(|_| "Staged all".into())
            }),
            Message::UnstageAll => self.run_git_op("Unstage All", move |svc, repo| async move {
                svc.git
                    .unstage_all(&repo.path)
                    .await
                    .map(|_| "Unstaged all".into())
            }),
            Message::ApplyHunk {
                patch,
                cached,
                reverse,
            } => {
                self.diff_selection = None;
                let label = match (cached, reverse) {
                    (true, false) => "Stage Hunk",
                    (true, true) => "Unstage Hunk",
                    _ => "Discard Hunk",
                };
                self.run_git_op(label, move |svc, repo| async move {
                    svc.git
                        .apply_hunk(&repo.path, &patch, cached, reverse)
                        .await
                        .map(|_| "Applied hunk".into())
                })
            }
            Message::ApplySelectedDiffLines { cached, reverse } => {
                if !self.diff_is_working_file {
                    return Task::none();
                }
                let Some((count, patch)) = build_selected_lines_patch(
                    &self.diff_parsed,
                    &self.diff_cache.layout,
                    self.diff_selection,
                    reverse,
                ) else {
                    self.toast_is_error = false;
                    self.toast_message = "Select changed (+ / -) lines in the diff first".into();
                    return Task::none();
                };
                self.diff_selection = None;
                let label = match (cached, reverse) {
                    (true, false) => "Stage Lines",
                    (true, true) => "Unstage Lines",
                    _ => "Discard Lines",
                };
                self.run_git_op(label, move |svc, repo| async move {
                    svc.git
                        .apply_hunk(&repo.path, &patch, cached, reverse)
                        .await
                        .map(|_| format!("Applied {count} selected line(s)"))
                })
            }
            Message::RequestDiscardSelectedDiffLines => {
                if !self.diff_is_working_file || self.diff_is_staged {
                    return Task::none();
                }
                let Some((count, patch)) = build_selected_lines_patch(
                    &self.diff_parsed,
                    &self.diff_cache.layout,
                    self.diff_selection,
                    true,
                ) else {
                    return Task::none();
                };
                self.diff_selection = None;
                self.pending_confirm = Some(PendingConfirm::discard_selected_lines(
                    &self.diff_title,
                    count,
                    patch,
                ));
                Task::none()
            }
            Message::StashPaths(paths) => {
                if paths.is_empty() {
                    return Task::none();
                }
                let count = paths.len();
                let msg = if count == 1 {
                    format!("Stash {}", paths[0])
                } else {
                    format!("Stash {count} selected files")
                };
                self.selected_changed_files.clear();
                self.run_git_op("Stash Selected", move |svc, repo| async move {
                    svc.git.stash_push_paths(&repo.path, Some(&msg), &paths).await
                })
            }
            Message::CommitSummaryChanged(val) => {
                self.terminal_focused = false;
                self.focus_pane = ActiveFocusPane::TextInput;
                self.commit_summary = val;
                Task::none()
            }
            Message::FocusCommitSummary => {
                self.terminal_focused = false;
                self.focus_pane = ActiveFocusPane::TextInput;
                self.graph_selection = GraphSelection::WorkingTree;
                iced::widget::text_input::focus(self.commit_summary_id.clone())
            }
            Message::ApplyConventionalPrefix(prefix) => {
                self.terminal_focused = false;
                self.focus_pane = ActiveFocusPane::TextInput;
                let known = ["feat:", "fix:", "refactor:", "perf:", "docs:", "chore:", "test:"];
                let trimmed = self.commit_summary.trim_start();
                let stripped = known
                    .iter()
                    .find_map(|k| trimmed.strip_prefix(k).map(str::trim_start))
                    .unwrap_or(trimmed);
                self.commit_summary = format!("{prefix}{stripped}");
                iced::widget::text_input::focus(self.commit_summary_id.clone())
            }
            Message::CommitMessageAction(action) => {
                self.terminal_focused = false;
                self.focus_pane = ActiveFocusPane::TextInput;
                self.commit_editor.perform(action);
                Task::none()
            }
            Message::AmendToggled(v) => {
                self.amend_commit = v;
                if v && let Some(repo) = self.current_repo() {
                    let svc = self.service.clone();
                    return Task::perform(
                        async move {
                            svc.git
                                .amend_target(&repo.path)
                                .await
                                .map_err(|e| e.to_string())
                        },
                        Message::AmendTargetLoaded,
                    );
                } else if !v {
                    self.amend_target = None;
                }
                Task::none()
            }
            Message::AmendTargetLoaded(res) => {
                if let Ok(target) = res {
                    if self.composed_commit_message().trim().is_empty() {
                        self.set_full_commit_message(&target.message);
                    }
                    self.amend_target = Some(target);
                }
                Task::none()
            }
            Message::SignCommitToggled(v) => {
                self.sign_commit = v;
                Task::none()
            }
            Message::LoadCommitTemplate => {
                let Some(repo) = self.current_repo() else {
                    return Task::none();
                };
                let svc = self.service.clone();
                let path = repo.path;
                Task::perform(
                    async move { svc.git.read_commit_template(&path).await },
                    Message::CommitTemplateLoaded,
                )
            }
            Message::CommitTemplateLoaded(tpl) => {
                match tpl {
                    Some(text) => {
                        self.set_full_commit_message(&text);
                        self.toast_is_error = false;
                        self.toast_message = "Loaded commit template".into();
                        return iced::widget::text_input::focus(self.commit_summary_id.clone());
                    }
                    None => {
                        self.toast_is_error = false;
                        self.toast_message =
                            "No commit.template or .gitmessage found in this repository".into();
                    }
                }
                Task::none()
            }
            Message::CommitSubmitted => self.try_commit(false),
            Message::CommitAndPushSubmitted => self.try_commit(true),
            Message::FetchSelected => self.run_git_op("Fetch", move |svc, repo| async move {
                svc.git.fetch(&repo.path).await
            }),
            Message::PullSelected => {
                let Some(repo) = self.current_repo() else {
                    return Task::none();
                };
                self.busy_label = Some("Pull…".into());
                let svc = self.service.clone();
                Task::perform(
                    async move {
                        svc.git
                            .pull_smart(&repo.path, PullStrategy::FastForwardOnly, false, false)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    |result| Message::PullSmartDone {
                        push_after: false,
                        result,
                    },
                )
            }
            Message::SmartPullClicked => {
                if self.selected_repo.is_none() {
                    return Task::none();
                }
                let (dirty, diverged) = self
                    .active_status()
                    .map(|s| {
                        (
                            (s.staged_count + s.unstaged_count + s.untracked_count) > 0,
                            s.ahead > 0 && s.behind > 0,
                        )
                    })
                    .unwrap_or((false, false));
                if dirty || diverged {
                    self.update(Message::OpenPullSyncModal { push_after: false })
                } else {
                    self.update(Message::PullSelected)
                }
            }
            Message::OpenPullSyncModal { push_after } => {
                if self.selected_repo.is_none() {
                    return Task::none();
                }
                self.context_menu = None;
                self.show_palette = false;
                let st = self.active_status();
                let dirty = st.is_some_and(|s| {
                    (s.staged_count + s.unstaged_count + s.untracked_count) > 0
                });
                let ahead = st.map(|s| s.ahead).unwrap_or(0);
                let strategy = if ahead > 0 {
                    PullStrategy::Rebase
                } else {
                    PullStrategy::FastForwardOnly
                };
                self.pull_sync_modal = Some(PullSyncModalState {
                    strategy,
                    auto_stash: dirty,
                    push_after,
                });
                Task::none()
            }
            Message::ClosePullSyncModal => {
                self.pull_sync_modal = None;
                Task::none()
            }
            Message::PullSyncSetStrategy(strat) => {
                if let Some(m) = &mut self.pull_sync_modal {
                    m.strategy = strat;
                }
                Task::none()
            }
            Message::PullSyncToggleAutoStash(v) => {
                if let Some(m) = &mut self.pull_sync_modal {
                    m.auto_stash = v;
                }
                Task::none()
            }
            Message::PullSyncTogglePushAfter(v) => {
                if let Some(m) = &mut self.pull_sync_modal {
                    m.push_after = v;
                }
                Task::none()
            }
            Message::PullSyncFetchOnly => {
                self.pull_sync_modal = None;
                self.update(Message::FetchSelected)
            }
            Message::ConfirmPullSyncModal => {
                let Some(modal) = self.pull_sync_modal.take() else {
                    return Task::none();
                };
                let Some(repo) = self.current_repo() else {
                    return Task::none();
                };
                let strategy = modal.strategy;
                let auto_stash = modal.auto_stash;
                let push_after = modal.push_after;
                self.busy_label = Some(if push_after {
                    "Syncing…".into()
                } else {
                    "Pulling…".into()
                });
                let svc = self.service.clone();
                Task::perform(
                    async move {
                        if strategy != PullStrategy::FastForwardOnly
                            && let Ok(prev) = svc.git.head_oid(&repo.path).await
                        {
                            svc.undo.record(UndoEntry {
                                id: format!("{}", Utc::now().timestamp_millis()),
                                repo_id: repo.id.clone(),
                                label: format!("Pull ({})", strategy.label()),
                                previous_head: prev,
                                created_at: Utc::now(),
                            });
                        }
                        svc.git
                            .pull_smart(&repo.path, strategy, auto_stash, push_after)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    move |result| Message::PullSmartDone { push_after, result },
                )
            }
            Message::PullSmartDone { push_after, result } => {
                self.busy_label = None;
                match result {
                    Ok(out) if out.pull_conflicted => {
                        self.auto_open_first_conflict = true;
                        self.toast_is_error = true;
                        self.toast_message = if out.auto_stashed {
                            "Upstream conflicts during Pull — local edits kept safely in stash@{0}; opening Conflict Solver".into()
                        } else {
                            "Upstream conflicts during Pull — opening 3-Way Conflict Solver".into()
                        };
                    }
                    Ok(out) if out.stash_pop_conflicted => {
                        self.auto_open_first_conflict = true;
                        self.toast_is_error = true;
                        self.toast_message =
                            "Pulled latest commits; local edits conflicted on Stash Pop — stash@{0} preserved safely; opening Conflict Solver"
                                .into();
                    }
                    Ok(out) => {
                        self.toast_is_error = false;
                        self.toast_message = match (out.pushed, out.auto_stashed) {
                            (true, true) => {
                                "Synced (Auto-Stashed, Pulled, Restored & Pushed)".into()
                            }
                            (true, false) => "Synced (Pulled & Pushed)".into(),
                            (false, true) => {
                                "Pulled & restored Auto-Stashed changes cleanly".into()
                            }
                            (false, false) => "Pull succeeded".into(),
                        };
                    }
                    Err(e) => {
                        self.report_action_error(
                            if push_after {
                                "Sync Failed"
                            } else {
                                "Pull Failed"
                            },
                            e,
                            true,
                        );
                    }
                }
                self.update(Message::RefreshSelected)
            }
            Message::PushSelected => self.run_git_op("Push", move |svc, repo| async move {
                svc.git.push(&repo.path).await
            }),
            Message::StashSelected => self.run_git_op("Save Stash", move |svc, repo| async move {
                svc.git.stash_push(&repo.path, None).await
            }),
            Message::OpenSaveStashModal => {
                if self.selected_repo.is_none() {
                    return Task::none();
                }
                self.context_menu = None;
                self.show_palette = false;
                let selected_paths = self.selected_discard_paths();
                self.save_stash_modal = Some(SaveStashModalState {
                    message: String::new(),
                    include_untracked: true,
                    keep_index: false,
                    selected_only: false,
                    selected_paths,
                });
                self.focus_pane = ActiveFocusPane::TextInput;
                iced::widget::text_input::focus(self.save_stash_input_id.clone())
            }
            Message::CloseSaveStashModal => {
                self.save_stash_modal = None;
                self.focus_pane = ActiveFocusPane::Other;
                Task::none()
            }
            Message::SaveStashMessageChanged(v) => {
                self.focus_pane = ActiveFocusPane::TextInput;
                if let Some(m) = &mut self.save_stash_modal {
                    m.message = v;
                }
                Task::none()
            }
            Message::SaveStashToggleUntracked(v) => {
                if let Some(m) = &mut self.save_stash_modal {
                    m.include_untracked = v;
                }
                Task::none()
            }
            Message::SaveStashToggleKeepIndex(v) => {
                if let Some(m) = &mut self.save_stash_modal {
                    m.keep_index = v;
                }
                Task::none()
            }
            Message::SaveStashToggleSelectedOnly(v) => {
                if let Some(m) = &mut self.save_stash_modal {
                    m.selected_only = v;
                }
                Task::none()
            }
            Message::ConfirmSaveStashModal => {
                let Some(modal) = self.save_stash_modal.take() else {
                    return Task::none();
                };
                self.focus_pane = ActiveFocusPane::Other;
                let msg = modal.message.trim().to_string();
                let include_untracked = modal.include_untracked;
                let keep_index = modal.keep_index;
                let selected_only = modal.selected_only;
                let paths = modal.selected_paths;
                if selected_only {
                    self.selected_changed_files.clear();
                }
                self.run_git_op("Save Stash", move |svc, repo| async move {
                    let msg_opt = (!msg.is_empty()).then_some(msg.as_str());
                    if selected_only && !paths.is_empty() {
                        svc.git.stash_push_paths(&repo.path, msg_opt, &paths).await
                    } else {
                        svc.git
                            .stash_push_with_options(
                                &repo.path,
                                msg_opt,
                                include_untracked,
                                keep_index,
                            )
                            .await
                    }
                })
            }
            Message::OpenApplyStashModal => {
                if self.selected_repo.is_none() {
                    return Task::none();
                }
                self.context_menu = None;
                self.show_palette = false;
                self.apply_stash_modal = Some(ApplyStashModalState { selected_index: 0 });
                if let Some(repo) = self.current_repo() {
                    return self.load_stashes_task(repo);
                }
                Task::none()
            }
            Message::CloseApplyStashModal => {
                self.apply_stash_modal = None;
                Task::none()
            }
            Message::SelectApplyStashIndex(idx) => {
                if let Some(m) = &mut self.apply_stash_modal {
                    m.selected_index = idx;
                }
                Task::none()
            }
            Message::InspectStashFromModal(idx) => {
                self.apply_stash_modal = None;
                self.update(Message::InspectStash(idx))
            }
            Message::ConfirmApplyStashModal { pop } => {
                let Some(modal) = self.apply_stash_modal.take() else {
                    return Task::none();
                };
                if self.stashes.is_empty() {
                    return Task::none();
                }
                let index = modal
                    .selected_index
                    .min(self.stashes.len().saturating_sub(1));
                if pop {
                    self.update(Message::PopStash(index))
                } else {
                    self.update(Message::ApplyStash(index))
                }
            }
            Message::StashApplySmartDone { index, pop, result } => {
                self.busy_label = None;
                match result {
                    Ok(out) if out.conflicted => {
                        self.auto_open_first_conflict = true;
                        self.toast_is_error = true;
                        self.toast_message = format!(
                            "Conflict applying stash@{{{index}}} — stash entry preserved safely; opening 3-Way Conflict Solver"
                        );
                    }
                    Ok(_) => {
                        self.toast_is_error = false;
                        self.toast_message = if pop {
                            format!("Popped stash@{{{index}}} cleanly")
                        } else {
                            format!("Applied stash@{{{index}}} cleanly")
                        };
                    }
                    Err(e) => {
                        self.report_action_error(
                            if pop {
                                "Pop Stash Failed"
                            } else {
                                "Apply Stash Failed"
                            },
                            e,
                            true,
                        );
                    }
                }
                self.update(Message::RefreshSelected)
            }
            Message::GitActionDone(label, res) => {
                self.busy_label = None;
                match res {
                    Ok(_) => {
                        self.toast_is_error = false;
                        self.toast_message = format!("{label} succeeded");
                    }
                    Err(e) => {
                        self.report_action_error(format!("{label} Failed"), e, true);
                    }
                }
                self.update(Message::RefreshSelected)
            }
            Message::SubmodulesLoaded(id, res) => {
                if let Ok(list) = res {
                    if !Self::is_submodule_repo_id(&id) {
                        self.repo_submodules.insert(id.clone(), list.clone());
                    }
                    if self.active_repo_key().as_ref() == Some(&id) {
                        self.submodules = list;
                    }
                }
                Task::none()
            }
            Message::WorktreesLoaded(id, res) => {
                if self.active_repo_key().as_ref() == Some(&id)
                    && let Ok(list) = res
                {
                    self.worktrees = list;
                }
                Task::none()
            }
            Message::SubmoduleUrlChanged(v) => {
                self.new_submodule_url = v;
                Task::none()
            }
            Message::SubmodulePathChanged(v) => {
                self.new_submodule_path = v;
                Task::none()
            }
            Message::AddSubmoduleSubmitted => {
                let url = self.new_submodule_url.trim().to_string();
                let dest = self.new_submodule_path.trim().to_string();
                if url.is_empty() || dest.is_empty() {
                    return Task::none();
                }
                self.new_submodule_url.clear();
                self.new_submodule_path.clear();
                self.run_git_op("Add Submodule", move |svc, repo| async move {
                    svc.git.submodule_add(&repo.path, &url, &dest).await
                })
            }
            Message::UpdateSubmodules => {
                self.run_git_op("Update Submodules", move |svc, repo| async move {
                    svc.git.submodule_update(&repo.path, None).await
                })
            }
            Message::SyncSubmodules => {
                self.run_git_op("Sync Submodules", move |svc, repo| async move {
                    svc.git.submodule_sync(&repo.path).await
                })
            }
            Message::RemoveSubmodule(sub_path) => {
                self.run_git_op("Remove Submodule", move |svc, repo| async move {
                    svc.git.submodule_remove(&repo.path, &sub_path).await
                })
            }
            Message::RunPushPreflight => {
                let Some(repo) = self.current_repo() else {
                    return Task::none();
                };
                let svc = self.service.clone();
                Task::perform(
                    async move {
                        svc.git
                            .push_preflight(&repo.path)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    Message::PushPreflightLoaded,
                )
            }
            Message::PushPreflightLoaded(res) => {
                match res {
                    Ok(orphans) => {
                        if orphans.is_empty() {
                            self.toast_is_error = false;
                            self.toast_message =
                                "Push preflight OK — no orphan submodule commits".into();
                        } else {
                            let details = orphans
                                .iter()
                                .map(|o| {
                                    format!(
                                        "{} ({})",
                                        o.submodule_path,
                                        &o.commit_oid[..o.commit_oid.len().min(7)]
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            self.report_action_error(
                                "Push Preflight Warning",
                                format!(
                                    "{} submodule(s) have unpushed commits — use Ordered Push before pushing the parent repository:\n{details}",
                                    orphans.len()
                                ),
                                false,
                            );
                        }
                        self.orphan_gitlinks = orphans;
                    }
                    Err(e) => {
                        self.report_action_error("Push Preflight Failed", e, true);
                    }
                }
                Task::none()
            }
            Message::RunOrderedPush => {
                let sub_paths: Vec<String> = if !self.orphan_gitlinks.is_empty() {
                    self.orphan_gitlinks
                        .iter()
                        .map(|o| o.submodule_path.clone())
                        .collect()
                } else {
                    self.submodules.iter().map(|s| s.path.clone()).collect()
                };
                self.orphan_gitlinks.clear();
                self.run_git_op("Ordered Push", move |svc, repo| async move {
                    svc.git.ordered_push(&repo.path, &sub_paths).await
                })
            }
            Message::NewWorktreeBranchChanged(v) => {
                self.new_worktree_branch = v;
                Task::none()
            }
            Message::AddWorktreeSubmitted => {
                let branch = self.new_worktree_branch.trim().to_string();
                if branch.is_empty() {
                    return Task::none();
                }
                let Some(repo) = self.current_repo() else {
                    return Task::none();
                };
                let safe_name = branch.replace('/', "-");
                let dest = repo
                    .path
                    .parent()
                    .unwrap_or(&repo.path)
                    .join(format!("{}-{safe_name}", repo.name));
                self.new_worktree_branch.clear();
                let exists_local = self
                    .branches
                    .iter()
                    .any(|b| !b.is_remote && b.name == branch);
                self.run_git_op("Add Worktree", move |svc, r| async move {
                    svc.git
                        .add_worktree(&r.path, &dest, &branch, !exists_local)
                        .await
                })
            }
            Message::RemoveWorktree(wt_path) => {
                self.run_git_op("Remove Worktree", move |svc, repo| async move {
                    svc.git.remove_worktree(&repo.path, &wt_path, false).await
                })
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
