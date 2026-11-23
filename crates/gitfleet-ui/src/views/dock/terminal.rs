use crate::app::{GitFleetApp, Message, STATUS_BAR_HEIGHT, TerminalMenuSubmenu};
use crate::icons;
use crate::theme::kit::{ButtonSize, ButtonVariant, UiButton, status_pill};
use crate::theme::{self, Palette, ThemeMode, icon_size, layout, pad, space, type_scale};
use crate::views::{slim_scroll, tip, truncate_chars};
use iced::widget::{
    Space, button, column, container, horizontal_rule, opaque, row, text, text_input, tooltip,
    vertical_rule,
};
use iced::{Alignment, Color, Element, Length, Padding};

impl GitFleetApp {
    pub(crate) fn view_terminal_drawer(&self, p: Palette) -> Element<'_, Message> {
        let default_pid = self.default_terminal_profile_id();
        let default_label = self
            .service
            .terminal
            .profile_by_id(&default_pid)
            .map(|pr| pr.label)
            .unwrap_or_else(|| default_pid.clone());
        let font_name = self.active_terminal_font();
        let font_size = self.service.settings().terminal_font_size.clamp(12, 24) as f32;
        let is_dark = self.theme_mode == ThemeMode::Dark;
        let term_bg = p.surface_input;
        let term_fg = p.text_primary;

