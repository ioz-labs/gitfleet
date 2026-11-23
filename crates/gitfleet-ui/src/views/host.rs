use crate::app::{
    BottomViewMode, ContextMenuTarget, DockPosition, GitFleetApp, HostLayoutMode, Message,
    PendingConfirm, RIBBON_HEIGHT, ResizeTarget,
};
use crate::icons;
use crate::theme::kit::{
    SegmentItem, hairline_h_splitter, hairline_v_splitter, kbd_pill, segmented_control,
};
use crate::theme::{self, Palette, ThemeMode, icon_size, type_scale};
use crate::views::{tip, truncate_chars};
use iced::widget::{
    Space, Stack, button, column, container, horizontal_rule, mouse_area, row, text, tooltip,
};
use iced::{Alignment, Color, Element, Length};

fn ratio_portions(ratio: f32) -> (u16, u16) {
    let first = (ratio.clamp(0.18, 0.82) * 1000.0).round() as u16;
    let second = 1000u16.saturating_sub(first).max(100);
    (first.max(100), second)
}

impl GitFleetApp {
    pub(crate) fn view_host(&self, p: Palette) -> Element<'_, Message> {
        let ws = self.service.workspace.current();
        let active_status = self.active_status();
        let ownership = self.service.workspace.ownership();

        let has_repo = self.selected_repo.is_some();
        let ic = if has_repo {
            p.text_primary
        } else {
            p.text_muted
        };

        let undo_entries = self
            .active_repo_key()
            .as_ref()
            .map(|id| self.service.undo.list(id))
            .unwrap_or_default();
        let undo_cnt = undo_entries.len();
        let last_undo_label = undo_entries
            .last()
            .map(|e| e.label.clone())
            .unwrap_or_else(|| "Last Git operation".into());

        let active_branch_name = active_status
            .and_then(|s| s.branch.as_deref())
            .unwrap_or("HEAD")
            .to_string();
        let active_ahead = active_status.map(|s| s.ahead).unwrap_or(0);
        let active_behind = active_status.map(|s| s.behind).unwrap_or(0);
        let active_conflicts = active_status.map(|s| s.conflict_count).unwrap_or(0);
        let active_staged = active_status.map(|s| s.staged_count).unwrap_or(0);
        let active_unstaged = active_status
            .map(|s| s.unstaged_count + s.untracked_count + s.conflict_count)
            .unwrap_or(0);
        let active_changed = active_staged + active_unstaged;
        let has_changes = active_changed > 0;
        let has_unstaged = active_unstaged > 0;
        let has_staged = active_staged > 0;
        let stash_cnt = self.stashes.len();
        let sel_unstaged_paths = self.selected_unstaged_paths();
        let sel_staged_paths = self.selected_staged_paths();
        let sel_discard_paths = self.selected_discard_paths();
        let sel_unstaged_n = sel_unstaged_paths.len();
        let sel_staged_n = sel_staged_paths.len();
        let sel_discard_n = sel_discard_paths.len();

        // ── 1. Spatially-Aligned SmartGit Flight-Deck Ribbon (48px, zero horizontal scroll) ──
        let compact_ribbon = self.window_size.width < 1280.0;
        let btn_pad_h: u16 = if compact_ribbon { 5 } else { 7 };
        let has_sidebar = self.host_layout != HostLayoutMode::ZenDiff;
        let repo_col_w = self
            .sidebar_width
            .clamp(220.0, (self.window_size.width * 0.30).max(240.0));
        let show_ws_text = !has_sidebar || repo_col_w >= 276.0;

        let ws_inner = if show_ws_text {
            row![
                icons::folder(icon_size::XS, p.accent_fg),
                text(truncate_chars(&ws.name, 10))
                    .size(type_scale::CAPTION)
                    .font(theme::UI_FONT_MEDIUM)
                    .color(p.text_primary)
                    .wrapping(text::Wrapping::None),
                icons::chevron_down(10.0, p.text_muted),
            ]
            .spacing(4)
            .align_y(Alignment::Center)
        } else {
            row![
                icons::folder(icon_size::SM, p.accent_fg),
                icons::chevron_down(10.0, p.text_muted),
            ]
            .spacing(3)
            .align_y(Alignment::Center)
        };

