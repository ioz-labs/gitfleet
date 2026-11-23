use crate::app::{ActiveCloneState, GitFleetApp, Message};
use crate::icons;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use crate::views::truncate_chars;
use iced::widget::{
    Space, button, checkbox, column, container, horizontal_rule, opaque, row, scrollable, text,
    text_input,
};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding};

fn format_elapsed_mm_ss(secs: u64) -> String {
    let mins = secs / 60;
    let rem = secs % 60;
    format!("{mins:02}:{rem:02}")
}

fn check_dest_conflict(dest: &std::path::Path) -> Option<String> {
    if dest.is_file() {
        return Some("Destination path already exists as a file.".into());
    }
    if dest.is_dir()
        && std::fs::read_dir(dest)
            .ok()
            .is_some_and(|mut it| it.next().is_some())
    {
        return Some("Destination folder already exists and is not empty.".into());
    }
    None
}

impl GitFleetApp {
    pub(crate) fn view_clone_modal(&self, p: Palette) -> Element<'_, Message> {
        if let Some(active) = &self.active_clone {
            return self.view_active_clone_modal(p, active);
        }

        let ws = self.service.workspace.current();
        let modal = &self.clone_modal;

        let url_trimmed = modal.url_input.trim();
        let parent_trimmed = modal.parent_dir_input.trim();
        let folder_trimmed = modal.folder_input.trim();
        let resolved_dest = modal.resolved_dest_path();
        let dest_conflict = resolved_dest.as_deref().and_then(check_dest_conflict);

        let can_submit = !url_trimmed.is_empty()
            && !parent_trimmed.is_empty()
            && !folder_trimmed.is_empty()
            && dest_conflict.is_none();

        let mut url_input = text_input(
            "https://github.com/org/repo.git or git@github.com:org/repo.git",
            &modal.url_input,
        )
        .id(self.clone_url_input_id.clone())
        .on_input(Message::CloneUrlChanged)
        .padding(pad::INPUT)
        .size(type_scale::BODY)
        .width(Length::Fill)
        .style(theme::dark_input(p));
        if can_submit {
            url_input = url_input.on_submit(Message::CloneRepoSubmitted);
        }

