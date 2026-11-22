//! Window-local state update handlers for GitFleet Studio (tabs, diff scroll/selection,
//! editor actions, in-file find/replace, search filters, and status menus).

use super::io::{
    editor_buffer_text, file_display_name, jump_editor_to_span, spawn_compare_task,
};
use super::state::{
    DetachedAction, DetachedViewMode, StudioExplorerMenu, StudioPendingDelete, StudioSidebarPanel,
    StudioStatusMenu,
};
use crate::app::{
    DiffGapExpandMode, DiffSelectionSide, DiffTextPos, DiffTextSelection, EncodingModalMode,
    GitFleetApp, Message,
};
use crate::views::csv_diff::CsvDiffMsg;
use crate::views::diff_viewer::{
    diff_line_text_at, diff_row_count, extract_selected_diff_text, map_scroll_y, word_bounds_at,
};
use crate::views::image_diff::ImageDiffMsg;
use crate::views::overlays::file_ops_modal::{
    AddGitignoreModalState, RenameEntryModalState, collect_workspace_ignore_candidates,
};
use gitfleet_search::{replace_all_in_text, replace_single_match_in_text};
use iced::widget::{scrollable, text_editor, text_input};
use iced::{Task, clipboard, window};
use std::sync::Arc;
use std::time::{Duration, Instant};

