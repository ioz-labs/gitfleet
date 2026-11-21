use crate::app::{ContextMenuTarget, GitFleetApp, Message, PendingConfirm, RepoFilter};
use crate::icons;
use crate::theme::{self, Palette, tree, type_scale};
use crate::views::{slim_scroll_h, tip};
use gitfleet_git::bulk::BulkOperation;
use gitfleet_workspace::RepoEntry;
use iced::widget::{
    Space, button, checkbox, column, container, horizontal_rule, mouse_area, row, text, text_input,
    tooltip,
};
use iced::{Alignment, Element, Length, Padding};
use std::collections::BTreeMap;

#[derive(Default)]
struct RepoGroupNode {
    subgroups: BTreeMap<String, RepoGroupNode>,
    repos: Vec<RepoEntry>,
}

impl RepoGroupNode {
    fn ensure_group(&mut self, path: &str) -> &mut Self {
        let mut cursor = self;
        for seg in path.split('/').filter(|s| !s.is_empty()) {
            cursor = cursor.subgroups.entry(seg.to_string()).or_default();
        }
        cursor
    }

    fn insert_repo(&mut self, group_path: Option<&str>, repo: RepoEntry) {
        if let Some(g) = group_path
            && !g.is_empty()
        {
            self.ensure_group(g).repos.push(repo);
        } else {
            self.repos.push(repo);
        }
    }

    fn total_repos(&self) -> usize {
        self.repos.len()
            + self
                .subgroups
                .values()
                .map(Self::total_repos)
                .sum::<usize>()
    }
}

