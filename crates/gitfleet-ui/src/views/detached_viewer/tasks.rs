//! Async Git diff, file explorer, search, and tab save helpers for GitFleet Studio.

use super::io::file_display_name;
use super::state::{
    DetachedAction, DetachedGitDiffTarget, DetachedViewMode, LoadedTabFile, StudioTab,
};
use crate::app::{GitFleetApp, Message};
use crate::syntax::CodeLanguage;
use crate::views::csv_diff::{compute_csv_diff, is_csv_diff_path};
use crate::views::diff_viewer::parse_and_cache_diff;
use crate::views::image_diff::{
    ImageDiffData, decode_image_diff_pair, is_image_diff_path, is_raster_image_path, is_svg_path,
};
use gitfleet_git::{FileChangeKind, FileVersionPair};
use gitfleet_search::{
    ScopedSearchRequest, ScopedSearchResult, list_directory_files, search_in_directory,
};
use iced::widget::text_input;
use iced::{Task, window};
use std::path::PathBuf;

impl GitFleetApp {
    pub(crate) fn on_detached_viewer_opened(&mut self, win_id: window::Id) -> Task<Message> {
        let mut tasks = Vec::new();
        let (focus_id, diff_init, req_id, file_init, has_root) = {
            let Some(v) = self.detached_viewers.get_mut(&win_id) else {
                return Task::none();
            };
            let fid = if v.focus_search_on_open {
                v.focus_search_on_open = false;
                Some(v.search_input_id.clone())
            } else {
                None
            };
            let target = v.initial_diff.take();
            let rid = if target.is_some() {
                v.request_id = v.request_id.wrapping_add(1);
                v.request_id
            } else {
                v.request_id
            };
            let file_load = v.initial_file_load.take().map(|(tid, path)| {
                let f_req = if let Some(t) = v.tabs.iter_mut().find(|t| t.id == tid) {
                    t.file_load_req_id = t.file_load_req_id.wrapping_add(1);
                    t.file_load_req_id
                } else {
                    1
                };
                (tid, f_req, path)
            });
            (fid, target, rid, file_load, v.search_root.is_some())
        };

        if let Some(fid) = focus_id {
            tasks.push(text_input::focus(fid));
        }
        if let Some((tab_id, f_req, path)) = file_init {
            tasks.push(self.spawn_tab_file_load_task(win_id, tab_id, f_req, path, None));
        }
        if let Some((tab_id, target)) = diff_init {
            tasks.push(self.spawn_detached_git_diff_task(win_id, tab_id, req_id, target));
        }
        if has_root {
            tasks.push(self.spawn_explorer_scan_task(win_id));
        }
        Task::batch(tasks)
    }

    pub(crate) fn spawn_tab_file_load_task(
        &self,
        win_id: window::Id,
        tab_id: u64,
        req_id: u64,
        path: PathBuf,
        jump_match: Option<(usize, usize, usize)>,
    ) -> Task<Message> {
        let svc = self.service.clone();
        let theme_mode = self.theme_mode;
        Task::perform(
            async move {
                let p_clone = path.clone();
                let (doc, prebuilt_preview) = tokio::task::spawn_blocking(move || {
                    let doc = svc.docs.open_document(&p_clone).ok();
                    let needs_preview = doc.is_none()
                        || StudioTab::is_rich_preview_target(
                            Some(p_clone.as_path()),
                            CodeLanguage::from_path(&p_clone),
                        );
                    let prebuilt = if needs_preview {
                        crate::preview::PreviewState::from_file_prewarmed(
                            &p_clone,
                            doc.as_ref().map(|c| c.content.as_str()),
                            theme_mode,
                        )
                    } else {
                        crate::preview::PreviewState::Empty
                    };
                    (doc, crate::preview::PrebuiltPreviewSlot::new(prebuilt))
                })
                .await
                .unwrap_or_else(|_| (None, crate::preview::PrebuiltPreviewSlot::empty()));
                Box::new(LoadedTabFile {
                    path,
                    doc,
                    prebuilt_preview,
                    jump_match,
                })
            },
            move |payload| {
                Message::Detached(win_id, DetachedAction::TabFileLoaded(tab_id, req_id, payload))
            },
        )
    }

