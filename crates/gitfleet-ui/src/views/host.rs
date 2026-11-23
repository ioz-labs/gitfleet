use crate::app::{
    BottomViewMode, ContextMenuTarget, GitFleetApp, HostLayoutMode, Message, PendingConfirm,
    RIBBON_HEIGHT, ResizeTarget,
};
use crate::icons;
use crate::theme::kit::{
    ButtonSize, ButtonVariant, SegmentItem, UiButton, hairline_h_splitter, hairline_v_splitter,
    kbd_pill, segmented_control,
};
use crate::theme::{self, Palette, ThemeMode, icon_size, type_scale};
use crate::views::{slim_scroll_h, tip, truncate_chars};
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

        let active_repo_name = self
            .current_repo()
            .map(|r| r.name)
            .unwrap_or_else(|| "No repository".into());
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
        let sel_unstaged_paths = self.selected_unstaged_paths();
        let sel_staged_paths = self.selected_staged_paths();
        let sel_discard_paths = self.selected_discard_paths();
        let sel_unstaged_n = sel_unstaged_paths.len();
        let sel_staged_n = sel_staged_paths.len();
        let sel_discard_n = sel_discard_paths.len();

        // ── 1. Unified Command Ribbon (40px — merges Titlebar + Git Toolbar + Smart Next Action) ──
        let ws_pill = tip(
            mouse_area(
                button(
                    row![
                        icons::folder(icon_size::XS, p.accent_fg),
                        text(truncate_chars(&ws.name, 20))
                            .size(type_scale::CAPTION)
                            .font(theme::UI_FONT_MEDIUM)
                            .color(p.text_primary)
                            .wrapping(text::Wrapping::None),
                        icons::chevron_down(11.0, p.text_muted),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([4, 8])
                .style(theme::subtle_btn(p))
                .on_press(Message::ToggleWorkspaceModal),
            )
            .on_right_press(Message::OpenContextMenu(ContextMenuTarget::WorkspacePill)),
            "Workspace Manager (Switch, Rename & Groups) · Right-click for quick actions",
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
                button(
                    row![
                        icons::lock(icon_size::XS, p.warning_fg),
                        text("Claim")
                            .size(type_scale::BADGE)
                            .color(p.warning_fg)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([3, 7])
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

        let branch_pill: Element<'_, Message> = if has_repo {
            tip(
                button(
                    row![
                        icons::branch(icon_size::XS, p.accent_fg),
                        text(truncate_chars(&active_repo_name, 16))
                            .size(type_scale::CAPTION)
                            .font(theme::UI_FONT_MEDIUM)
                            .color(p.text_primary)
                            .wrapping(text::Wrapping::None),
                        text("/")
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                        text(truncate_chars(&active_branch_name, 18))
                            .size(type_scale::CAPTION)
                            .font(theme::CODE_FONT)
                            .color(p.accent_fg)
                            .wrapping(text::Wrapping::None),
                        icons::chevron_down(11.0, p.text_muted),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([4, 8])
                .style(theme::subtle_btn(p))
                .on_press(Message::ToggleBranchPopover),
                "Quick Branch Switcher & Creator (Ctrl+B)",
                tooltip::Position::Bottom,
                p,
            )
        } else {
            Space::with_width(Length::Shrink).into()
        };

        // Smart Next Action 1-Click Workflow CTA
        let smart_cta: Element<'_, Message> = if !has_repo {
            Space::with_width(Length::Shrink).into()
        } else if active_conflicts > 0 {
            tip(
                button(
                    row![
                        icons::alert_triangle(icon_size::XS, Color::WHITE),
                        text(format!("Resolve ({active_conflicts})"))
                            .size(type_scale::CAPTION)
                            .font(theme::UI_FONT_MEDIUM)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([4, 9])
                .style(theme::danger_btn(p))
                .on_press(Message::SetBottomViewMode(BottomViewMode::ResolveConflicts)),
                "Smart Next Action: Open 3-way conflict solver for conflicted files",
                tooltip::Position::Bottom,
                p,
            )
        } else if active_behind > 0 && active_ahead == 0 {
            tip(
                button(
                    row![
                        icons::pull(icon_size::XS, theme::on_accent_ink(p.accent_primary)),
                        text(format!("Pull ({active_behind})"))
                            .size(type_scale::CAPTION)
                            .font(theme::UI_FONT_MEDIUM)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([4, 9])
                .style(theme::primary_btn(p))
                .on_press(Message::PullSelected),
                "Smart Next Action: Pull incoming commits from upstream",
                tooltip::Position::Bottom,
                p,
            )
        } else if active_staged > 0 {
            let has_msg = !self.composed_commit_message().trim().is_empty();
            let commit_msg = if has_msg {
                Message::CommitSubmitted
            } else {
                Message::FocusCommitSummary
            };
            tip(
                button(
                    row![
                        icons::check(icon_size::XS, theme::on_accent_ink(p.accent_primary)),
                        text(format!("Commit ({active_staged})"))
                            .size(type_scale::CAPTION)
                            .font(theme::UI_FONT_MEDIUM)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([4, 9])
                .style(theme::primary_btn(p))
                .on_press(commit_msg),
                if has_msg {
                    "Smart Next Action: Commit staged changes now"
                } else {
                    "Smart Next Action: Focus Commit Summary input"
                },
                tooltip::Position::Bottom,
                p,
            )
        } else if active_unstaged > 0 {
            tip(
                UiButton::new(p, "Stage All")
                    .variant(ButtonVariant::OutlineAccent)
                    .size(ButtonSize::Xs)
                    .badge(active_unstaged.to_string())
                    .icon(icons::stage_all(icon_size::XS, p.accent_fg))
                    .on_press(Message::StageAll),
                "Smart Next Action: Stage modified & untracked files to Index",
                tooltip::Position::Bottom,
                p,
            )
        } else if active_ahead > 0 {
            tip(
                button(
                    row![
                        icons::push(icon_size::XS, theme::on_accent_ink(p.accent_primary)),
                        text(format!("Push ({active_ahead})"))
                            .size(type_scale::CAPTION)
                            .font(theme::UI_FONT_MEDIUM)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([4, 9])
                .style(theme::primary_btn(p))
                .on_press(Message::RequestConfirm(PendingConfirm::push_selected(
                    &active_branch_name,
                    active_ahead,
                ))),
                "Smart Next Action: Push local commits to remote",
                tooltip::Position::Bottom,
                p,
            )
        } else {
            Space::with_width(Length::Shrink).into()
        };

        let tb_btn = |icon: Element<'static, Message>,
                      label: String,
                      label_color: Color,
                      active: bool,
                      msg: Option<Message>,
                      hint: &'static str|
         -> Element<'_, Message> {
            let fg = if msg.is_some() {
                label_color
            } else {
                p.text_muted
            };
            tip(
                button(
                    row![
                        icon,
                        text(label)
                            .size(type_scale::CAPTION)
                            .color(fg)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([4, 7])
                .style(theme::toolbar_btn(p, active))
                .on_press_maybe(msg),
                hint,
                tooltip::Position::Bottom,
                p,
            )
        };

        let tb_sep = || {
            container(
                container(Space::new(1, 16)).style(move |_| container::Style {
                    background: Some(iced::Background::Color(p.border_default)),
                    ..Default::default()
                }),
            )
            .padding([0, 3])
        };

        let pull_label = if active_behind > 0 {
            format!("Pull ({active_behind})")
        } else {
            "Pull".to_string()
        };
        let push_label = if active_ahead > 0 {
            format!("Push ({active_ahead})")
        } else {
            "Push".to_string()
        };
        let undo_label = if undo_cnt > 0 {
            format!("Undo ({undo_cnt})")
        } else {
            "Undo".to_string()
        };

        let git_actions_strip = slim_scroll_h(
            row![
                tb_btn(
                    icons::fetch(icon_size::XS, ic).into(),
                    "Fetch".into(),
                    p.text_primary,
                    false,
                    has_repo.then_some(Message::FetchSelected),
                    "Fetch all remotes & prune",
                ),
                tb_btn(
                    icons::pull(
                        icon_size::XS,
                        if active_behind > 0 { p.accent_fg } else { ic },
                    )
                    .into(),
                    pull_label,
                    if active_behind > 0 {
                        p.accent_fg
                    } else {
                        p.text_primary
                    },
                    false,
                    has_repo.then_some(Message::PullSelected),
                    "Pull fast-forward from remote",
                ),
                tb_btn(
                    icons::push(
                        icon_size::XS,
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
                    has_repo.then(|| {
                        Message::RequestConfirm(PendingConfirm::push_selected(
                            &active_branch_name,
                            active_ahead,
                        ))
                    }),
                    "Push commits to remote…",
                ),
                tb_sep(),
                tb_btn(
                    icons::stage_all(
                        icon_size::XS,
                        if has_unstaged {
                            p.success_fg
                        } else {
                            p.text_muted
                        },
                    )
                    .into(),
                    if sel_unstaged_n > 0 {
                        format!("Stage ({sel_unstaged_n})")
                    } else {
                        "Stage".into()
                    },
                    p.text_primary,
                    false,
                    (has_repo && has_unstaged).then_some(if sel_unstaged_n > 0 {
                        Message::StageSelectedFiles
                    } else {
                        Message::StageAll
                    }),
                    "Stage files to Index (Ctrl+T)",
                ),
                tb_btn(
                    icons::unstage_all(
                        icon_size::XS,
                        if has_staged {
                            p.warning_fg
                        } else {
                            p.text_muted
                        },
                    )
                    .into(),
                    if sel_staged_n > 0 {
                        format!("Unstage ({sel_staged_n})")
                    } else {
                        "Unstage".into()
                    },
                    p.text_primary,
                    false,
                    (has_repo && has_staged).then_some(if sel_staged_n > 0 {
                        Message::UnstageSelectedFiles
                    } else {
                        Message::UnstageAll
                    }),
                    "Unstage files back to Working Tree (Ctrl+Shift+T)",
                ),
                tb_btn(
                    icons::stash(icon_size::XS, if has_changes { ic } else { p.text_muted }).into(),
                    "Stash".into(),
                    p.text_primary,
                    false,
                    has_changes.then(|| {
                        Message::RequestConfirm(PendingConfirm::stash_selected(active_changed))
                    }),
                    "Save Stash (working tree + untracked)…",
                ),
                tb_btn(
                    icons::trash(
                        icon_size::XS,
                        if has_changes {
                            p.danger_fg
                        } else {
                            p.text_muted
                        },
                    )
                    .into(),
                    if sel_discard_n > 0 {
                        format!("Discard ({sel_discard_n})")
                    } else {
                        "Discard".into()
                    },
                    p.text_primary,
                    self.discard_modal.is_some(),
                    has_changes.then(|| {
                        if sel_discard_n > 0 {
                            Message::OpenDiscardModal(Some(sel_discard_paths.clone()))
                        } else {
                            Message::OpenDiscardModal(None)
                        }
                    }),
                    "Discard local or staged changes… (Ctrl+Z)",
                ),
                tb_sep(),
                tb_btn(
                    icons::git_merge(icon_size::XS, ic).into(),
                    "Merge".into(),
                    p.text_primary,
                    self.show_merge_modal,
                    has_repo.then_some(Message::ToggleMergeModal),
                    "Merge / Rebase + Live Conflict Forecast",
                ),
                tb_btn(
                    icons::branch(icon_size::XS, ic).into(),
                    "Git-Flow".into(),
                    p.text_primary,
                    self.show_git_flow_modal,
                    has_repo.then_some(Message::ToggleGitFlowModal),
                    "Git-Flow (Initialize, Start or Finish branches)",
                ),
                tb_btn(
                    icons::undo(
                        icon_size::XS,
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
                    (undo_cnt > 0).then(|| {
                        Message::RequestConfirm(PendingConfirm::undo_last_action(&last_undo_label))
                    }),
                    "Undo last Reset / Revert / Merge / Rebase in this repo…",
                ),
                tb_btn(
                    icons::refresh(icon_size::XS, ic).into(),
                    String::new(),
                    p.text_primary,
                    false,
                    has_repo.then_some(Message::RefreshSelected),
                    "Refresh active repository (F5)",
                ),
            ]
            .spacing(2)
            .align_y(Alignment::Center),
            520.0,
            p,
        );

        let layout_switcher = tip(
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
            ),
            "Switch Layout Mode · Alt+1 Cockpit · Alt+2 Changes · Alt+3 History · Alt+Z Zen Diff",
            tooltip::Position::Bottom,
            p,
        );

        let ribbon = container(
            row![
                ws_pill,
                lock_badge,
                branch_pill,
                smart_cta,
                tb_sep(),
                container(git_actions_strip).width(Length::Fill),
                if self.bulk_cancel.is_some() {
                    Element::from(
                        button(
                            text("Cancel Bulk")
                                .size(type_scale::BADGE)
                                .color(p.danger_fg)
                                .wrapping(text::Wrapping::None),
                        )
                        .padding([3, 8])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::CancelBulkOp),
                    )
                } else {
                    Space::with_width(Length::Shrink).into()
                },
                layout_switcher,
                tip(
                    button(
                        row![
                            icons::command(icon_size::XS, p.text_secondary),
                            kbd_pill(p, "Ctrl+K"),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                    )
                    .padding([3, 6])
                    .style(theme::subtle_btn(p))
                    .on_press(Message::ToggleCommandPalette),
                    "Command Palette (Ctrl+K)",
                    tooltip::Position::Bottom,
                    p,
                ),
                tip(
                    button(icons::external_window(icon_size::SM, p.text_secondary))
                        .padding([4, 6])
                        .style(theme::vscode_icon_btn(p, false))
                        .on_press(Message::OpenDetachedViewer),
                    "Open GitFleet Studio (Edit, Search & Replace, Preview, Diff)",
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
            .spacing(5)
            .align_y(Alignment::Center),
        )
        .padding([0, 8])
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

        let mut root_col = column![
            ribbon,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            main_split,
        ];
        if let Some(dtab) = self.drawer_tab {
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