impl GitFleetApp {
    fn render_repo_entry<'a>(
        &'a self,
        repo: &RepoEntry,
        current_group: Option<String>,
        depth: usize,
        p: Palette,
        out: &mut Vec<Element<'a, Message>>,
        max_row_w: &mut f32,
    ) {
        let subs = self
            .repo_submodules
            .get(&repo.id)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        let has_subs = !subs.is_empty();
        let subs_expanded = has_subs && !self.collapsed_repo_submodules.contains(&repo.id);
        let selected =
            self.selected_repo.as_ref() == Some(&repo.id) && self.active_submodule.is_none();
        let is_being_dragged = self
            .dragging_repo
            .as_ref()
            .is_some_and(|(drag_id, _)| drag_id == &repo.id);
        let checked = self.bulk_selected.contains(&repo.id);
        let st = self.statuses.get(&repo.id);

        let dot_color = if !repo.available {
            p.text_muted
        } else if let Some(s) = st {
            if s.conflict_count > 0 {
                p.danger_fg
            } else if !s.is_clean {
                p.warning_fg
            } else {
                p.success_fg
            }
        } else {
            p.text_muted
        };

        let branch = st.and_then(|s| s.branch.as_deref()).unwrap_or("—");
        let changed = st.map(|s| s.total_changes()).unwrap_or(0);
        let id_check = repo.id.clone();

        let left_x = tree::chevron_x(depth);
        let branch_chars = if repo.available {
            branch.chars().count() + 2
        } else {
            9
        };
        let repo_est_w = tree::content_x(depth)
            + 28.0
            + repo.name.chars().count() as f32 * 7.5
            + branch_chars as f32 * 6.9
            + if changed > 0 { 26.0 } else { 0.0 }
            + if !repo.available { 54.0 } else { 0.0 };
        if repo_est_w > *max_row_w {
            *max_row_w = repo_est_w;
        }
        let sub_toggle: Element<'a, Message> = if has_subs {
            button(
                container(if subs_expanded {
                    icons::chevron_down(tree::CHEVRON_ICON_SIZE, p.text_secondary)
                } else {
                    icons::chevron_right(tree::CHEVRON_ICON_SIZE, p.text_muted)
                })
                .width(Length::Fill)
                .center_x(Length::Fill),
            )
            .padding([tree::ROW_PAD_V, 0.0])
            .width(Length::Fixed(tree::CHEVRON_W))
            .style(theme::vscode_icon_btn(p, false))
            .on_press(Message::ToggleRepoSubmodules(repo.id.clone()))
            .into()
        } else {
            Space::with_width(Length::Fixed(tree::CHEVRON_W)).into()
        };

        let bulk_box = container(
            checkbox("", checked)
                .size(tree::CHECKBOX_SIZE)
                .spacing(0)
                .style(theme::tree_checkbox(p))
                .on_toggle(move |v| Message::ToggleBulkSelect(id_check.clone(), v)),
        )
        .width(Length::Fixed(tree::LEAD_W))
        .center_x(Length::Fixed(tree::LEAD_W));

        let mut repo_row = row![
            sub_toggle,
            Space::with_width(Length::Fixed(tree::CHEVRON_GAP)),
            bulk_box,
            Space::with_width(Length::Fixed(tree::ROW_BTN_GAP)),
            theme::pressable_row(
                p,
                selected,
                Padding::from([tree::ROW_PAD_V, tree::ROW_PAD_H]),
                row![
                    container(Space::new(7, 7)).style(theme::dot_box(dot_color)),
                    theme::clipped_line(
                        row![
                            text(repo.name.clone())
                                .size(type_scale::BODY)
                                .color(if is_being_dragged {
                                    p.text_muted
                                } else {
                                    p.text_primary
                                }),
                            text(if repo.available {
                                format!("({branch})")
                            } else {
                                "[missing]".to_string()
                            })
                            .size(type_scale::CAPTION)
                            .color(if repo.available {
                                p.text_muted
                            } else {
                                p.danger_fg
                            }),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                        Length::Fill,
                    ),
                    if changed > 0 {
                        Element::from(
                            text(format!("{changed}"))
                                .size(type_scale::BADGE)
                                .color(p.warning_fg),
                        )
                    } else {
                        Space::with_width(Length::Shrink).into()
                    },
                ]
                .spacing(6)
                .align_y(Alignment::Center),
                Message::RepoMouseDown(repo.id.clone(), repo.name.clone()),
            ),
        ]
        .spacing(0)
        .align_y(Alignment::Center);

        if !repo.available {
            repo_row = repo_row
                .push(Space::with_width(Length::Fixed(4.0)))
                .push(
                    button(text("Locate").size(type_scale::BADGE).color(p.warning_fg))
                        .padding([2, 5])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::LocateRepo(repo.id.clone())),
                );
        }

        let padded_repo_row = container(repo_row)
            .width(Length::Fill)
            .padding(Padding {
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
                left: left_x,
            });

        let mut row_mouse = mouse_area(padded_repo_row).on_right_press(Message::OpenContextMenu(
            ContextMenuTarget::Repo {
                id: repo.id.clone(),
                name: repo.name.clone(),
                path: repo.path.clone(),
                current_group: current_group.clone(),
            },
        ));
        if !is_being_dragged {
            let token = format!("repo:{}", repo.id.0);
            row_mouse = row_mouse
                .on_enter(Message::DragHoverGroup(token.clone(), current_group))
                .on_exit(Message::DragHoverGroupExit(token));
        }
        out.push(row_mouse.into());

        if subs_expanded {
            let sub_depth = depth + 1;
            let sub_btn_left = (tree::lead_x(sub_depth) - tree::ROW_PAD_H).max(0.0);
            for sub in subs {
                let sub_key = Self::submodule_repo_id(&repo.id, &sub.path);
                let sub_st = self.statuses.get(&sub_key);
                let sub_selected = self
                    .active_submodule
                    .as_ref()
                    .is_some_and(|(pid, sp)| pid == &repo.id && sp == &sub.path);
                let sub_dirty = sub.dirty || sub_st.is_some_and(|s| !s.is_clean);
                let sub_changed = sub_st.map(|s| s.total_changes()).unwrap_or(0);
                let sub_ref = sub_st
                    .and_then(|s| s.branch.as_deref())
                    .or(sub.branch.as_deref())
                    .or(sub.head_oid.as_deref())
                    .unwrap_or(if sub.initialized { "detached" } else { "uninit" });
                let sub_est_w = tree::content_x(sub_depth)
                    + 22.0
                    + sub.path.chars().count() as f32 * 7.5
                    + (sub_ref.chars().count() + 2) as f32 * 6.9
                    + if sub_changed > 0 || sub_dirty { 24.0 } else { 0.0 };
                if sub_est_w > *max_row_w {
                    *max_row_w = sub_est_w;
                }
                let sub_icon_color = if !sub.initialized {
                    p.text_muted
                } else if sub_dirty {
                    p.warning_fg
                } else {
                    p.text_secondary
                };

                let sub_btn = button(
                    row![
                        container(icons::repo_stack(tree::LEAD_ICON_SIZE, sub_icon_color))
                            .width(Length::Fixed(tree::LEAD_W))
                            .center_x(Length::Fixed(tree::LEAD_W)),
                        Space::with_width(Length::Fixed(tree::LABEL_GAP)),
                        theme::clipped_line(
                            row![
                                text(sub.path.clone())
                                    .size(type_scale::BODY)
                                    .color(if sub.initialized {
                                        p.text_primary
                                    } else {
                                        p.text_muted
                                    }),
                                text(format!("({sub_ref})"))
                                    .size(type_scale::CAPTION)
                                    .color(if !sub.initialized {
                                        p.danger_fg
                                    } else {
                                        p.text_muted
                                    }),
                            ]
                            .spacing(5)
                            .align_y(Alignment::Center),
                            Length::Fill,
                        ),
                        if sub_changed > 0 {
                            Element::from(
                                container(
                                    text(format!("{sub_changed}"))
                                        .size(type_scale::BADGE)
                                        .color(p.warning_fg),
                                )
                                .padding(Padding {
                                    top: 0.0,
                                    right: 0.0,
                                    bottom: 0.0,
                                    left: 5.0,
                                }),
                            )
                        } else if sub_dirty {
                            Element::from(
                                container(
                                    container(Space::new(7, 7))
                                        .style(theme::dot_box(p.warning_fg)),
                                )
                                .padding(Padding {
                                    top: 0.0,
                                    right: 0.0,
                                    bottom: 0.0,
                                    left: 5.0,
                                }),
                            )
                        } else {
                            Space::with_width(Length::Shrink).into()
                        },
                    ]
                    .spacing(0)
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding([tree::ROW_PAD_V, tree::ROW_PAD_H])
                .style(theme::repo_row_btn(p, sub_selected))
                .on_press(Message::SelectSubmodule(repo.id.clone(), sub.path.clone()));

                let mut sub_tip = column![
                    text(format!("Submodule: {}", sub.path))
                        .size(type_scale::BODY)
                        .color(p.text_primary),
                    text(format!(
                        "Ref: {sub_ref}{}",
                        sub.head_oid
                            .as_deref()
                            .map(|o| format!("  ({o})"))
                            .unwrap_or_default()
                    ))
                    .size(type_scale::CAPTION)
                    .color(p.accent_fg),
                ]
                .spacing(2);
                if !sub.url.is_empty() {
                    sub_tip = sub_tip.push(
                        text(sub.url.clone())
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                    );
                }
                sub_tip = sub_tip.push(
                    text(if !sub.initialized {
                        "Not initialized — run Update Submodules"
                    } else if sub_dirty {
                        "Dirty submodule — click to open"
                    } else {
                        "Click to open submodule in GitFleet"
                    })
                    .size(type_scale::CAPTION)
                    .color(if sub_dirty {
                        p.warning_fg
                    } else {
                        p.text_muted
                    }),
                );

                out.push(
                    row![
                        Space::with_width(Length::Fixed(sub_btn_left)),
                        tooltip(
                            sub_btn,
                            container(sub_tip)
                                .padding([6, 9])
                                .style(theme::tooltip_box(p)),
                            tooltip::Position::Right,
                        )
                        .gap(6),
                    ]
                    .align_y(Alignment::Center)
                    .into(),
                );
            }
        }
    }

    fn render_group_subtree<'a>(
        &'a self,
        node: &RepoGroupNode,
        parent_path: &str,
        depth: usize,
        collapsed_groups: &[String],
        p: Palette,
        out: &mut Vec<Element<'a, Message>>,
        max_row_w: &mut f32,
    ) {
        let dragging = self.dragging_repo.is_some();

        // Subgroups first (alphabetical via BTreeMap)
        for (seg_name, child) in &node.subgroups {
            let full_path = if parent_path.is_empty() {
                seg_name.clone()
            } else {
                format!("{parent_path}/{seg_name}")
            };
            let total = child.total_repos();
            if total == 0 && !self.filter_query.is_empty() {
                continue;
            }
            let is_collapsed = collapsed_groups.contains(&full_path);
            let is_drop_target = dragging
                && self
                    .drag_hover_group
                    .as_ref()
                    .is_some_and(|(_, t)| t.as_deref() == Some(full_path.as_str()));
            let left_x = tree::chevron_x(depth);
            let count_str = format!("({total})");
            let grp_est_w = tree::content_x(depth)
                + 20.0
                + seg_name.chars().count() as f32 * 7.5
                + (count_str.chars().count() + 1) as f32 * 6.9;
            if grp_est_w > *max_row_w {
                *max_row_w = grp_est_w;
            }

            let chevron = container(if is_collapsed {
                icons::chevron_right(tree::CHEVRON_ICON_SIZE, p.text_muted)
            } else {
                icons::chevron_down(tree::CHEVRON_ICON_SIZE, p.text_muted)
            })
            .width(Length::Fixed(tree::CHEVRON_W))
            .center_x(Length::Fixed(tree::CHEVRON_W));

            let folder_icon = container(if is_collapsed {
                icons::folder(
                    tree::LEAD_ICON_SIZE,
                    if is_drop_target {
                        p.success_fg
                    } else {
                        p.accent_fg
                    },
                )
            } else {
                icons::folder_open(
                    tree::LEAD_ICON_SIZE,
                    if is_drop_target {
                        p.success_fg
                    } else {
                        p.accent_fg
                    },
                )
            })
            .width(Length::Fixed(tree::LEAD_W))
            .center_x(Length::Fixed(tree::LEAD_W));

            let mut header_row = row![
                chevron,
                Space::with_width(Length::Fixed(tree::CHEVRON_GAP)),
                folder_icon,
                Space::with_width(Length::Fixed(tree::LABEL_GAP)),
                theme::clipped_line(
                    row![
                        text(seg_name.clone())
                            .size(type_scale::BODY)
                            .color(if is_drop_target {
                                p.accent_fg
                            } else {
                                p.text_secondary
                            }),
                        text(count_str)
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                    Length::Fill,
                ),
            ]
            .spacing(0)
            .align_y(Alignment::Center);

            if is_drop_target {
                header_row = header_row.push(theme::badge(
                    "Drop",
                    p.fill_selected,
                    p.accent_fg,
                    p.accent_primary,
                    false,
                ));
            }

            let group_btn = button(header_row)
                .width(Length::Fill)
                .padding(Padding {
                    top: tree::ROW_PAD_V,
                    right: 6.0,
                    bottom: tree::ROW_PAD_V,
                    left: left_x,
                })
                .style(theme::repo_row_btn(p, is_drop_target))
                .on_press(Message::ToggleGroupCollapse(full_path.clone()));

            let group_token = format!("grp:{full_path}");
            out.push(
                mouse_area(group_btn)
                    .on_enter(Message::DragHoverGroup(
                        group_token.clone(),
                        Some(full_path.clone()),
                    ))
                    .on_exit(Message::DragHoverGroupExit(group_token))
                    .on_right_press(Message::OpenContextMenu(ContextMenuTarget::Group {
                        name: full_path.clone(),
                    }))
                    .into(),
            );

            if !is_collapsed {
                self.render_group_subtree(
                    child,
                    &full_path,
                    depth + 1,
                    collapsed_groups,
                    p,
                    out,
                    max_row_w,
                );
            }
        }

        // Direct repositories inside this group (same depth as sibling subgroups!)
        let current_group = if parent_path.is_empty() {
            None
        } else {
            Some(parent_path.to_string())
        };
        for repo in &node.repos {
            self.render_repo_entry(repo, current_group.clone(), depth, p, out, max_row_w);
        }
    }

    pub(crate) fn view_repositories_pane(&self, p: Palette, repos: &[RepoEntry]) -> Element<'_, Message> {
        let header = row![
            theme::clipped_line(
                tip(
                    text("Repositories")
                        .size(type_scale::BODY)
                        .color(p.text_muted),
                    "Right-click any group, repository, or empty space for context menu",
                    tooltip::Position::Bottom,
                    p,
                ),
                Length::Fill,
            ),
            tip(
                button(icons::folder_plus(15.0, p.text_secondary))
                    .padding([4, 6])
                    .style(theme::vscode_icon_btn(p, false))
                    .on_press(Message::PickAddRepo),
                "Add a single local Git repository",
                tooltip::Position::Bottom,
                p,
            ),
            tip(
                button(icons::folder_search(15.0, p.text_secondary))
                    .padding([4, 6])
                    .style(theme::vscode_icon_btn(p, false))
                    .on_press(Message::PickScanFolder(None)),
                "Scan a folder for Git repositories and auto-create groups",
                tooltip::Position::Bottom,
                p,
            ),
        ]
        .spacing(2)
        .align_y(Alignment::Center);

        let filter = text_input("Filter repos…", &self.filter_query)
            .on_input(Message::FilterChanged)
            .padding([5, 8])
            .size(type_scale::BODY)
            .style(theme::dark_input(p));

        let dirty_cnt = repos
            .iter()
            .filter(|r| self.statuses.get(&r.id).is_some_and(|s| !s.is_clean))
            .count();
        let sync_cnt = repos
            .iter()
            .filter(|r| {
                self.statuses
                    .get(&r.id)
                    .is_some_and(|s| s.ahead > 0 || s.behind > 0)
            })
            .count();

        let pills = row![
            button(
                text(format!("All ({})", repos.len()))
                    .size(type_scale::CAPTION)
                    .wrapping(text::Wrapping::None)
            )
            .padding([3, 8])
            .style(theme::filter_pill_btn(p, self.repo_filter == RepoFilter::All))
            .on_press(Message::SetRepoFilter(RepoFilter::All)),
            button(
                text(format!("Dirty ({dirty_cnt})"))
                    .size(type_scale::CAPTION)
                    .wrapping(text::Wrapping::None)
            )
            .padding([3, 8])
            .style(theme::filter_pill_btn(
                p,
                self.repo_filter == RepoFilter::Dirty
            ))
            .on_press(Message::SetRepoFilter(RepoFilter::Dirty)),
            button(
                text(format!("Sync ({sync_cnt})"))
                    .size(type_scale::CAPTION)
                    .wrapping(text::Wrapping::None)
            )
            .padding([3, 8])
            .style(theme::filter_pill_btn(p, self.repo_filter == RepoFilter::Sync))
            .on_press(Message::SetRepoFilter(RepoFilter::Sync)),
        ]
        .spacing(4);

        let ws = self.service.workspace.current();
        let visible = self.filtered_repos(repos);
        let mut group_tree = RepoGroupNode::default();
        for g in &ws.groups {
            let trimmed = g.trim();
            if !trimmed.is_empty() {
                group_tree.ensure_group(trimmed);
            }
        }
        for repo in visible {
            let group_opt = repo
                .group
                .as_deref()
                .map(str::trim)
                .filter(|g| !g.is_empty())
                .map(str::to_string);
            group_tree.insert_repo(group_opt.as_deref(), repo);
        }

        let mut rows: Vec<Element<'_, Message>> = Vec::new();
        if self.show_inline_group_input {
            let placeholder = if let Some(parent) = &self.new_group_parent {
                format!("Subgroup in {parent}/…")
            } else {
                "New group (supports parent/child)…".to_string()
            };
            let mut creator_col = column![].spacing(3);
            if let Some(parent) = &self.new_group_parent {
                creator_col = creator_col.push(
                    text(format!("Creating subgroup inside '{parent}'"))
                        .size(type_scale::CAPTION)
                        .color(p.accent_fg),
                );
            }
            creator_col = creator_col.push(
                row![
                    text_input(&placeholder, &self.new_group_input)
                        .id(self.inline_group_input_id.clone())
                        .on_input(Message::NewGroupNameChanged)
                        .on_submit(Message::AddGroupSubmitted)
                        .padding([4, 7])
                        .size(type_scale::CAPTION)
                        .style(theme::dark_input(p)),
                    button(text("Create").size(type_scale::CAPTION))
                        .padding([4, 8])
                        .style(theme::primary_btn(p))
                        .on_press(Message::AddGroupSubmitted),
                    button(icons::close_x(12.0, p.text_secondary))
                        .padding([4, 6])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::ToggleInlineGroupCreator),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
            );
            rows.push(creator_col.into());
        }
        if let Some(reloc) = &self.pending_relocation {
            rows.push(
                container(
                    column![
                        text(format!(
                            "Relocate {} missing sibling repo(s)?",
                            reloc.affected_repo_ids.len()
                        ))
                        .size(type_scale::CAPTION)
                        .color(p.warning_fg),
                        row![
                            button(text("Apply All").size(type_scale::CAPTION))
                                .padding([3, 7])
                                .style(theme::primary_btn(p))
                                .on_press(Message::ApplyPrefixRelocation),
                            button(text("Dismiss").size(type_scale::CAPTION))
                                .padding([3, 7])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::DismissPrefixRelocation),
                        ]
                        .spacing(4),
                    ]
                    .spacing(4),
                )
                .padding(6)
                .width(Length::Fill)
                .style(theme::card_box(p))
                .into(),
            );
        }

        // Render nested groups tree followed directly by ungrouped repos at root level (SmartGit-style)
        let mut max_row_w: f32 = 180.0;
        self.render_group_subtree(
            &group_tree,
            "",
            0,
            &ws.ui.collapsed_groups,
            p,
            &mut rows,
            &mut max_row_w,
        );

        // Only while actively dragging a repo, reveal a root drop target to ungroup it
        if self.dragging_repo.is_some() {
            let is_drop_target = self
                .drag_hover_group
                .as_ref()
                .is_some_and(|(_, t)| t.is_none());
            let root_drop = container(
                row![
                    theme::clipped_line(
                        text(if is_drop_target {
                            "Release to move to root (no group)"
                        } else {
                            "Drop here to remove from group (root)"
                        })
                        .size(type_scale::CAPTION)
                        .color(if is_drop_target {
                            p.accent_fg
                        } else {
                            p.text_muted
                        }),
                        Length::Fill,
                    ),
                    if is_drop_target {
                        Element::from(theme::badge(
                            "Drop",
                            p.fill_selected,
                            p.accent_fg,
                            p.accent_primary,
                            false,
                        ))
                    } else {
                        Space::with_width(Length::Shrink).into()
                    },
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding([6, 8])
            .style(move |t| {
                if is_drop_target {
                    theme::pill_box(p.fill_selected, p.accent_fg, p.accent_primary)(t)
                } else {
                    theme::section_panel_box(p)(t)
                }
            });

            rows.push(
                mouse_area(root_drop)
                    .on_enter(Message::DragHoverGroup("root".to_string(), None))
                    .on_exit(Message::DragHoverGroupExit("root".to_string()))
                    .into(),
            );
        }

        let rows_col = column(rows).spacing(tree::ROW_SPACING);
        let min_content_w = max_row_w.clamp(180.0, 960.0);

        let scroll_area = mouse_area(
            container(slim_scroll_h(rows_col, min_content_w, p))
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_right_press(Message::OpenContextMenu(ContextMenuTarget::RepositoriesPane));

        let mut col = column![
            header,
            filter,
            pills,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            scroll_area
        ]
        .spacing(6);

        if !self.bulk_selected.is_empty() {
            let bulk_count = self.bulk_selected.len();
            col = col.push(
                container(
                    row![
                        text(format!("{bulk_count} sel"))
                            .size(type_scale::CAPTION)
                            .color(p.accent_fg),
                        Space::with_width(Length::Fill),
                        tip(
                            button(icons::fetch(14.0, iced::Color::WHITE))
                                .padding([4, 6])
                                .style(theme::primary_btn(p))
                                .on_press(Message::RunBulkOp(BulkOperation::Fetch)),
                            "Bulk Fetch selected repositories",
                            tooltip::Position::Top,
                            p,
                        ),
                        tip(
                            button(icons::pull(14.0, p.text_primary))
                                .padding([4, 6])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::RequestConfirm(PendingConfirm::bulk_op(
                                    BulkOperation::Pull,
                                    bulk_count,
                                ))),
                            "Bulk Pull selected repositories…",
                            tooltip::Position::Top,
                            p,
                        ),
                        tip(
                            button(icons::push(14.0, p.text_primary))
                                .padding([4, 6])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::RequestConfirm(PendingConfirm::bulk_op(
                                    BulkOperation::Push,
                                    bulk_count,
                                ))),
                            "Bulk Push selected repositories…",
                            tooltip::Position::Top,
                            p,
                        ),
                        tip(
                            button(icons::trash(14.0, p.danger_fg))
                                .padding([4, 6])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::RequestConfirm(PendingConfirm::remove_bulk(
                                    bulk_count,
                                ))),
                            "Remove selected repositories from workspace",
                            tooltip::Position::Top,
                            p,
                        ),
                        tip(
                            button(icons::close_x(13.0, p.text_secondary))
                                .padding([4, 6])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::ClearBulkSelection),
                            "Clear selection",
                            tooltip::Position::Top,
                            p,
                        ),
                    ]
                    .spacing(3)
                    .align_y(Alignment::Center),
                )
                .padding(5)
                .width(Length::Fill)
                .style(theme::card_box(p)),
            );
        }

        mouse_area(container(col).padding(8).height(Length::Fill))
            .on_right_press(Message::OpenContextMenu(ContextMenuTarget::RepositoriesPane))
            .into()
    }
}
