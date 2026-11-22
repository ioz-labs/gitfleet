use crate::app::{
    BottomViewMode, CherryPickModalState, ConfirmAction, ContextMenuState, ContextMenuSubmenu,
    DiscardFileEntry, DiscardModalState, DrawerTab, GitFleetApp, Message, ResizeTarget,
    STATUS_BAR_HEIGHT, TITLEBAR_HEIGHT, TOOLBAR_HEIGHT, TerminalMenuSubmenu, WindowKind,
};
use crate::theme::ThemeMode;
use chrono::Utc;
use gitfleet_core::{RepoId, UndoEntry};
use gitfleet_git::bulk::BulkOperation;
use gitfleet_git::{DiscardTarget, FileChangeKind};
use gitfleet_search::SearchRequest;
use iced::keyboard::Modifiers;
use iced::widget::text_input;
use iced::{Task, clipboard, window};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

impl GitFleetApp {
    pub(crate) fn try_update_ui_misc(&mut self, message: Message) -> Result<Task<Message>, Message> {
        let mut unhandled: Option<Message> = None;
        let task = (|| -> Task<Message> {
            match message {
            Message::ToggleTheme => {
                self.theme_mode = match self.theme_mode {
                    ThemeMode::Dark => ThemeMode::Light,
                    ThemeMode::Light => ThemeMode::Dark,
                };
                self.rebuild_diff_cache();
                for viewer in self.detached_viewers.values_mut() {
                    for tab in &mut viewer.tabs {
                        tab.rebuild_diff_cache(self.theme_mode);
                    }
                }
                let mut s = self.service.settings();
                s.theme = match self.theme_mode {
                    ThemeMode::Dark => "dark".into(),
                    ThemeMode::Light => "light".into(),
                };
                self.service.update_settings(s);
                self.persist_ui_prefs();
                Task::none()
            }
            Message::SetThemeMode(mode) => {
                self.theme_mode = mode;
                self.rebuild_diff_cache();
                for viewer in self.detached_viewers.values_mut() {
                    for tab in &mut viewer.tabs {
                        tab.rebuild_diff_cache(self.theme_mode);
                    }
                }
                let mut s = self.service.settings();
                s.theme = match mode {
                    ThemeMode::Dark => "dark".into(),
                    ThemeMode::Light => "light".into(),
                };
                self.service.update_settings(s);
                self.persist_ui_prefs();
                Task::none()
            }
            Message::ToggleCommandPalette => {
                self.show_branch_popover = false;
                self.show_eol_modal = false;
                self.show_encoding_modal = false;
                self.show_syntax_modal = false;
                self.show_palette = !self.show_palette;
                self.palette_query.clear();
                if self.show_palette {
                    return text_input::focus(self.palette_input_id.clone());
                }
                Task::none()
            }
            Message::PaletteQueryChanged(q) => {
                self.palette_query = q;
                Task::none()
            }
            Message::ToggleBranchPopover => {
                if self.selected_repo.is_none() {
                    return Task::none();
                }
                self.show_palette = false;
                self.show_eol_modal = false;
                self.show_encoding_modal = false;
                self.show_syntax_modal = false;
                self.context_menu = None;
                self.show_branch_popover = !self.show_branch_popover;
                self.branch_filter_query.clear();
                self.branch_create_base = None;
                self.branch_create_checkout = true;
                if self.show_branch_popover {
                    return text_input::focus(self.branch_filter_id.clone());
                }
                Task::none()
            }
            Message::OpenCreateBranch { base_ref, prefix } => {
                if self.selected_repo.is_none() {
                    return Task::none();
                }
                self.show_palette = false;
                self.context_menu = None;
                self.show_branch_popover = true;
                self.branch_create_base = base_ref;
                self.branch_create_checkout = true;
                self.branch_filter_query = prefix.unwrap_or_default();
                text_input::focus(self.branch_filter_id.clone())
            }
            Message::ToggleCreateBranchCheckout(v) => {
                self.branch_create_checkout = v;
                Task::none()
            }
            Message::BranchFilterChanged(q) => {
                self.branch_filter_query = q;
                Task::none()
            }
            Message::CloseOverlays => {
                if self.action_error.is_some() {
                    self.action_error = None;
                    return Task::none();
                }
                if self.dragging_repo.is_some() || self.repo_drag_candidate.is_some() {
                    self.dragging_repo = None;
                    self.repo_drag_candidate = None;
                    self.drag_hover_group = None;
                    return Task::none();
                }
                if self.pending_confirm.is_some() {
                    self.pending_confirm = None;
                    return Task::none();
                }
                if self.cherry_pick_modal.is_some() {
                    self.cherry_pick_modal = None;
                    return Task::none();
                }
                if self.discard_modal.is_some() {
                    self.discard_modal = None;
                    return Task::none();
                }
                if self.rename_branch_modal.is_some() {
                    self.rename_branch_modal = None;
                    return Task::none();
                }
                if self.rename_workspace_modal.is_some() {
                    self.rename_workspace_modal = None;
                    return Task::none();
                }
                if self.rename_group_modal.is_some() {
                    self.rename_group_modal = None;
                    return Task::none();
                }
                if self.rename_entry_modal.is_some() {
                    self.rename_entry_modal = None;
                    return Task::none();
                }
                if self.add_gitignore_modal.is_some() {
                    self.add_gitignore_modal = None;
                    return Task::none();
                }
                if self.commit_modal.is_some() {
                    self.commit_modal = None;
                    return Task::none();
                }
                if self.scan_modal.is_some() {
                    self.scan_modal = None;
                    self.scan_request_id = self.scan_request_id.wrapping_add(1);
                    if self
                        .busy_label
                        .as_deref()
                        .is_some_and(|l| l.starts_with("Scanning folder"))
                    {
                        self.busy_label = None;
                    }
                    return Task::none();
                }
                if self.context_menu.is_some() {
                    self.context_menu = None;
                    return Task::none();
                }
                if self.show_terminal_menu {
                    self.show_terminal_menu = false;
                    self.terminal_menu_submenu = TerminalMenuSubmenu::None;
                    return Task::none();
                }
                if self.show_inline_group_input {
                    self.show_inline_group_input = false;
                }
                self.show_palette = false;
                self.show_branch_popover = false;
                self.show_merge_modal = false;
                self.show_git_flow_modal = false;
                self.git_flow_editing_config = false;
                self.show_workspace_modal = false;
                self.show_settings_modal = false;
                self.show_eol_modal = false;
                self.show_encoding_modal = false;
                self.show_syntax_modal = false;
                Task::none()
            }
            Message::ToggleDrawer(tab) => {
                if self.drawer_tab == Some(tab) {
                    self.drawer_tab = None;
                    self.terminal_focused = false;
                    self.show_terminal_menu = false;
                    self.terminal_menu_submenu = TerminalMenuSubmenu::None;
                } else {
                    self.drawer_tab = Some(tab);
                    self.show_terminal_menu = false;
                    self.terminal_menu_submenu = TerminalMenuSubmenu::None;
                    match tab {
                        DrawerTab::Secrets | DrawerTab::Ignore => {
                            self.terminal_focused = false;
                            self.refresh_vault_and_ignore();
                        }
                        DrawerTab::Terminal => {
                            if self.terminal_tabs.is_empty() {
                                self.spawn_terminal_pane(None, false);
                            } else {
                                self.terminal_focused = true;
                                self.sync_terminal_snapshots();
                            }
                        }
                        _ => {
                            self.terminal_focused = false;
                        }
                    }
                }
                Task::none()
            }
            Message::CloseDrawer => {
                self.drawer_tab = None;
                self.terminal_focused = false;
                self.show_terminal_menu = false;
                self.terminal_menu_submenu = TerminalMenuSubmenu::None;
                Task::none()
            }
            Message::ClearCommandLog => {
                self.service.command_log.clear();
                Task::none()
            }
            Message::OpenDetachedViewer => {
                let snapshot = crate::views::detached_viewer::DetachedViewerState::from_host(self);
                self.open_detached_viewer_window(snapshot)
            }
            Message::OpenDetachedForChangedFile(file) => {
                let path = file.path.clone();
                let state = crate::views::detached_viewer::DetachedViewerState::for_repo_file(
                    self,
                    &path,
                    Some(file),
                    false,
                );
                self.open_detached_viewer_window(state)
            }
            Message::OpenDetachedForRepoFile(rel_path) => {
                let state = crate::views::detached_viewer::DetachedViewerState::for_repo_file(
                    self,
                    &rel_path,
                    None,
                    true,
                );
                self.open_detached_viewer_window(state)
            }
            Message::OpenDetachedSearch(initial_query) => {
                let state = crate::views::detached_viewer::DetachedViewerState::for_search(
                    self,
                    initial_query,
                );
                self.open_detached_viewer_window(state)
            }
            Message::ViewerOpened(id) => {
                self.windows.insert(id, WindowKind::DetachedViewer);
                let state = self
                    .pending_detached_viewers
                    .pop_front()
                    .unwrap_or_else(crate::views::detached_viewer::DetachedViewerState::empty);
                self.detached_viewers.insert(id, state);
                self.on_detached_viewer_opened(id)
            }
            Message::Detached(id, action) => self.update_detached_viewer(id, action),
            Message::ShortcutSave(id) => {
                if self.detached_viewers.contains_key(&id) {
                    self.update_detached_viewer(
                        id,
                        crate::views::detached_viewer::DetachedAction::SaveEditor,
                    )
                } else {
                    self.update(Message::SaveDocument)
                }
            }
            Message::ShortcutFind(id) => {
                if self.detached_viewers.contains_key(&id) {
                    self.update_detached_viewer(
                        id,
                        crate::views::detached_viewer::DetachedAction::OpenInFileFind {
                            show_replace: false,
                        },
                    )
                } else {
                    Task::none()
                }
            }
            Message::ShortcutReplace(id) => {
                if self.detached_viewers.contains_key(&id) {
                    self.update_detached_viewer(
                        id,
                        crate::views::detached_viewer::DetachedAction::OpenInFileFind {
                            show_replace: true,
                        },
                    )
                } else {
                    Task::none()
                }
            }
            Message::ShortcutSearchFiles(id) => {
                if self.detached_viewers.contains_key(&id) {
                    self.update_detached_viewer(
                        id,
                        crate::views::detached_viewer::DetachedAction::OpenAndFocusSearchSidebar,
                    )
                } else {
                    self.update(Message::ToggleDrawer(DrawerTab::Search))
                }
            }
            Message::ShortcutExplorer(id) => {
                if self.detached_viewers.contains_key(&id) {
                    self.update_detached_viewer(
                        id,
                        crate::views::detached_viewer::DetachedAction::OpenAndFocusExplorer,
                    )
                } else {
                    Task::none()
                }
            }
            Message::ShortcutToggleSidebar(id) => {
                if self.detached_viewers.contains_key(&id) {
                    self.update_detached_viewer(
                        id,
                        crate::views::detached_viewer::DetachedAction::ToggleSidebar,
                    )
                } else {
                    self.update(Message::ToggleBranchPopover)
                }
            }
            Message::ShortcutCloseTab(id) => {
                if self.detached_viewers.contains_key(&id) {
                    self.update_detached_viewer(
                        id,
                        crate::views::detached_viewer::DetachedAction::CloseActiveTab,
                    )
                } else {
                    Task::none()
                }
            }
            Message::ShortcutOpenFile(id) => {
                if self.detached_viewers.contains_key(&id) {
                    self.update_detached_viewer(
                        id,
                        crate::views::detached_viewer::DetachedAction::PickSingleFile,
                    )
                } else {
                    Task::none()
                }
            }
            Message::ShortcutZoomFont(id, delta) => {
                if self.detached_viewers.contains_key(&id) {
                    self.update_detached_viewer(
                        id,
                        crate::views::detached_viewer::DetachedAction::AdjustFontSize(delta),
                    )
                } else {
                    Task::none()
                }
            }
            Message::ShortcutToggleWrap(id) => {
                if self.detached_viewers.contains_key(&id) {
                    self.update_detached_viewer(
                        id,
                        crate::views::detached_viewer::DetachedAction::ToggleWordWrap,
                    )
                } else {
                    Task::none()
                }
            }
            Message::ShortcutEscape(id) => {
                if self.detached_viewers.contains_key(&id) {
                    self.update_detached_viewer(
                        id,
                        crate::views::detached_viewer::DetachedAction::EscapePressed,
                    )
                } else {
                    self.update(Message::CloseOverlays)
                }
            }
            Message::WindowClosed(id) => {
                self.detached_viewers.remove(&id);
                let kind = self.windows.remove(&id);
                if kind == Some(WindowKind::Host) || self.windows.is_empty() {
                    return iced::exit();
                }
                Task::none()
            }

            Message::SearchQueryChanged(q) => {
                self.search_query = q;
                Task::none()
            }
            Message::SearchCaseToggled(v) => {
                self.search_case_sensitive = v;
                Task::none()
            }
            Message::SearchRegexToggled(v) => {
                self.search_regex = v;
                Task::none()
            }
            Message::RunFleetSearch => {
                let q = self.search_query.trim().to_string();
                if q.is_empty() {
                    return Task::none();
                }
                self.search_running = true;
                self.search_results.clear();
                let targets: Vec<(RepoId, String, PathBuf)> = self
                    .service
                    .workspace
                    .current()
                    .repos
                    .into_iter()
                    .filter(|r| r.available)
                    .map(|r| (r.id, r.name, r.path))
                    .collect();
                let req = SearchRequest {
                    query: q,
                    case_sensitive: self.search_case_sensitive,
                    regex: self.search_regex,
                    targets,
                };
                let svc = self.service.clone();
                Task::perform(
                    async move {
                        svc.search
                            .execute(req)
                            .await
                            .map(|(_, m)| m)
                            .unwrap_or_default()
                    },
                    Message::FleetSearchCompleted,
                )
            }
            Message::CancelFleetSearch => {
                self.service.search.cancel_all();
                self.search_running = false;
                Task::none()
            }
            Message::FleetSearchCompleted(matches) => {
                self.search_running = false;
                self.toast_is_error = false;
                self.toast_message = format!("Fleet Search found {} match(es)", matches.len());
                self.search_results = matches;
                Task::none()
            }
            Message::JumpToSearchMatch(repo_id, file_path) => {
                let mut tasks = Vec::new();
                if self.selected_repo.as_ref() != Some(&repo_id) {
                    tasks.push(self.update(Message::SelectRepo(repo_id)));
                }
                self.bottom_mode = BottomViewMode::Preview;
                tasks.push(self.update(Message::SelectRepoFile(file_path)));
                Task::batch(tasks)
            }
            Message::BackgroundTick => {
                if self.drawer_tab == Some(DrawerTab::Terminal) {
                    self.sync_terminal_snapshots();
                }
                if self.service.settings().watch_filesystem {
                    let refs_dirty = self.service.scheduler.take_git_refs_dirty();
                    let fs_dirty = self.service.scheduler.take_fs_dirty();
                    if (refs_dirty || fs_dirty)
                        && let Some(repo) = self.current_repo()
                    {
                        self.refresh_vault_summary();
                        let mut tasks = vec![self.load_status_task(repo.clone())];
                        if refs_dirty {
                            tasks.push(self.load_log_task(repo.clone()));
                            tasks.push(self.load_branches_task(repo.clone()));
                            tasks.push(self.load_tags_task(repo.clone()));
                            tasks.push(self.load_stashes_task(repo.clone()));
                            tasks.push(self.load_worktrees_task(repo.clone()));
                            tasks.push(self.load_submodules_task(repo.clone()));
                            if self.graph_tab == crate::app::GraphTab::Reflog {
                                tasks.push(self.load_reflog_task(repo.clone()));
                            }
                        }
                        if self.files_mode == crate::app::FilesViewMode::AllFiles {
                            tasks.push(self.load_all_files_task(repo.clone()));
                            if self.show_ignored_files {
                                self.ignored_entries_stale = false;
                                tasks.push(self.load_ignored_entries_task(repo));
                            }
                        } else {
                            self.ignored_entries_stale = true;
                        }
                        return Task::batch(tasks);
                    }
                }
                if self.service.scheduler.take_scheduled_fetch_due() {
                    let ws_repos = self.service.workspace.current().repos;
                    let name_map: std::collections::HashMap<RepoId, String> = ws_repos
                        .iter()
                        .map(|r| (r.id.clone(), r.name.clone()))
                        .collect();
                    let targets: Vec<(RepoId, PathBuf)> = ws_repos
                        .into_iter()
                        .filter(|r| r.available)
                        .map(|r| (r.id, r.path))
                        .collect();
                    if !targets.is_empty() {
                        let svc = self.service.clone();
                        let max_p = svc.settings().max_parallel_bulk_ops;
                        return Task::perform(
                            async move {
                                let results = svc
                                    .git
                                    .bulk_execute_cancellable(
                                        BulkOperation::Fetch,
                                        targets,
                                        max_p,
                                        Arc::new(AtomicBool::new(false)),
                                    )
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
                                (
                                    "Scheduled Fetch".to_string(),
                                    ok,
                                    failures.len(),
                                    failures,
                                )
                            },
                            |(label, ok, fail, failures)| Message::BulkOpDone {
                                label,
                                ok,
                                fail,
                                failures,
                                is_scheduled: true,
                            },
                        );
                    }
                }
                Task::none()
            }
            Message::ToggleSettingsModal => {
                self.show_palette = false;
                self.show_branch_popover = false;
                self.show_merge_modal = false;
                self.show_git_flow_modal = false;
                self.show_workspace_modal = false;
                self.show_eol_modal = false;
                self.show_encoding_modal = false;
                self.show_syntax_modal = false;
                self.show_terminal_menu = false;
                self.terminal_menu_submenu = TerminalMenuSubmenu::None;
                self.show_settings_modal = !self.show_settings_modal;
                if self.show_settings_modal {
                    self.settings_font_filter.clear();
                    self.detected_tools = self.service.scan_diff_tools();
                    let _ = self.service.terminal.refresh_profiles();
                }
                Task::none()
            }
            Message::SetSettingsTab(tab) => {
                self.settings_tab = tab;
                Task::none()
            }
            Message::SettingsFontFilterChanged(q) => {
                self.settings_font_filter = q;
                Task::none()
            }
            Message::SetMaxParallelBulkOps(n) => {
                let mut s = self.service.settings();
                s.max_parallel_bulk_ops = n.clamp(1, 16);
                self.service.update_settings(s);
                Task::none()
            }
            Message::RescanExternalTools => {
                self.detected_tools = self.service.scan_diff_tools();
                let _ = self.service.terminal.refresh_profiles();
                self.toast_is_error = false;
                self.toast_message = format!(
                    "Detected {} external tool(s) on PATH",
                    self.detected_tools.len()
                );
                Task::none()
            }
            Message::CloseErrorModal => {
                self.action_error = None;
                Task::none()
            }
            Message::OpenLastErrorModal => {
                if let Some(err) = self.last_action_error.clone() {
                    self.action_error = Some(err);
                }
                Task::none()
            }
            Message::OpenErrorCommandOutput => {
                self.action_error = None;
                self.drawer_tab = Some(DrawerTab::CommandLog);
                self.terminal_focused = false;
                Task::none()
            }
            Message::ToggleFsWatcher(enabled) => {
                let mut s = self.service.settings();
                s.watch_filesystem = enabled;
                self.service.update_settings(s);
                if enabled {
                    if let Some(repo) = self.current_repo() {
                        self.service.scheduler.watch_repo(&repo.path);
                    }
                } else {
                    self.service.scheduler.stop_watching();
                }
                Task::none()
            }
            Message::ToggleScheduledFetch(enabled) => {
                let mut s = self.service.settings();
                s.scheduled_fetch_enabled = enabled;
                self.service.update_settings(s);
                Task::none()
            }
            Message::SetFetchInterval(secs) => {
                let mut s = self.service.settings();
                s.auto_fetch_interval_secs = Some(secs);
                self.service.update_settings(s);
                Task::none()
            }
            Message::SetOversizedThreshold(mb) => {
                let mut s = self.service.settings();
                s.oversized_threshold_mb = mb;
                self.service.update_settings(s);
                Task::none()
            }
            Message::GitVersionLoaded(v) => {
                self.git_version = v;
                Task::none()
            }
            Message::PickExportDiagnostics => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_file_name("gitfleet-diagnostics.json")
                        .add_filter("JSON", &["json"])
                        .save_file()
                        .await
                        .map(|h| h.path().to_path_buf())
                },
                Message::ExportDiagnosticsPath,
            ),
            Message::ExportDiagnosticsPath(Some(path)) => {
                let json = self.service.build_diagnostics_json(&self.git_version);
                match std::fs::write(&path, json) {
                    Ok(_) => {
                        self.toast_is_error = false;
                        self.toast_message = format!("Exported diagnostics to {}", path.display());
                    }
                    Err(e) => {
                        self.report_action_error("Export Diagnostics Failed", e.to_string(), false);
                    }
                }
                Task::none()
            }
            Message::ExportDiagnosticsPath(None) => Task::none(),

            // ── Context Menus, Confirmations & Clipboard ──────────────────────────
            Message::CursorMoved(win_id, pos) => {
                if self.windows.get(&win_id) != Some(&WindowKind::Host) {
                    if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                        v.cursor_pos = pos;
                    }
                    return Task::none();
                }
                self.cursor_pos = pos;
                if self.dragging_repo.is_none()
                    && let Some((id, name, origin)) = &self.repo_drag_candidate
                {
                    let dx = pos.x - origin.x;
                    let dy = pos.y - origin.y;
                    if dx * dx + dy * dy >= 25.0 {
                        self.dragging_repo = Some((id.clone(), name.clone()));
                    }
                }
                if let Some(target) = self.active_resize {
                    let top_chrome = TITLEBAR_HEIGHT + TOOLBAR_HEIGHT;
                    let bottom_chrome = STATUS_BAR_HEIGHT
                        + if self.drawer_tab.is_some() {
                            self.drawer_height
                        } else {
                            0.0
                        };
                    let main_h = (self.window_size.height - top_chrome - bottom_chrome).max(200.0);
                    let main_w = self.window_size.width.max(640.0);

                    match target {
                        ResizeTarget::SidebarWidth => {
                            self.sidebar_width = pos.x.clamp(190.0, (main_w * 0.45).max(280.0));
                        }
                        ResizeTarget::LeftSplit => {
                            self.left_split_ratio = ((pos.y - top_chrome) / main_h).clamp(0.20, 0.80);
                        }
                        ResizeTarget::RightSplit => {
                            self.right_split_ratio =
                                ((pos.y - top_chrome) / main_h).clamp(0.20, 0.80);
                        }
                        ResizeTarget::TopRightSplit => {
                            let right_left = self.sidebar_width + 6.0;
                            let right_w = (main_w - right_left - 12.0).max(300.0);
                            let max_ratio = (1.0 - 185.0 / right_w).clamp(0.45, 0.78);
                            self.top_right_split_ratio =
                                ((pos.x - right_left) / right_w).clamp(0.22, max_ratio);
                        }
                        ResizeTarget::CommitFilesSplit => {
                            let top_right_top = top_chrome + 6.0;
                            let top_right_h = ((main_h - 12.0) * self.right_split_ratio).max(160.0);
                            let min_ratio = (145.0 / top_right_h).clamp(0.18, 0.50);
                            self.commit_files_split_ratio =
                                ((pos.y - top_right_top) / top_right_h).clamp(min_ratio, 0.76);
                        }
                        ResizeTarget::FilesStagedSplit => {
                            let top_right_top = top_chrome + 6.0;
                            let top_right_h = ((main_h - 12.0) * self.right_split_ratio).max(160.0);
                            let files_panel_top =
                                top_right_top + top_right_h * self.commit_files_split_ratio + 5.0;
                            let files_body_top = files_panel_top + 66.0;
                            let files_body_h = (top_right_h
                                * (1.0 - self.commit_files_split_ratio)
                                - 76.0)
                                .max(100.0);
                            self.files_staged_split_ratio =
                                ((pos.y - files_body_top) / files_body_h).clamp(0.22, 0.78);
                        }
                        ResizeTarget::DrawerHeight => {
                            if self.drawer_tab.is_some() {
                                let max_h = (self.window_size.height - top_chrome - 140.0).max(220.0);
                                let next_h = (self.window_size.height - STATUS_BAR_HEIGHT - pos.y)
                                    .clamp(crate::theme::layout::DOCK_MIN_HEIGHT, max_h);
                                self.drawer_height = next_h;
                                self.terminal_expanded = next_h >= 420.0;
                            }
                        }
                    }
                }
                Task::none()
            }
            Message::WindowResized(win_id, size) => {
                if self.windows.get(&win_id) == Some(&WindowKind::Host) {
                    self.window_size = size;
                }
                Task::none()
            }
            Message::ModifiersChanged(modifiers) => {
                self.keyboard_modifiers = modifiers;
                Task::none()
            }
            Message::WindowUnfocused => {
                self.keyboard_modifiers = Modifiers::empty();
                self.terminal_focused = false;
                Task::none()
            }
            Message::OpenContextMenu(target) => {
                if self.pending_confirm.is_some()
                    || self.action_error.is_some()
                    || self.cherry_pick_modal.is_some()
                    || self.discard_modal.is_some()
                    || self.rename_branch_modal.is_some()
                    || self.rename_workspace_modal.is_some()
                    || self.rename_group_modal.is_some()
                    || self.rename_entry_modal.is_some()
                    || self.add_gitignore_modal.is_some()
                    || self.scan_modal.is_some()
                {
                    return Task::none();
                }
                self.repo_drag_candidate = None;
                self.dragging_repo = None;
                self.drag_hover_group = None;
                self.context_menu = Some(ContextMenuState {
                    position: self.current_host_cursor_pos(),
                    target,
                    submenu: ContextMenuSubmenu::None,
                });
                Task::none()
            }
            Message::SetContextSubmenu(sub) => {
                if let Some(menu) = &mut self.context_menu {
                    menu.submenu = sub;
                }
                Task::none()
            }
            Message::CloseContextMenu => {
                self.context_menu = None;
                Task::none()
            }
            Message::ContextAction(inner) => {
                self.context_menu = None;
                self.update(*inner)
            }
            Message::RequestConfirm(confirm) => {
                self.context_menu = None;
                self.show_palette = false;
                self.pending_confirm = Some(confirm);
                Task::none()
            }
            Message::ConfirmCancelled => {
                self.pending_confirm = None;
                Task::none()
            }
            Message::ConfirmAccepted => {
                let Some(confirm) = self.pending_confirm.take() else {
                    return Task::none();
                };
                match confirm.action {
                    ConfirmAction::DiscardHunk { patch } => self.update(Message::ApplyHunk {
                        patch,
                        cached: false,
                        reverse: true,
                    }),
                    ConfirmAction::ResetCommit { oid, mode } => {
                        self.update(Message::ResetCommit(oid, mode))
                    }
                    ConfirmAction::RevertCommit { oid } => self.update(Message::RevertCommit(oid)),
                    ConfirmAction::AbortGitOp => self.update(Message::AbortGitOp),
                    ConfirmAction::DropStash { index } => self.update(Message::DropStash(index)),
                    ConfirmAction::DeleteTag { name } => self.update(Message::DeleteTag(name)),
                    ConfirmAction::DeleteBranches { names, force } => {
                        self.update(Message::DeleteBranches(names, force))
                    }
                    ConfirmAction::RemoveRepo { id } => self.update(Message::RemoveRepo(id)),
                    ConfirmAction::RemoveBulkSelected => self.update(Message::RemoveBulkSelected),
                    ConfirmAction::DeleteGroup { name } => self.update(Message::DeleteGroup(name)),
                    ConfirmAction::DeleteWorkspace { path } => {
                        self.update(Message::DeleteWorkspace(path))
                    }
                    ConfirmAction::RemoveSubmodule { path } => {
                        self.update(Message::RemoveSubmodule(path))
                    }
                    ConfirmAction::RemoveWorktree { path } => {
                        self.update(Message::RemoveWorktree(path))
                    }
                    ConfirmAction::DeleteVaultKey { key_id } => {
                        self.update(Message::DeleteVaultKey(key_id))
                    }
                    ConfirmAction::DestroyRepoVault => self.update(Message::DestroyRepoVault),
                    ConfirmAction::ForceCommit { push_after } => self.execute_commit(push_after),
                    ConfirmAction::CommitAllWorkingTree { push_after } => {
                        self.execute_commit_all(push_after)
                    }
                    ConfirmAction::ExecuteMerge { target, mode } => {
                        self.update(Message::ExecuteMerge(target, mode))
                    }
                    ConfirmAction::FastForwardMerge { branch } => {
                        self.update(Message::FastForwardMergeBranch(branch))
                    }
                    ConfirmAction::ExecuteRebase { target } => {
                        self.update(Message::ExecuteRebase(target))
                    }
                    ConfirmAction::CheckoutCommit { oid } => {
                        self.update(Message::CheckoutCommit(oid))
                    }
                    ConfirmAction::PushSelected => self.update(Message::PushSelected),
                    ConfirmAction::PushBranch { branch } => {
                        self.update(Message::PushBranch(branch))
                    }
                    ConfirmAction::PushTag { name } => self.update(Message::PushTag(name)),
                    ConfirmAction::UnsetUpstream { branch } => {
                        self.update(Message::UnsetBranchUpstream(branch))
                    }
                    ConfirmAction::ApplyStash { index } => self.update(Message::ApplyStash(index)),
                    ConfirmAction::PopStash { index } => self.update(Message::PopStash(index)),
                    ConfirmAction::StashSelected => self.update(Message::StashSelected),
                    ConfirmAction::UndoLastAction => self.update(Message::UndoLastAction),
                    ConfirmAction::BulkOp { op } => self.update(Message::RunBulkOp(op)),
                    ConfirmAction::UninitGitFlow => self.update(Message::UninitGitFlow),
                    ConfirmAction::AbortGitFlowBranch {
                        branch,
                        base_branch,
                        force,
                    } => self.update(Message::AbortGitFlowBranch {
                        branch,
                        base_branch,
                        force,
                    }),
                    ConfirmAction::DeleteRepoEntry { rel_path, is_dir } => {
                        self.update(Message::DeleteRepoEntry { rel_path, is_dir })
                    }
                    ConfirmAction::DeleteVaultProfile { profile } => {
                        self.update(Message::DeleteVaultProfile(profile))
                    }
                }
            }
            Message::OpenCherryPickModal { revisions, detail } => {
                self.context_menu = None;
                self.show_palette = false;
                if revisions.is_empty() {
                    return Task::none();
                }
                self.cherry_pick_modal = Some(CherryPickModalState {
                    revisions,
                    detail,
                    append_source_id: false,
                });
                Task::none()
            }
            Message::ToggleCherryPickAppendId(v) => {
                if let Some(m) = &mut self.cherry_pick_modal {
                    m.append_source_id = v;
                }
                Task::none()
            }
            Message::CloseCherryPickModal => {
                self.cherry_pick_modal = None;
                Task::none()
            }
            Message::ConfirmCherryPick { commit } => {
                let Some(modal) = self.cherry_pick_modal.take() else {
                    return Task::none();
                };
                let revs = modal.revisions;
                let no_commit = !commit;
                let append_id = modal.append_source_id;
                let op_label = if commit {
                    "Cherry-Pick & Commit"
                } else {
                    "Cherry-Pick (No Commit)"
                };
                self.run_git_op(op_label, move |svc, repo| async move {
                    if let Ok(prev) = svc.git.head_oid(&repo.path).await {
                        svc.undo.record(UndoEntry {
                            id: format!("{}", Utc::now().timestamp_millis()),
                            repo_id: repo.id.clone(),
                            label: format!("Cherry-pick {} ref(s)", revs.len()),
                            previous_head: prev,
                            created_at: Utc::now(),
                        });
                    }
                    svc.git
                        .cherry_pick_with_options(&repo.path, &revs, no_commit, append_id)
                        .await
                })
            }
            Message::OpenDiscardModal(preselect_paths) => {
                self.context_menu = None;
                self.show_palette = false;
                let Some(st) = self.active_status() else {
                    return Task::none();
                };
                let preselect_set: Option<HashSet<String>> =
                    preselect_paths.map(|v| v.into_iter().collect());
                let is_preselected = |path: &str| -> bool {
                    preselect_set
                        .as_ref()
                        .map(|set| set.contains(path))
                        .unwrap_or(true)
                };
                let mut by_path: BTreeMap<String, DiscardFileEntry> = BTreeMap::new();
                for f in &st.conflict_files {
                    by_path.insert(
                        f.path.clone(),
                        DiscardFileEntry {
                            path: f.path.clone(),
                            state_label: f.kind.state_label(false).to_string(),
                            is_untracked: false,
                            is_added_staged: false,
                            selected: is_preselected(&f.path),
                        },
                    );
                }
                for f in &st.staged_files {
                    by_path.entry(f.path.clone()).or_insert_with(|| DiscardFileEntry {
                        path: f.path.clone(),
                        state_label: f.kind.state_label(true).to_string(),
                        is_untracked: false,
                        is_added_staged: f.kind == FileChangeKind::Added,
                        selected: is_preselected(&f.path),
                    });
                }
                for f in &st.unstaged_files {
                    let is_untracked = f.kind == FileChangeKind::Untracked;
                    by_path
                        .entry(f.path.clone())
                        .and_modify(|e| {
                            if !e.state_label.contains("Modified") && !is_untracked {
                                e.state_label =
                                    format!("{} + {}", e.state_label, f.kind.state_label(false));
                            }
                        })
                        .or_insert_with(|| DiscardFileEntry {
                            path: f.path.clone(),
                            state_label: f.kind.state_label(false).to_string(),
                            is_untracked,
                            is_added_staged: false,
                            selected: is_preselected(&f.path),
                        });
                }
                let files: Vec<DiscardFileEntry> = by_path.into_values().collect();
                if files.is_empty() {
                    self.toast_is_error = false;
                    self.toast_message = "Working tree is clean — nothing to discard.".into();
                    return Task::none();
                }
                // Default to Index (revert working tree only, preserving staged changes) whenever
                // any selected file has unstaged working-tree edits; default to HEAD when only
                // staged-only / conflict entries are selected.
                let unstaged_paths: HashSet<&str> =
                    st.unstaged_files.iter().map(|f| f.path.as_str()).collect();
                let any_selected_unstaged = files
                    .iter()
                    .any(|f| f.selected && unstaged_paths.contains(f.path.as_str()));
                let default_target = if any_selected_unstaged {
                    DiscardTarget::Index
                } else {
                    DiscardTarget::Head
                };
                self.discard_modal = Some(DiscardModalState {
                    target: default_target,
                    files,
                });
                Task::none()
            }
            Message::SetDiscardTarget(target) => {
                if let Some(m) = &mut self.discard_modal {
                    m.target = target;
                }
                Task::none()
            }
            Message::ToggleDiscardFile(idx, selected) => {
                if let Some(m) = &mut self.discard_modal
                    && let Some(entry) = m.files.get_mut(idx)
                {
                    entry.selected = selected;
                }
                Task::none()
            }
            Message::ToggleDiscardAll(selected) => {
                if let Some(m) = &mut self.discard_modal {
                    for entry in &mut m.files {
                        entry.selected = selected;
                    }
                }
                Task::none()
            }
            Message::CloseDiscardModal => {
                self.discard_modal = None;
                Task::none()
            }
            Message::ConfirmDiscardModal => {
                let Some(modal) = self.discard_modal.take() else {
                    return Task::none();
                };
                let target = modal.target;
                let items: Vec<(String, bool, bool)> = modal
                    .files
                    .into_iter()
                    .filter(|f| f.selected)
                    .map(|f| (f.path, f.is_untracked, f.is_added_staged))
                    .collect();
                if items.is_empty() {
                    return Task::none();
                }
                let count = items.len();
                self.run_git_op("Discard Local Changes", move |svc, repo| async move {
                    svc.git
                        .discard_changes(&repo.path, &items, target)
                        .await
                        .map(|_| format!("Discarded {count} file(s)"))
                })
            }
            Message::CopyText(label, val) => {
                self.toast_is_error = false;
                self.toast_message = format!("Copied {label} to clipboard");
                clipboard::write(val)
            }
            Message::OpenInlineGroupInput(parent) => {
                self.show_inline_group_input = true;
                self.new_group_parent = parent;
                self.new_group_input.clear();
                text_input::focus(self.inline_group_input_id.clone())
            }
            Message::OpenMergeWithTarget(branch) => {
                self.show_palette = false;
                self.show_branch_popover = false;
                self.show_git_flow_modal = false;
                self.show_workspace_modal = false;
                self.show_settings_modal = false;
                self.show_merge_modal = true;
                self.update(Message::SelectMergeTarget(branch))
            }
            Message::ContextRepoGitOp(repo_id, op) => {
                let select_task = if self.selected_repo.as_ref() != Some(&repo_id) {
                    self.update(Message::SelectRepo(repo_id))
                } else {
                    Task::none()
                };
                let op_task = match op {
                    "refresh" => self.update(Message::RefreshSelected),
                    "fetch" => self.update(Message::FetchSelected),
                    "pull" => self.update(Message::PullSelected),
                    "push" => self.update(Message::PushSelected),
                    "terminal" => self.update(Message::TerminalNewTab(None)),
                    _ => Task::none(),
                };
                Task::batch([select_task, op_task])
            }
            Message::OpenFileInMode(file, mode) => {
                self.bottom_mode = mode;
                self.update(Message::SelectChangedFile(file))
            }
            Message::Noop => Task::none(),
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

    pub(crate) fn open_detached_viewer_window(
        &mut self,
        state: crate::views::detached_viewer::DetachedViewerState,
    ) -> Task<Message> {
        self.pending_detached_viewers.push_back(state);
        let (_id, task) = window::open(window::Settings {
            size: iced::Size::new(1180.0, 760.0),
            min_size: Some(iced::Size::new(720.0, 460.0)),
            icon: crate::app::app_window_icon(),
            ..Default::default()
        });
        task.map(Message::ViewerOpened)
    }
}
