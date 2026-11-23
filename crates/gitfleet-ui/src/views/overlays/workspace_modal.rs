use crate::app::{GitFleetApp, Message, PendingConfirm, WorkspaceModalTab};
use crate::icons;
use crate::theme::kit::{
    ButtonSize, ButtonVariant, ModalShell, ModalSize, UiButton, UnderlineTab, status_pill,
    underline_tabs,
};
use crate::theme::{self, Palette, icon_size, pad, space, type_scale};
use crate::views::{slim_scroll, tip, truncate_chars};
use iced::widget::{Space, button, column, container, horizontal_rule, row, text, text_input, tooltip};
use iced::{Alignment, Element, Length};

impl GitFleetApp {
    pub(crate) fn view_workspace_modal(&self, p: Palette) -> Element<'_, Message> {
        let ws = self.service.workspace.current();
        let all_workspaces = self.service.workspace.list_workspaces();
        let own = self.service.workspace.ownership();

        let tab_bar = row![
            underline_tabs(
                p,
                [
                    UnderlineTab::new(WorkspaceModalTab::Workspaces, "Workspaces")
                        .count(all_workspaces.len()),
                    UnderlineTab::new(WorkspaceModalTab::GroupsAndBlueprints, "Groups & Blueprints")
                        .count(ws.groups.len()),
                ],
                self.workspace_modal_tab,
                Message::SetWorkspaceModalTab,
            ),
            Space::with_width(Length::Fill),
            status_pill(
                if own.is_owner {
                    "Workspace Owner"
                } else {
                    "Locked by other PID"
                },
                if own.is_owner { p.success_fg } else { p.warning_fg },
                if own.is_owner { p.success_bg } else { p.warning_bg },
            ),
        ]
        .align_y(Alignment::Center)
        .width(Length::Fill);

