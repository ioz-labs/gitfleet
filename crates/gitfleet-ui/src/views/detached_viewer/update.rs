//! Host-coupled & async action update handlers for GitFleet Studio.

use super::io::{file_display_name, format_editor_text_for_save, jump_editor_to_span};
use super::smart_edit::parse_goto_line_spec;
use super::state::{
    DetachedAction, DetachedGitDiffTarget, DetachedViewMode, StudioCreateEntryKind, StudioTab,
};
use crate::app::{GitFleetApp, Message};
use crate::preview::PreviewState;
use crate::syntax::CodeLanguage;
use crate::views::image_diff::ImageDiffZoom;
use crate::views::overlays::file_ops_modal::{remap_rel_path, validate_rename_input};
use gitfleet_git::{FileChange, FileChangeKind};
use gitfleet_search::{ReplaceReport, replace_in_file, replace_in_files};
use iced::widget::text_editor;
use iced::{Task, window};
use std::path::PathBuf;

impl GitFleetApp {
    pub(crate) fn update_detached_viewer(
        &mut self,
        win_id: window::Id,
        action: DetachedAction,
    ) -> Task<Message> {
        match action {
            DetachedAction::SaveEditor => {
                let (idx_opt, needs_save_as) = self
                    .detached_viewers
                    .get(&win_id)
                    .map(|v| {
                        let idx = v.active_tab;
                        let untitled = idx
                            .and_then(|i| v.tabs.get(i))
                            .is_some_and(|t| t.is_untitled || t.file_path.is_none());
                        (idx, untitled)
                    })
                    .unwrap_or((None, false));
                if needs_save_as {
                    return self.update_detached_viewer(win_id, DetachedAction::SaveTabAs);
                }
                if let Some(i) = idx_opt {
                    let (_, task) = self.save_tab_by_index(win_id, i);
                    return task;
                }
                Task::none()
            }
            DetachedAction::SaveTabAs => {
                let params = self.detached_viewers.get(&win_id).and_then(|v| {
                    let tab = v.active_tab()?;
                    if !tab.is_editable() {
                        return None;
                    }
                    let start_dir = tab
                        .file_path
                        .as_deref()
                        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                        .or_else(|| v.search_root.clone());
                    Some((tab.id, tab.title.clone(), start_dir))
                });
                let Some((tab_id, default_name, start_dir)) = params else {
                    return Task::none();
                };
                Task::perform(
                    async move {
                        let mut dlg = rfd::AsyncFileDialog::new()
                            .set_title("Save File As…")
                            .set_file_name(default_name);
                        if let Some(d) = start_dir {
                            dlg = dlg.set_directory(d);
                        }
                        dlg.save_file().await.map(|f| f.path().to_path_buf())
                    },
                    move |p| Message::Detached(win_id, DetachedAction::SaveTabAsPicked(tab_id, p)),
                )
            }
            DetachedAction::SaveTabAsPicked(tab_id, Some(dest)) => {
                let svc = self.service.clone();
                let docs = &svc.docs;
                let host_repo_path = self.current_repo().map(|r| r.path);
                let mut rescan_explorer = false;
                let mut refresh_host = false;

                if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                    let root = v.search_root.clone();
                    if let Some(tab) = v.tabs.iter_mut().find(|t| t.id == tab_id) {
                        let content = if tab.editor_hydrated {
                            format_editor_text_for_save(&tab.editor_content, tab.trailing_newline)
                        } else {
                            tab.raw_text.clone().unwrap_or_default()
                        };
                        match docs.save_document_as(&dest, &content, tab.line_ending, tab.encoding)
                        {
                            Ok(lossy) => {
                                tab.file_path = Some(dest.clone());
                                tab.is_untitled = false;
                                tab.title = file_display_name(&dest);
                                tab.language = CodeLanguage::from_path(&dest);
                                tab.rel_path = root
                                    .as_deref()
                                    .and_then(|r| dest.strip_prefix(r).ok())
                                    .map(|p| p.to_string_lossy().replace('\\', "/"));
                                if let Ok(cap) = docs.open_document(&dest) {
                                    tab.doc_id = Some(cap.document_id);
                                    tab.doc_version = Some(cap.version);
                                    tab.line_ending = cap.line_ending;
                                    tab.encoding = cap.encoding;
                                    tab.trailing_newline = cap.content.ends_with('\n');
                                    tab.raw_text = Some(cap.content);
                                }
                                tab.dirty = false;
                                tab.preview_hydrated = false;
                                if tab.mode == DetachedViewMode::Preview {
                                    tab.ensure_preview_hydrated();
                                }
                                let saved_title = tab.title.clone();
                                if let Some(rel) = tab.rel_path.clone() {
                                    v.expand_ancestors_for_rel(&rel);
                                }
                                v.status_is_error = false;
                                v.status_note = if lossy > 0 {
                                    format!("Saved as {saved_title} ({lossy} lossy char(s))")
                                } else {
                                    format!("Saved as {saved_title}")
                                };
                                rescan_explorer = root
                                    .as_deref()
                                    .is_some_and(|r| dest.starts_with(r));
                                refresh_host = host_repo_path
                                    .as_deref()
                                    .is_some_and(|rp| dest.starts_with(rp));
                            }
                            Err(e) => {
                                v.status_is_error = true;
                                v.status_note = format!("Save As failed: {e}");
                            }
                        }
                    }
                }

                let mut tasks = Vec::new();
                if rescan_explorer {
                    tasks.push(self.spawn_explorer_scan_task(win_id));
                }
                if refresh_host {
                    tasks.push(self.update(Message::RefreshSelected));
                }
                Task::batch(tasks)
            }
            DetachedAction::SaveTabAsPicked(_, None) => Task::none(),
            DetachedAction::SaveAllTabs => {
                let indices: Vec<usize> = self
                    .detached_viewers
                    .get(&win_id)
                    .map(|v| {
                        v.tabs
                            .iter()
                            .enumerate()
                            .filter_map(|(i, t)| {
                                (t.dirty && t.file_path.is_some() && t.doc_id.is_some())
                                    .then_some(i)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if indices.is_empty() {
                    if let Some(v) = self.detached_viewers.get(&win_id)
                        && v.active_tab().is_some_and(|t| t.is_untitled && t.dirty)
                    {
                        return self.update_detached_viewer(win_id, DetachedAction::SaveTabAs);
                    }
                    return Task::none();
                }
                let mut tasks = Vec::new();
                let mut saved_count = 0usize;
                for idx in indices {
                    let (ok, t) = self.save_tab_by_index(win_id, idx);
                    if ok {
                        saved_count += 1;
                    }
                    tasks.push(t);
                }
                if saved_count > 0
                    && let Some(v) = self.detached_viewers.get_mut(&win_id)
                {
                    v.status_is_error = false;
                    v.status_note = format!("Saved {saved_count} file(s)");
                }
                Task::batch(tasks)
            }
            DetachedAction::SubmitExplorerCreate => {
                let params = self.detached_viewers.get_mut(&win_id).and_then(|v| {
                    let root = v.search_root.clone()?;
                    let modal = v.create_modal.as_mut()?;
                    let trimmed = modal.input.trim().to_string();
                    if trimmed.is_empty() {
                        modal.error = Some("Please enter a name.".into());
                        return None;
                    }
                    Some((root, modal.kind.clone(), trimmed))
                });
                let Some((root, kind, name_input)) = params else {
                    return Task::none();
                };
                let is_dir = matches!(kind, StudioCreateEntryKind::Folder { .. });
                Task::perform(
                    async move {
                        let result = tokio::task::spawn_blocking(move || match kind {
                            StudioCreateEntryKind::File { parent_rel } => {
                                gitfleet_git::create_workspace_entry(
                                    &root,
                                    &parent_rel,
                                    &name_input,
                                    false,
                                )
                                .map_err(|e| e.to_string())
                            }
                            StudioCreateEntryKind::Folder { parent_rel } => {
                                gitfleet_git::create_workspace_entry(
                                    &root,
                                    &parent_rel,
                                    &name_input,
                                    true,
                                )
                                .map_err(|e| e.to_string())
                            }
                            StudioCreateEntryKind::Duplicate { src_rel } => {
                                gitfleet_git::duplicate_workspace_file(&root, &src_rel, &name_input)
                                    .map_err(|e| e.to_string())
                            }
                        })
                        .await
                        .unwrap_or_else(|e| Err(format!("File operation failed: {e}")));
                        (is_dir, result)
                    },
                    move |(is_dir, result)| {
                        Message::Detached(
                            win_id,
                            DetachedAction::ExplorerEntryCreated { is_dir, result },
                        )
                    },
                )
            }
            DetachedAction::ExplorerEntryCreated { is_dir, result } => {
                let host_repo_path = self.current_repo().map(|r| r.path);
                match result {
                    Ok(created_rel) => {
                        let abs_opt = self.detached_viewers.get_mut(&win_id).and_then(|v| {
                            v.create_modal = None;
                            v.expand_ancestors_for_rel(&created_rel);
                            if is_dir {
                                v.explorer_expanded_dirs.insert(created_rel.clone());
                            }
                            v.status_is_error = false;
                            v.status_note = if is_dir {
                                format!("Created folder '{created_rel}'")
                            } else {
                                format!("Created '{created_rel}'")
                            };
                            v.search_root.as_ref().map(|r| r.join(&created_rel))
                        });

                        let mut tasks = vec![self.spawn_explorer_scan_task(win_id)];
                        if let Some(abs) = abs_opt {
                            if host_repo_path
                                .as_deref()
                                .is_some_and(|rp| abs.starts_with(rp))
                            {
                                tasks.push(self.update(Message::RefreshSelected));
                            }
                            if !is_dir {
                                tasks.push(self.open_or_focus_file_tab(win_id, abs, None, false));
                            }
                        }
                        return Task::batch(tasks);
                    }
                    Err(err) => {
                        if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                            if let Some(m) = &mut v.create_modal {
                                m.error = Some(err.clone());
                            }
                            v.status_is_error = true;
                            v.status_note = err;
                        }
                    }
                }
                Task::none()
            }
            DetachedAction::ReopenClosedTab => {
                let reopened_path = self.detached_viewers.get_mut(&win_id).and_then(|v| {
                    while let Some(p) = v.closed_tabs.pop_back() {
                        if p.is_file() {
                            return Some(p);
                        }
                    }
                    v.status_is_error = false;
                    v.status_note = "No recently closed tab to reopen".into();
                    None
                });
                if let Some(p) = reopened_path {
                    return self.open_or_focus_file_tab(win_id, p, None, false);
                }
                Task::none()
            }
            DetachedAction::DismissDiskConflict(tab_id) => {
                let docs = &self.service.docs;
                if let Some(v) = self.detached_viewers.get_mut(&win_id)
                    && let Some(tab) = v.tabs.iter_mut().find(|t| t.id == tab_id)
                {
                    tab.disk_conflict = false;
                    tab.dirty = true;
                    if let Some(path) = tab.file_path.as_deref()
                        && let Ok(cap) = docs.open_document(path)
                    {
                        tab.doc_id = Some(cap.document_id);
                        tab.doc_version = Some(cap.version);
                    }
                    v.status_is_error = false;
                    v.status_note = "Kept in-editor changes (Ctrl+S to overwrite disk)".into();
                }
                Task::none()
            }
            DetachedAction::DiffActiveWithHead => {
                let diff_req = self.detached_viewers.get_mut(&win_id).and_then(|v| {
                    let root = v.search_root.clone()?;
                    if !root.join(".git").exists() {
                        v.status_is_error = true;
                        v.status_note = "Current folder is not a Git repository".into();
                        return None;
                    }
                    let git_map = v.explorer_git_statuses.clone();
                    let tab = v.active_tab_mut()?;
                    let rel = tab.rel_path.clone()?;
                    let target = tab.git_diff_target.clone().unwrap_or_else(|| {
                        let code = git_map.get(&rel).copied().unwrap_or('M');
                        let kind = match code {
                            '?' => FileChangeKind::Untracked,
                            'A' => FileChangeKind::Added,
                            'D' => FileChangeKind::Deleted,
                            'U' => FileChangeKind::Conflicted,
                            _ => FileChangeKind::Modified,
                        };
                        DetachedGitDiffTarget::WorkingFile {
                            repo_path: root,
                            file: FileChange {
                                path: rel.clone(),
                                kind,
                                staged: false,
                            },
                        }
                    });
                    tab.git_diff_target = Some(target.clone());
                    tab.mode = tab.last_text_diff_mode;
                    let tid = tab.id;
                    v.request_id = v.request_id.wrapping_add(1);
                    v.status_is_error = false;
                    v.status_note = format!("Loading Git diff for {rel}…");
                    Some((tid, v.request_id, target))
                });
                if let Some((tid, rid, target)) = diff_req {
                    return self.spawn_detached_git_diff_task(win_id, tid, rid, target);
                }
                Task::none()
            }
            DetachedAction::QuickOpenConfirmSelected => {
                let action_plan = self.detached_viewers.get_mut(&win_id).and_then(|v| {
                    let q = v.quick_open_query.trim().to_string();
                    if q.starts_with('@') {
                        let syms = v.quick_open_symbols(80);
                        let (sym, _) = syms.get(v.quick_open_selected)?.clone();
                        let cfg = crate::views::code_editor::NumberedEditorConfig::studio(
                            v.editor_font_size,
                            v.editor_word_wrap,
                        );
                        if let Some(tab) = v.active_tab_mut()
                            && tab.is_editable()
                        {
                            tab.mode = DetachedViewMode::Edit;
                            tab.ensure_editor_hydrated();
                            jump_editor_to_span(
                                &mut tab.editor_content,
                                sym.line_idx,
                                0,
                                0,
                            );
                            let scroll_t =
                                crate::views::code_editor::scroll_numbered_editor_to_line(
                                    &tab.editor_content,
                                    &mut tab.editor_scroll,
                                    sym.line_idx,
                                    cfg,
                                );
                            v.quick_open_visible = false;
                            v.status_is_error = false;
                            v.status_note = format!(
                                "{} {} (Ln {})",
                                sym.kind_badge,
                                sym.name,
                                sym.line_idx + 1
                            );
                            return Some((None, Some(scroll_t)));
                        }
                        return None;
                    }
                    if q.starts_with(':') {
                        let cfg = crate::views::code_editor::NumberedEditorConfig::studio(
                            v.editor_font_size,
                            v.editor_word_wrap,
                        );
                        if let Some(tab) = v.active_tab_mut()
                            && tab.is_editable()
                        {
                            tab.mode = DetachedViewMode::Edit;
                            tab.ensure_editor_hydrated();
                            let total = tab.editor_content.line_count();
                            if let Some((ln, col)) = parse_goto_line_spec(&q, total) {
                                jump_editor_to_span(&mut tab.editor_content, ln, col, col);
                                let scroll_t =
                                    crate::views::code_editor::scroll_numbered_editor_to_line(
                                        &tab.editor_content,
                                        &mut tab.editor_scroll,
                                        ln,
                                        cfg,
                                    );
                                v.quick_open_visible = false;
                                v.status_is_error = false;
                                v.status_note = format!("Jumped to Ln {}, Col {}", ln + 1, col + 1);
                                return Some((None, Some(scroll_t)));
                            }
                        }
                        return None;
                    }
                    let list = v.quick_open_candidates(60);
                    let chosen = list.get(v.quick_open_selected)?.rel_path.clone();
                    v.quick_open_visible = false;
                    let abs = v
                        .tabs
                        .iter()
                        .find(|t| t.rel_path.as_deref() == Some(chosen.as_str()))
                        .and_then(|t| t.file_path.clone())
                        .or_else(|| v.search_root.as_ref().map(|r| r.join(&chosen)))?;
                    Some((Some(abs), None))
                });
                match action_plan {
                    Some((Some(abs), _)) => self.open_or_focus_file_tab(win_id, abs, None, false),
                    Some((None, Some(scroll_t))) => scroll_t,
                    _ => Task::none(),
                }
            }
            DetachedAction::QuickOpenSelectRelPath(rel) => {
                let abs_opt = self.detached_viewers.get_mut(&win_id).and_then(|v| {
                    v.quick_open_visible = false;
                    v.tabs
                        .iter()
                        .find(|t| t.rel_path.as_deref() == Some(rel.as_str()))
                        .and_then(|t| t.file_path.clone())
                        .or_else(|| v.search_root.as_ref().map(|r| r.join(&rel)))
                });
                if let Some(abs) = abs_opt {
                    return self.open_or_focus_file_tab(win_id, abs, None, false);
                }
                Task::none()
            }
            DetachedAction::ConfirmCloseTab { tab_id, save_first } => {
                let idx = self
                    .detached_viewers
                    .get(&win_id)
                    .and_then(|v| v.tabs.iter().position(|t| t.id == tab_id));
                let Some(i) = idx else {
                    if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                        v.pending_close_tab = None;
                    }
                    return Task::none();
                };
                if save_first {
                    let (ok, task) = self.save_tab_by_index(win_id, i);
                    if ok && let Some(v) = self.detached_viewers.get_mut(&win_id) {
                        v.remove_tab_at(i);
                    }
                    task
                } else {
                    if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                        v.remove_tab_at(i);
                    }
                    Task::none()
                }
            }
            DetachedAction::RevertEditor => {
                let svc = self.service.clone();
                let docs = &svc.docs;
                if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                    let root = v.search_root.clone();
                    if let Some(tab) = v.active_tab_mut()
                        && let Some(path) = tab.file_path.clone()
                    {
                        tab.load_local_file(docs, &path, root.as_deref());
                        v.recompute_in_file_matches(false);
                        v.status_is_error = false;
                        v.status_note = format!("Reverted {}", file_display_name(&path));
                    }
                }
                Task::none()
            }
            DetachedAction::SetEditorEol(target_eol) => {
                let svc = self.service.clone();
                let docs = &svc.docs;
                let host_repo_path = self.current_repo().map(|r| r.path);
                let mut should_refresh_host = false;
                if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                    v.status_menu = None;
                    if let Some(tab) = v.active_tab_mut() {
                        if tab.line_ending == target_eol {
                            return Task::none();
                        }
                        if let Some(doc_id) = tab.doc_id.clone() {
                            if tab.dirty {
                                if let Err(e) = docs.set_document_line_ending(&doc_id, target_eol) {
                                    v.status_is_error = true;
                                    v.status_note = format!("EOL change failed: {e}");
                                } else {
                                    tab.line_ending = target_eol;
                                    v.status_is_error = false;
                                    v.status_note = format!(
                                        "EOL set to {} (Ctrl+S to save)",
                                        target_eol.label()
                                    );
                                }
                            } else if let Some(ver) = tab.doc_version.clone() {
                                match docs.convert_document_on_disk(
                                    &doc_id,
                                    &ver,
                                    target_eol,
                                    tab.encoding,
                                ) {
                                    Ok(res) => {
                                        tab.doc_version = Some(res.version);
                                        tab.line_ending = res.line_ending;
                                        tab.encoding = res.encoding;
                                        tab.trailing_newline = res.content.ends_with('\n');
                                        tab.raw_text = Some(res.content);
                                        tab.editor_hydrated = false;
                                        tab.preview_hydrated = false;
                                        tab.hydrate_active_mode();
                                        if let Some(p) = tab.file_path.as_deref() {
                                            should_refresh_host = host_repo_path
                                                .as_ref()
                                                .is_some_and(|rp| p.starts_with(rp));
                                        }
                                        v.status_is_error = false;
                                        v.status_note =
                                            format!("Converted to {}", res.line_ending.label());
                                    }
                                    Err(e) => {
                                        v.status_is_error = true;
                                        v.status_note = format!("Convert EOL failed: {e}");
                                    }
                                }
                            }
                        }
                    }
                }
                if should_refresh_host {
                    return self.update(Message::RefreshSelected);
                }
                Task::none()
            }
            DetachedAction::SaveEditorWithEncoding(encoding) => {
                let svc = self.service.clone();
                let docs = &svc.docs;
                let host_repo_path = self.current_repo().map(|r| r.path);
                let mut should_refresh_host = false;
                if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                    v.status_menu = None;
                    if let Some(tab) = v.active_tab_mut()
                        && let (Some(doc_id), Some(ver)) =
                            (tab.doc_id.clone(), tab.doc_version.clone())
                    {
                        let res = if tab.dirty {
                            let content = if tab.editor_hydrated {
                                format_editor_text_for_save(
                                    &tab.editor_content,
                                    tab.trailing_newline,
                                )
                            } else {
                                tab.raw_text.clone().unwrap_or_default()
                            };
                            docs.save_document_detailed(
                                &doc_id,
                                &content,
                                &ver,
                                Some(tab.line_ending),
                                Some(encoding),
                            )
                        } else {
                            docs.convert_document_on_disk(&doc_id, &ver, tab.line_ending, encoding)
                        };
                        match res {
                            Ok(saved) => {
                                tab.doc_version = Some(saved.version);
                                tab.line_ending = saved.line_ending;
                                tab.encoding = saved.encoding;
                                tab.trailing_newline = saved.content.ends_with('\n');
                                tab.raw_text = Some(saved.content);
                                tab.dirty = false;
                                tab.editor_hydrated = false;
                                tab.preview_hydrated = false;
                                tab.hydrate_active_mode();
                                if let Some(p) = tab.file_path.as_deref() {
                                    should_refresh_host = host_repo_path
                                        .as_ref()
                                        .is_some_and(|rp| p.starts_with(rp));
                                }
                                v.recompute_in_file_matches(true);
                                v.status_is_error = false;
                                v.status_note = if saved.lossy_chars > 0 {
                                    format!(
                                        "Saved as {} ({} lossy char(s))",
                                        saved.encoding.label(),
                                        saved.lossy_chars
                                    )
                                } else {
                                    format!("Saved as {}", saved.encoding.label())
                                };
                            }
                            Err(e) => {
                                v.status_is_error = true;
                                v.status_note = format!("Encoding save failed: {e}");
                            }
                        }
                    }
                }
                if should_refresh_host {
                    return self.update(Message::RefreshSelected);
                }
                Task::none()
            }
            DetachedAction::ReopenEditorWithEncoding(encoding) => {
                let svc = self.service.clone();
                let docs = &svc.docs;
                if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                    v.status_menu = None;
                    if let Some(tab) = v.active_tab_mut()
                        && let Some(path) = tab.file_path.clone()
                    {
                        match docs.open_document_with_encoding(&path, encoding) {
                            Ok(cap) => {
                                tab.doc_id = Some(cap.document_id);
                                tab.doc_version = Some(cap.version);
                                tab.line_ending = cap.line_ending;
                                tab.encoding = cap.encoding;
                                tab.trailing_newline = cap.content.ends_with('\n');
                                tab.raw_text = Some(cap.content);
                                tab.dirty = false;
                                tab.editor_hydrated = false;
                                tab.preview_hydrated = false;
                                tab.hydrate_active_mode();
                                v.recompute_in_file_matches(false);
                                v.status_is_error = false;
                                v.status_note = format!("Reopened with {}", encoding.label());
                            }
                            Err(e) => {
                                v.status_is_error = true;
                                v.status_note = format!("Reopen failed: {e}");
                            }
                        }
                    }
                }
                Task::none()
            }
            DetachedAction::RefreshExplorer => self.spawn_explorer_scan_task(win_id),
            DetachedAction::ToggleExplorerShowIgnored => {
                let should_rescan = if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                    v.explorer_show_ignored = !v.explorer_show_ignored;
                    if !v.explorer_show_ignored {
                        v.rebuild_explorer_tree();
                    }
                    v.explorer_show_ignored
                } else {
                    false
                };
                if should_rescan {
                    self.spawn_explorer_scan_task(win_id)
                } else {
                    Task::none()
                }
            }
            DetachedAction::ToggleExplorerDir(dir) => {
                let load_params = if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                    v.explorer_menu = None;
                    let expanded = if v.explorer_expanded_dirs.remove(&dir) {
                        false
                    } else {
                        v.explorer_expanded_dirs.insert(dir.clone());
                        true
                    };
                    if expanded
                        && v.explorer_show_ignored
                        && v.is_explorer_path_in_ignored_dir(&dir)
                        && !v.explorer_ignored_dir_children.contains_key(&dir)
                    {
                        v.search_root
                            .clone()
                            .map(|root| (v.explorer_request_id, root, dir))
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some((rid, root, rel_dir)) = load_params {
                    self.spawn_explorer_ignored_dir_task(win_id, rid, root, rel_dir)
                } else {
                    Task::none()
                }
            }
            DetachedAction::ExplorerFilesLoaded(rid, files, ignored, git_map) => {
                let reload_params = if let Some(v) = self.detached_viewers.get_mut(&win_id)
                    && rid == v.explorer_request_id
                {
                    v.explorer_loading = false;
                    v.explorer_files = files;
                    v.explorer_ignored_files = ignored.files;
                    v.explorer_ignored_dirs = ignored.dirs;
                    v.explorer_ignored_dir_children.clear();
                    v.set_explorer_git_statuses(git_map);
                    v.rebuild_explorer_tree();
                    if v.explorer_show_ignored
                        && let Some(root) = v.search_root.clone()
                    {
                        let dirs: Vec<String> = v
                            .explorer_expanded_dirs
                            .iter()
                            .filter(|d| v.is_explorer_path_in_ignored_dir(d))
                            .cloned()
                            .collect();
                        Some((rid, root, dirs))
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some((req_id, root, dirs)) = reload_params
                    && !dirs.is_empty()
                {
                    let tasks = dirs.into_iter().map(|d| {
                        self.spawn_explorer_ignored_dir_task(win_id, req_id, root.clone(), d)
                    });
                    return Task::batch(tasks);
                }
                Task::none()
            }
            DetachedAction::OpenExplorerFile(rel) => {
                let full = self.detached_viewers.get_mut(&win_id).and_then(|v| {
                    v.explorer_menu = None;
                    v.search_root.as_ref().map(|r| r.join(&rel))
                });
                if let Some(p) = full {
                    return self.open_or_focus_file_tab(win_id, p, None, false);
                }
                Task::none()
            }
            DetachedAction::RevealExplorerEntry(rel) => {
                let abs = self.detached_viewers.get_mut(&win_id).and_then(|v| {
                    v.explorer_menu = None;
                    v.search_root.as_ref().map(|r| r.join(&rel))
                });
                if let Some(p) = abs {
                    return self.update(Message::RevealPathInExplorer(p));
                }
                Task::none()
            }
            DetachedAction::ConfirmDeleteExplorerEntry => {
                let params = self.detached_viewers.get_mut(&win_id).and_then(|v| {
                    let pending = v.pending_delete.take()?;
                    let root = v.search_root.clone()?;
                    v.status_is_error = false;
                    v.status_note = format!(
                        "Deleting {} '{}'…",
                        if pending.is_dir { "folder" } else { "file" },
                        pending.rel_path
                    );
                    Some((root, pending.rel_path, pending.is_dir))
                });
                let Some((root, rel_path, is_dir)) = params else {
                    return Task::none();
                };
                let abs_path = root.join(&rel_path);
                let rel_for_task = rel_path.clone();
                Task::perform(
                    async move {
                        let result = tokio::task::spawn_blocking(move || {
                            gitfleet_git::delete_workspace_entry(&root, &rel_for_task)
                                .map_err(|e| e.to_string())
                        })
                        .await
                        .unwrap_or_else(|e| Err(format!("Delete task failed: {e}")));
                        (rel_path, is_dir, abs_path, result)
                    },
                    move |(rel_path, is_dir, abs_path, result)| {
                        Message::Detached(
                            win_id,
                            DetachedAction::ExplorerEntryDeleted {
                                rel_path,
                                is_dir,
                                abs_path,
                                result,
                            },
                        )
                    },
                )
            }
            DetachedAction::ExplorerEntryDeleted {
                rel_path,
                is_dir,
                abs_path,
                result,
            } => {
                let kind_label = if is_dir { "folder" } else { "file" };
                let host_repo_path = self.current_repo().map(|r| r.path);
                match result {
                    Ok(removed) => {
                        if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                            v.close_tabs_under_path(&abs_path, is_dir);
                            let matches_deleted = |candidate: &str| -> bool {
                                candidate == rel_path
                                    || (is_dir
                                        && candidate
                                            .strip_prefix(rel_path.as_str())
                                            .is_some_and(|rest| rest.starts_with('/')))
                            };
                            v.explorer_expanded_dirs.retain(|d| !matches_deleted(d));
                            v.explorer_ignored_dir_children
                                .retain(|d, _| !matches_deleted(d));
                            v.status_is_error = false;
                            v.status_note = if removed {
                                format!("Deleted {kind_label} '{rel_path}'")
                            } else {
                                format!("'{rel_path}' was already removed")
                            };
                        }

                        // Clear Host active editor if the deleted entry was currently open there
                        if let Some(open_p) = self.open_doc_path.as_deref()
                            && (open_p == abs_path || (is_dir && open_p.starts_with(&abs_path)))
                        {
                            self.active_file_path = None;
                            self.open_doc_raw_text = None;
                            self.open_doc_id = None;
                            self.open_doc_version = None;
                            self.open_doc_path = None;
                            self.editor_dirty = false;
                            self.preview_hydrated = false;
                            self.editor_hydrated = false;
                            self.preview_state = PreviewState::Empty;
                            self.editor_content = text_editor::Content::new();
                            self.diff_title = "No file selected".into();
                            self.clear_diff_parsed_and_cache();
                        }

                        let mut tasks = vec![self.spawn_explorer_scan_task(win_id)];
                        if host_repo_path
                            .as_deref()
                            .is_some_and(|rp| abs_path.starts_with(rp))
                        {
                            tasks.push(self.update(Message::RefreshSelected));
                        }
                        return Task::batch(tasks);
                    }
                    Err(err) => {
                        if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                            v.status_is_error = true;
                            v.status_note = format!("Delete failed: {err}");
                        }
                    }
                }
                Task::none()
            }
            DetachedAction::SubmitExplorerRename => {
                let params = self.detached_viewers.get_mut(&win_id).and_then(|v| {
                    let root = v.search_root.clone()?;
                    let modal = v.rename_modal.as_mut()?;
                    match validate_rename_input(&modal.rel_path, &modal.new_name) {
                        Ok(true) => {}
                        Ok(false) => {
                            v.rename_modal = None;
                            return None;
                        }
                        Err(err) => {
                            modal.error = Some(err);
                            return None;
                        }
                    }
                    let old_rel = modal.rel_path.clone();
                    let new_name = modal.new_name.trim().to_string();
                    let is_dir = modal.is_dir;
                    v.status_is_error = false;
                    v.status_note = format!(
                        "Renaming {} '{}'…",
                        if is_dir { "folder" } else { "file" },
                        old_rel
                    );
                    Some((root, old_rel, new_name, is_dir))
                });
                let Some((root, old_rel, new_name, is_dir)) = params else {
                    return Task::none();
                };
                let old_abs = root.join(&old_rel);
                let rel_for_task = old_rel.clone();
                Task::perform(
                    async move {
                        let result = tokio::task::spawn_blocking(move || {
                            gitfleet_git::rename_workspace_entry(&root, &rel_for_task, &new_name)
                                .map_err(|e| e.to_string())
                        })
                        .await
                        .unwrap_or_else(|e| Err(format!("Rename task failed: {e}")));
                        (old_rel, is_dir, old_abs, result)
                    },
                    move |(old_rel, is_dir, old_abs, result)| {
                        Message::Detached(
                            win_id,
                            DetachedAction::ExplorerEntryRenamed {
                                old_rel,
                                is_dir,
                                old_abs,
                                result,
                            },
                        )
                    },
                )
            }
            DetachedAction::ExplorerEntryRenamed {
                old_rel,
                is_dir,
                old_abs,
                result,
            } => {
                let kind_label = if is_dir { "folder" } else { "file" };
                let host_repo_path = self.current_repo().map(|r| r.path);
                let docs = self.service.clone();
                match result {
                    Ok(new_rel) => {
                        let new_abs = self
                            .detached_viewers
                            .get(&win_id)
                            .and_then(|v| v.search_root.as_ref().map(|r| r.join(&new_rel)))
                            .unwrap_or_else(|| {
                                old_abs
                                    .parent()
                                    .map(|p| {
                                        let leaf = new_rel.rsplit('/').next().unwrap_or(&new_rel);
                                        p.join(leaf)
                                    })
                                    .unwrap_or_else(|| old_abs.clone())
                            });

                        if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                            v.rename_modal = None;
                            v.rename_tabs_under_path(&docs.docs, &old_abs, &new_abs, is_dir);
                            v.explorer_expanded_dirs = v
                                .explorer_expanded_dirs
                                .iter()
                                .map(|d| {
                                    remap_rel_path(d, &old_rel, &new_rel, is_dir)
                                        .unwrap_or_else(|| d.clone())
                                })
                                .collect();
                            v.explorer_ignored_dir_children.clear();
                            v.status_is_error = false;
                            v.status_note =
                                format!("Renamed {kind_label} '{old_rel}' → '{new_rel}'");
                        }

                        // Sync Host active document if the renamed file/folder was open in Host
                        let host_updated_path = self.open_doc_path.as_deref().and_then(|open_p| {
                            if open_p == old_abs {
                                Some(new_abs.clone())
                            } else if is_dir {
                                open_p.strip_prefix(&old_abs).ok().map(|t| new_abs.join(t))
                            } else {
                                None
                            }
                        });
                        if let Some(next_host_abs) = host_updated_path {
                            self.open_doc_path = Some(next_host_abs.clone());
                            self.open_doc_language = CodeLanguage::from_path(&next_host_abs);
                            if let Some(rp) = host_repo_path.as_deref()
                                && let Ok(rel) = next_host_abs.strip_prefix(rp)
                            {
                                let rel_str = rel.to_string_lossy().replace('\\', "/");
                                self.active_file_path = Some(rel_str.clone());
                                self.diff_title = rel_str;
                            }
                            if let Ok(cap) = docs.docs.open_document(&next_host_abs) {
                                self.open_doc_id = Some(cap.document_id);
                                self.open_doc_version = Some(cap.version);
                                if !self.editor_dirty {
                                    self.open_doc_raw_text = Some(cap.content);
                                }
                            }
                        }

                        let mut tasks = vec![self.spawn_explorer_scan_task(win_id)];
                        if host_repo_path
                            .as_deref()
                            .is_some_and(|rp| old_abs.starts_with(rp) || new_abs.starts_with(rp))
                        {
                            self.ignored_entries_stale = !self.show_ignored_files;
                            tasks.push(self.update(Message::RefreshSelected));
                        }
                        return Task::batch(tasks);
                    }
                    Err(err) => {
                        if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                            if let Some(m) = &mut v.rename_modal {
                                m.error = Some(err.clone());
                            }
                            v.status_is_error = true;
                            v.status_note = format!("Rename failed: {err}");
                        }
                    }
                }
                Task::none()
            }
            DetachedAction::SubmitExplorerGitignore => {
                let params = self.detached_viewers.get(&win_id).and_then(|v| {
                    let root = v.search_root.clone()?;
                    let modal = v.gitignore_modal.as_ref()?;
                    let rules = modal.resolved_rules();
                    Some((root, modal.layer, modal.target_file_label(), rules))
                });
                let Some((root, layer, target_label, rules)) = params else {
                    return Task::none();
                };
                if rules.is_empty() {
                    if let Some(v) = self.detached_viewers.get_mut(&win_id)
                        && let Some(m) = &mut v.gitignore_modal
                    {
                        m.error = Some("No valid ignore pattern to append.".into());
                    }
                    return Task::none();
                }

                match self.service.security.append_ignore_rules(&root, layer, &rules) {
                    Ok(res) => {
                        if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                            v.gitignore_modal = None;
                            v.status_is_error = false;
                            v.status_note = if res.added == 0 {
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
                        }

                        let mut tasks = vec![self.spawn_explorer_scan_task(win_id)];
                        if self
                            .current_repo()
                            .is_some_and(|rp| rp.path == root || root.starts_with(&rp.path))
                        {
                            self.refresh_vault_and_ignore();
                            self.ignored_entries_stale = !self.show_ignored_files;
                            tasks.push(self.update(Message::RefreshSelected));
                        }
                        return Task::batch(tasks);
                    }
                    Err(err) => {
                        if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                            if let Some(m) = &mut v.gitignore_modal {
                                m.error = Some(err.to_string());
                            }
                            v.status_is_error = true;
                            v.status_note = format!("Failed to update {target_label}: {err}");
                        }
                    }
                }
                Task::none()
            }
            DetachedAction::SingleFilePicked(Some(path)) => {
                self.open_or_focus_file_tab(win_id, path, None, false)
            }
            DetachedAction::ReloadFiles => {
                let target = self.detached_viewers.get(&win_id).and_then(|v| {
                    v.active_tab().map(|tab| {
                        (
                            tab.id,
                            tab.left_path.clone(),
                            tab.right_path.clone(),
                            tab.file_path.clone(),
                            tab.last_text_diff_mode,
                        )
                    })
                });
                if let Some((_, Some(left), Some(right), _, pref)) = target {
                    if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                        v.request_id = v.request_id.wrapping_add(1);
                        let req_id = v.request_id;
                        v.loading = true;
                        v.status_is_error = false;
                        v.status_note = "Reloading comparison…".into();
                        return super::io::spawn_compare_task(win_id, req_id, left, right, pref);
                    }
                } else if let Some((tab_id, _, _, Some(path), _)) = target {
                    let req = self.detached_viewers.get_mut(&win_id).and_then(|v| {
                        v.status_is_error = false;
                        v.status_note = format!("Reloading {}…", file_display_name(&path));
                        let tab = v.tabs.iter_mut().find(|t| t.id == tab_id)?;
                        tab.file_load_req_id = tab.file_load_req_id.wrapping_add(1);
                        tab.loading_file = true;
                        Some(tab.file_load_req_id)
                    });
                    if let Some(f_req) = req {
                        return self.spawn_tab_file_load_task(win_id, tab_id, f_req, path, None);
                    }
                }
                Task::none()
            }
            DetachedAction::OpenSearchFile(path) => {
                self.open_or_focus_file_tab(win_id, path, None, false)
            }
            DetachedAction::OpenSearchMatch {
                path,
                line_number,
                col_start,
                col_end,
            } => self.open_or_focus_file_tab(
                win_id,
                path,
                Some((line_number.saturating_sub(1), col_start, col_end)),
                true,
            ),
            DetachedAction::RunSearch => self.spawn_detached_search_task(win_id),
            DetachedAction::ToggleSearchCase => {
                let rerun = if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                    v.search_options.case_sensitive = !v.search_options.case_sensitive;
                    v.search_confirm_replace_all = false;
                    v.search_has_run && !v.search_query.is_empty()
                } else {
                    false
                };
                if rerun {
                    self.spawn_detached_search_task(win_id)
                } else {
                    Task::none()
                }
            }
            DetachedAction::ToggleSearchWholeWord => {
                let rerun = if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                    v.search_options.whole_word = !v.search_options.whole_word;
                    v.search_confirm_replace_all = false;
                    v.search_has_run && !v.search_query.is_empty()
                } else {
                    false
                };
                if rerun {
                    self.spawn_detached_search_task(win_id)
                } else {
                    Task::none()
                }
            }
            DetachedAction::ToggleSearchRegex => {
                let rerun = if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                    v.search_options.regex = !v.search_options.regex;
                    v.search_confirm_replace_all = false;
                    v.search_has_run && !v.search_query.is_empty()
                } else {
                    false
                };
                if rerun {
                    self.spawn_detached_search_task(win_id)
                } else {
                    Task::none()
                }
            }
            DetachedAction::ToggleSearchGitignore => {
                let rerun = if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                    v.search_use_gitignore = !v.search_use_gitignore;
                    v.search_confirm_replace_all = false;
                    v.search_has_run
                } else {
                    false
                };
                if rerun {
                    self.spawn_detached_search_task(win_id)
                } else {
                    Task::none()
                }
            }
            DetachedAction::SearchRootPicked(Some(dir)) => {
                let rerun = if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                    v.search_root_name = Some(file_display_name(&dir));
                    v.search_root = Some(dir);
                    v.explorer_expanded_dirs.clear();
                    v.clear_explorer_ignored_cache();
                    v.search_confirm_replace_all = false;
                    v.search_has_run
                } else {
                    false
                };
                let exp_task = self.spawn_explorer_scan_task(win_id);
                if rerun {
                    Task::batch([exp_task, self.spawn_detached_search_task(win_id)])
                } else {
                    exp_task
                }
            }
            DetachedAction::ReplaceInSearchFile(path) => {
                let params = self.detached_viewers.get_mut(&win_id).and_then(|v| {
                    if v.search_query.is_empty() {
                        return None;
                    }
                    v.search_request_id = v.search_request_id.wrapping_add(1);
                    v.search_running = true;
                    v.search_confirm_replace_all = false;
                    Some((
                        v.search_request_id,
                        v.search_query.clone(),
                        v.search_replace_text.clone(),
                        v.search_options,
                    ))
                });
                let Some((rid, query, repl, opts)) = params else {
                    return Task::none();
                };
                let target_paths = vec![path];
                Task::perform(
                    async move {
                        let paths_clone = target_paths.clone();
                        let report = tokio::task::spawn_blocking(move || {
                            replace_in_files(&paths_clone, &query, &repl, opts)
                        })
                        .await
                        .unwrap_or_else(|e| ReplaceReport {
                            errors: vec![format!("Replace failed: {e}")],
                            ..Default::default()
                        });
                        (rid, report, target_paths)
                    },
                    move |(id, rep, touched)| {
                        Message::Detached(
                            win_id,
                            DetachedAction::SearchReplaceCompleted(id, rep, touched),
                        )
                    },
                )
            }
            DetachedAction::ReplaceSingleSearchMatch { path, line_number } => {
                let params = self.detached_viewers.get_mut(&win_id).and_then(|v| {
                    if v.search_query.is_empty() {
                        return None;
                    }
                    v.search_request_id = v.search_request_id.wrapping_add(1);
                    v.search_running = true;
                    v.search_confirm_replace_all = false;
                    Some((
                        v.search_request_id,
                        v.search_query.clone(),
                        v.search_replace_text.clone(),
                        v.search_options,
                    ))
                });
                let Some((rid, query, repl, opts)) = params else {
                    return Task::none();
                };
                let p_clone = path.clone();
                Task::perform(
                    async move {
                        let report = tokio::task::spawn_blocking(move || {
                            let mut rep = ReplaceReport::default();
                            match replace_in_file(&p_clone, &query, &repl, opts, Some(line_number)) {
                                Ok(0) => {}
                                Ok(n) => {
                                    rep.files_modified = 1;
                                    rep.replacements = n;
                                }
                                Err(e) => rep.errors.push(e),
                            }
                            rep
                        })
                        .await
                        .unwrap_or_else(|e| ReplaceReport {
                            errors: vec![format!("Replace failed: {e}")],
                            ..Default::default()
                        });
                        (rid, report, vec![path])
                    },
                    move |(id, rep, touched)| {
                        Message::Detached(
                            win_id,
                            DetachedAction::SearchReplaceCompleted(id, rep, touched),
                        )
                    },
                )
            }
            DetachedAction::ConfirmReplaceAllInSearch => {
                let params = self.detached_viewers.get_mut(&win_id).and_then(|v| {
                    if v.search_query.is_empty() || v.search_result.groups.is_empty() {
                        v.search_confirm_replace_all = false;
                        return None;
                    }
                    v.search_request_id = v.search_request_id.wrapping_add(1);
                    v.search_running = true;
                    v.search_confirm_replace_all = false;
                    let paths: Vec<PathBuf> = v
                        .search_result
                        .groups
                        .iter()
                        .map(|g| g.abs_path.clone())
                        .collect();
                    Some((
                        v.search_request_id,
                        paths,
                        v.search_query.clone(),
                        v.search_replace_text.clone(),
                        v.search_options,
                    ))
                });
                let Some((rid, paths, query, repl, opts)) = params else {
                    return Task::none();
                };
                Task::perform(
                    async move {
                        let p_clone = paths.clone();
                        let report = tokio::task::spawn_blocking(move || {
                            replace_in_files(&p_clone, &query, &repl, opts)
                        })
                        .await
                        .unwrap_or_else(|e| ReplaceReport {
                            errors: vec![format!("Replace All failed: {e}")],
                            ..Default::default()
                        });
                        (rid, report, paths)
                    },
                    move |(id, rep, touched)| {
                        Message::Detached(
                            win_id,
                            DetachedAction::SearchReplaceCompleted(id, rep, touched),
                        )
                    },
                )
            }
            DetachedAction::SearchReplaceCompleted(rid, report, touched_paths) => {
                let svc = self.service.clone();
                let docs = &svc.docs;
                let host_repo_path = self.current_repo().map(|r| r.path);
                let mut should_reload_search = false;
                let mut touched_host_repo = false;

                if let Some(v) = self.detached_viewers.get_mut(&win_id) {
                    if rid != v.search_request_id {
                        return Task::none();
                    }
                    v.search_running = false;
                    if !report.errors.is_empty() {
                        v.status_is_error = true;
                        v.status_note = format!(
                            "Replaced {} in {} file(s) ({} error(s))",
                            report.replacements,
                            report.files_modified,
                            report.errors.len()
                        );
                    } else {
                        v.status_is_error = false;
                        v.status_note = format!(
                            "Replaced {} across {} file(s)",
                            report.replacements, report.files_modified
                        );
                    }

                    // Reload any clean open tab affected by the replace
                    let root = v.search_root.clone();
                    for tab in &mut v.tabs {
                        if let Some(p) = tab.file_path.clone()
                            && !tab.dirty
                            && touched_paths.iter().any(|tp| tp == &p)
                        {
                            tab.load_local_file(docs, &p, root.as_deref());
                        }
                    }
                    v.recompute_in_file_matches(true);
                    should_reload_search = report.files_modified > 0;
                    if report.files_modified > 0
                        && let Some(rp) = host_repo_path.as_ref()
                    {
                        touched_host_repo = touched_paths.iter().any(|p| p.starts_with(rp));
                    }
                }

                if report.files_modified > 0
                    && let Some(host_open) = self.open_doc_path.clone()
                    && !self.editor_dirty
                    && touched_paths.iter().any(|p| p == &host_open)
                    && let Ok(cap) = docs.open_document(&host_open)
                {
                    self.open_doc_id = Some(cap.document_id);
                    self.open_doc_version = Some(cap.version);
                    self.open_doc_encoding = cap.encoding;
                    self.open_doc_line_ending = cap.line_ending;
                    self.open_doc_raw_text = Some(cap.content);
                    self.preview_hydrated = false;
                    self.editor_hydrated = false;
                    if self.bottom_mode == crate::app::BottomViewMode::Preview {
                        self.ensure_preview_hydrated();
                    } else if self.bottom_mode == crate::app::BottomViewMode::Edit {
                        self.ensure_editor_hydrated();
                    }
                }

                let mut tasks = Vec::new();
                if should_reload_search {
                    tasks.push(self.spawn_detached_search_task(win_id));
                }
                if touched_host_repo {
                    tasks.push(self.update(Message::RefreshSelected));
                }
                Task::batch(tasks)
            }
            DetachedAction::PayloadLoaded(req_id, payload) => {
                let svc = self.service.clone();
                let docs = &svc.docs;
                let theme_mode = self.theme_mode;
                let Some(v) = self.detached_viewers.get_mut(&win_id) else {
                    return Task::none();
                };
                if req_id != v.request_id {
                    return Task::none();
                }
                v.loading = false;

                let is_compare = payload.left_path.is_some() && payload.right_path.is_some();
                let target_idx = if is_compare {
                    v.tabs
                        .iter()
                        .position(|t| t.left_path.is_some() && t.right_path.is_some())
                } else if let Some(sp) = &payload.single_file_path {
                    v.tabs
                        .iter()
                        .position(|t| t.file_path.as_ref() == Some(sp) && t.left_path.is_none())
                } else {
                    None
                };

                let idx = match target_idx {
                    Some(i) => i,
                    None => {
                        let id = v.alloc_tab_id();
                        v.tabs.push(StudioTab::new_empty(id, payload.title.clone()));
                        v.tabs.len() - 1
                    }
                };

                let root = v.search_root.clone();
                let tab = &mut v.tabs[idx];
                tab.title = payload.title;
                tab.left_path = payload.left_path;
                tab.right_path = payload.right_path;
                tab.git_diff_target = None;
                tab.diff_parsed = payload.parsed_diff;
                if let Some(img) = &payload.image_diff {
                    tab.image_state.swapped = false;
                    tab.image_state.zoom = ImageDiffZoom::Fit;
                    tab.image_state.blink_after = false;
                    tab.image_state.swipe = match (&img.before, &img.after) {
                        (None, Some(_)) => 0.0,
                        (Some(_), None) => 1.0,
                        _ => 0.5,
                    };
                }
                tab.diff_image = payload.image_diff;
                tab.diff_csv = payload.csv_diff;
                tab.csv_filter.clear();
                tab.loading_file = false;
                tab.editor_hydrated = false;
                tab.preview_hydrated = false;
                tab.preview_state = PreviewState::Empty;
                tab.preview_editor = text_editor::Content::new();

                if let Some((path, text_str, enc, eol)) = payload.editable_source {
                    tab.language = CodeLanguage::from_path(&path);
                    tab.rel_path = root
                        .as_deref()
                        .and_then(|r| path.strip_prefix(r).ok())
                        .map(|p| p.to_string_lossy().replace('\\', "/"));
                    if let Ok(cap) = docs.open_document(&path) {
                        tab.doc_id = Some(cap.document_id);
                        tab.doc_version = Some(cap.version);
                        tab.encoding = cap.encoding;
                        tab.line_ending = cap.line_ending;
                        tab.trailing_newline = cap.content.ends_with('\n');
                        tab.raw_text = Some(cap.content);
                    } else {
                        tab.doc_id = None;
                        tab.doc_version = None;
                        tab.encoding = enc;
                        tab.line_ending = eol;
                        tab.trailing_newline = text_str.ends_with('\n');
                        tab.raw_text = Some(text_str);
                    }
                    tab.file_path = Some(path);
                    tab.dirty = false;
                } else {
                    tab.file_path = payload.single_file_path;
                    tab.language = tab
                        .file_path
                        .as_deref()
                        .map(CodeLanguage::from_path)
                        .unwrap_or(CodeLanguage::PlainText);
                    tab.doc_id = None;
                    tab.doc_version = None;
                    tab.raw_text = payload.preview_source.and_then(|(_, t)| t);
                    tab.editor_content = text_editor::Content::new();
                    tab.dirty = false;
                }

                tab.mode = payload.preferred_mode;
                tab.hydrate_active_mode();
                tab.reset_scroll_state();
                tab.rebuild_diff_cache(theme_mode);
                let scroll_task = tab.reset_scroll_task();

                v.active_tab = Some(idx);
                v.status_is_error = false;
                v.status_note = payload.status_note;
                v.recompute_in_file_matches(false);
                scroll_task
            }
            other => self.update_detached_viewer_local(win_id, other),
        }
    }
}
