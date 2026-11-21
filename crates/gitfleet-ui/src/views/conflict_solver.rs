use crate::app::{BottomViewMode, GitFleetApp, Message};
use crate::icons;
use crate::theme::{self, Palette, icon_size, space, type_scale};
use crate::views::tip;
use gitfleet_git::{ConflictHunk, ConflictSegment, ConflictStrategy, HunkChoice};
use iced::widget::{
    Space, button, column, container, row, scrollable, text, text_editor, tooltip,
};
use iced::{Alignment, Background, Border, Color, Element, Length};

fn card_border_style(
    bg: Color,
    border_color: Color,
    radius: f32,
) -> impl Fn(&iced::Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: radius.into(),
        },
        ..Default::default()
    }
}

fn render_code_lines<'a>(
    lines: &[String],
    text_color: Color,
    empty_hint: &'static str,
    p: Palette,
) -> Element<'a, Message> {
    if lines.is_empty() {
        return container(
            text(empty_hint)
                .size(type_scale::CAPTION)
                .color(p.text_muted),
        )
        .padding([6, 8])
        .width(Length::Fill)
        .into();
    }

    let mut col = column![].spacing(1).width(Length::Fill);
    for (idx, line) in lines.iter().enumerate() {
        let sanitized = theme::sanitize_code_line(line);
        col = col.push(
            row![
                text(format!("{:>3}", idx + 1))
                    .size(type_scale::CAPTION)
                    .font(theme::CODE_FONT)
                    .color(p.text_muted)
                    .width(Length::Fixed(30.0))
                    .wrapping(text::Wrapping::None),
                text(if sanitized.is_empty() {
                    " ".to_string()
                } else {
                    sanitized
                })
                .size(type_scale::CODE)
                .font(theme::CODE_FONT)
                .color(text_color)
                .width(Length::Fill),
            ]
            .spacing(6)
            .align_y(Alignment::Start),
        );
    }
    container(col)
        .padding([6, 8])
        .width(Length::Fill)
        .into()
}

fn render_common_lines<'a>(
    start_line: usize,
    lines: &[String],
    p: Palette,
) -> Element<'a, Message> {
    let mut col = column![].spacing(1).width(Length::Fill);
    for (offset, line) in lines.iter().enumerate() {
        let sanitized = theme::sanitize_code_line(line);
        col = col.push(
            row![
                text(format!("{:>4}", start_line + offset))
                    .size(type_scale::CAPTION)
                    .font(theme::CODE_FONT)
                    .color(p.text_muted)
                    .width(Length::Fixed(38.0))
                    .wrapping(text::Wrapping::None),
                text(if sanitized.is_empty() {
                    " ".to_string()
                } else {
                    sanitized
                })
                .size(type_scale::CODE)
                .font(theme::CODE_FONT)
                .color(p.text_secondary)
                .width(Length::Fill),
            ]
            .spacing(8)
            .align_y(Alignment::Start),
        );
    }
    container(col).padding([2, 6]).width(Length::Fill).into()
}

impl GitFleetApp {
    /// Interactive SmartGit-style Conflict Solver rendered in the bottom viewer when a conflicted
    /// file is selected. Supports hunk-by-hunk Ours / Theirs / Both / Custom editing, diff3 base
    /// inspection, Prev/Next conflict navigation, and one-click Save & Mark Resolved (`git add`).
    pub(crate) fn view_conflict_solver(&self, p: Palette) -> Element<'_, Message> {
        let Some(rel_path) = self.conflict_file_path.as_deref() else {
            return theme::empty_state(
                p,
                icons::check(icon_size::LG, p.success_fg),
                "No conflicted file selected",
                "Select a conflicted file in the Files pane to open the interactive Conflict Solver.",
                None,
            );
        };

        let total = self.conflict_parsed.conflict_count();
        let unresolved = self.conflict_parsed.unresolved_count();
        let all_resolved = total > 0 && unresolved == 0;

