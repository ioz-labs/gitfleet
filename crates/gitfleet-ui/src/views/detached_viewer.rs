use crate::app::{
    BottomViewMode, DiffGapExpandMode, DiffGapExpansion, DiffSelectionSide, DiffTextPos,
    DiffTextSelection, GitFleetApp, Message,
};
use crate::icons;
use crate::preview::PreviewState;
use crate::theme::{self, Palette, type_scale};
use crate::views::csv_diff::{CsvDiffData, CsvDiffMsg, compute_csv_diff, is_csv_diff_path, render_csv_diff};
use crate::views::diff_viewer::{
    TextDiffRenderCtx, build_diff_layout, diff_line_text_at, diff_row_count,
    extract_selected_diff_text, map_scroll_y, render_file_preview, render_side_by_side_diff_with,
    render_unified_diff_with, word_bounds_at,
};
use crate::views::image_diff::{
    ImageDiffData, ImageDiffMode, ImageDiffMsg, ImageDiffViewState, ImageDiffZoom,
    decode_image_diff_pair, is_image_diff_path, is_raster_image_path, is_svg_path,
    render_image_diff,
};
use crate::views::{tip, truncate_chars};
use gitfleet_git::{FileVersionPair, ParsedDiff, diff_two_texts};
use iced::widget::{
    Space, button, column, container, horizontal_rule, row, scrollable, text, text_editor, tooltip,
};
use iced::{Alignment, Color, Element, Length, Task, clipboard, window};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DetachedViewMode {
    UnifiedDiff,
    SideBySideDiff,
    ImageDiff,
    CsvDiff,
    Preview,
}

#[derive(Debug, Clone)]
pub(crate) struct DetachedComparePayload {
    pub(crate) title: String,
    pub(crate) left_path: Option<PathBuf>,
    pub(crate) right_path: Option<PathBuf>,
    pub(crate) single_file_path: Option<PathBuf>,
    pub(crate) parsed_diff: ParsedDiff,
    pub(crate) image_diff: Option<ImageDiffData>,
    pub(crate) csv_diff: Option<CsvDiffData>,
    pub(crate) preview_source: Option<(PathBuf, Option<String>)>,
    pub(crate) preferred_mode: DetachedViewMode,
    pub(crate) status_note: String,
}

#[derive(Debug, Clone)]
pub(crate) enum DetachedAction {
    SetMode(DetachedViewMode),
    ToggleCompact,
    ExpandGap {
        gap_id: usize,
        mode: DiffGapExpandMode,
    },
    LeftScrolled(scrollable::Viewport),
    RightScrolled(scrollable::Viewport),
    JumpChange(i32),
    SelStart {
        side: DiffSelectionSide,
        row: usize,
        col: usize,
    },
    SelDrag {
        side: DiffSelectionSide,
        row: usize,
        col: usize,
    },
    CopySelection,
    SelectAll,
    ImageMsg(ImageDiffMsg),
    CsvMsg(CsvDiffMsg),
    PreviewEditorAction(text_editor::Action),
    PreviewLinkClicked(String),
    TogglePreviewRaw,
    CopyPreviewText,
    PickSingleFile,
    PickLeftFile,
    PickRightFile,
    SwapCompareFiles,
    ReloadFiles,
    SingleFilePicked(Option<PathBuf>),
    LeftFilePicked(Option<PathBuf>),
    RightFilePicked(Option<PathBuf>),
    PayloadLoaded(u64, Box<DetachedComparePayload>),
    LoadFailed(u64, String),
}

pub(crate) struct DetachedViewerState {
    pub(crate) title: String,
    pub(crate) mode: DetachedViewMode,
    pub(crate) last_text_diff_mode: DetachedViewMode,
    pub(crate) request_id: u64,
    pub(crate) loading: bool,
    pub(crate) status_note: String,
    pub(crate) status_is_error: bool,

    // Local standalone file handles (optional; None when opened as a read-only Git diff snapshot)
    pub(crate) single_file_path: Option<PathBuf>,
    pub(crate) left_path: Option<PathBuf>,
    pub(crate) right_path: Option<PathBuf>,

    // Independent text diff state
    pub(crate) diff_parsed: ParsedDiff,
    pub(crate) diff_compact: bool,
    pub(crate) diff_expanded_gaps: HashMap<usize, DiffGapExpansion>,
    pub(crate) diff_left_scroll_id: scrollable::Id,
    pub(crate) diff_right_scroll_id: scrollable::Id,
    pub(crate) diff_unified_scroll_id: scrollable::Id,
    pub(crate) diff_left_scroll_y: f32,
    pub(crate) diff_right_scroll_y: f32,
    pub(crate) diff_left_scroll_x: f32,
    pub(crate) diff_right_scroll_x: f32,
    pub(crate) diff_active_change: usize,
    pub(crate) diff_selection: Option<DiffTextSelection>,
    pub(crate) diff_last_click: Option<(Instant, DiffSelectionSide, usize, usize, u8)>,

    // Independent Image & CSV diff state
    pub(crate) diff_image: Option<ImageDiffData>,
    pub(crate) image_state: ImageDiffViewState,
    pub(crate) diff_csv: Option<CsvDiffData>,
    pub(crate) csv_changes_only: bool,
    pub(crate) csv_ignore_format: bool,
    pub(crate) csv_filter: String,

    // Independent file preview state
    pub(crate) preview_state: PreviewState,
    pub(crate) preview_editor: text_editor::Content,
    pub(crate) preview_raw_mode: bool,
}

impl DetachedViewerState {
    pub(crate) fn empty() -> Self {
        Self {
            title: "Standalone Diff & Preview".into(),
            mode: DetachedViewMode::SideBySideDiff,
            last_text_diff_mode: DetachedViewMode::SideBySideDiff,
            request_id: 0,
            loading: false,
            status_note: "Independent popup — open any file to preview or pick Left & Right files to compare.".into(),
            status_is_error: false,
            single_file_path: None,
            left_path: None,
            right_path: None,
            diff_parsed: ParsedDiff::default(),
            diff_compact: true,
            diff_expanded_gaps: HashMap::new(),
            diff_left_scroll_id: scrollable::Id::unique(),
            diff_right_scroll_id: scrollable::Id::unique(),
            diff_unified_scroll_id: scrollable::Id::unique(),
            diff_left_scroll_y: 0.0,
            diff_right_scroll_y: 0.0,
            diff_left_scroll_x: 0.0,
            diff_right_scroll_x: 0.0,
            diff_active_change: 0,
            diff_selection: None,
            diff_last_click: None,
            diff_image: None,
            image_state: ImageDiffViewState {
                mode: ImageDiffMode::Swipe,
                zoom: ImageDiffZoom::Fit,
                swapped: false,
                swipe: 0.5,
                onion: 0.5,
                blink_after: false,
                blink_paused: false,
            },
            diff_csv: None,
            csv_changes_only: false,
            csv_ignore_format: false,
            csv_filter: String::new(),
            preview_state: PreviewState::Empty,
            preview_editor: text_editor::Content::new(),
            preview_raw_mode: false,
        }
    }

