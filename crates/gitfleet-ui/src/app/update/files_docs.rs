use crate::app::{
    ActiveFocusPane, BottomViewMode, DiffGapExpandMode, DiffSelectionSide, DiffTextPos,
    DiffTextSelection, FilesViewMode, GitFleetApp, GraphSelection, Message,
};
use crate::views::commit_files::apply_file_selection;
use crate::views::diff_viewer::{
    build_diff_layout, diff_line_text_at, diff_row_count, extract_selected_diff_text,
    map_scroll_y, word_bounds_at,
};
use crate::{graph, preview};
use gitfleet_docs::LineEnding;
use gitfleet_git::{FileChangeKind, ParsedDiff, parse_unified_diff};
use iced::widget::{scrollable, text_editor};
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
                self.graph_selection = GraphSelection::WorkingTree;
                self.selected_dir_filter = None;
                self.selected_changed_file = None;
                self.selected_changed_files.clear();
                self.file_selection_anchor = None;
                self.focus_pane = ActiveFocusPane::FilesUnstaged;
                self.active_file_path = None;
                self.diff_title = "Select a file to view changes".into();
                self.diff_parsed = ParsedDiff::default();
                self.diff_selection = None;
                self.diff_is_working_file = false;
                if let Some(first) = self.active_changed_files().into_iter().next() {
                    return self.update(Message::SelectChangedFile(first));
                }
                Task::none()
            }
            Message::SelectCommit(oid) => {
                self.graph_selection = GraphSelection::Commit(oid.clone());
                self.selected_dir_filter = None;
                self.selected_changed_file = None;
                self.selected_changed_files.clear();
                self.file_selection_anchor = None;
                self.focus_pane = ActiveFocusPane::FilesCommit;
                self.active_file_path = None;
                self.diff_is_working_file = false;
                self.diff_title = "Loading commit files…".into();
                self.diff_parsed = ParsedDiff::default();
                self.diff_selection = None;
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
                            self.diff_parsed = ParsedDiff::default();
                        }
                    }
                }
                Task::none()
            }
            Message::SetFilesViewMode(mode) => {
                self.files_mode = mode;
                if mode == FilesViewMode::AllFiles
                    && self.all_repo_files.is_empty()
                    && let Some(repo) = self.current_repo()
                {
                    return self.load_all_files_task(repo);
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
                self.selected_all_dir = dir;
                Task::none()
            }
            Message::ToggleAllFilesDirNode(dir) => {
                if !self.expanded_all_dirs.remove(&dir) {
                    self.expanded_all_dirs.insert(dir);
                }
                Task::none()
            }
            Message::AllFilesLoaded(id, res) => {
                if self.active_repo_key().as_ref() == Some(&id) {
                    self.all_repo_files = res.unwrap_or_default();
                }
                Task::none()
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
                self.open_file_for_preview_and_edit(&file.path);

                let Some(repo) = self.current_repo() else {
                    return Task::none();
                };
                let svc = self.service.clone();

                match &self.graph_selection {
                    GraphSelection::WorkingTree => {
                        self.diff_is_working_file = file.kind != FileChangeKind::Untracked;
                        self.diff_is_staged = file.staged;
                        let kind_label = file.kind.state_label(false);
                        let title = if file.staged {
                            format!("Changes of {} (Staged · {kind_label})", file.path)
                        } else {
                            format!("Changes of {} ({kind_label})", file.path)
                        };
                        let untracked = file.kind == FileChangeKind::Untracked;
                        Task::perform(
                            async move {
                                let res = svc
                                    .git
                                    .get_diff(&repo.path, &file.path, file.staged, untracked)
                                    .await
                                    .map_err(|e| e.to_string());
                                (title, res)
                            },
                            |(t, r)| Message::DiffLoaded(t, r),
                        )
                    }
                    GraphSelection::Commit(oid) => {
                        self.diff_is_working_file = false;
                        self.diff_is_staged = false;
                        let oid = oid.clone();
                        let title = format!(
                            "{} @ {}",
                            file.path,
                            &oid[..oid.len().min(8)]
                        );
                        Task::perform(
                            async move {
                                let res = svc
                                    .git
                                    .get_commit_file_diff(&repo.path, &oid, &file.path)
                                    .await
                                    .map_err(|e| e.to_string());
                                (title, res)
                            },
                            |(t, r)| Message::DiffLoaded(t, r),
                        )
                    }
                }
            }
            Message::SelectRepoFile(rel_path) => {
                self.open_file_for_preview_and_edit(&rel_path);
                let Some(repo) = self.current_repo() else {
                    return Task::none();
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

                if !is_dirty && matches!(self.bottom_mode, BottomViewMode::UnifiedDiff | BottomViewMode::SideBySideDiff) {
                    self.bottom_mode = BottomViewMode::Preview;
                }

                self.diff_is_working_file = is_dirty;
                self.diff_is_staged = false;
                let title = rel_path.clone();
                let svc = self.service.clone();
                Task::perform(
                    async move {
                        let res = svc
                            .git
                            .get_diff(&repo.path, &rel_path, false, false)
                            .await
                            .map_err(|e| e.to_string());
                        (title, res)
                    },
                    |(t, r)| Message::DiffLoaded(t, r),
                )
            }
            Message::SetBottomViewMode(mode) => {
                self.bottom_mode = mode;
                self.diff_selection = None;
                Task::none()
            }
            Message::ToggleDiffCompact => {
                self.diff_compact = !self.diff_compact;
                self.diff_selection = None;
                self.diff_left_scroll_y = 0.0;
                self.diff_right_scroll_y = 0.0;
                Task::batch([
                    scrollable::scroll_to(
                        self.diff_left_scroll_id.clone(),
                        scrollable::AbsoluteOffset {
                            x: self.diff_left_scroll_x,
                            y: 0.0,
                        },
                    ),
                    scrollable::scroll_to(
                        self.diff_right_scroll_id.clone(),
                        scrollable::AbsoluteOffset {
                            x: self.diff_right_scroll_x,
                            y: 0.0,
                        },
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
                Task::none()
            }
            Message::DiffLeftScrolled(vp) => {
                let off = vp.absolute_offset();
                let pane_h = vp.bounds().height.max(1.0);
                self.diff_left_viewport_h = pane_h;
                self.diff_right_viewport_h = pane_h;
                let x_changed = (off.x - self.diff_left_scroll_x).abs() >= 0.5;
                let y_changed = (off.y - self.diff_left_scroll_y).abs() >= 0.5;
                if !x_changed && !y_changed {
                    return Task::none();
                }
                self.diff_left_scroll_x = off.x;
                self.diff_left_scroll_y = off.y;

                let target_right_x = off.x.max(0.0);
                let target_right_y = if y_changed {
                    let layout = build_diff_layout(
                        &self.diff_parsed,
                        self.diff_compact,
                        &self.diff_expanded_gaps,
                    );
                    let raw_right_y = map_scroll_y(off.y, &layout.anchors, true);
                    let max_right_y = ((layout.total_right_h + 10.0) - pane_h).max(0.0);
                    raw_right_y.clamp(0.0, max_right_y)
                } else {
                    self.diff_right_scroll_y
                };

                if (target_right_x - self.diff_right_scroll_x).abs() < 0.5
                    && (target_right_y - self.diff_right_scroll_y).abs() < 0.5
                {
                    return Task::none();
                }
                self.diff_right_scroll_x = target_right_x;
                self.diff_right_scroll_y = target_right_y;
                scrollable::scroll_to(
                    self.diff_right_scroll_id.clone(),
                    scrollable::AbsoluteOffset {
                        x: target_right_x,
                        y: target_right_y,
                    },
                )
            }
            Message::DiffRightScrolled(vp) => {
                let off = vp.absolute_offset();
                let pane_h = vp.bounds().height.max(1.0);
                self.diff_left_viewport_h = pane_h;
                self.diff_right_viewport_h = pane_h;
                let x_changed = (off.x - self.diff_right_scroll_x).abs() >= 0.5;
                let y_changed = (off.y - self.diff_right_scroll_y).abs() >= 0.5;
                if !x_changed && !y_changed {
                    return Task::none();
                }
                self.diff_right_scroll_x = off.x;
                self.diff_right_scroll_y = off.y;

                let target_left_x = off.x.max(0.0);
                let target_left_y = if y_changed {
                    let layout = build_diff_layout(
                        &self.diff_parsed,
                        self.diff_compact,
                        &self.diff_expanded_gaps,
                    );
                    let raw_left_y = map_scroll_y(off.y, &layout.anchors, false);
                    let max_left_y = ((layout.total_left_h + 10.0) - pane_h).max(0.0);
                    raw_left_y.clamp(0.0, max_left_y)
                } else {
                    self.diff_left_scroll_y
                };

                if (target_left_x - self.diff_left_scroll_x).abs() < 0.5
                    && (target_left_y - self.diff_left_scroll_y).abs() < 0.5
                {
                    return Task::none();
                }
                self.diff_left_scroll_x = target_left_x;
                self.diff_left_scroll_y = target_left_y;
                scrollable::scroll_to(
                    self.diff_left_scroll_id.clone(),
                    scrollable::AbsoluteOffset {
                        x: target_left_x,
                        y: target_left_y,
                    },
                )
            }
            Message::DiffJumpChange(delta) => {
                let layout = build_diff_layout(
                    &self.diff_parsed,
                    self.diff_compact,
                    &self.diff_expanded_gaps,
                );
                let count = layout.connectors.len();
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
                let span = &layout.connectors[next_idx];
                let margin = 48.0;
                let ly = (span.left_top - margin).max(0.0);
                let ry = (span.right_top - margin).max(0.0);
                let uy = (span.unified_top - margin).max(0.0);
                self.diff_left_scroll_y = ly;
                self.diff_right_scroll_y = ry;
                match self.bottom_mode {
                    BottomViewMode::SideBySideDiff => Task::batch([
                        scrollable::scroll_to(
                            self.diff_left_scroll_id.clone(),
                            scrollable::AbsoluteOffset {
                                x: self.diff_left_scroll_x,
                                y: ly,
                            },
                        ),
                        scrollable::scroll_to(
                            self.diff_right_scroll_id.clone(),
                            scrollable::AbsoluteOffset {
                                x: self.diff_right_scroll_x,
                                y: ry,
                            },
                        ),
                    ]),
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

                if click_count >= 2 {
                    let layout = build_diff_layout(
                        &self.diff_parsed,
                        self.diff_compact,
                        &self.diff_expanded_gaps,
                    );
                    if let Some(line) = diff_line_text_at(&self.diff_parsed, &layout, side, row) {
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
            Message::CopyDiffSelection => {
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
                    let layout = build_diff_layout(
                        &self.diff_parsed,
                        self.diff_compact,
                        &self.diff_expanded_gaps,
                    );
                    let copied = extract_selected_diff_text(&self.diff_parsed, &layout, sel);
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
            Message::SelectAllFocused => {
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
                    let layout = build_diff_layout(
                        &self.diff_parsed,
                        self.diff_compact,
                        &self.diff_expanded_gaps,
                    );
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
                    let count = diff_row_count(&self.diff_parsed, &layout, side);
                    if count > 0 {
                        let last_row = count - 1;
                        let last_col =
                            diff_line_text_at(&self.diff_parsed, &layout, side, last_row)
                                .map(|l| l.chars().count())
                                .unwrap_or(0);
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
            Message::DiffLoaded(title, res) => {
                self.diff_title = title;
                self.diff_expanded_gaps.clear();
                self.diff_selection = None;
                self.diff_left_scroll_y = 0.0;
                self.diff_right_scroll_y = 0.0;
                self.diff_left_scroll_x = 0.0;
                self.diff_right_scroll_x = 0.0;
                self.diff_active_change = 0;
                match res {
                    Ok(raw) => self.diff_parsed = parse_unified_diff(&raw),
                    Err(e) => {
                        self.diff_parsed = ParsedDiff {
                            raw: format!("Failed to load diff: {e}"),
                            ..Default::default()
                        };
                    }
                }
                Task::batch([
                    scrollable::scroll_to(
                        self.diff_left_scroll_id.clone(),
                        scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
                    ),
                    scrollable::scroll_to(
                        self.diff_right_scroll_id.clone(),
                        scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
                    ),
                    scrollable::scroll_to(
                        self.diff_unified_scroll_id.clone(),
                        scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
                    ),
                ])
            }
            Message::PreviewLinkClicked(url) => {
                self.toast_is_error = false;
                self.toast_message = format!("Link: {url}");
                Task::none()
            }
            Message::PreviewEditorAction(action) => {
                self.focus_pane = ActiveFocusPane::TextInput;
                if !action.is_edit() {
                    self.preview_editor.perform(action);
                }
                Task::none()
            }
            Message::TogglePreviewRaw => {
                self.preview_raw_mode = !self.preview_raw_mode;
                Task::none()
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
                self.editor_content.perform(action);
                if is_edit {
                    self.editor_dirty = true;
                }
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
                    match self.service.docs.save_document(&doc_id, &text_val, &ver) {
                        Ok(new_ver) => {
                            self.open_doc_version = Some(new_ver);
                            if let (Some(repo), Some(rel)) =
                                (self.current_repo(), self.active_file_path.as_deref())
                            {
                                self.preview_state = preview::PreviewState::from_file(
                                    &repo.path.join(rel),
                                    Some(&text_val),
                                );
                                self.sync_preview_editor();
                            }
                            self.editor_dirty = false;
                            self.toast_is_error = false;
                            self.toast_message = "File saved".into();
                            return self.update(Message::RefreshSelected);
                        }
                        Err(e) => {
                            self.toast_is_error = true;
                            self.toast_message = format!("Save failed: {e}");
                        }
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
                        self.preview_state =
                            preview::PreviewState::from_file(&full_path, Some(&cap.content));
                        self.sync_preview_editor();
                        self.editor_content = text_editor::Content::with_text(&cap.content);
                        self.open_doc_id = Some(cap.document_id);
                        self.open_doc_version = Some(cap.version);
                        self.editor_dirty = false;
                        self.bottom_mode = BottomViewMode::Edit;
                    }
                    Err(e) => {
                        self.toast_is_error = true;
                        self.toast_message = format!("Cannot open file: {e}");
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
                let content = self.editor_content.text();
                match self
                    .service
                    .docs
                    .save_document_as(&dest, &content, LineEnding::Lf)
                {
                    Ok(_) => {
                        self.toast_is_error = false;
                        self.toast_message = format!("Saved copy to {}", dest.display());
                    }
                    Err(e) => {
                        self.toast_is_error = true;
                        self.toast_message = format!("Save As failed: {e}");
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
}
