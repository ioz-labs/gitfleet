use crate::app::{
    ActiveFocusPane, GitFleetApp, GraphSelection, GraphTab, Message, RenameWorkspaceModalState,
    ScanModalState,
};
use crate::preview;
use gitfleet_core::RepoId;
use gitfleet_workspace::DEFAULT_SCAN_DEPTH;
use iced::Task;
use iced::widget::{text_editor, text_input};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

impl GitFleetApp {
    fn clear_active_repo_view_state(&mut self) {
        self.selected_branch = None;
        self.last_branch_click = None;
        self.graph_checked_branches.clear();
        self.show_palette = false;
        self.show_branch_popover = false;
        self.rename_branch_modal = None;
        self.show_create_profile_bar = false;
        self.new_profile_name_input.clear();
        self.renaming_profile = None;
        self.clear_conflict_state();
        self.graph_selection = GraphSelection::WorkingTree;
        self.commits.clear();
        self.graph_rows.clear();
        self.graph_max_lanes = 1;
        self.reflog.clear();
        self.branches.clear();
        self.stashes.clear();
        self.tags.clear();
        self.submodules.clear();
        self.worktrees.clear();
        self.commit_files.clear();
        self.all_repo_files.clear();
        self.clear_ignored_repo_cache();
        self.selected_dir_filter = None;
        self.selected_all_dir.clear();
        self.expanded_all_dirs.clear();
        self.toggled_branch_nodes.clear();
        self.selected_changed_file = None;
        self.selected_changed_files.clear();
        self.file_selection_anchor = None;
        self.focus_pane = ActiveFocusPane::Other;
        self.active_file_path = None;
        self.diff_title = "Select a file to inspect diff / preview / edit".into();
        self.clear_diff_parsed_and_cache();
        self.diff_image = None;
        self.diff_csv = None;
        self.diff_is_working_file = false;
        self.diff_is_staged = false;
        self.open_doc_raw_text = None;
        self.preview_hydrated = false;
        self.editor_hydrated = false;
        self.preview_state = preview::PreviewState::Empty;
        self.sync_preview_editor();
        self.editor_content = text_editor::Content::new();
        self.open_doc_id = None;
        self.open_doc_version = None;
        self.open_doc_path = None;
        self.open_doc_line_ending = gitfleet_docs::LineEnding::Lf;
        self.open_doc_encoding = gitfleet_docs::FileEncoding::Utf8;
        self.editor_dirty = false;
        self.show_eol_modal = false;
        self.show_encoding_modal = false;
        self.show_syntax_modal = false;
        self.amend_commit = false;
        self.amend_target = None;
        self.git_flow_state = gitfleet_git::GitFlowState::default();
        self.git_flow_editing_config = false;
    }

    fn reset_workspace_view_state(&mut self) {
        self.selected_repo = None;
        self.active_submodule = None;
        self.bulk_selected.clear();
        self.statuses.clear();
        self.repo_submodules.clear();
        self.collapsed_repo_submodules.clear();
        self.rename_workspace_modal = None;
        self.scan_modal = None;
        self.scan_request_id = self.scan_request_id.wrapping_add(1);
        self.clear_active_repo_view_state();
        self.diff_title = "No repository selected".into();
    }

