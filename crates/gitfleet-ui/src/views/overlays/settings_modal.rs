use crate::app::{GitFleetApp, Message, SettingsTab};
use crate::icons;
use crate::theme::{self, DARK, LIGHT, Palette, ThemeMode, icon_size, pad, space, type_scale};
use crate::views::{slim_scroll, truncate_chars};
use iced::widget::{
    Space, button, column, container, horizontal_rule, opaque, row, text, text_input,
    vertical_rule,
};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding};

fn setting_group_title<'a>(p: Palette, title: &'a str) -> Element<'a, Message> {
    text(title)
        .size(type_scale::CAPTION)
        .color(p.text_muted)
        .into()
}

fn setting_row<'a>(
    p: Palette,
    title: &'a str,
    subtitle: &'a str,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    row![
        column![
            text(title).size(type_scale::BODY).color(p.text_primary),
            text(subtitle).size(type_scale::CAPTION).color(p.text_muted),
        ]
        .spacing(2)
        .width(Length::Fill),
        control.into(),
    ]
    .spacing(space::MD)
    .align_y(Alignment::Center)
    .padding([11, 14])
    .width(Length::Fill)
    .into()
}

fn grouped_card<'a>(p: Palette, items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let mut col = column![].spacing(0).width(Length::Fill);
    let len = items.len();
    for (idx, item) in items.into_iter().enumerate() {
        col = col.push(item);
        if idx + 1 < len {
            col = col.push(horizontal_rule(1).style(theme::subtle_rule(p)));
        }
    }
    container(col)
        .width(Length::Fill)
        .style(theme::section_panel_box(p))
        .into()
}

