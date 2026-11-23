use crate::app::{
    ActiveFocusPane, BottomViewMode, DiffGapExpandMode, DiffSelectionSide, DiffTextPos,
    DiffTextSelection, EncodingModalMode, FilesViewMode, GitFleetApp, GraphSelection, Message,
};
use crate::views::commit_files::apply_file_selection;
use crate::views::csv_diff::{compute_csv_diff, is_csv_diff_path};
use crate::views::diff_viewer::{
    diff_line_text_at, diff_row_count, extract_selected_diff_text, parse_and_cache_diff,
    word_bounds_at,
};
use crate::views::image_diff::{
    ImageDiffData, ImageDiffZoom, decode_image_diff_pair, is_image_diff_path,
    is_raster_image_path, is_svg_path,
};
use crate::views::overlays::file_ops_modal::{
    AddGitignoreModalState, RenameEntryModalState, collect_workspace_ignore_candidates,
    remap_rel_path, validate_rename_input,
};
use crate::{graph, preview};
use gitfleet_git::{FileChangeKind, ParsedDiff};
use iced::widget::{scrollable, text_editor, text_input};
use iced::{Task, clipboard};
use std::time::{Duration, Instant};

impl GitFleetApp {
    pub(crate) fn try_update_files_docs(&mut self, message: Message) -> Result<Task<Message>, Message> {
        let mut unhandled: Option<Message> = None;
        let task = (|| -> Task<Message> {
            match message {
            Message::CommitFilterChanged(q) => {
                self.focus_pane = ActiveFocusPane::TextInput;
                self.commit_filter = q;
                Task::none()
            }
            Message::SelectWorkingTree => {
                self.last_file_click = None;
                self.graph_selection = GraphSelection::WorkingTree;
                self.selected_dir_filter = None;
                self.selected_changed_file = None;
                self.selected_changed_files.clear();
                self.file_selection_anchor = None;
                self.focus_pane = ActiveFocusPane::FilesUnstaged;
                self.active_file_path = None;
                self.diff_title = "Select a file to view changes".into();
                self.clear_diff_parsed_and_cache();
                self.diff_image = None;
                self.diff_csv = None;
                self.diff_is_working_file = false;
                if matches!(
                    self.bottom_mode,
                    BottomViewMode::ImageDiff | BottomViewMode::CsvDiff
                ) {
                    self.bottom_mode = self.last_text_diff_mode;
                }
                if let Some(first) = self.active_changed_files().into_iter().next() {
                    return self.update(Message::SelectChangedFile(first));
                }
                Task::none()
            }
            Message::SelectCommit(oid) => {
                self.last_file_click = None;
                self.graph_selection = GraphSelection::Commit(oid.clone());
                self.selected_dir_filter = None;
                self.selected_changed_file = None;
                self.selected_changed_files.clear();
                self.file_selection_anchor = None;
                self.focus_pane = ActiveFocusPane::FilesCommit;
                self.active_file_path = None;
                self.diff_is_working_file = false;
                self.diff_title = "Loading commit files…".into();
                self.clear_diff_parsed_and_cache();
                self.diff_image = None;
                self.diff_csv = None;
                if let Some(repo) = self.current_repo() {
                    let svc = self.service.clone();
                    let path = repo.path;
                    return Task::perform(
                        async move {
                            let res = svc
                                .git
                                .get_commit_files(&path, &oid)
                                .await
                                .map_err(|e| e.to_string());
                            (oid, res)
                        },
                        |(o, r)| Message::CommitFilesLoaded(o, r),
                    );
                }
                Task::none()
            }
            Message::LogLoaded(id, res) => {
                if self.active_repo_key().as_ref() == Some(&id) {
                    match res {
                        Ok(list) => {
                            let (rows, max_lanes) = graph::compute_graph_layout(&list);
                            self.commits = list;
                            self.graph_rows = rows;
                            self.graph_max_lanes = max_lanes;
                        }
                        Err(_) => {
                            self.commits.clear();
                            self.graph_rows.clear();
                            self.graph_max_lanes = 1;
                        }
                    }
                }
                Task::none()
            }
            Message::CommitFilesLoaded(oid, res) => {
                if let Ok(files) = res {
                    self.commit_files = files;
                    // Only display diff of the selected file (auto-select first file of this commit)
                    if matches!(&self.graph_selection, GraphSelection::Commit(cur) if cur == &oid) {
                        if let Some(first) = self.commit_files.first().cloned() {
                            return self.update(Message::SelectChangedFile(first));
                        } else {
                            self.diff_title = "No files changed in this commit".into();
                            self.clear_diff_parsed_and_cache();
                        }
                    }
                }
                Task::none()
            }
            Message::SetFilesViewMode(mode) => {
                self.files_mode = mode;
                if mode == FilesViewMode::AllFiles
                    && let Some(repo) = self.current_repo()
                {
                    let mut tasks = Vec::new();
                    if self.all_repo_files.is_empty() || self.ignored_entries_stale {
                        tasks.push(self.load_all_files_task(repo.clone()));
                    }
                    if self.show_ignored_files
                        && (!self.ignored_entries_loaded || self.ignored_entries_stale)
                    {
                        self.ignored_entries_stale = false;
                        tasks.push(self.load_ignored_entries_task(repo));
                    }
                    if !tasks.is_empty() {
                        return Task::batch(tasks);
                    }
                }
                Task::none()
            }
            Message::ToggleShowIgnoredFiles => {
                self.show_ignored_files = !self.show_ignored_files;
                self.persist_ui_prefs();
                if self.show_ignored_files {
                    if let Some(repo) = self.current_repo() {
                        self.ignored_entries_stale = false;
                        return self.load_ignored_entries_task(repo);
                    }
                } else if self.is_path_in_ignored_dir(&self.selected_all_dir)
                    || (!self.selected_all_dir.is_empty() && {
                        let prefix = format!("{}/", self.selected_all_dir);
                        !self.all_repo_files.iter().any(|f| f.starts_with(&prefix))
                    })
                {
                    self.selected_all_dir.clear();
                }
                Task::none()
            }
            Message::FileFilterChanged(q) => {
                self.focus_pane = ActiveFocusPane::TextInput;
                self.file_filter = q;
                Task::none()
            }
            Message::SelectDirFilter(dir) => {
                self.selected_dir_filter = dir;
                Task::none()
            }
            Message::SelectAllFilesDir(dir) => {
                if !dir.is_empty() {
                    self.expanded_all_dirs.insert(dir.clone());
                }
                self.selected_all_dir = dir.clone();
                if self.show_ignored_files
                    && !dir.is_empty()
                    && self.is_path_in_ignored_dir(&dir)
                    && !self.ignored_dir_children.contains_key(&dir)
                    && let Some(repo) = self.current_repo()
                {
                    return self.load_ignored_dir_children_task(repo, dir);
                }
                Task::none()
            }
            Message::ToggleAllFilesDirNode(dir) => {
                let expanded = if self.expanded_all_dirs.remove(&dir) {
                    false
                } else {
                    self.expanded_all_dirs.insert(dir.clone());
                    true
                };
                if expanded
                    && self.show_ignored_files
                    && !dir.is_empty()
                    && self.is_path_in_ignored_dir(&dir)
                    && !self.ignored_dir_children.contains_key(&dir)
                    && let Some(repo) = self.current_repo()
                {
                    return self.load_ignored_dir_children_task(repo, dir);
                }
                Task::none()
            }
            Message::AllFilesLoaded(id, res) => {
                if self.active_repo_key().as_ref() == Some(&id) {
                    self.all_repo_files = res.unwrap_or_default();
                }
                Task::none()
            }
            Message::IgnoredEntriesLoaded(id, res) => {
                if self.active_repo_key().as_ref() == Some(&id) {
                    self.ignored_entries_loaded = true;
                    self.ignored_entries_stale = false;
                    let entries = res.unwrap_or_default();
                    self.ignored_repo_files = entries.files;
                    self.ignored_repo_dirs = entries.dirs;

                    let mut dirs_to_reload: Vec<String> = self
                        .ignored_dir_children
                        .keys()
                        .filter(|d| self.is_path_in_ignored_dir(d))
                        .cloned()
                        .collect();
                    if !self.selected_all_dir.is_empty()
                        && self.is_path_in_ignored_dir(&self.selected_all_dir)
                        && !dirs_to_reload.contains(&self.selected_all_dir)
                    {
                        dirs_to_reload.push(self.selected_all_dir.clone());
                    }
                    self.ignored_dir_children.clear();

                    if !dirs_to_reload.is_empty()
                        && let Some(repo) = self.current_repo()
                    {
                        let tasks = dirs_to_reload
                            .into_iter()
                            .map(|d| self.load_ignored_dir_children_task(repo.clone(), d));
                        return Task::batch(tasks);
                    }
                }
                Task::none()
            }
            Message::IgnoredDirChildrenLoaded(id, rel_dir, res) => {
                if self.active_repo_key().as_ref() == Some(&id) {
                    self.ignored_dir_children
                        .insert(rel_dir, res.unwrap_or_default());
                }
                Task::none()
            }
            Message::ClickChangedFile(file) => {
                // Match Windows native 500ms press-to-press double-click interval (+50ms frame margin)
                const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(550);
                let multi_mod =
                    self.keyboard_modifiers.shift() || self.keyboard_modifiers.command();
                let click_key = format!("changed:{}:{}", file.staged, file.path);
                let now = Instant::now();
                let is_double = !multi_mod
                    && self.last_file_click.as_ref().is_some_and(|(prev, ts)| {
                        prev == &click_key && now.duration_since(*ts) <= DOUBLE_CLICK_WINDOW
                    });
                if is_double {
                    self.last_file_click = None;
                    return self.update(Message::OpenDetachedForChangedFile(file));
                }
                self.last_file_click = if multi_mod {
                    None
                } else {
                    Some((click_key, now))
                };
                self.update(Message::SelectChangedFile(file))
            }
            Message::ClickRepoFile(rel_path) => {
                const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(550);
                let click_key = format!("repo:{rel_path}");
                let now = Instant::now();
                let is_double = self.last_file_click.as_ref().is_some_and(|(prev, ts)| {
                    prev == &click_key && now.duration_since(*ts) <= DOUBLE_CLICK_WINDOW
                });
                if is_double {
                    self.last_file_click = None;
                    return self.update(Message::OpenDetachedForRepoFile(rel_path));
                }
                self.last_file_click = Some((click_key, now));
                self.update(Message::SelectRepoFile(rel_path))
            }
            Message::SelectChangedFile(file) => {
                self.terminal_focused = false;
                self.focus_pane = if self.graph_selection == GraphSelection::WorkingTree {
                    if file.staged {
                        ActiveFocusPane::FilesStaged
                    } else {
                        ActiveFocusPane::FilesUnstaged
                    }
                } else {
                    ActiveFocusPane::FilesCommit
                };
                let dir_filter = self.current_files_dir_filter().map(str::to_string);
                let visible = self.visible_files_for_section(file.staged, dir_filter.as_deref());
                apply_file_selection(
                    &mut self.selected_changed_files,
                    &mut self.file_selection_anchor,
                    &visible,
                    &file,
                    self.keyboard_modifiers.shift(),
                    self.keyboard_modifiers.command(),
                );
                self.selected_changed_file = Some(file.clone());
                let doc_task = self.open_file_for_preview_and_edit_async(&file.path);

                let is_img = is_image_diff_path(&file.path);
                let is_raster = is_raster_image_path(&file.path);
                let is_csv = is_csv_diff_path(&file.path);

                self.diff_request_id = self.diff_request_id.wrapping_add(1);
                let req_id = self.diff_request_id;
                self.diff_image = None;
                self.diff_csv = None;
                self.image_diff_swapped = false;
                self.image_diff_zoom = ImageDiffZoom::Fit;
                self.image_diff_swipe = 0.5;
                self.image_diff_blink_after = false;
                self.csv_diff_filter.clear();

                let is_conflict = self.graph_selection == GraphSelection::WorkingTree
                    && (file.kind == FileChangeKind::Conflicted
                        || self
                            .active_status()
                            .is_some_and(|st| st.conflict_files.iter().any(|f| f.path == file.path)));
                if is_conflict {
                    self.bottom_mode = BottomViewMode::ResolveConflicts;
                } else {
                    let in_any_diff = matches!(
                        self.bottom_mode,
                        BottomViewMode::UnifiedDiff
                            | BottomViewMode::SideBySideDiff
                            | BottomViewMode::ImageDiff
                            | BottomViewMode::CsvDiff
                            | BottomViewMode::ResolveConflicts
                    );
                    if is_img && (in_any_diff || (is_raster && self.bottom_mode == BottomViewMode::Edit)) {
                        self.bottom_mode = BottomViewMode::ImageDiff;
                    } else if is_csv && in_any_diff {
                        self.bottom_mode = if self.csv_prefer_table {
                            BottomViewMode::CsvDiff
                        } else {
                            self.last_text_diff_mode
                        };
                    } else if !is_img
                        && !is_csv
                        && matches!(
                            self.bottom_mode,
                            BottomViewMode::ImageDiff
                                | BottomViewMode::CsvDiff
                                | BottomViewMode::ResolveConflicts
                        )
                    {
                        self.bottom_mode = self.last_text_diff_mode;
                    }
                }

                let Some(repo) = self.current_repo() else {
                    return doc_task;
                };

                match &self.graph_selection {
                    GraphSelection::WorkingTree => Task::batch([
                        doc_task,
                        self.load_working_file_diff_tasks(repo.path.clone(), &file, req_id),
                    ]),
                    GraphSelection::Commit(oid) => {
                        self.diff_is_working_file = false;
                        self.diff_is_staged = false;
                        let oid = oid.clone();
                        let title = format!(
                            "{} @ {}",
                            file.path,
                            &oid[..oid.len().min(8)]
                        );
                        self.diff_title = title.clone();
                        let mut tasks = vec![doc_task];

                        if !is_raster {
                            let svc = self.service.clone();
                            let repo_path = repo.path.clone();
                            let fpath = file.path.clone();
                            let o = oid.clone();
                            let t = title.clone();
                            let compact = self.diff_compact;
                            let lang = self.open_doc_language;
                            let theme_mode = self.theme_mode;
                            tasks.push(Task::perform(
                                async move {
                                    let res = match svc
                                        .git
                                        .get_commit_file_diff(&repo_path, &o, &fpath)
                                        .await
                                    {
                                        Ok(raw) => tokio::task::spawn_blocking(move || {
                                            parse_and_cache_diff(&raw, compact, lang, theme_mode)
                                        })
                                        .await
                                        .map_err(|e| format!("Diff parse task failed: {e}")),
                                        Err(e) => Err(e.to_string()),
                                    };
                                    (req_id, t, res)
                                },
                                |(id, t, r)| Message::DiffLoaded(id, t, r),
                            ));
                        } else {
                            self.clear_diff_parsed_and_cache();
                        }

                        if is_img {
                            let svc = self.service.clone();
                            let repo_path = repo.path.clone();
                            let fpath = file.path.clone();
                            let o = oid.clone();
                            let t = title.clone();
                            tasks.push(Task::perform(
                                async move {
                                    let data = match svc
                                        .git
                                        .get_commit_file_versions(&repo_path, &o, &fpath)
                                        .await
                                    {
                                        Ok(pair) => tokio::task::spawn_blocking(move || {
                                            decode_image_diff_pair(pair)
                                        })
                                        .await
                                        .unwrap_or_else(|e| ImageDiffData {
                                            file_path: fpath.clone(),
                                            is_svg: is_svg_path(&fpath),
                                            before_label: "Parent".into(),
                                            after_label: "Commit".into(),
                                            before: None,
                                            after: None,
                                            diff_handle: None,
                                            diff_width: 0,
                                            diff_height: 0,
                                            diff_changed_pixels: 0,
                                            diff_total_pixels: 0,
                                            load_error: Some(format!("Decode task failed: {e}")),
                                        }),
                                        Err(e) => ImageDiffData {
                                            file_path: fpath.clone(),
                                            is_svg: is_svg_path(&fpath),
                                            before_label: "Parent".into(),
                                            after_label: "Commit".into(),
                                            before: None,
                                            after: None,
                                            diff_handle: None,
                                            diff_width: 0,
                                            diff_height: 0,
                                            diff_changed_pixels: 0,
                                            diff_total_pixels: 0,
                                            load_error: Some(e.to_string()),
                                        },
                                    };
                                    (req_id, t, data)
                                },
                                |(id, t, d)| Message::ImageDiffLoaded(id, t, d),
                            ));
                        } else if is_csv {
                            let svc = self.service.clone();
                            let repo_path = repo.path.clone();
                            let fpath = file.path.clone();
                            let o = oid.clone();
                            let t = title.clone();
                            tasks.push(Task::perform(
                                async move {
                                    let pair = svc
                                        .git
                                        .get_commit_file_versions(&repo_path, &o, &fpath)
                                        .await
                                        .unwrap_or(gitfleet_git::FileVersionPair {
                                            file_path: fpath,
                                            before_label: "Parent".into(),
                                            after_label: "Commit".into(),
                                            before_bytes: None,
                                            after_bytes: None,
                                        });
                                    let data = tokio::task::spawn_blocking(move || {
                                        compute_csv_diff(pair)
                                    })
                                    .await
                                    .unwrap_or_else(|_| {
                                        compute_csv_diff(gitfleet_git::FileVersionPair {
                                            file_path: String::new(),
                                            before_label: "Parent".into(),
                                            after_label: "Commit".into(),
                                            before_bytes: None,
                                            after_bytes: None,
                                        })
                                    });
                                    (req_id, t, data)
                                },
                                |(id, t, d)| Message::CsvDiffLoaded(id, t, d),
                            ));
                        }

                        Task::batch(tasks)
                    }
                }
            }
            Message::OpenRepoFileInMode(rel_path, mode) => {
                self.bottom_mode = mode;
                self.update(Message::SelectRepoFile(rel_path))
            }
            Message::DeleteRepoEntry { rel_path, is_dir } => {
                let Some(repo) = self.current_repo() else {
                    return Task::none();
                };
                self.busy_label = Some(format!(
                    "Deleting {}…",
                    if is_dir { "folder" } else { "file" }
                ));
                let repo_id = repo.id;
                let repo_path = repo.path;
                let rel_for_task = rel_path.clone();
                Task::perform(
                    async move {
                        let result = tokio::task::spawn_blocking(move || {
                            gitfleet_git::delete_workspace_entry(&repo_path, &rel_for_task)
                                .map_err(|e| e.to_string())
                        })
                        .await
                        .unwrap_or_else(|e| Err(format!("Delete task failed: {e}")));
                        (repo_id, rel_path, is_dir, result)
                    },
                    |(repo_id, rel_path, is_dir, result)| Message::RepoEntryDeleted {
                        repo_id,
                        rel_path,
                        is_dir,
                        result,
                    },
                )
            }
            Message::RepoEntryDeleted {
                repo_id,
                rel_path,
                is_dir,
                result,
            } => {
                self.busy_label = None;
                let kind_label = if is_dir { "folder" } else { "file" };
                match result {
                    Ok(removed) => {
                        let matches_deleted = |candidate: &str| -> bool {
                            candidate == rel_path
                                || (is_dir
                                    && candidate
                                        .strip_prefix(rel_path.as_str())
                                        .is_some_and(|rest| rest.starts_with('/')))
                        };

                        if self.active_repo_key().as_ref() == Some(&repo_id) {
                            if self
                                .active_file_path
                                .as_deref()
                                .is_some_and(&matches_deleted)
                            {
                                self.active_file_path = None;
                                self.selected_changed_file = None;
                                self.selected_changed_files.clear();
                                self.file_selection_anchor = None;
                                self.open_doc_raw_text = None;
                                self.open_doc_id = None;
                                self.open_doc_version = None;
                                self.open_doc_path = None;
                                self.editor_dirty = false;
                                self.preview_hydrated = false;
                                self.editor_hydrated = false;
                                self.preview_state = preview::PreviewState::Empty;
                                self.editor_content = text_editor::Content::new();
                                self.diff_title = "No file selected".into();
                                self.clear_diff_parsed_and_cache();
                                self.diff_image = None;
                                self.diff_csv = None;
                            }
                            if matches_deleted(&self.selected_all_dir) {
                                self.selected_all_dir =
                                    crate::views::parent_dir(&rel_path).to_string();
                            }
                            self.expanded_all_dirs.retain(|d| !matches_deleted(d));
                            self.ignored_dir_children.retain(|d, _| !matches_deleted(d));
                        }

                        // Sync any open GitFleet Studio windows rooted inside the same repository
                        let mut studio_tasks = Vec::new();
                        if let Some(repo) = self.current_repo().filter(|r| r.id == repo_id) {
                            let abs_deleted = repo.path.join(&rel_path);
                            let affected_wins: Vec<iced::window::Id> = self
                                .detached_viewers
                                .iter_mut()
                                .filter_map(|(wid, v)| {
                                    let same_root = v
                                        .search_root
                                        .as_deref()
                                        .is_some_and(|sr| {
                                            sr == repo.path || abs_deleted.starts_with(sr)
                                        });
                                    if !same_root {
                                        return None;
                                    }
                                    v.close_tabs_under_path(&abs_deleted, is_dir);
                                    Some(*wid)
                                })
                                .collect();
                            for wid in affected_wins {
                                studio_tasks.push(self.spawn_explorer_scan_task(wid));
                            }
                        }

                        self.toast_is_error = false;
                        self.toast_message = if removed {
                            format!("Deleted {kind_label} '{rel_path}'")
                        } else {
                            format!("'{rel_path}' was already removed")
                        };
                        let mut tasks = vec![self.update(Message::RefreshSelected)];
                        tasks.extend(studio_tasks);
                        return Task::batch(tasks);
                    }
                    Err(e) => {
                        self.report_action_error(
                            format!(
                                "Delete {} Failed",
                                if is_dir { "Folder" } else { "File" }
                            ),
                            e,
                            false,
                        );
                    }
                }
                Task::none()
            }
            Message::OpenRenameRepoEntry { rel_path, is_dir } => {
                self.context_menu = None;
                self.show_palette = false;
                self.add_gitignore_modal = None;
                self.rename_entry_modal = Some(RenameEntryModalState::new(rel_path, is_dir));
                text_input::focus(self.rename_entry_input_id.clone())
            }
            Message::RenameRepoEntryInputChanged(val) => {
                if let Some(m) = &mut self.rename_entry_modal {
                    m.new_name = val;
                    m.error = None;
                }
                Task::none()
            }
            Message::SubmitRenameRepoEntry => {
                let Some(repo) = self.current_repo() else {
                    return Task::none();
                };
                let Some(modal) = &mut self.rename_entry_modal else {
                    return Task::none();
                };
                match validate_rename_input(&modal.rel_path, &modal.new_name) {
                    Ok(true) => {}
                    Ok(false) => {
                        self.rename_entry_modal = None;
                        return Task::none();
                    }
                    Err(err) => {
                        modal.error = Some(err);
                        return Task::none();
                    }
                }
                let old_path = modal.rel_path.clone();
                let new_name = modal.new_name.trim().to_string();
                let is_dir = modal.is_dir;
                self.busy_label = Some(format!(
                    "Renaming {}…",
                    if is_dir { "folder" } else { "file" }
                ));
                let repo_id = repo.id;
                let repo_path = repo.path;
                let rel_for_task = old_path.clone();
                Task::perform(
                    async move {
                        let result = tokio::task::spawn_blocking(move || {
                            gitfleet_git::rename_workspace_entry(
                                &repo_path,
                                &rel_for_task,
                                &new_name,
                            )
                            .map_err(|e| e.to_string())
                        })
                        .await
                        .unwrap_or_else(|e| Err(format!("Rename task failed: {e}")));
                        (repo_id, old_path, is_dir, result)
                    },
                    |(repo_id, old_path, is_dir, result)| Message::RepoEntryRenamed {
                        repo_id,
                        old_path,
                        is_dir,
                        result,
                    },
                )
            }
            Message::RepoEntryRenamed {
                repo_id,
                old_path,
                is_dir,
                result,
            } => {
                self.busy_label = None;
                let kind_label = if is_dir { "folder" } else { "file" };
                match result {
                    Ok(new_rel) => {
                        self.rename_entry_modal = None;
                        let active_repo = self.current_repo().filter(|r| r.id == repo_id);

                        if self.active_repo_key().as_ref() == Some(&repo_id) {
                            if let Some(cur) = self.active_file_path.clone()
                                && let Some(updated_rel) =
                                    remap_rel_path(&cur, &old_path, &new_rel, is_dir)
                            {
                                self.active_file_path = Some(updated_rel.clone());
                                self.diff_title = updated_rel.clone();
                                if let Some(repo) = &active_repo {
                                    let new_abs = repo.path.join(&updated_rel);
                                    self.open_doc_path = Some(new_abs.clone());
                                    self.open_doc_language =
                                        crate::syntax::CodeLanguage::from_path(&new_abs);
                                    if let Ok(cap) = self.service.docs.open_document(&new_abs) {
                                        self.open_doc_id = Some(cap.document_id);
                                        self.open_doc_version = Some(cap.version);
                                        if !self.editor_dirty {
                                            self.open_doc_raw_text = Some(cap.content);
                                        }
                                    }
                                }
                            }
                            if let Some(updated_dir) =
                                remap_rel_path(&self.selected_all_dir, &old_path, &new_rel, is_dir)
                            {
                                self.selected_all_dir = updated_dir;
                            }
                            self.expanded_all_dirs = self
                                .expanded_all_dirs
                                .iter()
                                .map(|d| {
                                    remap_rel_path(d, &old_path, &new_rel, is_dir)
                                        .unwrap_or_else(|| d.clone())
                                })
                                .collect();
                            self.ignored_dir_children.clear();
                            self.ignored_entries_stale = !self.show_ignored_files;
                        }

                        // Sync any open GitFleet Studio windows rooted inside the same repository
                        let mut studio_tasks = Vec::new();
                        if let Some(repo) = active_repo {
                            let old_abs = repo.path.join(&old_path);
                            let new_abs = repo.path.join(&new_rel);
                            let docs = self.service.clone();
                            let affected_wins: Vec<iced::window::Id> = self
                                .detached_viewers
                                .iter_mut()
                                .filter_map(|(wid, v)| {
                                    let same_root = v.search_root.as_deref().is_some_and(|sr| {
                                        sr == repo.path || old_abs.starts_with(sr)
                                    });
                                    if !same_root {
                                        return None;
                                    }
                                    v.rename_tabs_under_path(
                                        &docs.docs,
                                        &old_abs,
                                        &new_abs,
                                        is_dir,
                                    );
                                    if v.search_root.as_deref() == Some(repo.path.as_path()) {
                                        v.explorer_expanded_dirs = v
                                            .explorer_expanded_dirs
                                            .iter()
                                            .map(|d| {
                                                remap_rel_path(d, &old_path, &new_rel, is_dir)
                                                    .unwrap_or_else(|| d.clone())
                                            })
                                            .collect();
                                        v.explorer_ignored_dir_children.clear();
                                    }
                                    Some(*wid)
                                })
                                .collect();
                            for wid in affected_wins {
                                studio_tasks.push(self.spawn_explorer_scan_task(wid));
                            }
                        }

                        self.toast_is_error = false;
                        self.toast_message =
                            format!("Renamed {kind_label} '{old_path}' → '{new_rel}'");
                        let mut tasks = vec![self.update(Message::RefreshSelected)];
                        tasks.extend(studio_tasks);
                        return Task::batch(tasks);
                    }
                    Err(e) => {
                        if let Some(m) = &mut self.rename_entry_modal {
                            m.error = Some(e);
                        } else {
                            self.report_action_error(
                                format!(
                                    "Rename {} Failed",
                                    if is_dir { "Folder" } else { "File" }
                                ),
                                e,
                                false,
                            );
                        }
                    }
                }
                Task::none()
            }
            Message::OpenAddGitignoreModal { rel_path, is_dir } => {
                self.context_menu = None;
                self.show_palette = false;
                self.rename_entry_modal = None;
                let candidates = collect_workspace_ignore_candidates(
                    &self.all_repo_files,
                    &self.ignored_repo_files,
                    &self.ignored_repo_dirs,
                    &self.ignored_dir_children,
                );
                self.add_gitignore_modal = Some(AddGitignoreModalState::new(
                    rel_path,
                    is_dir,
                    true,
                    candidates,
                ));
                text_input::focus(self.add_gitignore_input_id.clone())
            }
            Message::AddGitignoreModeChanged(use_regex) => {
                if let Some(m) = &mut self.add_gitignore_modal {
                    m.use_regex = use_regex;
                    m.error = None;
                    if use_regex {
                        m.recompute_regex_preview();
                    }
                }
                text_input::focus(self.add_gitignore_input_id.clone())
            }
            Message::AddGitignoreGlobChanged(val) => {
                if let Some(m) = &mut self.add_gitignore_modal {
                    m.glob_pattern = val;
                    m.error = None;
                }
                Task::none()
            }
            Message::AddGitignoreRegexChanged(val) => {
                if let Some(m) = &mut self.add_gitignore_modal {
                    m.regex_pattern = val;
                    m.error = None;
                    m.recompute_regex_preview();
                }
                Task::none()
            }
            Message::AddGitignoreExpandMatchesToggled(expand) => {
                if let Some(m) = &mut self.add_gitignore_modal {
                    m.regex_expand_matches = expand;
                    m.error = None;
                    m.recompute_regex_preview();
                }
                Task::none()
            }
            Message::AddGitignoreLayerChanged(layer) => {
                if let Some(m) = &mut self.add_gitignore_modal {
                    m.layer = layer;
                    m.error = None;
                }
                Task::none()
            }
            Message::SubmitAddGitignore => {
                let Some(repo) = self.current_repo() else {
                    return Task::none();
                };
                let Some(modal) = self.add_gitignore_modal.as_ref() else {
                    return Task::none();
                };
                let rules = modal.resolved_rules();
                let layer = modal.layer;
                let target_label = modal.target_file_label();
                if rules.is_empty() {
                    if let Some(m) = &mut self.add_gitignore_modal {
                        m.error = Some("No valid ignore pattern to append.".into());
                    }
                    return Task::none();
                }

                match self
                    .service
                    .security
                    .append_ignore_rules(&repo.path, layer, &rules)
                {
                    Ok(res) => {
                        self.add_gitignore_modal = None;
                        self.refresh_vault_and_ignore();
                        self.ignored_entries_stale = !self.show_ignored_files;
                        self.toast_is_error = false;
                        self.toast_message = if res.added == 0 {
                            format!("Pattern already exists in {target_label}")
                        } else if res.skipped_existing > 0 {
                            format!(
                                "Added {} rule(s) to {target_label} ({} already present)",
                                res.added, res.skipped_existing
                            )
                        } else {
                            format!(
                                "Added {} rule{} to {target_label}",
                                res.added,
                                if res.added == 1 { "" } else { "s" }
                            )
                        };

                        let mut tasks = vec![self.update(Message::RefreshSelected)];
                        let affected_wins: Vec<iced::window::Id> = self
                            .detached_viewers
                            .iter()
                            .filter_map(|(wid, v)| {
                                v.search_root
                                    .as_deref()
                                    .is_some_and(|sr| sr == repo.path || sr.starts_with(&repo.path))
                                    .then_some(*wid)
                            })
                            .collect();
                        for wid in affected_wins {
                            tasks.push(self.spawn_explorer_scan_task(wid));
                        }
                        return Task::batch(tasks);
                    }
                    Err(e) => {
                        if let Some(m) = &mut self.add_gitignore_modal {
                            m.error = Some(e.to_string());
                        }
                    }
                }
                Task::none()
            }
            Message::SelectRepoFile(rel_path) => {
                self.terminal_focused = false;
                self.selected_changed_file = None;
                self.selected_changed_files.clear();
                self.file_selection_anchor = None;
                let doc_task = self.open_file_for_preview_and_edit_async(&rel_path);
                let Some(repo) = self.current_repo() else {
                    return doc_task;
                };
                // Also load diff if file has working tree changes; default to Preview if clean
                let is_dirty = self
                    .selected_repo
                    .as_ref()
                    .and_then(|id| self.statuses.get(id))
                    .is_some_and(|st| {
                        st.staged_files.iter().any(|f| f.path == rel_path)
                            || st.unstaged_files.iter().any(|f| f.path == rel_path)
                    });
                let is_ignored = !is_dirty && self.is_ignored_repo_file(&rel_path);

                let is_img = is_image_diff_path(&rel_path);
                let is_raster = is_raster_image_path(&rel_path);
                let is_csv = is_csv_diff_path(&rel_path);
                self.diff_request_id = self.diff_request_id.wrapping_add(1);
                let req_id = self.diff_request_id;
                self.diff_image = None;
                self.diff_csv = None;
                self.csv_diff_filter.clear();

                if !is_dirty
                    && matches!(
                        self.bottom_mode,
                        BottomViewMode::UnifiedDiff
                            | BottomViewMode::SideBySideDiff
                            | BottomViewMode::ImageDiff
                            | BottomViewMode::CsvDiff
                            | BottomViewMode::ResolveConflicts
                    )
                {
                    self.bottom_mode = BottomViewMode::Preview;
                } else if is_dirty {
                    if is_img
                        && matches!(
                            self.bottom_mode,
                            BottomViewMode::UnifiedDiff
                                | BottomViewMode::SideBySideDiff
                                | BottomViewMode::CsvDiff
                        )
                    {
                        self.bottom_mode = BottomViewMode::ImageDiff;
                    } else if is_csv
                        && self.csv_prefer_table
                        && matches!(
                            self.bottom_mode,
                            BottomViewMode::UnifiedDiff
                                | BottomViewMode::SideBySideDiff
                                | BottomViewMode::ImageDiff
                        )
                    {
                        self.bottom_mode = BottomViewMode::CsvDiff;
                    } else if !is_img
                        && !is_csv
                        && matches!(
                            self.bottom_mode,
                            BottomViewMode::ImageDiff | BottomViewMode::CsvDiff
                        )
                    {
                        self.bottom_mode = self.last_text_diff_mode;
                    }
                }

                self.diff_is_working_file = is_dirty;
                self.diff_is_staged = false;
                if is_ignored {
                    self.diff_title = format!("{rel_path} (Ignored)");
                    self.clear_diff_parsed_and_cache();
                    return doc_task;
                }
                let title = rel_path.clone();
                let mut tasks = vec![doc_task];

                if !is_raster {
                    let svc = self.service.clone();
                    let repo_path = repo.path.clone();
                    let rp = rel_path.clone();
                    let t = title.clone();
                    let compact = self.diff_compact;
                    let lang = self.open_doc_language;
                    let theme_mode = self.theme_mode;
                    tasks.push(Task::perform(
                        async move {
                            let res = match svc.git.get_diff(&repo_path, &rp, false, false).await {
                                Ok(raw) => tokio::task::spawn_blocking(move || {
                                    parse_and_cache_diff(&raw, compact, lang, theme_mode)
                                })
                                .await
                                .map_err(|e| format!("Diff parse task failed: {e}")),
                                Err(e) => Err(e.to_string()),
                            };
                            (req_id, t, res)
                        },
                        |(id, t, r)| Message::DiffLoaded(id, t, r),
                    ));
                }

                if is_img {
                    let svc = self.service.clone();
                    let repo_path = repo.path.clone();
                    let rp = rel_path.clone();
                    let t = title.clone();
                    tasks.push(Task::perform(
                        async move {
                            let data = match svc
                                .git
                                .get_working_file_versions(&repo_path, &rp, false, false)
                                .await
                            {
                                Ok(pair) => tokio::task::spawn_blocking(move || {
                                    decode_image_diff_pair(pair)
                                })
                                .await
                                .unwrap_or_else(|e| ImageDiffData {
                                    file_path: rp.clone(),
                                    is_svg: is_svg_path(&rp),
                                    before_label: "Index".into(),
                                    after_label: "Working Tree".into(),
                                    before: None,
                                    after: None,
                                    diff_handle: None,
                                    diff_width: 0,
                                    diff_height: 0,
                                    diff_changed_pixels: 0,
                                    diff_total_pixels: 0,
                                    load_error: Some(format!("Decode task failed: {e}")),
                                }),
                                Err(e) => ImageDiffData {
                                    file_path: rp.clone(),
                                    is_svg: is_svg_path(&rp),
                                    before_label: "Index".into(),
                                    after_label: "Working Tree".into(),
                                    before: None,
                                    after: None,
                                    diff_handle: None,
                                    diff_width: 0,
                                    diff_height: 0,
                                    diff_changed_pixels: 0,
                                    diff_total_pixels: 0,
                                    load_error: Some(e.to_string()),
                                },
                            };
                            (req_id, t, data)
                        },
                        |(id, t, d)| Message::ImageDiffLoaded(id, t, d),
                    ));
                } else if is_csv {
                    let svc = self.service.clone();
                    let repo_path = repo.path.clone();
                    let rp = rel_path.clone();
                    let t = title.clone();
                    tasks.push(Task::perform(
                        async move {
                            let pair = svc
                                .git
                                .get_working_file_versions(&repo_path, &rp, false, false)
                                .await
                                .unwrap_or(gitfleet_git::FileVersionPair {
                                    file_path: rp,
                                    before_label: "Index".into(),
                                    after_label: "Working Tree".into(),
                                    before_bytes: None,
                                    after_bytes: None,
                                });
                            let data =
                                tokio::task::spawn_blocking(move || compute_csv_diff(pair))
                                    .await
                                    .unwrap_or_else(|_| {
                                        compute_csv_diff(gitfleet_git::FileVersionPair {
                                            file_path: String::new(),
                                            before_label: "Index".into(),
                                            after_label: "Working Tree".into(),
                                            before_bytes: None,
                                            after_bytes: None,
                                        })
                                    });
                            (req_id, t, data)
                        },
                        |(id, t, d)| Message::CsvDiffLoaded(id, t, d),
                    ));
                }

                Task::batch(tasks)
            }
            Message::SetBottomViewMode(mode) => {
                self.bottom_mode = mode;
                if matches!(
                    mode,
                    BottomViewMode::UnifiedDiff | BottomViewMode::SideBySideDiff
                ) {
                    self.last_text_diff_mode = mode;
                    if self.diff_csv.is_some() {
                        self.csv_prefer_table = false;
                    }
                } else if mode == BottomViewMode::CsvDiff {
                    self.csv_prefer_table = true;
                }
                self.diff_selection = None;
                if matches!(mode, BottomViewMode::Edit | BottomViewMode::Preview) {
                    if mode == BottomViewMode::Preview {
                        self.ensure_preview_hydrated();
                    } else {
                        self.ensure_editor_hydrated();
                    }
                    if let Some(rel) = self.active_file_path.clone() {
                        self.reload_file_for_preview_and_edit_if_changed(&rel);
                    }
                }
                Task::none()
            }
            Message::ToggleDiffCompact => {
                self.diff_compact = !self.diff_compact;
                self.diff_selection = None;
                self.diff_left_scroll_y = 0.0;
                self.diff_unified_scroll_y = 0.0;
                self.rebuild_diff_cache();
                Task::batch([
                    scrollable::scroll_to(
                        self.diff_left_scroll_id.clone(),
                        scrollable::AbsoluteOffset {
                            x: self.diff_left_scroll_x,
                            y: 0.0,
                        },
                    ),
                    scrollable::scroll_to(
                        self.diff_unified_scroll_id.clone(),
                        scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
                    ),
                ])
            }
            Message::ExpandDiffGap { gap_id, mode } => {
                self.diff_selection = None;
                let entry = self.diff_expanded_gaps.entry(gap_id).or_default();
                match mode {
                    DiffGapExpandMode::Top(n) => entry.top_shown += n,
                    DiffGapExpandMode::Bottom(n) => entry.bottom_shown += n,
                    DiffGapExpandMode::All => entry.show_all = true,
                }
                self.rebuild_diff_cache();
                Task::none()
            }
            Message::DiffLeftScrolled(vp) => {
                let off = vp.absolute_offset();
                self.diff_left_viewport_h = vp.bounds().height.max(1.0);
                if (off.x - self.diff_left_scroll_x).abs() >= 0.5 {
                    self.diff_left_scroll_x = off.x;
                }
                if (off.y - self.diff_left_scroll_y).abs() >= 0.5 {
                    self.diff_left_scroll_y = off.y;
                }
                Task::none()
            }
            Message::DiffUnifiedScrolled(vp) => {
                let off = vp.absolute_offset();
                self.diff_unified_viewport_h = vp.bounds().height.max(1.0);
                if (off.y - self.diff_unified_scroll_y).abs() >= 0.5 {
                    self.diff_unified_scroll_y = off.y;
                }
                Task::none()
            }
            Message::DiffJumpChange(delta) => {
                let count = self.diff_cache.layout.connectors.len();
                if count == 0 {
                    return Task::none();
                }
                let next_idx = if delta < 0 {
                    if self.diff_active_change == 0 {
                        count - 1
                    } else {
                        self.diff_active_change - 1
                    }
                } else {
                    (self.diff_active_change + 1) % count
                };
                self.diff_active_change = next_idx;
                let span = self.diff_cache.layout.connectors[next_idx];
                let margin = 48.0;
                let sy = (span.split_top - margin).max(0.0);
                let uy = (span.unified_top - margin).max(0.0);
                self.diff_left_scroll_y = sy;
                self.diff_unified_scroll_y = uy;
                match self.bottom_mode {
                    BottomViewMode::SideBySideDiff => scrollable::scroll_to(
                        self.diff_left_scroll_id.clone(),
                        scrollable::AbsoluteOffset {
                            x: self.diff_left_scroll_x,
                            y: sy,
                        },
                    ),
                    BottomViewMode::UnifiedDiff => scrollable::scroll_to(
                        self.diff_unified_scroll_id.clone(),
                        scrollable::AbsoluteOffset { x: 0.0, y: uy },
                    ),
                    _ => Task::none(),
                }
            }
            Message::DiffSelStart { side, row, col } => {
                self.terminal_focused = false;
                self.focus_pane = ActiveFocusPane::Diff;
                let now = Instant::now();
                let click_count = match self.diff_last_click {
                    Some((prev_t, prev_side, prev_row, prev_col, prev_cnt))
                        if now.duration_since(prev_t) <= Duration::from_millis(400)
                            && prev_side == side
                            && prev_row == row
                            && col.abs_diff(prev_col) <= 1 =>
                    {
                        match prev_cnt {
                            1 => 2,
                            2 => 3,
                            _ => 1,
                        }
                    }
                    _ => 1,
                };
                self.diff_last_click = Some((now, side, row, col, click_count));

                if click_count >= 2
                    && let Some(line) = diff_line_text_at(
                        &self.diff_parsed,
                        &self.diff_cache.layout,
                        side,
                        row,
                    )
                {
                    let (s, e) = if click_count == 2 {
                        word_bounds_at(&line, col)
                    } else {
                        (0, line.chars().count())
                    };
                    self.diff_selection = Some(DiffTextSelection {
                        side,
                        anchor: DiffTextPos { row, col: s },
                        focus: DiffTextPos { row, col: e },
                        dragging: false,
                    });
                    return Task::none();
                }

                let pos = DiffTextPos { row, col };
                self.diff_selection = Some(DiffTextSelection {
                    side,
                    anchor: pos,
                    focus: pos,
                    dragging: true,
                });
                Task::none()
            }
            Message::DiffSelDrag { side, row, col } => {
                if let Some(sel) = &mut self.diff_selection
                    && sel.dragging
                    && sel.side == side
                {
                    sel.focus = DiffTextPos { row, col };
                }
                Task::none()
            }
            Message::CopyDiffSelection(win_id) => {
                if self.detached_viewers.contains_key(&win_id) {
                    return self.update_detached_viewer(
                        win_id,
                        crate::views::detached_viewer::DetachedAction::CopySelection,
                    );
                }
                if self.has_modal_or_menu_open()
                    || !matches!(
                        self.bottom_mode,
                        BottomViewMode::UnifiedDiff | BottomViewMode::SideBySideDiff
                    )
                {
                    return Task::none();
                }
                if let Some(sel) = self.diff_selection
                    && !sel.is_empty()
                {
                    let copied = extract_selected_diff_text(
                        &self.diff_parsed,
                        &self.diff_cache.layout,
                        sel,
                    );
                    if !copied.is_empty() {
                        let line_cnt = copied.lines().count();
                        self.toast_is_error = false;
                        self.toast_message = if line_cnt > 1 {
                            format!("Copied {line_cnt} lines to clipboard")
                        } else {
                            "Copied selected text to clipboard".into()
                        };
                        return clipboard::write(copied);
                    }
                }
                Task::none()
            }
            Message::SelectAllFocused(win_id) => {
                if self.detached_viewers.contains_key(&win_id) {
                    return self.update_detached_viewer(
                        win_id,
                        crate::views::detached_viewer::DetachedAction::SelectAll,
                    );
                }
                if self.has_modal_or_menu_open() || self.focus_pane == ActiveFocusPane::TextInput {
                    return Task::none();
                }
                if self.focus_pane == ActiveFocusPane::Diff {
                    if self.diff_parsed.raw.is_empty()
                        || !matches!(
                            self.bottom_mode,
                            BottomViewMode::UnifiedDiff | BottomViewMode::SideBySideDiff
                        )
                    {
                        return Task::none();
                    }
                    let side = if self.diff_parsed.blocks.is_empty() {
                        DiffSelectionSide::Raw
                    } else if self.bottom_mode == BottomViewMode::UnifiedDiff {
                        DiffSelectionSide::Unified
                    } else {
                        self.diff_selection
                            .map(|s| s.side)
                            .filter(|s| {
                                matches!(s, DiffSelectionSide::Left | DiffSelectionSide::Right)
                            })
                            .unwrap_or(DiffSelectionSide::Right)
                    };
                    let count = diff_row_count(&self.diff_parsed, &self.diff_cache.layout, side);
                    if count > 0
                        && let Some((last_row, last_col)) = (0..count).rev().find_map(|r| {
                            diff_line_text_at(&self.diff_parsed, &self.diff_cache.layout, side, r)
                                .map(|l| (r, l.chars().count()))
                        })
                    {
                        self.diff_selection = Some(DiffTextSelection {
                            side,
                            anchor: DiffTextPos { row: 0, col: 0 },
                            focus: DiffTextPos {
                                row: last_row,
                                col: last_col,
                            },
                            dragging: false,
                        });
                    }
                    return Task::none();
                }

                // Select all visible files in the active Files section (Unstaged, Staged, or Commit)
                let dir_filter = self.current_files_dir_filter().map(str::to_string);
                let (target_files, section_label) = if self.graph_selection
                    != GraphSelection::WorkingTree
                {
                    (
                        self.visible_files_for_section(false, dir_filter.as_deref()),
                        "commit",
                    )
                } else {
                    match self.focus_pane {
                        ActiveFocusPane::FilesStaged => (
                            self.visible_files_for_section(true, dir_filter.as_deref()),
                            "staged",
                        ),
                        ActiveFocusPane::FilesUnstaged => (
                            self.visible_files_for_section(false, dir_filter.as_deref()),
                            "unstaged",
                        ),
                        _ => {
                            let unstaged =
                                self.visible_files_for_section(false, dir_filter.as_deref());
                            if !unstaged.is_empty() {
                                self.focus_pane = ActiveFocusPane::FilesUnstaged;
                                (unstaged, "unstaged")
                            } else {
                                self.focus_pane = ActiveFocusPane::FilesStaged;
                                (
                                    self.visible_files_for_section(true, dir_filter.as_deref()),
                                    "staged",
                                )
                            }
                        }
                    }
                };
                if !target_files.is_empty() {
                    self.selected_changed_files.clear();
                    for f in &target_files {
                        self.selected_changed_files
                            .insert((f.path.clone(), f.staged));
                    }
                    if self.file_selection_anchor.is_none() {
                        self.file_selection_anchor = target_files.first().cloned();
                    }
                    self.toast_is_error = false;
                    self.toast_message = format!(
                        "Selected all {} {} file(s)",
                        target_files.len(),
                        section_label
                    );
                }
                Task::none()
            }
            Message::DiffLoaded(req_id, title, res) => {
                if req_id != self.diff_request_id {
                    return Task::none();
                }
                // Ignore stale in-flight working-tree diff responses after active file was cleared
                if self.graph_selection == GraphSelection::WorkingTree
                    && self.selected_changed_file.is_none()
                    && self.active_file_path.is_none()
                    && self.diff_title == "No file selected"
                {
                    return Task::none();
                }
                if let Ok((parsed, _)) = &res
                    && self.diff_title == title
                    && self.diff_parsed.raw == parsed.raw
                    && self.diff_cache.matches_config(
                        self.diff_compact,
                        self.open_doc_language,
                        self.theme_mode,
                    )
                {
                    return Task::none();
                }
                self.diff_title = title;
                self.diff_expanded_gaps.clear();
                self.diff_selection = None;
                self.diff_left_scroll_y = 0.0;
                self.diff_unified_scroll_y = 0.0;
                self.diff_left_scroll_x = 0.0;
                self.diff_active_change = 0;
                match res {
                    Ok((parsed, cache)) => {
                        self.diff_parsed_path = self.active_file_path.clone();
                        self.diff_parsed = parsed;
                        if cache.matches_config(
                            self.diff_compact,
                            self.open_doc_language,
                            self.theme_mode,
                        ) {
                            self.diff_cache = cache;
                        } else {
                            self.rebuild_diff_cache();
                        }
                    }
                    Err(e) => {
                        self.diff_parsed = ParsedDiff {
                            raw: format!("Failed to load diff: {e}"),
                            ..Default::default()
                        };
                        self.rebuild_diff_cache();
                    }
                }
                Task::batch([
                    scrollable::scroll_to(
                        self.diff_left_scroll_id.clone(),
                        scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
                    ),
                    scrollable::scroll_to(
                        self.diff_unified_scroll_id.clone(),
                        scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
                    ),
                ])
            }
            Message::DocContextLoaded(payload) => {
                if payload.req_id != self.doc_request_id {
                    return Task::none();
                }
                match payload.doc {
                    Some(cap) => {
                        self.open_doc_id = Some(cap.document_id);
                        self.open_doc_version = Some(cap.version);
                        self.open_doc_path = Some(payload.full_path);
                        self.open_doc_line_ending = cap.line_ending;
                        self.open_doc_encoding = cap.encoding;
                        self.open_doc_raw_text = Some(cap.content);
                        self.editor_dirty = false;
                        self.preview_hydrated = false;
                        self.editor_hydrated = false;
                    }
                    None => {
                        self.open_doc_id = None;
                        self.open_doc_version = None;
                        self.open_doc_path = None;
                        self.open_doc_raw_text = None;
                        self.open_doc_line_ending = gitfleet_docs::LineEnding::Lf;
                        self.open_doc_encoding = gitfleet_docs::FileEncoding::Utf8;
                        self.editor_dirty = false;
                        self.preview_hydrated = false;
                        self.editor_hydrated = false;
                    }
                }

                if let Some(parsed) = payload.conflict_parsed {
                    let has_hunks = parsed.conflict_count() > 0;
                    self.conflict_file_path = Some(payload.rel_path);
                    self.conflict_parsed = parsed;
                    self.conflict_active_hunk = has_hunks.then_some(0);
                    self.conflict_editing_hunk = None;
                    self.conflict_hunk_editor = text_editor::Content::new();
                    self.conflict_expanded_segments.clear();
                    if self.graph_selection == GraphSelection::WorkingTree && has_hunks {
                        self.bottom_mode = BottomViewMode::ResolveConflicts;
                    }
                } else if self.bottom_mode == BottomViewMode::ResolveConflicts {
                    self.bottom_mode = self.last_text_diff_mode;
                }

                if self.bottom_mode == BottomViewMode::Preview {
                    self.ensure_preview_hydrated();
                } else if self.bottom_mode == BottomViewMode::Edit {
                    self.ensure_editor_hydrated();
                }
                Task::batch([self.preview_scroll.reset(), self.editor_scroll.reset()])
            }
            Message::ImageDiffLoaded(req_id, title, data) => {
                if req_id == self.diff_request_id {
                    self.diff_title = title;
                    self.image_diff_swipe = match (&data.before, &data.after) {
                        (None, Some(_)) => 0.0,
                        (Some(_), None) => 1.0,
                        _ => 0.5,
                    };
                    self.diff_image = Some(data);
                }
                Task::none()
            }
            Message::CsvDiffLoaded(req_id, title, data) => {
                if req_id == self.diff_request_id {
                    self.diff_title = title;
                    self.diff_csv = Some(data);
                }
                Task::none()
            }
            Message::SetImageDiffMode(mode) => {
                self.image_diff_mode = mode;
                self.image_diff_blink_after = false;
                Task::none()
            }
            Message::SetImageDiffZoom(zoom) => {
                self.image_diff_zoom = zoom;
                Task::none()
            }
            Message::ToggleImageDiffSwap => {
                self.image_diff_swapped = !self.image_diff_swapped;
                Task::none()
            }
            Message::ImageDiffSwipeChanged(pos) => {
                self.image_diff_swipe = pos.clamp(0.0, 1.0);
                Task::none()
            }
            Message::ImageDiffOnionChanged(alpha) => {
                self.image_diff_onion = alpha.clamp(0.0, 1.0);
                Task::none()
            }
            Message::ImageDiffBlinkTick => {
                if self.bottom_mode == BottomViewMode::ImageDiff
                    && self.image_diff_mode == crate::views::image_diff::ImageDiffMode::Blink
                    && !self.image_diff_blink_paused
                    && self.diff_image.is_some()
                {
                    self.image_diff_blink_after = !self.image_diff_blink_after;
                }
                for viewer in self.detached_viewers.values_mut() {
                    if viewer.needs_blink_timer()
                        && let Some(tab) = viewer.active_tab_mut()
                    {
                        tab.image_state.blink_after = !tab.image_state.blink_after;
                    }
                }
                Task::none()
            }
            Message::ImageDiffBlinkFlip => {
                self.image_diff_blink_after = !self.image_diff_blink_after;
                Task::none()
            }
            Message::ToggleImageDiffBlinkPause => {
                self.image_diff_blink_paused = !self.image_diff_blink_paused;
                Task::none()
            }
            Message::SetCsvDiffChangesOnly(only) => {
                self.csv_diff_changes_only = only;
                Task::none()
            }
            Message::ToggleCsvDiffIgnoreFormat => {
                self.csv_diff_ignore_format = !self.csv_diff_ignore_format;
                Task::none()
            }
            Message::CsvDiffFilterChanged(q) => {
                self.focus_pane = ActiveFocusPane::TextInput;
                self.csv_diff_filter = q;
                Task::none()
            }
            Message::PreviewLinkClicked(url) => {
                self.toast_is_error = false;
                self.toast_message = format!("Link: {url}");
                Task::none()
            }
            Message::PreviewEditorAction(action) => {
                self.focus_pane = ActiveFocusPane::TextInput;
                if !action.is_edit() {
                    let cfg = crate::views::code_editor::NumberedEditorConfig::preview(
                        crate::theme::type_scale::CODE,
                        false,
                    );
                    return crate::views::code_editor::perform_numbered_editor_action(
                        &mut self.preview_editor,
                        action,
                        &mut self.preview_scroll,
                        cfg,
                    );
                }
                Task::none()
            }
            Message::PreviewScrolled(vp) => {
                self.preview_scroll.on_scroll(vp);
                Task::none()
            }
            Message::TogglePreviewRaw => {
                self.preview_raw_mode = !self.preview_raw_mode;
                self.preview_scroll.reset()
            }
            Message::CopyPreviewText => {
                if let Some(sel) = self.preview_editor.selection().filter(|s| !s.is_empty()) {
                    self.toast_is_error = false;
                    self.toast_message = "Copied selected preview text to clipboard".into();
                    return clipboard::write(sel);
                }
                if let Some(full) = self.preview_state.copyable_text() {
                    self.toast_is_error = false;
                    self.toast_message = "Copied preview to clipboard".into();
                    return clipboard::write(full.to_string());
                }
                Task::none()
            }
            Message::EditorAction(action) => {
                self.focus_pane = ActiveFocusPane::TextInput;
                let is_edit = action.is_edit();
                let scroll_task = crate::views::code_editor::perform_numbered_editor_action(
                    &mut self.editor_content,
                    action,
                    &mut self.editor_scroll,
                    crate::views::code_editor::NumberedEditorConfig::host_edit(),
                );
                if is_edit {
                    self.editor_dirty = true;
                }
                scroll_task
            }
            Message::EditorScrolled(vp) => {
                self.editor_scroll.on_scroll(vp);
                Task::none()
            }
            Message::SaveDocument => {
                if self.bottom_mode != BottomViewMode::Edit || !self.editor_dirty {
                    return Task::none();
                }
                if let (Some(doc_id), Some(ver)) =
                    (self.open_doc_id.clone(), self.open_doc_version.clone())
                {
                    let text_val = self.editor_content.text();
                    match self.service.docs.save_document_detailed(
                        &doc_id,
                        &text_val,
                        &ver,
                        Some(self.open_doc_line_ending),
                        Some(self.open_doc_encoding),
                    ) {
                        Ok(res) => {
                            self.open_doc_version = Some(res.version);
                            self.open_doc_line_ending = res.line_ending;
                            self.open_doc_encoding = res.encoding;
                            self.open_doc_raw_text = Some(res.content);
                            self.preview_hydrated = false;
                            if self.bottom_mode == BottomViewMode::Preview {
                                self.ensure_preview_hydrated();
                            }
                            self.editor_dirty = false;
                            self.toast_is_error = false;
                            self.toast_message = if res.lossy_chars > 0 {
                                format!(
                                    "File saved ({} · {}, {} char(s) replaced with '?')",
                                    res.encoding.label(),
                                    res.line_ending.label(),
                                    res.lossy_chars
                                )
                            } else {
                                format!(
                                    "File saved ({} · {})",
                                    res.encoding.label(),
                                    res.line_ending.label()
                                )
                            };
                            return self.update(Message::RefreshSelected);
                        }
                        Err(e) => {
                            self.report_action_error("Save File Failed", e.to_string(), false);
                        }
                    }
                }
                Task::none()
            }
            Message::ToggleEolModal => {
                if self.open_doc_id.is_none() {
                    self.toast_is_error = false;
                    self.toast_message = "Open a text file first to change End of Line Sequence".into();
                    return Task::none();
                }
                self.doc_format_menu_pos = self.current_host_cursor_pos();
                self.show_palette = false;
                self.show_branch_popover = false;
                self.show_merge_modal = false;
                self.show_git_flow_modal = false;
                self.show_workspace_modal = false;
                self.show_settings_modal = false;
                self.show_encoding_modal = false;
                self.show_syntax_modal = false;
                self.show_eol_modal = !self.show_eol_modal;
                Task::none()
            }
            Message::ToggleEncodingModal => {
                if self.open_doc_id.is_none() {
                    self.toast_is_error = false;
                    self.toast_message = "Open a text file first to change File Encoding".into();
                    return Task::none();
                }
                self.doc_format_menu_pos = self.current_host_cursor_pos();
                self.show_palette = false;
                self.show_branch_popover = false;
                self.show_merge_modal = false;
                self.show_git_flow_modal = false;
                self.show_workspace_modal = false;
                self.show_settings_modal = false;
                self.show_eol_modal = false;
                self.show_syntax_modal = false;
                self.show_encoding_modal = !self.show_encoding_modal;
                if self.show_encoding_modal {
                    self.encoding_modal_mode = EncodingModalMode::SaveWithEncoding;
                }
                Task::none()
            }
            Message::ToggleSyntaxModal => {
                if self.active_file_path.is_none() && self.open_doc_id.is_none() {
                    self.toast_is_error = false;
                    self.toast_message = "Select a file first to change Syntax Highlighting".into();
                    return Task::none();
                }
                self.doc_format_menu_pos = self.current_host_cursor_pos();
                self.show_palette = false;
                self.show_branch_popover = false;
                self.show_merge_modal = false;
                self.show_git_flow_modal = false;
                self.show_workspace_modal = false;
                self.show_settings_modal = false;
                self.show_eol_modal = false;
                self.show_encoding_modal = false;
                self.show_syntax_modal = !self.show_syntax_modal;
                if self.show_syntax_modal {
                    self.syntax_filter.clear();
                    return text_input::focus(self.syntax_filter_id.clone());
                }
                Task::none()
            }
            Message::SyntaxFilterChanged(q) => {
                self.syntax_filter = q;
                Task::none()
            }
            Message::SetDocLanguage(lang) => {
                self.show_syntax_modal = false;
                self.open_doc_language = lang;
                self.rebuild_diff_cache();
                self.toast_is_error = false;
                self.toast_message = format!("Syntax highlight: {}", lang.label());
                Task::none()
            }
            Message::SetEncodingModalMode(mode) => {
                self.encoding_modal_mode = mode;
                Task::none()
            }
            Message::ChangeDocLineEnding(eol) => {
                self.show_eol_modal = false;
                let Some(doc_id) = self.open_doc_id.clone() else {
                    return Task::none();
                };
                if eol == self.open_doc_line_ending {
                    return Task::none();
                }
                let file_name = self
                    .active_file_path
                    .as_deref()
                    .map(|f| f.rsplit(['/', '\\']).next().unwrap_or(f).to_string())
                    .unwrap_or_else(|| "file".into());
                if self.editor_dirty {
                    if let Err(e) = self.service.docs.set_document_line_ending(&doc_id, eol) {
                        self.report_action_error("Change Line Ending Failed", e.to_string(), false);
                        return Task::none();
                    }
                    self.open_doc_line_ending = eol;
                    self.toast_is_error = false;
                    self.toast_message = format!(
                        "{file_name}: line ending set to {} (press Ctrl+S to save)",
                        eol.label()
                    );
                    return Task::none();
                }
                let Some(ver) = self.open_doc_version.clone() else {
                    return Task::none();
                };
                match self.service.docs.convert_document_on_disk(
                    &doc_id,
                    &ver,
                    eol,
                    self.open_doc_encoding,
                ) {
                    Ok(res) => {
                        self.open_doc_version = Some(res.version);
                        self.open_doc_line_ending = res.line_ending;
                        self.open_doc_encoding = res.encoding;
                        self.open_doc_raw_text = Some(res.content);
                        self.preview_hydrated = false;
                        self.editor_hydrated = false;
                        if self.bottom_mode == BottomViewMode::Preview {
                            self.ensure_preview_hydrated();
                        } else if self.bottom_mode == BottomViewMode::Edit {
                            self.ensure_editor_hydrated();
                        }
                        self.toast_is_error = false;
                        self.toast_message = format!("Converted {file_name} to {}", eol.label());
                        return self.update(Message::RefreshSelected);
                    }
                    Err(e) => {
                        self.report_action_error("Convert Line Ending Failed", e.to_string(), false);
                    }
                }
                Task::none()
            }
            Message::SaveDocWithEncoding(encoding) => {
                self.show_encoding_modal = false;
                let (Some(doc_id), Some(ver)) =
                    (self.open_doc_id.clone(), self.open_doc_version.clone())
                else {
                    return Task::none();
                };
                let file_name = self
                    .active_file_path
                    .as_deref()
                    .map(|f| f.rsplit(['/', '\\']).next().unwrap_or(f).to_string())
                    .unwrap_or_else(|| "file".into());
                let save_res = if self.editor_dirty {
                    let text_val = self.editor_content.text();
                    self.service.docs.save_document_detailed(
                        &doc_id,
                        &text_val,
                        &ver,
                        Some(self.open_doc_line_ending),
                        Some(encoding),
                    )
                } else {
                    self.service.docs.convert_document_on_disk(
                        &doc_id,
                        &ver,
                        self.open_doc_line_ending,
                        encoding,
                    )
                };
                match save_res {
                    Ok(res) => {
                        self.open_doc_version = Some(res.version);
                        self.open_doc_line_ending = res.line_ending;
                        self.open_doc_encoding = res.encoding;
                        self.open_doc_raw_text = Some(res.content);
                        self.editor_dirty = false;
                        self.preview_hydrated = false;
                        self.editor_hydrated = false;
                        if self.bottom_mode == BottomViewMode::Preview {
                            self.ensure_preview_hydrated();
                        } else if self.bottom_mode == BottomViewMode::Edit {
                            self.ensure_editor_hydrated();
                        }
                        self.toast_is_error = false;
                        self.toast_message = if res.lossy_chars > 0 {
                            format!(
                                "Saved {file_name} as {} ({} char(s) replaced with '?')",
                                encoding.label(),
                                res.lossy_chars
                            )
                        } else {
                            format!("Saved {file_name} as {}", encoding.label())
                        };
                        return self.update(Message::RefreshSelected);
                    }
                    Err(e) => {
                        self.report_action_error("Save with Encoding Failed", e.to_string(), false);
                    }
                }
                Task::none()
            }
            Message::ReopenDocWithEncoding(encoding) => {
                self.show_encoding_modal = false;
                let Some(full_path) = self.open_doc_path.clone() else {
                    return Task::none();
                };
                let file_name = self
                    .active_file_path
                    .as_deref()
                    .map(|f| f.rsplit(['/', '\\']).next().unwrap_or(f).to_string())
                    .unwrap_or_else(|| "file".into());
                match self
                    .service
                    .docs
                    .open_document_with_encoding(&full_path, encoding)
                {
                    Ok(cap) => {
                        self.open_doc_id = Some(cap.document_id);
                        self.open_doc_version = Some(cap.version);
                        self.open_doc_line_ending = cap.line_ending;
                        self.open_doc_encoding = cap.encoding;
                        self.open_doc_raw_text = Some(cap.content);
                        self.editor_dirty = false;
                        self.preview_hydrated = false;
                        self.editor_hydrated = false;
                        if self.bottom_mode == BottomViewMode::Preview {
                            self.ensure_preview_hydrated();
                        } else if self.bottom_mode == BottomViewMode::Edit {
                            self.ensure_editor_hydrated();
                        }
                        self.toast_is_error = false;
                        self.toast_message =
                            format!("Reopened {file_name} with {}", encoding.label());
                    }
                    Err(e) => {
                        self.report_action_error(
                            "Reopen with Encoding Failed",
                            e.to_string(),
                            false,
                        );
                    }
                }
                Task::none()
            }
            Message::PickOpenIgnoredFile => {
                let start_dir = self.current_repo().map(|r| r.path);
                Task::perform(
                    async move {
                        let mut dlg =
                            rfd::AsyncFileDialog::new().set_title("Open file (including ignored .env)");
                        if let Some(dir) = start_dir {
                            dlg = dlg.set_directory(dir);
                        }
                        dlg.pick_file().await.map(|h| h.path().to_path_buf())
                    },
                    Message::IgnoredFilePicked,
                )
            }
            Message::IgnoredFilePicked(Some(full_path)) => {
                match self.service.docs.open_document(&full_path) {
                    Ok(cap) => {
                        self.diff_title = full_path.display().to_string();
                        self.active_file_path = Some(
                            full_path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .map(str::to_string)
                                .unwrap_or_else(|| full_path.display().to_string()),
                        );
                        self.preview_state =
                            preview::PreviewState::from_file(&full_path, Some(&cap.content));
                        self.sync_preview_editor();
                        self.editor_content = text_editor::Content::with_text(&cap.content);
                        self.open_doc_raw_text = Some(cap.content);
                        self.preview_hydrated = true;
                        self.editor_hydrated = true;
                        self.open_doc_id = Some(cap.document_id);
                        self.open_doc_version = Some(cap.version);
                        self.open_doc_line_ending = cap.line_ending;
                        self.open_doc_encoding = cap.encoding;
                        self.open_doc_language =
                            crate::syntax::CodeLanguage::from_path(&full_path);
                        self.open_doc_path = Some(full_path);
                        self.editor_dirty = false;
                        self.bottom_mode = BottomViewMode::Edit;
                        return Task::batch([
                            self.preview_scroll.reset(),
                            self.editor_scroll.reset(),
                        ]);
                    }
                    Err(e) => {
                        self.report_action_error("Open File Failed", e.to_string(), false);
                    }
                }
                Task::none()
            }
            Message::IgnoredFilePicked(None) => Task::none(),
            Message::PickSaveDocAs => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_title("Save document copy as…")
                        .save_file()
                        .await
                        .map(|h| h.path().to_path_buf())
                },
                Message::SaveDocAsPicked,
            ),
            Message::SaveDocAsPicked(Some(dest)) => {
                let content = if self.editor_hydrated {
                    self.editor_content.text()
                } else {
                    self.open_doc_raw_text.clone().unwrap_or_default()
                };
                match self.service.docs.save_document_as(
                    &dest,
                    &content,
                    self.open_doc_line_ending,
                    self.open_doc_encoding,
                ) {
                    Ok(lossy) => {
                        self.toast_is_error = false;
                        self.toast_message = if lossy > 0 {
                            format!(
                                "Saved copy to {} ({} · {}, {lossy} char(s) replaced with '?')",
                                dest.display(),
                                self.open_doc_encoding.label(),
                                self.open_doc_line_ending.label(),
                            )
                        } else {
                            format!(
                                "Saved copy to {} ({} · {})",
                                dest.display(),
                                self.open_doc_encoding.label(),
                                self.open_doc_line_ending.label(),
                            )
                        };
                    }
                    Err(e) => {
                        self.report_action_error("Save As Failed", e.to_string(), false);
                    }
                }
                Task::none()
            }
            Message::SaveDocAsPicked(None) => Task::none(),

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

    pub(crate) fn load_working_file_diff_tasks(
        &mut self,
        repo_path: std::path::PathBuf,
        file: &gitfleet_git::FileChange,
        req_id: u64,
    ) -> Task<Message> {
        let is_img = is_image_diff_path(&file.path);
        let is_raster = is_raster_image_path(&file.path);
        let is_csv = is_csv_diff_path(&file.path);

        self.diff_is_working_file = file.kind != FileChangeKind::Untracked;
        self.diff_is_staged = file.staged;
        let kind_label = file.kind.state_label(false);
        let title = if file.staged {
            format!("Changes of {} (Staged · {kind_label})", file.path)
        } else {
            format!("Changes of {} ({kind_label})", file.path)
        };
        self.diff_title = title.clone();
        let untracked = file.kind == FileChangeKind::Untracked;
        let mut tasks = Vec::new();

        if !is_raster {
            let svc = self.service.clone();
            let rp = repo_path.clone();
            let fpath = file.path.clone();
            let staged = file.staged;
            let t = title.clone();
            let compact = self.diff_compact;
            let lang = self.open_doc_language;
            let theme_mode = self.theme_mode;
            tasks.push(Task::perform(
                async move {
                    let res = match svc.git.get_diff(&rp, &fpath, staged, untracked).await {
                        Ok(raw) => tokio::task::spawn_blocking(move || {
                            parse_and_cache_diff(&raw, compact, lang, theme_mode)
                        })
                        .await
                        .map_err(|e| format!("Diff parse task failed: {e}")),
                        Err(e) => Err(e.to_string()),
                    };
                    (req_id, t, res)
                },
                |(id, t, r)| Message::DiffLoaded(id, t, r),
            ));
        } else {
            self.clear_diff_parsed_and_cache();
        }

        if is_img {
            let svc = self.service.clone();
            let rp = repo_path.clone();
            let fpath = file.path.clone();
            let staged = file.staged;
            let t = title.clone();
            tasks.push(Task::perform(
                async move {
                    let data = match svc
                        .git
                        .get_working_file_versions(&rp, &fpath, staged, untracked)
                        .await
                    {
                        Ok(pair) => tokio::task::spawn_blocking(move || decode_image_diff_pair(pair))
                            .await
                            .unwrap_or_else(|e| ImageDiffData {
                                file_path: fpath.clone(),
                                is_svg: is_svg_path(&fpath),
                                before_label: "Before".into(),
                                after_label: "After".into(),
                                before: None,
                                after: None,
                                diff_handle: None,
                                diff_width: 0,
                                diff_height: 0,
                                diff_changed_pixels: 0,
                                diff_total_pixels: 0,
                                load_error: Some(format!("Decode task failed: {e}")),
                            }),
                        Err(e) => ImageDiffData {
                            file_path: fpath.clone(),
                            is_svg: is_svg_path(&fpath),
                            before_label: "Before".into(),
                            after_label: "After".into(),
                            before: None,
                            after: None,
                            diff_handle: None,
                            diff_width: 0,
                            diff_height: 0,
                            diff_changed_pixels: 0,
                            diff_total_pixels: 0,
                            load_error: Some(e.to_string()),
                        },
                    };
                    (req_id, t, data)
                },
                |(id, t, d)| Message::ImageDiffLoaded(id, t, d),
            ));
        } else if is_csv {
            let svc = self.service.clone();
            let rp = repo_path;
            let fpath = file.path.clone();
            let staged = file.staged;
            let t = title;
            tasks.push(Task::perform(
                async move {
                    let pair = svc
                        .git
                        .get_working_file_versions(&rp, &fpath, staged, untracked)
                        .await
                        .unwrap_or(gitfleet_git::FileVersionPair {
                            file_path: fpath,
                            before_label: "Before".into(),
                            after_label: "After".into(),
                            before_bytes: None,
                            after_bytes: None,
                        });
                    let data = tokio::task::spawn_blocking(move || compute_csv_diff(pair))
                        .await
                        .unwrap_or_else(|_| {
                            compute_csv_diff(gitfleet_git::FileVersionPair {
                                file_path: String::new(),
                                before_label: "Before".into(),
                                after_label: "After".into(),
                                before_bytes: None,
                                after_bytes: None,
                            })
                        });
                    (req_id, t, data)
                },
                |(id, t, d)| Message::CsvDiffLoaded(id, t, d),
            ));
        }

        Task::batch(tasks)
    }

    /// Reconciles the bottom Diff / Preview / Edit panel after a fresh `StatusLoaded`:
    /// - Clears stale diff immediately if the selected working-tree file was discarded/committed.
    /// - Reloads editor, preview, and diff cleanly if the active file changed on disk or in index.
    pub(crate) fn sync_active_working_file_after_status(&mut self) -> Task<Message> {
        if self.graph_selection != GraphSelection::WorkingTree {
            return Task::none();
        }
        let Some(repo) = self.current_repo() else {
            return Task::none();
        };

        let find_in_status =
            |app: &Self, path: &str, prefer_staged: Option<bool>| -> Option<gitfleet_git::FileChange> {
                let st = app.active_status()?;
                let all = st
                    .conflict_files
                    .iter()
                    .chain(st.staged_files.iter())
                    .chain(st.unstaged_files.iter());
                if let Some(staged) = prefer_staged
                    && let Some(exact) = all.clone().find(|f| f.path == path && f.staged == staged)
                {
                    return Some(exact.clone());
                }
                all.into_iter().find(|f| f.path == path).cloned()
            };

        if let Some(sel) = self.selected_changed_file.clone() {
            if let Some(fresh) = find_in_status(self, &sel.path, Some(sel.staged)) {
                self.selected_changed_file = Some(fresh.clone());
                self.reload_file_for_preview_and_edit_if_changed(&fresh.path);
                self.diff_request_id = self.diff_request_id.wrapping_add(1);
                let req_id = self.diff_request_id;
                return self.load_working_file_diff_tasks(repo.path, &fresh, req_id);
            } else {
                // Selected changed file is no longer dirty (discarded, committed, or reverted)
                self.clear_active_diff_viewer();
                return Task::none();
            }
        }

        if let Some(rel) = self.active_file_path.clone() {
            self.reload_file_for_preview_and_edit_if_changed(&rel);
            if let Some(fresh) = find_in_status(self, &rel, None) {
                self.selected_changed_file = Some(fresh.clone());
                self.diff_request_id = self.diff_request_id.wrapping_add(1);
                let req_id = self.diff_request_id;
                return self.load_working_file_diff_tasks(repo.path, &fresh, req_id);
            }
        }

        Task::none()
    }
}