        let main_area: Element<'_, Message> =
            if let Some(tab) = self.terminal_tabs.get(self.active_terminal_tab) {
                let split_mode = tab.panes.len() > 1;
                let mut panes_row = row![].spacing(0).height(Length::Fill).width(Length::Fill);
                for (p_idx, pane) in tab.panes.iter().enumerate() {
                    if p_idx > 0 {
                        panes_row = panes_row.push(vertical_rule(1).style(theme::subtle_rule(p)));
                    }
                    let is_focused = self.terminal_focused && p_idx == tab.focused_pane;
                    let canvas_el = crate::terminal::terminal_pane_canvas(
                        pane.session_id.clone(),
                        &pane.snapshot,
                        &font_name,
                        font_size,
                        is_dark,
                        is_focused,
                        term_bg,
                        term_fg,
                        p.accent_primary,
                    );

                    let pane_box = container(canvas_el)
                        .width(Length::FillPortion(1))
                        .height(Length::Fill)
                        .style(move |_| container::Style {
                            background: Some(iced::Background::Color(term_bg)),
                            border: iced::Border {
                                color: if is_focused && split_mode {
                                    p.accent_primary
                                } else {
                                    Color::TRANSPARENT
                                },
                                width: if is_focused && split_mode { 1.0 } else { 0.0 },
                                radius: 0.0.into(),
                            },
                            ..Default::default()
                        });
                    panes_row = panes_row.push(pane_box);
                }

                if self.terminal_search_open {
                    let q = self.terminal_search_query.trim().to_lowercase();
                    let focused_pane = tab.panes.get(tab.focused_pane).or_else(|| tab.panes.first());
                    let mut matched_lines: Vec<(usize, String)> = Vec::new();
                    if !q.is_empty()
                        && let Some(fp) = focused_pane
                    {
                        for (r_idx, trow) in fp.snapshot.lines.iter().enumerate() {
                            let line_str: String =
                                trow.runs.iter().map(|r| r.text.as_str()).collect();
                            let trimmed = line_str.trim();
                            if !trimmed.is_empty() && trimmed.to_lowercase().contains(&q) {
                                matched_lines.push((r_idx + 1, trimmed.to_string()));
                            }
                        }
                    }

                    let mut search_bar = row![
                        icons::search(icon_size::XS, p.accent_fg),
                        text_input(
                            "Search active terminal buffer…",
                            &self.terminal_search_query
                        )
                        .on_input(Message::TerminalBufferSearchChanged)
                        .padding([4, 8])
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .width(Length::Fixed(240.0))
                        .style(theme::dark_input(p)),
                        status_pill(
                            format!("{} match(es)", matched_lines.len()),
                            if matched_lines.is_empty() {
                                p.text_muted
                            } else {
                                p.accent_fg
                            },
                            p.surface_card,
                        ),
                    ]
                    .spacing(space::XS)
                    .align_y(Alignment::Center);

                    if let Some((ln, preview)) = matched_lines.first() {
                        search_bar = search_bar.push(
                            text(format!("L{ln}: {}", truncate_chars(preview, 54)))
                                .size(type_scale::BADGE)
                                .font(theme::CODE_FONT)
                                .color(p.text_secondary),
                        );
                    }
                    search_bar = search_bar
                        .push(Space::with_width(Length::Fill))
                        .push(
                            UiButton::new(p, "Clear Screen")
                                .variant(ButtonVariant::Subtle)
                                .size(ButtonSize::Xs)
                                .on_press(Message::TerminalClearActiveScreen),
                        )
                        .push(
                            UiButton::icon_only(p, icons::close_x(icon_size::XS, p.text_secondary))
                                .variant(ButtonVariant::Ghost)
                                .size(ButtonSize::IconSm)
                                .on_press(Message::ToggleTerminalBufferSearch),
                        );

                    column![
                        theme::dock_toolbar(p, search_bar),
                        panes_row,
                    ]
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
                } else {
                    panes_row.into()
                }
            } else {
                let actions = row![
                    button(
                        row![
                            icons::plus(icon_size::XS, Color::WHITE),
                            text(format!("New {default_label}")).size(type_scale::BODY),
                        ]
                        .spacing(space::XS)
                        .align_y(Alignment::Center),
                    )
                    .padding(pad::BTN_MD)
                    .style(theme::primary_btn(p))
                    .on_press(Message::TerminalNewTab(None)),
                    button(
                        row![
                            text("Choose Profile").size(type_scale::BODY),
                            icons::chevron_down(icon_size::XS, p.text_secondary),
                        ]
                        .spacing(space::XS)
                        .align_y(Alignment::Center),
                    )
                    .padding(pad::BTN_MD)
                    .style(theme::subtle_btn(p))
                    .on_press(Message::ToggleTerminalMenu),
                ]
                .spacing(space::SM)
                .align_y(Alignment::Center);

                container(theme::empty_state(
                    p,
                    icons::terminal(icon_size::LG, p.accent_fg),
                    "No active terminal session",
                    format!("Default profile: {default_label} — open a new shell or split panes."),
                    Some(actions.into()),
                ))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(iced::Background::Color(term_bg)),
                    ..Default::default()
                })
                .into()
            };

        let show_sidebar = self.terminal_tabs.len() > 1
            || self.show_terminal_sidebar
            || self
                .terminal_tabs
                .first()
                .is_some_and(|t| t.panes.len() > 1);

        if !show_sidebar {
            return main_area;
        }

        // VS Code-style right Terminal Sessions List using crisp SVG icons (no Unicode box glyphs)
        let mut sessions_col = column![].spacing(2).padding([6, 6]);
        for (t_idx, tab) in self.terminal_tabs.iter().enumerate() {
            let is_active_tab = t_idx == self.active_terminal_tab;
            if tab.panes.len() <= 1 {
                if let Some(pane) = tab.panes.first() {
                    sessions_col = sessions_col.push(
                        row![
                            tip(
                                button(
                                    row![
                                        icons::terminal(
                                            icon_size::XS,
                                            if is_active_tab {
                                                p.accent_fg
                                            } else {
                                                p.text_muted
                                            }
                                        ),
                                        text(pane.short_label.clone())
                                            .size(type_scale::BODY)
                                            .color(if is_active_tab {
                                                p.text_primary
                                            } else {
                                                p.text_secondary
                                            }),
                                    ]
                                    .spacing(space::XS)
                                    .align_y(Alignment::Center),
                                )
                                .width(Length::Fill)
                                .padding([5, 8])
                                .style(theme::repo_row_btn(p, is_active_tab))
                                .on_press(Message::TerminalSelectTab(t_idx)),
                                &pane.profile_label,
                                tooltip::Position::Left,
                                p,
                            ),
                            button(icons::trash(icon_size::XS, p.text_muted))
                                .padding(pad::BTN_ICON)
                                .style(theme::vscode_icon_btn(p, false))
                                .on_press(Message::TerminalCloseTab(t_idx)),
                        ]
                        .spacing(3)
                        .align_y(Alignment::Center),
                    );
                }
            } else {
                for (p_idx, pane) in tab.panes.iter().enumerate() {
                    let is_focused = is_active_tab && p_idx == tab.focused_pane;
                    let leading: Element<'_, Message> = if p_idx == 0 {
                        icons::split_horizontal(icon_size::XS, p.text_muted).into()
                    } else {
                        icons::corner_down_right(icon_size::XS, p.text_muted).into()
                    };
                    sessions_col = sessions_col.push(
                        row![
                            tip(
                                button(
                                    row![
                                        leading,
                                        icons::terminal(
                                            icon_size::XS,
                                            if is_focused {
                                                p.accent_fg
                                            } else {
                                                p.text_muted
                                            }
                                        ),
                                        text(pane.short_label.clone())
                                            .size(type_scale::BODY)
                                            .color(if is_focused {
                                                p.text_primary
                                            } else {
                                                p.text_secondary
                                            }),
                                    ]
                                    .spacing(5)
                                    .align_y(Alignment::Center),
                                )
                                .width(Length::Fill)
                                .padding([5, 8])
                                .style(theme::repo_row_btn(p, is_focused))
                                .on_press(Message::TerminalFocusPane(t_idx, p_idx)),
                                &pane.profile_label,
                                tooltip::Position::Left,
                                p,
                            ),
                            button(icons::trash(icon_size::XS, p.text_muted))
                                .padding(pad::BTN_ICON)
                                .style(theme::vscode_icon_btn(p, false))
                                .on_press(Message::TerminalClosePane(t_idx, p_idx)),
                        ]
                        .spacing(3)
                        .align_y(Alignment::Center),
                    );
                }
            }
        }

        let sidebar = container(slim_scroll(sessions_col, p).height(Length::Fill))
            .width(Length::Fixed(layout::TERMINAL_SIDEBAR_WIDTH))
            .height(Length::Fill)
            .style(theme::vscode_dock_box(p));

        row![main_area, vertical_rule(1).style(theme::subtle_rule(p)), sidebar]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub(crate) fn view_terminal_profile_menu(&self, p: Palette) -> Element<'_, Message> {
        let profiles = self.service.terminal.profiles();
        let default_pid = self.default_terminal_profile_id();

        let menu_item = |label: String, shortcut: &'static str, msg: Message| -> Element<'_, Message> {
            button(
                row![
                    text(label)
                        .size(type_scale::BODY)
                        .color(p.text_primary),
                    Space::with_width(Length::Fill),
                    text(shortcut)
                        .size(type_scale::CAPTION)
                        .color(p.text_muted),
                ]
                .spacing(space::MD)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding(pad::ROW)
            .style(theme::context_menu_item(p, false))
            .on_press(msg)
            .into()
        };

        let submenu_toggle = |label: &'static str, open: bool, sub: TerminalMenuSubmenu| -> Element<'_, Message> {
            let chevron: Element<'_, Message> = if open {
                icons::chevron_down(icon_size::XS, p.text_secondary).into()
            } else {
                icons::chevron_right(icon_size::XS, p.text_secondary).into()
            };
            button(
                row![
                    text(label).size(type_scale::BODY).color(p.text_primary),
                    Space::with_width(Length::Fill),
                    chevron,
                ]
                .spacing(space::MD)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding(pad::ROW)
            .style(theme::context_menu_item(p, false))
            .on_press(Message::SetTerminalMenuSubmenu(sub))
            .into()
        };

        let mut items = column![
            menu_item(
                "New Terminal".into(),
                "Ctrl+Shift+`",
                Message::TerminalNewTab(None),
            ),
            menu_item(
                "New Terminal Window".into(),
                "External",
                Message::TerminalOpenExternalWindow(None),
            ),
            menu_item(
                "Split Terminal".into(),
                "Ctrl+Shift+5",
                Message::TerminalSplitPane(None),
            ),
            horizontal_rule(1).style(theme::subtle_rule(p)),
        ]
        .spacing(2);

        for prof in &profiles {
            let is_def = prof.id == default_pid;
            let title = if is_def {
                format!("{} (Default)", prof.label)
            } else {
                prof.label.clone()
            };
            items = items.push(menu_item(
                title,
                "",
                Message::TerminalNewTab(Some(prof.id.clone())),
            ));
        }

        // Split Terminal with Profile expandable submenu (uses crisp SVG chevrons & arrows)
        let split_open = self.terminal_menu_submenu == TerminalMenuSubmenu::SplitWithProfile;
        items = items.push(submenu_toggle(
            "Split Terminal with Profile",
            split_open,
            TerminalMenuSubmenu::SplitWithProfile,
        ));
        if split_open {
            let mut sub = column![].spacing(2);
            for prof in &profiles {
                let is_def = prof.id == default_pid;
                let label = if is_def {
                    format!("{} (Default)", prof.label)
                } else {
                    prof.label.clone()
                };
                sub = sub.push(
                    button(
                        row![
                            icons::corner_down_right(icon_size::XS, p.accent_fg),
                            text(label).size(type_scale::BODY).color(p.accent_fg),
                            Space::with_width(Length::Fill),
                            text("Split").size(type_scale::BADGE).color(p.text_muted),
                        ]
                        .spacing(space::XS)
                        .align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .padding([5, 14])
                    .style(theme::context_menu_item(p, false))
                    .on_press(Message::TerminalSplitPane(Some(prof.id.clone()))),
                );
            }
            items = items.push(sub);
        }

        items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));

        // Select Default Profile expandable submenu (uses crisp SVG checkmark)
        let def_open = self.terminal_menu_submenu == TerminalMenuSubmenu::SelectDefaultProfile;
        items = items.push(submenu_toggle(
            "Select Default Profile",
            def_open,
            TerminalMenuSubmenu::SelectDefaultProfile,
        ));
        if def_open {
            let mut sub = column![].spacing(2);
            for prof in &profiles {
                let is_def = prof.id == default_pid;
                let mark: Element<'_, Message> = if is_def {
                    icons::check(icon_size::XS, p.success_fg).into()
                } else {
                    Space::with_width(Length::Fixed(icon_size::XS)).into()
                };
                sub = sub.push(
                    button(
                        row![
                            mark,
                            text(prof.label.clone())
                                .size(type_scale::BODY)
                                .color(if is_def {
                                    p.success_fg
                                } else {
                                    p.text_secondary
                                }),
                            Space::with_width(Length::Fill),
                        ]
                        .spacing(space::XS)
                        .align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .padding([5, 14])
                    .style(theme::context_menu_item(p, false))
                    .on_press(Message::SetDefaultTerminalProfile(prof.id.clone())),
                );
            }
            items = items.push(sub);
        }

        items = items.push(menu_item(
            "Configure Terminal Settings".into(),
            "",
            Message::ToggleSettingsModal,
        ));
        items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
        items = items.push(menu_item(
            "Paste Clipboard".into(),
            "Ctrl+V",
            Message::TerminalPasteClipboard,
        ));
        items = items.push(menu_item(
            "Copy Terminal Screen".into(),
            "Ctrl+Shift+C",
            Message::TerminalCopyScreen,
        ));
        items = items.push(menu_item(
            "Send Interrupt (Ctrl+C)".into(),
            "SIGINT",
            Message::TerminalSendCtrlC,
        ));
        items = items.push(menu_item(
            "Open Active Repo in Explorer".into(),
            "",
            Message::RevealSelectedInExplorer,
        ));

        let menu_card = opaque(
            container(crate::views::slim_scroll_overlay(items, p).height(Length::Shrink))
                .padding(6)
                .width(Length::Fixed(layout::TERMINAL_MENU_WIDTH))
                .max_height(480.0)
                .style(theme::context_menu_box(p)),
        );

        let bottom_pad = (self.drawer_height + STATUS_BAR_HEIGHT - 28.0).max(40.0);
        let positioned = container(menu_card)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Bottom)
            .padding(Padding {
                top: 24.0,
                right: 76.0,
                bottom: bottom_pad,
                left: 0.0,
            });

        theme::overlay_backdrop(positioned, Message::ToggleTerminalMenu)
    }
}