impl GitFleetApp {
    pub(crate) fn update_detached_viewer_local(
        &mut self,
        win_id: window::Id,
        action: DetachedAction,
    ) -> Task<Message> {
        let theme_mode = self.theme_mode;
        let Some(v) = self.detached_viewers.get_mut(&win_id) else {
            return Task::none();
        };

        match action {
            // ── Tabs & Sidebar ───────────────────────────────────────────────
            DetachedAction::SelectTab(idx) => {
                if idx < v.tabs.len() {
                    v.active_tab = Some(idx);
                    v.pending_close_tab = None;
                    v.status_menu = None;
                    v.explorer_menu = None;
                    if let Some(tab) = v.tabs.get_mut(idx)
                        && !tab.loading_file
                    {
                        tab.hydrate_active_mode();
                    }
                    v.recompute_in_file_matches(false);
                }
                Task::none()
            }
            DetachedAction::CloseTab(idx) => {
                v.status_menu = None;
                if let Some(tab) = v.tabs.get(idx) {
                    if tab.dirty {
                        v.active_tab = Some(idx);
                        v.pending_close_tab = Some(tab.id);
                    } else {
                        v.remove_tab_at(idx);
                    }
                }
                Task::none()
            }
            DetachedAction::CloseActiveTab => {
                v.status_menu = None;
                if let Some(idx) = v.active_tab {
                    if v.tabs.get(idx).is_some_and(|t| t.dirty) {
                        v.pending_close_tab = v.tabs.get(idx).map(|t| t.id);
                    } else {
                        v.remove_tab_at(idx);
                    }
                }
                Task::none()
            }
            DetachedAction::CancelCloseTab => {
                v.pending_close_tab = None;
                Task::none()
            }
            DetachedAction::SetSidebarPanel(panel) => {
                if v.sidebar_open && v.sidebar_panel == panel {
                    v.sidebar_open = false;
                    return Task::none();
                }
                v.sidebar_open = true;
                v.sidebar_panel = panel;
                v.search_confirm_replace_all = false;
                match panel {
                    StudioSidebarPanel::Search => text_input::focus(v.search_input_id.clone()),
                    StudioSidebarPanel::Explorer => {
                        text_input::focus(v.explorer_filter_id.clone())
                    }
                    StudioSidebarPanel::Compare => Task::none(),
                }
            }
            DetachedAction::ToggleSidebar => {
                v.sidebar_open = !v.sidebar_open;
                Task::none()
            }
            DetachedAction::OpenAndFocusExplorer => {
                v.sidebar_open = true;
                v.sidebar_panel = StudioSidebarPanel::Explorer;
                text_input::focus(v.explorer_filter_id.clone())
            }
            DetachedAction::OpenAndFocusSearchSidebar => {
                v.sidebar_open = true;
                v.sidebar_panel = StudioSidebarPanel::Search;
                v.search_confirm_replace_all = false;
                if let Some(sel) = v
                    .active_tab()
                    .and_then(|t| t.editor_content.selection())
                    .filter(|s| !s.is_empty() && !s.contains('\n') && s.len() <= 120)
                {
                    v.search_query = sel;
                }
                text_input::focus(v.search_input_id.clone())
            }

            // ── Explorer ─────────────────────────────────────────────────────
            DetachedAction::ExplorerFilterChanged(q) => {
                v.explorer_filter = q;
                Task::none()
            }
            DetachedAction::ExplorerIgnoredDirLoaded(rid, rel_dir, listing) => {
                if rid == v.explorer_request_id {
                    v.explorer_ignored_dir_children.insert(rel_dir, listing);
                    v.rebuild_explorer_tree();
                }
                Task::none()
            }
            DetachedAction::OpenExplorerMenu { rel_path, is_dir } => {
                v.status_menu = None;
                let pos = crate::app::subscription::latest_cursor_pos(win_id)
                    .unwrap_or(v.cursor_pos);
                v.explorer_menu = Some(StudioExplorerMenu {
                    rel_path,
                    is_dir,
                    position: pos,
                });
                Task::none()
            }
            DetachedAction::CloseExplorerMenu => {
                v.explorer_menu = None;
                Task::none()
            }
            DetachedAction::CopyExplorerPath(rel_path) => {
                v.explorer_menu = None;
                v.status_is_error = false;
                v.status_note = format!("Copied path '{rel_path}'");
                clipboard::write(rel_path)
            }
            DetachedAction::RequestDeleteExplorerEntry { rel_path, is_dir } => {
                v.explorer_menu = None;
                v.status_menu = None;
                v.pending_delete = Some(StudioPendingDelete { rel_path, is_dir });
                Task::none()
            }
            DetachedAction::CancelDeleteExplorerEntry => {
                v.pending_delete = None;
                Task::none()
            }
            DetachedAction::OpenExplorerRename { rel_path, is_dir } => {
                v.explorer_menu = None;
                v.status_menu = None;
                v.pending_delete = None;
                v.gitignore_modal = None;
                v.rename_modal = Some(RenameEntryModalState::new(rel_path, is_dir));
                text_input::focus(v.rename_input_id.clone())
            }
            DetachedAction::ExplorerRenameInputChanged(val) => {
                if let Some(m) = &mut v.rename_modal {
                    m.new_name = val;
                    m.error = None;
                }
                Task::none()
            }
            DetachedAction::CancelExplorerRename => {
                v.rename_modal = None;
                Task::none()
            }
            DetachedAction::OpenExplorerGitignore { rel_path, is_dir } => {
                v.explorer_menu = None;
                v.status_menu = None;
                v.pending_delete = None;
                v.rename_modal = None;
                let allow_personal = v
                    .search_root
                    .as_deref()
                    .is_some_and(|r| r.join(".git").exists());
                let candidates = collect_workspace_ignore_candidates(
                    &v.explorer_files,
                    &v.explorer_ignored_files,
                    &v.explorer_ignored_dirs,
                    &v.explorer_ignored_dir_children,
                );
                v.gitignore_modal = Some(AddGitignoreModalState::new(
                    rel_path,
                    is_dir,
                    allow_personal,
                    candidates,
                ));
                text_input::focus(v.gitignore_input_id.clone())
            }
            DetachedAction::ExplorerGitignoreModeChanged(use_regex) => {
                if let Some(m) = &mut v.gitignore_modal {
                    m.use_regex = use_regex;
                    m.error = None;
                    if use_regex {
                        m.recompute_regex_preview();
                    }
                }
                text_input::focus(v.gitignore_input_id.clone())
            }
            DetachedAction::ExplorerGitignoreGlobChanged(val) => {
                if let Some(m) = &mut v.gitignore_modal {
                    m.glob_pattern = val;
                    m.error = None;
                }
                Task::none()
            }
            DetachedAction::ExplorerGitignoreRegexChanged(val) => {
                if let Some(m) = &mut v.gitignore_modal {
                    m.regex_pattern = val;
                    m.error = None;
                    m.recompute_regex_preview();
                }
                Task::none()
            }
            DetachedAction::ExplorerGitignoreExpandMatchesToggled(expand) => {
                if let Some(m) = &mut v.gitignore_modal {
                    m.regex_expand_matches = expand;
                    m.error = None;
                    m.recompute_regex_preview();
                }
                Task::none()
            }
            DetachedAction::ExplorerGitignoreLayerChanged(layer) => {
                if let Some(m) = &mut v.gitignore_modal {
                    m.layer = layer;
                    m.error = None;
                }
                Task::none()
            }
            DetachedAction::CancelExplorerGitignore => {
                v.gitignore_modal = None;
                Task::none()
            }

            // ── View Mode & Diff Controls ────────────────────────────────────
            DetachedAction::SetMode(mode) => {
                if let Some(tab) = v.active_tab_mut() {
                    tab.mode = mode;
                    if matches!(
                        mode,
                        DetachedViewMode::UnifiedDiff | DetachedViewMode::SideBySideDiff
                    ) {
                        tab.last_text_diff_mode = mode;
                    } else if mode == DetachedViewMode::Edit && !tab.loading_file {
                        tab.ensure_editor_hydrated();
                    } else if mode == DetachedViewMode::Preview && !tab.loading_file {
                        if tab.dirty {
                            tab.preview_hydrated = false;
                        }
                        tab.ensure_preview_hydrated();
                    }
                    tab.diff_selection = None;
                }
                v.recompute_in_file_matches(false);
                Task::none()
            }
            DetachedAction::ToggleCompact => {
                if let Some(tab) = v.active_tab_mut() {
                    tab.diff_compact = !tab.diff_compact;
                    tab.diff_selection = None;
                    tab.diff_left_scroll_y = 0.0;
                    tab.diff_right_scroll_y = 0.0;
                    tab.diff_unified_scroll_y = 0.0;
                    tab.rebuild_diff_cache(theme_mode);
                    return Task::batch([
                        scrollable::scroll_to(
                            tab.diff_left_scroll_id.clone(),
                            scrollable::AbsoluteOffset {
                                x: tab.diff_left_scroll_x,
                                y: 0.0,
                            },
                        ),
                        scrollable::scroll_to(
                            tab.diff_right_scroll_id.clone(),
                            scrollable::AbsoluteOffset {
                                x: tab.diff_right_scroll_x,
                                y: 0.0,
                            },
                        ),
                        scrollable::scroll_to(
                            tab.diff_unified_scroll_id.clone(),
                            scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
                        ),
                    ]);
                }
                Task::none()
            }
            DetachedAction::ExpandGap { gap_id, mode } => {
                if let Some(tab) = v.active_tab_mut() {
                    tab.diff_selection = None;
                    let entry = tab.diff_expanded_gaps.entry(gap_id).or_default();
                    match mode {
                        DiffGapExpandMode::Top(n) => entry.top_shown += n,
                        DiffGapExpandMode::Bottom(n) => entry.bottom_shown += n,
                        DiffGapExpandMode::All => entry.show_all = true,
                    }
                    tab.rebuild_diff_cache(theme_mode);
                }
                Task::none()
            }
            DetachedAction::LeftScrolled(vp) => {
                let Some(tab) = v.active_tab_mut() else {
                    return Task::none();
                };
                let off = vp.absolute_offset();
                let pane_h = vp.bounds().height.max(1.0);
                tab.diff_left_viewport_h = pane_h;
                tab.diff_right_viewport_h = pane_h;
                let x_changed = (off.x - tab.diff_left_scroll_x).abs() >= 0.5;
                let y_changed = (off.y - tab.diff_left_scroll_y).abs() >= 0.5;
                if !x_changed && !y_changed {
                    return Task::none();
                }
                tab.diff_left_scroll_x = off.x;
                tab.diff_left_scroll_y = off.y;

                let target_right_x = off.x.max(0.0);
                let target_right_y = if y_changed {
                    let layout = &tab.diff_cache.layout;
                    let raw_right_y = map_scroll_y(off.y, &layout.anchors, true);
                    let max_right_y = ((layout.total_right_h + 10.0) - pane_h).max(0.0);
                    raw_right_y.clamp(0.0, max_right_y)
                } else {
                    tab.diff_right_scroll_y
                };

                if (target_right_x - tab.diff_right_scroll_x).abs() < 0.5
                    && (target_right_y - tab.diff_right_scroll_y).abs() < 0.5
                {
                    return Task::none();
                }
                tab.diff_right_scroll_x = target_right_x;
                tab.diff_right_scroll_y = target_right_y;
                scrollable::scroll_to(
                    tab.diff_right_scroll_id.clone(),
                    scrollable::AbsoluteOffset {
                        x: target_right_x,
                        y: target_right_y,
                    },
                )
            }
            DetachedAction::RightScrolled(vp) => {
                let Some(tab) = v.active_tab_mut() else {
                    return Task::none();
                };
                let off = vp.absolute_offset();
                let pane_h = vp.bounds().height.max(1.0);
                tab.diff_left_viewport_h = pane_h;
                tab.diff_right_viewport_h = pane_h;
                let x_changed = (off.x - tab.diff_right_scroll_x).abs() >= 0.5;
                let y_changed = (off.y - tab.diff_right_scroll_y).abs() >= 0.5;
                if !x_changed && !y_changed {
                    return Task::none();
                }
                tab.diff_right_scroll_x = off.x;
                tab.diff_right_scroll_y = off.y;

                let target_left_x = off.x.max(0.0);
                let target_left_y = if y_changed {
                    let layout = &tab.diff_cache.layout;
                    let raw_left_y = map_scroll_y(off.y, &layout.anchors, false);
                    let max_left_y = ((layout.total_left_h + 10.0) - pane_h).max(0.0);
                    raw_left_y.clamp(0.0, max_left_y)
                } else {
                    tab.diff_left_scroll_y
                };

                if (target_left_x - tab.diff_left_scroll_x).abs() < 0.5
                    && (target_left_y - tab.diff_left_scroll_y).abs() < 0.5
                {
                    return Task::none();
                }
                tab.diff_left_scroll_x = target_left_x;
                tab.diff_left_scroll_y = target_left_y;
                scrollable::scroll_to(
                    tab.diff_left_scroll_id.clone(),
                    scrollable::AbsoluteOffset {
                        x: target_left_x,
                        y: target_left_y,
                    },
                )
            }
            DetachedAction::UnifiedScrolled(vp) => {
                if let Some(tab) = v.active_tab_mut() {
                    let off = vp.absolute_offset();
                    let pane_h = vp.bounds().height.max(1.0);
                    tab.diff_unified_viewport_h = pane_h;
                    if (off.y - tab.diff_unified_scroll_y).abs() >= 0.5 {
                        tab.diff_unified_scroll_y = off.y;
                    }
                }
                Task::none()
            }
            DetachedAction::JumpChange(delta) => {
                let Some(tab) = v.active_tab_mut() else {
                    return Task::none();
                };
                let count = tab.diff_cache.layout.connectors.len();
                if count == 0 {
                    return Task::none();
                }
                let next_idx = if delta < 0 {
                    if tab.diff_active_change == 0 {
                        count - 1
                    } else {
                        tab.diff_active_change - 1
                    }
                } else {
                    (tab.diff_active_change + 1) % count
                };
                tab.diff_active_change = next_idx;
                let span = tab.diff_cache.layout.connectors[next_idx];
                let margin = 48.0;
                let ly = (span.left_top - margin).max(0.0);
                let ry = (span.right_top - margin).max(0.0);
                let uy = (span.unified_top - margin).max(0.0);
                tab.diff_left_scroll_y = ly;
                tab.diff_right_scroll_y = ry;
                tab.diff_unified_scroll_y = uy;
                match tab.mode {
                    DetachedViewMode::SideBySideDiff => Task::batch([
                        scrollable::scroll_to(
                            tab.diff_left_scroll_id.clone(),
                            scrollable::AbsoluteOffset {
                                x: tab.diff_left_scroll_x,
                                y: ly,
                            },
                        ),
                        scrollable::scroll_to(
                            tab.diff_right_scroll_id.clone(),
                            scrollable::AbsoluteOffset {
                                x: tab.diff_right_scroll_x,
                                y: ry,
                            },
                        ),
                    ]),
                    DetachedViewMode::UnifiedDiff => scrollable::scroll_to(
                        tab.diff_unified_scroll_id.clone(),
                        scrollable::AbsoluteOffset { x: 0.0, y: uy },
                    ),
                    _ => Task::none(),
                }
            }
            DetachedAction::SelStart { side, row, col } => {
                let Some(tab) = v.active_tab_mut() else {
                    return Task::none();
                };
                let now = Instant::now();
                let click_count = match tab.diff_last_click {
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
                tab.diff_last_click = Some((now, side, row, col, click_count));

                if click_count >= 2
                    && let Some(line) =
                        diff_line_text_at(&tab.diff_parsed, &tab.diff_cache.layout, side, row)
                {
                    let (s, e) = if click_count == 2 {
                        word_bounds_at(&line, col)
                    } else {
                        (0, line.chars().count())
                    };
                    tab.diff_selection = Some(DiffTextSelection {
                        side,
                        anchor: DiffTextPos { row, col: s },
                        focus: DiffTextPos { row, col: e },
                        dragging: false,
                    });
                    return Task::none();
                }

                let pos = DiffTextPos { row, col };
                tab.diff_selection = Some(DiffTextSelection {
                    side,
                    anchor: pos,
                    focus: pos,
                    dragging: true,
                });
                Task::none()
            }
            DetachedAction::SelDrag { side, row, col } => {
                if let Some(tab) = v.active_tab_mut()
                    && let Some(sel) = &mut tab.diff_selection
                    && sel.dragging
                    && sel.side == side
                {
                    sel.focus = DiffTextPos { row, col };
                }
                Task::none()
            }
            DetachedAction::CopySelection => {
                if let Some(tab) = v.active_tab()
                    && matches!(
                        tab.mode,
                        DetachedViewMode::UnifiedDiff | DetachedViewMode::SideBySideDiff
                    )
                    && let Some(sel) = tab.diff_selection
                    && !sel.is_empty()
                {
                    let copied =
                        extract_selected_diff_text(&tab.diff_parsed, &tab.diff_cache.layout, sel);
                    if !copied.is_empty() {
                        let lines = copied.lines().count();
                        v.status_is_error = false;
                        v.status_note = if lines > 1 {
                            format!("Copied {lines} lines")
                        } else {
                            "Copied selection".into()
                        };
                        return clipboard::write(copied);
                    }
                }
                Task::none()
            }
            DetachedAction::SelectAll => {
                if let Some(tab) = v.active_tab_mut()
                    && !tab.diff_parsed.raw.is_empty()
                    && matches!(
                        tab.mode,
                        DetachedViewMode::UnifiedDiff | DetachedViewMode::SideBySideDiff
                    )
                {
                    let side = if tab.diff_parsed.blocks.is_empty() {
                        DiffSelectionSide::Raw
                    } else if tab.mode == DetachedViewMode::UnifiedDiff {
                        DiffSelectionSide::Unified
                    } else {
                        tab.diff_selection
                            .map(|s| s.side)
                            .filter(|s| {
                                matches!(s, DiffSelectionSide::Left | DiffSelectionSide::Right)
                            })
                            .unwrap_or(DiffSelectionSide::Right)
                    };
                    let count = diff_row_count(&tab.diff_parsed, &tab.diff_cache.layout, side);
                    if count > 0 {
                        let last_row = count - 1;
                        let last_col = diff_line_text_at(
                            &tab.diff_parsed,
                            &tab.diff_cache.layout,
                            side,
                            last_row,
                        )
                        .map(|l| l.chars().count())
                        .unwrap_or(0);
                        tab.diff_selection = Some(DiffTextSelection {
                            side,
                            anchor: DiffTextPos { row: 0, col: 0 },
                            focus: DiffTextPos {
                                row: last_row,
                                col: last_col,
                            },
                            dragging: false,
                        });
                    }
                }
                Task::none()
            }
            DetachedAction::ImageMsg(msg) => {
                if let Some(tab) = v.active_tab_mut() {
                    match msg {
                        ImageDiffMsg::SetMode(m) => {
                            tab.image_state.mode = m;
                            tab.image_state.blink_after = false;
                        }
                        ImageDiffMsg::SetZoom(z) => tab.image_state.zoom = z,
                        ImageDiffMsg::ToggleSwap => {
                            tab.image_state.swapped = !tab.image_state.swapped
                        }
                        ImageDiffMsg::SwipeChanged(pos) => {
                            tab.image_state.swipe = pos.clamp(0.0, 1.0)
                        }
                        ImageDiffMsg::OnionChanged(alpha) => {
                            tab.image_state.onion = alpha.clamp(0.0, 1.0)
                        }
                        ImageDiffMsg::BlinkFlip => {
                            tab.image_state.blink_after = !tab.image_state.blink_after;
                        }
                        ImageDiffMsg::ToggleBlinkPause => {
                            tab.image_state.blink_paused = !tab.image_state.blink_paused;
                        }
                    }
                }
                Task::none()
            }
            DetachedAction::CsvMsg(msg) => {
                if let Some(tab) = v.active_tab_mut() {
                    match msg {
                        CsvDiffMsg::SetChangesOnly(only) => tab.csv_changes_only = only,
                        CsvDiffMsg::ToggleIgnoreFormat => {
                            tab.csv_ignore_format = !tab.csv_ignore_format
                        }
                        CsvDiffMsg::FilterChanged(q) => tab.csv_filter = q,
                    }
                }
                Task::none()
            }
            DetachedAction::PreviewEditorAction(action) => {
                if !action.is_edit()
                    && let Some(tab) = v.active_tab_mut()
                {
                    tab.preview_editor.perform(action);
                }
                Task::none()
            }
            DetachedAction::PreviewLinkClicked(url) => {
                v.status_is_error = false;
                v.status_note = format!("Link: {url}");
                Task::none()
            }
            DetachedAction::TogglePreviewRaw => {
                if let Some(tab) = v.active_tab_mut() {
                    tab.preview_raw_mode = !tab.preview_raw_mode;
                }
                Task::none()
            }
            DetachedAction::CopyPreviewText => {
                let copy_str = v.active_tab().and_then(|tab| {
                    if let Some(sel) = tab.preview_editor.selection().filter(|s| !s.is_empty()) {
                        Some((sel, "Copied selection"))
                    } else {
                        tab.preview_state
                            .copyable_text()
                            .map(|full| (full.to_string(), "Copied preview"))
                    }
                });
                if let Some((txt, msg)) = copy_str {
                    v.status_is_error = false;
                    v.status_note = msg.into();
                    return clipboard::write(txt);
                }
                Task::none()
            }

            // ── File / Compare Pickers ───────────────────────────────────────
            DetachedAction::PickSingleFile => {
                let start = v.search_root.clone();
                Task::perform(
                    async move {
                        let mut dlg =
                            rfd::AsyncFileDialog::new().set_title("Open File in GitFleet Studio");
                        if let Some(d) = start {
                            dlg = dlg.set_directory(d);
                        }
                        dlg.pick_file().await.map(|h| h.path().to_path_buf())
                    },
                    move |p| Message::Detached(win_id, DetachedAction::SingleFilePicked(p)),
                )
            }
            DetachedAction::PickLeftFile => {
                let start = v.search_root.clone();
                Task::perform(
                    async move {
                        let mut dlg = rfd::AsyncFileDialog::new()
                            .set_title("Select Left (Before) File to Compare");
                        if let Some(d) = start {
                            dlg = dlg.set_directory(d);
                        }
                        dlg.pick_file().await.map(|h| h.path().to_path_buf())
                    },
                    move |p| Message::Detached(win_id, DetachedAction::LeftFilePicked(p)),
                )
            }
            DetachedAction::PickRightFile => {
                let start = v.search_root.clone();
                Task::perform(
                    async move {
                        let mut dlg = rfd::AsyncFileDialog::new()
                            .set_title("Select Right (After) File to Compare");
                        if let Some(d) = start {
                            dlg = dlg.set_directory(d);
                        }
                        dlg.pick_file().await.map(|h| h.path().to_path_buf())
                    },
                    move |p| Message::Detached(win_id, DetachedAction::RightFilePicked(p)),
                )
            }
            DetachedAction::SingleFilePicked(None) => Task::none(),
            DetachedAction::TabFileLoaded(tab_id, req_id, payload) => {
                let root = v.search_root.clone();
                let tab_idx = v
                    .tabs
                    .iter()
                    .position(|t| t.id == tab_id && t.file_load_req_id == req_id);
                let Some(idx) = tab_idx else {
                    return Task::none();
                };
                let is_active = v.active_tab == Some(idx);
                let editable = {
                    let tab = &mut v.tabs[idx];
                    let ok = tab.apply_loaded_doc(&payload.path, root.as_deref(), payload.doc);
                    if !ok && tab.mode == DetachedViewMode::Edit {
                        tab.mode = DetachedViewMode::Preview;
                    }
                    if payload.jump_match.is_some() && ok {
                        tab.mode = DetachedViewMode::Edit;
                    }
                    tab.hydrate_active_mode();
                    if let Some((line_idx, col_start, col_end)) = payload.jump_match
                        && ok
                    {
                        jump_editor_to_span(&mut tab.editor_content, line_idx, col_start, col_end);
                    }
                    ok
                };
                if is_active {
                    v.recompute_in_file_matches(false);
                    if let Some((line_idx, col_start, _)) = payload.jump_match
                        && editable
                    {
                        if let Some(pos) = v
                            .in_file_matches
                            .iter()
                            .position(|m| m.line_index == line_idx && m.col_start == col_start)
                        {
                            v.in_file_active_idx = Some(pos);
                        }
                        v.status_is_error = false;
                        v.status_note = format!("Line {}", line_idx + 1);
                    } else if let Some(tab) = v.tabs.get(idx) {
                        v.status_is_error = false;
                        v.status_note = tab.rel_path.clone().unwrap_or_else(|| tab.title.clone());
                    }
                }
                Task::none()
            }
            DetachedAction::LeftFilePicked(Some(left)) => {
                v.compare_left = Some(left.clone());
                if let Some(right) = v.compare_right.clone() {
                    v.request_id = v.request_id.wrapping_add(1);
                    let req_id = v.request_id;
                    let pref = v
                        .active_tab()
                        .map(|t| t.last_text_diff_mode)
                        .unwrap_or(DetachedViewMode::SideBySideDiff);
                    v.loading = true;
                    v.status_is_error = false;
                    v.status_note = format!(
                        "Comparing {} ↔ {}…",
                        file_display_name(&left),
                        file_display_name(&right)
                    );
                    spawn_compare_task(win_id, req_id, left, right, pref)
                } else {
                    v.status_is_error = false;
                    v.status_note = format!(
                        "Left file set ({}) — pick Right file to compare",
                        file_display_name(&left)
                    );
                    Task::none()
                }
            }
            DetachedAction::LeftFilePicked(None) => Task::none(),
            DetachedAction::RightFilePicked(Some(right)) => {
                v.compare_right = Some(right.clone());
                if let Some(left) = v.compare_left.clone() {
                    v.request_id = v.request_id.wrapping_add(1);
                    let req_id = v.request_id;
                    let pref = v
                        .active_tab()
                        .map(|t| t.last_text_diff_mode)
                        .unwrap_or(DetachedViewMode::SideBySideDiff);
                    v.loading = true;
                    v.status_is_error = false;
                    v.status_note = format!(
                        "Comparing {} ↔ {}…",
                        file_display_name(&left),
                        file_display_name(&right)
                    );
                    spawn_compare_task(win_id, req_id, left, right, pref)
                } else {
                    v.status_is_error = false;
                    v.status_note = format!(
                        "Right file set ({}) — pick Left file to compare",
                        file_display_name(&right)
                    );
                    Task::none()
                }
            }
            DetachedAction::RightFilePicked(None) => Task::none(),
            DetachedAction::SwapCompareFiles => {
                if let (Some(left), Some(right)) = (v.compare_left.clone(), v.compare_right.clone())
                {
                    v.compare_left = Some(right.clone());
                    v.compare_right = Some(left.clone());
                    v.request_id = v.request_id.wrapping_add(1);
                    let req_id = v.request_id;
                    let pref = v
                        .active_tab()
                        .map(|t| t.last_text_diff_mode)
                        .unwrap_or(DetachedViewMode::SideBySideDiff);
                    v.loading = true;
                    spawn_compare_task(win_id, req_id, right, left, pref)
                } else if let Some(tab) = v.active_tab_mut()
                    && tab.diff_image.is_some()
                {
                    tab.image_state.swapped = !tab.image_state.swapped;
                    Task::none()
                } else {
                    Task::none()
                }
            }
            DetachedAction::LoadFailed(req_id, err) => {
                if req_id == v.request_id {
                    v.loading = false;
                    v.status_is_error = true;
                    v.status_note = err;
                }
                Task::none()
            }
            DetachedAction::GitDiffLoaded(tab_id, req_id, res) => {
                if req_id != v.request_id {
                    return Task::none();
                }
                if let Ok((parsed, cache)) = res
                    && let Some(tab) = v.tabs.iter_mut().find(|t| t.id == tab_id)
                {
                    tab.diff_parsed = parsed;
                    tab.reset_scroll_state();
                    if cache.matches_config(tab.diff_compact, tab.language, theme_mode) {
                        tab.diff_cache = cache;
                    } else {
                        tab.rebuild_diff_cache(theme_mode);
                    }
                }
                Task::none()
            }
            DetachedAction::GitImageDiffLoaded(tab_id, req_id, data) => {
                if req_id != v.request_id {
                    return Task::none();
                }
                if let Some(tab) = v.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.image_state.swipe = match (&data.before, &data.after) {
                        (None, Some(_)) => 0.0,
                        (Some(_), None) => 1.0,
                        _ => 0.5,
                    };
                    tab.diff_image = Some(data);
                }
                Task::none()
            }
            DetachedAction::GitCsvDiffLoaded(tab_id, req_id, data) => {
                if req_id != v.request_id {
                    return Task::none();
                }
                if let Some(tab) = v.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.diff_csv = Some(data);
                }
                Task::none()
            }

            // ── Direct Code Editor ───────────────────────────────────────────
            DetachedAction::EditorAction(action) => {
                let is_edit = action.is_edit();
                if let Some(tab) = v.active_tab_mut() {
                    tab.editor_content.perform(action);
                    if is_edit {
                        tab.dirty = true;
                        tab.preview_hydrated = false;
                    }
                }
                if is_edit && v.in_file_find_open && !v.in_file_query.is_empty() {
                    v.recompute_in_file_matches(true);
                }
                Task::none()
            }
            DetachedAction::InsertIndent => {
                if let Some(tab) = v.active_tab_mut()
                    && tab.mode == DetachedViewMode::Edit
                    && tab.is_editable()
                {
                    tab.ensure_editor_hydrated();
                    let spaces = " ".repeat(tab.language.default_indent_spaces());
                    tab.editor_content
                        .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                            Arc::new(spaces),
                        )));
                    tab.dirty = true;
                    tab.preview_hydrated = false;
                }
                if v.in_file_find_open && !v.in_file_query.is_empty() {
                    v.recompute_in_file_matches(true);
                }
                Task::none()
            }
            DetachedAction::OpenStatusMenu(kind) => {
                if v.status_menu.is_some_and(|(cur, _)| cur == kind) {
                    v.status_menu = None;
                    return Task::none();
                }
                let anchor = crate::app::subscription::latest_cursor_pos(win_id)
                    .unwrap_or(v.cursor_pos);
                v.status_menu = Some((kind, anchor));
                if kind == StudioStatusMenu::Encoding {
                    v.encoding_menu_mode = EncodingModalMode::SaveWithEncoding;
                }
                if kind == StudioStatusMenu::Syntax {
                    v.syntax_filter.clear();
                    return text_input::focus(v.syntax_filter_id.clone());
                }
                Task::none()
            }
            DetachedAction::CloseStatusMenu => {
                v.status_menu = None;
                Task::none()
            }
            DetachedAction::SetEncodingMenuMode(mode) => {
                v.encoding_menu_mode = mode;
                Task::none()
            }
            DetachedAction::SyntaxFilterChanged(q) => {
                v.syntax_filter = q;
                Task::none()
            }
            DetachedAction::SetTabLanguage(lang) => {
                v.status_menu = None;
                if let Some(tab) = v.active_tab_mut() {
                    tab.language = lang;
                    tab.rebuild_diff_cache(theme_mode);
                }
                v.status_is_error = false;
                v.status_note = format!("Syntax: {}", lang.label());
                Task::none()
            }
            DetachedAction::ToggleWordWrap => {
                v.editor_word_wrap = !v.editor_word_wrap;
                v.status_is_error = false;
                v.status_note = if v.editor_word_wrap {
                    "Word Wrap: On".into()
                } else {
                    "Word Wrap: Off".into()
                };
                Task::none()
            }
            DetachedAction::AdjustFontSize(delta) => {
                v.editor_font_size = (v.editor_font_size + delta).clamp(11.0, 22.0);
                Task::none()
            }

            // ── In-File Find & Replace ───────────────────────────────────────
            DetachedAction::OpenInFileFind { show_replace } => {
                let can_find = v
                    .active_tab()
                    .is_some_and(|t| t.is_editable() || t.has_preview_available());
                if !can_find {
                    v.status_is_error = false;
                    v.status_note = "Open a text file to search inside it".into();
                    return Task::none();
                }

                if let Some(tab) = v.active_tab_mut() {
                    if matches!(
                        tab.mode,
                        DetachedViewMode::ImageDiff
                            | DetachedViewMode::CsvDiff
                            | DetachedViewMode::SideBySideDiff
                            | DetachedViewMode::UnifiedDiff
                    ) && tab.is_editable()
                    {
                        tab.mode = DetachedViewMode::Edit;
                        tab.ensure_editor_hydrated();
                    } else if tab.mode == DetachedViewMode::Preview {
                        tab.ensure_preview_hydrated();
                    }
                    if let Some(sel) = tab
                        .editor_content
                        .selection()
                        .or_else(|| tab.preview_editor.selection())
                        .filter(|s| !s.is_empty() && !s.contains('\n') && s.len() <= 120)
                    {
                        v.in_file_query = sel;
                    }
                }

                v.in_file_find_open = true;
                if show_replace {
                    v.in_file_show_replace = true;
                }
                v.recompute_in_file_matches(false);
                if show_replace && !v.in_file_query.is_empty() {
                    text_input::focus(v.in_file_replace_input_id.clone())
                } else {
                    text_input::focus(v.in_file_input_id.clone())
                }
            }
            DetachedAction::CloseInFileFind => {
                v.in_file_find_open = false;
                Task::none()
            }
            DetachedAction::ToggleInFileReplace => {
                v.in_file_show_replace = !v.in_file_show_replace;
                if v.in_file_show_replace {
                    text_input::focus(v.in_file_replace_input_id.clone())
                } else {
                    Task::none()
                }
            }
            DetachedAction::InFileQueryChanged(q) => {
                v.in_file_query = q;
                v.recompute_in_file_matches(false);
                v.jump_active_editor_to_match();
                Task::none()
            }
            DetachedAction::InFileReplaceChanged(r) => {
                v.in_file_replace = r;
                Task::none()
            }
            DetachedAction::ToggleInFileCase => {
                v.in_file_options.case_sensitive = !v.in_file_options.case_sensitive;
                v.recompute_in_file_matches(false);
                v.jump_active_editor_to_match();
                Task::none()
            }
            DetachedAction::ToggleInFileWholeWord => {
                v.in_file_options.whole_word = !v.in_file_options.whole_word;
                v.recompute_in_file_matches(false);
                v.jump_active_editor_to_match();
                Task::none()
            }
            DetachedAction::ToggleInFileRegex => {
                v.in_file_options.regex = !v.in_file_options.regex;
                v.recompute_in_file_matches(false);
                v.jump_active_editor_to_match();
                Task::none()
            }
            DetachedAction::InFileJump(delta) => {
                let len = v.in_file_matches.len();
                if len == 0 {
                    return Task::none();
                }
                let next = match v.in_file_active_idx {
                    None => 0,
                    Some(cur) => {
                        if delta < 0 {
                            if cur == 0 { len - 1 } else { cur - 1 }
                        } else {
                            (cur + 1) % len
                        }
                    }
                };
                v.in_file_active_idx = Some(next);
                v.jump_active_editor_to_match();
                Task::none()
            }
            DetachedAction::InFileJumpIndex(idx) => {
                if idx < v.in_file_matches.len() {
                    v.in_file_active_idx = Some(idx);
                    if let Some(tab) = v.active_tab_mut()
                        && tab.is_editable()
                    {
                        tab.mode = DetachedViewMode::Edit;
                        tab.ensure_editor_hydrated();
                    }
                    v.jump_active_editor_to_match();
                }
                Task::none()
            }
            DetachedAction::InFileReplaceCurrent => {
                let Some(target) = v
                    .in_file_active_idx
                    .and_then(|i| v.in_file_matches.get(i))
                    .cloned()
                else {
                    return Task::none();
                };
                let query = v.in_file_query.clone();
                let repl = v.in_file_replace.clone();
                let opts = v.in_file_options;
                let res = v.active_tab_mut().and_then(|tab| {
                    if !tab.is_editable() {
                        return None;
                    }
                    tab.mode = DetachedViewMode::Edit;
                    tab.ensure_editor_hydrated();
                    let cur = editor_buffer_text(&tab.editor_content);
                    Some(replace_single_match_in_text(&cur, &target, &query, &repl, opts))
                });
                match res {
                    Some(Ok(updated)) => {
                        if let Some(tab) = v.active_tab_mut() {
                            tab.editor_content = text_editor::Content::with_text(&updated);
                            tab.editor_hydrated = true;
                            tab.dirty = true;
                            tab.preview_hydrated = false;
                        }
                        v.recompute_in_file_matches(true);
                        v.jump_active_editor_to_match();
                        v.status_is_error = false;
                        v.status_note = "Replaced 1 occurrence".into();
                    }
                    Some(Err(err)) => {
                        v.status_is_error = true;
                        v.status_note = err;
                    }
                    None => {}
                }
                Task::none()
            }
            DetachedAction::InFileReplaceAll => {
                if v.in_file_query.is_empty() {
                    return Task::none();
                }
                let query = v.in_file_query.clone();
                let repl = v.in_file_replace.clone();
                let opts = v.in_file_options;
                let res = v.active_tab_mut().and_then(|tab| {
                    if !tab.is_editable() {
                        return None;
                    }
                    tab.mode = DetachedViewMode::Edit;
                    tab.ensure_editor_hydrated();
                    let cur = editor_buffer_text(&tab.editor_content);
                    Some(replace_all_in_text(&cur, &query, &repl, opts))
                });
                match res {
                    Some(Ok((updated, count))) => {
                        if count > 0 {
                            if let Some(tab) = v.active_tab_mut() {
                                tab.editor_content = text_editor::Content::with_text(&updated);
                                tab.editor_hydrated = true;
                                tab.dirty = true;
                                tab.preview_hydrated = false;
                            }
                            v.recompute_in_file_matches(false);
                            v.status_is_error = false;
                            v.status_note = format!("Replaced {count} occurrence(s)");
                        } else {
                            v.status_is_error = false;
                            v.status_note = "No matches to replace".into();
                        }
                    }
                    Some(Err(err)) => {
                        v.status_is_error = true;
                        v.status_note = err;
                    }
                    None => {}
                }
                Task::none()
            }

            // ── Search & Replace Sidebar ─────────────────────────────────────
            DetachedAction::PickSearchRoot => {
                let start = v.search_root.clone();
                Task::perform(
                    async move {
                        let mut dlg = rfd::AsyncFileDialog::new()
                            .set_title("Open Folder in GitFleet Studio");
                        if let Some(d) = start {
                            dlg = dlg.set_directory(d);
                        }
                        dlg.pick_folder().await.map(|h| h.path().to_path_buf())
                    },
                    move |p| Message::Detached(win_id, DetachedAction::SearchRootPicked(p)),
                )
            }
            DetachedAction::SearchRootPicked(None) => Task::none(),
            DetachedAction::SearchQueryChanged(q) => {
                v.search_query = q;
                v.search_confirm_replace_all = false;
                Task::none()
            }
            DetachedAction::SearchReplaceChanged(r) => {
                v.search_replace_text = r;
                v.search_confirm_replace_all = false;
                Task::none()
            }
            DetachedAction::SearchIncludeChanged(inc) => {
                v.search_include = inc;
                v.search_confirm_replace_all = false;
                Task::none()
            }
            DetachedAction::SearchExcludeChanged(exc) => {
                v.search_exclude = exc;
                v.search_confirm_replace_all = false;
                Task::none()
            }
            DetachedAction::ToggleSearchReplaceRow => {
                v.search_show_replace = !v.search_show_replace;
                v.search_confirm_replace_all = false;
                Task::none()
            }
            DetachedAction::ToggleSearchFilters => {
                v.search_show_filters = !v.search_show_filters;
                Task::none()
            }
            DetachedAction::SearchCompleted(rid, result) => {
                if rid != v.search_request_id {
                    return Task::none();
                }
                v.search_running = false;
                v.search_collapsed_groups.clear();
                v.search_confirm_replace_all = false;
                if let Some(err) = &result.error {
                    v.status_is_error = true;
                    v.status_note = err.clone();
                } else {
                    v.status_is_error = false;
                    v.status_note = if v.search_query.is_empty() {
                        format!("{} file(s) matched", result.matched_files.len())
                    } else {
                        format!(
                            "{} result(s) in {} file(s)",
                            result.total_matches,
                            result.groups.len()
                        )
                    };
                }
                v.search_result = result;
                Task::none()
            }
            DetachedAction::ToggleSearchGroup(path) => {
                if !v.search_collapsed_groups.remove(&path) {
                    v.search_collapsed_groups.insert(path);
                }
                Task::none()
            }
            DetachedAction::ToggleSearchFilesSection => {
                v.search_files_collapsed = !v.search_files_collapsed;
                Task::none()
            }
            DetachedAction::CollapseExpandAllSearchGroups => {
                let all_collapsed = !v.search_result.groups.is_empty()
                    && v.search_result
                        .groups
                        .iter()
                        .all(|g| v.search_collapsed_groups.contains(&g.file_path));
                if all_collapsed {
                    v.search_collapsed_groups.clear();
                } else {
                    v.search_collapsed_groups = v
                        .search_result
                        .groups
                        .iter()
                        .map(|g| g.file_path.clone())
                        .collect();
                }
                Task::none()
            }
            DetachedAction::RequestReplaceAllInSearch => {
                if !v.search_query.is_empty() && !v.search_result.groups.is_empty() {
                    v.search_confirm_replace_all = true;
                }
                Task::none()
            }
            DetachedAction::CancelReplaceAllInSearch => {
                v.search_confirm_replace_all = false;
                Task::none()
            }
            DetachedAction::EscapePressed => {
                if v.rename_modal.is_some() {
                    v.rename_modal = None;
                } else if v.gitignore_modal.is_some() {
                    v.gitignore_modal = None;
                } else if v.pending_delete.is_some() {
                    v.pending_delete = None;
                } else if v.explorer_menu.is_some() {
                    v.explorer_menu = None;
                } else if v.status_menu.is_some() {
                    v.status_menu = None;
                } else if v.pending_close_tab.is_some() {
                    v.pending_close_tab = None;
                } else if v.search_confirm_replace_all {
                    v.search_confirm_replace_all = false;
                } else if v.in_file_find_open {
                    v.in_file_find_open = false;
                }
                Task::none()
            }

            // Already handled in `update_detached_viewer`
            DetachedAction::SaveEditor
            | DetachedAction::ConfirmCloseTab { .. }
            | DetachedAction::RevertEditor
            | DetachedAction::SetEditorEol(_)
            | DetachedAction::SaveEditorWithEncoding(_)
            | DetachedAction::ReopenEditorWithEncoding(_)
            | DetachedAction::RefreshExplorer
            | DetachedAction::ToggleExplorerShowIgnored
            | DetachedAction::ToggleExplorerDir(_)
            | DetachedAction::ExplorerFilesLoaded(_, _, _)
            | DetachedAction::OpenExplorerFile(_)
            | DetachedAction::RevealExplorerEntry(_)
            | DetachedAction::ConfirmDeleteExplorerEntry
            | DetachedAction::ExplorerEntryDeleted { .. }
            | DetachedAction::SubmitExplorerRename
            | DetachedAction::ExplorerEntryRenamed { .. }
            | DetachedAction::SubmitExplorerGitignore
            | DetachedAction::SingleFilePicked(Some(_))
            | DetachedAction::ReloadFiles
            | DetachedAction::OpenSearchFile(_)
            | DetachedAction::OpenSearchMatch { .. }
            | DetachedAction::RunSearch
            | DetachedAction::ToggleSearchCase
            | DetachedAction::ToggleSearchWholeWord
            | DetachedAction::ToggleSearchRegex
            | DetachedAction::ToggleSearchGitignore
            | DetachedAction::SearchRootPicked(Some(_))
            | DetachedAction::ReplaceInSearchFile(_)
            | DetachedAction::ReplaceSingleSearchMatch { .. }
            | DetachedAction::ConfirmReplaceAllInSearch
            | DetachedAction::SearchReplaceCompleted(_, _, _)
            | DetachedAction::PayloadLoaded(_, _) => Task::none(),
        }
    }
}
