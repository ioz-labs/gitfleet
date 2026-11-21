mod ignore;
mod output;
mod search;
mod secrets;
mod terminal;

use crate::app::{DrawerTab, GitFleetApp, Message, ResizeTarget};
use crate::icons;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use crate::views::tip;
use iced::widget::{
    Space, button, column, container, horizontal_rule, mouse_area, row, text, tooltip,
};
use iced::{Alignment, Color, Element, Length, mouse};

impl GitFleetApp {
    pub(crate) fn view_auxiliary_drawer(
        &self,
        p: Palette,
        active_tab: DrawerTab,
    ) -> Element<'_, Message> {
        // 1. Draggable 4px top resize border
        let resizing_dock = self.active_resize == Some(ResizeTarget::DrawerHeight);
        let resize_handle = mouse_area(
            container(Space::new(Length::Fill, Length::Fixed(4.0)))
                .width(Length::Fill)
                .height(Length::Fixed(4.0))
                .style(theme::vscode_resize_handle(p, resizing_dock)),
        )
        .interaction(mouse::Interaction::ResizingVertically)
        .on_press(Message::StartResize(ResizeTarget::DrawerHeight));

        // 2. VS Code-style underline tab helper using Design System tokens
        let dock_tab = |label: String, badge: Option<String>, tab: DrawerTab| -> Element<'_, Message> {
            let is_active = active_tab == tab;
            let mut label_row = row![
                text(label).size(type_scale::BODY).color(if is_active {
                    p.text_primary
                } else {
                    p.text_muted
                }),
            ]
            .spacing(space::XS)
            .align_y(Alignment::Center);

            if let Some(b) = badge {
                label_row = label_row.push(theme::badge(
                    b,
                    if is_active {
                        p.fill_selected
                    } else {
                        p.surface_card
                    },
                    if is_active { p.accent_fg } else { p.text_muted },
                    Color::TRANSPARENT,
                    false,
                ));
            }

            let underline = container(Space::new(Length::Fill, Length::Fixed(2.0)))
                .width(Length::Fill)
                .height(Length::Fixed(2.0))
                .style(move |_| container::Style {
                    background: Some(iced::Background::Color(if is_active {
                        p.accent_primary
                    } else {
                        Color::TRANSPARENT
                    })),
                    ..Default::default()
                });

