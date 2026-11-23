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

        // ── 1. 3-Zone Spatially-Aligned Ribbon (48px, true split dropdowns, zero scroll) ──
        let compact_ribbon = self.window_size.width < 1340.0;
        let btn_pad_h: u16 = if compact_ribbon { 5 } else { 7 };
        let split_pad_h: u16 = if compact_ribbon { 4 } else { 6 };
        let has_sidebar = self.host_layout != HostLayoutMode::ZenDiff;
        let repo_col_w = self
            .sidebar_width
            .clamp(236.0, (self.window_size.width * 0.30).max(248.0));
        let show_ws_text = !has_sidebar || repo_col_w >= 288.0;
        let has_upstream = active_status.is_some_and(|s| s.upstream.is_some());
        let is_unpublished_local =
            has_repo && active_status.is_some_and(|s| !s.detached && s.upstream.is_none());

        let ws_inner = if show_ws_text {
            row![
                icons::layers(icon_size::XS, p.accent_fg),
                text("WS:")
                    .size(type_scale::BADGE)
                    .color(p.text_muted)
                    .wrapping(text::Wrapping::None),
                text(truncate_chars(&ws.name, 8))
                    .size(type_scale::CAPTION)
                    .font(theme::UI_FONT_MEDIUM)
                    .color(p.text_primary)
                    .wrapping(text::Wrapping::None),
                icons::chevron_down(10.0, p.text_muted),
            ]
            .spacing(3)
            .align_y(Alignment::Center)
        } else {
            row![
                icons::layers(icon_size::SM, p.accent_fg),
                icons::chevron_down(10.0, p.text_muted),
            ]
            .spacing(3)
            .align_y(Alignment::Center)
        };

        let ws_pill = tip(
            mouse_area(
                button(ws_inner)
                    .padding([5, 6])
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
                    .padding([5, 5])
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

        // Single-action vertical button: centered icon over centered label (NO fake chevron).
        let ribbon_plain_btn = |icon: Element<'static, Message>,
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
            let content = column![
                icon,
                text(label)
                    .size(type_scale::BADGE)
                    .color(fg)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(2)
            .align_x(Alignment::Center);

            tip(
                button(content)
                    .padding([3, btn_pad_h])
                    .style(theme::toolbar_btn(p, active))
                    .on_press_maybe(msg),
                hint,
                tooltip::Position::Bottom,
                p,
            )
        };

        // True two-part Split Button:
        // - Left button (centered icon + label): executes primary 1-click action immediately.
        // - Right caret (`▾`) button: opens anchored left-click dropdown menu with extra options.
        let ribbon_split_btn = |icon: Element<'static, Message>,
                                label: String,
                                label_color: Color,
                                active: bool,
                                main_msg: Option<Message>,
                                dropdown_enabled: bool,
                                dropdown_target: ContextMenuTarget,
                                main_hint: &'static str,
                                menu_hint: &'static str|
         -> Element<'_, Message> {
            let main_enabled = main_msg.is_some();
            let fg = if main_enabled {
                label_color
            } else {
                p.text_muted
            };
            let menu_open = self
                .context_menu
                .as_ref()
                .is_some_and(|m| m.target.is_same_ribbon_dropdown(&dropdown_target));

            let main_content = column![
                icon,
                text(label)
                    .size(type_scale::BADGE)
                    .color(fg)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(2)
            .align_x(Alignment::Center);

            let main_btn_raw: Element<'_, Message> = button(main_content)
                .padding([3, split_pad_h])
                .style(theme::toolbar_btn(p, active || menu_open))
                .on_press_maybe(main_msg)
                .into();

            let main_with_rclick: Element<'_, Message> = if dropdown_enabled {
                let rtarget = dropdown_target.clone();
                mouse_area(main_btn_raw)
                    .on_right_press(Message::ToggleRibbonDropdown(rtarget))
                    .into()
            } else {
                main_btn_raw
            };

            let caret_btn = tip(
                button(
                    container(icons::chevron_down(
                        10.0,
                        if dropdown_enabled {
                            if menu_open {
                                p.accent_fg
                            } else {
                                p.text_secondary
                            }
                        } else {
                            p.text_muted
                        },
                    ))
                    .height(Length::Fixed(32.0))
                    .align_y(Alignment::Center),
                )
                .padding([2, 3])
                .style(theme::toolbar_btn(p, menu_open))
                .on_press_maybe(
                    dropdown_enabled.then_some(Message::ToggleRibbonDropdown(dropdown_target)),
                ),
                menu_hint,
                tooltip::Position::Bottom,
                p,
            );

            row![
                tip(main_with_rclick, main_hint, tooltip::Position::Bottom, p),
                caret_btn,
            ]
            .spacing(0)
            .align_y(Alignment::Center)
            .into()
        };

        let tb_sep = || {
            container(
                container(Space::new(1, 24)).style(move |_| container::Style {
                    background: Some(iced::Background::Color(p.border_default)),
                    ..Default::default()
                }),
            )
            .padding([0, 4])
        };

        // ── Zone 1 (above Repositories pane): Workspace + Remote Split Actions (Pull ▾, Sync ▾, Push ▾) ──
        let pull_label = if active_behind > 0 {
            format!("Pull·{active_behind}")
        } else {
            "Pull".to_string()
        };
        let push_label = if is_unpublished_local {
            "Publish".to_string()
        } else if active_ahead > 0 {
            format!("Push·{active_ahead}")
        } else {
            "Push".to_string()
        };

        let repo_col_row = row![
            ws_pill,
            lock_badge,
            Space::with_width(3.0),
            ribbon_split_btn(
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
                has_repo.then_some(Message::SmartPullClicked),
                has_repo,
                ContextMenuTarget::RibbonPull,
                "Pull incoming commits (auto-opens Auto-Stash / Strategy dialog if working tree is dirty)",
                "Pull & Fetch options (Fetch, Fetch All, Rebase + Auto-Stash, Pull More…)",
            ),
            ribbon_split_btn(
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
                has_repo.then_some(Message::SmartSyncClicked),
                has_repo,
                ContextMenuTarget::RibbonSync,
                "Sync repository using active direction (Pull, then Push / Push, then Pull)",
                "Choose Sync order (Pull, then Push · Push, then Pull · Sync More…)",
            ),
            ribbon_split_btn(
                icons::push(
                    icon_size::SM,
                    if is_unpublished_local {
                        p.accent_fg
                    } else if active_ahead > 0 {
                        p.success_fg
                    } else {
                        ic
                    },
                )
                .into(),
                push_label,
                if is_unpublished_local {
                    p.accent_fg
                } else if active_ahead > 0 {
                    p.success_fg
                } else {
                    p.text_primary
                },
                false,
                has_repo.then(|| {
                    if !has_upstream {
                        Message::RequestConfirm(PendingConfirm::publish_branch(&active_branch_name))
                    } else {
                        Message::RequestConfirm(PendingConfirm::push_selected(
                            &active_branch_name,
                            active_ahead,
                        ))
                    }
                }),
                has_repo,
                ContextMenuTarget::RibbonPush,
                "Push local commits to remote (or publish local branch to origin)",
                "Push options (Publish Branch, Push All Tags, Force Push --force-with-lease…)",
            ),
            Space::with_width(if has_sidebar {
                Length::Fill
            } else {
                Length::Shrink
            }),
        ]
        .spacing(1)
        .align_y(Alignment::Center);

        let repo_col_slot = container(repo_col_row)
            .padding([0, 5])
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

        // ── Zone 2 (above Graph): Working Tree -> Stash -> Branch/Commit History -> Undo ──
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

        let undo_label = if undo_cnt > 0 {
            format!("Undo·{undo_cnt}")
        } else {
            "Undo".to_string()
        };

        let git_workflow_zone = row![
            ribbon_plain_btn(
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
                (has_repo && has_unstaged).then_some(if sel_unstaged_n > 0 {
                    Message::StageSelectedFiles
                } else {
                    Message::StageAll
                }),
                "Stage files to Index (Ctrl+T)",
            ),
            ribbon_plain_btn(
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
                (has_repo && has_staged).then_some(if sel_staged_n > 0 {
                    Message::UnstageSelectedFiles
                } else {
                    Message::UnstageAll
                }),
                "Unstage files back to Working Tree (Ctrl+Shift+T)",
            ),
            ribbon_plain_btn(
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
            ribbon_split_btn(
                icons::stash_save(
                    icon_size::SM,
                    if has_changes { p.accent_fg } else { p.text_muted },
                )
                .into(),
                "Save Stash".into(),
                p.text_primary,
                self.save_stash_modal.is_some(),
                (has_repo && has_changes).then_some(Message::OpenSaveStashModal),
                has_repo && has_changes,
                ContextMenuTarget::RibbonSaveStash,
                "Save Stash with custom message & options…",
                "Stash options (Quick Stash All, Keep Staged --keep-index, Stash Selected Files)",
            ),
            ribbon_split_btn(
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
                    "Apply".into()
                },
                p.text_primary,
                self.apply_stash_modal.is_some(),
                (has_repo && stash_cnt > 0).then_some(Message::OpenApplyStashModal),
                has_repo && stash_cnt > 0,
                ContextMenuTarget::RibbonApplyStash,
                "Apply or Pop Stash back into working tree…",
                "Quick Stash actions (Pop stash@{0}, Apply & Keep, Inspect all stashes)",
            ),
            tb_sep(),
            ribbon_plain_btn(
                icons::git_merge(icon_size::SM, ic).into(),
                "Merge".into(),
                p.text_primary,
                self.show_merge_modal,
                has_repo.then_some(Message::ToggleMergeModal),
                "Merge branch into current branch + Live Conflict Forecast (Ctrl+M)",
            ),
            ribbon_split_btn(
                icons::git_rebase(
                    icon_size::SM,
                    if has_repo { p.violet_fg } else { p.text_muted },
                )
                .into(),
                "Rebase".into(),
                p.text_primary,
                self.rebase_modal.is_some(),
                has_repo.then_some(Message::RibbonRebaseClicked),
                has_repo,
                ContextMenuTarget::RibbonRebase,
                "Interactive Rebase (Squash, Reword, Fixup, Reorder, Drop)…",
                "Rebase options (Interactive Rebase · Rebase Branch onto Another Branch)",
            ),
            ribbon_plain_btn(
                icons::branch(icon_size::SM, ic).into(),
                "Git-Flow".into(),
                p.text_primary,
                self.show_git_flow_modal,
                has_repo.then_some(Message::ToggleGitFlowModal),
                "Git-Flow (Initialize, Start or Finish Feature / Release / Hotfix branches)",
            ),
            ribbon_plain_btn(
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
                (undo_cnt > 0).then(|| {
                    Message::RequestConfirm(PendingConfirm::undo_last_action(&last_undo_label))
                }),
                "Undo last Reset / Revert / Merge / Rebase in this repository…",
            ),
            conflict_pill,
        ]
        .spacing(2)
        .align_y(Alignment::Center);

        // ── Zone 3 (Right Edge): Studio IDE + Layout Switcher + App Controls ──
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

        let app_tools_zone = row![
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
            ribbon_plain_btn(
                icons::external_window(icon_size::SM, p.accent_fg).into(),
                "Studio".into(),
                p.text_primary,
                !self.detached_viewers.is_empty(),
                Some(Message::OpenDetachedViewer),
                "Open GitFleet Studio (Multi-tab Code Editor, Search & Replace, Split Live Preview)",
            ),
            tb_sep(),
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
                git_workflow_zone,
                Space::with_width(Length::Fill),
                app_tools_zone,
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
