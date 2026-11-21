use crate::app::{ActiveFocusPane, GitFleetApp, GraphSelection, GraphTab, Message};
use crate::preview;
use gitfleet_core::RepoId;
use gitfleet_git::ParsedDiff;
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
        self.diff_parsed = ParsedDiff::default();
        self.diff_expanded_gaps.clear();
        self.diff_selection = None;
        self.diff_is_working_file = false;
        self.diff_is_staged = false;
        self.preview_state = preview::PreviewState::Empty;
        self.sync_preview_editor();
        self.editor_content = text_editor::Content::new();
        self.open_doc_id = None;
        self.open_doc_version = None;
        self.editor_dirty = false;
        self.amend_commit = false;
        self.amend_target = None;
    }

    fn reset_workspace_view_state(&mut self) {
        self.selected_repo = None;
        self.active_submodule = None;
        self.bulk_selected.clear();
        self.statuses.clear();
        self.repo_submodules.clear();
        self.collapsed_repo_submodules.clear();
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
            Message::PickScanFolder => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_title("Scan folder for Git repositories")
                        .pick_folder()
                        .await
                        .map(|h| h.path().to_path_buf())
                },
                Message::ScanFolderPath,
            ),
            Message::AddRepoPath(Some(path)) => {
                match self.service.workspace.add_repo(path, None) {
                    Ok(entry) => {
                        self.toast_is_error = false;
                        self.toast_message = format!("Added '{}'", entry.name);
                        return self.update(Message::SelectRepo(entry.id));
                    }
                    Err(e) => {
                        self.toast_is_error = true;
                        self.toast_message = format!("Add failed: {e}");
                    }
                }
                Task::none()
            }
            Message::AddRepoPath(None) => Task::none(),
            Message::ScanFolderPath(Some(root)) => match self.service.workspace.scan_folder(&root) {
                Ok(paths) => {
                    let mut tasks = Vec::new();
                    let mut first_id = None;
                    let mut added = 0usize;
                    for p in paths {
                        if let Ok(entry) = self.service.workspace.add_repo(p, None) {
                            if first_id.is_none() {
                                first_id = Some(entry.id.clone());
                            }
                            added += 1;
                            tasks.push(self.load_status_task(entry.clone()));
                            tasks.push(self.load_submodules_task(entry));
                        }
                    }
                    self.toast_is_error = false;
                    self.toast_message = format!("Discovered {added} repositories");
                    if self.selected_repo.is_none()
                        && let Some(id) = first_id
                    {
                        tasks.push(self.update(Message::SelectRepo(id)));
                    }
                    Task::batch(tasks)
                }
                Err(e) => {
                    self.toast_is_error = true;
                    self.toast_message = format!("Scan failed: {e}");
                    Task::none()
                }
            },
            Message::ScanFolderPath(None) => Task::none(),
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
                        self.toast_is_error = true;
                        self.toast_message = format!(
                            "Submodule '{}' is not initialized on disk (run Update Submodules)",
                            repo.name
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
                        let fail = results.len() - ok;
                        (label, ok, fail)
                    },
                    |(l, ok, fail)| Message::BulkOpDone(l, ok, fail),
                )
            }
            Message::BulkOpDone(label, ok, fail) => {
                self.busy_label = None;
                self.bulk_cancel = None;
                self.toast_is_error = fail > 0;
                self.toast_message = format!("Bulk {label}: {ok} OK, {fail} failed");
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
                        self.diff_title = "No repository selected".into();
                        self.diff_parsed = ParsedDiff::default();
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
                        self.diff_title = "No repository selected".into();
                        self.diff_parsed = ParsedDiff::default();
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
                let _ = self.service.workspace.delete_group(&name);
                self.toast_is_error = false;
                self.toast_message =
                    format!("Deleted group '{name}' (repos moved to parent or root)");
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
                self.repo_drag_candidate = Some((id, name, self.cursor_pos));
                self.dragging_repo = None;
                self.drag_hover_group = None;
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
                        self.toast_is_error = true;
                        self.toast_message = format!("Create workspace failed: {e}");
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
                    self.toast_is_error = true;
                    self.toast_message = format!("Open workspace failed: {e}");
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
                    self.toast_is_error = true;
                    self.toast_message = format!("Save workspace failed: {e}");
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
                        self.toast_is_error = true;
                        self.toast_message = format!("Delete workspace failed: {e}");
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
                    self.toast_is_error = true;
                    self.toast_message = "Pick a parent directory for Clone first".into();
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
                        self.toast_is_error = true;
                        self.toast_message = format!("Clone failed: {e}");
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
                    self.toast_is_error = true;
                    self.toast_message = "Pick a parent directory first".into();
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
                        self.toast_is_error = true;
                        self.toast_message = format!("Locate failed: {e}");
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
                if let Some(repo) = self.current_repo() {
                    let _ = self.service.reveal_in_file_manager(&repo.path);
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