    pub(crate) fn spawn_explorer_scan_task(&mut self, win_id: window::Id) -> Task<Message> {
        let params = {
            let Some(v) = self.detached_viewers.get_mut(&win_id) else {
                return Task::none();
            };
            let Some(root) = v.search_root.clone() else {
                v.explorer_files.clear();
                v.clear_explorer_ignored_cache();
                v.explorer_loading = false;
                return Task::none();
            };
            v.explorer_request_id = v.explorer_request_id.wrapping_add(1);
            v.explorer_loading = true;
            Some((v.explorer_request_id, root, v.explorer_show_ignored))
        };
        let Some((rid, root, show_ignored)) = params else {
            return Task::none();
        };
        let svc = self.service.clone();
        Task::perform(
            async move {
                match svc.git.list_repo_files(&root).await {
                    Ok(files) => {
                        let ignored = if show_ignored {
                            svc.git
                                .list_ignored_entries(&root)
                                .await
                                .unwrap_or_default()
                        } else {
                            gitfleet_git::IgnoredEntries::default()
                        };
                        let mut git_map = std::collections::HashMap::new();
                        if let Ok(st) = svc.git.get_status(&root).await {
                            for f in &st.staged_files {
                                let code = match f.kind {
                                    FileChangeKind::Added => 'A',
                                    FileChangeKind::Deleted => 'D',
                                    FileChangeKind::Renamed | FileChangeKind::Copied => 'R',
                                    FileChangeKind::Conflicted => 'U',
                                    _ => 'M',
                                };
                                git_map.insert(f.path.clone(), code);
                            }
                            for f in &st.unstaged_files {
                                let code = match f.kind {
                                    FileChangeKind::Untracked => '?',
                                    FileChangeKind::Deleted => 'D',
                                    FileChangeKind::Conflicted => 'U',
                                    _ => 'M',
                                };
                                git_map.insert(f.path.clone(), code);
                            }
                            for f in &st.conflict_files {
                                git_map.insert(f.path.clone(), 'U');
                            }
                        }
                        (rid, files, ignored, git_map)
                    }
                    Err(_) => {
                        // Plain non-Git folder: list all files on disk (skipping `.git`).
                        let files = tokio::task::spawn_blocking(move || {
                            list_directory_files(&root, false, 4000)
                        })
                        .await
                        .unwrap_or_default();
                        (
                            rid,
                            files,
                            gitfleet_git::IgnoredEntries::default(),
                            std::collections::HashMap::new(),
                        )
                    }
                }
            },
            move |(id, files, ignored, git_map)| {
                Message::Detached(
                    win_id,
                    DetachedAction::ExplorerFilesLoaded(id, files, ignored, git_map),
                )
            },
        )
    }

    pub(crate) fn spawn_explorer_ignored_dir_task(
        &self,
        win_id: window::Id,
        req_id: u64,
        root: PathBuf,
        rel_dir: String,
    ) -> Task<Message> {
        Task::perform(
            async move {
                let dir_clone = rel_dir.clone();
                let listing = tokio::task::spawn_blocking(move || {
                    gitfleet_git::list_ignored_dir_children(&root, &dir_clone, 500)
                        .unwrap_or_default()
                })
                .await
                .unwrap_or_default();
                (req_id, rel_dir, listing)
            },
            move |(id, dir, listing)| {
                Message::Detached(
                    win_id,
                    DetachedAction::ExplorerIgnoredDirLoaded(id, dir, listing),
                )
            },
        )
    }

