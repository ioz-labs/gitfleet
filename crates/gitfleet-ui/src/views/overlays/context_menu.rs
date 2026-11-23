use crate::app::{
    BottomViewMode, ContextMenuState, ContextMenuSubmenu, ContextMenuTarget, GitFleetApp, Message,
    PendingConfirm,
};
use crate::icons;
use crate::theme::{self, Palette, type_scale};
use crate::views::{slim_scroll_overlay, truncate_chars};
use gitfleet_git::{ConflictStrategy, FileChangeKind, ResetMode};
use iced::widget::{Space, button, column, container, horizontal_rule, mouse_area, row, text};
use iced::{Alignment, Element, Length};
impl GitFleetApp {
    pub(crate) fn view_context_menu(&self, p: Palette, menu: &ContextMenuState) -> Element<'_, Message> {
        let ws = self.service.workspace.current();
        let ctx_btn = |icon: Element<'static, Message>, label: String, msg: Message, danger: bool| -> Element<'static, Message> {
            button(
                row![
                    icon,
                    text(label)
                        .size(type_scale::BODY)
                        .line_height(text::LineHeight::Relative(1.15))
                        .color(if danger { p.danger_fg } else { p.text_primary }),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding([5, 10])
            .style(theme::context_menu_item(p, danger))
            .on_press(Message::ContextAction(Box::new(msg)))
            .into()
        };

        let mut items = column![].spacing(2);
        let mut submenu_card: Option<Element<'static, Message>> = None;
        let mut submenu_trigger_idx: Option<usize> = None;

        match &menu.target {
            ContextMenuTarget::WorkspacePill => {
                items = items.push(
                    container(
                        text(format!("Workspace: {}", truncate_chars(&ws.name, 22)))
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                    )
                    .padding([5, 10]),
                );
                items = items.push(ctx_btn(
                    icons::pencil(12.0, p.accent_fg).into(),
                    "Rename Workspace…".into(),
                    Message::OpenRenameWorkspaceModal(None),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::folder(12.0, p.text_secondary).into(),
                    "Manage Workspaces…".into(),
                    Message::ToggleWorkspaceModal,
                    false,
                ));
                items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                items = items.push(ctx_btn(
                    icons::folder_open(12.0, p.text_secondary).into(),
                    "Open Workspace…".into(),
                    Message::PickOpenWorkspace,
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::save(12.0, p.text_secondary).into(),
                    "Save Workspace As…".into(),
                    Message::PickSaveWorkspaceAs,
                    false,
                ));
                if !self.service.workspace.is_current_home() {
                    items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                    items = items.push(ctx_btn(
                        icons::folder(12.0, p.success_fg).into(),
                        "Switch to Home Workspace".into(),
                        Message::OpenHomeWorkspace,
                        false,
                    ));
                }
            }
            ContextMenuTarget::WorkingTree => {
                let changed = self
                    .active_status()
                    .map(|s| s.total_changes())
                    .unwrap_or(0);
                items = items.push(
                    container(
                        text(format!("Working Tree ({changed} changed)"))
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                    )
                    .padding([5, 10]),
                );
                items = items.push(ctx_btn(
                    icons::stage_all(12.0, p.success_fg).into(),
                    "Stage All Changes".into(),
                    Message::StageAll,
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::unstage_all(12.0, p.warning_fg).into(),
                    "Unstage All Changes".into(),
                    Message::UnstageAll,
                    false,
                ));
                items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                items = items.push(ctx_btn(
                    icons::stash(12.0, p.text_secondary).into(),
                    "Save Stash…".into(),
                    Message::RequestConfirm(PendingConfirm::stash_selected(changed)),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::trash(12.0, p.danger_fg).into(),
                    "Discard Local Changes…".into(),
                    Message::OpenDiscardModal(None),
                    true,
                ));
            }
            ContextMenuTarget::RepositoriesPane => {
                items = items.push(
                    container(text("Repositories").size(type_scale::CAPTION).color(p.text_muted))
                        .padding([5, 10]),
                );
                items = items.push(ctx_btn(
                    icons::plus(12.0, p.accent_fg).into(),
                    "New Group…".into(),
                    Message::OpenInlineGroupInput(None),
                    false,
                ));
                items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                items = items.push(ctx_btn(
                    icons::cloud_download(12.0, p.accent_fg).into(),
                    "Clone Repository…".into(),
                    Message::OpenCloneModal(None),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::folder_plus(12.0, p.text_secondary).into(),
                    "Add / Create Local Repository…".into(),
                    Message::OpenAddRepoModal(None),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::folder_search(12.0, p.text_secondary).into(),
                    "Scan Folder & Auto-Group…".into(),
                    Message::PickScanFolder(None),
                    false,
                ));
                items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                items = items.push(ctx_btn(
                    icons::refresh(12.0, p.text_secondary).into(),
                    "Refresh All Repositories".into(),
                    Message::RefreshAll,
                    false,
                ));
            }
            ContextMenuTarget::Group { name } => {
                let close_sub = |el: Element<'static, Message>| -> Element<'static, Message> {
                    mouse_area(el)
                        .on_enter(Message::SetContextSubmenu(ContextMenuSubmenu::None))
                        .into()
                };
                let is_collapsed = ws.ui.collapsed_groups.contains(name);
                let keyring = self.service.security.keyring_summary();
                let explicit_group_key = ws.group_secret_keys.get(name).map(String::as_str);
                let assigned_key_hint = explicit_group_key
                    .and_then(|kid| keyring.find(kid))
                    .map(|k| truncate_chars(&k.name, 14));

                items = items.push(
                    container(text(format!("Group: {name}")).size(type_scale::CAPTION).color(p.text_muted))
                        .padding([5, 10]),
                );
                items = items.push(close_sub(ctx_btn(
                    if is_collapsed {
                        icons::chevron_down(12.0, p.text_secondary).into()
                    } else {
                        icons::chevron_right(12.0, p.text_secondary).into()
                    },
                    if is_collapsed {
                        "Expand Group".into()
                    } else {
                        "Collapse Group".into()
                    },
                    Message::ToggleGroupCollapse(name.clone()),
                    false,
                )));
                items = items.push(close_sub(ctx_btn(
                    icons::folder_plus(12.0, p.accent_fg).into(),
                    "New Subgroup…".into(),
                    Message::OpenInlineGroupInput(Some(name.clone())),
                    false,
                )));
                items = items.push(close_sub(ctx_btn(
                    icons::plus(12.0, p.text_secondary).into(),
                    "New Top-Level Group…".into(),
                    Message::OpenInlineGroupInput(None),
                    false,
                )));

                let mut has_move_active = false;
                if let Some(active) = self.current_repo()
                    && !Self::is_submodule_repo_id(&active.id)
                    && active.group.as_deref() != Some(name.as_str())
                {
                    has_move_active = true;
                    items = items.push(close_sub(ctx_btn(
                        icons::folder(12.0, p.accent_fg).into(),
                        format!("Move '{}' Here", truncate_chars(&active.name, 18)),
                        Message::AssignRepoGroup(active.id, Some(name.clone())),
                        false,
                    )));
                }
                items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                items = items.push(close_sub(ctx_btn(
                    icons::cloud_download(12.0, p.accent_fg).into(),
                    "Clone Repository into Group…".into(),
                    Message::OpenCloneModal(Some(name.clone())),
                    false,
                )));
                items = items.push(close_sub(ctx_btn(
                    icons::folder_plus(12.0, p.text_secondary).into(),
                    "Add Local Repository to Group…".into(),
                    Message::OpenAddRepoModal(Some(name.clone())),
                    false,
                )));
                items = items.push(close_sub(ctx_btn(
                    icons::folder_search(12.0, p.accent_fg).into(),
                    "Scan Folder into Group…".into(),
                    Message::PickScanFolder(Some(name.clone())),
                    false,
                )));
                items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));

                let key_sub_open = menu.submenu == ContextMenuSubmenu::GroupSecretKey;
                if key_sub_open {
                    submenu_trigger_idx = Some(9 + usize::from(has_move_active));
                }
                let mut key_row = row![
                    icons::key(12.0, p.success_fg),
                    text("Secret Key")
                        .size(type_scale::BODY)
                        .line_height(text::LineHeight::Relative(1.15))
                        .color(p.text_primary),
                    Space::with_width(Length::Fill),
                ]
                .spacing(8)
                .align_y(Alignment::Center);
                if let Some(hint) = assigned_key_hint {
                    key_row = key_row.push(text(hint).size(type_scale::CAPTION).color(p.text_muted));
                }
                key_row = key_row.push(icons::chevron_right(11.0, p.text_muted));
                let key_trigger = button(key_row)
                    .width(Length::Fill)
                    .padding([5, 10])
                    .style(theme::repo_row_btn(p, key_sub_open))
                    .on_press(Message::SetContextSubmenu(if key_sub_open {
                        ContextMenuSubmenu::None
                    } else {
                        ContextMenuSubmenu::GroupSecretKey
                    }));
                items = items.push(
                    mouse_area(key_trigger)
                        .on_enter(Message::SetContextSubmenu(ContextMenuSubmenu::GroupSecretKey)),
                );

                if key_sub_open {
                    let mut sub_items = column![
                        container(text("Group Secret Key").size(type_scale::CAPTION).color(p.text_muted))
                            .padding([5, 10]),
                    ]
                    .spacing(2);
                    sub_items = sub_items.push(ctx_btn(
                        if explicit_group_key.is_none() {
                            icons::check(12.0, p.success_fg).into()
                        } else {
                            icons::close_x(11.0, p.text_muted).into()
                        },
                        "Inherit (Parent / Default)".into(),
                        Message::SetGroupSecretKey(name.clone(), None),
                        false,
                    ));
                    if !keyring.keys.is_empty() {
                        sub_items = sub_items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                    }
                    for k in &keyring.keys {
                        let is_cur = explicit_group_key == Some(k.id.as_str());
                        let label = if k.is_default {
                            format!("{} (default)", truncate_chars(&k.name, 18))
                        } else {
                            truncate_chars(&k.name, 22)
                        };
                        sub_items = sub_items.push(ctx_btn(
                            if is_cur {
                                icons::check(12.0, p.success_fg).into()
                            } else {
                                icons::key(12.0, p.accent_fg).into()
                            },
                            label,
                            Message::SetGroupSecretKey(name.clone(), Some(k.id.clone())),
                            false,
                        ));
                    }
                    sub_items = sub_items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                    sub_items = sub_items.push(ctx_btn(
                        icons::settings(12.0, p.text_secondary).into(),
                        "Manage Keys…".into(),
                        Message::OpenSecretsDrawer { show_keys: true },
                        false,
                    ));
                    submenu_card = Some(iced::widget::opaque(
                        container(slim_scroll_overlay(sub_items, p).height(Length::Shrink))
                            .padding(6)
                            .width(Length::Fixed(248.0))
                            .max_height(360.0)
                            .style(theme::context_menu_box(p)),
                    ));
                }

                let child_repo_count = gitfleet_workspace::count_repos_in_group(&ws, name);
                items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                items = items.push(close_sub(ctx_btn(
                    icons::pencil(12.0, p.text_secondary).into(),
                    "Rename Group…".into(),
                    Message::OpenRenameGroupModal(name.clone()),
                    false,
                )));
                items = items.push(close_sub(ctx_btn(
                    icons::trash(12.0, p.danger_fg).into(),
                    if child_repo_count > 0 {
                        format!(
                            "Delete Group '{}' ({child_repo_count})…",
                            truncate_chars(name, 16)
                        )
                    } else {
                        format!("Delete Group '{}'…", truncate_chars(name, 18))
                    },
                    Message::RequestConfirm(PendingConfirm::delete_group(
                        name.clone(),
                        child_repo_count,
                    )),
                    true,
                )));
            }
            ContextMenuTarget::Repo {
                id,
                name,
                path,
                current_group,
            } => {
                let close_sub = |el: Element<'static, Message>| -> Element<'static, Message> {
                    mouse_area(el)
                        .on_enter(Message::SetContextSubmenu(ContextMenuSubmenu::None))
                        .into()
                };
                let keyring = self.service.security.keyring_summary();
                let explicit_repo_key = ws
                    .repos
                    .iter()
                    .find(|r| &r.id == id)
                    .and_then(|r| r.secret_key_id.as_deref());
                let resolved_repo_key_hint = self
                    .service
                    .workspace
                    .resolve_repo_secret_key(id)
                    .and_then(|(kid, _)| keyring.find(&kid).map(|k| truncate_chars(&k.name, 14)));

                items = items.push(
                    container(
                        text(truncate_chars(name, 26))
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                    )
                    .padding([5, 10]),
                );
                items = items.push(close_sub(ctx_btn(
                    icons::check(12.0, p.success_fg).into(),
                    "Commit…".into(),
                    Message::OpenCommitModal(id.clone()),
                    false,
                )));
                items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                items = items.push(close_sub(ctx_btn(
                    icons::refresh(12.0, p.text_secondary).into(),
                    "Refresh".into(),
                    Message::ContextRepoGitOp(id.clone(), "refresh"),
                    false,
                )));
                items = items.push(close_sub(ctx_btn(
                    icons::fetch(12.0, p.text_secondary).into(),
                    "Fetch".into(),
                    Message::ContextRepoGitOp(id.clone(), "fetch"),
                    false,
                )));
                items = items.push(close_sub(ctx_btn(
                    icons::pull(12.0, p.accent_fg).into(),
                    "Pull".into(),
                    Message::ContextRepoGitOp(id.clone(), "pull"),
                    false,
                )));
                items = items.push(close_sub(ctx_btn(
                    icons::push(12.0, p.success_fg).into(),
                    "Push".into(),
                    Message::ContextRepoGitOp(id.clone(), "push"),
                    false,
                )));
                items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));