    /// Creates an independent detached viewer by taking a point-in-time snapshot of the host's
    /// active diff/preview (if any). Subsequent changes in the host window never mutate this state.
    pub(crate) fn from_host(app: &GitFleetApp) -> Self {
        let has_content = !app.diff_parsed.raw.is_empty()
            || app.diff_image.is_some()
            || app.diff_csv.is_some()
            || !matches!(app.preview_state, PreviewState::Empty);

        if !has_content {
            return Self::empty();
        }

        let last_text = match app.last_text_diff_mode {
            BottomViewMode::UnifiedDiff => DetachedViewMode::UnifiedDiff,
            _ => DetachedViewMode::SideBySideDiff,
        };

        let mode = match app.bottom_mode {
            BottomViewMode::UnifiedDiff => DetachedViewMode::UnifiedDiff,
            BottomViewMode::SideBySideDiff => DetachedViewMode::SideBySideDiff,
            BottomViewMode::ImageDiff if app.diff_image.is_some() => DetachedViewMode::ImageDiff,
            BottomViewMode::CsvDiff if app.diff_csv.is_some() => DetachedViewMode::CsvDiff,
            BottomViewMode::Preview | BottomViewMode::Edit => DetachedViewMode::Preview,
            BottomViewMode::ResolveConflicts => {
                if !app.diff_parsed.raw.is_empty() {
                    last_text
                } else {
                    DetachedViewMode::Preview
                }
            }
            _ => {
                if app.diff_image.is_some() {
                    DetachedViewMode::ImageDiff
                } else if app.diff_csv.is_some() {
                    DetachedViewMode::CsvDiff
                } else if !app.diff_parsed.raw.is_empty() {
                    last_text
                } else {
                    DetachedViewMode::Preview
                }
            }
        };

        let preview_state = app.preview_state.clone_snapshot();
        let preview_editor = match preview_state.copyable_text() {
            Some(raw) => text_editor::Content::with_text(raw),
            None => text_editor::Content::new(),
        };

        let single_file_path = app.current_repo().and_then(|repo| {
            app.active_file_path
                .as_deref()
                .map(|rel| repo.path.join(rel))
                .filter(|p| p.is_file())
        });

        let title = if app.diff_title.is_empty() || app.diff_title == "No file selected" {
            app.active_file_path
                .clone()
                .unwrap_or_else(|| "Standalone Diff & Preview".into())
        } else {
            app.diff_title.clone()
        };

        Self {
            title,
            mode,
            last_text_diff_mode: last_text,
            request_id: 0,
            loading: false,
            status_note: "Snapshot detached — changes in the main window will not affect this popup.".into(),
            status_is_error: false,
            single_file_path,
            left_path: None,
            right_path: None,
            diff_parsed: app.diff_parsed.clone(),
            diff_compact: app.diff_compact,
            diff_expanded_gaps: app.diff_expanded_gaps.clone(),
            diff_left_scroll_id: scrollable::Id::unique(),
            diff_right_scroll_id: scrollable::Id::unique(),
            diff_unified_scroll_id: scrollable::Id::unique(),
            diff_left_scroll_y: 0.0,
            diff_right_scroll_y: 0.0,
            diff_left_scroll_x: 0.0,
            diff_right_scroll_x: 0.0,
            diff_active_change: 0,
            diff_selection: None,
            diff_last_click: None,
            diff_image: app.diff_image.clone(),
            image_state: ImageDiffViewState {
                mode: app.image_diff_mode,
                zoom: app.image_diff_zoom,
                swapped: app.image_diff_swapped,
                swipe: app.image_diff_swipe,
                onion: app.image_diff_onion,
                blink_after: false,
                blink_paused: app.image_diff_blink_paused,
            },
            diff_csv: app.diff_csv.clone(),
            csv_changes_only: app.csv_diff_changes_only,
            csv_ignore_format: app.csv_diff_ignore_format,
            csv_filter: app.csv_diff_filter.clone(),
            preview_state,
            preview_editor,
            preview_raw_mode: app.preview_raw_mode,
        }
    }

    pub(crate) fn needs_blink_timer(&self) -> bool {
        self.mode == DetachedViewMode::ImageDiff
            && self.image_state.mode == ImageDiffMode::Blink
            && !self.image_state.blink_paused
            && self.diff_image.is_some()
    }

    fn has_any_content(&self) -> bool {
        !self.diff_parsed.raw.is_empty()
            || self.diff_image.is_some()
            || self.diff_csv.is_some()
            || !matches!(self.preview_state, PreviewState::Empty)
    }

    fn sync_preview_editor(&mut self) {
        self.preview_editor = match self.preview_state.copyable_text() {
            Some(raw) => text_editor::Content::with_text(raw),
            None => text_editor::Content::new(),
        };
    }

    fn reset_scroll_state(&mut self) {
        self.diff_expanded_gaps.clear();
        self.diff_selection = None;
        self.diff_left_scroll_y = 0.0;
        self.diff_right_scroll_y = 0.0;
        self.diff_left_scroll_x = 0.0;
        self.diff_right_scroll_x = 0.0;
        self.diff_active_change = 0;
    }