fn theme_preview_card<'a>(
    p: Palette,
    target_mode: ThemeMode,
    active: bool,
    title: &'static str,
    desc: &'static str,
) -> Element<'a, Message> {
    let sample = match target_mode {
        ThemeMode::Dark => DARK,
        ThemeMode::Light => LIGHT,
    };

    let mini_window = container(
        column![
            // Mini titlebar
            container(
                row![
                    container(Space::new(6, 6)).style(theme::dot_box(sample.danger_fg)),
                    container(Space::new(6, 6)).style(theme::dot_box(sample.warning_fg)),
                    container(Space::new(6, 6)).style(theme::dot_box(sample.success_fg)),
                    Space::with_width(Length::Fill),
                    container(Space::new(28, 5)).style(move |_| container::Style {
                        background: Some(Background::Color(sample.accent_primary)),
                        border: Border {
                            radius: 999.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
            )
            .padding([5, 8])
            .width(Length::Fill)
            .style(move |_| container::Style {
                background: Some(Background::Color(sample.surface_titlebar)),
                ..Default::default()
            }),
            // Mini 2-pane body
            row![
                container(
                    column![
                        container(Space::new(Length::Fill, 6)).style(move |_| container::Style {
                            background: Some(Background::Color(sample.fill_selected)),
                            border: Border {
                                radius: 2.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        container(Space::new(34, 4)).style(move |_| container::Style {
                            background: Some(Background::Color(sample.border_strong)),
                            border: Border {
                                radius: 2.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                        container(Space::new(28, 4)).style(move |_| container::Style {
                            background: Some(Background::Color(sample.border_default)),
                            border: Border {
                                radius: 2.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                    ]
                    .spacing(5),
                )
                .width(Length::Fixed(58.0))
                .height(Length::Fill)
                .padding(6)
                .style(move |_| container::Style {
                    background: Some(Background::Color(sample.surface_sidebar)),
                    ..Default::default()
                }),
                container(
                    column![
                        container(Space::new(Length::Fill, 6)).style(move |_| container::Style {
                            background: Some(Background::Color(sample.success_bg)),
                            border: Border {
                                color: sample.success_fg,
                                width: 0.5,
                                radius: 2.0.into(),
                            },
                            ..Default::default()
                        }),
                        container(Space::new(Length::Fill, 6)).style(move |_| container::Style {
                            background: Some(Background::Color(sample.danger_bg)),
                            border: Border {
                                color: sample.danger_fg,
                                width: 0.5,
                                radius: 2.0.into(),
                            },
                            ..Default::default()
                        }),
                        container(Space::new(80, 4)).style(move |_| container::Style {
                            background: Some(Background::Color(sample.border_strong)),
                            border: Border {
                                radius: 2.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                    ]
                    .spacing(5),
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(7)
                .style(move |_| container::Style {
                    background: Some(Background::Color(sample.surface_panel)),
                    ..Default::default()
                }),
            ]
            .height(Length::Fixed(52.0)),
        ]
        .spacing(0),
    )
    .width(Length::Fill)
    .style(move |_| container::Style {
        background: Some(Background::Color(sample.surface_app)),
        border: Border {
            color: sample.border_default,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    });

    let footer_row = row![
        match target_mode {
            ThemeMode::Dark => icons::moon(14.0, if active { p.accent_fg } else { p.text_secondary }),
            ThemeMode::Light => icons::sun(14.0, if active { p.warning_fg } else { p.text_secondary }),
        },
        column![
            text(title).size(type_scale::BODY).color(p.text_primary),
            text(desc).size(type_scale::CAPTION).color(p.text_muted),
        ]
        .spacing(1)
        .width(Length::Fill),
        if active {
            Element::from(
                container(icons::check(11.0, Color::WHITE))
                    .width(Length::Fixed(18.0))
                    .height(Length::Fixed(18.0))
                    .center_x(Length::Fixed(18.0))
                    .center_y(Length::Fixed(18.0))
                    .style(theme::dot_box(p.accent_primary)),
            )
        } else {
            Element::from(
                container(Space::new(16, 16)).style(move |_| container::Style {
                    border: Border {
                        color: p.border_strong,
                        width: 1.0,
                        radius: 999.0.into(),
                    },
                    ..Default::default()
                }),
            )
        },
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    button(
        column![mini_window, footer_row]
            .spacing(10)
            .width(Length::Fill),
    )
    .padding(10)
    .width(Length::FillPortion(1))
    .style(theme::choice_card_btn(p, active))
    .on_press(Message::SetThemeMode(target_mode))
    .into()
}

impl GitFleetApp {
    pub(crate) fn view_settings_modal(&self, p: Palette) -> Element<'_, Message> {
        let s = self.service.settings();

        // ── Left Navigation Rail ──────────────────────────────────────────────────────────
        let nav_item = |tab: SettingsTab,
                        label: &'static str,
                        icon: Element<'static, Message>|
         -> Element<'_, Message> {
            let active = self.settings_tab == tab;
            button(
                row![
                    container(Space::new(
                        Length::Fixed(3.0),
                        Length::Fixed(16.0),
                    ))
                    .style(move |_| container::Style {
                        background: Some(Background::Color(if active {
                            p.accent_primary
                        } else {
                            Color::TRANSPARENT
                        })),
                        border: Border {
                            radius: 999.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    icon,
                    text(label)
                        .size(type_scale::BODY)
                        .color(if active { p.text_primary } else { p.text_secondary })
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(9)
                .align_y(Alignment::Center),
            )
            .padding([8, 10])
            .width(Length::Fill)
            .style(theme::settings_nav_btn(p, active))
            .on_press(Message::SetSettingsTab(tab))
            .into()
        };

        let ic_col = |tab: SettingsTab| -> Color {
            if self.settings_tab == tab {
                p.accent_fg
            } else {
                p.text_muted
            }
        };

        let left_rail = container(
            column![
                // Rail title header
                row![
                    container(icons::settings(icon_size::SM, p.accent_fg))
                        .padding(6)
                        .style(theme::pill_box(p.fill_selected, p.accent_fg, p.border_default)),
                    column![
                        text("Settings")
                            .size(type_scale::HEADING)
                            .color(p.text_primary),
                        text(format!("GitFleet v{}", env!("CARGO_PKG_VERSION")))
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                    ]
                    .spacing(1),
                ]
                .spacing(10)
                .align_y(Alignment::Center)
                .padding([16, 14]),
                horizontal_rule(1).style(theme::subtle_rule(p)),
                // Category list
                container(
                    column![
                        nav_item(
                            SettingsTab::General,
                            "General",
                            icons::sliders(14.0, ic_col(SettingsTab::General)).into(),
                        ),
                        nav_item(
                            SettingsTab::Terminal,
                            "Terminal",
                            icons::terminal(14.0, ic_col(SettingsTab::Terminal)).into(),
                        ),
                        nav_item(
                            SettingsTab::Automation,
                            "Automation",
                            icons::shield(14.0, ic_col(SettingsTab::Automation)).into(),
                        ),
                        nav_item(
                            SettingsTab::Diagnostics,
                            "Tools & Info",
                            icons::wrench(14.0, ic_col(SettingsTab::Diagnostics)).into(),
                        ),
                    ]
                    .spacing(4),
                )
                .padding([12, 10]),
                Space::with_height(Length::Fill),
                // Bottom compact system pill
                container(
                    container(
                        column![
                            row![
                                container(Space::new(7, 7)).style(theme::dot_box(p.success_fg)),
                                text(truncate_chars(&self.git_version, 22))
                                    .size(type_scale::CAPTION)
                                    .font(theme::CODE_FONT)
                                    .color(p.text_secondary)
                                    .wrapping(text::Wrapping::None),
                            ]
                            .spacing(6)
                            .align_y(Alignment::Center),
                            text(format!(
                                "{} · {}",
                                std::env::consts::OS,
                                std::env::consts::ARCH
                            ))
                            .size(type_scale::CAPTION)
                            .font(theme::CODE_FONT)
                            .color(p.text_muted),
                        ]
                        .spacing(3),
                    )
                    .padding([8, 10])
                    .width(Length::Fill)
                    .style(theme::section_panel_box(p)),
                )
                .padding([12, 12]),
            ]
            .height(Length::Fill),
        )
        .width(Length::Fixed(214.0))
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(p.surface_sidebar)),
            text_color: Some(p.text_primary),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: iced::border::Radius {
                    top_left: 12.0,
                    top_right: 0.0,
                    bottom_right: 0.0,
                    bottom_left: 12.0,
                },
            },
            ..Default::default()
        });

        // ── Right Content Header & Body ───────────────────────────────────────────────────
        let (page_title, page_subtitle) = match self.settings_tab {
            SettingsTab::General => (
                "General & Appearance",
                "Customize workspace theme, diff defaults, and multi-repo concurrency.",
            ),
            SettingsTab::Terminal => (
                "Embedded Terminal",
                "Configure default shell profile, Nerd Font family, and terminal typography.",
            ),
            SettingsTab::Automation => (
                "Automation & Pre-Commit Hygiene",
                "Filesystem watching, scheduled background fetches, and oversized file warnings.",
            ),
            SettingsTab::Diagnostics => (
                "Tools & Diagnostics",
                "Inspect detected external diff/editor tools and export diagnostics reports.",
            ),
        };

        let page_header = row![
            column![
                text(page_title)
                    .size(type_scale::TITLE)
                    .color(p.text_primary),
                text(page_subtitle)
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
            ]
            .spacing(2),
            Space::with_width(Length::Fill),
            button(icons::close_x(icon_size::SM, p.text_secondary))
                .padding(pad::BTN_ICON)
                .style(theme::vscode_icon_btn(p, false))
                .on_press(Message::CloseOverlays),
        ]
        .align_y(Alignment::Center)
        .padding([16, 22]);

        let page_content: Element<'_, Message> = match self.settings_tab {
            SettingsTab::General => self.view_settings_general_tab(p, &s),
            SettingsTab::Terminal => self.view_settings_terminal_tab(p, &s),
            SettingsTab::Automation => self.view_settings_automation_tab(p, &s),
            SettingsTab::Diagnostics => self.view_settings_diagnostics_tab(p),
        };

        let footer_bar = row![
            container(Space::new(7, 7)).style(theme::dot_box(p.success_fg)),
            text("Changes are saved automatically")
                .size(type_scale::CAPTION)
                .color(p.text_muted),
            Space::with_width(Length::Fill),
            text("Esc to close")
                .size(type_scale::CAPTION)
                .color(p.text_muted),
            button(text("Done").size(type_scale::BODY))
                .padding([6, 16])
                .style(theme::primary_btn(p))
                .on_press(Message::CloseOverlays),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .padding([10, 22]);

        let right_pane = column![
            page_header,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            container(
                slim_scroll(
                    container(page_content)
                        .padding([16, 22])
                        .width(Length::Fill),
                    p,
                )
                .height(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill),
            horizontal_rule(1).style(theme::subtle_rule(p)),
            footer_bar,
        ]
        .width(Length::Fill)
        .height(Length::Fill);

        let modal_card = opaque(
            container(
                row![
                    left_rail,
                    vertical_rule(1).style(theme::subtle_rule(p)),
                    right_pane,
                ]
                .width(Length::Fill)
                .height(Length::Fill),
            )
            .width(Length::Fixed(860.0))
            .height(Length::Fixed(590.0))
            .style(theme::modal_box(p)),
        );

        let positioned = container(modal_card)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .padding(Padding::from([24, 24]))
            .style(theme::backdrop_box);

        theme::overlay_backdrop(positioned, Message::CloseOverlays)
    }

    fn view_settings_general_tab(
        &self,
        p: Palette,
        s: &gitfleet_core::AppSettings,
    ) -> Element<'_, Message> {
        let theme_cards = row![
            theme_preview_card(
                p,
                ThemeMode::Dark,
                self.theme_mode == ThemeMode::Dark,
                "Dark",
                "Low-glare deep slate",
            ),
            theme_preview_card(
                p,
                ThemeMode::Light,
                self.theme_mode == ThemeMode::Light,
                "Light",
                "Crisp high-contrast daylight",
            ),
        ]
        .spacing(space::MD)
        .width(Length::Fill);

        let mut parallel_btns = row![].spacing(2).align_y(Alignment::Center);
        for n in [2usize, 4, 6, 8, 12] {
            parallel_btns = parallel_btns.push(
                button(text(format!("{n}")).size(type_scale::CAPTION))
                    .padding([4, 10])
                    .style(theme::segmented_btn(p, s.max_parallel_bulk_ops == n))
                    .on_press(Message::SetMaxParallelBulkOps(n)),
            );
        }

        let diff_and_perf = grouped_card(
            p,
            vec![
                setting_row(
                    p,
                    "Compact Diff Context",
                    "Collapse unchanged code runs into expandable bars in Unified & Side-by-Side diffs.",
                    theme::toggle_switch(p, self.diff_compact, Message::ToggleDiffCompact),
                ),
                setting_row(
                    p,
                    "Parallel Bulk Workers",
                    "Maximum concurrent Git operations during workspace Fetch / Pull / Push.",
                    theme::segmented_group(p, parallel_btns),
                ),
            ],
        );

        column![
            column![setting_group_title(p, "APPEARANCE"), theme_cards].spacing(space::SM),
            column![setting_group_title(p, "DIFF & PERFORMANCE"), diff_and_perf]
                .spacing(space::SM),
        ]
        .spacing(space::XL)
        .width(Length::Fill)
        .into()
    }

    fn view_settings_terminal_tab(
        &self,
        p: Palette,
        s: &gitfleet_core::AppSettings,
    ) -> Element<'_, Message> {
        let profiles = self.service.terminal.profiles();
        let default_pid = self.default_terminal_profile_id();
        let active_profile_label = profiles
            .iter()
            .find(|pr| pr.id == default_pid)
            .map(|pr| pr.label.clone())
            .unwrap_or_else(|| default_pid.clone());
        let active_font = self.active_terminal_font();
        let preview_size = (s.terminal_font_size as f32).clamp(12.0, 18.0);

        // Live Terminal Preview Card
        let term_preview = container(
            column![
                container(
                    row![
                        container(Space::new(8, 8)).style(theme::dot_box(DARK.danger_fg)),
                        container(Space::new(8, 8)).style(theme::dot_box(DARK.warning_fg)),
                        container(Space::new(8, 8)).style(theme::dot_box(DARK.success_fg)),
                        Space::with_width(Length::Fixed(6.0)),
                        text(format!(
                            "{active_profile_label}  ·  {active_font}  ·  {}pt",
                            s.terminal_font_size
                        ))
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(DARK.text_muted),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([6, 12])
                .width(Length::Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(Color::from_rgb(0.086, 0.110, 0.137))),
                    ..Default::default()
                }),
                container(
                    column![
                        row![
                            text("~/projects/gitfleet")
                                .size(preview_size)
                                .font(theme::CODE_FONT)
                                .color(DARK.accent_fg),
                            text(" main")
                                .size(preview_size)
                                .font(theme::CODE_FONT)
                                .color(DARK.success_fg),
                            text(" $ git status --short")
                                .size(preview_size)
                                .font(theme::CODE_FONT)
                                .color(DARK.text_primary),
                        ]
                        .spacing(0),
                        row![
                            text(" M ")
                                .size(preview_size)
                                .font(theme::CODE_FONT)
                                .color(DARK.warning_fg),
                            text("src/views/diff_viewer.rs    0O 1lI != => ->")
                                .size(preview_size)
                                .font(theme::CODE_FONT)
                                .color(DARK.text_secondary),
                        ]
                        .spacing(0),
                    ]
                    .spacing(3),
                )
                .padding([10, 12])
                .width(Length::Fill),
            ]
            .spacing(0),
        )
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgb(0.055, 0.075, 0.098))),
            border: Border {
                color: DARK.border_default,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        });

        // 2-Column Uniform Shell Profile Grid
        let mut prof_grid = column![].spacing(space::XS).width(Length::Fill);
        let mut prof_iter = profiles.iter();
        while let Some(left_p) = prof_iter.next() {
            let mk_prof_card = |prof: &gitfleet_terminal::TerminalProfile| -> Element<'_, Message> {
                let selected = prof.id == default_pid;
                button(
                    row![
                        icons::terminal(
                            14.0,
                            if selected { p.accent_fg } else { p.text_muted }
                        ),
                        column![
                            text(prof.label.clone())
                                .size(type_scale::BODY)
                                .color(p.text_primary)
                                .wrapping(text::Wrapping::None),
                            text(prof.id.clone())
                                .size(type_scale::CAPTION)
                                .font(theme::CODE_FONT)
                                .color(p.text_muted)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(1)
                        .width(Length::Fill),
                        if selected {
                            Element::from(icons::check(13.0, p.accent_fg))
                        } else {
                            Space::with_width(Length::Shrink).into()
                        },
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding([8, 11])
                .width(Length::FillPortion(1))
                .style(theme::choice_card_btn(p, selected))
                .on_press(Message::SetDefaultTerminalProfile(prof.id.clone()))
                .into()
            };

            let r = if let Some(right_p) = prof_iter.next() {
                row![mk_prof_card(left_p), mk_prof_card(right_p)]
                    .spacing(space::SM)
                    .width(Length::Fill)
            } else {
                row![
                    mk_prof_card(left_p),
                    Space::with_width(Length::FillPortion(1)),
                ]
                .spacing(space::SM)
                .width(Length::Fill)
            };
            prof_grid = prof_grid.push(r);
        }

        // Segmented Font Size Control
        let mut size_btns = row![].spacing(2).align_y(Alignment::Center);
        for sz in [12u16, 13, 14, 15, 16, 18] {
            size_btns = size_btns.push(
                button(text(format!("{sz}pt")).size(type_scale::CAPTION))
                    .padding([4, 9])
                    .style(theme::segmented_btn(p, s.terminal_font_size == sz))
                    .on_press(Message::SetTerminalFontSize(sz)),
            );
        }

        // Uniform 2-Column Searchable Font Family Picker
        let configured_font = s.terminal_font_family.as_deref();
        let fq = self.settings_font_filter.trim().to_lowercase();

        let mut font_choices: Vec<(Option<String>, String, Option<&'static str>)> = Vec::new();
        if fq.is_empty()
            || "auto".contains(&fq)
            || self.terminal_fonts.default_font.to_lowercase().contains(&fq)
        {
            font_choices.push((
                None,
                format!("Auto ({})", self.terminal_fonts.default_font),
                Some("Auto"),
            ));
        }
        for fam in &self.terminal_fonts.available {
            if fq.is_empty() || fam.to_lowercase().contains(&fq) {
                font_choices.push((Some(fam.clone()), fam.clone(), None));
            }
        }

        let mut font_grid = column![].spacing(space::XS).width(Length::Fill);
        let visible_fonts: Vec<_> = font_choices.into_iter().take(24).collect();
        let mut f_iter = visible_fonts.into_iter();
        while let Some((val_l, label_l, tag_l)) = f_iter.next() {
            let mk_font_card =
                |val: Option<String>, label: String, tag: Option<&'static str>| -> Element<'_, Message> {
                    let selected = match (&val, configured_font) {
                        (None, None) => true,
                        (Some(v), Some(c)) => v.eq_ignore_ascii_case(c),
                        _ => false,
                    };
                    let mut r = row![
                        theme::clipped_line(
                            text(label)
                                .size(type_scale::CAPTION)
                                .font(theme::CODE_FONT)
                                .color(if selected { p.text_primary } else { p.text_secondary }),
                            Length::Fill,
                        ),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center);
                    if let Some(t) = tag {
                        r = r.push(theme::badge(
                            t,
                            p.fill_selected,
                            p.accent_fg,
                            p.border_subtle,
                            false,
                        ));
                    }
                    if selected {
                        r = r.push(icons::check(12.0, p.accent_fg));
                    }
                    button(r)
                        .padding([7, 10])
                        .width(Length::FillPortion(1))
                        .style(theme::choice_card_btn(p, selected))
                        .on_press(Message::SetTerminalFontFamily(val))
                        .into()
                };

            let r = if let Some((val_r, label_r, tag_r)) = f_iter.next() {
                row![
                    mk_font_card(val_l, label_l, tag_l),
                    mk_font_card(val_r, label_r, tag_r),
                ]
                .spacing(space::SM)
                .width(Length::Fill)
            } else {
                row![
                    mk_font_card(val_l, label_l, tag_l),
                    Space::with_width(Length::FillPortion(1)),
                ]
                .spacing(space::SM)
                .width(Length::Fill)
            };
            font_grid = font_grid.push(r);
        }

        if self.terminal_fonts.available.is_empty() && !fq.is_empty() {
            font_grid = font_grid.push(
                text("No installed fonts match this filter.")
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
            );
        }

        let font_section_card = grouped_card(
            p,
            vec![
                setting_row(
                    p,
                    "Font Size",
                    "Adjust embedded PTY cell scale (12pt – 18pt).",
                    theme::segmented_group(p, size_btns),
                ),
                container(
                    column![
                        row![
                            column![
                                text("Font Family")
                                    .size(type_scale::BODY)
                                    .color(p.text_primary),
                                text("Nerd Fonts & monospace families detected on this machine.")
                                    .size(type_scale::CAPTION)
                                    .color(p.text_muted),
                            ]
                            .spacing(2)
                            .width(Length::Fill),
                            text_input("Filter fonts…", &self.settings_font_filter)
                                .on_input(Message::SettingsFontFilterChanged)
                                .padding([5, 9])
                                .size(type_scale::CAPTION)
                                .width(Length::Fixed(180.0))
                                .style(theme::dark_input(p)),
                        ]
                        .spacing(space::SM)
                        .align_y(Alignment::Center),
                        font_grid,
                    ]
                    .spacing(space::SM),
                )
                .padding([11, 14])
                .width(Length::Fill)
                .into(),
            ],
        );

        column![
            term_preview,
            column![setting_group_title(p, "DEFAULT SHELL PROFILE"), prof_grid].spacing(space::SM),
            column![setting_group_title(p, "TYPOGRAPHY"), font_section_card].spacing(space::SM),
        ]
        .spacing(space::LG)
        .width(Length::Fill)
        .into()
    }

    fn view_settings_automation_tab(
        &self,
        p: Palette,
        s: &gitfleet_core::AppSettings,
    ) -> Element<'_, Message> {
        let interval = s.auto_fetch_interval_secs.unwrap_or(300);

        let mut interval_btns = row![].spacing(2).align_y(Alignment::Center);
        for (secs, lbl) in [(60u64, "1 min"), (300, "5 min"), (900, "15 min"), (1800, "30 min")] {
            interval_btns = interval_btns.push(
                button(text(lbl).size(type_scale::CAPTION))
                    .padding([4, 10])
                    .style(theme::segmented_btn(p, interval == secs))
                    .on_press(Message::SetFetchInterval(secs)),
            );
        }

        let mut threshold_btns = row![].spacing(2).align_y(Alignment::Center);
        for mb in [1u64, 5, 10, 20, 50] {
            threshold_btns = threshold_btns.push(
                button(text(format!("{mb} MB")).size(type_scale::CAPTION))
                    .padding([4, 9])
                    .style(theme::segmented_btn(p, s.oversized_threshold_mb == mb))
                    .on_press(Message::SetOversizedThreshold(mb)),
            );
        }

        let fetch_sub = if s.scheduled_fetch_enabled {
            "Interval between automatic workspace fetch cycles."
        } else {
            "Enable Scheduled Background Fetch above to run periodically."
        };

        let sync_card = grouped_card(
            p,
            vec![
                setting_row(
                    p,
                    "Watch Active Repository Filesystem",
                    "Automatically refresh status & vault indicators when files change on disk (notify).",
                    theme::toggle_switch(p, s.watch_filesystem, Message::ToggleFsWatcher(!s.watch_filesystem)),
                ),
                setting_row(
                    p,
                    "Scheduled Background Fetch",
                    "Periodically run `git fetch` across available workspace repositories.",
                    theme::toggle_switch(
                        p,
                        s.scheduled_fetch_enabled,
                        Message::ToggleScheduledFetch(!s.scheduled_fetch_enabled),
                    ),
                ),
                setting_row(
                    p,
                    "Fetch Interval",
                    fetch_sub,
                    theme::segmented_group(p, interval_btns),
                ),
            ],
        );

        let hygiene_card = grouped_card(
            p,
            vec![setting_row(
                p,
                "Oversized File Warning Threshold",
                "Highlight staged binaries or assets exceeding this size before committing.",
                theme::segmented_group(p, threshold_btns),
            )],
        );

        column![
            column![setting_group_title(p, "SYNC & WATCHERS"), sync_card].spacing(space::SM),
            column![setting_group_title(p, "PRE-COMMIT HYGIENE"), hygiene_card].spacing(space::SM),
        ]
        .spacing(space::XL)
        .width(Length::Fill)
        .into()
    }

    fn view_settings_diagnostics_tab(&self, p: Palette) -> Element<'_, Message> {
        let ws = self.service.workspace.current();
        let s = self.service.settings();
        let has_repo = self.current_repo().is_some();
        let channel_label = self.service.updater.channel().label();
        let update_subtitle = match &self.update_status {
            crate::app::UpdateUiStatus::Checking => {
                format!("Checking GitHub Releases…  ·  {channel_label}")
            }
            crate::app::UpdateUiStatus::UpToDate => format!(
                "v{} is up to date  ·  {channel_label}",
                env!("CARGO_PKG_VERSION")
            ),
            crate::app::UpdateUiStatus::Available { release } => format!(
                "v{} available (installed: v{})  ·  {channel_label}",
                release.version,
                env!("CARGO_PKG_VERSION")
            ),
            crate::app::UpdateUiStatus::Applying { phase, .. } => {
                format!("{}  ·  {channel_label}", phase.label())
            }
            crate::app::UpdateUiStatus::AppliedManual { release, .. } => {
                format!("v{} downloaded  ·  {channel_label}", release.version)
            }
            crate::app::UpdateUiStatus::Error(err) => {
                format!("Last check failed: {}  ·  {channel_label}", truncate_chars(err, 48))
            }
            crate::app::UpdateUiStatus::Idle => format!(
                "GitFleet v{}  ·  {channel_label}",
                env!("CARGO_PKG_VERSION")
            ),
        };

        let mut update_actions = row![].spacing(space::XS).align_y(Alignment::Center);
        if let Some(skipped) = &s.skipped_update_version {
            update_actions = update_actions.push(
                button(
                    text(format!("Unskip v{skipped}"))
                        .size(type_scale::CAPTION)
                        .wrapping(text::Wrapping::None),
                )
                .padding([6, 10])
                .style(theme::subtle_btn(p))
                .on_press(Message::ClearSkippedUpdate),
            );
        }
        if let Some(rel) = self.update_status.release() {
            update_actions = update_actions.push(
                button(
                    row![
                        icons::sparkle(13.0, Color::WHITE),
                        text(format!("View v{}…", rel.version))
                            .size(type_scale::CAPTION)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([6, 12])
                .style(theme::primary_btn(p))
                .on_press(Message::OpenUpdateModal),
            );
        } else {
            let checking = self.update_status.is_busy();
            update_actions = update_actions.push(
                button(
                    row![
                        icons::refresh(
                            13.0,
                            if checking { p.text_muted } else { p.text_primary }
                        ),
                        text(if checking {
                            "Checking…"
                        } else {
                            "Check for Updates"
                        })
                        .size(type_scale::CAPTION)
                        .wrapping(text::Wrapping::None),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([6, 12])
                .style(theme::subtle_btn(p))
                .on_press_maybe((!checking).then_some(Message::CheckForUpdates { manual: true })),
            );
        }

        let updates_card = grouped_card(
            p,
            vec![
                setting_row(
                    p,
                    "Check for updates automatically",
                    "Silently check GitHub Releases at startup (at most once every 12 hours, with SHA-256 verification).",
                    theme::toggle_switch(
                        p,
                        s.auto_check_updates,
                        Message::ToggleAutoCheckUpdates(!s.auto_check_updates),
                    ),
                ),
                row![
                    column![
                        text("Software Updates")
                            .size(type_scale::BODY)
                            .color(p.text_primary),
                        text(update_subtitle)
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                    update_actions,
                ]
                .spacing(space::MD)
                .align_y(Alignment::Center)
                .padding([11, 14])
                .width(Length::Fill)
                .into(),
            ],
        );

        // 2-Column Environment Summary Tiles
        let env_tiles = row![
            container(
                column![
                    row![
                        text("Git CLI")
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                        Space::with_width(Length::Fill),
                        theme::badge(
                            "Detected",
                            p.success_bg,
                            p.success_fg,
                            p.border_subtle,
                            false,
                        ),
                    ]
                    .align_y(Alignment::Center),
                    text(self.git_version.clone())
                        .size(type_scale::BODY)
                        .font(theme::CODE_FONT)
                        .color(p.text_primary),
                ]
                .spacing(6),
            )
            .padding(12)
            .width(Length::FillPortion(1))
            .style(theme::section_panel_box(p)),
            container(
                column![
                    row![
                        text("Workspace & Host")
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                        Space::with_width(Length::Fill),
                        theme::badge(
                            format!("{} / {}", std::env::consts::OS, std::env::consts::ARCH),
                            p.fill_selected,
                            p.accent_fg,
                            p.border_subtle,
                            true,
                        ),
                    ]
                    .align_y(Alignment::Center),
                    text(format!("{}  ·  {} repo(s)", ws.name, ws.repos.len()))
                        .size(type_scale::BODY)
                        .color(p.text_primary),
                ]
                .spacing(6),
            )
            .padding(12)
            .width(Length::FillPortion(1))
            .style(theme::section_panel_box(p)),
        ]
        .spacing(space::MD)
        .width(Length::Fill);

        // Detected Tools List Card
        let tools_header = row![
            setting_group_title(p, "EXTERNAL DIFF & EDITOR TOOLS"),
            Space::with_width(Length::Fill),
            button(
                row![
                    icons::refresh(12.0, p.text_secondary),
                    text("Rescan PATH").size(type_scale::CAPTION),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding([3, 9])
            .style(theme::subtle_btn(p))
            .on_press(Message::RescanExternalTools),
        ]
        .align_y(Alignment::Center);

        let tools_card: Element<'_, Message> = if self.detected_tools.is_empty() {
            container(
                row![
                    icons::wrench(16.0, p.text_muted),
                    text("No external diff or editor binaries detected on PATH.")
                        .size(type_scale::BODY)
                        .color(p.text_muted),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding([14, 16])
            .width(Length::Fill)
            .style(theme::section_panel_box(p))
            .into()
        } else {
            let mut rows = Vec::with_capacity(self.detected_tools.len());
            for t in &self.detected_tools {
                let path_str = t.path.display().to_string();
                rows.push(
                    row![
                        container(Space::new(7, 7)).style(theme::dot_box(p.success_fg)),
                        text(t.name.clone())
                            .size(type_scale::BODY)
                            .color(p.text_primary)
                            .wrapping(text::Wrapping::None),
                        theme::badge(
                            t.command.clone(),
                            p.fill_selected,
                            p.accent_fg,
                            p.border_subtle,
                            true,
                        ),
                        theme::clipped_line(
                            text(path_str.clone())
                                .size(type_scale::CAPTION)
                                .font(theme::CODE_FONT)
                                .color(p.text_muted),
                            Length::Fill,
                        ),
                        button(icons::copy(12.0, p.text_secondary))
                            .padding([4, 6])
                            .style(theme::subtle_btn(p))
                            .on_press(Message::CopyText(t.name.clone(), path_str)),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center)
                    .padding([9, 14])
                    .width(Length::Fill)
                    .into(),
                );
            }
            grouped_card(p, rows)
        };

        // Troubleshooting & Export Actions
        let actions_card = grouped_card(
            p,
            vec![
                setting_row(
                    p,
                    "Diagnostics Report (JSON)",
                    "Export settings, detected tools, Git version, and recent command log for support.",
                    button(
                        row![
                            icons::save(13.0, Color::WHITE),
                            text("Export JSON…").size(type_scale::CAPTION),
                        ]
                        .spacing(6)
                        .align_y(Alignment::Center),
                    )
                    .padding([6, 12])
                    .style(theme::primary_btn(p))
                    .on_press(Message::PickExportDiagnostics),
                ),
                setting_row(
                    p,
                    "Active Repository Directory",
                    "Reveal the currently selected repository root in your system file explorer.",
                    button(
                        row![
                            icons::folder_open(
                                13.0,
                                if has_repo { p.text_primary } else { p.text_muted }
                            ),
                            text("Open Folder").size(type_scale::CAPTION),
                        ]
                        .spacing(6)
                        .align_y(Alignment::Center),
                    )
                    .padding([6, 12])
                    .style(theme::subtle_btn(p))
                    .on_press_maybe(has_repo.then_some(Message::RevealSelectedInExplorer)),
                ),
            ],
        );

        column![
            column![setting_group_title(p, "UPDATES"), updates_card].spacing(space::SM),
            column![setting_group_title(p, "ENVIRONMENT"), env_tiles].spacing(space::SM),
            column![tools_header, tools_card].spacing(space::SM),
            column![setting_group_title(p, "TROUBLESHOOTING"), actions_card].spacing(space::SM),
        ]
        .spacing(space::LG)
        .width(Length::Fill)
        .into()
    }
}