        let status_badge = if total == 0 {
            theme::badge(
                "No conflict markers",
                p.warning_bg,
                p.warning_fg,
                p.border_subtle,
                false,
            )
        } else if all_resolved {
            theme::badge(
                format!("All {total} conflict(s) resolved"),
                p.success_bg,
                p.success_fg,
                p.border_subtle,
                false,
            )
        } else {
            theme::badge(
                format!("{unresolved} / {total} unresolved"),
                p.danger_bg,
                p.danger_fg,
                p.border_subtle,
                false,
            )
        };

        let mut solver_bar = row![status_badge].spacing(6).align_y(Alignment::Center);

        if total > 0 {
            solver_bar = solver_bar
                .push(tip(
                    button(icons::push(11.0, p.text_secondary))
                        .padding([4, 6])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::ConflictJump(-1)),
                    "Previous Unresolved Conflict",
                    tooltip::Position::Bottom,
                    p,
                ))
                .push(tip(
                    button(icons::pull(11.0, p.text_secondary))
                        .padding([4, 6])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::ConflictJump(1)),
                    "Next Unresolved Conflict",
                    tooltip::Position::Bottom,
                    p,
                ))
                .push(tip(
                    button(text("All Ours").size(type_scale::CAPTION).wrapping(text::Wrapping::None))
                        .padding([3, 8])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::ConflictResolveAll(HunkChoice::Ours)),
                    "Accept Ours (current branch) for every conflict block",
                    tooltip::Position::Bottom,
                    p,
                ))
                .push(tip(
                    button(text("All Theirs").size(type_scale::CAPTION).wrapping(text::Wrapping::None))
                        .padding([3, 8])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::ConflictResolveAll(HunkChoice::Theirs)),
                    "Accept Theirs (incoming branch) for every conflict block",
                    tooltip::Position::Bottom,
                    p,
                ))
                .push(tip(
                    button(text("All Both").size(type_scale::CAPTION).wrapping(text::Wrapping::None))
                        .padding([3, 8])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::ConflictResolveAll(HunkChoice::BothOursFirst)),
                    "Keep Both (Ours followed by Theirs) for every conflict block",
                    tooltip::Position::Bottom,
                    p,
                ));
        }

        solver_bar = solver_bar.push(Space::with_width(Length::Fill));

        if total > 0 {
            solver_bar = solver_bar
                .push(tip(
                    button(text("Save Draft").size(type_scale::CAPTION).wrapping(text::Wrapping::None))
                        .padding([4, 9])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::SaveResolvedConflict {
                            mark_resolved: false,
                        }),
                    "Write current resolutions to disk without marking resolved",
                    tooltip::Position::Bottom,
                    p,
                ))
                .push(tip(
                    button(
                        row![
                            icons::check(icon_size::XS, Color::WHITE),
                            text("Save & Mark Resolved")
                                .size(type_scale::CAPTION)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                    )
                    .padding([4, 11])
                    .style(theme::primary_btn(p))
                    .on_press_maybe(all_resolved.then_some(Message::SaveResolvedConflict {
                        mark_resolved: true,
                    })),
                    if all_resolved {
                        "Write resolved file to disk and stage it (git add)"
                    } else {
                        "Resolve all remaining conflicts first to enable Save & Mark Resolved"
                    },
                    tooltip::Position::Bottom,
                    p,
                ));
        } else {
            // File is marked conflicted in Git index (e.g. modify/delete, binary, or manually edited)
            let path_owned = rel_path.to_string();
            solver_bar = solver_bar
                .push(
                    button(text("Keep Ours (File)").size(type_scale::CAPTION).wrapping(text::Wrapping::None))
                        .padding([4, 9])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::ResolveConflict(
                            path_owned.clone(),
                            ConflictStrategy::Ours,
                        )),
                )
                .push(
                    button(text("Keep Theirs (File)").size(type_scale::CAPTION).wrapping(text::Wrapping::None))
                        .padding([4, 9])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::ResolveConflict(
                            path_owned.clone(),
                            ConflictStrategy::Theirs,
                        )),
                )
                .push(
                    button(
                        row![
                            icons::check(icon_size::XS, Color::WHITE),
                            text("Mark Resolved").size(type_scale::CAPTION).wrapping(text::Wrapping::None),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                    )
                    .padding([4, 11])
                    .style(theme::primary_btn(p))
                    .on_press(Message::ResolveConflict(
                        path_owned,
                        ConflictStrategy::MarkResolved,
                    )),
                );
        }

        let bar_container = container(solver_bar)
            .padding([5, 8])
            .width(Length::Fill)
            .style(theme::section_header_box(p));

        if total == 0 {
            let empty_info = container(
                column![
                    row![
                        icons::alert_triangle(16.0, p.warning_fg),
                        text("No inline conflict markers found in this file")
                            .size(type_scale::SUBHEADING)
                            .color(p.text_primary),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                    text(
                        "This file may be a delete/modify conflict, a binary conflict, or all conflict markers have already been edited manually. Choose Keep Ours, Keep Theirs, inspect in Edit mode, or Mark Resolved."
                    )
                    .size(type_scale::CAPTION)
                    .color(p.text_secondary),
                    row![
                        button(text("Open in Editor").size(type_scale::CAPTION))
                            .padding([4, 10])
                            .style(theme::subtle_btn(p))
                            .on_press(Message::SetBottomViewMode(BottomViewMode::Edit)),
                    ]
                    .spacing(space::SM),
                ]
                .spacing(space::SM),
            )
            .padding(14)
            .width(Length::Fill)
            .style(theme::section_panel_box(p));

            return column![bar_container, empty_info]
                .spacing(6)
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        }

        let mut content = column![].spacing(6).width(Length::Fill);

        for (seg_idx, seg) in self.conflict_parsed.segments.iter().enumerate() {
            match seg {
                ConflictSegment::Common { start_line, lines } => {
                    if lines.len() > 10 && !self.conflict_expanded_segments.contains(&seg_idx) {
                        let hidden_cnt = lines.len() - 6;
                        content = content
                            .push(render_common_lines(*start_line, &lines[..3], p))
                            .push(
                                button(
                                    text(format!("⋯ Show {hidden_cnt} unchanged lines ⋯"))
                                        .size(type_scale::CAPTION)
                                        .color(p.text_muted),
                                )
                                .width(Length::Fill)
                                .padding([3, 10])
                                .style(theme::toolbar_btn(p, false))
                                .on_press(Message::ConflictToggleContextSegment(seg_idx)),
                            )
                            .push(render_common_lines(
                                *start_line + lines.len() - 3,
                                &lines[lines.len() - 3..],
                                p,
                            ));
                    } else {
                        content = content.push(render_common_lines(*start_line, lines, p));
                    }
                }
                ConflictSegment::Conflict(hunk) => {
                    content = content.push(self.view_conflict_hunk_card(hunk, total, p));
                }
            }
        }

        let scroll = scrollable(container(content).padding([4, 6]).width(Length::Fill))
            .id(self.conflict_scroll_id.clone())
            .width(Length::Fill)
            .height(Length::Fill)
            .direction(theme::slim_vertical_direction())
            .style(theme::slim_scrollable(p));

        column![bar_container, scroll]
            .spacing(6)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_conflict_hunk_card<'a>(
        &'a self,
        hunk: &'a ConflictHunk,
        total_hunks: usize,
        p: Palette,
    ) -> Element<'a, Message> {
        let is_active = self.conflict_active_hunk == Some(hunk.id);
        let is_editing = self.conflict_editing_hunk == Some(hunk.id);
        let resolved = hunk.choice.is_resolved();

        let card_border = if is_active {
            p.accent_primary
        } else if resolved {
            Color {
                a: 0.55,
                ..p.success_fg
            }
        } else {
            Color {
                a: 0.65,
                ..p.danger_fg
            }
        };

        let status_pill = if resolved {
            theme::badge(
                format!("✓ {}", hunk.choice.label()),
                p.success_bg,
                p.success_fg,
                p.border_subtle,
                false,
            )
        } else {
            theme::badge(
                "Unresolved",
                p.danger_bg,
                p.danger_fg,
                p.border_subtle,
                false,
            )
        };

        let choice_btn = |label: &'static str, choice: HunkChoice, hint: &'static str| {
            let active = hunk.choice == choice && !is_editing;
            tip(
                button(text(label).size(type_scale::CAPTION).wrapping(text::Wrapping::None))
                    .padding([3, 8])
                    .style(theme::filter_pill_btn(p, active))
                    .on_press(Message::ConflictSetHunkChoice(hunk.id, choice)),
                hint,
                tooltip::Position::Top,
                p,
            )
        };

        let mut hdr = row![
            theme::badge(
                format!("Conflict #{} / {total_hunks}", hunk.id + 1),
                p.surface_card,
                p.text_primary,
                p.border_subtle,
                false,
            ),
            text(format!("Line {}", hunk.start_line))
                .size(type_scale::BADGE)
                .font(theme::CODE_FONT)
                .color(p.text_muted),
            status_pill,
            Space::with_width(Length::Fill),
            choice_btn("Accept Ours", HunkChoice::Ours, "Keep current branch changes"),
            choice_btn(
                "Accept Theirs",
                HunkChoice::Theirs,
                "Keep incoming branch changes"
            ),
            choice_btn(
                "Accept Both",
                HunkChoice::BothOursFirst,
                "Keep Ours followed by Theirs"
            ),
            choice_btn(
                "Theirs + Ours",
                HunkChoice::BothTheirsFirst,
                "Keep Theirs followed by Ours"
            ),
            tip(
                button(
                    text(if is_editing { "Done Edit" } else { "Edit" })
                        .size(type_scale::CAPTION)
                        .wrapping(text::Wrapping::None),
                )
                .padding([3, 8])
                .style(theme::filter_pill_btn(
                    p,
                    is_editing || hunk.choice == HunkChoice::Custom,
                ))
                .on_press(Message::ConflictToggleCustomEdit(hunk.id)),
                "Edit merged output manually for this conflict block",
                tooltip::Position::Top,
                p,
            ),
        ]
        .spacing(5)
        .align_y(Alignment::Center);

        if resolved {
            hdr = hdr.push(tip(
                button(icons::undo(11.0, p.text_secondary))
                    .padding([3, 6])
                    .style(theme::subtle_btn(p))
                    .on_press(Message::ConflictSetHunkChoice(
                        hunk.id,
                        HunkChoice::Unresolved,
                    )),
                "Reset this conflict block back to unresolved",
                tooltip::Position::Top,
                p,
            ));
        }

        let header_box = container(hdr)
            .padding([5, 8])
            .width(Length::Fill)
            .style(theme::section_header_box(p));

        let ours_chosen = matches!(
            hunk.choice,
            HunkChoice::Ours | HunkChoice::BothOursFirst | HunkChoice::BothTheirsFirst
        );
        let theirs_chosen = matches!(
            hunk.choice,
            HunkChoice::Theirs | HunkChoice::BothOursFirst | HunkChoice::BothTheirsFirst
        );

        let ours_bg = Color {
            a: if ours_chosen { 0.16 } else { 0.08 },
            ..p.accent_primary
        };
        let ours_border = Color {
            a: if ours_chosen { 0.85 } else { 0.30 },
            ..p.accent_primary
        };
        let theirs_bg = Color {
            a: if theirs_chosen { 0.16 } else { 0.08 },
            ..p.violet_fg
        };
        let theirs_border = Color {
            a: if theirs_chosen { 0.85 } else { 0.30 },
            ..p.violet_fg
        };

        let ours_pane = container(
            column![
                container(
                    row![
                        container(Space::new(6, 6)).style(theme::dot_box(p.accent_fg)),
                        text(format!("Ours · {}", hunk.ours_label))
                            .size(type_scale::CAPTION)
                            .color(p.accent_fg),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([4, 8])
                .width(Length::Fill),
                render_code_lines(
                    &hunk.ours_lines,
                    if resolved && !ours_chosen {
                        p.text_muted
                    } else {
                        p.text_primary
                    },
                    "(empty on Ours)",
                    p,
                ),
            ]
            .width(Length::Fill),
        )
        .width(Length::FillPortion(1))
        .style(card_border_style(ours_bg, ours_border, 5.0));

        let theirs_pane = container(
            column![
                container(
                    row![
                        container(Space::new(6, 6)).style(theme::dot_box(p.violet_fg)),
                        text(format!("Theirs · {}", hunk.theirs_label))
                            .size(type_scale::CAPTION)
                            .color(p.violet_fg),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([4, 8])
                .width(Length::Fill),
                render_code_lines(
                    &hunk.theirs_lines,
                    if resolved && !theirs_chosen {
                        p.text_muted
                    } else {
                        p.text_primary
                    },
                    "(empty on Theirs)",
                    p,
                ),
            ]
            .width(Length::Fill),
        )
        .width(Length::FillPortion(1))
        .style(card_border_style(theirs_bg, theirs_border, 5.0));

        let mut body_col = column![header_box].spacing(6).width(Length::Fill);

        if let Some(base_lines) = &hunk.base_lines {
            let blabel = hunk.base_label.as_deref().unwrap_or("common ancestor");
            body_col = body_col.push(
                container(
                    column![
                        container(
                            text(format!("Base · {blabel}"))
                                .size(type_scale::BADGE)
                                .color(p.text_muted),
                        )
                        .padding([3, 8]),
                        render_code_lines(base_lines, p.text_muted, "(empty in Base)", p),
                    ]
                    .width(Length::Fill),
                )
                .width(Length::Fill)
                .style(card_border_style(p.surface_sidebar, p.border_subtle, 4.0)),
            );
        }

        body_col = body_col.push(
            row![ours_pane, theirs_pane]
                .spacing(6)
                .width(Length::Fill),
        );

        if is_editing {
            let editor = text_editor(&self.conflict_hunk_editor)
                .on_action(Message::ConflictCustomEditAction)
                .font(theme::CODE_FONT)
                .size(type_scale::CODE)
                .height(Length::Fixed(130.0))
                .style(theme::dark_editor(p));
            body_col = body_col.push(
                container(
                    column![
                        text("Manual Merged Result (editable)")
                            .size(type_scale::CAPTION)
                            .color(p.success_fg),
                        editor,
                    ]
                    .spacing(4)
                    .width(Length::Fill),
                )
                .padding(6)
                .width(Length::Fill)
                .style(card_border_style(
                    p.surface_sidebar,
                    Color {
                        a: 0.65,
                        ..p.success_fg
                    },
                    5.0,
                )),
            );
        } else if let Some(resolved_lines) = hunk.resolved_lines() {
            body_col = body_col.push(
                container(
                    column![
                        container(
                            row![
                                icons::check(11.0, p.success_fg),
                                text(format!("Result Preview · {}", hunk.choice.label()))
                                    .size(type_scale::CAPTION)
                                    .color(p.success_fg),
                            ]
                            .spacing(5)
                            .align_y(Alignment::Center),
                        )
                        .padding([4, 8]),
                        render_code_lines(
                            &resolved_lines,
                            p.text_primary,
                            "(empty result — conflict lines removed)",
                            p,
                        ),
                    ]
                    .width(Length::Fill),
                )
                .width(Length::Fill)
                .style(card_border_style(
                    Color {
                        a: 0.55,
                        ..p.success_bg
                    },
                    Color {
                        a: 0.55,
                        ..p.success_fg
                    },
                    5.0,
                )),
            );
        }

        container(body_col)
            .padding(6)
            .width(Length::Fill)
            .style(card_border_style(p.surface_panel, card_border, 6.0))
            .into()
    }
}