    fn reset_scroll_task(&self) -> Task<Message> {
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
}

fn file_display_name(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn load_single_file_payload(path: PathBuf) -> Result<DetachedComparePayload, String> {
    let name = file_display_name(&path);
    let is_raster = is_raster_image_path(&name);
    let preview_text = if is_raster {
        None
    } else {
        let bytes = std::fs::read(&path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
        if bytes.iter().take(4096).any(|&b| b == 0) {
            None
        } else {
            Some(String::from_utf8_lossy(&bytes).into_owned())
        }
    };

    Ok(DetachedComparePayload {
        title: name.clone(),
        left_path: None,
        right_path: None,
        single_file_path: Some(path.clone()),
        parsed_diff: ParsedDiff::default(),
        image_diff: None,
        csv_diff: None,
        preview_source: Some((path.clone(), preview_text)),
        preferred_mode: DetachedViewMode::Preview,
        status_note: format!("Previewing {}", path.display()),
    })
}

pub(crate) fn compare_local_files_payload(
    left: PathBuf,
    right: PathBuf,
    preferred_text_mode: DetachedViewMode,
) -> Result<DetachedComparePayload, String> {
    let left_bytes =
        std::fs::read(&left).map_err(|e| format!("Cannot read left file {}: {e}", left.display()))?;
    let right_bytes = std::fs::read(&right)
        .map_err(|e| format!("Cannot read right file {}: {e}", right.display()))?;

    let left_name = file_display_name(&left);
    let right_name = file_display_name(&right);
    let title = format!("{left_name}  ↔  {right_name}");

    let is_img = is_image_diff_path(&left_name) || is_image_diff_path(&right_name);
    let is_raster = is_raster_image_path(&left_name) || is_raster_image_path(&right_name);
    let is_svg = is_svg_path(&left_name) && is_svg_path(&right_name);
    let is_csv = is_csv_diff_path(&left_name) || is_csv_diff_path(&right_name);

    let mut image_diff = None;
    let mut csv_diff = None;
    let mut parsed_diff = ParsedDiff::default();

    let representative_path = if is_image_diff_path(&right_name) || is_csv_diff_path(&right_name) {
        right_name.clone()
    } else {
        left_name.clone()
    };

    if is_img {
        image_diff = Some(decode_image_diff_pair(FileVersionPair {
            file_path: representative_path.clone(),
            before_label: left_name.clone(),
            after_label: right_name.clone(),
            before_bytes: Some(left_bytes.clone()),
            after_bytes: Some(right_bytes.clone()),
        }));
    }

    if is_csv {
        csv_diff = Some(compute_csv_diff(FileVersionPair {
            file_path: representative_path,
            before_label: left_name.clone(),
            after_label: right_name.clone(),
            before_bytes: Some(left_bytes.clone()),
            after_bytes: Some(right_bytes.clone()),
        }));
    }

    let left_is_binary = left_bytes.iter().take(4096).any(|&b| b == 0);
    let right_is_binary = right_bytes.iter().take(4096).any(|&b| b == 0);
    let right_text = if !is_raster && !right_is_binary {
        Some(String::from_utf8_lossy(&right_bytes).into_owned())
    } else {
        None
    };

    if (!is_raster || is_svg) && !left_is_binary && !right_is_binary {
        let left_str = String::from_utf8_lossy(&left_bytes);
        let right_str = right_text.as_deref().unwrap_or("");
        parsed_diff = diff_two_texts(&left_name, &left_str, &right_name, right_str);
    } else if !is_img {
        parsed_diff = ParsedDiff {
            raw: format!(
                "Binary files differ or cannot be displayed as UTF-8 text:\n  Left:  {} ({} bytes)\n  Right: {} ({} bytes)",
                left.display(),
                left_bytes.len(),
                right.display(),
                right_bytes.len(),
            ),
            ..Default::default()
        };
    }

    let preferred_mode = if is_img {
        DetachedViewMode::ImageDiff
    } else if is_csv {
        DetachedViewMode::CsvDiff
    } else {
        preferred_text_mode
    };

    Ok(DetachedComparePayload {
        title,
        left_path: Some(left.clone()),
        right_path: Some(right.clone()),
        single_file_path: None,
        parsed_diff,
        image_diff,
        csv_diff,
        preview_source: Some((right.clone(), right_text)),
        preferred_mode,
        status_note: format!("Comparing {} ↔ {}", left.display(), right.display()),
    })
}

fn spawn_compare_task(
    win_id: window::Id,
    req_id: u64,
    left: PathBuf,
    right: PathBuf,
    preferred_text_mode: DetachedViewMode,
) -> Task<Message> {
    Task::perform(
        async move {
            let res = tokio::task::spawn_blocking(move || {
                compare_local_files_payload(left, right, preferred_text_mode)
            })
            .await
            .unwrap_or_else(|e| Err(format!("Comparison task failed: {e}")));
            (req_id, res)
        },
        move |(id, res)| match res {
            Ok(payload) => {
                Message::Detached(win_id, DetachedAction::PayloadLoaded(id, Box::new(payload)))
            }
            Err(err) => Message::Detached(win_id, DetachedAction::LoadFailed(id, err)),
        },
    )
}

fn spawn_single_file_task(win_id: window::Id, req_id: u64, path: PathBuf) -> Task<Message> {
    Task::perform(
        async move {
            let res = tokio::task::spawn_blocking(move || load_single_file_payload(path))
                .await
                .unwrap_or_else(|e| Err(format!("File load task failed: {e}")));
            (req_id, res)
        },
        move |(id, res)| match res {
            Ok(payload) => {
                Message::Detached(win_id, DetachedAction::PayloadLoaded(id, Box::new(payload)))
            }
            Err(err) => Message::Detached(win_id, DetachedAction::LoadFailed(id, err)),
        },
    )
}

fn map_text_diff_msg(win_id: window::Id, msg: Message) -> Message {
    match msg {
        Message::ExpandDiffGap { gap_id, mode } => {
            Message::Detached(win_id, DetachedAction::ExpandGap { gap_id, mode })
        }
        Message::DiffLeftScrolled(vp) => {
            Message::Detached(win_id, DetachedAction::LeftScrolled(vp))
        }
        Message::DiffRightScrolled(vp) => {
            Message::Detached(win_id, DetachedAction::RightScrolled(vp))
        }
        Message::DiffSelStart { side, row, col } => {
            Message::Detached(win_id, DetachedAction::SelStart { side, row, col })
        }
        Message::DiffSelDrag { side, row, col } => {
            Message::Detached(win_id, DetachedAction::SelDrag { side, row, col })
        }
        Message::PreviewEditorAction(action) => {
            Message::Detached(win_id, DetachedAction::PreviewEditorAction(action))
        }
        Message::PreviewLinkClicked(url) => {
            Message::Detached(win_id, DetachedAction::PreviewLinkClicked(url))
        }
        _ => Message::Noop,
    }
}

impl GitFleetApp {
    pub(crate) fn update_detached_viewer(
        &mut self,
        win_id: window::Id,
        action: DetachedAction,
    ) -> Task<Message> {
        let Some(v) = self.detached_viewers.get_mut(&win_id) else {
            return Task::none();
        };

        match action {
            DetachedAction::SetMode(mode) => {
                v.mode = mode;
                if matches!(
                    mode,
                    DetachedViewMode::UnifiedDiff | DetachedViewMode::SideBySideDiff
                ) {
                    v.last_text_diff_mode = mode;
                }
                v.diff_selection = None;
                Task::none()
            }
            DetachedAction::ToggleCompact => {
                v.diff_compact = !v.diff_compact;
                v.diff_selection = None;
                v.diff_left_scroll_y = 0.0;
                v.diff_right_scroll_y = 0.0;
                Task::batch([
                    scrollable::scroll_to(
                        v.diff_left_scroll_id.clone(),
                        scrollable::AbsoluteOffset {
                            x: v.diff_left_scroll_x,
                            y: 0.0,
                        },
                    ),
                    scrollable::scroll_to(
                        v.diff_right_scroll_id.clone(),
                        scrollable::AbsoluteOffset {
                            x: v.diff_right_scroll_x,
                            y: 0.0,
                        },
                    ),
                ])
            }
            DetachedAction::ExpandGap { gap_id, mode } => {
                v.diff_selection = None;
                let entry = v.diff_expanded_gaps.entry(gap_id).or_default();
                match mode {
                    DiffGapExpandMode::Top(n) => entry.top_shown += n,
                    DiffGapExpandMode::Bottom(n) => entry.bottom_shown += n,
                    DiffGapExpandMode::All => entry.show_all = true,
                }
                Task::none()
            }
            DetachedAction::LeftScrolled(vp) => {
                let off = vp.absolute_offset();
                let pane_h = vp.bounds().height.max(1.0);
                let x_changed = (off.x - v.diff_left_scroll_x).abs() >= 0.5;
                let y_changed = (off.y - v.diff_left_scroll_y).abs() >= 0.5;
                if !x_changed && !y_changed {
                    return Task::none();
                }
                v.diff_left_scroll_x = off.x;
                v.diff_left_scroll_y = off.y;

                let target_right_x = off.x.max(0.0);
                let target_right_y = if y_changed {
                    let layout = build_diff_layout(
                        &v.diff_parsed,
                        v.diff_compact,
                        &v.diff_expanded_gaps,
                    );
                    let raw_right_y = map_scroll_y(off.y, &layout.anchors, true);
                    let max_right_y = ((layout.total_right_h + 10.0) - pane_h).max(0.0);
                    raw_right_y.clamp(0.0, max_right_y)
                } else {
                    v.diff_right_scroll_y
                };

                if (target_right_x - v.diff_right_scroll_x).abs() < 0.5
                    && (target_right_y - v.diff_right_scroll_y).abs() < 0.5
                {
                    return Task::none();
                }
                v.diff_right_scroll_x = target_right_x;
                v.diff_right_scroll_y = target_right_y;
                scrollable::scroll_to(
                    v.diff_right_scroll_id.clone(),
                    scrollable::AbsoluteOffset {
                        x: target_right_x,
                        y: target_right_y,
                    },
                )
            }
            DetachedAction::RightScrolled(vp) => {
                let off = vp.absolute_offset();
                let pane_h = vp.bounds().height.max(1.0);
                let x_changed = (off.x - v.diff_right_scroll_x).abs() >= 0.5;
                let y_changed = (off.y - v.diff_right_scroll_y).abs() >= 0.5;
                if !x_changed && !y_changed {
                    return Task::none();
                }
                v.diff_right_scroll_x = off.x;
                v.diff_right_scroll_y = off.y;

                let target_left_x = off.x.max(0.0);
                let target_left_y = if y_changed {
                    let layout = build_diff_layout(
                        &v.diff_parsed,
                        v.diff_compact,
                        &v.diff_expanded_gaps,
                    );
                    let raw_left_y = map_scroll_y(off.y, &layout.anchors, false);
                    let max_left_y = ((layout.total_left_h + 10.0) - pane_h).max(0.0);
                    raw_left_y.clamp(0.0, max_left_y)
                } else {
                    v.diff_left_scroll_y
                };

                if (target_left_x - v.diff_left_scroll_x).abs() < 0.5
                    && (target_left_y - v.diff_left_scroll_y).abs() < 0.5
                {
                    return Task::none();
                }
                v.diff_left_scroll_x = target_left_x;
                v.diff_left_scroll_y = target_left_y;
                scrollable::scroll_to(
                    v.diff_left_scroll_id.clone(),
                    scrollable::AbsoluteOffset {
                        x: target_left_x,
                        y: target_left_y,
                    },
                )
            }
            DetachedAction::JumpChange(delta) => {
                let layout = build_diff_layout(
                    &v.diff_parsed,
                    v.diff_compact,
                    &v.diff_expanded_gaps,
                );
                let count = layout.connectors.len();
                if count == 0 {
                    return Task::none();
                }
                let next_idx = if delta < 0 {
                    if v.diff_active_change == 0 {
                        count - 1
                    } else {
                        v.diff_active_change - 1
                    }
                } else {
                    (v.diff_active_change + 1) % count
                };
                v.diff_active_change = next_idx;
                let span = &layout.connectors[next_idx];
                let margin = 48.0;
                let ly = (span.left_top - margin).max(0.0);
                let ry = (span.right_top - margin).max(0.0);
                let uy = (span.unified_top - margin).max(0.0);
                v.diff_left_scroll_y = ly;
                v.diff_right_scroll_y = ry;
                match v.mode {
                    DetachedViewMode::SideBySideDiff => Task::batch([
                        scrollable::scroll_to(
                            v.diff_left_scroll_id.clone(),
                            scrollable::AbsoluteOffset {
                                x: v.diff_left_scroll_x,
                                y: ly,
                            },
                        ),
                        scrollable::scroll_to(
                            v.diff_right_scroll_id.clone(),
                            scrollable::AbsoluteOffset {
                                x: v.diff_right_scroll_x,
                                y: ry,
                            },
                        ),
                    ]),
                    DetachedViewMode::UnifiedDiff => scrollable::scroll_to(
                        v.diff_unified_scroll_id.clone(),
                        scrollable::AbsoluteOffset { x: 0.0, y: uy },
                    ),
                    _ => Task::none(),
                }
            }
            DetachedAction::SelStart { side, row, col } => {
                let now = Instant::now();
                let click_count = match v.diff_last_click {
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
                v.diff_last_click = Some((now, side, row, col, click_count));

                if click_count >= 2 {
                    let layout = build_diff_layout(
                        &v.diff_parsed,
                        v.diff_compact,
                        &v.diff_expanded_gaps,
                    );
                    if let Some(line) = diff_line_text_at(&v.diff_parsed, &layout, side, row) {
                        let (s, e) = if click_count == 2 {
                            word_bounds_at(&line, col)
                        } else {
                            (0, line.chars().count())
                        };
                        v.diff_selection = Some(DiffTextSelection {
                            side,
                            anchor: DiffTextPos { row, col: s },
                            focus: DiffTextPos { row, col: e },
                            dragging: false,
                        });
                        return Task::none();
                    }
                }

                let pos = DiffTextPos { row, col };
                v.diff_selection = Some(DiffTextSelection {
                    side,
                    anchor: pos,
                    focus: pos,
                    dragging: true,
                });
                Task::none()
            }
            DetachedAction::SelDrag { side, row, col } => {
                if let Some(sel) = &mut v.diff_selection
                    && sel.dragging
                    && sel.side == side
                {
                    sel.focus = DiffTextPos { row, col };
                }
                Task::none()
            }
            DetachedAction::CopySelection => {
                if matches!(
                    v.mode,
                    DetachedViewMode::UnifiedDiff | DetachedViewMode::SideBySideDiff
                ) && let Some(sel) = v.diff_selection
                    && !sel.is_empty()
                {
                    let layout = build_diff_layout(
                        &v.diff_parsed,
                        v.diff_compact,
                        &v.diff_expanded_gaps,
                    );
                    let copied = extract_selected_diff_text(&v.diff_parsed, &layout, sel);
                    if !copied.is_empty() {
                        let lines = copied.lines().count();
                        v.status_is_error = false;
                        v.status_note = if lines > 1 {
                            format!("Copied {lines} lines to clipboard")
                        } else {
                            "Copied selected text to clipboard".into()
                        };
                        return clipboard::write(copied);
                    }
                }
                Task::none()
            }
            DetachedAction::SelectAll => {
                if v.diff_parsed.raw.is_empty()
                    || !matches!(
                        v.mode,
                        DetachedViewMode::UnifiedDiff | DetachedViewMode::SideBySideDiff
                    )
                {
                    return Task::none();
                }
                let layout = build_diff_layout(
                    &v.diff_parsed,
                    v.diff_compact,
                    &v.diff_expanded_gaps,
                );
                let side = if v.diff_parsed.blocks.is_empty() {
                    DiffSelectionSide::Raw
                } else if v.mode == DetachedViewMode::UnifiedDiff {
                    DiffSelectionSide::Unified
                } else {
                    v.diff_selection
                        .map(|s| s.side)
                        .filter(|s| matches!(s, DiffSelectionSide::Left | DiffSelectionSide::Right))
                        .unwrap_or(DiffSelectionSide::Right)
                };
                let count = diff_row_count(&v.diff_parsed, &layout, side);
                if count > 0 {
                    let last_row = count - 1;
                    let last_col = diff_line_text_at(&v.diff_parsed, &layout, side, last_row)
                        .map(|l| l.chars().count())
                        .unwrap_or(0);
                    v.diff_selection = Some(DiffTextSelection {
                        side,
                        anchor: DiffTextPos { row: 0, col: 0 },
                        focus: DiffTextPos {
                            row: last_row,
                            col: last_col,
                        },
                        dragging: false,
                    });
                }
                Task::none()
            }
            DetachedAction::ImageMsg(msg) => {
                match msg {
                    ImageDiffMsg::SetMode(m) => {
                        v.image_state.mode = m;
                        v.image_state.blink_after = false;
                    }
                    ImageDiffMsg::SetZoom(z) => v.image_state.zoom = z,
                    ImageDiffMsg::ToggleSwap => v.image_state.swapped = !v.image_state.swapped,
                    ImageDiffMsg::SwipeChanged(pos) => v.image_state.swipe = pos.clamp(0.0, 1.0),
                    ImageDiffMsg::OnionChanged(alpha) => v.image_state.onion = alpha.clamp(0.0, 1.0),
                    ImageDiffMsg::BlinkFlip => {
                        v.image_state.blink_after = !v.image_state.blink_after;
                    }
                    ImageDiffMsg::ToggleBlinkPause => {
                        v.image_state.blink_paused = !v.image_state.blink_paused;
                    }
                }
                Task::none()
            }
            DetachedAction::CsvMsg(msg) => {
                match msg {
                    CsvDiffMsg::SetChangesOnly(only) => v.csv_changes_only = only,
                    CsvDiffMsg::ToggleIgnoreFormat => v.csv_ignore_format = !v.csv_ignore_format,
                    CsvDiffMsg::FilterChanged(q) => v.csv_filter = q,
                }
                Task::none()
            }
            DetachedAction::PreviewEditorAction(action) => {
                if !action.is_edit() {
                    v.preview_editor.perform(action);
                }
                Task::none()
            }
            DetachedAction::PreviewLinkClicked(url) => {
                v.status_is_error = false;
                v.status_note = format!("Link: {url}");
                Task::none()
            }
            DetachedAction::TogglePreviewRaw => {
                v.preview_raw_mode = !v.preview_raw_mode;
                Task::none()
            }
            DetachedAction::CopyPreviewText => {
                if let Some(sel) = v.preview_editor.selection().filter(|s| !s.is_empty()) {
                    v.status_is_error = false;
                    v.status_note = "Copied selected preview text to clipboard".into();
                    return clipboard::write(sel);
                }
                if let Some(full) = v.preview_state.copyable_text() {
                    v.status_is_error = false;
                    v.status_note = "Copied preview to clipboard".into();
                    return clipboard::write(full.to_string());
                }
                Task::none()
            }
            DetachedAction::PickSingleFile => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_title("Open File in Standalone Preview")
                        .pick_file()
                        .await
                        .map(|h| h.path().to_path_buf())
                },
                move |p| Message::Detached(win_id, DetachedAction::SingleFilePicked(p)),
            ),
            DetachedAction::PickLeftFile => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_title("Select Left (Before) File to Compare")
                        .pick_file()
                        .await
                        .map(|h| h.path().to_path_buf())
                },
                move |p| Message::Detached(win_id, DetachedAction::LeftFilePicked(p)),
            ),
            DetachedAction::PickRightFile => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_title("Select Right (After) File to Compare")
                        .pick_file()
                        .await
                        .map(|h| h.path().to_path_buf())
                },
                move |p| Message::Detached(win_id, DetachedAction::RightFilePicked(p)),
            ),
            DetachedAction::SingleFilePicked(Some(path)) => {
                v.request_id = v.request_id.wrapping_add(1);
                let req_id = v.request_id;
                v.loading = true;
                v.status_is_error = false;
                v.status_note = format!("Opening {}…", file_display_name(&path));
                spawn_single_file_task(win_id, req_id, path)
            }
            DetachedAction::SingleFilePicked(None) => Task::none(),
            DetachedAction::LeftFilePicked(Some(left)) => {
                v.left_path = Some(left.clone());
                v.single_file_path = None;
                if let Some(right) = v.right_path.clone() {
                    v.request_id = v.request_id.wrapping_add(1);
                    let req_id = v.request_id;
                    let pref = v.last_text_diff_mode;
                    v.loading = true;
                    v.status_is_error = false;
                    v.status_note = format!(
                        "Comparing {} ↔ {}…",
                        file_display_name(&left),
                        file_display_name(&right)
                    );
                    spawn_compare_task(win_id, req_id, left, right, pref)
                } else {
                    v.request_id = v.request_id.wrapping_add(1);
                    let req_id = v.request_id;
                    v.loading = true;
                    v.status_is_error = false;
                    let note = format!(
                        "Left file set ({}) — click 'Right…' to compare",
                        file_display_name(&left)
                    );
                    let left_clone = left.clone();
                    Task::perform(
                        async move {
                            let res = tokio::task::spawn_blocking(move || {
                                let mut payload = load_single_file_payload(left_clone.clone())?;
                                payload.left_path = Some(left_clone);
                                payload.single_file_path = None;
                                payload.status_note = note;
                                Ok::<_, String>(payload)
                            })
                            .await
                            .unwrap_or_else(|e| Err(format!("Load failed: {e}")));
                            (req_id, res)
                        },
                        move |(id, res)| match res {
                            Ok(p) => Message::Detached(
                                win_id,
                                DetachedAction::PayloadLoaded(id, Box::new(p)),
                            ),
                            Err(e) => Message::Detached(win_id, DetachedAction::LoadFailed(id, e)),
                        },
                    )
                }
            }
            DetachedAction::LeftFilePicked(None) => Task::none(),
            DetachedAction::RightFilePicked(Some(right)) => {
                v.right_path = Some(right.clone());
                v.single_file_path = None;
                if let Some(left) = v.left_path.clone() {
                    v.request_id = v.request_id.wrapping_add(1);
                    let req_id = v.request_id;
                    let pref = v.last_text_diff_mode;
                    v.loading = true;
                    v.status_is_error = false;
                    v.status_note = format!(
                        "Comparing {} ↔ {}…",
                        file_display_name(&left),
                        file_display_name(&right)
                    );
                    spawn_compare_task(win_id, req_id, left, right, pref)
                } else {
                    v.request_id = v.request_id.wrapping_add(1);
                    let req_id = v.request_id;
                    v.loading = true;
                    v.status_is_error = false;
                    let note = format!(
                        "Right file set ({}) — click 'Left…' to compare",
                        file_display_name(&right)
                    );
                    let right_clone = right.clone();
                    Task::perform(
                        async move {
                            let res = tokio::task::spawn_blocking(move || {
                                let mut payload = load_single_file_payload(right_clone.clone())?;
                                payload.right_path = Some(right_clone);
                                payload.single_file_path = None;
                                payload.status_note = note;
                                Ok::<_, String>(payload)
                            })
                            .await
                            .unwrap_or_else(|e| Err(format!("Load failed: {e}")));
                            (req_id, res)
                        },
                        move |(id, res)| match res {
                            Ok(p) => Message::Detached(
                                win_id,
                                DetachedAction::PayloadLoaded(id, Box::new(p)),
                            ),
                            Err(e) => Message::Detached(win_id, DetachedAction::LoadFailed(id, e)),
                        },
                    )
                }
            }
            DetachedAction::RightFilePicked(None) => Task::none(),
            DetachedAction::SwapCompareFiles => {
                if let (Some(left), Some(right)) = (v.left_path.clone(), v.right_path.clone()) {
                    v.left_path = Some(right.clone());
                    v.right_path = Some(left.clone());
                    v.request_id = v.request_id.wrapping_add(1);
                    let req_id = v.request_id;
                    let pref = v.last_text_diff_mode;
                    v.loading = true;
                    spawn_compare_task(win_id, req_id, right, left, pref)
                } else if v.diff_image.is_some() {
                    v.image_state.swapped = !v.image_state.swapped;
                    Task::none()
                } else {
                    Task::none()
                }
            }
            DetachedAction::ReloadFiles => {
                if let (Some(left), Some(right)) = (v.left_path.clone(), v.right_path.clone()) {
                    v.request_id = v.request_id.wrapping_add(1);
                    let req_id = v.request_id;
                    let pref = v.last_text_diff_mode;
                    v.loading = true;
                    v.status_is_error = false;
                    v.status_note = "Reloading compared files…".into();
                    spawn_compare_task(win_id, req_id, left, right, pref)
                } else if let Some(path) = v.single_file_path.clone() {
                    v.request_id = v.request_id.wrapping_add(1);
                    let req_id = v.request_id;
                    v.loading = true;
                    v.status_is_error = false;
                    v.status_note = format!("Reloading {}…", file_display_name(&path));
                    spawn_single_file_task(win_id, req_id, path)
                } else {
                    Task::none()
                }
            }
            DetachedAction::PayloadLoaded(req_id, payload) => {
                if req_id != v.request_id {
                    return Task::none();
                }
                v.loading = false;
                v.title = payload.title;
                v.left_path = payload.left_path;
                v.right_path = payload.right_path;
                v.single_file_path = payload.single_file_path;
                v.diff_parsed = payload.parsed_diff;
                if let Some(img) = &payload.image_diff {
                    v.image_state.swapped = false;
                    v.image_state.zoom = ImageDiffZoom::Fit;
                    v.image_state.blink_after = false;
                    v.image_state.swipe = match (&img.before, &img.after) {
                        (None, Some(_)) => 0.0,
                        (Some(_), None) => 1.0,
                        _ => 0.5,
                    };
                }
                v.diff_image = payload.image_diff;
                v.diff_csv = payload.csv_diff;
                v.csv_filter.clear();
                v.preview_state = match payload.preview_source {
                    Some((path, text_opt)) => PreviewState::from_file(&path, text_opt.as_deref()),
                    None => PreviewState::Empty,
                };
                v.sync_preview_editor();
                v.mode = payload.preferred_mode;
                v.status_is_error = false;
                v.status_note = payload.status_note;
                v.reset_scroll_state();
                v.reset_scroll_task()
            }
            DetachedAction::LoadFailed(req_id, err) => {
                if req_id == v.request_id {
                    v.loading = false;
                    v.status_is_error = true;
                    v.status_note = err;
                }
                Task::none()
            }
        }
    }

    pub(crate) fn view_detached_viewer(&self, win_id: window::Id, p: Palette) -> Element<'_, Message> {
        let Some(v) = self.detached_viewers.get(&win_id) else {
            return container(
                theme::empty_state(
                    p,
                    icons::external_window(theme::icon_size::LG, p.text_muted),
                    "Standalone Diff & Preview",
                    "Initializing detached viewer window…",
                    None,
                ),
            )
            .padding(10)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::app_bg(p))
            .into();
        };

        let is_text_diff = matches!(
            v.mode,
            DetachedViewMode::UnifiedDiff | DetachedViewMode::SideBySideDiff
        );
        let stats = v.diff_parsed.stats;
        let has_changes = stats.added_blocks + stats.removed_blocks + stats.modified_blocks > 0;
        let is_raster_only = v.diff_image.as_ref().is_some_and(|d| !d.is_svg);
        let can_reload = (v.left_path.is_some() && v.right_path.is_some())
            || v.single_file_path.is_some();
        let can_swap =
            (v.left_path.is_some() && v.right_path.is_some()) || v.diff_image.is_some();

        // ── Row 1: Standalone File / Compare Bar ─────────────────────────────────
        let left_label = v
            .left_path
            .as_deref()
            .map(|p| format!("Left: {}", truncate_chars(&file_display_name(p), 20)))
            .unwrap_or_else(|| "Left…".into());
        let right_label = v
            .right_path
            .as_deref()
            .map(|p| format!("Right: {}", truncate_chars(&file_display_name(p), 20)))
            .unwrap_or_else(|| "Right…".into());

        let mut top_bar = row![
            container(
                row![
                    icons::external_window(12.0, p.accent_fg),
                    text("Standalone")
                        .size(type_scale::BADGE)
                        .color(p.accent_fg)
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
            )
            .padding([2, 7])
            .style(theme::pill_box(p.fill_selected, p.accent_fg, p.border_default)),
            theme::clipped_line(
                text(&v.title)
                    .size(type_scale::BODY)
                    .color(p.text_primary)
                    .wrapping(text::Wrapping::None),
                Length::Fill,
            ),
            tip(
                button(
                    row![
                        icons::file_code(12.0, p.text_secondary),
                        text("Open File…")
                            .size(type_scale::CAPTION)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([4, 9])
                .style(theme::subtle_btn(p))
                .on_press(Message::Detached(win_id, DetachedAction::PickSingleFile)),
                "Open any local file for standalone Preview (Markdown, Image, SVG, JSON, CSV, Code)",
                tooltip::Position::Bottom,
                p,
            ),
            tip(
                button(
                    row![
                        icons::file_diff(12.0, p.text_secondary),
                        text(left_label)
                            .size(type_scale::CAPTION)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([4, 9])
                .style(theme::filter_pill_btn(p, v.left_path.is_some()))
                .on_press(Message::Detached(win_id, DetachedAction::PickLeftFile)),
                "Choose Left (Before) file from disk to compare",
                tooltip::Position::Bottom,
                p,
            ),
            tip(
                button(
                    row![
                        icons::file_diff(12.0, p.text_secondary),
                        text(right_label)
                            .size(type_scale::CAPTION)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([4, 9])
                .style(theme::filter_pill_btn(p, v.right_path.is_some()))
                .on_press(Message::Detached(win_id, DetachedAction::PickRightFile)),
                "Choose Right (After) file from disk to compare",
                tooltip::Position::Bottom,
                p,
            ),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        if can_swap {
            top_bar = top_bar.push(tip(
                button(icons::swap_arrows(12.0, p.text_secondary))
                    .padding([5, 7])
                    .style(theme::subtle_btn(p))
                    .on_press(Message::Detached(win_id, DetachedAction::SwapCompareFiles)),
                "Swap Left and Right",
                tooltip::Position::Bottom,
                p,
            ));
        }

        if can_reload {
            top_bar = top_bar.push(tip(
                button(icons::refresh(12.0, p.text_secondary))
                    .padding([5, 7])
                    .style(theme::subtle_btn(p))
                    .on_press(Message::Detached(win_id, DetachedAction::ReloadFiles)),
                "Reload file(s) from disk",
                tooltip::Position::Bottom,
                p,
            ));
        }

        // ── Row 2: Mode Pills & Mode-Specific Controls ───────────────────────────
        let mut mode_bar = row![].spacing(5).align_y(Alignment::Center);

        if is_text_diff && has_changes {
            let mut stat_row = row![].spacing(4).align_y(Alignment::Center);
            if stats.added_blocks > 0 {
                stat_row = stat_row.push(
                    text(format!("+{}", stats.added_blocks))
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(p.success_fg)
                        .wrapping(text::Wrapping::None),
                );
            }
            if stats.removed_blocks > 0 {
                stat_row = stat_row.push(
                    text(format!("-{}", stats.removed_blocks))
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(p.danger_fg)
                        .wrapping(text::Wrapping::None),
                );
            }
            if stats.modified_blocks > 0 {
                stat_row = stat_row.push(
                    text(format!("~{}", stats.modified_blocks))
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(p.warning_fg)
                        .wrapping(text::Wrapping::None),
                );
            }
            mode_bar = mode_bar.push(
                container(stat_row)
                    .padding([2, 6])
                    .style(theme::section_panel_box(p)),
            );
        }

        if v.diff_image.is_some() {
            mode_bar = mode_bar.push(
                button(
                    row![
                        icons::image_frame(
                            12.0,
                            if v.mode == DetachedViewMode::ImageDiff {
                                p.accent_fg
                            } else {
                                p.text_muted
                            }
                        ),
                        text("Image Diff")
                            .size(type_scale::CAPTION)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([3, 8])
                .style(theme::filter_pill_btn(
                    p,
                    v.mode == DetachedViewMode::ImageDiff,
                ))
                .on_press(Message::Detached(
                    win_id,
                    DetachedAction::SetMode(DetachedViewMode::ImageDiff),
                )),
            );
        }

        if v.diff_csv.is_some() {
            mode_bar = mode_bar.push(
                button(
                    row![
                        icons::table_grid(
                            12.0,
                            if v.mode == DetachedViewMode::CsvDiff {
                                p.accent_fg
                            } else {
                                p.text_muted
                            }
                        ),
                        text("Table")
                            .size(type_scale::CAPTION)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([3, 8])
                .style(theme::filter_pill_btn(
                    p,
                    v.mode == DetachedViewMode::CsvDiff,
                ))
                .on_press(Message::Detached(
                    win_id,
                    DetachedAction::SetMode(DetachedViewMode::CsvDiff),
                )),
            );
        }

        if !is_raster_only && !v.diff_parsed.raw.is_empty() {
            mode_bar = mode_bar
                .push(
                    button(
                        text("Unified")
                            .size(type_scale::CAPTION)
                            .wrapping(text::Wrapping::None),
                    )
                    .padding([3, 8])
                    .style(theme::filter_pill_btn(
                        p,
                        v.mode == DetachedViewMode::UnifiedDiff,
                    ))
                    .on_press(Message::Detached(
                        win_id,
                        DetachedAction::SetMode(DetachedViewMode::UnifiedDiff),
                    )),
                )
                .push(
                    button(
                        text("Side by Side")
                            .size(type_scale::CAPTION)
                            .wrapping(text::Wrapping::None),
                    )
                    .padding([3, 8])
                    .style(theme::filter_pill_btn(
                        p,
                        v.mode == DetachedViewMode::SideBySideDiff,
                    ))
                    .on_press(Message::Detached(
                        win_id,
                        DetachedAction::SetMode(DetachedViewMode::SideBySideDiff),
                    )),
                );
        }

        if !matches!(v.preview_state, PreviewState::Empty) {
            mode_bar = mode_bar.push(
                button(
                    text("Preview")
                        .size(type_scale::CAPTION)
                        .wrapping(text::Wrapping::None),
                )
                .padding([3, 8])
                .style(theme::filter_pill_btn(
                    p,
                    v.mode == DetachedViewMode::Preview,
                ))
                .on_press(Message::Detached(
                    win_id,
                    DetachedAction::SetMode(DetachedViewMode::Preview),
                )),
            );
        }

        if is_text_diff && !v.diff_parsed.blocks.is_empty() {
            mode_bar = mode_bar.push(tip(
                button(
                    text("Compact")
                        .size(type_scale::CAPTION)
                        .wrapping(text::Wrapping::None),
                )
                .padding([3, 8])
                .style(theme::filter_pill_btn(p, v.diff_compact))
                .on_press(Message::Detached(win_id, DetachedAction::ToggleCompact)),
                if v.diff_compact {
                    "Compact mode ON (click to show full file)"
                } else {
                    "Compact mode OFF (click to collapse unchanged lines)"
                },
                tooltip::Position::Bottom,
                p,
            ));
            if has_changes {
                mode_bar = mode_bar
                    .push(tip(
                        button(icons::push(12.0, p.text_secondary))
                            .padding([4, 6])
                            .style(theme::subtle_btn(p))
                            .on_press(Message::Detached(win_id, DetachedAction::JumpChange(-1))),
                        "Previous Change",
                        tooltip::Position::Bottom,
                        p,
                    ))
                    .push(tip(
                        button(icons::pull(12.0, p.text_secondary))
                            .padding([4, 6])
                            .style(theme::subtle_btn(p))
                            .on_press(Message::Detached(win_id, DetachedAction::JumpChange(1))),
                        "Next Change",
                        tooltip::Position::Bottom,
                        p,
                    ));
            }
        }

        if v.mode == DetachedViewMode::Preview {
            if v.preview_state.has_rich_view() {
                mode_bar = mode_bar.push(
                    button(
                        text(if v.preview_raw_mode {
                            "Rendered"
                        } else {
                            "Select Text"
                        })
                        .size(type_scale::CAPTION)
                        .wrapping(text::Wrapping::None),
                    )
                    .padding([3, 8])
                    .style(theme::filter_pill_btn(p, v.preview_raw_mode))
                    .on_press(Message::Detached(win_id, DetachedAction::TogglePreviewRaw)),
                );
            }
            if v.preview_state.copyable_text().is_some() {
                let has_sel = v.preview_editor.selection().is_some_and(|s| !s.is_empty());
                mode_bar = mode_bar.push(
                    button(
                        row![
                            icons::copy(
                                12.0,
                                if has_sel {
                                    p.accent_fg
                                } else {
                                    p.text_secondary
                                }
                            ),
                            text(if has_sel { "Copy Sel" } else { "Copy" })
                                .size(type_scale::CAPTION)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                    )
                    .padding([3, 8])
                    .style(theme::filter_pill_btn(p, has_sel))
                    .on_press(Message::Detached(win_id, DetachedAction::CopyPreviewText)),
                );
            }
        }

        mode_bar = mode_bar.push(Space::with_width(Length::Fill)).push(
            text(&v.status_note)
                .size(type_scale::CAPTION)
                .color(if v.status_is_error {
                    p.danger_fg
                } else if v.loading {
                    p.accent_fg
                } else {
                    p.text_muted
                })
                .wrapping(text::Wrapping::None),
        );

        // ── Body ─────────────────────────────────────────────────────────────────
        let body: Element<'_, Message> = if !v.has_any_content() {
            let quick_actions = row![
                button(
                    row![
                        icons::file_code(13.0, Color::WHITE),
                        text("Open File to Preview")
                            .size(type_scale::CAPTION)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([6, 14])
                .style(theme::primary_btn(p))
                .on_press(Message::Detached(win_id, DetachedAction::PickSingleFile)),
                button(
                    row![
                        icons::file_diff(13.0, p.text_primary),
                        text("Pick Left File to Compare…")
                            .size(type_scale::CAPTION)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([6, 14])
                .style(theme::subtle_btn(p))
                .on_press(Message::Detached(win_id, DetachedAction::PickLeftFile)),
            ]
            .spacing(10)
            .align_y(Alignment::Center);

            theme::empty_state(
                p,
                icons::external_window(theme::icon_size::LG, p.accent_fg),
                "Independent Diff & Preview Window",
                "This popup operates completely independently from the main GitFleet window. Open any file to preview or pick Left & Right files to compare code, images, or CSV tables.",
                Some(quick_actions.into()),
            )
        } else {
            match v.mode {
                DetachedViewMode::ImageDiff => {
                    render_image_diff(v.diff_image.as_ref(), v.image_state, p, self.theme_mode)
                        .map(move |m| Message::Detached(win_id, DetachedAction::ImageMsg(m)))
                }
                DetachedViewMode::CsvDiff => render_csv_diff(
                    v.diff_csv.as_ref(),
                    v.csv_changes_only,
                    v.csv_ignore_format,
                    &v.csv_filter,
                    p,
                    self.theme_mode,
                )
                .map(move |m| Message::Detached(win_id, DetachedAction::CsvMsg(m))),
                DetachedViewMode::UnifiedDiff => render_unified_diff_with(
                    TextDiffRenderCtx {
                        parsed: &v.diff_parsed,
                        compact: v.diff_compact,
                        expanded_gaps: &v.diff_expanded_gaps,
                        is_working_file: false,
                        is_staged: false,
                        diff_title: &v.title,
                        selection: v.diff_selection,
                        left_scroll_id: v.diff_left_scroll_id.clone(),
                        right_scroll_id: v.diff_right_scroll_id.clone(),
                        unified_scroll_id: v.diff_unified_scroll_id.clone(),
                        left_scroll_y: v.diff_left_scroll_y,
                        right_scroll_y: v.diff_right_scroll_y,
                    },
                    p,
                )
                .map(move |m| map_text_diff_msg(win_id, m)),
                DetachedViewMode::SideBySideDiff => render_side_by_side_diff_with(
                    TextDiffRenderCtx {
                        parsed: &v.diff_parsed,
                        compact: v.diff_compact,
                        expanded_gaps: &v.diff_expanded_gaps,
                        is_working_file: false,
                        is_staged: false,
                        diff_title: &v.title,
                        selection: v.diff_selection,
                        left_scroll_id: v.diff_left_scroll_id.clone(),
                        right_scroll_id: v.diff_right_scroll_id.clone(),
                        unified_scroll_id: v.diff_unified_scroll_id.clone(),
                        left_scroll_y: v.diff_left_scroll_y,
                        right_scroll_y: v.diff_right_scroll_y,
                    },
                    p,
                )
                .map(move |m| map_text_diff_msg(win_id, m)),
                DetachedViewMode::Preview => render_file_preview(
                    &v.preview_state,
                    &v.preview_editor,
                    v.preview_raw_mode,
                    self.theme_mode,
                    p,
                )
                .map(move |m| map_text_diff_msg(win_id, m)),
            }
        };

        let content_panel = container(
            column![
                top_bar,
                horizontal_rule(1).style(theme::subtle_rule(p)),
                mode_bar,
                horizontal_rule(1).style(theme::subtle_rule(p)),
                body,
            ]
            .spacing(6)
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .padding(10)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::panel_box(p));

        container(content_panel)
            .padding(8)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::app_bg(p))
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_arbitrary_local_csv_and_text_files_independently() {
        let dir = std::env::temp_dir().join(format!(
            "gitfleet-detached-test-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);

        let left_csv = dir.join("old.csv");
        let right_csv = dir.join("new.csv");
        std::fs::write(&left_csv, "sku,stock\nA1,10\nA2,20\n").unwrap();
        std::fs::write(&right_csv, "sku,stock\nA1,15\nA2,20\n").unwrap();

        let csv_payload = compare_local_files_payload(
            left_csv.clone(),
            right_csv.clone(),
            DetachedViewMode::SideBySideDiff,
        )
        .expect("csv compare should succeed");

        assert_eq!(csv_payload.preferred_mode, DetachedViewMode::CsvDiff);
        let csv_data = csv_payload.csv_diff.expect("csv diff data");
        assert_eq!(csv_data.stats.modified_rows, 1);
        assert_eq!(csv_data.stats.changed_cells, 1);
        assert!(!csv_payload.parsed_diff.blocks.is_empty());

        let left_rs = dir.join("a.rs");
        let right_rs = dir.join("b.rs");
        std::fs::write(&left_rs, "fn main() {\n    println!(\"old\");\n}\n").unwrap();
        std::fs::write(&right_rs, "fn main() {\n    println!(\"new\");\n}\n").unwrap();

        let text_payload = compare_local_files_payload(
            left_rs.clone(),
            right_rs.clone(),
            DetachedViewMode::SideBySideDiff,
        )
        .expect("text compare should succeed");
        assert_eq!(text_payload.preferred_mode, DetachedViewMode::SideBySideDiff);
        assert_eq!(text_payload.parsed_diff.stats.modified_blocks, 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
