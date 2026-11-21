use crate::app::{
    BottomViewMode, ContextMenuState, ContextMenuTarget, GitFleetApp, Message, PendingConfirm,
};
use crate::icons;
use crate::theme::{self, Palette};
use crate::views::{slim_scroll, truncate_chars};
use gitfleet_git::{FileChangeKind, ResetMode};
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
                        .size(11)
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

        match &menu.target {
            ContextMenuTarget::WorkingTree => {
                let changed = self
                    .active_status()
                    .map(|s| s.total_changes())
                    .unwrap_or(0);
                items = items.push(
                    container(
                        text(format!("Working Tree ({changed} changed)"))
                            .size(10)
                            .color(p.text_muted),
                    )
                    .padding([2, 10]),
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
                items = items.push(horizontal_rule(1));
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
                    container(text("Repositories").size(10).color(p.text_muted))
                        .padding([2, 10]),
                );
                items = items.push(ctx_btn(
                    icons::plus(12.0, p.accent_fg).into(),
                    "New Group…".into(),
                    Message::OpenInlineGroupInput(None),
                    false,
                ));
                items = items.push(horizontal_rule(1));
                items = items.push(ctx_btn(
                    icons::folder_plus(12.0, p.text_secondary).into(),
                    "Add Local Repository…".into(),
                    Message::PickAddRepo,
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::folder_search(12.0, p.text_secondary).into(),
                    "Scan Folder for Repositories…".into(),
                    Message::PickScanFolder,
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::pull(12.0, p.accent_fg).into(),
                    "Clone / Create Repository…".into(),
                    Message::ToggleWorkspaceModal,
                    false,
                ));
                items = items.push(horizontal_rule(1));
                items = items.push(ctx_btn(
                    icons::refresh(12.0, p.text_secondary).into(),
                    "Refresh All Repositories".into(),
                    Message::RefreshAll,
                    false,
                ));
            }
            ContextMenuTarget::Group { name } => {
                let is_collapsed = ws.ui.collapsed_groups.contains(name);
                items = items.push(
                    container(text(format!("Group: {name}")).size(10).color(p.text_muted))
                        .padding([2, 10]),
                );
                items = items.push(ctx_btn(
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
                ));
                items = items.push(ctx_btn(
                    icons::folder_plus(12.0, p.accent_fg).into(),
                    "New Subgroup…".into(),
                    Message::OpenInlineGroupInput(Some(name.clone())),
                    false,
                ));
                items = items.push(ctx_btn(
                    icons::plus(12.0, p.text_secondary).into(),
                    "New Top-Level Group…".into(),
                    Message::OpenInlineGroupInput(None),
                    false,
                ));

                if let Some(active) = self.current_repo()
                    && !Self::is_submodule_repo_id(&active.id)
                    && active.group.as_deref() != Some(name.as_str())
                {
                    items = items.push(ctx_btn(
                        icons::folder(12.0, p.accent_fg).into(),
                        format!("Move '{}' Here", truncate_chars(&active.name, 18)),
                        Message::AssignRepoGroup(active.id, Some(name.clone())),
                        false,
                    ));
                }
                items = items.push(horizontal_rule(1));
                items = items.push(ctx_btn(
                    icons::folder_plus(12.0, p.text_secondary).into(),
                    "Add Local Repository…".into(),
                    Message::PickAddRepo,
                    false,
                ));
                items = items.push(horizontal_rule(1));
                items = items.push(ctx_btn(
                    icons::trash(12.0, p.danger_fg).into(),
                    format!("Delete Group '{}'…", truncate_chars(name, 18)),
                    Message::RequestConfirm(PendingConfirm::delete_group(name.clone())),
                    true,
                ));
            }
            ContextMenuTarget::Repo {
                id,
                name,
                path,
                current_group,
            } => {
                let close_sub = |el: Element<'static, Message>| -> Element<'static, Message> {
                    mouse_area(el)
                        .on_enter(Message::SetContextSubmenu(false))
                        .into()
                };
                items = items.push(
                    container(
                        text(truncate_chars(name, 26))
                            .size(10)
                            .color(p.text_muted),
                    )
                    .padding([2, 10]),
                );
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
                items = items.push(horizontal_rule(1));

                let move_trigger = button(
                    row![
                        icons::folder(12.0, p.accent_fg),
                        text("Move to").size(11).color(p.text_primary),
                        Space::with_width(Length::Fill),
                        icons::chevron_right(11.0, p.text_muted),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding([5, 10])
                .style(theme::repo_row_btn(p, menu.submenu_open))
                .on_press(Message::SetContextSubmenu(!menu.submenu_open));

                items = items.push(
                    mouse_area(move_trigger)
                        .on_enter(Message::SetContextSubmenu(true)),
                );

                if menu.submenu_open {
                    let mut sub_items = column![
                        container(text("Move to Group").size(10).color(p.text_muted))
                            .padding([2, 10]),
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
                        sub_items = sub_items.push(horizontal_rule(1));
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

                    sub_items = sub_items.push(horizontal_rule(1));
                    sub_items = sub_items.push(ctx_btn(
                        icons::plus(12.0, p.accent_fg).into(),
                        "New Group…".into(),
                        Message::OpenInlineGroupInput(None),
                        false,
                    ));

                    submenu_card = Some(iced::widget::opaque(
                        container(slim_scroll(sub_items, p).height(Length::Shrink))
                            .padding(6)
                            .width(Length::Fixed(214.0))
                            .max_height(360.0)
                            .style(theme::context_menu_box(p)),
                    ));
                }

                items = items.push(horizontal_rule(1));
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
                items = items.push(horizontal_rule(1));
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
                            .size(10)
                            .font(theme::CODE_FONT)
                            .color(p.accent_fg),
                    )
                    .padding([2, 10]),
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
                items = items.push(horizontal_rule(1));
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
                items = items.push(horizontal_rule(1));
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
                items = items.push(horizontal_rule(1));
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
                            .size(10)
                            .color(p.text_muted),
                    )
                    .padding([2, 10]),
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
                    items = items.push(horizontal_rule(1));
                    items = items.push(ctx_btn(
                        icons::git_merge(12.0, p.accent_fg).into(),
                        "Merge / Rebase Forecast…".into(),
                        Message::OpenMergeWithTarget(name.clone()),
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

                items = items.push(horizontal_rule(1));

                if !is_remote {
                    items = items.push(ctx_btn(
                        icons::push(12.0, p.success_fg).into(),
                        "Push Branch…".into(),
                        Message::RequestConfirm(PendingConfirm::push_branch(name.clone())),
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
                    items = items.push(horizontal_rule(1));
                    items = items.push(ctx_btn(
                        icons::close_x(12.0, p.text_secondary).into(),
                        "Stop Tracking Upstream…".into(),
                        Message::RequestConfirm(PendingConfirm::unset_upstream(name.clone())),
                        false,
                    ));
                }

                if !is_head && !is_remote {
                    items = items.push(horizontal_rule(1));
                    items = items.push(ctx_btn(
                        icons::trash(12.0, p.danger_fg).into(),
                        format!("Delete Branch '{}'…", truncate_chars(name, 16)),
                        Message::RequestConfirm(PendingConfirm::delete_branch(name.clone(), false)),
                        true,
                    ));
                    items = items.push(ctx_btn(
                        icons::trash(12.0, p.danger_fg).into(),
                        "Force Delete Branch (-D)…".into(),
                        Message::RequestConfirm(PendingConfirm::delete_branch(name.clone(), true)),
                        true,
                    ));
                }

                items = items.push(horizontal_rule(1));
                items = items.push(ctx_btn(
                    icons::copy(12.0, p.text_secondary).into(),
                    "Copy Branch Name".into(),
                    Message::CopyText("branch name".into(), name.clone()),
                    false,
                ));
            }
            ContextMenuTarget::ChangedFile {
                file,
                is_working_tree,
            } => {
                let fname = file.path.rsplit('/').next().unwrap_or(&file.path);
                items = items.push(
                    container(text(truncate_chars(fname, 26)).size(10).color(p.text_muted))
                        .padding([2, 10]),
                );
                let in_multi_sel = self.selected_changed_files.len() > 1
                    && self
                        .selected_changed_files
                        .contains(&(file.path.clone(), file.staged));
                let multi_cnt = self.selected_changed_files.len();

                if *is_working_tree && file.kind != FileChangeKind::Conflicted {
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
                    items = items.push(horizontal_rule(1));
                }
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
                    icons::copy(12.0, p.text_secondary).into(),
                    "Copy Relative Path".into(),
                    Message::CopyText("file path".into(), file.path.clone()),
                    false,
                ));
                if *is_working_tree && file.kind != FileChangeKind::Conflicted {
                    items = items.push(horizontal_rule(1));
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
        }

        let menu_w = 248.0_f32;

        let main_card = iced::widget::opaque(
            container(slim_scroll(items, p).height(Length::Shrink))
                .padding(6)
                .width(Length::Fixed(menu_w))
                .max_height(480.0)
                .style(theme::context_menu_box(p)),
        );

        let popup_body: Element<'_, Message> = if let Some(sub) = submenu_card {
            row![main_card, sub]
                .spacing(4)
                .align_y(Alignment::Start)
                .into()
        } else {
            main_card
        };

        let positioned = theme::popup_at(menu.position, popup_body);

        theme::overlay_backdrop(positioned, Message::CloseContextMenu)
    }
}
