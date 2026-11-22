//! Async Git diff, file explorer, search, and tab save helpers for GitFleet Studio.

use super::io::{file_display_name, format_editor_text_for_save, jump_editor_to_span};
use super::state::{
    DetachedAction, DetachedGitDiffTarget, DetachedViewMode, StudioTab,
};
use crate::app::{GitFleetApp, Message};
use crate::preview::PreviewState;
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
        let (focus_id, diff_init, req_id, has_root) = {
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
            (fid, target, rid, v.search_root.is_some())
        };

        if let Some(fid) = focus_id {
            tasks.push(text_input::focus(fid));
        }
        if let Some((tab_id, target)) = diff_init {
            tasks.push(self.spawn_detached_git_diff_task(win_id, tab_id, req_id, target));
        }
        if has_root {
            tasks.push(self.spawn_explorer_scan_task(win_id));
        }
        Task::batch(tasks)
    }

    pub(crate) fn spawn_explorer_scan_task(&mut self, win_id: window::Id) -> Task<Message> {
        let params = {
            let Some(v) = self.detached_viewers.get_mut(&win_id) else {
                return Task::none();
            };
            let Some(root) = v.search_root.clone() else {
                v.explorer_files.clear();
                v.explorer_loading = false;
                return Task::none();
            };
            v.explorer_request_id = v.explorer_request_id.wrapping_add(1);
            v.explorer_loading = true;
            Some((v.explorer_request_id, root, v.search_use_gitignore))
        };
        let Some((rid, root, use_gi)) = params else {
            return Task::none();
        };
        Task::perform(
            async move {
                let files =
                    tokio::task::spawn_blocking(move || list_directory_files(&root, use_gi, 4000))
                        .await
                        .unwrap_or_default();
                (rid, files)
            },
            move |(id, files)| {
                Message::Detached(win_id, DetachedAction::ExplorerFilesLoaded(id, files))
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

    /// Opens a file into a tab (or focuses its existing tab without losing unsaved edits),
    /// optionally syncing in-file find and jumping straight to a search match line/column.
    pub(crate) fn open_or_focus_file_tab(
        &mut self,
        win_id: window::Id,
        path: PathBuf,
        jump_match: Option<(usize, usize, usize)>,
        sync_in_file_find: bool,
    ) -> Task<Message> {
        let svc = self.service.clone();
        let docs = &svc.docs;
        let active_repo = self.current_repo();
        let active_status = self.active_status().cloned();

        let (diff_load_info,) = {
            let Some(v) = self.detached_viewers.get_mut(&win_id) else {
                return Task::none();
            };

            let existing_idx = v
                .tabs
                .iter()
                .position(|t| t.file_path.as_ref() == Some(&path) && t.left_path.is_none());

            let mut diff_load: Option<(u64, u64, DetachedGitDiffTarget)> = None;

            if let Some(idx) = existing_idx {
                v.active_tab = Some(idx);
                if let Some(tab) = v.tabs.get_mut(idx)
                    && jump_match.is_some()
                    && tab.is_editable()
                {
                    tab.mode = DetachedViewMode::Edit;
                }
            } else {
                let tab_id = v.alloc_tab_id();
                let mut tab = StudioTab::new_empty(tab_id, file_display_name(&path));
                let opened_editable = tab.load_local_file(docs, &path, v.search_root.as_deref());
                tab.mode = if opened_editable {
                    DetachedViewMode::Edit
                } else {
                    DetachedViewMode::Preview
                };
                if let Some(rel) = tab.rel_path.clone() {
                    v.expand_ancestors_for_rel(&rel);
                }

                // Attach working-tree Git diff if file has changes in active repo
                let diff_target = active_repo.as_ref().and_then(|repo| {
                    let rel = path.strip_prefix(&repo.path).ok()?;
                    let rel_str = rel.to_string_lossy().replace('\\', "/");
                    let st = active_status.as_ref()?;
                    let fc = st
                        .unstaged_files
                        .iter()
                        .chain(st.staged_files.iter())
                        .find(|f| f.path == rel_str)?
                        .clone();
                    Some(DetachedGitDiffTarget::WorkingFile {
                        repo_path: repo.path.clone(),
                        file: fc,
                    })
                });
                tab.git_diff_target = diff_target.clone();
                if let Some(t) = diff_target {
                    v.request_id = v.request_id.wrapping_add(1);
                    diff_load = Some((tab_id, v.request_id, t));
                }

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
            v.recompute_in_file_matches(false);

            if let Some((line_idx, col_start, col_end)) = jump_match {
                if let Some(pos) = v
                    .in_file_matches
                    .iter()
                    .position(|m| m.line_index == line_idx && m.col_start == col_start)
                {
                    v.in_file_active_idx = Some(pos);
                }
                if let Some(tab) = v.active_tab_mut()
                    && tab.is_editable()
                {
                    tab.mode = DetachedViewMode::Edit;
                    jump_editor_to_span(&mut tab.editor_content, line_idx, col_start, col_end);
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

            (diff_load,)
        };

        if let Some((tid, rid, target)) = diff_load_info {
            self.spawn_detached_git_diff_task(win_id, tid, rid, target)
        } else {
            Task::none()
        }
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

            let content = format_editor_text_for_save(&tab.editor_content, tab.trailing_newline);
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
                    tab.dirty = false;
                    tab.preview_state = PreviewState::from_file(&path, Some(&res.content));
                    tab.sync_preview_editor();
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
