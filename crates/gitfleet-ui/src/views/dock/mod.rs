mod ignore;
mod output;
mod problems;
mod search;
mod secrets;
mod terminal;

use crate::app::{DockPosition, DrawerTab, GitFleetApp, Message, ResizeTarget};
use crate::icons;
use crate::theme::kit::{hairline_h_splitter, hairline_v_splitter, status_pill};
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use crate::views::{slim_scroll_h, tip};
use iced::widget::{button, column, container, horizontal_rule, row, text, tooltip};
use iced::{Alignment, Color, Element, Length};

impl GitFleetApp {
    pub(crate) fn view_auxiliary_drawer(
        &self,
        p: Palette,
        active_tab: DrawerTab,
    ) -> Element<'_, Message> {
        let dock_tab = |label: &'static str,
                        badge: Option<(String, Option<(Color, Color)>)>,
                        tab: DrawerTab|
         -> Element<'_, Message> {
            let is_active = active_tab == tab;
            let mut label_row = row![
                text(label)
                    .size(type_scale::CAPTION)
                    .font(if is_active {
                        theme::UI_FONT_MEDIUM
                    } else {
                        theme::UI_FONT
                    })
                    .color(if is_active {
                        p.text_primary
                    } else {
                        p.text_muted
                    }),
            ]
            .spacing(space::XXS)
            .align_y(Alignment::Center);

            if let Some((b, custom_colors)) = badge {
                let (bg, fg) = custom_colors.unwrap_or(if is_active {
                    (p.fill_selected, p.accent_fg)
                } else {
                    (p.surface_card, p.text_muted)
                });
                label_row = label_row.push(status_pill(b, fg, bg));
            }

            let indicator = if is_active {
                p.focus_ring
            } else {
                Color::TRANSPARENT
            };