        let tab_body: Element<'_, Message> = match self.workspace_modal_tab {
            WorkspaceModalTab::Workspaces => {
                let ws_actions = row![
                    text_input("New workspace name…", &self.new_workspace_name)
                        .on_input(Message::NewWorkspaceNameChanged)
                        .on_submit(Message::CreateNewWorkspace)
                        .padding(pad::INPUT_COMPACT)
                        .size(type_scale::BODY)
                        .width(Length::Fill)
                        .style(theme::dark_input(p)),
                    UiButton::new(p, "New")
                        .variant(ButtonVariant::Primary)
                        .size(ButtonSize::Sm)
                        .icon(icons::plus(icon_size::XS, theme::on_accent_ink(p.accent_primary)))
                        .on_press(Message::CreateNewWorkspace),
                    UiButton::new(p, "Open…")
                        .variant(ButtonVariant::Secondary)
                        .size(ButtonSize::Sm)
                        .icon(icons::folder_open(icon_size::XS, p.text_secondary))
                        .on_press(Message::PickOpenWorkspace),
                    UiButton::new(p, "Save As…")
                        .variant(ButtonVariant::Subtle)
                        .size(ButtonSize::Sm)
                        .icon(icons::save(icon_size::XS, p.text_secondary))
                        .on_press(Message::PickSaveWorkspaceAs),
                    UiButton::new(p, "Home")
                        .variant(ButtonVariant::Subtle)
                        .size(ButtonSize::Sm)
                        .on_press(Message::OpenHomeWorkspace),
                ]
                .spacing(space::SM)
                .align_y(Alignment::Center);

                let blueprint_banner = container(
                    row![
                        icons::repo_stack(icon_size::MD, p.accent_fg),
                        column![
                            text("Portable Group / Workspace Blueprint (.gitfleet-pack.json)")
                                .size(type_scale::BODY)
                                .font(theme::UI_FONT_MEDIUM)
                                .color(p.text_primary),
                            text("Export remote URLs + group structure to another machine and auto-clone every repo into matching folders.")
                                .size(type_scale::CAPTION)
                                .color(p.text_muted),
                        ]
                        .spacing(2)
                        .width(Length::Fill),
                        UiButton::new(p, "Import & Auto-Clone…")
                            .variant(ButtonVariant::OutlineAccent)
                            .size(ButtonSize::Sm)
                            .icon(icons::cloud_download(icon_size::XS, p.accent_fg))
                            .on_press(Message::PickImportBlueprintFile(None)),
                        UiButton::new(p, "Export Workspace…")
                            .variant(ButtonVariant::Primary)
                            .size(ButtonSize::Sm)
                            .icon(icons::save(icon_size::XS, theme::on_accent_ink(p.accent_primary)))
                            .on_press_maybe((!ws.repos.is_empty()).then_some(Message::OpenExportBlueprintModal(None))),
                    ]
                    .spacing(space::SM)
                    .align_y(Alignment::Center),
                )
                .padding(pad::CARD)
                .width(Length::Fill)
                .style(theme::card_box(p));

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
                                        .font(if item.is_current {
                                            theme::UI_FONT_MEDIUM
                                        } else {
                                            theme::UI_FONT
                                        })
                                        .color(if item.is_current {
                                            p.accent_fg
                                        } else {
                                            p.text_primary
                                        }),
                                    status_pill(
                                        format!("{} repo(s)", item.repo_count),
                                        p.text_muted,
                                        p.surface_card,
                                    ),
                                    if item.is_current {
                                        status_pill("Active", p.success_fg, p.success_bg)
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
                                .on_press(Message::RequestConfirm(
                                    PendingConfirm::delete_workspace(
                                        &item.name,
                                        item.path.clone(),
                                    ),
                                )),
                            "Delete workspace",
                            tooltip::Position::Top,
                            p,
                        ));
                    }
                    ws_list_col = ws_list_col.push(row_el);
                }

                column![
                    blueprint_banner,
                    ws_actions,
                    slim_scroll(ws_list_col, p).height(Length::Fixed(240.0)),
                ]
                .spacing(space::MD)
                .into()
            }

            WorkspaceModalTab::GroupsAndBlueprints => {
                let keyring = self.service.security.keyring_summary();
                let mut group_rows = column![].spacing(space::XS);
                let mut sorted_groups = ws.groups.clone();
                sorted_groups.sort();
                if sorted_groups.is_empty() {
                    group_rows = group_rows.push(
                        text("No groups yet. Create nested groups using '/' (e.g. company/backend) or import a Blueprint.")
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                    );
                }
                for g in &sorted_groups {
                    let repo_count = gitfleet_workspace::count_repos_in_group(&ws, g);
                    let assigned_key_name = ws
                        .group_secret_keys
                        .get(g)
                        .and_then(|kid| keyring.find(kid))
                        .map(|k| k.name.clone());
                    let mut r = row![
                        icons::folder(icon_size::SM, p.accent_fg),
                        text(g.clone())
                            .size(type_scale::BODY)
                            .font(theme::CODE_FONT)
                            .color(p.text_primary),
                        status_pill(format!("{repo_count}"), p.text_muted, p.surface_panel),
                    ]
                    .spacing(space::SM)
                    .align_y(Alignment::Center);
                    if let Some(key_name) = assigned_key_name {
                        r = r.push(status_pill(
                            format!("Key: {}", truncate_chars(&key_name, 14)),
                            p.success_fg,
                            p.success_bg,
                        ));
                    }
                    r = r.push(Space::with_width(Length::Fill));

                    r = r.push(
                        UiButton::new(p, "Export Blueprint")
                            .variant(ButtonVariant::OutlineAccent)
                            .size(ButtonSize::Xs)
                            .icon(icons::save(icon_size::XS, p.accent_fg))
                            .on_press_maybe(
                                (repo_count > 0)
                                    .then_some(Message::OpenExportBlueprintModal(Some(g.clone()))),
                            ),
                    );
                    r = r.push(
                        UiButton::new(p, "Import Here")
                            .variant(ButtonVariant::Subtle)
                            .size(ButtonSize::Xs)
                            .icon(icons::cloud_download(icon_size::XS, p.text_secondary))
                            .on_press(Message::PickImportBlueprintFile(Some(g.clone()))),
                    );
                    r = r.push(
                        UiButton::new(p, "Rename")
                            .variant(ButtonVariant::Subtle)
                            .size(ButtonSize::Xs)
                            .on_press(Message::OpenRenameGroupModal(g.clone())),
                    );
                    r = r.push(
                        UiButton::icon_only(p, icons::trash(icon_size::XS, p.danger_fg))
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::IconSm)
                            .on_press(Message::RequestConfirm(PendingConfirm::delete_group(
                                g.clone(),
                                repo_count,
                            ))),
                    );
                    group_rows = group_rows.push(
                        container(r)
                            .padding([6, 10])
                            .width(Length::Fill)
                            .style(theme::card_box(p)),
                    );
                }

                column![
                    row![
                        text_input(
                            "New group name (e.g. company/backend or platform/infra)…",
                            &self.new_group_input
                        )
                        .on_input(Message::NewGroupNameChanged)
                        .on_submit(Message::AddGroupSubmitted)
                        .padding(pad::INPUT_COMPACT)
                        .size(type_scale::BODY)
                        .width(Length::Fill)
                        .style(theme::dark_input(p)),
                        UiButton::new(p, "Add Group")
                            .variant(ButtonVariant::Primary)
                            .size(ButtonSize::Sm)
                            .icon(icons::plus(
                                icon_size::XS,
                                theme::on_accent_ink(p.accent_primary)
                            ))
                            .on_press(Message::AddGroupSubmitted),
                    ]
                    .spacing(space::SM)
                    .align_y(Alignment::Center),
                    slim_scroll(group_rows, p).height(Length::Fixed(310.0)),
                ]
                .spacing(space::MD)
                .into()
            }
        };

        let body = column![
            tab_bar,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            tab_body,
        ]
        .spacing(space::MD);

        ModalShell::new(p, "Workspace & Group Blueprint Manager", body)
            .subtitle("Switch workspaces, manage nested groups, or export/import portable Group = Folder blueprints.")
            .icon(icons::folder(icon_size::MD, p.accent_fg))
            .size(ModalSize::Lg)
            .scrollable_body(false)
            .on_close(Message::CloseOverlays)
            .into_element()
    }
}