                let move_sub_open = menu.submenu == ContextMenuSubmenu::MoveToGroup;
                if move_sub_open {
                    submenu_trigger_idx = Some(8);
                }
                let move_trigger = button(
                    row![
                        icons::folder(12.0, p.accent_fg),
                        text("Move to")
                            .size(type_scale::BODY)
                            .line_height(text::LineHeight::Relative(1.15))
                            .color(p.text_primary),
                        Space::with_width(Length::Fill),
                        icons::chevron_right(11.0, p.text_muted),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding([5, 10])
                .style(theme::repo_row_btn(p, move_sub_open))
                .on_press(Message::SetContextSubmenu(if move_sub_open {
                    ContextMenuSubmenu::None
                } else {
                    ContextMenuSubmenu::MoveToGroup
                }));

                items = items.push(
                    mouse_area(move_trigger)
                        .on_enter(Message::SetContextSubmenu(ContextMenuSubmenu::MoveToGroup)),
                );

                let key_sub_open = menu.submenu == ContextMenuSubmenu::RepoSecretKey;
                if key_sub_open {
                    submenu_trigger_idx = Some(9);
                }
                let mut key_row = row![
                    icons::key(12.0, p.success_fg),
                    text("Secret Key")
                        .size(type_scale::BODY)
                        .line_height(text::LineHeight::Relative(1.15))
                        .color(p.text_primary),
                    Space::with_width(Length::Fill),
                ]
                .spacing(8)
                .align_y(Alignment::Center);
                if let Some(hint) = resolved_repo_key_hint {
                    key_row = key_row.push(text(hint).size(type_scale::CAPTION).color(p.text_muted));
                }
                key_row = key_row.push(icons::chevron_right(11.0, p.text_muted));
                let key_trigger = button(key_row)
                    .width(Length::Fill)
                    .padding([5, 10])
                    .style(theme::repo_row_btn(p, key_sub_open))
                    .on_press(Message::SetContextSubmenu(if key_sub_open {
                        ContextMenuSubmenu::None
                    } else {
                        ContextMenuSubmenu::RepoSecretKey
                    }));
                items = items.push(
                    mouse_area(key_trigger)
                        .on_enter(Message::SetContextSubmenu(ContextMenuSubmenu::RepoSecretKey)),
                );

                if move_sub_open {
                    let mut sub_items = column![
                        container(text("Move to Group").size(type_scale::CAPTION).color(p.text_muted))
                            .padding([5, 10]),
                    ]
                    .spacing(2);

                    let in_no_group = current_group.is_none();
                    sub_items = sub_items.push(ctx_btn(
                        if in_no_group {
                            icons::check(12.0, p.success_fg).into()
                        } else {
                            icons::close_x(11.0, p.text_muted).into()
                        },
                        "No group (Root)".into(),
                        Message::AssignRepoGroup(id.clone(), None),
                        false,
                    ));

                    let mut sorted_groups: Vec<String> = ws.groups.clone();
                    sorted_groups.sort();
                    sorted_groups.dedup();
                    if !sorted_groups.is_empty() {
                        sub_items = sub_items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                    }
                    for g in sorted_groups {
                        let depth = g.matches('/').count();
                        let leaf = g.rsplit('/').next().unwrap_or(&g);
                        let is_cur = current_group.as_deref() == Some(g.as_str());
                        let label = if depth > 0 {
                            format!("{}{}", "  ".repeat(depth), leaf)
                        } else {
                            leaf.to_string()
                        };
                        sub_items = sub_items.push(ctx_btn(
                            if is_cur {
                                icons::check(12.0, p.success_fg).into()
                            } else {
                                icons::folder(12.0, p.accent_fg).into()
                            },
                            label,
                            Message::AssignRepoGroup(id.clone(), Some(g)),
                            false,
                        ));
                    }

                    sub_items = sub_items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                    sub_items = sub_items.push(ctx_btn(
                        icons::plus(12.0, p.accent_fg).into(),
                        "New Group…".into(),
                        Message::OpenInlineGroupInput(None),
                        false,
                    ));

                    submenu_card = Some(iced::widget::opaque(
                        container(slim_scroll_overlay(sub_items, p).height(Length::Shrink))
                            .padding(6)
                            .width(Length::Fixed(240.0))
                            .max_height(360.0)
                            .style(theme::context_menu_box(p)),
                    ));
                } else if key_sub_open {
                    let mut sub_items = column![
                        container(text("Repo Secret Key").size(type_scale::CAPTION).color(p.text_muted))
                            .padding([5, 10]),
                    ]
                    .spacing(2);
                    sub_items = sub_items.push(ctx_btn(
                        if explicit_repo_key.is_none() {
                            icons::check(12.0, p.success_fg).into()
                        } else {
                            icons::close_x(11.0, p.text_muted).into()
                        },
                        "Inherit (Group / Default)".into(),
                        Message::SetRepoSecretKey(id.clone(), None),
                        false,
                    ));
                    if !keyring.keys.is_empty() {
                        sub_items = sub_items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                    }
                    for k in &keyring.keys {
                        let is_cur = explicit_repo_key == Some(k.id.as_str());
                        let label = if k.is_default {
                            format!("{} (default)", truncate_chars(&k.name, 18))
                        } else {
                            truncate_chars(&k.name, 22)
                        };
                        sub_items = sub_items.push(ctx_btn(
                            if is_cur {
                                icons::check(12.0, p.success_fg).into()
                            } else {
                                icons::key(12.0, p.accent_fg).into()
                            },
                            label,
                            Message::SetRepoSecretKey(id.clone(), Some(k.id.clone())),
                            false,
                        ));
                    }
                    sub_items = sub_items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                    sub_items = sub_items.push(ctx_btn(
                        icons::settings(12.0, p.text_secondary).into(),
                        "Manage Keys…".into(),
                        Message::OpenSecretsDrawer { show_keys: true },
                        false,
                    ));

                    submenu_card = Some(iced::widget::opaque(
                        container(slim_scroll_overlay(sub_items, p).height(Length::Shrink))
                            .padding(6)
                            .width(Length::Fixed(248.0))
                            .max_height(360.0)
                            .style(theme::context_menu_box(p)),
                    ));
                }

                items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                items = items.push(close_sub(ctx_btn(
                    icons::folder_open(12.0, p.accent_fg).into(),
                    "Open in Explorer".into(),
                    Message::RevealPathInExplorer(path.clone()),
                    false,
                )));
                items = items.push(close_sub(ctx_btn(
                    icons::terminal(12.0, p.text_secondary).into(),
                    "Open Terminal Here".into(),
                    Message::ContextRepoGitOp(id.clone(), "terminal"),
                    false,
                )));
                items = items.push(close_sub(ctx_btn(
                    icons::copy(12.0, p.text_secondary).into(),
                    "Copy Repository Path".into(),
                    Message::CopyText("repository path".into(), path.display().to_string()),
                    false,
                )));
                items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                items = items.push(close_sub(ctx_btn(
                    icons::trash(12.0, p.danger_fg).into(),
                    "Remove from Workspace…".into(),
                    Message::RequestConfirm(PendingConfirm::remove_repo(id.clone(), name, path)),
                    true,
                )));
            }
            ContextMenuTarget::Commit {
                oid,
                short_oid,
                summary,
            } => {
                let full_msg = self
                    .commits
                    .iter()
                    .find(|c| &c.oid == oid)
                    .map(|c| {
                        if c.body.trim().is_empty() {
                            c.summary.clone()
                        } else {
                            format!("{}\n\n{}", c.summary, c.body)
                        }
                    })
                    .unwrap_or_else(|| summary.clone());

                items = items.push(
                    container(
                        text(format!("Commit {short_oid}"))
                            .size(type_scale::CAPTION)
                            .font(theme::CODE_FONT)
                            .color(p.accent_fg),
                    )
                    .padding([5, 10]),
                );
                items = items.push(ctx_btn(
                    icons::branch(12.0, p.success_fg).into(),
                    "Check Out Commit (Detached)…".into(),
                    Message::RequestConfirm(PendingConfirm::checkout_commit(
                        oid.clone(),
                        short_oid,
                        summary,
                    )),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::git_merge(12.0, p.accent_fg).into(),
                    "Merge Commit into HEAD…".into(),
                    Message::RequestConfirm(PendingConfirm::merge_ref(
                        oid.clone(),
                        &format!("{short_oid}  {summary}"),
                    )),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::cherry_pick(12.0, p.accent_fg).into(),
                    "Cherry-Pick Commit…".into(),
                    Message::OpenCherryPickModal {
                        revisions: vec![oid.clone()],
                        detail: format!("{short_oid}  {summary}"),
                    },
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::revert(12.0, p.warning_fg).into(),
                    "Revert Commit…".into(),
                    Message::RequestConfirm(PendingConfirm::revert_commit(
                        oid.clone(),
                        short_oid,
                        summary,
                    )),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::branch(12.0, p.violet_fg).into(),
                    "Rebase HEAD onto Commit…".into(),
                    Message::RequestConfirm(PendingConfirm::rebase_onto(
                        oid.clone(),
                        &format!("{short_oid}  {summary}"),
                    )),
                    true,
                ));
                items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                items = items.push(ctx_btn(
                    icons::file_diff(12.0, p.text_secondary).into(),
                    "Compare with HEAD".into(),
                    Message::CompareRefs("HEAD".into(), oid.clone()),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::tag(12.0, p.warning_fg).into(),
                    "Create Tag Here".into(),
                    Message::CreateTagAtCommit(oid.clone()),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::plus(12.0, p.success_fg).into(),
                    "Add Branch Here…".into(),
                    Message::OpenCreateBranch {
                        base_ref: Some(oid.clone()),
                        prefix: None,
                    },
                    false,
                ));
                items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                items = items.push(ctx_btn(
                    icons::reset_soft(12.0, p.text_secondary).into(),
                    "Reset Soft to Here…".into(),
                    Message::RequestConfirm(PendingConfirm::reset_commit(
                        oid.clone(),
                        short_oid,
                        ResetMode::Soft,
                    )),
                    true,
                ));
                items = items.push(ctx_btn(
                    icons::reset_mixed(12.0, p.warning_fg).into(),
                    "Reset Mixed to Here…".into(),
                    Message::RequestConfirm(PendingConfirm::reset_commit(
                        oid.clone(),
                        short_oid,
                        ResetMode::Mixed,
                    )),
                    true,
                ));
                items = items.push(ctx_btn(
                    icons::trash(12.0, p.danger_fg).into(),
                    "Reset Hard to Here…".into(),
                    Message::RequestConfirm(PendingConfirm::reset_commit(
                        oid.clone(),
                        short_oid,
                        ResetMode::Hard,
                    )),
                    true,
                ));
                items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                items = items.push(ctx_btn(
                    icons::file_code(12.0, p.text_secondary).into(),
                    "Use Message for Commit".into(),
                    Message::UseCommitMessage(oid.clone()),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::copy(12.0, p.text_secondary).into(),
                    "Copy Commit Message".into(),
                    Message::CopyText("commit message".into(), full_msg),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::copy(12.0, p.text_secondary).into(),
                    format!("Copy SHA ({short_oid})"),
                    Message::CopyText("commit SHA".into(), oid.clone()),
                    false,
                ));
            }
            ContextMenuTarget::Branch {
                name,
                is_head,
                is_remote,
            } => {
                let in_graph = self.graph_checked_branches.contains(name);
                let has_upstream = self
                    .branches
                    .iter()
                    .find(|b| !b.is_remote && &b.name == name)
                    .is_some_and(|b| b.upstream.is_some());

                items = items.push(
                    container(
                        text(truncate_chars(name, 26))
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                    )
                    .padding([5, 10]),
                );

                if !is_head {
                    if *is_remote {
                        items = items.push(ctx_btn(
                            icons::branch(12.0, p.success_fg).into(),
                            "Check Out Tracking Branch".into(),
                            Message::CheckoutRemoteBranch(name.clone()),
                            false,
                        ));
                    } else {
                        items = items.push(ctx_btn(
                            icons::branch(12.0, p.success_fg).into(),
                            format!("Check Out '{}'", truncate_chars(name, 18)),
                            Message::CheckoutBranch(name.clone()),
                            false,
                        ));
                    }
                }

                items = items.push(ctx_btn(
                    icons::check(12.0, if in_graph { p.accent_fg } else { p.text_secondary }).into(),
                    if in_graph {
                        "Hide from Graph (Toggle)".into()
                    } else {
                        "Show in Graph (Toggle)".into()
                    },
                    Message::ToggleGraphBranch(name.clone()),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::eye(12.0, p.text_secondary).into(),
                    "Reveal Commit in Graph".into(),
                    Message::RevealBranchCommit(name.clone()),
                    false,
                ));

                if !is_head {
                    items = items.push(ctx_btn(
                        icons::file_diff(12.0, p.text_secondary).into(),
                        "Compare with HEAD".into(),
                        Message::CompareRefs("HEAD".into(), name.clone()),
                        false,
                    ));
                    items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                    items = items.push(ctx_btn(
                        icons::git_merge(12.0, p.accent_fg).into(),
                        "Merge… (Ctrl+M)".into(),
                        Message::OpenMergeWithTarget(name.clone()),
                        false,
                    ));
                    items = items.push(ctx_btn(
                        icons::branch(12.0, p.violet_fg).into(),
                        "Rebase… (Ctrl+D)".into(),
                        Message::RequestConfirm(PendingConfirm::rebase_onto(name.clone(), name)),
                        false,
                    ));
                    items = items.push(ctx_btn(
                        icons::git_merge(12.0, p.success_fg).into(),
                        "Fast-Forward Merge…".into(),
                        Message::RequestConfirm(PendingConfirm::ff_merge_branch(name.clone())),
                        false,
                    ));
                    items = items.push(ctx_btn(
                        icons::cherry_pick(12.0, p.accent_fg).into(),
                        "Cherry-Pick Tip Commit…".into(),
                        Message::OpenCherryPickModal {
                            revisions: vec![name.clone()],
                            detail: format!("Branch tip: {name}"),
                        },
                        false,
                    ));
                }

                items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));