            button(theme::bottom_underline(
                container(label_row).padding([6, 8]),
                indicator,
                2.0,
            ))
            .padding(0)
            .style(theme::vscode_dock_tab(p, is_active))
            .on_press(Message::ToggleDrawer(tab))
            .into()
        };

        let term_badge = (self.terminal_tabs.len() > 1)
            .then(|| (self.terminal_tabs.len().to_string(), None));
        let search_badge = (!self.search_results.is_empty())
            .then(|| (self.search_results.len().to_string(), None));
        let problems_cnt = self.total_problems_count();
        let problems_badge = (problems_cnt > 0)
            .then(|| (problems_cnt.to_string(), Some((p.warning_bg, p.warning_fg))));
        let encrypt_needed = self.vault_summary.encrypt_needed_count();
        let restore_needed = self.vault_summary.restore_needed_count();
        let secrets_badge = if self.vault_summary.vault_locked {
            Some(("locked".into(), Some((p.warning_bg, p.warning_fg))))
        } else if encrypt_needed > 0 {
            Some((
                format!("{encrypt_needed} mod"),
                Some((p.warning_bg, p.warning_fg)),
            ))
        } else if restore_needed > 0 {
            Some((
                format!("{restore_needed} restore"),
                Some((p.fill_selected, p.accent_fg)),
            ))
        } else if !self.vault_summary.files.is_empty() {
            Some((self.vault_summary.files.len().to_string(), None))
        } else {
            None
        };
        let log_count = self.service.command_log.list().len();
        let output_badge = (log_count > 0).then(|| (log_count.to_string(), None));

        let tabs_strip = slim_scroll_h(
            row![
                dock_tab("Terminal", term_badge, DrawerTab::Terminal),
                dock_tab("Problems", problems_badge, DrawerTab::Problems),
                dock_tab("Search", search_badge, DrawerTab::Search),
                dock_tab("Secrets", secrets_badge, DrawerTab::Secrets),
                dock_tab("Ignore", None, DrawerTab::Ignore),
                dock_tab("Output", output_badge, DrawerTab::CommandLog),
            ]
            .spacing(2)
            .align_y(Alignment::Center),
            340.0,
            p,
        );

        let mut right_actions = row![].spacing(2).align_y(Alignment::Center);

        if active_tab == DrawerTab::Terminal {
            let active_shell = self
                .terminal_tabs
                .get(self.active_terminal_tab)
                .and_then(|t| t.panes.get(t.focused_pane).or_else(|| t.panes.first()))
                .map(|pane| pane.short_label.clone())
                .unwrap_or_else(|| self.default_terminal_profile_id());
            let show_list = self.terminal_tabs.len() > 1 || self.show_terminal_sidebar;

            right_actions = right_actions
                .push(tip(
                    button(icons::search(icon_size::XS, p.text_secondary))
                        .padding(pad::BTN_ICON_SM)
                        .style(theme::vscode_icon_btn(p, self.terminal_search_open))
                        .on_press(Message::ToggleTerminalBufferSearch),
                    "Search Terminal Output Buffer",
                    tooltip::Position::Top,
                    p,
                ))
                .push(tip(
                    button(
                        row![
                            icons::terminal(icon_size::XS, p.text_secondary),
                            text(active_shell)
                                .size(type_scale::CAPTION)
                                .color(p.text_secondary),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                    )
                    .padding([3, 7])
                    .style(theme::vscode_icon_btn(p, show_list))
                    .on_press(Message::ToggleTerminalSidebar),
                    "Toggle Terminal Sessions List",
                    tooltip::Position::Top,
                    p,
                ))
                .push(tip(
                    button(icons::plus(icon_size::SM, p.text_secondary))
                        .padding(pad::BTN_ICON_SM)
                        .style(theme::vscode_icon_btn(p, false))
                        .on_press(Message::TerminalNewTab(None)),
                    "New Terminal (Ctrl+Shift+`)",
                    tooltip::Position::Top,
                    p,
                ))
                .push(tip(
                    button(icons::split_horizontal(icon_size::SM, p.text_secondary))
                        .padding(pad::BTN_ICON_SM)
                        .style(theme::vscode_icon_btn(p, false))
                        .on_press(Message::TerminalSplitPane(None)),
                    "Split Terminal (Ctrl+Shift+5)",
                    tooltip::Position::Top,
                    p,
                ))
                .push(tip(
                    button(icons::trash(icon_size::SM, p.text_secondary))
                        .padding(pad::BTN_ICON_SM)
                        .style(theme::vscode_icon_btn(p, false))
                        .on_press(Message::TerminalKillFocusedPane),
                    "Kill Active Terminal",
                    tooltip::Position::Top,
                    p,
                ))
                .push(tip(
                    button(icons::more_horizontal(icon_size::SM, p.text_secondary))
                        .padding(pad::BTN_ICON_SM)
                        .style(theme::vscode_icon_btn(p, self.show_terminal_menu))
                        .on_press(Message::ToggleTerminalMenu),
                    "More Terminal Actions (Profiles, Clear, Fonts, Settings)",
                    tooltip::Position::Top,
                    p,
                ));
        }

        // Dock Position Toggle (Bottom <-> Right Side)
        right_actions = right_actions
            .push(tip(
                button(icons::split_horizontal(
                    icon_size::SM,
                    if self.dock_position == DockPosition::Right {
                        p.accent_fg
                    } else {
                        p.text_secondary
                    },
                ))
                .padding(pad::BTN_ICON_SM)
                .style(theme::vscode_icon_btn(
                    p,
                    self.dock_position == DockPosition::Right,
                ))
                .on_press(Message::ToggleDockPosition),
                match self.dock_position {
                    DockPosition::Bottom => "Move Dock to Right Side (Ultrawide Mode)",
                    DockPosition::Right => "Move Dock back to Bottom",
                },
                tooltip::Position::Top,
                p,
            ))
            .push(tip(
                button(if self.terminal_expanded {
                    icons::chevron_down(icon_size::SM, p.text_secondary)
                } else {
                    icons::chevron_up(icon_size::SM, p.text_secondary)
                })
                .padding(pad::BTN_ICON_SM)
                .style(theme::vscode_icon_btn(p, false))
                .on_press(Message::ToggleTerminalExpanded),
                if self.terminal_expanded {
                    "Restore Dock Size"
                } else {
                    "Maximize Dock Size"
                },
                tooltip::Position::Top,
                p,
            ))
            .push(tip(
                button(icons::close_x(icon_size::SM, p.text_secondary))
                    .padding(pad::BTN_ICON_SM)
                    .style(theme::vscode_icon_btn(p, false))
                    .on_press(Message::CloseDrawer),
                "Close Dock Panel",
                tooltip::Position::Top,
                p,
            ));

        let header = container(
            row![
                container(tabs_strip).width(Length::Fill),
                right_actions,
            ]
            .spacing(space::XS)
            .padding([0, 8])
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .style(theme::vscode_dock_header(p));

        let body: Element<'_, Message> = match active_tab {
            DrawerTab::Terminal => self.view_terminal_drawer(p),
            DrawerTab::Problems => self.view_problems_drawer(p),
            DrawerTab::Search => self.view_search_drawer(p),
            DrawerTab::Secrets => self.view_secrets_drawer(p),
            DrawerTab::Ignore => self.view_ignore_drawer(p),
            DrawerTab::CommandLog => self.view_command_log_body(p),
        };

        match self.dock_position {
            DockPosition::Bottom => {
                let dragging = self.active_resize == Some(ResizeTarget::DrawerHeight);
                container(column![
                    hairline_h_splitter(p, dragging, Message::StartResize(ResizeTarget::DrawerHeight)),
                    header,
                    horizontal_rule(1).style(theme::subtle_rule(p)),
                    body,
                ])
                .width(Length::Fill)
                .height(Length::Fixed(self.drawer_height.max(layout::DOCK_MIN_HEIGHT)))
                .style(theme::vscode_dock_box(p))
                .into()
            }
            DockPosition::Right => {
                let dragging = self.active_resize == Some(ResizeTarget::DrawerWidth);
                row![
                    hairline_v_splitter(p, dragging, Message::StartResize(ResizeTarget::DrawerWidth)),
                    container(column![
                        header,
                        horizontal_rule(1).style(theme::subtle_rule(p)),
                        body,
                    ])
                    .width(Length::Fixed(
                        self.drawer_width.max(layout::DOCK_RIGHT_MIN_WIDTH),
                    ))
                    .height(Length::Fill)
                    .style(theme::vscode_dock_box(p)),
                ]
                .height(Length::Fill)
                .into()
            }
        }
    }
}
