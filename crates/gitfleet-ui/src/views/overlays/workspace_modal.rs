use crate::app::{GitFleetApp, Message, PendingConfirm};
use crate::icons;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use crate::views::{slim_scroll, tip, truncate_chars};
use iced::widget::{Space, button, column, container, row, text, text_input, tooltip};
use iced::{Alignment, Color, Element, Length};

impl GitFleetApp {
    pub(crate) fn view_workspace_modal(&self, p: Palette) -> Element<'_, Message> {
        let ws = self.service.workspace.current();
        let all_workspaces = self.service.workspace.list_workspaces();
        let own = self.service.workspace.ownership();

        // 1. Active Workspace & Recent Switcher card
        let ws_actions = row![
            text_input("New workspace name…", &self.new_workspace_name)
                .on_input(Message::NewWorkspaceNameChanged)
                .on_submit(Message::CreateNewWorkspace)
                .padding(pad::INPUT)
                .size(type_scale::BODY)
                .width(Length::Fill)
                .style(theme::dark_input(p)),
            button(
                row![
                    icons::plus(icon_size::XS, Color::WHITE),
                    text("New").size(type_scale::BODY),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding(pad::BTN_MD)
            .style(theme::primary_btn(p))
            .on_press(Message::CreateNewWorkspace),
            button(
                row![
                    icons::folder_open(icon_size::XS, p.text_secondary),
                    text("Open…").size(type_scale::BODY),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding(pad::BTN_MD)
            .style(theme::subtle_btn(p))
            .on_press(Message::PickOpenWorkspace),
            button(
                row![
                    icons::save(icon_size::XS, p.text_secondary),
                    text("Save As…").size(type_scale::BODY),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding(pad::BTN_MD)
            .style(theme::subtle_btn(p))
            .on_press(Message::PickSaveWorkspaceAs),
            button(text("Home").size(type_scale::BODY))
                .padding(pad::BTN_MD)
                .style(theme::subtle_btn(p))
                .on_press(Message::OpenHomeWorkspace),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center);

        let mut ws_list_col = column![].spacing(space::XXS);
        for item in &all_workspaces {
            let open_msg = if item.is_current {
                Message::CloseOverlays
            } else if item.is_home && !item.path.exists() {
                Message::OpenHomeWorkspace
            } else {
                Message::OpenWorkspacePath(Some(item.path.clone()))
            };
            let file_hint = item
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string)
                .unwrap_or_else(|| item.path.display().to_string());

            let switch_btn = button(
                row![
                    if item.is_current {
                        icons::check(icon_size::SM, p.success_fg)
                    } else {
                        icons::folder(icon_size::SM, p.accent_fg)
                    },
                    theme::clipped_line(
                        row![
                            text(item.name.clone())
                                .size(type_scale::BODY)
                                .color(if item.is_current {
                                    p.accent_fg
                                } else {
                                    p.text_primary
                                }),
                            theme::badge(
                                format!("{} repo(s)", item.repo_count),
                                p.surface_card,
                                p.text_muted,
                                p.border_subtle,
                                false,
                            ),
                            if item.is_current {
                                Element::from(theme::badge(
                                    "Active",
                                    p.success_bg,
                                    p.success_fg,
                                    p.border_subtle,
                                    false,
                                ))
                            } else {
                                Space::with_width(Length::Shrink).into()
                            },
                        ]
                        .spacing(space::XS)
                        .align_y(Alignment::Center),
                        Length::Fill,
                    ),
                    text(truncate_chars(&file_hint, 34))
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(p.text_muted),
                ]
                .spacing(space::SM)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding(pad::ROW)
            .style(theme::repo_row_btn(p, item.is_current))
            .on_press(open_msg);

            let rename_target = if item.is_current {
                None
            } else {
                Some(item.path.clone())
            };
            let mut row_el = row![
                switch_btn,
                tip(
                    button(icons::pencil(icon_size::XS, p.text_secondary))
                        .padding([6, 8])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::OpenRenameWorkspaceModal(rename_target)),
                    "Rename workspace",
                    tooltip::Position::Top,
                    p,
                ),
            ]
            .spacing(space::XS)
            .align_y(Alignment::Center);
            if !item.is_home {
                row_el = row_el.push(tip(
                    button(icons::trash(icon_size::XS, p.danger_fg))
                        .padding([6, 8])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::RequestConfirm(PendingConfirm::delete_workspace(
                            &item.name,
                            item.path.clone(),
                        ))),
                    "Delete workspace",
                    tooltip::Position::Top,
                    p,
                ));
            }
            ws_list_col = ws_list_col.push(row_el);
        }

        let mut ws_section_body = column![
            ws_actions,
            row![
                theme::field_label(p, "Switch Workspace"),
                theme::badge(
                    format!("{}", all_workspaces.len()),
                    p.surface_card,
                    p.text_muted,
                    p.border_subtle,
                    false,
                ),
            ]
            .spacing(space::XS)
            .align_y(Alignment::Center),
            container(slim_scroll(ws_list_col, p).height(Length::Shrink)).max_height(210.0),
        ]
        .spacing(space::SM);
        if all_workspaces.len() <= 1 {
            ws_section_body = ws_section_body.push(
                text("Tip: Enter a name above and click New, or Open… an existing .gitfleet.json file to switch between workspaces.")
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
            );
        }

        let workspace_section = theme::section_card(
            p,
            format!("Workspace: {}", ws.name),
            Some("Create, switch, rename, or save portable JSON workspace snapshots."),
            Some(
                row![
                    button(
                        row![
                            icons::pencil(icon_size::XS, p.text_secondary),
                            text("Rename").size(type_scale::CAPTION),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                    )
                    .padding(pad::BTN_SM)
                    .style(theme::subtle_btn(p))
                    .on_press(Message::OpenRenameWorkspaceModal(None)),
                    theme::badge(
                        format!("{} repo(s)", ws.repos.len()),
                        p.fill_selected,
                        p.accent_fg,
                        p.border_subtle,
                        false,
                    ),
                ]
                .spacing(space::XS)
                .align_y(Alignment::Center)
                .into(),
            ),
            ws_section_body,
        );

        // 2. Repository Groups card (only real user groups; ungrouped repos live directly at root)
        let mut group_rows = column![].spacing(space::XS);
        let mut sorted_groups = ws.groups.clone();
        sorted_groups.sort();
        if sorted_groups.is_empty() {
            group_rows = group_rows.push(
                text("No groups yet. Repositories without a group appear directly at root level.")
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
            );
        }
        for g in &sorted_groups {
            let repo_count = ws
                .repos
                .iter()
                .filter(|r| r.group.as_deref() == Some(g.as_str()))
                .count();
            let mut r = row![
                icons::folder(icon_size::SM, p.accent_fg),
                text(g.clone())
                    .size(type_scale::SUBHEADING)
                    .color(p.text_primary),
                theme::badge(
                    format!("{repo_count}"),
                    p.surface_card,
                    p.text_muted,
                    p.border_subtle,
                    false
                ),
                Space::with_width(Length::Fill),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center);

            if let Some(sel_id) = &self.selected_repo {
                r = r.push(
                    button(text("Move Active Repo Here").size(type_scale::CAPTION))
                        .padding(pad::BTN_SM)
                        .style(theme::subtle_btn(p))
                        .on_press(Message::AssignRepoGroup(sel_id.clone(), Some(g.clone()))),
                );
            }
            r = r.push(
                button(
                    row![
                        icons::trash(icon_size::XS, p.danger_fg),
                        text("Delete").size(type_scale::CAPTION).color(p.danger_fg),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding(pad::BTN_SM)
                .style(theme::subtle_btn(p))
                .on_press(Message::RequestConfirm(PendingConfirm::delete_group(
                    g.clone(),
                ))),
            );
            group_rows = group_rows.push(
                container(r)
                    .padding(pad::ROW)
                    .width(Length::Fill)
                    .style(theme::card_box(p)),
            );
        }

        let active_in_group = self
            .selected_repo
            .as_ref()
            .and_then(|sel_id| {
                ws.repos
                    .iter()
                    .find(|r| &r.id == sel_id && r.group.as_deref().is_some_and(|g| !g.trim().is_empty()))
                    .map(|r| r.id.clone())
            });

        let groups_section = theme::section_card(
            p,
            "Repository Groups",
            Some("Organize repositories into nested sidebar groups (use '/' for subgroups)."),
            active_in_group.map(|sel_id| {
                button(text("Remove Active Repo from Group").size(type_scale::CAPTION))
                    .padding(pad::BTN_SM)
                    .style(theme::subtle_btn(p))
                    .on_press(Message::AssignRepoGroup(sel_id, None))
                    .into()
            }),
            column![
                row![
                    text_input(
                        "New group name (e.g. Backend or Backend/Services)…",
                        &self.new_group_input
                    )
                    .on_input(Message::NewGroupNameChanged)
                    .on_submit(Message::AddGroupSubmitted)
                    .padding(pad::INPUT)
                    .size(type_scale::BODY)
                    .width(Length::Fill)
                    .style(theme::dark_input(p)),
                    button(
                        row![
                            icons::plus(icon_size::XS, Color::WHITE),
                            text("Add Group").size(type_scale::BODY),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                    )
                    .padding(pad::BTN_MD)
                    .style(theme::primary_btn(p))
                    .on_press(Message::AddGroupSubmitted),
                ]
                .spacing(space::SM)
                .align_y(Alignment::Center),
                container(slim_scroll(group_rows, p).height(Length::Shrink)).max_height(170.0),
            ]
            .spacing(space::SM),
        );

        // 3. Two-column spacious Clone + Create & Init section cards
        let clone_dir_label = self
            .clone_parent_dir
            .as_ref()
            .map(|d| truncate_chars(&d.display().to_string(), 34))
            .unwrap_or_else(|| "Choose Parent Directory…".into());

        let clone_card = theme::section_card(
            p,
            "Clone Repository",
            Some("Clone a remote Git repository and add it directly to this workspace."),
            None,
            column![
                column![
                    theme::field_label(p, "Repository URL"),
                    text_input("https://github.com/org/repo.git", &self.clone_url_input)
                        .on_input(Message::CloneUrlChanged)
                        .padding(pad::INPUT)
                        .size(type_scale::BODY)
                        .width(Length::Fill)
                        .style(theme::dark_input(p)),
                ]
                .spacing(space::XXS),
                column![
                    theme::field_label(p, "Target Folder & Parent Directory"),
                    row![
                        text_input("Folder name (optional)", &self.clone_folder_input)
                            .on_input(Message::CloneFolderChanged)
                            .padding(pad::INPUT)
                            .size(type_scale::BODY)
                            .width(Length::FillPortion(1))
                            .style(theme::dark_input(p)),
                        button(
                            row![
                                icons::folder_open(
                                    icon_size::XS,
                                    if self.clone_parent_dir.is_some() {
                                        p.success_fg
                                    } else {
                                        p.text_secondary
                                    }
                                ),
                                text(clone_dir_label)
                                    .size(type_scale::CAPTION)
                                    .color(if self.clone_parent_dir.is_some() {
                                        p.success_fg
                                    } else {
                                        p.text_primary
                                    }),
                            ]
                            .spacing(5)
                            .align_y(Alignment::Center),
                        )
                        .padding(pad::BTN_MD)
                        .style(theme::subtle_btn(p))
                        .on_press(Message::PickCloneParent),
                    ]
                    .spacing(space::SM)
                    .align_y(Alignment::Center),
                ]
                .spacing(space::XXS),
                row![
                    Space::with_width(Length::Fill),
                    button(
                        row![
                            icons::fetch(icon_size::XS, Color::WHITE),
                            text("Clone Repository").size(type_scale::BODY),
                        ]
                        .spacing(6)
                        .align_y(Alignment::Center),
                    )
                    .padding(pad::BTN_MD)
                    .style(theme::primary_btn(p))
                    .on_press(Message::CloneRepoSubmitted),
                ],
            ]
            .spacing(space::SM),
        );

        let create_dir_label = self
            .create_parent_dir
            .as_ref()
            .map(|d| truncate_chars(&d.display().to_string(), 34))
            .unwrap_or_else(|| "Choose Parent Directory…".into());

        let create_card = theme::section_card(
            p,
            "Create & Init Repository",
            Some("Initialize a fresh local Git repository and register it in this workspace."),
            None,
            column![
                column![
                    theme::field_label(p, "New Repository Folder Name"),
                    text_input("e.g. my-service", &self.create_repo_name)
                        .on_input(Message::CreateRepoNameChanged)
                        .on_submit(Message::CreateRepoSubmitted)
                        .padding(pad::INPUT)
                        .size(type_scale::BODY)
                        .width(Length::Fill)
                        .style(theme::dark_input(p)),
                ]
                .spacing(space::XXS),
                column![
                    theme::field_label(p, "Parent Directory"),
                    button(
                        row![
                            icons::folder_open(
                                icon_size::XS,
                                if self.create_parent_dir.is_some() {
                                    p.success_fg
                                } else {
                                    p.text_secondary
                                }
                            ),
                            text(create_dir_label)
                                .size(type_scale::CAPTION)
                                .color(if self.create_parent_dir.is_some() {
                                    p.success_fg
                                } else {
                                    p.text_primary
                                }),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .padding(pad::BTN_MD)
                    .style(theme::subtle_btn(p))
                    .on_press(Message::PickCreateParent),
                ]
                .spacing(space::XXS),
                row![
                    Space::with_width(Length::Fill),
                    button(
                        row![
                            icons::folder_plus(icon_size::XS, Color::WHITE),
                            text("Create & Init").size(type_scale::BODY),
                        ]
                        .spacing(6)
                        .align_y(Alignment::Center),
                    )
                    .padding(pad::BTN_MD)
                    .style(theme::primary_btn(p))
                    .on_press(Message::CreateRepoSubmitted),
                ],
            ]
            .spacing(space::SM),
        );

        let repo_setup_row = row![
            container(clone_card).width(Length::FillPortion(1)),
            container(create_card).width(Length::FillPortion(1)),
        ]
        .spacing(space::MD)
        .width(Length::Fill);

        let body = slim_scroll(
            column![workspace_section, groups_section, repo_setup_row].spacing(space::MD),
            p,
        )
        .height(Length::Shrink);

        let owner_badge = theme::badge(
            if own.is_owner {
                "Owner"
            } else {
                "Locked by another process"
            },
            if own.is_owner {
                p.success_bg
            } else {
                p.warning_bg
            },
            if own.is_owner {
                p.success_fg
            } else {
                p.warning_fg
            },
            p.border_subtle,
            false,
        );

        theme::modal_shell(
            p,
            layout::MODAL_XL,
            icons::folder(icon_size::MD, p.accent_fg),
            "Workspace Manager",
            Some(owner_badge),
            body,
            Message::CloseOverlays,
        )
    }
}