        let ws_pill = tip(
            mouse_area(
                button(ws_inner)
                    .padding([5, 7])
                    .style(theme::subtle_btn(p))
                    .on_press(Message::ToggleWorkspaceModal),
            )
            .on_right_press(Message::OpenContextMenu(ContextMenuTarget::WorkspacePill)),
            "Workspace Manager (Switch, Rename & Blueprints) · Right-click for quick actions",
            tooltip::Position::Bottom,
            p,
        );

        let lock_badge: Element<'_, Message> = if !ownership.is_owner {
            let holder_line = match ownership.owner_pid {
                Some(pid) => format!("Workspace lock is held by PID {pid}"),
                None => "Workspace lock is held by another process".to_string(),
            };
            let tip_col = column![
                text(holder_line).size(type_scale::CAPTION).color(p.warning_fg),
                text("Click to claim workspace ownership and enable saving.")
                    .size(type_scale::CAPTION)
                    .color(p.text_secondary),
            ]
            .spacing(3);
            tooltip(
                button(icons::lock(icon_size::XS, p.warning_fg))
                    .padding([5, 6])
                    .style(theme::subtle_btn(p))
                    .on_press(Message::TakeWorkspaceOwnership),
                container(tip_col)
                    .padding([7, 10])
                    .max_width(320.0)
                    .style(theme::tooltip_box(p)),
                tooltip::Position::Bottom,
            )
            .gap(5)
            .into()
        } else {
            Space::with_width(Length::Shrink).into()
        };

        let ribbon_btn = |icon: Element<'static, Message>,
                          label: String,
                          label_color: Color,
                          active: bool,
                          has_dropdown: bool,
                          msg: Option<Message>,
                          right_menu: Option<ContextMenuTarget>,
                          hint: &'static str|
         -> Element<'_, Message> {
            let enabled = msg.is_some();
            let fg = if enabled { label_color } else { p.text_muted };
            let mut top_row = row![icon].spacing(2).align_y(Alignment::Center);
            if has_dropdown {
                top_row = top_row.push(icons::chevron_down(
                    9.0,
                    if enabled { p.text_secondary } else { p.text_muted },
                ));
            }
            let content = column![
                top_row,
                text(label)
                    .size(type_scale::BADGE)
                    .color(fg)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(2)
            .align_x(Alignment::Center);

            let btn_el: Element<'_, Message> = button(content)
                .padding([3, btn_pad_h])
                .style(theme::toolbar_btn(p, active))
                .on_press_maybe(msg)
                .into();

            let interactive: Element<'_, Message> = if let Some(target) = right_menu.filter(|_| has_repo)
            {
                mouse_area(btn_el)
                    .on_right_press(Message::OpenContextMenu(target))
                    .into()
            } else {
                btn_el
            };

            tip(interactive, hint, tooltip::Position::Bottom, p)
        };

        let tb_sep = || {
            container(
                container(Space::new(1, 24)).style(move |_| container::Style {
                    background: Some(iced::Background::Color(p.border_default)),
                    ..Default::default()
                }),
            )
            .padding([0, 3])
        };

        // ── Column 1 (above Repositories pane): Workspace + Remote Sync (Pull, Sync, Push) ──
        let pull_label = if active_behind > 0 {
            format!("Pull·{active_behind}")
        } else {
            "Pull".to_string()
        };
        let push_label = if active_ahead > 0 {
            format!("Push·{active_ahead}")
        } else {
            "Push".to_string()
        };

        let repo_col_row = row![
            ws_pill,
            lock_badge,
            Space::with_width(4.0),
            ribbon_btn(
                icons::pull(
                    icon_size::SM,
                    if active_behind > 0 { p.accent_fg } else { ic },
                )
                .into(),
                pull_label,
                if active_behind > 0 {
                    p.accent_fg
                } else {
                    p.text_primary
                },
                self.pull_sync_modal
                    .as_ref()
                    .is_some_and(|m| !m.push_after),
                true,
                has_repo.then_some(Message::SmartPullClicked),
                Some(ContextMenuTarget::RibbonPull),
                "Pull from remote (auto-opens Auto-Stash / Strategy dialog if working tree has local changes) · Right-click for Fetch & options",
            ),
            ribbon_btn(
                icons::sync_arrows(
                    icon_size::SM,
                    if active_behind > 0 || active_ahead > 0 {
                        p.accent_fg
                    } else {
                        ic
                    },
                )
                .into(),
                "Sync".into(),
                p.text_primary,
                self.pull_sync_modal
                    .as_ref()
                    .is_some_and(|m| m.push_after),
                true,
                has_repo.then_some(Message::OpenPullSyncModal { push_after: true }),
                Some(ContextMenuTarget::RibbonPull),
                "Sync repository (Pull with Rebase/Merge + Auto-Stash + Push)… · Right-click for Fetch",
            ),
            ribbon_btn(
                icons::push(
                    icon_size::SM,
                    if active_ahead > 0 { p.success_fg } else { ic },
                )
                .into(),
                push_label,
                if active_ahead > 0 {
                    p.success_fg
                } else {
                    p.text_primary
                },
                false,
                false,
                has_repo.then(|| {
                    Message::RequestConfirm(PendingConfirm::push_selected(
                        &active_branch_name,
                        active_ahead,
                    ))
                }),
                None,
                "Push local commits to remote…",
            ),
            Space::with_width(if has_sidebar {
                Length::Fill
            } else {
                Length::Shrink
            }),
        ]
        .spacing(2)
        .align_y(Alignment::Center);

        let repo_col_slot = container(repo_col_row)
            .padding([0, 6])
            .width(if has_sidebar {
                Length::Fixed(repo_col_w)
            } else {
                Length::Shrink
            })
            .height(Length::Fill)
            .align_y(Alignment::Center);

        let col_divider = container(
            container(Space::new(1, 28)).style(move |_| container::Style {
                background: Some(iced::Background::Color(p.border_default)),
                ..Default::default()
            }),
        )
        .padding([0, 3])
        .align_y(Alignment::Center);

        // ── Column 2 (above Graph / Working Tree pane): Stage, Unstage, Discard | Save Stash, Apply Stash ──
        let conflict_pill: Element<'_, Message> = if has_repo && active_conflicts > 0 {
            tip(
                button(
                    column![
                        icons::alert_triangle(icon_size::SM, Color::WHITE),
                        text(format!("Resolve·{active_conflicts}"))
                            .size(type_scale::BADGE)
                            .color(Color::WHITE)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(2)
                    .align_x(Alignment::Center),
                )
                .padding([3, 8])
                .style(theme::danger_btn(p))
                .on_press(Message::SetBottomViewMode(BottomViewMode::ResolveConflicts)),
                "Open 3-Way Interactive Conflict Solver for conflicted files",
                tooltip::Position::Bottom,
                p,
            )
        } else {
            Space::with_width(Length::Shrink).into()
        };

        let graph_col_group = row![
            ribbon_btn(
                icons::stage_all(
                    icon_size::SM,
                    if has_unstaged {
                        p.success_fg
                    } else {
                        p.text_muted
                    },
                )
                .into(),
                if sel_unstaged_n > 0 {
                    format!("Stage·{sel_unstaged_n}")
                } else {
                    "Stage".into()
                },
                p.text_primary,
                false,
                false,
                (has_repo && has_unstaged).then_some(if sel_unstaged_n > 0 {
                    Message::StageSelectedFiles
                } else {
                    Message::StageAll
                }),
                None,
                "Stage files to Index (Ctrl+T)",
            ),
            ribbon_btn(
                icons::unstage_all(
                    icon_size::SM,
                    if has_staged {
                        p.warning_fg
                    } else {
                        p.text_muted
                    },
                )
                .into(),
                if sel_staged_n > 0 {
                    format!("Unstage·{sel_staged_n}")
                } else {
                    "Unstage".into()
                },
                p.text_primary,
                false,
                false,
                (has_repo && has_staged).then_some(if sel_staged_n > 0 {
                    Message::UnstageSelectedFiles
                } else {
                    Message::UnstageAll
                }),
                None,
                "Unstage files back to Working Tree (Ctrl+Shift+T)",
            ),
            ribbon_btn(
                icons::trash(
                    icon_size::SM,
                    if has_changes {
                        p.danger_fg
                    } else {
                        p.text_muted
                    },
                )
                .into(),
                if sel_discard_n > 0 {
                    format!("Discard·{sel_discard_n}")
                } else {
                    "Discard".into()
                },
                p.text_primary,
                self.discard_modal.is_some(),
                false,
                has_changes.then(|| {
                    if sel_discard_n > 0 {
                        Message::OpenDiscardModal(Some(sel_discard_paths.clone()))
                    } else {
                        Message::OpenDiscardModal(None)
                    }
                }),
                None,
                "Discard local or staged changes… (Ctrl+Z)",
            ),
            tb_sep(),
            ribbon_btn(
                icons::stash_save(
                    icon_size::SM,
                    if has_changes { p.accent_fg } else { p.text_muted },
                )
                .into(),
                "Save Stash".into(),
                p.text_primary,
                self.save_stash_modal.is_some(),
                true,
                (has_repo && has_changes).then_some(Message::OpenSaveStashModal),
                Some(ContextMenuTarget::RibbonStash),
                "Save Stash (custom message, untracked -u, --keep-index)… · Right-click for Quick Stash",
            ),
            ribbon_btn(
                icons::stash_apply(
                    icon_size::SM,
                    if stash_cnt > 0 {
                        p.success_fg
                    } else {
                        p.text_muted
                    },
                )
                .into(),
                if stash_cnt > 0 {
                    format!("Apply·{stash_cnt}")
                } else {
                    "Apply Stash".into()
                },
                p.text_primary,
                self.apply_stash_modal.is_some(),
                false,
                (has_repo && stash_cnt > 0).then_some(Message::OpenApplyStashModal),
                Some(ContextMenuTarget::RibbonStash),
                "Apply or Pop Stash back into working tree… · Right-click to Pop stash@{0} immediately",
            ),
            conflict_pill,
        ]
        .spacing(2)
        .align_y(Alignment::Center);

        // ── Column 3 (Center-Right Inspection): Blame, Studio, Undo ──
        let has_active_file = self.active_file_path.is_some();
        let blame_active = self.bottom_mode == BottomViewMode::Blame;
        let undo_label = if undo_cnt > 0 {
            format!("Undo·{undo_cnt}")
        } else {
            "Undo".to_string()
        };

        let inspect_group = row![
            ribbon_btn(
                icons::git_blame(
                    icon_size::SM,
                    if blame_active {
                        p.accent_fg
                    } else if has_active_file {
                        ic
                    } else {
                        p.text_muted
                    },
                )
                .into(),
                "Blame".into(),
                if blame_active {
                    p.accent_fg
                } else {
                    p.text_primary
                },
                blame_active,
                false,
                (has_repo && has_active_file).then_some(if blame_active {
                    Message::SetBottomViewMode(self.last_text_diff_mode)
                } else {
                    Message::SetBottomViewMode(BottomViewMode::Blame)
                }),
                None,
                "Inspect line-by-line Git Blame & commit authorship for the selected file",
            ),
            ribbon_btn(
                icons::external_window(icon_size::SM, ic).into(),
                "Studio".into(),
                p.text_primary,
                !self.detached_viewers.is_empty(),
                false,
                Some(Message::OpenDetachedViewer),
                None,
                "Open GitFleet Studio (Multi-tab Code Editor, Search & Replace, Split Live Preview)",
            ),
            ribbon_btn(
                icons::undo(
                    icon_size::SM,
                    if undo_cnt > 0 {
                        p.warning_fg
                    } else {
                        p.text_muted
                    },
                )
                .into(),
                undo_label,
                if undo_cnt > 0 {
                    p.warning_fg
                } else {
                    p.text_muted
                },
                false,
                false,
                (undo_cnt > 0).then(|| {
                    Message::RequestConfirm(PendingConfirm::undo_last_action(&last_undo_label))
                }),
                None,
                "Undo last Reset / Revert / Merge / Rebase in this repository…",
            ),
        ]
        .spacing(2)
        .align_y(Alignment::Center);

        // ── Column 4 (above Commit / Files pane): Git-Flow, Merge, Rebase + Layout & App Controls ──
        let layout_switcher = tip(
            if compact_ribbon {
                segmented_control(
                    p,
                    [
                        SegmentItem::new(HostLayoutMode::Cockpit, "1"),
                        SegmentItem::new(HostLayoutMode::Changes, "2"),
                        SegmentItem::new(HostLayoutMode::History, "3"),
                        SegmentItem::new(HostLayoutMode::ZenDiff, "Z"),
                    ],
                    self.host_layout,
                    Message::SetHostLayout,
                )
            } else {
                segmented_control(
                    p,
                    [
                        SegmentItem::new(HostLayoutMode::Cockpit, "Cockpit"),
                        SegmentItem::new(HostLayoutMode::Changes, "Changes"),
                        SegmentItem::new(HostLayoutMode::History, "History"),
                        SegmentItem::new(HostLayoutMode::ZenDiff, "Zen"),
                    ],
                    self.host_layout,
                    Message::SetHostLayout,
                )
            },
            "Switch Layout Mode · Alt+1 Cockpit · Alt+2 Changes · Alt+3 History · Alt+Z Zen Diff",
            tooltip::Position::Bottom,
            p,
        );

        let right_col_group = row![
            ribbon_btn(
                icons::branch(icon_size::SM, ic).into(),
                "Git-Flow".into(),
                p.text_primary,
                self.show_git_flow_modal,
                false,
                has_repo.then_some(Message::ToggleGitFlowModal),
                None,
                "Git-Flow (Initialize, Start or Finish Feature / Release / Hotfix branches)",
            ),
            ribbon_btn(
                icons::git_merge(icon_size::SM, ic).into(),
                "Merge".into(),
                p.text_primary,
                self.show_merge_modal,
                false,
                has_repo.then_some(Message::ToggleMergeModal),
                None,
                "Merge / Rebase + Live Conflict Forecast (Ctrl+M)",
            ),
            ribbon_btn(
                icons::git_rebase(icon_size::SM, p.violet_fg).into(),
                "Rebase".into(),
                p.text_primary,
                self.rebase_modal.is_some(),
                true,
                has_repo.then_some(Message::RibbonRebaseClicked),
                Some(ContextMenuTarget::RibbonRebase),
                "Interactive Rebase (Squash, Reword, Fixup, Reorder, Drop)… · Right-click for Branch Rebase",
            ),
            tb_sep(),
            if self.bulk_cancel.is_some() {
                Element::from(
                    button(
                        text("Cancel Bulk")
                            .size(type_scale::BADGE)
                            .color(p.danger_fg)
                            .wrapping(text::Wrapping::None),
                    )
                    .padding([4, 8])
                    .style(theme::subtle_btn(p))
                    .on_press(Message::CancelBulkOp),
                )
            } else {
                Space::with_width(Length::Shrink).into()
            },
            layout_switcher,
            tip(
                button(if compact_ribbon {
                    Element::from(icons::command(icon_size::SM, p.text_secondary))
                } else {
                    row![
                        icons::command(icon_size::XS, p.text_secondary),
                        kbd_pill(p, "Ctrl+K"),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center)
                    .into()
                })
                .padding([4, 6])
                .style(theme::subtle_btn(p))
                .on_press(Message::ToggleCommandPalette),
                "Command Palette (Ctrl+K)",
                tooltip::Position::Bottom,
                p,
            ),
            tip(
                button(icons::refresh(icon_size::SM, p.text_secondary))
                    .padding([4, 6])
                    .style(theme::vscode_icon_btn(p, false))
                    .on_press_maybe(has_repo.then_some(Message::RefreshSelected)),
                "Refresh active repository (F5)",
                tooltip::Position::Bottom,
                p,
            ),
            tip(
                button(icons::settings(icon_size::SM, p.text_secondary))
                    .padding([4, 6])
                    .style(theme::vscode_icon_btn(p, false))
                    .on_press(Message::ToggleSettingsModal),
                "Settings, 5 Theme Presets, Terminal Profiles & Diagnostics",
                tooltip::Position::Bottom,
                p,
            ),
            tip(
                button(match self.theme_mode {
                    ThemeMode::Dark => icons::sun(icon_size::SM, p.warning_fg),
                    ThemeMode::Light => icons::moon(icon_size::SM, p.accent_fg),
                })
                .padding([4, 6])
                .style(theme::vscode_icon_btn(p, false))
                .on_press(Message::ToggleTheme),
                "Toggle Dark / Light Polarity",
                tooltip::Position::Bottom,
                p,
            ),
        ]
        .spacing(4)
        .align_y(Alignment::Center);

        let ribbon = container(
            row![
                repo_col_slot,
                col_divider,
                Space::with_width(4.0),
                graph_col_group,
                Space::with_width(Length::Fill),
                inspect_group,
                Space::with_width(Length::Fill),
                right_col_group,
                Space::with_width(6.0),
            ]
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fixed(RIBBON_HEIGHT))
        .align_y(Alignment::Center)
        .style(theme::titlebar_box(p));

        // ── 2. Edge-to-Edge 1px Hairline Split Panes across 4 Layout Modes ──────────────────
        let h_splitter = |target: ResizeTarget| -> Element<'_, Message> {
            let active = self.active_resize == Some(target);
            hairline_h_splitter(p, active, Message::StartResize(target))
        };

        let v_splitter = |target: ResizeTarget| -> Element<'_, Message> {
            let active = self.active_resize == Some(target);
            hairline_v_splitter(p, active, Message::StartResize(target))
        };

        let main_split: Element<'_, Message> = match self.host_layout {
            HostLayoutMode::ZenDiff => container(self.view_bottom_viewer(p))
                .width(Length::Fill)
                .height(Length::Fill)
                .padding([6, 8])
                .style(theme::panel_box(p))
                .into(),

            HostLayoutMode::Changes => {
                let (left_top_p, left_bot_p) = ratio_portions(self.left_split_ratio);
                let left_column = column![
                    container(self.view_repositories_pane(p, &ws.repos))
                        .height(Length::FillPortion(left_top_p))
                        .width(Length::Fill)
                        .style(theme::sidebar_box(p)),
                    h_splitter(ResizeTarget::LeftSplit),
                    container(self.view_branches_pane(p))
                        .height(Length::FillPortion(left_bot_p))
                        .width(Length::Fill)
                        .style(theme::sidebar_box(p)),
                ]
                .width(Length::Fixed(self.sidebar_width))
                .height(Length::Fill);

                let (commit_p, files_p) = ratio_portions(self.commit_files_split_ratio);
                let (mid_p, diff_p) = ratio_portions(self.top_right_split_ratio.clamp(0.24, 0.52));

                let changes_mid = column![
                    container(self.view_commit_pane(p))
                        .width(Length::Fill)
                        .height(Length::FillPortion(commit_p))
                        .padding(8)
                        .style(theme::panel_box(p)),
                    h_splitter(ResizeTarget::CommitFilesSplit),
                    container(self.view_files_pane(p))
                        .width(Length::Fill)
                        .height(Length::FillPortion(files_p))
                        .padding(8)
                        .style(theme::panel_box(p)),
                ]
                .width(Length::FillPortion(mid_p))
                .height(Length::Fill);

                let full_diff = container(self.view_bottom_viewer(p))
                    .width(Length::FillPortion(diff_p))
                    .height(Length::Fill)
                    .padding(8)
                    .style(theme::panel_box(p));

                row![
                    left_column,
                    v_splitter(ResizeTarget::SidebarWidth),
                    changes_mid,
                    v_splitter(ResizeTarget::TopRightSplit),
                    full_diff,
                ]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            }

            HostLayoutMode::History => {
                let (left_top_p, left_bot_p) = ratio_portions(self.left_split_ratio);
                let left_column = column![
                    container(self.view_repositories_pane(p, &ws.repos))
                        .height(Length::FillPortion(left_top_p))
                        .width(Length::Fill)
                        .style(theme::sidebar_box(p)),
                    h_splitter(ResizeTarget::LeftSplit),
                    container(self.view_branches_pane(p))
                        .height(Length::FillPortion(left_bot_p))
                        .width(Length::Fill)
                        .style(theme::sidebar_box(p)),
                ]
                .width(Length::Fixed(self.sidebar_width))
                .height(Length::Fill);

                let (right_top_p, right_bot_p) = ratio_portions(self.right_split_ratio);
                let (files_p, diff_p) = ratio_portions(self.top_right_split_ratio.clamp(0.24, 0.50));

                let bottom_inspect = row![
                    container(self.view_files_pane(p))
                        .width(Length::FillPortion(files_p))
                        .height(Length::Fill)
                        .padding(8)
                        .style(theme::panel_box(p)),
                    v_splitter(ResizeTarget::TopRightSplit),
                    container(self.view_bottom_viewer(p))
                        .width(Length::FillPortion(diff_p))
                        .height(Length::Fill)
                        .padding(8)
                        .style(theme::panel_box(p)),
                ]
                .height(Length::FillPortion(right_bot_p))
                .width(Length::Fill);

                let history_right = column![
                    container(self.view_graph_pane(p))
                        .width(Length::Fill)
                        .height(Length::FillPortion(right_top_p))
                        .padding(8)
                        .style(theme::panel_box(p)),
                    h_splitter(ResizeTarget::RightSplit),
                    bottom_inspect,
                ]
                .width(Length::Fill)
                .height(Length::Fill);

                row![
                    left_column,
                    v_splitter(ResizeTarget::SidebarWidth),
                    history_right,
                ]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            }

            HostLayoutMode::Cockpit => {
                let (left_top_p, left_bot_p) = ratio_portions(self.left_split_ratio);
                let left_column = column![
                    container(self.view_repositories_pane(p, &ws.repos))
                        .height(Length::FillPortion(left_top_p))
                        .width(Length::Fill)
                        .style(theme::sidebar_box(p)),
                    h_splitter(ResizeTarget::LeftSplit),
                    container(self.view_branches_pane(p))
                        .height(Length::FillPortion(left_bot_p))
                        .width(Length::Fill)
                        .style(theme::sidebar_box(p)),
                ]
                .width(Length::Fixed(self.sidebar_width))
                .height(Length::Fill);

                let (graph_p, right_col_p) = ratio_portions(self.top_right_split_ratio);
                let (right_top_p, right_bot_p) = ratio_portions(self.right_split_ratio);
                let (commit_p, files_p) = ratio_portions(self.commit_files_split_ratio);

                let commit_and_files = column![
                    container(self.view_commit_pane(p))
                        .width(Length::Fill)
                        .height(Length::FillPortion(commit_p))
                        .padding(8)
                        .style(theme::panel_box(p)),
                    h_splitter(ResizeTarget::CommitFilesSplit),
                    container(self.view_files_pane(p))
                        .width(Length::Fill)
                        .height(Length::FillPortion(files_p))
                        .padding(8)
                        .style(theme::panel_box(p)),
                ]
                .width(Length::FillPortion(right_col_p))
                .height(Length::Fill);

                let top_right = row![
                    container(self.view_graph_pane(p))
                        .width(Length::FillPortion(graph_p))
                        .height(Length::Fill)
                        .padding(8)
                        .style(theme::panel_box(p)),
                    v_splitter(ResizeTarget::TopRightSplit),
                    commit_and_files,
                ]
                .height(Length::FillPortion(right_top_p));

                let bottom_right = container(self.view_bottom_viewer(p))
                    .width(Length::Fill)
                    .height(Length::FillPortion(right_bot_p))
                    .padding(8)
                    .style(theme::panel_box(p));

                let right_workspace = column![
                    top_right,
                    h_splitter(ResizeTarget::RightSplit),
                    bottom_right,
                ]
                .width(Length::Fill)
                .height(Length::Fill);

                row![
                    left_column,
                    v_splitter(ResizeTarget::SidebarWidth),
                    right_workspace,
                ]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            }
        };

        let center_body: Element<'_, Message> = match (self.drawer_tab, self.dock_position) {
            (Some(dtab), DockPosition::Right) => row![
                container(main_split).width(Length::Fill).height(Length::Fill),
                self.view_auxiliary_drawer(p, dtab),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            _ => main_split,
        };

        let mut root_col = column![
            ribbon,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            center_body,
        ];
        if let (Some(dtab), DockPosition::Bottom) = (self.drawer_tab, self.dock_position) {
            root_col = root_col.push(self.view_auxiliary_drawer(p, dtab));
        }
        root_col = root_col
            .push(horizontal_rule(1).style(theme::subtle_rule(p)))
            .push(self.view_status_bar(p, &ws.name, ws.repos.len()));

        let base: Element<'_, Message> = container(root_col)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::app_bg(p))
            .into();

        // Keep `base` permanently at child index 0 of a single stable root `Stack` wrapped in a
        // stable `inert_when` guard so opening/closing popups never resets scrollbars.
        let has_overlay = self.has_modal_or_menu_open();
        let mut layers: Vec<Element<'_, Message>> = vec![theme::inert_when(base, has_overlay)];
        let mut modal_layers: Vec<Element<'_, Message>> = Vec::new();

        if self.show_palette {
            modal_layers.push(self.view_command_palette(p, &ws.repos));
        } else if self.show_branch_popover {
            modal_layers.push(self.view_branch_popover(p));
        } else if self.show_merge_modal {
            modal_layers.push(self.view_merge_modal(p));
        } else if self.show_git_flow_modal {
            modal_layers.push(self.view_git_flow_modal(p));
        } else if self.show_workspace_modal {
            modal_layers.push(self.view_workspace_modal(p));
        } else if self.show_settings_modal {
            modal_layers.push(self.view_settings_modal(p));
        } else if self.show_eol_modal {
            modal_layers.push(self.view_eol_modal(p));
        } else if self.show_encoding_modal {
            modal_layers.push(self.view_encoding_modal(p));
        } else if self.show_syntax_modal {
            modal_layers.push(self.view_syntax_modal(p));
        }

        if self.show_terminal_menu {
            modal_layers.push(self.view_terminal_profile_menu(p));
        }

        if let Some(menu) = &self.context_menu {
            modal_layers.push(self.view_context_menu(p, menu));
        }

        if let Some(modal) = &self.cherry_pick_modal {
            modal_layers.push(self.view_cherry_pick_modal(p, modal));
        }

        if let Some(modal) = &self.rebase_modal {
            modal_layers.push(self.view_interactive_rebase_modal(p, modal));
        }

        if let Some(modal) = &self.save_stash_modal {
            modal_layers.push(self.view_save_stash_modal(p, modal));
        }

        if let Some(modal) = &self.apply_stash_modal {
            modal_layers.push(self.view_apply_stash_modal(p, modal));
        }

        if let Some(modal) = &self.pull_sync_modal {
            modal_layers.push(self.view_pull_sync_modal(p, modal));
        }

        if let Some(modal) = &self.discard_modal {
            modal_layers.push(self.view_discard_modal(p, modal));
        }

        if let Some(modal) = &self.rename_branch_modal {
            modal_layers.push(self.view_rename_branch_modal(p, modal));
        }

        if let Some(modal) = &self.rename_workspace_modal {
            modal_layers.push(self.view_rename_workspace_modal(p, modal));
        }

        if let Some(modal) = &self.rename_group_modal {
            modal_layers.push(self.view_rename_group_modal(p, modal));
        }

        if let Some(modal) = &self.rename_entry_modal {
            modal_layers.push(self.view_rename_entry_modal(p, modal));
        }

        if let Some(modal) = &self.add_gitignore_modal {
            modal_layers.push(self.view_add_gitignore_modal(p, modal));
        }

        if let Some(modal) = &self.commit_modal {
            modal_layers.push(self.view_commit_modal(p, modal));
        }

        if let Some(modal) = &self.scan_modal {
            modal_layers.push(self.view_scan_modal(p, modal));
        }

        if let Some(modal) = &self.blueprint_export_modal {
            modal_layers.push(self.view_blueprint_export_modal(p, modal));
        }

        if let Some(modal) = &self.blueprint_import_modal {
            modal_layers.push(self.view_blueprint_import_modal(p, modal));
        }

        if let Some(modal) = &self.add_repo_modal {
            modal_layers.push(self.view_add_repo_modal(p, modal));
        }

        if self.show_clone_modal {
            modal_layers.push(self.view_clone_modal(p));
        }

        if self.show_update_modal {
            modal_layers.push(self.view_update_modal(p));
        }

        if let Some(confirm) = &self.pending_confirm {
            modal_layers.push(self.view_confirm_modal(p, confirm));
        }

        if let Some(err) = &self.action_error {
            modal_layers.push(self.view_error_modal(p, err));
        }

        let modal_count = modal_layers.len();
        for (idx, layer) in modal_layers.into_iter().enumerate() {
            let occluded = idx + 1 < modal_count;
            layers.push(theme::inert_when(layer, occluded));
        }

        if let Some((_, repo_name)) = &self.dragging_repo {
            let dest_label = match self.drag_hover_group.as_ref().map(|(_, g)| g) {
                Some(Some(g)) => format!("-> {g}"),
                Some(None) => "-> Root (no group)".to_string(),
                None => "Drop on a group".to_string(),
            };
            let ghost = container(
                row![
                    icons::folder(12.0, p.accent_fg),
                    text(repo_name.clone())
                        .size(type_scale::CAPTION)
                        .color(p.text_primary),
                    text(dest_label)
                        .size(type_scale::BADGE)
                        .color(if self.drag_hover_group.is_some() {
                            p.success_fg
                        } else {
                            p.text_muted
                        }),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([5, 10])
            .style(theme::tooltip_box(p));
            layers.push(theme::floating_ghost(self.cursor_pos, ghost));
        }

        Stack::with_children(layers)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