    pub(crate) fn spawn_detached_git_diff_task(
        &self,
        win_id: window::Id,
        tab_id: u64,
        req_id: u64,
        target: DetachedGitDiffTarget,
    ) -> Task<Message> {
        let svc = self.service.clone();
        let theme_mode = self.theme_mode;
        let (compact, tab_lang) = self
            .detached_viewers
            .get(&win_id)
            .and_then(|v| v.tabs.iter().find(|t| t.id == tab_id))
            .map(|t| (t.diff_compact, Some(t.language)))
            .unwrap_or((true, None));
        let mut tasks = Vec::new();
        match target {
            DetachedGitDiffTarget::WorkingFile { repo_path, file } => {
                let is_img = is_image_diff_path(&file.path);
                let is_raster = is_raster_image_path(&file.path);
                let is_csv = is_csv_diff_path(&file.path);
                let untracked = file.kind == FileChangeKind::Untracked;
                let lang = tab_lang
                    .unwrap_or_else(|| CodeLanguage::from_path(std::path::Path::new(&file.path)));

                if !is_raster {
                    let s = svc.clone();
                    let rp = repo_path.clone();
                    let f = file.clone();
                    tasks.push(Task::perform(
                        async move {
                            let res = match s
                                .git
                                .get_diff(&rp, &f.path, f.staged, untracked)
                                .await
                            {
                                Ok(raw) => tokio::task::spawn_blocking(move || {
                                    parse_and_cache_diff(&raw, compact, lang, theme_mode)
                                })
                                .await
                                .map_err(|e| format!("Diff parse task failed: {e}")),
                                Err(e) => Err(e.to_string()),
                            };
                            (tab_id, req_id, res)
                        },
                        move |(tid, id, res)| {
                            Message::Detached(win_id, DetachedAction::GitDiffLoaded(tid, id, res))
                        },
                    ));
                }
                if is_img {
                    let s = svc.clone();
                    let rp = repo_path.clone();
                    let f = file.clone();
                    tasks.push(Task::perform(
                        async move {
                            let data = match s
                                .git
                                .get_working_file_versions(&rp, &f.path, f.staged, untracked)
                                .await
                            {
                                Ok(pair) => {
                                    tokio::task::spawn_blocking(move || decode_image_diff_pair(pair))
                                        .await
                                        .unwrap_or_else(|e| ImageDiffData {
                                            file_path: f.path.clone(),
                                            is_svg: is_svg_path(&f.path),
                                            before_label: "Before".into(),
                                            after_label: "After".into(),
                                            before: None,
                                            after: None,
                                            diff_handle: None,
                                            diff_width: 0,
                                            diff_height: 0,
                                            diff_changed_pixels: 0,
                                            diff_total_pixels: 0,
                                            load_error: Some(format!("Decode failed: {e}")),
                                        })
                                }
                                Err(e) => ImageDiffData {
                                    file_path: f.path.clone(),
                                    is_svg: is_svg_path(&f.path),
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
                            (tab_id, req_id, data)
                        },
                        move |(tid, id, d)| {
                            Message::Detached(
                                win_id,
                                DetachedAction::GitImageDiffLoaded(tid, id, d),
                            )
                        },
                    ));
                } else if is_csv {
                    let s = svc.clone();
                    let rp = repo_path.clone();
                    let f = file.clone();
                    tasks.push(Task::perform(
                        async move {
                            let pair = s
                                .git
                                .get_working_file_versions(&rp, &f.path, f.staged, untracked)
                                .await
                                .unwrap_or(FileVersionPair {
                                    file_path: f.path,
                                    before_label: "Before".into(),
                                    after_label: "After".into(),
                                    before_bytes: None,
                                    after_bytes: None,
                                });
                            let data = tokio::task::spawn_blocking(move || compute_csv_diff(pair))
                                .await
                                .unwrap_or_else(|_| {
                                    compute_csv_diff(FileVersionPair {
                                        file_path: String::new(),
                                        before_label: "Before".into(),
                                        after_label: "After".into(),
                                        before_bytes: None,
                                        after_bytes: None,
                                    })
                                });
                            (tab_id, req_id, data)
                        },
                        move |(tid, id, d)| {
                            Message::Detached(win_id, DetachedAction::GitCsvDiffLoaded(tid, id, d))
                        },
                    ));
                }
            }
            DetachedGitDiffTarget::CommitFile {
                repo_path,
                oid,
                rel_path,
            } => {
                let is_img = is_image_diff_path(&rel_path);
                let is_raster = is_raster_image_path(&rel_path);
                let is_csv = is_csv_diff_path(&rel_path);
                let lang = tab_lang
                    .unwrap_or_else(|| CodeLanguage::from_path(std::path::Path::new(&rel_path)));

                if !is_raster {
                    let s = svc.clone();
                    let rp = repo_path.clone();
                    let o = oid.clone();
                    let p = rel_path.clone();
                    tasks.push(Task::perform(
                        async move {
                            let res = match s.git.get_commit_file_diff(&rp, &o, &p).await {
                                Ok(raw) => tokio::task::spawn_blocking(move || {
                                    parse_and_cache_diff(&raw, compact, lang, theme_mode)
                                })
                                .await
                                .map_err(|e| format!("Diff parse task failed: {e}")),
                                Err(e) => Err(e.to_string()),
                            };
                            (tab_id, req_id, res)
                        },
                        move |(tid, id, res)| {
                            Message::Detached(win_id, DetachedAction::GitDiffLoaded(tid, id, res))
                        },
                    ));
                }
                if is_img {
                    let s = svc.clone();
                    let rp = repo_path.clone();
                    let o = oid.clone();
                    let p = rel_path.clone();
                    tasks.push(Task::perform(
                        async move {
                            let data = match s.git.get_commit_file_versions(&rp, &o, &p).await {
                                Ok(pair) => {
                                    tokio::task::spawn_blocking(move || decode_image_diff_pair(pair))
                                        .await
                                        .unwrap_or_else(|e| ImageDiffData {
                                            file_path: p.clone(),
                                            is_svg: is_svg_path(&p),
                                            before_label: "Parent".into(),
                                            after_label: "Commit".into(),
                                            before: None,
                                            after: None,
                                            diff_handle: None,
                                            diff_width: 0,
                                            diff_height: 0,
                                            diff_changed_pixels: 0,
                                            diff_total_pixels: 0,
                                            load_error: Some(format!("Decode failed: {e}")),
                                        })
                                }
                                Err(e) => ImageDiffData {
                                    file_path: p.clone(),
                                    is_svg: is_svg_path(&p),
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
                            (tab_id, req_id, data)
                        },
                        move |(tid, id, d)| {
                            Message::Detached(
                                win_id,
                                DetachedAction::GitImageDiffLoaded(tid, id, d),
                            )
                        },
                    ));
                } else if is_csv {
                    let s = svc.clone();
                    let rp = repo_path.clone();
                    let o = oid.clone();
                    let p = rel_path.clone();
                    tasks.push(Task::perform(
                        async move {
                            let pair = s
                                .git
                                .get_commit_file_versions(&rp, &o, &p)
                                .await
                                .unwrap_or(FileVersionPair {
                                    file_path: p,
                                    before_label: "Parent".into(),
                                    after_label: "Commit".into(),
                                    before_bytes: None,
                                    after_bytes: None,
                                });
                            let data = tokio::task::spawn_blocking(move || compute_csv_diff(pair))
                                .await
                                .unwrap_or_else(|_| {
                                    compute_csv_diff(FileVersionPair {
                                        file_path: String::new(),
                                        before_label: "Parent".into(),
                                        after_label: "Commit".into(),
                                        before_bytes: None,
                                        after_bytes: None,
                                    })
                                });
                            (tab_id, req_id, data)
                        },
                        move |(tid, id, d)| {
                            Message::Detached(win_id, DetachedAction::GitCsvDiffLoaded(tid, id, d))
                        },
                    ));
                }
            }
        }
        Task::batch(tasks)
    }

    /// Opens a file into a tab asynchronously (or focuses its existing tab without losing unsaved edits),
    /// optionally syncing in-file find and jumping straight to a search match line/column.
    pub(crate) fn open_or_focus_file_tab(
        &mut self,
        win_id: window::Id,
        path: PathBuf,
        jump_match: Option<(usize, usize, usize)>,
        sync_in_file_find: bool,
    ) -> Task<Message> {
        let active_repo_path = self.current_repo().map(|r| r.path);
        let matching_git_change = active_repo_path.as_ref().and_then(|rp| {
            let rel = path.strip_prefix(rp).ok()?;
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            let st = self.active_status()?;
            let fc = st
                .unstaged_files
                .iter()
                .chain(st.staged_files.iter())
                .find(|f| f.path == rel_str)?
                .clone();
            Some(DetachedGitDiffTarget::WorkingFile {
                repo_path: rp.clone(),
                file: fc,
            })
        });

        struct PendingTabFileLoad {
            tab_id: u64,
            req_id: u64,
            path: PathBuf,
            jump_match: Option<(usize, usize, usize)>,
        }

        let (file_load_info, diff_load_info, jump_scroll_task) = {
            let Some(v) = self.detached_viewers.get_mut(&win_id) else {
                return Task::none();
            };

            let existing_idx = v
                .tabs
                .iter()
                .position(|t| t.file_path.as_ref() == Some(&path) && t.left_path.is_none());

            let mut file_load: Option<PendingTabFileLoad> = None;
            let mut diff_load: Option<(u64, u64, DetachedGitDiffTarget)> = None;
            let mut opened_new = false;

            if let Some(idx) = existing_idx {
                v.active_tab = Some(idx);
                if let Some(tab) = v.tabs.get_mut(idx) {
                    if jump_match.is_some() && tab.is_editable() {
                        tab.mode = DetachedViewMode::Edit;
                    }
                    if !tab.loading_file {
                        tab.hydrate_active_mode();
                    }
                }
            } else {
                opened_new = true;
                let tab_id = v.alloc_tab_id();
                let mut tab = StudioTab::new_empty(tab_id, file_display_name(&path));
                tab.file_path = Some(path.clone());
                tab.rel_path = v
                    .search_root
                    .as_deref()
                    .and_then(|r| path.strip_prefix(r).ok())
                    .map(|p| p.to_string_lossy().replace('\\', "/"));
                tab.language = CodeLanguage::from_path(&path);
                tab.mode = if is_raster_image_path(&tab.title) {
                    DetachedViewMode::Preview
                } else {
                    DetachedViewMode::Edit
                };
                tab.loading_file = true;
                tab.file_load_req_id = 1;

                if let Some(rel) = tab.rel_path.clone() {
                    v.expand_ancestors_for_rel(&rel);
                }

                tab.git_diff_target = matching_git_change.clone();
                if let Some(t) = matching_git_change {
                    v.request_id = v.request_id.wrapping_add(1);
                    diff_load = Some((tab_id, v.request_id, t));
                }

                file_load = Some(PendingTabFileLoad {
                    tab_id,
                    req_id: tab.file_load_req_id,
                    path: path.clone(),
                    jump_match,
                });
                v.tabs.push(tab);
                v.active_tab = Some(v.tabs.len() - 1);
            }

            if sync_in_file_find && !v.search_query.is_empty() {
                v.in_file_find_open = true;
                v.in_file_query = v.search_query.clone();
                v.in_file_options = v.search_options;
                if v.search_show_replace && !v.search_replace_text.is_empty() {
                    v.in_file_show_replace = true;
                    v.in_file_replace = v.search_replace_text.clone();
                }
            }

            let mut jump_scroll_task = Task::none();
            if !opened_new {
                v.recompute_in_file_matches(false);
                if let Some((line_idx, col_start, col_end)) = jump_match {
                    if let Some(pos) = v
                        .in_file_matches
                        .iter()
                        .position(|m| m.line_index == line_idx && m.col_start == col_start)
                    {
                        v.in_file_active_idx = Some(pos);
                    }
                    let cfg = crate::views::code_editor::NumberedEditorConfig::studio(
                        v.editor_font_size,
                        v.editor_word_wrap,
                    );
                    if let Some(tab) = v.active_tab_mut()
                        && tab.is_editable()
                    {
                        tab.mode = DetachedViewMode::Edit;
                        tab.ensure_editor_hydrated();
                        jump_scroll_task =
                            tab.jump_to_line_and_scroll(line_idx, col_start, col_end, cfg);
                    }
                    v.status_is_error = false;
                    v.status_note = format!("Line {}", line_idx + 1);
                } else if let Some(note) = v
                    .active_tab()
                    .map(|tab| tab.rel_path.clone().unwrap_or_else(|| tab.title.clone()))
                {
                    v.status_is_error = false;
                    v.status_note = note;
                }
            } else {
                v.status_is_error = false;
                v.status_note = format!("Opening {}…", file_display_name(&path));
            }

            (file_load, diff_load, jump_scroll_task)
        };

        let mut tasks = Vec::new();
        tasks.push(jump_scroll_task);
        if let Some(load) = file_load_info {
            tasks.push(self.spawn_tab_file_load_task(
                win_id,
                load.tab_id,
                load.req_id,
                load.path,
                load.jump_match,
            ));
        }
        if let Some((tid, rid, target)) = diff_load_info {
            tasks.push(self.spawn_detached_git_diff_task(win_id, tid, rid, target));
        }
        Task::batch(tasks)
    }

    pub(crate) fn save_tab_by_index(&mut self, win_id: window::Id, tab_idx: usize) -> (bool, Task<Message>) {
        let svc = self.service.clone();
        let docs = &svc.docs;
        let host_repo_path = self.current_repo().map(|r| r.path);

        let (saved_ok, saved_path, saved_content, saved_ver, saved_eol, saved_enc, diff_refresh) = {
            let Some(v) = self.detached_viewers.get_mut(&win_id) else {
                return (false, Task::none());
            };
            let Some(tab) = v.tabs.get_mut(tab_idx) else {
                return (false, Task::none());
            };
            let (Some(doc_id), Some(ver), Some(path)) = (
                tab.doc_id.clone(),
                tab.doc_version.clone(),
                tab.file_path.clone(),
            ) else {
                return (false, Task::none());
            };

            let content = tab.active_full_text().unwrap_or_default();
            match docs.save_document_detailed(
                &doc_id,
                &content,
                &ver,
                Some(tab.line_ending),
                Some(tab.encoding),
            ) {
                Ok(res) => {
                    tab.doc_version = Some(res.version.clone());
                    tab.line_ending = res.line_ending;
                    tab.encoding = res.encoding;
                    tab.raw_text = Some(res.content.clone());
                    tab.dirty = false;
                    tab.disk_conflict = false;
                    tab.preview_hydrated = false;
                    tab.refresh_git_line_marks();
                    if tab.supports_preview()
                        && (tab.mode == DetachedViewMode::Preview || v.split_right_preview)
                    {
                        tab.ensure_preview_hydrated();
                    }
                    let tid = tab.id;
                    let name = tab.title.clone();
                    let target_opt = tab.git_diff_target.clone();

                    v.recompute_in_file_matches(true);
                    v.status_is_error = false;
                    v.status_note = if res.lossy_chars > 0 {
                        format!("Saved {name} ({} lossy char(s))", res.lossy_chars)
                    } else {
                        format!("Saved {name}")
                    };
                    let refresh_diff = target_opt.map(|t| {
                        v.request_id = v.request_id.wrapping_add(1);
                        (tid, v.request_id, t)
                    });
                    (
                        true,
                        Some(path),
                        Some(res.content),
                        Some(res.version),
                        Some(res.line_ending),
                        Some(res.encoding),
                        refresh_diff,
                    )
                }
                Err(e) => {
                    v.status_is_error = true;
                    v.status_note = format!("Save failed: {e}");
                    (false, None, None, None, None, None, None)
                }
            }
        };

        if !saved_ok {
            return (false, Task::none());
        }

        let mut tasks = Vec::new();
        if let Some((tid, rid, target)) = diff_refresh {
            tasks.push(self.spawn_detached_git_diff_task(win_id, tid, rid, target));
        }
        if let Some(path) = saved_path.as_ref()
            && self
                .detached_viewers
                .get(&win_id)
                .and_then(|v| v.search_root.as_deref())
                .is_some_and(|r| path.starts_with(r))
        {
            tasks.push(self.spawn_explorer_scan_task(win_id));
        }

        if let (Some(path), Some(content)) = (saved_path.as_ref(), saved_content.as_ref()) {
            if self.open_doc_path.as_ref() == Some(path) && !self.editor_dirty {
                self.open_doc_raw_text = Some(content.clone());
                self.preview_hydrated = false;
                self.editor_hydrated = false;
                if self.bottom_mode == crate::app::BottomViewMode::Preview {
                    self.ensure_preview_hydrated();
                } else if self.bottom_mode == crate::app::BottomViewMode::Edit {
                    self.ensure_editor_hydrated();
                }
                if let Some(ver) = saved_ver {
                    self.open_doc_version = Some(ver);
                }
                if let Some(eol) = saved_eol {
                    self.open_doc_line_ending = eol;
                }
                if let Some(enc) = saved_enc {
                    self.open_doc_encoding = enc;
                }
            }
            if host_repo_path
                .as_ref()
                .is_some_and(|rp| path.starts_with(rp))
            {
                tasks.push(self.update(Message::RefreshSelected));
            }
        }

        (true, Task::batch(tasks))
    }

    pub(crate) fn spawn_detached_search_task(&mut self, win_id: window::Id) -> Task<Message> {
        let req_opt = {
            let Some(v) = self.detached_viewers.get_mut(&win_id) else {
                return Task::none();
            };
            let Some(root) = v.search_root.clone() else {
                v.status_is_error = true;
                v.status_note = "Pick a folder to search first".into();
                return Task::none();
            };
            v.search_request_id = v.search_request_id.wrapping_add(1);
            v.search_running = true;
            v.search_has_run = true;
            v.search_confirm_replace_all = false;
            v.status_is_error = false;
            v.status_note = "Searching…".into();
            Some((
                v.search_request_id,
                ScopedSearchRequest {
                    root,
                    query: v.search_query.clone(),
                    options: v.search_options,
                    include_patterns: v.search_include.clone(),
                    exclude_patterns: v.search_exclude.clone(),
                    use_gitignore: v.search_use_gitignore,
                    max_matches: 1500,
                },
            ))
        };

        let Some((rid, req)) = req_opt else {
            return Task::none();
        };

        Task::perform(
            async move {
                let res = tokio::task::spawn_blocking(move || search_in_directory(&req))
                    .await
                    .unwrap_or_else(|e| ScopedSearchResult {
                        error: Some(format!("Search failed: {e}")),
                        ..Default::default()
                    });
                (rid, res)
            },
            move |(id, res)| Message::Detached(win_id, DetachedAction::SearchCompleted(id, res)),
        )
    }
}
