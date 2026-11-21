use crate::app::{
    ActiveFocusPane, BottomViewMode, DiffGapExpandMode, DiffSelectionSide, DiffTextPos,
    DiffTextSelection, FilesViewMode, GitFleetApp, GraphSelection, Message,
};
use crate::views::commit_files::apply_file_selection;
use crate::views::csv_diff::{compute_csv_diff, is_csv_diff_path};
use crate::views::diff_viewer::{
    build_diff_layout, diff_line_text_at, diff_row_count, extract_selected_diff_text,
    map_scroll_y, word_bounds_at,
};
use crate::views::image_diff::{
    ImageDiffData, ImageDiffZoom, decode_image_diff_pair, is_image_diff_path,
    is_raster_image_path, is_svg_path,
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
                self.diff_image = None;
                self.diff_csv = None;
                self.diff_selection = None;
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
                self.diff_image = None;
                self.diff_csv = None;
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
                        || self.conflict_parsed.conflict_count() > 0);
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
                    return Task::none();
                };

                match &self.graph_selection {
                    GraphSelection::WorkingTree => self.load_working_file_diff_tasks(
                        repo.path.clone(),
                        &file,
                        req_id,
                    ),
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
                        let mut tasks = Vec::new();

                        if !is_raster {
                            let svc = self.service.clone();
                            let repo_path = repo.path.clone();
                            let fpath = file.path.clone();
                            let o = oid.clone();
                            let t = title.clone();
                            tasks.push(Task::perform(
                                async move {
                                    let res = svc
                                        .git
                                        .get_commit_file_diff(&repo_path, &o, &fpath)
                                        .await
                                        .map_err(|e| e.to_string());
                                    (t, res)
                                },
                                |(t, r)| Message::DiffLoaded(t, r),
                            ));
                        } else {
                            self.diff_parsed = ParsedDiff::default();
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
                let title = rel_path.clone();
                let mut tasks = Vec::new();

                if !is_raster {
                    let svc = self.service.clone();
                    let repo_path = repo.path.clone();
                    let rp = rel_path.clone();
                    let t = title.clone();
                    tasks.push(Task::perform(
                        async move {
                            let res = svc
                                .git
                                .get_diff(&repo_path, &rp, false, false)
                                .await
                                .map_err(|e| e.to_string());
                            (t, res)
                        },
                        |(t, r)| Message::DiffLoaded(t, r),
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
                if matches!(mode, BottomViewMode::Edit | BottomViewMode::Preview)
                    && let Some(rel) = self.active_file_path.clone()
                {
                    self.reload_file_for_preview_and_edit_if_changed(&rel);
                }
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
                // Ignore stale in-flight working-tree diff responses after active file was cleared
                if self.graph_selection == GraphSelection::WorkingTree
                    && self.selected_changed_file.is_none()
                    && self.active_file_path.is_none()
                    && self.diff_title == "No file selected"
                {
                    return Task::none();
                }
                if let Ok(raw) = &res
                    && self.diff_title == title
                    && self.diff_parsed.raw == *raw
                {
                    return Task::none();
                }
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
                    if viewer.needs_blink_timer() {
                        viewer.image_state.blink_after = !viewer.image_state.blink_after;
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
                            self.report_action_error("Save File Failed", e.to_string(), false);
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
            tasks.push(Task::perform(
                async move {
                    let res = svc
                        .git
                        .get_diff(&rp, &fpath, staged, untracked)
                        .await
                        .map_err(|e| e.to_string());
                    (t, res)
                },
                |(t, r)| Message::DiffLoaded(t, r),
            ));
        } else {
            self.diff_parsed = ParsedDiff::default();
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
