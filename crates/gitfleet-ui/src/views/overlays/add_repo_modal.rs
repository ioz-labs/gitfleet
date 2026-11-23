use crate::app::{
    AddRepoModalState, AddRepoMode, GitFleetApp, LocalRepoPathCheck, Message,
    inspect_local_repo_path, is_git_repo_dir,
};
use crate::icons;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use crate::views::truncate_chars;
use gitfleet_core::canonicalize_clean;
use iced::widget::{
    Space, button, checkbox, column, container, horizontal_rule, row, scrollable, text, text_input,
};
use iced::{Alignment, Color, Element, Length};
use std::path::Path;

fn path_matches_existing(a: &Path, b: &Path) -> bool {
    let ca = canonicalize_clean(a);
    let cb = canonicalize_clean(b);
    #[cfg(windows)]
    {
        ca.to_string_lossy()
            .replace('/', "\\")
            .eq_ignore_ascii_case(&cb.to_string_lossy().replace('/', "\\"))
    }
    #[cfg(not(windows))]
    {
        ca == cb
    }
}

impl GitFleetApp {
    pub(crate) fn view_add_repo_modal<'a>(
        &'a self,
        p: Palette,
        modal: &'a AddRepoModalState,
    ) -> Element<'a, Message> {
        let ws = self.service.workspace.current();

        let mode_tabs = theme::segmented_group_fill(
            p,
            row![
                button(
                    row![
                        icons::folder_open(
                            icon_size::XS,
                            if modal.mode == AddRepoMode::Existing {
                                p.accent_fg
                            } else {
                                p.text_secondary
                            }
                        ),
                        text("Add Existing Repository").size(type_scale::BODY),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .width(Length::FillPortion(1))
                .padding([6, 12])
                .style(theme::segmented_btn(p, modal.mode == AddRepoMode::Existing))
                .on_press_maybe(
                    (!modal.submitting).then_some(Message::AddRepoSetMode(AddRepoMode::Existing))
                ),
                button(
                    row![
                        icons::folder_plus(
                            icon_size::XS,
                            if modal.mode == AddRepoMode::InitNew {
                                p.accent_fg
                            } else {
                                p.text_secondary
                            }
                        ),
                        text("Create New Repository (git init)").size(type_scale::BODY),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .width(Length::FillPortion(1))
                .padding([6, 12])
                .style(theme::segmented_btn(p, modal.mode == AddRepoMode::InitNew))
                .on_press_maybe(
                    (!modal.submitting).then_some(Message::AddRepoSetMode(AddRepoMode::InitNew))
                ),
            ]
            .spacing(2),
        );

        let (mode_body, can_submit, primary_label): (Element<'a, Message>, bool, &'static str) =
            match modal.mode {
                AddRepoMode::Existing => {
                    let check = inspect_local_repo_path(&modal.existing_path_input);
                    let already_in_ws = matches!(check, LocalRepoPathCheck::ValidGitRepo { .. })
                        && ws.repos.iter().any(|r| {
                            path_matches_existing(&r.path, Path::new(modal.existing_path_input.trim()))
                        });

                    let valid = match &check {
                        LocalRepoPathCheck::ValidGitRepo { .. } => true,
                        LocalRepoPathCheck::UninitFolder { .. } => modal.init_if_needed,
                        _ => false,
                    } && !modal.submitting;

                    let cta_label = if modal.submitting {
                        "Initializing…"
                    } else if matches!(check, LocalRepoPathCheck::UninitFolder { .. })
                        && modal.init_if_needed
                    {
                        "Initialize & Add"
                    } else if already_in_ws {
                        "Select Repository"
                    } else {
                        "Add Repository"
                    };

                    let mut path_input = text_input(
                        "Paste local folder path or click Browse…",
                        &modal.existing_path_input,
                    )
                    .id(self.add_repo_path_input_id.clone())
                    .on_input(Message::AddRepoExistingPathChanged)
                    .padding(pad::INPUT)
                    .size(type_scale::BODY)
                    .width(Length::Fill)
                    .style(theme::dark_input(p));
                    if valid {
                        path_input = path_input.on_submit(Message::SubmitAddRepo);
                    }

                    let path_row = row![
                        path_input,
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
                        .on_press_maybe((!modal.submitting).then_some(Message::PickAddRepoFolder)),
                    ]
                    .spacing(space::SM)
                    .align_y(Alignment::Center);

                    let status_card: Element<'a, Message> = match check {
                        LocalRepoPathCheck::Empty => container(
                            text("Choose an existing local Git repository folder on disk, or pick a project folder to initialize with git init.")
                                .size(type_scale::CAPTION)
                                .color(p.text_muted),
                        )
                        .padding([8, 10])
                        .width(Length::Fill)
                        .style(theme::section_panel_box(p))
                        .into(),
                        LocalRepoPathCheck::Missing => container(
                            row![
                                icons::alert_triangle(icon_size::XS, p.danger_fg),
                                text("Folder path does not exist on disk.")
                                    .size(type_scale::CAPTION)
                                    .color(p.danger_fg),
                            ]
                            .spacing(space::XS)
                            .align_y(Alignment::Center),
                        )
                        .padding([8, 10])
                        .width(Length::Fill)
                        .style(theme::pill_box(p.danger_bg, p.danger_fg, p.border_default))
                        .into(),
                        LocalRepoPathCheck::NotADirectory => container(
                            row![
                                icons::alert_triangle(icon_size::XS, p.danger_fg),
                                text("Selected path is a file — select a directory instead.")
                                    .size(type_scale::CAPTION)
                                    .color(p.danger_fg),
                            ]
                            .spacing(space::XS)
                            .align_y(Alignment::Center),
                        )
                        .padding([8, 10])
                        .width(Length::Fill)
                        .style(theme::pill_box(p.danger_bg, p.danger_fg, p.border_default))
                        .into(),
                        LocalRepoPathCheck::ValidGitRepo { name } => {
                            if already_in_ws {
                                container(
                                    row![
                                        icons::check(icon_size::XS, p.accent_fg),
                                        text(format!(
                                            "Repository '{name}' is already in this workspace — submitting will select it (and update its group if chosen below)."
                                        ))
                                        .size(type_scale::CAPTION)
                                        .color(p.accent_fg),
                                    ]
                                    .spacing(space::XS)
                                    .align_y(Alignment::Center),
                                )
                                .padding([8, 10])
                                .width(Length::Fill)
                                .style(theme::pill_box(p.fill_selected, p.accent_fg, p.border_default))
                                .into()
                            } else {
                                container(
                                    row![
                                        icons::check(icon_size::XS, p.success_fg),
                                        text(format!(
                                            "Valid Git repository '{name}' — ready to add to workspace."
                                        ))
                                        .size(type_scale::CAPTION)
                                        .color(p.success_fg),
                                    ]
                                    .spacing(space::XS)
                                    .align_y(Alignment::Center),
                                )
                                .padding([8, 10])
                                .width(Length::Fill)
                                .style(theme::pill_box(p.success_bg, p.success_fg, p.border_subtle))
                                .into()
                            }
                        }
                        LocalRepoPathCheck::UninitFolder { name } => container(
                            column![
                                row![
                                    icons::alert_triangle(icon_size::XS, p.warning_fg),
                                    text(format!(
                                        "Folder '{name}' is not a Git repository yet (.git not found)."
                                    ))
                                    .size(type_scale::CAPTION)
                                    .color(p.warning_fg),
                                ]
                                .spacing(space::XS)
                                .align_y(Alignment::Center),
                                checkbox(
                                    "Initialize a new Git repository in this folder (git init)",
                                    modal.init_if_needed
                                )
                                .size(14)
                                .text_size(type_scale::CAPTION)
                                .style(theme::tree_checkbox(p))
                                .on_toggle(Message::AddRepoToggleInitIfNeeded),
                            ]
                            .spacing(space::XS),
                        )
                        .padding([9, 10])
                        .width(Length::Fill)
                        .style(theme::pill_box(p.warning_bg, p.warning_fg, p.border_default))
                        .into(),
                    };

                    (
                        column![
                            column![theme::field_label(p, "Local Repository Folder"), path_row]
                                .spacing(space::XXS),
                            status_card,
                        ]
                        .spacing(space::MD)
                        .into(),
                        valid,
                        cta_label,
                    )
                }
                AddRepoMode::InitNew => {
                    let resolved_dest = modal.resolved_init_dest();
                    let conflict: Option<&'static str> = resolved_dest.as_deref().and_then(|d| {
                        if d.is_file() {
                            Some("Destination path already exists as a file.")
                        } else if is_git_repo_dir(d) {
                            Some("Folder is already a Git repository — switch to 'Add Existing Repository'.")
                        } else {
                            None
                        }
                    });

                    let valid =
                        resolved_dest.is_some() && conflict.is_none() && !modal.submitting;
                    let cta_label = if modal.submitting {
                        "Creating…"
                    } else {
                        "Create & Add Repository"
                    };

                    let mut name_input = text_input("e.g. my-service", &modal.new_repo_name)
                        .id(self.add_repo_name_input_id.clone())
                        .on_input(Message::AddRepoNewNameChanged)
                        .padding(pad::INPUT)
                        .size(type_scale::BODY)
                        .width(Length::Fill)
                        .style(theme::dark_input(p));
                    if valid {
                        name_input = name_input.on_submit(Message::SubmitAddRepo);
                    }

                    let parent_row = row![
                        text_input(
                            "Choose or paste parent directory…",
                            &modal.create_parent_input
                        )
                        .on_input(Message::AddRepoCreateParentChanged)
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
                        .on_press_maybe(
                            (!modal.submitting).then_some(Message::PickAddRepoCreateParent)
                        ),
                    ]
                    .spacing(space::SM)
                    .align_y(Alignment::Center);

                    let preview_card: Element<'a, Message> = if let Some(dest) = resolved_dest {
                        let dest_str = dest.display().to_string();
                        if let Some(msg) = conflict {
                            container(
                                column![
                                    row![
                                        icons::alert_triangle(icon_size::XS, p.warning_fg),
                                        text(msg).size(type_scale::CAPTION).color(p.warning_fg),
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
                            .padding([8, 10])
                            .width(Length::Fill)
                            .style(theme::pill_box(p.warning_bg, p.warning_fg, p.border_default))
                            .into()
                        } else {
                            container(
                                row![
                                    text("Will initialize:")
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
                            .padding([7, 10])
                            .width(Length::Fill)
                            .style(theme::section_panel_box(p))
                            .into()
                        }
                    } else {
                        container(
                            text("Creates the target directory (if needed), runs git init, and adds it to this workspace.")
                                .size(type_scale::CAPTION)
                                .color(p.text_muted),
                        )
                        .padding([8, 10])
                        .width(Length::Fill)
                        .style(theme::section_panel_box(p))
                        .into()
                    };

                    (
                        column![
                            column![
                                theme::field_label(p, "New Repository Folder Name"),
                                name_input
                            ]
                            .spacing(space::XXS),
                            column![theme::field_label(p, "Parent Directory"), parent_row]
                                .spacing(space::XXS),
                            preview_card,
                        ]
                        .spacing(space::MD)
                        .into(),
                        valid,
                        cta_label,
                    )
                }
            };

        let cur_group_norm = modal.normalized_group();
        let mut group_col = column![
            theme::field_label(p, "Workspace Group (optional)"),
            text_input(
                "Root (no group) — or e.g. Backend/Services",
                &modal.group_input
            )
            .on_input(Message::AddRepoGroupChanged)
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
                .on_press(Message::AddRepoGroupChanged(String::new())),
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
                    .on_press(Message::AddRepoGroupChanged(g)),
                );
            }
            group_col = group_col.push(
                scrollable(chips)
                    .width(Length::Fill)
                    .direction(theme::slim_horizontal_direction())
                    .style(theme::slim_scrollable(p)),
            );
        }

        let mut body = column![mode_tabs, mode_body, group_col].spacing(space::MD);

        if let Some(err) = &modal.error {
            body = body.push(
                container(
                    row![
                        icons::alert_triangle(icon_size::XS, p.danger_fg),
                        text(truncate_chars(err, 240))
                            .size(type_scale::CAPTION)
                            .color(p.danger_fg),
                    ]
                    .spacing(space::XS)
                    .align_y(Alignment::Center),
                )
                .padding([8, 10])
                .width(Length::Fill)
                .style(theme::pill_box(p.danger_bg, p.danger_fg, p.danger_fg)),
            );
        }

        let close_msg = if modal.submitting {
            Message::Noop
        } else {
            Message::CloseAddRepoModal
        };

        let footer = row![
            text("Esc to close")
                .size(type_scale::CAPTION)
                .color(p.text_muted),
            Space::with_width(Length::Fill),
            button(text("Cancel").size(type_scale::BODY))
                .padding(pad::BTN_MD)
                .style(theme::subtle_btn(p))
                .on_press_maybe((!modal.submitting).then_some(Message::CloseAddRepoModal)),
            button(
                row![
                    icons::folder_plus(
                        icon_size::XS,
                        if can_submit {
                            Color::WHITE
                        } else {
                            p.text_muted
                        }
                    ),
                    text(primary_label).size(type_scale::BODY),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding(pad::BTN_MD)
            .style(theme::primary_btn(p))
            .on_press_maybe(can_submit.then_some(Message::SubmitAddRepo)),
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
            Element::from(icons::folder_plus(icon_size::MD, p.accent_fg)),
            "Add Repository",
            Some(ws_badge),
            body,
            close_msg,
        )
    }
}