        let parent_row = row![
            text_input(
                "Choose or paste parent directory…",
                &modal.parent_dir_input
            )
            .on_input(Message::CloneParentDirChanged)
            .padding(pad::INPUT)
            .size(type_scale::BODY)
            .width(Length::Fill)
            .style(theme::dark_input(p)),
            button(
                row![
                    icons::folder_open(icon_size::XS, p.accent_fg),
                    text("Browse…").size(type_scale::BODY),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding(pad::BTN_MD)
            .style(theme::subtle_btn(p))
            .on_press(Message::PickCloneParent),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center);

        let mut folder_input = text_input("Auto-filled from URL", &modal.folder_input)
            .on_input(Message::CloneFolderChanged)
            .padding(pad::INPUT)
            .size(type_scale::BODY)
            .width(Length::Fill)
            .style(theme::dark_input(p));
        if can_submit {
            folder_input = folder_input.on_submit(Message::CloneRepoSubmitted);
        }

        let mut branch_input = text_input("Default branch", &modal.branch_input)
            .on_input(Message::CloneBranchChanged)
            .padding(pad::INPUT)
            .size(type_scale::BODY)
            .width(Length::Fill)
            .style(theme::dark_input(p));
        if can_submit {
            branch_input = branch_input.on_submit(Message::CloneRepoSubmitted);
        }

        let folder_and_branch_row = row![
            column![theme::field_label(p, "Target Folder Name"), folder_input]
                .spacing(space::XXS)
                .width(Length::FillPortion(3)),
            column![theme::field_label(p, "Branch / Tag (optional)"), branch_input]
                .spacing(space::XXS)
                .width(Length::FillPortion(2)),
        ]
        .spacing(space::MD);

        let dest_preview: Element<'_, Message> = if let Some(dest) = &resolved_dest {
            let dest_str = dest.display().to_string();
            if let Some(conflict_msg) = &dest_conflict {
                container(
                    column![
                        row![
                            icons::alert_triangle(icon_size::XS, p.warning_fg),
                            text(conflict_msg.clone())
                                .size(type_scale::CAPTION)
                                .color(p.warning_fg),
                        ]
                        .spacing(space::XS)
                        .align_y(Alignment::Center),
                        theme::clipped_line(
                            text(dest_str)
                                .size(type_scale::CAPTION)
                                .font(theme::CODE_FONT)
                                .color(p.text_muted),
                            Length::Fill,
                        ),
                    ]
                    .spacing(3),
                )
                .padding([7, 10])
                .width(Length::Fill)
                .style(theme::pill_box(p.warning_bg, p.warning_fg, p.border_default))
                .into()
            } else {
                container(
                    row![
                        text("Destination:")
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                        theme::clipped_line(
                            text(dest_str)
                                .size(type_scale::CAPTION)
                                .font(theme::CODE_FONT)
                                .color(p.accent_fg),
                            Length::Fill,
                        ),
                    ]
                    .spacing(space::XS)
                    .align_y(Alignment::Center),
                )
                .padding([6, 10])
                .width(Length::Fill)
                .style(theme::section_panel_box(p))
                .into()
            }
        } else {
            Space::with_height(Length::Shrink).into()
        };

        let options_row = row![
            checkbox(
                "Recurse submodules (--recurse-submodules)",
                modal.recurse_submodules
            )
            .size(14)
            .text_size(type_scale::CAPTION)
            .style(theme::tree_checkbox(p))
            .on_toggle(Message::CloneToggleSubmodules),
            checkbox("Shallow clone (--depth 1)", modal.depth_one)
                .size(14)
                .text_size(type_scale::CAPTION)
                .style(theme::tree_checkbox(p))
                .on_toggle(Message::CloneToggleDepthOne),
        ]
        .spacing(space::LG)
        .align_y(Alignment::Center);

        let cur_group_norm = modal.normalized_group();
        let mut group_col = column![
            theme::field_label(p, "Workspace Group (optional)"),
            text_input(
                "Root (no group) — or e.g. Backend/Services",
                &modal.group_input
            )
            .on_input(Message::CloneGroupChanged)
            .padding(pad::INPUT_COMPACT)
            .size(type_scale::BODY)
            .width(Length::Fill)
            .style(theme::dark_input(p)),
        ]
        .spacing(space::XXS);

        if !ws.groups.is_empty() {
            let mut sorted_groups = ws.groups.clone();
            sorted_groups.sort();
            sorted_groups.dedup();
            let mut chips = row![
                button(
                    text("Root (no group)")
                        .size(type_scale::BADGE)
                        .wrapping(text::Wrapping::None)
                )
                .padding([3, 8])
                .style(theme::filter_pill_btn(p, cur_group_norm.is_none()))
                .on_press(Message::CloneGroupChanged(String::new())),
            ]
            .spacing(4)
            .align_y(Alignment::Center);

            for g in sorted_groups {
                let active = cur_group_norm.as_deref() == Some(g.as_str());
                chips = chips.push(
                    button(
                        text(g.clone())
                            .size(type_scale::BADGE)
                            .wrapping(text::Wrapping::None),
                    )
                    .padding([3, 8])
                    .style(theme::filter_pill_btn(p, active))
                    .on_press(Message::CloneGroupChanged(g)),
                );
            }
            group_col = group_col.push(
                scrollable(chips)
                    .width(Length::Fill)
                    .direction(theme::slim_horizontal_direction())
                    .style(theme::slim_scrollable(p)),
            );
        }

        let mut body = column![
            column![theme::field_label(p, "Repository URL"), url_input].spacing(space::XXS),
            column![theme::field_label(p, "Parent Directory"), parent_row].spacing(space::XXS),
            folder_and_branch_row,
            dest_preview,
            options_row,
            group_col,
        ]
        .spacing(space::MD);

        if modal.cancelled_notice {
            body = body.push(
                container(
                    row![
                        icons::alert_triangle(icon_size::XS, p.warning_fg),
                        text("Clone cancelled — partial files on disk were cleaned up.")
                            .size(type_scale::CAPTION)
                            .color(p.warning_fg),
                    ]
                    .spacing(space::XS)
                    .align_y(Alignment::Center),
                )
                .padding([8, 10])
                .width(Length::Fill)
                .style(theme::pill_box(p.warning_bg, p.warning_fg, p.border_default)),
            );
        }

        if let Some(err) = &modal.error {
            body = body.push(
                container(
                    column![
                        row![
                            icons::alert_triangle(icon_size::XS, p.danger_fg),
                            text("Clone failed")
                                .size(type_scale::CAPTION)
                                .color(p.danger_fg),
                            Space::with_width(Length::Fill),
                            button(text("Command Log").size(type_scale::BADGE))
                                .padding([2, 7])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::OpenErrorCommandOutput),
                        ]
                        .spacing(space::XS)
                        .align_y(Alignment::Center),
                        text(truncate_chars(err, 280))
                            .size(type_scale::CAPTION)
                            .font(theme::CODE_FONT)
                            .color(p.text_primary),
                    ]
                    .spacing(4),
                )
                .padding([8, 10])
                .width(Length::Fill)
                .style(theme::pill_box(p.danger_bg, p.danger_fg, p.danger_fg)),
            );
        }