            button(
                column![container(label_row).padding([7, 10]), underline].width(Length::Shrink),
            )
            .padding(0)
            .style(theme::vscode_dock_tab(p, is_active))
            .on_press(Message::ToggleDrawer(tab))
            .into()
        };

        let term_badge =
            (self.terminal_tabs.len() > 1).then(|| self.terminal_tabs.len().to_string());
        let search_badge =
            (!self.search_results.is_empty()).then(|| self.search_results.len().to_string());
        let secrets_badge = if self.vault_summary.vault_locked {
            Some("locked".into())
        } else if !self.vault_summary.files.is_empty() {
            Some(self.vault_summary.files.len().to_string())
        } else {
            None
        };
        let log_count = self.service.command_log.list().len();
        let output_badge = (log_count > 0).then(|| log_count.to_string());

        let tabs_row = row![
            dock_tab("Terminal".into(), term_badge, DrawerTab::Terminal),
            dock_tab("Search".into(), search_badge, DrawerTab::Search),
            dock_tab("Secrets".into(), secrets_badge, DrawerTab::Secrets),
            dock_tab("Ignore".into(), None, DrawerTab::Ignore),
            dock_tab("Output".into(), output_badge, DrawerTab::CommandLog),
        ]
        .spacing(space::XXS)
        .align_y(Alignment::Center);

        // 3. Contextual VS Code right-side icon actions
        let mut right_actions = row![].spacing(3).align_y(Alignment::Center);

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
                    button(
                        row![
                            icons::terminal(icon_size::XS, p.text_secondary),
                            text(active_shell)
                                .size(type_scale::BODY)
                                .color(p.text_secondary),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                    )
                    .padding([4, 8])
                    .style(theme::vscode_icon_btn(p, show_list))
                    .on_press(Message::ToggleTerminalSidebar),
                    "Toggle Terminal Sessions List",
                    tooltip::Position::Top,
                    p,
                ))
                .push(tip(
                    button(icons::plus(icon_size::SM, p.text_secondary))
                        .padding(pad::BTN_ICON)
                        .style(theme::vscode_icon_btn(p, false))
                        .on_press(Message::TerminalNewTab(None)),
                    "New Terminal (Ctrl+Shift+`)",
                    tooltip::Position::Top,
                    p,
                ))
                .push(tip(
                    button(icons::chevron_down(icon_size::XS, p.text_secondary))
                        .padding([5, 5])
                        .style(theme::vscode_icon_btn(p, self.show_terminal_menu))
                        .on_press(Message::ToggleTerminalMenu),
                    "Launch Profile / Split / Default Profile…",
                    tooltip::Position::Top,
                    p,
                ))
                .push(tip(
                    button(icons::split_horizontal(icon_size::SM, p.text_secondary))
                        .padding(pad::BTN_ICON)
                        .style(theme::vscode_icon_btn(p, false))
                        .on_press(Message::TerminalSplitPane(None)),
                    "Split Terminal (Ctrl+Shift+5)",
                    tooltip::Position::Top,
                    p,
                ))
                .push(tip(
                    button(icons::trash(icon_size::SM, p.text_secondary))
                        .padding(pad::BTN_ICON)
                        .style(theme::vscode_icon_btn(p, false))
                        .on_press(Message::TerminalKillFocusedPane),
                    "Kill Active Terminal",
                    tooltip::Position::Top,
                    p,
                ))
                .push(tip(
                    button(icons::more_horizontal(icon_size::SM, p.text_secondary))
                        .padding(pad::BTN_ICON)
                        .style(theme::vscode_icon_btn(p, self.show_terminal_menu))
                        .on_press(Message::ToggleTerminalMenu),
                    "More Terminal Actions (Ctrl+C, Paste, Profiles, Settings)",
                    tooltip::Position::Top,
                    p,
                ))
                .push(Space::with_width(Length::Fixed(space::XXS)));
        }

        right_actions = right_actions
            .push(tip(
                button(if self.terminal_expanded {
                    icons::chevron_down(icon_size::SM, p.text_secondary)
                } else {
                    icons::chevron_up(icon_size::SM, p.text_secondary)
                })
                .padding(pad::BTN_ICON)
                .style(theme::vscode_icon_btn(p, false))
                .on_press(Message::ToggleTerminalExpanded),
                if self.terminal_expanded {
                    "Restore Panel Size"
                } else {
                    "Maximize Panel Size"
                },
                tooltip::Position::Top,
                p,
            ))
            .push(tip(
                button(icons::close_x(icon_size::SM, p.text_secondary))
                    .padding(pad::BTN_ICON)
                    .style(theme::vscode_icon_btn(p, false))
                    .on_press(Message::CloseDrawer),
                "Close Panel",
                tooltip::Position::Top,
                p,
            ));

        let header = container(
            row![tabs_row, Space::with_width(Length::Fill), right_actions]
                .padding([0, 10])
                .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .style(theme::vscode_dock_header(p));

        // All dock tabs span full-bleed so their secondary `dock_toolbar` aligns flush with the header
        let body: Element<'_, Message> = match active_tab {
            DrawerTab::Terminal => self.view_terminal_drawer(p),
            DrawerTab::Search => self.view_search_drawer(p),
            DrawerTab::Secrets => self.view_secrets_drawer(p),
            DrawerTab::Ignore => self.view_ignore_drawer(p),
            DrawerTab::CommandLog => self.view_command_log_body(p),
        };

        container(column![resize_handle, header, horizontal_rule(1), body])
            .width(Length::Fill)
            .height(Length::Fixed(
                self.drawer_height.max(layout::DOCK_MIN_HEIGHT),
            ))
            .style(theme::vscode_dock_box(p))
            .into()
    }
}