                if !is_remote {
                    items = items.push(ctx_btn(
                        icons::plus(12.0, p.accent_fg).into(),
                        format!("Add Branch from '{}'…", truncate_chars(name, 16)),
                        Message::OpenCreateBranch {
                            base_ref: Some(name.clone()),
                            prefix: None,
                        },
                        false,
                    ));
                    items = items.push(ctx_btn(
                        icons::push(12.0, p.success_fg).into(),
                        "Push Branch…".into(),
                        Message::RequestConfirm(PendingConfirm::push_branch(name.clone())),
                        false,
                    ));
                } else {
                    items = items.push(ctx_btn(
                        icons::plus(12.0, p.accent_fg).into(),
                        format!("Add Local Branch from '{}'…", truncate_chars(name, 16)),
                        Message::OpenCreateBranch {
                            base_ref: Some(name.clone()),
                            prefix: None,
                        },
                        false,
                    ));
                }

                if !is_head {
                    items = items.push(ctx_btn(
                        icons::reset_soft(12.0, p.text_secondary).into(),
                        "Reset HEAD (Soft) to Branch…".into(),
                        Message::RequestConfirm(PendingConfirm::reset_commit(
                            name.clone(),
                            name,
                            ResetMode::Soft,
                        )),
                        true,
                    ));
                    items = items.push(ctx_btn(
                        icons::reset_mixed(12.0, p.warning_fg).into(),
                        "Reset HEAD (Mixed) to Branch…".into(),
                        Message::RequestConfirm(PendingConfirm::reset_commit(
                            name.clone(),
                            name,
                            ResetMode::Mixed,
                        )),
                        true,
                    ));
                }