        let footer = row![
            text("Esc to close")
                .size(type_scale::CAPTION)
                .color(p.text_muted),
            Space::with_width(Length::Fill),
            button(text("Cancel").size(type_scale::BODY))
                .padding(pad::BTN_MD)
                .style(theme::subtle_btn(p))
                .on_press(Message::CloseCloneModal),
            button(
                row![
                    icons::cloud_download(
                        icon_size::XS,
                        if can_submit {
                            Color::WHITE
                        } else {
                            p.text_muted
                        }
                    ),
                    text("Clone Repository").size(type_scale::BODY),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding(pad::BTN_MD)
            .style(theme::primary_btn(p))
            .on_press_maybe(can_submit.then_some(Message::CloneRepoSubmitted)),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center);

        body = body
            .push(horizontal_rule(1).style(theme::subtle_rule(p)))
            .push(footer);

        let ws_badge = theme::badge(
            truncate_chars(&ws.name, 24),
            p.surface_panel,
            p.text_secondary,
            p.border_subtle,
            false,
        );

        theme::modal_shell(
            p,
            layout::MODAL_MD,
            icons::cloud_download(icon_size::MD, p.accent_fg),
            "Clone Repository",
            Some(ws_badge),
            body,
            Message::CloseCloneModal,
        )
    }

    fn view_active_clone_modal<'a>(
        &'a self,
        p: Palette,
        active: &'a ActiveCloneState,
    ) -> Element<'a, Message> {
        let snap = &active.snapshot;
        let is_cancelling = snap.is_cancelled;

        let header = row![
            container(icons::cloud_download(
                icon_size::MD,
                if is_cancelling {
                    p.warning_fg
                } else {
                    p.accent_fg
                }
            ))
            .padding(7)
            .style(theme::pill_box(
                if is_cancelling {
                    p.warning_bg
                } else {
                    p.fill_selected
                },
                if is_cancelling {
                    p.warning_fg
                } else {
                    p.accent_fg
                },
                p.border_default,
            )),
            column![
                text(format!("Cloning '{}'", active.folder_name))
                    .size(type_scale::TITLE)
                    .color(p.text_primary),
                theme::clipped_line(
                    text(truncate_chars(&active.url, 72))
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(p.text_muted),
                    Length::Fill,
                ),
            ]
            .spacing(2)
            .width(Length::Fill),
            theme::badge(
                if is_cancelling {
                    "Cancelling"
                } else {
                    snap.phase.short_label()
                },
                if is_cancelling {
                    p.warning_bg
                } else {
                    p.fill_selected
                },
                if is_cancelling {
                    p.warning_fg
                } else {
                    p.accent_fg
                },
                p.border_default,
                false,
            ),
            button(icons::close_x(icon_size::SM, p.text_secondary))
                .padding(pad::BTN_ICON)
                .style(theme::vscode_icon_btn(p, false))
                .on_press(Message::CloseCloneModal),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center);