    pub(crate) fn try_update_workspace(&mut self, message: Message) -> Result<Task<Message>, Message> {
        let mut unhandled: Option<Message> = None;
        let task = (|| -> Task<Message> {
            match message {
            Message::FilterChanged(q) => {
                self.focus_pane = ActiveFocusPane::TextInput;
                self.filter_query = q;
                Task::none()
            }
            Message::SetRepoFilter(f) => {
                self.repo_filter = f;
                Task::none()
            }
            Message::PickAddRepo => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_title("Select a Git repository folder")
                        .pick_folder()
                        .await
                        .map(|h| h.path().to_path_buf())
                },
                Message::AddRepoPath,
            ),
            Message::PickScanFolder(target_parent_group) => {
                self.show_palette = false;
                self.context_menu = None;
                Task::perform(
                    async move {
                        let picked = rfd::AsyncFileDialog::new()
                            .set_title("Scan folder for Git repositories")
                            .pick_folder()
                            .await
                            .map(|h| h.path().to_path_buf());
                        (target_parent_group, picked)
                    },
                    |(parent, path)| Message::ScanFolderPath(parent, path),
                )
            }
            Message::AddRepoPath(Some(path)) => {
                match self.service.workspace.add_repo(path, None) {
                    Ok(entry) => {
                        self.toast_is_error = false;
                        self.toast_message = format!("Added '{}'", entry.name);
                        return self.update(Message::SelectRepo(entry.id));
                    }
                    Err(e) => {
                        self.report_action_error("Add Repository Failed", e.to_string(), false);
                    }
                }
                Task::none()
            }
            Message::AddRepoPath(None) => Task::none(),
            Message::ScanFolderPath(target_parent_group, Some(root)) => {
                let max_depth = self
                    .scan_modal
                    .as_ref()
                    .map(|m| m.max_depth)
                    .unwrap_or(DEFAULT_SCAN_DEPTH);
                if let Some(modal) = &mut self.scan_modal {
                    modal.scanning = true;
                }
                self.scan_request_id = self.scan_request_id.wrapping_add(1);
                let req_id = self.scan_request_id;
                self.busy_label = Some("Scanning folder for Git repositories…".into());
                let svc = self.service.clone();
                Task::perform(
                    async move {
                        let result = tokio::task::spawn_blocking(move || {
                            svc.workspace
                                .scan_folder_preview(&root, max_depth)
                                .map_err(|e| e.to_string())
                        })
                        .await
                        .unwrap_or_else(|e| Err(format!("Scan task failed: {e}")));
                        (req_id, target_parent_group, result)
                    },
                    |(request_id, target_parent_group, result)| Message::ScanPreviewLoaded {
                        request_id,
                        target_parent_group,
                        result,
                    },
                )
            }
            Message::ScanFolderPath(_, None) => Task::none(),
            Message::ScanPreviewLoaded {
                request_id,
                target_parent_group,
                result,
            } => {
                if request_id != self.scan_request_id {
                    return Task::none();
                }
                self.busy_label = None;
                match result {
                    Ok(scan_res) => {
                        if scan_res.repos.is_empty() && self.scan_modal.is_none() {
                            self.toast_is_error = false;
                            self.toast_message = format!(
                                "No Git repositories found in '{}'",
                                scan_res.root.display()
                            );
                            return Task::none();
                        }
                        self.show_palette = false;
                        self.context_menu = None;
                        self.scan_modal =
                            Some(ScanModalState::from_result(scan_res, target_parent_group));
                    }
                    Err(e) => {
                        if let Some(modal) = &mut self.scan_modal {
                            modal.scanning = false;
                        }
                        self.report_action_error("Scan Folder Failed", e, false);
                    }
                }
                Task::none()
            }
            Message::ScanModalSetDepth(depth) => {
                let Some(modal) = &mut self.scan_modal else {
                    return Task::none();
                };
                if modal.max_depth == depth && !modal.scanning {
                    return Task::none();
                }
                modal.max_depth = depth;
                modal.scanning = true;
                let root = modal.root.clone();
                let parent = modal.normalized_parent_group();
                self.scan_request_id = self.scan_request_id.wrapping_add(1);
                let req_id = self.scan_request_id;
                let svc = self.service.clone();
                self.busy_label = Some(format!("Scanning folder (depth {depth})…"));
                Task::perform(
                    async move {
                        let result = tokio::task::spawn_blocking(move || {
                            svc.workspace
                                .scan_folder_preview(&root, depth)
                                .map_err(|e| e.to_string())
                        })
                        .await
                        .unwrap_or_else(|e| Err(format!("Scan task failed: {e}")));
                        (req_id, parent, result)
                    },
                    |(request_id, target_parent_group, result)| Message::ScanPreviewLoaded {
                        request_id,
                        target_parent_group,
                        result,
                    },
                )
            }
            Message::ScanModalToggleLevel(level_idx) => {
                if let Some(modal) = &mut self.scan_modal
                    && let Some(slot) = modal.enabled_levels.get_mut(level_idx)
                {
                    *slot = !*slot;
                    if *slot {
                        // Restoring a level also clears individually dropped nodes at that exact depth
                        modal
                            .skipped_raw_nodes
                            .retain(|rp| rp.matches('/').count() != level_idx);
                    }
                }
                Task::none()
            }
            Message::ScanModalApplyPreset(preset) => {
                if let Some(modal) = &mut self.scan_modal {
                    modal.apply_preset(preset);
                }
                Task::none()
            }
            Message::ScanModalDropGroupNodes(raw_paths) => {
                if let Some(modal) = &mut self.scan_modal {
                    for rp in raw_paths {
                        modal.skipped_raw_nodes.insert(rp);
                    }
                }
                Task::none()
            }
            Message::ScanModalRestoreDroppedGroups => {
                if let Some(modal) = &mut self.scan_modal {
                    modal.skipped_raw_nodes.clear();
                }
                Task::none()
            }
            Message::ScanModalParentGroupChanged(val) => {
                if let Some(modal) = &mut self.scan_modal {
                    modal.target_parent_group = val;
                }
                Task::none()
            }
            Message::ScanModalToggleRepo(idx, selected) => {
                if let Some(modal) = &mut self.scan_modal
                    && let Some(item) = modal.repos.get_mut(idx)
                {
                    item.selected = selected;
                }
                Task::none()
            }
            Message::ScanModalToggleRepos(indices, selected) => {
                if let Some(modal) = &mut self.scan_modal {
                    for idx in indices {
                        if let Some(item) = modal.repos.get_mut(idx) {
                            item.selected = selected;
                        }
                    }
                }
                Task::none()
            }
            Message::ScanModalToggleAll(selected) => {
                if let Some(modal) = &mut self.scan_modal {
                    let actionable_indices: Vec<usize> = modal
                        .repos
                        .iter()
                        .enumerate()
                        .filter(|(_, item)| modal.is_repo_actionable(item))
                        .map(|(idx, _)| idx)
                        .collect();
                    for idx in actionable_indices {
                        if let Some(item) = modal.repos.get_mut(idx) {
                            item.selected = selected;
                        }
                    }
                }
                Task::none()
            }
            Message::ScanModalToggleUpdateExisting(enabled) => {
                if let Some(modal) = &mut self.scan_modal {
                    modal.update_existing_groups = enabled;
                }
                Task::none()
            }
            Message::CloseScanModal => {
                self.scan_modal = None;
                self.scan_request_id = self.scan_request_id.wrapping_add(1);
                if self
                    .busy_label
                    .as_deref()
                    .is_some_and(|l| l.starts_with("Scanning folder"))
                {
                    self.busy_label = None;
                }
                Task::none()
            }
            Message::ConfirmScanImport => {
                let Some(modal) = self.scan_modal.take() else {
                    return Task::none();
                };
                let items: Vec<(PathBuf, Option<String>)> = modal
                    .repos
                    .iter()
                    .filter(|item| item.selected && modal.is_repo_actionable(item))
                    .map(|item| (item.candidate.path.clone(), modal.resolved_group_for(item)))
                    .collect();
                if items.is_empty() {
                    return Task::none();
                }
                match self
                    .service
                    .workspace
                    .import_scanned_repos(&items, modal.update_existing_groups)
                {
                    Ok(report) => {
                        let mut tasks = Vec::new();
                        let first_id = report
                            .added
                            .first()
                            .or_else(|| report.regrouped.first())
                            .map(|r| r.id.clone());
                        for entry in &report.added {
                            tasks.push(self.load_status_task(entry.clone()));
                            tasks.push(self.load_submodules_task(entry.clone()));
                        }
                        let a = report.added.len();
                        let r = report.regrouped.len();
                        let g = report.groups_created;
                        self.toast_is_error = false;
                        self.toast_message = match (a > 0, r > 0, g > 0) {
                            (true, true, true) => format!(
                                "Imported {a} repo(s), regrouped {r} existing ({g} new group(s))"
                            ),
                            (true, true, false) => {
                                format!("Imported {a} repo(s) and regrouped {r} existing")
                            }
                            (true, false, true) => {
                                format!("Imported {a} repo(s) into {g} new group(s)")
                            }
                            (true, false, false) => format!("Imported {a} repo(s)"),
                            (false, true, true) => {
                                format!("Regrouped {r} repo(s) into {g} new group(s)")
                            }
                            (false, true, false) => format!("Updated groups for {r} repo(s)"),
                            (false, false, _) => "No workspace changes needed".to_string(),
                        };
                        if self.selected_repo.is_none()
                            && let Some(id) = first_id
                        {
                            tasks.push(self.update(Message::SelectRepo(id)));
                        }
                        Task::batch(tasks)
                    }
                    Err(e) => {
                        self.report_action_error("Import Repositories Failed", e.to_string(), false);
                        Task::none()
                    }
                }
            }
            Message::SelectRepo(id) => {
                self.selected_repo = Some(id);
                self.active_submodule = None;
                self.clear_active_repo_view_state();
                self.persist_ui_prefs();
                if let Some(repo) = self.current_repo() {
                    if self.service.settings().watch_filesystem {
                        self.service.scheduler.watch_repo(&repo.path);
                    }
                    self.orphan_gitlinks.clear();
                    self.staged_secrets.clear();
                    self.oversized_files.clear();
                    self.release_triggers = self.service.git.read_release_triggers(&repo.path);
                    self.refresh_vault_and_ignore();
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
                        tasks.push(self.load_ignored_entries_task(repo.clone()));
                    }
                    if self.graph_tab == GraphTab::Reflog {
                        tasks.push(self.load_reflog_task(repo));
                    }
                    return Task::batch(tasks);
                }
                Task::none()
            }
            Message::ToggleRepoSubmodules(repo_id) => {
                if !self.collapsed_repo_submodules.remove(&repo_id) {
                    self.collapsed_repo_submodules.insert(repo_id);
                }
                Task::none()
            }
            Message::SelectSubmodule(parent_id, sub_rel) => {
                self.selected_repo = Some(parent_id.clone());
                self.active_submodule = Some((parent_id, sub_rel));
                self.clear_active_repo_view_state();
                if let Some(repo) = self.current_repo() {
                    if !repo.available {
                        self.report_action_error(
                            "Submodule Not Initialized",
                            format!(
                                "Submodule '{}' is not initialized on disk. Run 'Update Submodules' first.",
                                repo.name
                            ),
                            false,
                        );
                        return Task::none();
                    }
                    if self.service.settings().watch_filesystem {
                        self.service.scheduler.watch_repo(&repo.path);
                    }
                    self.orphan_gitlinks.clear();
                    self.staged_secrets.clear();
                    self.oversized_files.clear();
                    self.release_triggers = self.service.git.read_release_triggers(&repo.path);
                    self.refresh_vault_and_ignore();
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
                        tasks.push(self.load_ignored_entries_task(repo.clone()));
                    }
                    if self.graph_tab == GraphTab::Reflog {
                        tasks.push(self.load_reflog_task(repo));
                    }
                    return Task::batch(tasks);
                }
                Task::none()
            }
            Message::SelectPrevRepo => {
                let ws = self.service.workspace.current();
                let list = self.filtered_repos(&ws.repos);
                if let Some(cur) = &self.selected_repo
                    && let Some(pos) = list.iter().position(|r| &r.id == cur)
                    && pos > 0
                {
                    return self.update(Message::SelectRepo(list[pos - 1].id.clone()));
                }
                Task::none()
            }
            Message::SelectNextRepo => {
                let ws = self.service.workspace.current();
                let list = self.filtered_repos(&ws.repos);
                if let Some(cur) = &self.selected_repo {
                    if let Some(pos) = list.iter().position(|r| &r.id == cur)
                        && pos + 1 < list.len()
                    {
                        return self.update(Message::SelectRepo(list[pos + 1].id.clone()));
                    }
                } else if let Some(first) = list.first() {
                    return self.update(Message::SelectRepo(first.id.clone()));
                }
                Task::none()
            }
            Message::ToggleBulkSelect(id, checked) => {
                if checked {
                    self.bulk_selected.insert(id);
                } else {
                    self.bulk_selected.remove(&id);
                }
                Task::none()
            }
            Message::ClearBulkSelection => {
                self.bulk_selected.clear();
                Task::none()
            }
            Message::RunBulkOp(op) => {
                let ws = self.service.workspace.current();
                let name_map: std::collections::HashMap<RepoId, String> = ws
                    .repos
                    .iter()
                    .map(|r| (r.id.clone(), r.name.clone()))
                    .collect();
                let targets: Vec<(RepoId, PathBuf)> = ws
                    .repos
                    .into_iter()
                    .filter(|r| self.bulk_selected.contains(&r.id))
                    .map(|r| (r.id, r.path))
                    .collect();
                if targets.is_empty() {
                    return Task::none();
                }
                let cancel_flag = Arc::new(AtomicBool::new(false));
                self.bulk_cancel = Some(cancel_flag.clone());
                let label = format!("{op:?}");
                self.busy_label = Some(format!("Bulk {label} ({})…", targets.len()));
                let svc = self.service.clone();
                let max_p = svc.settings().max_parallel_bulk_ops;
                Task::perform(
                    async move {
                        let results = svc
                            .git
                            .bulk_execute_cancellable(op, targets, max_p, cancel_flag)
                            .await;
                        let ok = results.iter().filter(|r| r.ok).count();
                        let failures: Vec<(String, String)> = results
                            .iter()
                            .filter(|r| !r.ok)
                            .map(|r| {
                                let name = name_map
                                    .get(&r.repo_id)
                                    .cloned()
                                    .unwrap_or_else(|| r.repo_id.0.clone());
                                (name, r.message.clone())
                            })
                            .collect();
                        (label, ok, failures.len(), failures)
                    },
                    |(label, ok, fail, failures)| Message::BulkOpDone {
                        label,
                        ok,
                        fail,
                        failures,
                        is_scheduled: false,
                    },
                )
            }
            Message::BulkOpDone {
                label,
                ok,
                fail,
                failures,
                is_scheduled,
            } => {
                self.busy_label = None;
                self.bulk_cancel = None;
                if fail > 0 {
                    let details = if failures.is_empty() {
                        format!("{ok} succeeded, {fail} failed.")
                    } else {
                        let list = failures
                            .iter()
                            .map(|(name, msg)| format!("• {name}: {}", msg.trim()))
                            .collect::<Vec<_>>()
                            .join("\n");
                        format!("{ok} succeeded, {fail} failed:\n{list}")
                    };
                    if is_scheduled {
                        self.record_background_error(format!("{label} Completed with Errors"), details, true);
                    } else {
                        self.report_action_error(format!("Bulk {label} Failed ({fail})"), details, true);
                    }
                } else {
                    self.toast_is_error = false;
                    self.toast_message = format!("Bulk {label}: {ok} OK, 0 failed");
                }
                self.update(Message::RefreshAll)
            }
            Message::RemoveRepo(id) => {
                let _ = self.service.workspace.remove_repos(&[id.clone()]);
                self.statuses.remove(&id);
                self.repo_submodules.remove(&id);
                self.collapsed_repo_submodules.remove(&id);
                self.bulk_selected.remove(&id);
                if self
                    .active_submodule
                    .as_ref()
                    .is_some_and(|(pid, _)| pid == &id)
                {
                    self.active_submodule = None;
                }
                self.toast_is_error = false;
                self.toast_message = "Removed repository from workspace (files on disk untouched)".into();
                if self.selected_repo.as_ref() == Some(&id) {
                    let next_id = self.service.workspace.current().repos.first().map(|r| r.id.clone());
                    if let Some(next) = next_id {
                        return self.update(Message::SelectRepo(next));
                    } else {
                        self.selected_repo = None;
                        self.branches.clear();
                        self.commits.clear();
                        self.commit_files.clear();
                        self.all_repo_files.clear();
                        self.clear_ignored_repo_cache();
                        self.diff_title = "No repository selected".into();
                        self.clear_diff_parsed_and_cache();
                        self.open_doc_raw_text = None;
                        self.preview_hydrated = false;
                        self.editor_hydrated = false;
                        self.preview_state = preview::PreviewState::Empty;
                        self.editor_content = text_editor::Content::new();
                        self.persist_ui_prefs();
                    }
                }
                Task::none()
            }
            Message::RemoveBulkSelected => {
                let ids: Vec<RepoId> = self.bulk_selected.iter().cloned().collect();
                if ids.is_empty() {
                    return Task::none();
                }
                let removed = self.service.workspace.remove_repos(&ids).unwrap_or(0);
                for id in &ids {
                    self.statuses.remove(id);
                    self.repo_submodules.remove(id);
                    self.collapsed_repo_submodules.remove(id);
                }
                if self
                    .active_submodule
                    .as_ref()
                    .is_some_and(|(pid, _)| ids.contains(pid))
                {
                    self.active_submodule = None;
                }
                self.bulk_selected.clear();
                self.toast_is_error = false;
                self.toast_message = format!("Removed {removed} repositories from workspace");
                if self
                    .selected_repo
                    .as_ref()
                    .is_some_and(|cur| ids.contains(cur))
                {
                    let next_id = self.service.workspace.current().repos.first().map(|r| r.id.clone());
                    if let Some(next) = next_id {
                        return self.update(Message::SelectRepo(next));
                    } else {
                        self.selected_repo = None;
                        self.branches.clear();
                        self.commits.clear();
                        self.commit_files.clear();
                        self.all_repo_files.clear();
                        self.clear_ignored_repo_cache();
                        self.diff_title = "No repository selected".into();
                        self.clear_diff_parsed_and_cache();
                        self.persist_ui_prefs();
                    }
                }
                Task::none()
            }
            Message::ToggleGroupCollapse(group) => {
                self.service.workspace.toggle_group_collapsed(&group);
                Task::none()
            }
            Message::ToggleInlineGroupCreator => {
                self.show_inline_group_input = !self.show_inline_group_input;
                if !self.show_inline_group_input {
                    self.new_group_parent = None;
                } else {
                    return text_input::focus(self.inline_group_input_id.clone());
                }
                Task::none()
            }
            Message::NewGroupNameChanged(v) => {
                self.new_group_input = v;
                Task::none()
            }
            Message::AddGroupSubmitted => {
                let raw = self.new_group_input.trim().to_string();
                if !raw.is_empty() {
                    let full_name = match &self.new_group_parent {
                        Some(parent) => format!("{parent}/{raw}"),
                        None => raw,
                    };
                    let normalized = gitfleet_workspace::normalize_group_path(&full_name);
                    if !normalized.is_empty() {
                        let _ = self.service.workspace.add_group(normalized.clone());
                        // Auto-expand parent so the newly created subgroup is visible immediately
                        if let Some(parent) = &self.new_group_parent
                            && self
                                .service
                                .workspace
                                .current()
                                .ui
                                .collapsed_groups
                                .contains(parent)
                        {
                            self.service.workspace.toggle_group_collapsed(parent);
                        }
                        self.new_group_input.clear();
                        self.new_group_parent = None;
                        self.show_inline_group_input = false;
                        self.toast_is_error = false;
                        self.toast_message = format!("Created group '{normalized}'");
                    }
                }
                Task::none()
            }
            Message::DeleteGroup(name) => {
                let removed_ids = self
                    .service
                    .workspace
                    .delete_group(&name)
                    .unwrap_or_default();
                let removed_cnt = removed_ids.len();
                for id in &removed_ids {
                    self.statuses.remove(id);
                    self.repo_submodules.remove(id);
                    self.collapsed_repo_submodules.remove(id);
                    self.bulk_selected.remove(id);
                }
                if self
                    .active_submodule
                    .as_ref()
                    .is_some_and(|(pid, _)| removed_ids.contains(pid))
                {
                    self.active_submodule = None;
                }
                self.toast_is_error = false;
                self.toast_message = if removed_cnt > 0 {
                    format!(
                        "Deleted group '{name}' and removed {removed_cnt} repo(s) from workspace (files on disk untouched)"
                    )
                } else {
                    format!("Deleted group '{name}'")
                };
                if self
                    .selected_repo
                    .as_ref()
                    .is_some_and(|cur| removed_ids.contains(cur))
                {
                    let next_id = self
                        .service
                        .workspace
                        .current()
                        .repos
                        .first()
                        .map(|r| r.id.clone());
                    if let Some(next) = next_id {
                        return self.update(Message::SelectRepo(next));
                    } else {
                        self.selected_repo = None;
                        self.active_submodule = None;
                        self.clear_active_repo_view_state();
                        self.diff_title = "No repository selected".into();
                        self.persist_ui_prefs();
                    }
                }
                Task::none()
            }
            Message::AssignRepoGroup(repo_id, group) => {
                let _ = self
                    .service
                    .workspace
                    .set_repo_group(&repo_id, group.clone());
                self.toast_is_error = false;
                self.toast_message = match group {
                    Some(g) if !g.is_empty() => format!("Moved repository to group '{g}'"),
                    _ => "Removed repository from group (moved to root)".to_string(),
                };
                Task::none()
            }
            Message::RepoMouseDown(id, name) => {
                let pos = self.current_host_cursor_pos();
                self.cursor_pos = pos;
                self.repo_drag_candidate = Some((id, name, pos));
                self.dragging_repo = None;
                self.drag_hover_group = None;
                crate::app::subscription::set_stream_cursor_events(true);
                Task::none()
            }
            Message::DragHoverGroup(token, target) => {
                if self.dragging_repo.is_some() || self.repo_drag_candidate.is_some() {
                    self.drag_hover_group = Some((token, target));
                }
                Task::none()
            }
            Message::DragHoverGroupExit(token) => {
                if self
                    .drag_hover_group
                    .as_ref()
                    .is_some_and(|(active_tok, _)| active_tok == &token)
                {
                    self.drag_hover_group = None;
                }
                Task::none()
            }
            Message::ToggleWorkspaceModal => {
                self.show_palette = false;
                self.show_branch_popover = false;
                self.show_merge_modal = false;
                self.show_git_flow_modal = false;
                self.show_settings_modal = false;
                self.show_eol_modal = false;
                self.show_encoding_modal = false;
                self.show_syntax_modal = false;
                self.rename_workspace_modal = None;
                self.scan_modal = None;
                self.scan_request_id = self.scan_request_id.wrapping_add(1);
                self.show_workspace_modal = !self.show_workspace_modal;
                Task::none()
            }
            Message::NewWorkspaceNameChanged(v) => {
                self.new_workspace_name = v;
                Task::none()
            }
            Message::CreateNewWorkspace => {
                let name = if self.new_workspace_name.trim().is_empty() {
                    "Untitled Workspace".to_string()
                } else {
                    self.new_workspace_name.trim().to_string()
                };
                match self.service.workspace.new_workspace(name.clone()) {
                    Ok(_) => {
                        self.new_workspace_name.clear();
                        self.reset_workspace_view_state();
                        self.show_workspace_modal = false;
                        self.toast_is_error = false;
                        self.toast_message = format!("Created & switched to workspace '{name}'");
                    }
                    Err(e) => {
                        self.report_action_error("Create Workspace Failed", e.to_string(), false);
                    }
                }
                Task::none()
            }
            Message::OpenRenameWorkspaceModal(target_path) => {
                let (path, old_name, file_hint) = if let Some(target) = target_path {
                    let all = self.service.workspace.list_workspaces();
                    let Some(item) = all.into_iter().find(|w| w.path == target) else {
                        self.report_action_error("Workspace Not Found", "The selected workspace file could not be found.", false);
                        return Task::none();
                    };
                    let hint = item
                        .path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(str::to_string);
                    let path_opt = if item.is_current {
                        None
                    } else {
                        Some(item.path)
                    };
                    (path_opt, item.name, hint)
                } else {
                    let ws = self.service.workspace.current();
                    let hint = ws
                        .file_path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .and_then(|n| n.to_str())
                        .map(str::to_string);
                    (None, ws.name, hint)
                };
                self.context_menu = None;
                self.show_palette = false;
                self.show_branch_popover = false;
                self.rename_branch_modal = None;
                self.rename_workspace_modal = Some(RenameWorkspaceModalState {
                    path,
                    new_name: old_name.clone(),
                    old_name,
                    file_hint,
                });
                Task::batch([
                    text_input::focus(self.rename_workspace_input_id.clone()),
                    text_input::select_all(self.rename_workspace_input_id.clone()),
                ])
            }
            Message::RenameWorkspaceInputChanged(val) => {
                if let Some(modal) = &mut self.rename_workspace_modal {
                    modal.new_name = val;
                }
                Task::none()
            }
            Message::SubmitRenameWorkspace => {
                let Some(modal) = &self.rename_workspace_modal else {
                    return Task::none();
                };
                let new_name = modal.new_name.trim().to_string();
                if new_name.is_empty() {
                    return Task::none();
                }
                if new_name == modal.old_name {
                    self.rename_workspace_modal = None;
                    return Task::none();
                }
                let Some(modal) = self.rename_workspace_modal.take() else {
                    return Task::none();
                };
                let res = match &modal.path {
                    Some(path) => self
                        .service
                        .workspace
                        .rename_workspace_at(path, new_name.clone())
                        .map(|_| ()),
                    None => self.service.workspace.rename_workspace(new_name.clone()),
                };
                match res {
                    Ok(()) => {
                        self.toast_is_error = false;
                        self.toast_message = format!(
                            "Renamed workspace '{}' to '{}'",
                            modal.old_name, new_name
                        );
                    }
                    Err(e) => {
                        self.report_action_error("Rename Workspace Failed", e.to_string(), false);
                    }
                }
                Task::none()
            }
            Message::PickOpenWorkspace => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .add_filter("GitFleet Workspace", &["json", "gitfleet"])
                        .pick_file()
                        .await
                        .map(|h| h.path().to_path_buf())
                },
                Message::OpenWorkspacePath,
            ),
            Message::OpenWorkspacePath(Some(path)) => match self.service.workspace.load_from_file(&path) {
                Ok(cfg) => {
                    self.show_workspace_modal = false;
                    self.show_palette = false;
                    self.reset_workspace_view_state();
                    self.toast_is_error = false;
                    self.toast_message = format!("Switched to workspace '{}'", cfg.name);
                    let mut tasks = vec![self.update(Message::RefreshAll)];
                    if let Some(first) = cfg.repos.first() {
                        tasks.push(self.update(Message::SelectRepo(first.id.clone())));
                    }
                    Task::batch(tasks)
                }
                Err(e) => {
                    self.report_action_error("Open Workspace Failed", e.to_string(), false);
                    Task::none()
                }
            },
            Message::OpenWorkspacePath(None) => Task::none(),
            Message::PickSaveWorkspaceAs => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_file_name("workspace.gitfleet.json")
                        .add_filter("GitFleet Workspace", &["json", "gitfleet"])
                        .save_file()
                        .await
                        .map(|h| h.path().to_path_buf())
                },
                Message::SaveWorkspaceAsPath,
            ),
            Message::SaveWorkspaceAsPath(Some(path)) => match self.service.workspace.save_to_file(&path) {
                Ok(_) => {
                    self.toast_is_error = false;
                    self.toast_message = format!("Saved workspace to {}", path.display());
                    Task::none()
                }
                Err(e) => {
                    self.report_action_error("Save Workspace Failed", e.to_string(), false);
                    Task::none()
                }
            },
            Message::SaveWorkspaceAsPath(None) => Task::none(),
            Message::OpenHomeWorkspace => {
                if let Ok(cfg) = self.service.workspace.open_home_workspace() {
                    self.show_workspace_modal = false;
                    self.show_palette = false;
                    self.reset_workspace_view_state();
                    self.toast_is_error = false;
                    self.toast_message = format!("Switched to {}", cfg.name);
                    let mut tasks = vec![self.update(Message::RefreshAll)];
                    if let Some(first) = cfg.repos.first() {
                        tasks.push(self.update(Message::SelectRepo(first.id.clone())));
                    }
                    return Task::batch(tasks);
                }
                Task::none()
            }
            Message::DeleteWorkspace(path) => {
                let prev_path = self.service.workspace.current().file_path;
                match self.service.workspace.delete_workspace(&path) {
                    Ok(_) => {
                        let cur = self.service.workspace.current();
                        let switched = prev_path != cur.file_path;
                        self.toast_is_error = false;
                        self.toast_message = if switched {
                            format!("Deleted workspace — switched to '{}'", cur.name)
                        } else {
                            "Deleted workspace".to_string()
                        };
                        if switched {
                            self.reset_workspace_view_state();
                            let mut tasks = vec![self.update(Message::RefreshAll)];
                            if let Some(first) = cur.repos.first() {
                                tasks.push(self.update(Message::SelectRepo(first.id.clone())));
                            }
                            return Task::batch(tasks);
                        }
                        Task::none()
                    }
                    Err(e) => {
                        self.report_action_error("Delete Workspace Failed", e.to_string(), false);
                        Task::none()
                    }
                }
            }
            Message::TakeWorkspaceOwnership => {
                let own = self.service.workspace.take_ownership();
                self.toast_is_error = false;
                self.toast_message = format!("Claimed workspace ownership (PID {:?})", own.owner_pid);
                Task::none()
            }
            Message::CloneUrlChanged(v) => {
                if self.clone_folder_input.is_empty()
                    && let Some(tail) = v.trim().rsplit('/').next()
                {
                    let clean = tail.trim_end_matches(".git");
                    if !clean.is_empty() && !clean.contains(':') {
                        self.clone_folder_input = clean.to_string();
                    }
                }
                self.clone_url_input = v;
                Task::none()
            }
            Message::CloneFolderChanged(v) => {
                self.clone_folder_input = v;
                Task::none()
            }
            Message::PickCloneParent => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_title("Choose parent directory for clone")
                        .pick_folder()
                        .await
                        .map(|h| h.path().to_path_buf())
                },
                Message::CloneParentPicked,
            ),
            Message::CloneParentPicked(p) => {
                self.clone_parent_dir = p;
                Task::none()
            }
            Message::CloneRepoSubmitted => {
                let url = self.clone_url_input.trim().to_string();
                let folder = self.clone_folder_input.trim().to_string();
                let Some(parent) = self.clone_parent_dir.clone() else {
                    self.report_action_error(
                        "Clone Repository Failed",
                        "Pick a parent directory before cloning a repository.",
                        false,
                    );
                    return Task::none();
                };
                if url.is_empty() || folder.is_empty() {
                    return Task::none();
                }
                self.show_workspace_modal = false;
                self.busy_label = Some(format!("Cloning {folder}…"));
                let svc = self.service.clone();
                Task::perform(
                    async move {
                        svc.git
                            .clone_repo(&url, &parent, &folder, true)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    Message::CloneRepoFinished,
                )
            }
            Message::CloneRepoFinished(res) => {
                self.busy_label = None;
                match res {
                    Ok(dest) => {
                        self.clone_url_input.clear();
                        self.clone_folder_input.clear();
                        if let Ok(entry) = self.service.workspace.add_repo(dest, None) {
                            self.toast_is_error = false;
                            self.toast_message = format!("Cloned '{}'", entry.name);
                            return self.update(Message::SelectRepo(entry.id));
                        }
                    }
                    Err(e) => {
                        self.report_action_error("Clone / Init Repository Failed", e, true);
                    }
                }
                Task::none()
            }
            Message::CreateRepoNameChanged(v) => {
                self.create_repo_name = v;
                Task::none()
            }
            Message::PickCreateParent => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_title("Choose parent directory for new Git repo")
                        .pick_folder()
                        .await
                        .map(|h| h.path().to_path_buf())
                },
                Message::CreateParentPicked,
            ),
            Message::CreateParentPicked(p) => {
                self.create_parent_dir = p;
                Task::none()
            }
            Message::CreateRepoSubmitted => {
                let name = self.create_repo_name.trim().to_string();
                let Some(parent) = self.create_parent_dir.clone() else {
                    self.report_action_error(
                        "Create Repository Failed",
                        "Pick a parent directory before creating a new repository.",
                        false,
                    );
                    return Task::none();
                };
                if name.is_empty() {
                    return Task::none();
                }
                let dest = parent.join(&name);
                self.create_repo_name.clear();
                self.show_workspace_modal = false;
                let svc = self.service.clone();
                Task::perform(
                    async move {
                        svc.git.init_repo(&dest).await.map(|_| dest).map_err(|e| e.to_string())
                    },
                    Message::CloneRepoFinished,
                )
            }
            Message::LocateRepo(repo_id) => Task::perform(
                async move {
                    let picked = rfd::AsyncFileDialog::new()
                        .set_title("Locate moved repository folder")
                        .pick_folder()
                        .await
                        .map(|h| h.path().to_path_buf());
                    (repo_id, picked)
                },
                |(id, p)| Message::LocateRepoPicked(id, p),
            ),
            Message::LocateRepoPicked(repo_id, Some(path)) => {
                match self.service.workspace.locate_repo(&repo_id, path) {
                    Ok(suggestion) => {
                        self.pending_relocation = suggestion;
                        self.toast_is_error = false;
                        self.toast_message = "Located repository path".into();
                        return self.update(Message::SelectRepo(repo_id));
                    }
                    Err(e) => {
                        self.report_action_error("Locate Repository Failed", e.to_string(), false);
                    }
                }
                Task::none()
            }
            Message::LocateRepoPicked(_, None) => Task::none(),
            Message::ApplyPrefixRelocation => {
                if let Some(mapping) = self.pending_relocation.take() {
                    let count = self.service.workspace.relocate_prefix(&mapping).unwrap_or(0);
                    self.toast_is_error = false;
                    self.toast_message = format!("Relocated {count} sibling repositories");
                    return self.update(Message::RefreshAll);
                }
                Task::none()
            }
            Message::DismissPrefixRelocation => {
                self.pending_relocation = None;
                Task::none()
            }
            Message::CancelBulkOp => {
                if let Some(flag) = &self.bulk_cancel {
                    flag.store(true, Ordering::Relaxed);
                    self.toast_is_error = false;
                    self.toast_message = "Cancelling bulk operation…".into();
                }
                Task::none()
            }
            Message::RevealSelectedInExplorer => {
                if let Some(repo) = self.current_repo()
                    && let Err(e) = self.service.reveal_in_file_manager(&repo.path)
                {
                    self.report_action_error("Open in Explorer Failed", e.to_string(), false);
                }
                Task::none()
            }
            Message::RevealPathInExplorer(path) => {
                if let Err(e) = self.service.reveal_in_file_manager(&path) {
                    self.report_action_error("Open in Explorer Failed", e.to_string(), false);
                }
                Task::none()
            }
            Message::RevealRepoFileInExplorer(rel_path) => {
                if let Some(repo) = self.current_repo() {
                    let full = repo.path.join(&rel_path);
                    if let Err(e) = self.service.reveal_in_file_manager(&full) {
                        self.report_action_error("Open in Explorer Failed", e.to_string(), false);
                    }
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