                if has_upstream {
                    items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                    items = items.push(ctx_btn(
                        icons::close_x(12.0, p.text_secondary).into(),
                        "Stop Tracking Upstream…".into(),
                        Message::RequestConfirm(PendingConfirm::unset_upstream(name.clone())),
                        false,
                    ));
                }

                if !is_remote {
                    items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                    items = items.push(ctx_btn(
                        icons::branch(12.0, p.accent_fg).into(),
                        "Rename Branch… (F2)".into(),
                        Message::OpenRenameBranchModal(Some(name.clone())),
                        false,
                    ));
                }

                if !is_head && !is_remote {
                    items = items.push(ctx_btn(
                        icons::trash(12.0, p.danger_fg).into(),
                        format!("Delete Branch '{}'…", truncate_chars(name, 16)),
                        Message::RequestDeleteBranches(vec![name.clone()]),
                        true,
                    ));
                }

                items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                items = items.push(ctx_btn(
                    icons::copy(12.0, p.text_secondary).into(),
                    "Copy Branch Name".into(),
                    Message::CopyText("branch name".into(), name.clone()),
                    false,
                ));
            }
            ContextMenuTarget::LocalBranchesGroup { local_names } => {
                items = items.push(
                    container(text("Local Branches").size(type_scale::CAPTION).color(p.text_muted))
                        .padding([5, 10]),
                );
                items = items.push(ctx_btn(
                    icons::plus(12.0, p.accent_fg).into(),
                    "Add Branch… (F7)".into(),
                    Message::OpenCreateBranch {
                        base_ref: None,
                        prefix: None,
                    },
                    false,
                ));
                items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                items = items.push(ctx_btn(
                    icons::chevron_down(12.0, p.text_secondary).into(),
                    "Toggle Expand / Collapse".into(),
                    Message::ToggleBranchTreeNode("local".into()),
                    false,
                ));
                if !local_names.is_empty() {
                    items = items.push(ctx_btn(
                        icons::check(12.0, p.text_secondary).into(),
                        "Toggle All in Graph".into(),
                        Message::ToggleGraphBranchGroup(local_names.clone()),
                        false,
                    ));
                }
            }
            ContextMenuTarget::LocalBranchFolder { folder_key, prefix } => {
                items = items.push(
                    container(
                        text(truncate_chars(prefix, 26))
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                    )
                    .padding([5, 10]),
                );
                items = items.push(ctx_btn(
                    icons::plus(12.0, p.accent_fg).into(),
                    format!("Add Branch in '{}'…", truncate_chars(prefix, 16)),
                    Message::OpenCreateBranch {
                        base_ref: None,
                        prefix: Some(prefix.clone()),
                    },
                    false,
                ));
                items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                items = items.push(ctx_btn(
                    icons::chevron_down(12.0, p.text_secondary).into(),
                    "Toggle Expand / Collapse".into(),
                    Message::ToggleBranchTreeNode(folder_key.clone()),
                    false,
                ));
            }
            ContextMenuTarget::ChangedFile {
                file,
                is_working_tree,
            } => {
                let fname = file.path.rsplit('/').next().unwrap_or(&file.path);
                items = items.push(
                    container(text(truncate_chars(fname, 26)).size(type_scale::CAPTION).color(p.text_muted))
                        .padding([5, 10]),
                );
                let in_multi_sel = self.selected_changed_files.len() > 1
                    && self
                        .selected_changed_files
                        .contains(&(file.path.clone(), file.staged));
                let multi_cnt = self.selected_changed_files.len();

                if *is_working_tree && file.kind == FileChangeKind::Conflicted {
                    items = items.push(ctx_btn(
                        icons::git_merge(12.0, p.danger_fg).into(),
                        "Open Conflict Solver".into(),
                        Message::OpenConflictSolver(file.path.clone()),
                        false,
                    ));
                    items = items.push(ctx_btn(
                        icons::check(12.0, p.accent_fg).into(),
                        "Keep Ours (Whole File)".into(),
                        Message::ResolveConflict(file.path.clone(), ConflictStrategy::Ours),
                        false,
                    ));
                    items = items.push(ctx_btn(
                        icons::check(12.0, p.violet_fg).into(),
                        "Keep Theirs (Whole File)".into(),
                        Message::ResolveConflict(file.path.clone(), ConflictStrategy::Theirs),
                        false,
                    ));
                    items = items.push(ctx_btn(
                        icons::check(12.0, p.success_fg).into(),
                        "Mark Resolved (git add)".into(),
                        Message::ResolveConflict(file.path.clone(), ConflictStrategy::MarkResolved),
                        false,
                    ));
                    items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                } else if *is_working_tree && file.kind != FileChangeKind::Conflicted {
                    if file.staged {
                        if in_multi_sel {
                            items = items.push(ctx_btn(
                                icons::minus(12.0, p.warning_fg).into(),
                                format!("Unstage {multi_cnt} Selected Files (Ctrl+Shift+T)"),
                                Message::UnstageSelectedFiles,
                                false,
                            ));
                        } else {
                            items = items.push(ctx_btn(
                                icons::minus(12.0, p.warning_fg).into(),
                                "Unstage File (Ctrl+Shift+T)".into(),
                                Message::UnstageFile(file.path.clone()),
                                false,
                            ));
                        }
                    } else if in_multi_sel {
                        items = items.push(ctx_btn(
                            icons::plus(12.0, p.success_fg).into(),
                            format!("Stage {multi_cnt} Selected Files (Ctrl+T)"),
                            Message::StageSelectedFiles,
                            false,
                        ));
                    } else {
                        items = items.push(ctx_btn(
                            icons::plus(12.0, p.success_fg).into(),
                            "Stage File (Ctrl+T)".into(),
                            Message::StageFile(file.path.clone()),
                            false,
                        ));
                    }
                    items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                }
                items = items.push(ctx_btn(
                    icons::external_window(12.0, p.accent_fg).into(),
                    "Open in GitFleet Studio".into(),
                    Message::OpenDetachedForChangedFile(file.clone()),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::file_code(12.0, p.text_secondary).into(),
                    "View Side-by-Side Diff".into(),
                    Message::OpenFileInMode(file.clone(), BottomViewMode::SideBySideDiff),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::file_code(12.0, p.accent_fg).into(),
                    "Open Preview".into(),
                    Message::OpenFileInMode(file.clone(), BottomViewMode::Preview),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::file_code(12.0, p.text_secondary).into(),
                    "Open in Editor".into(),
                    Message::OpenFileInMode(file.clone(), BottomViewMode::Edit),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::folder_open(12.0, p.accent_fg).into(),
                    "Open in Explorer".into(),
                    Message::RevealRepoFileInExplorer(file.path.clone()),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::copy(12.0, p.text_secondary).into(),
                    "Copy Relative Path".into(),
                    Message::CopyText("file path".into(), file.path.clone()),
                    false,
                ));
                if *is_working_tree && file.kind != FileChangeKind::Conflicted {
                    items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                    if in_multi_sel {
                        items = items.push(ctx_btn(
                            icons::trash(12.0, p.danger_fg).into(),
                            format!("Discard {multi_cnt} Selected Files… (Ctrl+Z)"),
                            Message::DiscardSelectedFiles,
                            true,
                        ));
                    } else {
                        items = items.push(ctx_btn(
                            icons::trash(12.0, p.danger_fg).into(),
                            if file.kind == FileChangeKind::Untracked {
                                "Delete / Discard File… (Ctrl+Z)".into()
                            } else {
                                "Discard Changes… (Ctrl+Z)".into()
                            },
                            Message::OpenDiscardModal(Some(vec![file.path.clone()])),
                            true,
                        ));
                    }
                }
            }
            ContextMenuTarget::RepoFile { path } => {
                let fname = path.rsplit('/').next().unwrap_or(path);
                items = items.push(
                    container(
                        text(truncate_chars(fname, 26))
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                    )
                    .padding([5, 10]),
                );
                items = items.push(ctx_btn(
                    icons::external_window(12.0, p.accent_fg).into(),
                    "Open in GitFleet Studio".into(),
                    Message::OpenDetachedForRepoFile(path.clone()),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::file_code(12.0, p.accent_fg).into(),
                    "Open Preview".into(),
                    Message::OpenRepoFileInMode(path.clone(), BottomViewMode::Preview),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::pencil(12.0, p.text_secondary).into(),
                    "Open in Editor".into(),
                    Message::OpenRepoFileInMode(path.clone(), BottomViewMode::Edit),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::folder_open(12.0, p.accent_fg).into(),
                    "Open in Explorer".into(),
                    Message::RevealRepoFileInExplorer(path.clone()),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::copy(12.0, p.text_secondary).into(),
                    "Copy Relative Path".into(),
                    Message::CopyText("file path".into(), path.clone()),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::pencil(12.0, p.text_secondary).into(),
                    "Rename…".into(),
                    Message::OpenRenameRepoEntry {
                        rel_path: path.clone(),
                        is_dir: false,
                    },
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::eye_off(12.0, p.text_secondary).into(),
                    "Add to .gitignore…".into(),
                    Message::OpenAddGitignoreModal {
                        rel_path: path.clone(),
                        is_dir: false,
                    },
                    false,
                ));
                items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                items = items.push(ctx_btn(
                    icons::trash(12.0, p.danger_fg).into(),
                    "Delete File…".into(),
                    Message::RequestConfirm(PendingConfirm::delete_repo_entry(path.clone(), false)),
                    true,
                ));
            }
            ContextMenuTarget::RepoDir { path } => {
                let dname = path.rsplit('/').next().unwrap_or(path);
                items = items.push(
                    container(
                        text(truncate_chars(dname, 26))
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                    )
                    .padding([5, 10]),
                );
                items = items.push(ctx_btn(
                    icons::folder_open(12.0, p.accent_fg).into(),
                    "Open in Explorer".into(),
                    Message::RevealRepoFileInExplorer(path.clone()),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::copy(12.0, p.text_secondary).into(),
                    "Copy Relative Path".into(),
                    Message::CopyText("folder path".into(), path.clone()),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::pencil(12.0, p.text_secondary).into(),
                    "Rename…".into(),
                    Message::OpenRenameRepoEntry {
                        rel_path: path.clone(),
                        is_dir: true,
                    },
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::eye_off(12.0, p.text_secondary).into(),
                    "Add to .gitignore…".into(),
                    Message::OpenAddGitignoreModal {
                        rel_path: path.clone(),
                        is_dir: true,
                    },
                    false,
                ));
                items = items.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                items = items.push(ctx_btn(
                    icons::trash(12.0, p.danger_fg).into(),
                    "Delete Folder…".into(),
                    Message::RequestConfirm(PendingConfirm::delete_repo_entry(path.clone(), true)),
                    true,
                ));
            }
        }

        let menu_w = 272.0_f32;

        let main_card = iced::widget::opaque(
            container(slim_scroll_overlay(items, p).height(Length::Shrink))
                .padding(6)
                .width(Length::Fixed(menu_w))
                .max_height(480.0)
                .style(theme::context_menu_box(p)),
        );

        let positioned = theme::popup_with_submenu(
            menu.position,
            main_card,
            submenu_card.map(|sub| (sub, submenu_trigger_idx)),
        );

        theme::overlay_backdrop(positioned, Message::CloseContextMenu)
    }
}