        // 4-step clone lifecycle strip
        let cur_step = snap.phase.step_index();
        let step_labels = ["1. Remote", "2. Receiving", "3. Resolving", "4. Checkout"];
        let mut steps_row = row![].spacing(space::XS).width(Length::Fill);
        for (idx, step_label) in step_labels.iter().enumerate() {
            let done = !is_cancelling && idx < cur_step;
            let current = !is_cancelling && idx == cur_step;
            let (fg, bg, border) = if done {
                (p.success_fg, p.success_bg, p.border_subtle)
            } else if current {
                (p.accent_fg, p.fill_selected, p.accent_primary)
            } else {
                (p.text_muted, p.surface_panel, p.border_subtle)
            };

            let mut step_inner = row![].spacing(5).align_y(Alignment::Center);
            if done {
                step_inner = step_inner.push(icons::check(11.0, p.success_fg));
            }
            step_inner = step_inner.push(
                text(*step_label)
                    .size(type_scale::BADGE)
                    .color(fg)
                    .wrapping(text::Wrapping::None),
            );

            steps_row = steps_row.push(
                container(step_inner)
                    .width(Length::FillPortion(1))
                    .padding([5, 8])
                    .center_x(Length::FillPortion(1))
                    .style(theme::pill_box(bg, fg, border)),
            );
        }

        // Progress bar fraction & right-hand telemetry string
        let bar_fraction = if is_cancelling {
            (snap.overall_percent as f32 / 100.0).clamp(0.15, 1.0)
        } else if let Some(pct) = snap.stage_percent {
            (pct as f32 / 100.0).clamp(0.02, 1.0)
        } else {
            (snap.overall_percent as f32 / 100.0).clamp(0.06, 1.0)
        };

        let elapsed_str = format_elapsed_mm_ss(snap.elapsed_secs);
        let mut right_parts: Vec<String> = Vec::new();
        if !snap.transfer_detail.is_empty() && !is_cancelling {
            right_parts.push(snap.transfer_detail.clone());
        }
        if let Some(pct) = snap.stage_percent
            && !is_cancelling
        {
            right_parts.push(format!("{pct}%"));
        }
        right_parts.push(elapsed_str);
        let right_stat = right_parts.join("  ·  ");

        let fill_portion = ((bar_fraction * 1000.0).round() as u16).max(1);
        let rem_portion = 1000u16.saturating_sub(fill_portion);
        let bar_color = if is_cancelling {
            p.warning_fg
        } else {
            p.accent_primary
        };

        let mut bar_row = row![
            container(Space::new(Length::Fill, Length::Fixed(8.0)))
                .width(Length::FillPortion(fill_portion))
                .style(move |_| container::Style {
                    background: Some(Background::Color(bar_color)),
                    border: Border {
                        radius: 999.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
        ];
        if rem_portion > 0 {
            bar_row = bar_row.push(Space::with_width(Length::FillPortion(rem_portion)));
        }

        let progress_track = container(bar_row)
            .width(Length::Fill)
            .height(Length::Fixed(8.0))
            .style(move |_| container::Style {
                background: Some(Background::Color(p.border_subtle)),
                border: Border {
                    radius: 999.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            });

        // Live Git stderr output console lines
        let mut log_col = column![].spacing(3).width(Length::Fill);
        if snap.recent_lines.is_empty() {
            log_col = log_col.push(
                theme::clipped_line(
                    text(theme::sanitize_code_line(&snap.status_line))
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(p.text_secondary),
                    Length::Fill,
                ),
            );
        } else {
            let count = snap.recent_lines.len();
            for (i, line) in snap.recent_lines.iter().enumerate() {
                let is_latest = i + 1 == count;
                log_col = log_col.push(theme::clipped_line(
                    text(theme::sanitize_code_line(line))
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(if is_latest {
                            p.text_primary
                        } else {
                            p.text_muted
                        }),
                    Length::Fill,
                ));
            }
        }

        let console_box = container(log_col)
            .padding([8, 10])
            .width(Length::Fill)
            .style(move |_| container::Style {
                background: Some(Background::Color(p.surface_input)),
                border: Border {
                    color: p.border_subtle,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            });

        let progress_card = container(
            column![
                row![
                    text(snap.phase.label())
                        .size(type_scale::BODY)
                        .color(if is_cancelling {
                            p.warning_fg
                        } else {
                            p.accent_fg
                        }),
                    Space::with_width(Length::Fill),
                    text(right_stat)
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(p.text_secondary),
                ]
                .align_y(Alignment::Center),
                progress_track,
                console_box,
            ]
            .spacing(space::SM),
        )
        .padding(pad::CARD)
        .width(Length::Fill)
        .style(theme::section_panel_box(p));

        // Destination & flags summary
        let mut meta_row = row![
            icons::folder(icon_size::XS, p.text_muted),
            theme::clipped_line(
                text(active.dest_path.display().to_string())
                    .size(type_scale::CAPTION)
                    .font(theme::CODE_FONT)
                    .color(p.text_secondary),
                Length::Fill,
            ),
        ]
        .spacing(space::XS)
        .align_y(Alignment::Center);

        if let Some(branch) = &active.branch {
            meta_row = meta_row.push(theme::badge(
                format!("branch: {}", truncate_chars(branch, 18)),
                p.surface_panel,
                p.accent_fg,
                p.border_subtle,
                true,
            ));
        }
        if active.depth_one {
            meta_row = meta_row.push(theme::badge(
                "--depth 1",
                p.surface_panel,
                p.text_secondary,
                p.border_subtle,
                true,
            ));
        }
        if active.recurse_submodules {
            meta_row = meta_row.push(theme::badge(
                "submodules",
                p.surface_panel,
                p.text_secondary,
                p.border_subtle,
                false,
            ));
        }
        if let Some(grp) = &active.target_group {
            meta_row = meta_row.push(theme::badge(
                truncate_chars(grp, 18),
                p.fill_selected,
                p.accent_fg,
                p.border_subtle,
                false,
            ));
        }

        let footer = row![
            button(text("Run in Background").size(type_scale::BODY))
                .padding(pad::BTN_MD)
                .style(theme::subtle_btn(p))
                .on_press(Message::CloseCloneModal),
            Space::with_width(Length::Fill),
            button(
                row![
                    icons::x_circle(
                        icon_size::XS,
                        if is_cancelling {
                            p.text_muted
                        } else {
                            Color::WHITE
                        }
                    ),
                    text(if is_cancelling {
                        "Cancelling…"
                    } else {
                        "Cancel Clone"
                    })
                    .size(type_scale::BODY),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding(pad::BTN_MD)
            .style(theme::danger_btn(p))
            .on_press_maybe((!is_cancelling).then_some(Message::CancelCloneRepo)),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center);

        let card = opaque(
            container(
                column![
                    header,
                    horizontal_rule(1).style(theme::subtle_rule(p)),
                    steps_row,
                    progress_card,
                    meta_row,
                    horizontal_rule(1).style(theme::subtle_rule(p)),
                    footer,
                ]
                .spacing(space::MD),
            )
            .padding(pad::MODAL)
            .width(Length::Fixed(layout::MODAL_MD))
            .max_height(layout::MODAL_MAX_HEIGHT)
            .style(theme::modal_box(p)),
        );

        let positioned = container(card)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .padding(Padding::from([24, 24]))
            .style(theme::backdrop_box);

        theme::overlay_backdrop(positioned, Message::Noop)
    }
}
