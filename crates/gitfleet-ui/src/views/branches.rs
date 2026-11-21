use crate::app::{BottomLeftTab, ContextMenuTarget, GitFleetApp, Message, PendingConfirm};
use crate::icons;
use crate::theme::{self, Palette, icon_size, type_scale};
use crate::views::{slim_scroll_h, tip};
use gitfleet_git::{BranchInfo, WorktreeInfo};
use iced::widget::{
    Space, button, checkbox, column, container, horizontal_rule, mouse_area, row, scrollable, text,
    text_input, tooltip,
};
use iced::{Alignment, Color, Element, Font, Length, Padding, font};
use std::collections::{BTreeMap, HashSet};

const BOLD_FONT: Font = Font {
    weight: font::Weight::Bold,
    ..Font::DEFAULT
};

#[cfg(test)]
const GROUP_CHECKBOX_X: f32 = 19.0; // chevron (14px) + row spacing (5px)
const TREE_INDENT_STEP: f32 = 18.0;

fn branch_folder_indent(depth: usize) -> f32 {
    (depth as f32) * TREE_INDENT_STEP
}

fn branch_leaf_indent(depth: usize) -> f32 {
    // Shift children inside their parent group so their checkbox aligns under the parent label
    // rather than flush with the parent group's checkbox (matching SmartGit's branch tree).
    (depth as f32) * TREE_INDENT_STEP + TREE_INDENT_STEP
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpstreamHintKind {
    Synced,
    Ahead,
    Behind,
    Diverged,
    Gone,
}

#[derive(Default)]
struct BranchTreeNode<'a> {
    folders: BTreeMap<String, BranchTreeNode<'a>>,
    leaves: Vec<(&'a str, &'a BranchInfo)>,
}

impl<'a> BranchTreeNode<'a> {
    fn insert(&mut self, display_path: &'a str, info: &'a BranchInfo) {
        if let Some((head, tail)) = display_path.split_once('/')
            && !head.is_empty()
            && !tail.is_empty()
        {
            self.folders
                .entry(head.to_string())
                .or_default()
                .insert(tail, info);
        } else {
            self.leaves.push((display_path, info));
        }
    }

    fn count(&self) -> usize {
        self.leaves.len() + self.folders.values().map(Self::count).sum::<usize>()
    }

    fn contains_head(&self) -> bool {
        self.leaves.iter().any(|(_, b)| b.is_head)
            || self.folders.values().any(Self::contains_head)
    }

    fn collect_names(&self, out: &mut Vec<String>) {
        for (_, b) in &self.leaves {
            out.push(b.name.clone());
        }
        for child in self.folders.values() {
            child.collect_names(out);
        }
    }
}

struct BranchRenderCtx<'a> {
    is_remote: bool,
    auto_expand_small: bool,
    toggled: &'a HashSet<String>,
    checked_branches: &'a HashSet<String>,
    selected_branch: Option<&'a str>,
    worktrees: &'a [WorktreeInfo],
    p: Palette,
}

fn classify_upstream_hint(branch: &BranchInfo) -> Option<(UpstreamHintKind, String)> {
    let up = branch.upstream.as_deref()?;
    let target = if let Some((remote, rest)) = up.split_once('/')
        && rest == branch.name
    {
        remote
    } else {
        up
    };
    if branch.upstream_gone {
        return Some((UpstreamHintKind::Gone, format!("? {target}")));
    }
    let (kind, op) = match (branch.ahead > 0, branch.behind > 0) {
        (false, false) => (UpstreamHintKind::Synced, "="),
        (true, false) => (UpstreamHintKind::Ahead, ">"),
        (false, true) => (UpstreamHintKind::Behind, "<"),
        (true, true) => (UpstreamHintKind::Diverged, "<>"),
    };
    Some((kind, format!("{op} {target}")))
}

#[cfg(test)]
fn format_upstream_hint(branch: &BranchInfo) -> Option<String> {
    classify_upstream_hint(branch).map(|(_, label)| label)
}

fn format_tracking_state(branch: &BranchInfo) -> Option<String> {
    if branch.upstream.is_none() {
        return Some("Local only (not tracking a remote)".to_string());
    }
    if branch.upstream_gone {
        return Some("Remote branch deleted (gone)".to_string());
    }
    match (branch.ahead, branch.behind) {
        (0, 0) => Some("Up to date".to_string()),
        (a, 0) => Some(format!(
            "{a} commit{} ahead",
            if a == 1 { "" } else { "s" }
        )),
        (0, b) => Some(format!(
            "{b} commit{} behind",
            if b == 1 { "" } else { "s" }
        )),
        (a, b) => Some(format!(
            "{a} commit{} ahead, {b} commit{} behind",
            if a == 1 { "" } else { "s" },
            if b == 1 { "" } else { "s" }
        )),
    }
}

fn linked_worktree_for_branch<'a>(
    branch: &'a BranchInfo,
    worktrees: &'a [WorktreeInfo],
) -> Option<(String, bool)> {
    if let Some(wt) = worktrees
        .iter()
        .find(|w| !w.is_main && w.branch.as_deref() == Some(branch.name.as_str()))
    {
        return Some((wt.path.display().to_string(), wt.dirty));
    }
    if !branch.is_head
        && let Some(wt_path) = &branch.worktree_path
    {
        return Some((wt_path.clone(), false));
    }
    None
}

fn branch_tooltip_card<'a>(
    b: &BranchInfo,
    linked_wt: Option<&(String, bool)>,
    p: Palette,
) -> Element<'a, Message> {
    let mut lines = column![].spacing(3);
    if let Some((wt_path, wt_dirty)) = linked_wt {
        lines = lines.push(
            row![
                text("Worktree:")
                    .size(type_scale::CAPTION)
                    .color(p.text_secondary)
                    .width(Length::Fixed(92.0)),
                text(if *wt_dirty {
                    format!("{wt_path} (dirty)")
                } else {
                    wt_path.clone()
                })
                .size(type_scale::CAPTION)
                .color(if *wt_dirty { p.warning_fg } else { p.text_primary }),
            ]
            .spacing(6),
        );
    }
    lines = lines.push(
        row![
            text("Local Branch:")
                .size(type_scale::CAPTION)
                .color(p.text_secondary)
                .width(Length::Fixed(92.0)),
            text(b.name.clone())
                .size(type_scale::CAPTION)
                .color(p.text_primary),
        ]
        .spacing(6),
    );
    if let Some(up) = &b.upstream {
        lines = lines.push(
            row![
                text("Tracked Branch:")
                    .size(type_scale::CAPTION)
                    .color(p.text_secondary)
                    .width(Length::Fixed(92.0)),
                text(up.clone())
                    .size(type_scale::CAPTION)
                    .color(p.accent_fg),
            ]
            .spacing(6),
        );
    }
    if let Some(state) = format_tracking_state(b) {
        lines = lines.push(
            row![
                text("Tracking State:")
                    .size(type_scale::CAPTION)
                    .color(p.text_secondary)
                    .width(Length::Fixed(92.0)),
                text(state).size(type_scale::CAPTION).color(
                    if b.upstream.is_none() {
                        p.text_muted
                    } else if b.upstream_gone {
                        p.danger_fg
                    } else if b.behind > 0 {
                        p.warning_fg
                    } else if b.ahead > 0 {
                        p.accent_fg
                    } else {
                        p.success_fg
                    }
                ),
            ]
            .spacing(6),
        );
    }
    let hint = if b.is_head {
        "Current checked-out branch (HEAD)"
    } else if linked_wt.is_some() {
        "Checked out in linked worktree · Right-click for actions"
    } else {
        "Double-click to checkout · Right-click for actions"
    };
    lines = lines.push(
        text(hint)
            .size(type_scale::BADGE)
            .color(p.text_muted),
    );

    container(lines)
        .padding([6, 10])
        .style(theme::tooltip_box(p))
        .into()
}

fn render_branch_subtree<'a>(
    node: &BranchTreeNode<'a>,
    key_prefix: &str,
    depth: usize,
    ctx: &BranchRenderCtx<'_>,
    out: &mut Vec<Element<'a, Message>>,
    max_row_w: &mut f32,
) {
    let p = ctx.p;
    // Folders first (alphabetical via BTreeMap), matching SmartGit's branch tree
    for (folder, child) in &node.folders {
        let folder_key = format!("{key_prefix}/{folder}");
        let default_expanded = !ctx.is_remote && (child.contains_head() || ctx.auto_expand_small);
        let expanded = default_expanded ^ ctx.toggled.contains(&folder_key);
        let indent = branch_folder_indent(depth);
        let folder_est_w = indent + 38.0 + (folder.chars().count() + 1) as f32 * 7.2 + 30.0;
        if folder_est_w > *max_row_w {
            *max_row_w = folder_est_w;
        }
        let chevron: Element<'a, Message> = if expanded {
            icons::chevron_down(icon_size::XS, p.text_muted).into()
        } else {
            icons::chevron_right(icon_size::XS, p.text_muted).into()
        };

        let folder_btn = button(
            row![
                chevron,
                theme::clipped_line(
                    text(format!("{folder}/"))
                        .size(type_scale::BODY)
                        .color(p.text_secondary),
                    Length::Fill,
                ),
                text(format!("({})", child.count()))
                    .size(type_scale::BADGE)
                    .color(p.text_muted),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding(Padding {
            top: 3.0,
            right: 5.0,
            bottom: 3.0,
            left: indent,
        })
        .style(theme::toolbar_btn(p, false))
        .on_press(Message::ToggleBranchTreeNode(folder_key.clone()));

        if ctx.is_remote {
            out.push(folder_btn.into());
        } else {
            let prefix = format!(
                "{}/",
                folder_key.strip_prefix("local/").unwrap_or(&folder_key)
            );
            out.push(
                mouse_area(folder_btn)
                    .on_right_press(Message::OpenContextMenu(
                        ContextMenuTarget::LocalBranchFolder {
                            folder_key: folder_key.clone(),
                            prefix,
                        },
                    ))
                    .into(),
            );
        }

        if expanded {
            render_branch_subtree(child, &folder_key, depth + 1, ctx, out, max_row_w);
        }
    }

    // Leaf branches at this depth (HEAD first, then alphabetical)
    let mut leaves = node.leaves.clone();
    leaves.sort_by(|(la, ba), (lb, bb)| bb.is_head.cmp(&ba.is_head).then_with(|| la.cmp(lb)));

    let leaf_indent = branch_leaf_indent(depth);
    for (leaf_name, b) in leaves {
        let linked_wt = if ctx.is_remote {
            None
        } else {
            linked_worktree_for_branch(b, ctx.worktrees)
        };
        let is_checked = ctx.checked_branches.contains(&b.name);
        let is_selected = !ctx.is_remote
            && (ctx.selected_branch == Some(b.name.as_str())
                || (ctx.selected_branch.is_none() && b.is_head));

        let fg = if b.is_head {
            p.text_primary
        } else if ctx.is_remote {
            p.text_secondary
        } else {
            p.text_primary
        };

        let mut name_and_up = row![].spacing(5).align_y(Alignment::Center);

        if b.is_head {
            name_and_up = name_and_up.push(icons::play_triangle(9.0, p.text_primary));
        }

        let name_txt = text(leaf_name.to_string())
            .size(type_scale::BODY)
            .color(fg);
        name_and_up = name_and_up.push(if b.is_head {
            name_txt.font(BOLD_FONT)
        } else {
            name_txt
        });

        if let Some((_, wt_dirty)) = &linked_wt {
            name_and_up = name_and_up.push(
                row![
                    text("(").size(type_scale::BADGE).color(p.text_muted),
                    container(Space::new(6, 6)).style(theme::dot_box(if *wt_dirty {
                        p.warning_fg
                    } else {
                        p.success_fg
                    })),
                    text("worktree)").size(type_scale::BADGE).color(p.text_muted),
                ]
                .spacing(3)
                .align_y(Alignment::Center),
            );
        }

        let upstream_info = if ctx.is_remote {
            None
        } else {
            classify_upstream_hint(b)
        };
        let hint_chars = upstream_info
            .as_ref()
            .map(|(_, h)| h.chars().count() + 1)
            .unwrap_or(0);
        let wt_extra = if linked_wt.is_some() { 76.0 } else { 0.0 };
        let badges_w = if ctx.is_remote {
            0.0
        } else {
            (if b.ahead > 0 { 36.0 } else { 0.0 }) + (if b.behind > 0 { 38.0 } else { 0.0 })
        };
        let leaf_est_w = leaf_indent
            + 38.0
            + if b.is_head { 16.0 } else { 0.0 }
            + (leaf_name.chars().count() + hint_chars) as f32 * 7.2
            + wt_extra
            + badges_w;
        if leaf_est_w > *max_row_w {
            *max_row_w = leaf_est_w;
        }

        if let Some((kind, hint)) = upstream_info {
            let hint_color = match kind {
                UpstreamHintKind::Gone => p.danger_fg,
                UpstreamHintKind::Synced => p.text_muted,
                UpstreamHintKind::Ahead => p.accent_fg,
                UpstreamHintKind::Behind | UpstreamHintKind::Diverged => p.warning_fg,
            };
            name_and_up = name_and_up.push(
                text(hint)
                    .size(type_scale::CAPTION)
                    .color(hint_color),
            );
        }

        let mut leaf_content = row![theme::clipped_line(name_and_up, Length::Fill)]
            .spacing(5)
            .align_y(Alignment::Center);

        if !ctx.is_remote {
            if b.ahead > 0 {
                leaf_content = leaf_content.push(theme::badge(
                    format!("+{}", b.ahead),
                    p.fill_selected,
                    p.accent_fg,
                    p.accent_primary,
                    true,
                ));
            }
            if b.behind > 0 {
                leaf_content = leaf_content.push(theme::badge(
                    format!("-{}", b.behind),
                    p.surface_card,
                    p.text_primary,
                    p.border_strong,
                    true,
                ));
            }
        }

        let b_name_cb = b.name.clone();
        let cb = checkbox("", is_checked)
            .size(12)
            .spacing(0)
            .on_toggle(move |_| Message::ToggleGraphBranch(b_name_cb.clone()));

        let main_btn: Element<'a, Message> = if ctx.is_remote {
            button(leaf_content)
                .width(Length::Fill)
                .padding([3, 5])
                .style(theme::repo_row_btn(p, is_checked))
                .on_press(Message::ToggleGraphBranch(b.name.clone()))
                .into()
        } else {
            let btn = button(leaf_content)
                .width(Length::Fill)
                .padding([3, 5])
                .style(theme::repo_row_btn(p, is_selected))
                .on_press(Message::BranchClicked(b.name.clone()));
            tooltip(
                btn,
                branch_tooltip_card(b, linked_wt.as_ref(), p),
                tooltip::Position::Right,
            )
            .gap(6)
            .into()
        };

        let full_row = container(
            row![cb, main_btn]
                .spacing(5)
                .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding(Padding {
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            left: leaf_indent,
        });

        out.push(
            mouse_area(full_row)
                .on_right_press(Message::OpenContextMenu(ContextMenuTarget::Branch {
                    name: b.name.clone(),
                    is_head: b.is_head,
                    is_remote: ctx.is_remote,
                }))
                .into(),
        );
    }
}

impl GitFleetApp {
    pub(crate) fn view_branches_pane(&self, p: Palette) -> Element<'_, Message> {
        // VS Code-style underline tabs in a horizontally scrollable header strip
        let pane_tab = |label: &'static str, count: usize, tab: BottomLeftTab| -> Element<'_, Message> {
            let active = self.bottom_left_tab == tab;
            let text_str = if count > 0 {
                format!("{label} {count}")
            } else {
                label.to_string()
            };
            let underline_color = if active {
                p.accent_primary
            } else {
                Color::TRANSPARENT
            };
            let label_box = container(
                text(text_str)
                    .size(type_scale::CAPTION)
                    .wrapping(text::Wrapping::None)
                    .color(if active { p.text_primary } else { p.text_muted }),
            )
            .padding([4, 6]);
            button(theme::bottom_underline(label_box, underline_color, 2.0))
                .padding(0)
                .style(theme::vscode_dock_tab(p, active))
                .on_press(Message::SetBottomLeftTab(tab))
                .into()
        };

        let header = scrollable(
            row![
                pane_tab("Branches", 0, BottomLeftTab::Branches),
                pane_tab("Stash", self.stashes.len(), BottomLeftTab::Stashes),
                pane_tab("Tags", self.tags.len(), BottomLeftTab::Tags),
                pane_tab("Submodules", self.submodules.len(), BottomLeftTab::Submodules),
                pane_tab("Worktrees", self.worktrees.len(), BottomLeftTab::Worktrees),
            ]
            .spacing(2)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Shrink)
        .direction(theme::slim_horizontal_direction())
        .style(theme::slim_scrollable(p));

        let body: Element<'_, Message> = match self.bottom_left_tab {
            BottomLeftTab::Branches => {
                let mut rows: Vec<Element<'_, Message>> = Vec::new();
                let mut local_tree = BranchTreeNode::default();
                let mut remote_groups: BTreeMap<String, BranchTreeNode<'_>> = BTreeMap::new();

                for b in &self.branches {
                    if !b.is_remote {
                        local_tree.insert(&b.name, b);
                    } else if !b.name.ends_with("/HEAD") {
                        if let Some((remote, rest)) = b.name.split_once('/') {
                            remote_groups
                                .entry(remote.to_string())
                                .or_default()
                                .insert(rest, b);
                        } else {
                            remote_groups
                                .entry("remote".to_string())
                                .or_default()
                                .insert(&b.name, b);
                        }
                    }
                }

                // 0. SmartGit-style HEAD row (checkable to include/exclude HEAD in Graph)
                let head_oid_short = self
                    .active_status()
                    .and_then(|s| s.head_oid.as_deref())
                    .map(|h| h.chars().take(7).collect::<String>())
                    .unwrap_or_default();
                let head_checked = self.graph_checked_branches.is_empty()
                    || self.graph_checked_branches.contains("HEAD");
                rows.push(
                    row![
                        Space::with_width(Length::Fixed(14.0)),
                        checkbox("", head_checked)
                            .size(12)
                            .spacing(0)
                            .on_toggle(|_| Message::ToggleGraphBranch("HEAD".into())),
                        theme::clipped_line(
                            row![
                                text("HEAD")
                                    .size(type_scale::BODY)
                                    .color(p.text_primary),
                                if !head_oid_short.is_empty() {
                                    Element::from(
                                        text(format!("({head_oid_short})"))
                                            .size(type_scale::BADGE)
                                            .font(theme::CODE_FONT)
                                            .color(p.text_muted),
                                    )
                                } else {
                                    Space::with_width(Length::Shrink).into()
                                },
                            ]
                            .spacing(5)
                            .align_y(Alignment::Center),
                            Length::Fill,
                        ),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center)
                    .into(),
                );

                // 1. Local Branches collapsible root + group checkbox
                let local_count = local_tree.count();
                let mut local_names = Vec::with_capacity(local_count);
                local_tree.collect_names(&mut local_names);
                let all_local_checked = !local_names.is_empty()
                    && local_names
                        .iter()
                        .all(|n| self.graph_checked_branches.contains(n));
                let local_expanded = !self.toggled_branch_nodes.contains("local");
                let local_chevron: Element<'_, Message> = if local_expanded {
                    icons::chevron_down(icon_size::XS, p.text_secondary).into()
                } else {
                    icons::chevron_right(icon_size::XS, p.text_secondary).into()
                };
                let local_names_cb = local_names.clone();
                let local_names_ctx = local_names.clone();
                let local_hdr_row = row![
                    button(local_chevron)
                        .padding([3, 1])
                        .width(Length::Fixed(14.0))
                        .style(theme::vscode_icon_btn(p, false))
                        .on_press(Message::ToggleBranchTreeNode("local".into())),
                    checkbox("", all_local_checked)
                        .size(12)
                        .spacing(0)
                        .on_toggle(move |_| {
                            Message::ToggleGraphBranchGroup(local_names_cb.clone())
                        }),
                    button(
                        row![
                            theme::clipped_line(
                                row![
                                    text("Local Branches")
                                        .size(type_scale::BODY)
                                        .font(BOLD_FONT)
                                        .color(p.text_primary),
                                    text(format!("({local_count})"))
                                        .size(type_scale::CAPTION)
                                        .color(p.text_muted),
                                ]
                                .spacing(4)
                                .align_y(Alignment::Center),
                                Length::Fill,
                            ),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .padding([3, 4])
                    .style(theme::toolbar_btn(p, false))
                    .on_press(Message::ToggleBranchTreeNode("local".into())),
                    tip(
                        button(icons::plus(11.0, p.text_secondary))
                            .padding([3, 5])
                            .style(theme::vscode_icon_btn(p, false))
                            .on_press(Message::OpenCreateBranch {
                                base_ref: None,
                                prefix: None,
                            }),
                        "Add Branch… (F7)",
                        tooltip::Position::Right,
                        p,
                    ),
                ]
                .spacing(5)
                .align_y(Alignment::Center);

                rows.push(
                    mouse_area(local_hdr_row)
                        .on_right_press(Message::OpenContextMenu(
                            ContextMenuTarget::LocalBranchesGroup {
                                local_names: local_names_ctx,
                            },
                        ))
                        .into(),
                );

                let mut max_branch_w: f32 = 180.0;
                if local_expanded {
                    let local_ctx = BranchRenderCtx {
                        is_remote: false,
                        auto_expand_small: local_count <= 10,
                        toggled: &self.toggled_branch_nodes,
                        checked_branches: &self.graph_checked_branches,
                        selected_branch: self.selected_branch.as_deref(),
                        worktrees: &self.worktrees,
                        p,
                    };
                    render_branch_subtree(
                        &local_tree,
                        "local",
                        1,
                        &local_ctx,
                        &mut rows,
                        &mut max_branch_w,
                    );
                }

                // 2. Per-remote collapsible trees (e.g. `origin (21)`) + group checkbox
                for (remote_name, rtree) in &remote_groups {
                    let rkey = format!("remote:{remote_name}");
                    let rexpanded = self.toggled_branch_nodes.contains(&rkey);
                    let rchevron: Element<'_, Message> = if rexpanded {
                        icons::chevron_down(icon_size::XS, p.text_muted).into()
                    } else {
                        icons::chevron_right(icon_size::XS, p.text_muted).into()
                    };
                    let mut remote_names = Vec::with_capacity(rtree.count());
                    rtree.collect_names(&mut remote_names);
                    let all_remote_checked = !remote_names.is_empty()
                        && remote_names
                            .iter()
                            .all(|n| self.graph_checked_branches.contains(n));
                    let remote_names_cb = remote_names.clone();

                    rows.push(Space::with_height(Length::Fixed(3.0)).into());
                    rows.push(
                        row![
                            button(rchevron)
                                .padding([3, 1])
                                .width(Length::Fixed(14.0))
                                .style(theme::vscode_icon_btn(p, false))
                                .on_press(Message::ToggleBranchTreeNode(rkey.clone())),
                            checkbox("", all_remote_checked)
                                .size(12)
                                .spacing(0)
                                .on_toggle(move |_| {
                                    Message::ToggleGraphBranchGroup(remote_names_cb.clone())
                                }),
                            button(
                                row![
                                    theme::clipped_line(
                                        row![
                                            text(remote_name.clone())
                                                .size(type_scale::BODY)
                                                .font(BOLD_FONT)
                                                .color(p.text_secondary),
                                            text(format!("({})", rtree.count()))
                                                .size(type_scale::CAPTION)
                                                .color(p.text_muted),
                                        ]
                                        .spacing(4)
                                        .align_y(Alignment::Center),
                                        Length::Fill,
                                    ),
                                ]
                                .spacing(5)
                                .align_y(Alignment::Center),
                            )
                            .width(Length::Fill)
                            .padding([3, 4])
                            .style(theme::toolbar_btn(p, false))
                            .on_press(Message::ToggleBranchTreeNode(rkey.clone())),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center)
                        .into(),
                    );
                    if rexpanded {
                        let remote_ctx = BranchRenderCtx {
                            is_remote: true,
                            auto_expand_small: false,
                            toggled: &self.toggled_branch_nodes,
                            checked_branches: &self.graph_checked_branches,
                            selected_branch: None,
                            worktrees: &[],
                            p,
                        };
                        render_branch_subtree(
                            rtree,
                            &rkey,
                            1,
                            &remote_ctx,
                            &mut rows,
                            &mut max_branch_w,
                        );
                    }
                }

                if self.branches.is_empty() {
                    rows.push(
                        text("No branches loaded.")
                            .size(type_scale::CAPTION)
                            .color(p.text_muted)
                            .into(),
                    );
                }

                // Keep the scroll list at a stable child index (slot 1) even when the multi-branch
                // action bar toggles visibility so checking branches never resets scroll position.
                let action_bar_slot: Element<'_, Message> = if !self.graph_checked_branches.is_empty()
                {
                    let mut checked_sorted: Vec<String> =
                        self.graph_checked_branches.iter().cloned().collect();
                    checked_sorted.sort();
                    let head_name = self
                        .branches
                        .iter()
                        .find(|b| !b.is_remote && b.is_head)
                        .map(|b| b.name.as_str());
                    let non_head_checked: Vec<String> = checked_sorted
                        .iter()
                        .filter(|n| n.as_str() != "HEAD" && Some(n.as_str()) != head_name)
                        .cloned()
                        .collect();
                    let deletable_locals: Vec<String> = non_head_checked
                        .iter()
                        .filter(|n| {
                            self.branches
                                .iter()
                                .any(|b| !b.is_remote && !b.is_head && &b.name == *n)
                        })
                        .cloned()
                        .collect();

                    let mut bar = row![
                        theme::badge(
                            format!("{} selected", self.graph_checked_branches.len()),
                            p.fill_selected,
                            p.accent_fg,
                            p.accent_primary,
                            false,
                        ),
                        Space::with_width(Length::Fill),
                    ]
                    .spacing(3)
                    .align_y(Alignment::Center);

                    if !non_head_checked.is_empty() {
                        bar = bar.push(tip(
                            button(icons::cherry_pick(12.0, p.accent_fg))
                                .padding([3, 5])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::OpenCherryPickModal {
                                    detail: non_head_checked.join("\n"),
                                    revisions: non_head_checked.clone(),
                                }),
                            "Cherry-Pick tip commit(s) of selected branch(es) onto HEAD…",
                            tooltip::Position::Top,
                            p,
                        ));
                    }

                    if checked_sorted.len() == 2 {
                        bar = bar.push(tip(
                            button(icons::file_diff(12.0, p.text_primary))
                                .padding([3, 5])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::CompareRefs(
                                    checked_sorted[0].clone(),
                                    checked_sorted[1].clone(),
                                )),
                            "Compare the 2 selected branches in Diff viewer",
                            tooltip::Position::Top,
                            p,
                        ));
                    } else if non_head_checked.len() == 1 {
                        bar = bar.push(tip(
                            button(icons::file_diff(12.0, p.text_primary))
                                .padding([3, 5])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::CompareRefs(
                                    "HEAD".into(),
                                    non_head_checked[0].clone(),
                                )),
                            "Compare selected branch with HEAD",
                            tooltip::Position::Top,
                            p,
                        ));
                    }

                    if non_head_checked.len() == 1 {
                        bar = bar.push(tip(
                            button(icons::git_merge(12.0, p.success_fg))
                                .padding([3, 5])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::OpenMergeWithTarget(
                                    non_head_checked[0].clone(),
                                )),
                            "Merge / Rebase Forecast with selected branch",
                            tooltip::Position::Top,
                            p,
                        ));
                    }

                    if !deletable_locals.is_empty() {
                        bar = bar.push(tip(
                            button(icons::trash(12.0, p.danger_fg))
                                .padding([3, 5])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::RequestDeleteBranches(deletable_locals)),
                            "Delete selected local branch(es)",
                            tooltip::Position::Top,
                            p,
                        ));
                    }

                    bar = bar.push(tip(
                        button(icons::close_x(11.0, p.text_muted))
                            .padding([3, 5])
                            .style(theme::vscode_icon_btn(p, false))
                            .on_press(Message::ClearGraphCheckedBranches),
                        "Clear branch selection (show Current branch in Graph)",
                        tooltip::Position::Top,
                        p,
                    ));

                    container(bar)
                        .padding([4, 6])
                        .width(Length::Fill)
                        .style(theme::card_box(p))
                        .into()
                } else {
                    container(Space::with_height(Length::Shrink))
                        .height(Length::Shrink)
                        .into()
                };

                column![
                    action_bar_slot,
                    slim_scroll_h(
                        column(rows).spacing(1),
                        max_branch_w.clamp(180.0, 960.0),
                        p,
                    ),
                ]
                .spacing(if self.graph_checked_branches.is_empty() {
                    0
                } else {
                    4
                })
                .height(Length::Fill)
                .into()
            }
            BottomLeftTab::Stashes => {
                let mut list = column![].spacing(4);
                if self.stashes.is_empty() {
                    list = list.push(
                        text("No stashes. Use 'Save Stash' in toolbar.")
                            .size(11)
                            .color(p.text_muted),
                    );
                }
                for st in &self.stashes {
                    list = list.push(
                        container(
                            column![
                                button(theme::clipped_line(
                                    row![
                                        text(st.ref_name.clone())
                                            .size(10)
                                            .font(theme::CODE_FONT)
                                            .color(p.accent_fg),
                                        text(st.message.clone()).size(11).color(p.text_primary),
                                    ]
                                    .spacing(6),
                                    Length::Fill,
                                ))
                                .width(Length::Fill)
                                .padding([2, 4])
                                .style(theme::repo_row_btn(p, false))
                                .on_press(Message::InspectStash(st.index)),
                                row![
                                    theme::clipped_line(
                                        text(st.date_relative.clone())
                                            .size(10)
                                            .color(p.text_muted),
                                        Length::Fill,
                                    ),
                                    tip(
                                        button(icons::play(11.0, p.text_primary))
                                            .padding([3, 5])
                                            .style(theme::subtle_btn(p))
                                            .on_press(Message::RequestConfirm(
                                                PendingConfirm::apply_stash(
                                                    st.index,
                                                    &st.ref_name,
                                                    &st.message,
                                                ),
                                            )),
                                        "Apply Stash (keep in stash list)…",
                                        tooltip::Position::Top,
                                        p,
                                    ),
                                    tip(
                                        button(icons::check(11.0, Color::WHITE))
                                            .padding([3, 5])
                                            .style(theme::primary_btn(p))
                                            .on_press(Message::RequestConfirm(
                                                PendingConfirm::pop_stash(
                                                    st.index,
                                                    &st.ref_name,
                                                    &st.message,
                                                ),
                                            )),
                                        "Pop Stash (apply & remove)…",
                                        tooltip::Position::Top,
                                        p,
                                    ),
                                    tip(
                                        button(icons::trash(11.0, p.danger_fg))
                                            .padding([3, 5])
                                            .style(theme::subtle_btn(p))
                                            .on_press(Message::RequestConfirm(
                                                PendingConfirm::drop_stash(
                                                    st.index,
                                                    &st.ref_name,
                                                    &st.message,
                                                ),
                                            )),
                                        "Drop Stash",
                                        tooltip::Position::Top,
                                        p,
                                    ),
                                ]
                                .spacing(4)
                                .align_y(Alignment::Center),
                            ]
                            .spacing(3),
                        )
                        .padding(5)
                        .style(theme::card_box(p)),
                    );
                }
                let max_stash_w = self
                    .stashes
                    .iter()
                    .map(|st| {
                        (st.ref_name.chars().count() + st.message.chars().count()) as f32 * 7.0
                            + 44.0
                    })
                    .fold(200.0_f32, f32::max)
                    .clamp(200.0, 960.0);
                slim_scroll_h(list, max_stash_w, p)
            }
            BottomLeftTab::Tags => {
                let mut col = column![
                    row![
                        text_input("New tag (e.g. v1.0.0)…", &self.new_tag_name)
                            .on_input(Message::NewTagNameChanged)
                            .on_submit(Message::CreateTagSubmitted)
                            .padding([3, 6])
                            .size(11)
                            .style(theme::dark_input(p)),
                        button(text("Create").size(10))
                            .padding([3, 7])
                            .style(theme::primary_btn(p))
                            .on_press(Message::CreateTagSubmitted),
                    ]
                    .spacing(4),
                ]
                .spacing(5);

                if !self.release_triggers.is_empty() {
                    let wf_names: Vec<String> = self
                        .release_triggers
                        .iter()
                        .map(|t| format!("{} ({})", t.workflow_file, t.tag_patterns.join(",")))
                        .collect();
                    col = col.push(
                        container(
                            text(format!("Release CI: {}", wf_names.join(" · ")))
                                .size(10)
                                .color(p.warning_fg),
                        )
                        .padding([3, 6])
                        .width(Length::Fill)
                        .style(theme::pill_box(p.warning_bg, p.warning_fg, p.border_default)),
                    );
                }

                let mut list = column![].spacing(3);
                for t in &self.tags {
                    list = list.push(
                        row![
                            theme::clipped_line(
                                row![
                                    icons::tag(11.0, p.warning_fg),
                                    text(t.name.clone()).size(11).color(p.warning_fg),
                                    text(t.target_oid.clone())
                                        .size(10)
                                        .font(theme::CODE_FONT)
                                        .color(p.text_muted),
                                ]
                                .spacing(5)
                                .align_y(Alignment::Center),
                                Length::Fill,
                            ),
                            tip(
                                button(icons::push(11.0, p.text_primary))
                                    .padding([3, 5])
                                    .style(theme::subtle_btn(p))
                                    .on_press(Message::RequestConfirm(PendingConfirm::push_tag(
                                        t.name.clone(),
                                    ))),
                                "Push tag to remote…",
                                tooltip::Position::Top,
                                p,
                            ),
                            tip(
                                button(icons::trash(11.0, p.danger_fg))
                                    .padding([3, 5])
                                    .style(theme::subtle_btn(p))
                                    .on_press(Message::RequestConfirm(PendingConfirm::delete_tag(
                                        t.name.clone(),
                                    ))),
                                "Delete tag",
                                tooltip::Position::Top,
                                p,
                            ),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                    );
                }
                if self.tags.is_empty() {
                    list = list.push(text("No tags yet.").size(11).color(p.text_muted));
                }
                let max_tag_w = self
                    .tags
                    .iter()
                    .map(|t| {
                        (t.name.chars().count() + t.target_oid.chars().count()) as f32 * 7.0 + 96.0
                    })
                    .fold(200.0_f32, f32::max)
                    .clamp(200.0, 960.0);
                col.push(slim_scroll_h(list, max_tag_w, p))
                    .height(Length::Fill)
                    .into()
            }
            BottomLeftTab::Submodules => {
                let mut col = column![
                    row![
                        tip(
                            button(icons::fetch(12.0, p.text_primary))
                                .padding([4, 7])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::UpdateSubmodules),
                            "Update Submodules (--init --recursive)",
                            tooltip::Position::Top,
                            p,
                        ),
                        tip(
                            button(icons::refresh(12.0, p.text_primary))
                                .padding([4, 7])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::SyncSubmodules),
                            "Sync Submodule URLs",
                            tooltip::Position::Top,
                            p,
                        ),
                        tip(
                            button(icons::shield(12.0, p.warning_fg))
                                .padding([4, 7])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::RunPushPreflight),
                            "Run Push Preflight (check orphan submodule commits)",
                            tooltip::Position::Top,
                            p,
                        ),
                        tip(
                            button(icons::push(12.0, Color::WHITE))
                                .padding([4, 7])
                                .style(theme::primary_btn(p))
                                .on_press(Message::RunOrderedPush),
                            "Ordered Push (submodules first, then superproject)",
                            tooltip::Position::Top,
                            p,
                        ),
                    ]
                    .spacing(4),
                    row![
                        text_input("Submodule URL…", &self.new_submodule_url)
                            .on_input(Message::SubmoduleUrlChanged)
                            .padding([3, 6])
                            .size(10)
                            .style(theme::dark_input(p)),
                        text_input("Path…", &self.new_submodule_path)
                            .on_input(Message::SubmodulePathChanged)
                            .on_submit(Message::AddSubmoduleSubmitted)
                            .padding([3, 6])
                            .size(10)
                            .width(Length::Fixed(72.0))
                            .style(theme::dark_input(p)),
                        tip(
                            button(icons::plus(11.0, Color::WHITE))
                                .padding([4, 6])
                                .style(theme::primary_btn(p))
                                .on_press(Message::AddSubmoduleSubmitted),
                            "Add Submodule",
                            tooltip::Position::Top,
                            p,
                        ),
                    ]
                    .spacing(3)
                    .align_y(Alignment::Center),
                ]
                .spacing(4);

                if !self.orphan_gitlinks.is_empty() {
                    col = col.push(
                        container(
                            text(format!(
                                "Orphan gitlinks ({}): click 'Ord Push'",
                                self.orphan_gitlinks.len()
                            ))
                            .size(10)
                            .color(p.danger_fg),
                        )
                        .padding([3, 6])
                        .width(Length::Fill)
                        .style(theme::pill_box(p.danger_bg, p.danger_fg, p.danger_fg)),
                    );
                }

                let mut list = column![].spacing(3);
                for s in &self.submodules {
                    list = list.push(
                        row![
                            theme::clipped_line(
                                row![
                                    text(s.path.clone()).size(11).color(p.text_primary),
                                    text(s.head_oid.clone().unwrap_or_default())
                                        .size(10)
                                        .font(theme::CODE_FONT)
                                        .color(p.accent_fg),
                                    if s.dirty {
                                        Element::from(text("*").size(11).color(p.warning_fg))
                                    } else {
                                        Space::with_width(Length::Shrink).into()
                                    },
                                ]
                                .spacing(4)
                                .align_y(Alignment::Center),
                                Length::Fill,
                            ),
                            tip(
                                button(icons::trash(11.0, p.danger_fg))
                                    .padding([3, 5])
                                    .style(theme::subtle_btn(p))
                                    .on_press(Message::RequestConfirm(
                                        PendingConfirm::remove_submodule(s.path.clone()),
                                    )),
                                "Remove submodule",
                                tooltip::Position::Top,
                                p,
                            ),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                    );
                }
                if self.submodules.is_empty() {
                    list = list.push(text("No submodules.").size(11).color(p.text_muted));
                }
                let max_sub_w = self
                    .submodules
                    .iter()
                    .map(|s| {
                        let oid_len = s.head_oid.as_deref().map_or(0, |o| o.chars().count());
                        (s.path.chars().count() + oid_len) as f32 * 7.0 + 68.0
                    })
                    .fold(200.0_f32, f32::max)
                    .clamp(200.0, 960.0);
                col.push(slim_scroll_h(list, max_sub_w, p))
                    .height(Length::Fill)
                    .into()
            }
            BottomLeftTab::Worktrees => {
                let col = column![
                    row![
                        text_input("Branch for new worktree…", &self.new_worktree_branch)
                            .on_input(Message::NewWorktreeBranchChanged)
                            .on_submit(Message::AddWorktreeSubmitted)
                            .padding([3, 6])
                            .size(10)
                            .style(theme::dark_input(p)),
                        tip(
                            button(icons::plus(11.0, Color::WHITE))
                                .padding([4, 6])
                                .style(theme::primary_btn(p))
                                .on_press(Message::AddWorktreeSubmitted),
                            "Create Linked Worktree",
                            tooltip::Position::Top,
                            p,
                        ),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                ]
                .spacing(5);

                let mut list = column![].spacing(3);
                for w in &self.worktrees {
                    list = list.push(
                        container(
                            column![
                                row![
                                    theme::clipped_line(
                                        row![
                                            text(w.name.clone()).size(11).color(p.text_primary),
                                            text(
                                                w.branch.clone().unwrap_or_else(|| "detached".into())
                                            )
                                            .size(10)
                                            .color(p.accent_fg),
                                        ]
                                        .spacing(5)
                                        .align_y(Alignment::Center),
                                        Length::Fill,
                                    ),
                                    if w.dirty {
                                        Element::from(text("dirty").size(9).color(p.warning_fg))
                                    } else {
                                        Element::from(text("clean").size(9).color(p.success_fg))
                                    },
                                    if !w.is_main {
                                        tip(
                                            button(icons::trash(11.0, p.danger_fg))
                                                .padding([3, 5])
                                                .style(theme::subtle_btn(p))
                                                .on_press(Message::RequestConfirm(
                                                    PendingConfirm::remove_worktree(w.path.clone()),
                                                )),
                                            "Remove worktree",
                                            tooltip::Position::Top,
                                            p,
                                        )
                                    } else {
                                        Space::with_width(Length::Shrink).into()
                                    },
                                ]
                                .spacing(4)
                                .align_y(Alignment::Center),
                                theme::clipped_line(
                                    text(format!(
                                        "+{} -{}  {}",
                                        w.ahead,
                                        w.behind,
                                        w.path.display()
                                    ))
                                    .size(9)
                                    .color(p.text_muted),
                                    Length::Fill,
                                ),
                            ]
                            .spacing(2),
                        )
                        .padding(4)
                        .style(theme::card_box(p)),
                    );
                }
                let max_wt_w = self
                    .worktrees
                    .iter()
                    .map(|w| {
                        let path_len =
                            w.path.display().to_string().chars().count() as f32 * 6.0 + 60.0;
                        let title_len = (w.name.chars().count()
                            + w.branch.as_deref().map_or(8, |b| b.chars().count())) as f32
                            * 7.0
                            + 96.0;
                        path_len.max(title_len)
                    })
                    .fold(220.0_f32, f32::max)
                    .clamp(220.0, 1100.0);
                col.push(slim_scroll_h(list, max_wt_w, p))
                    .height(Length::Fill)
                    .into()
            }
        };

        container(
            column![
                header,
                horizontal_rule(1).style(theme::subtle_rule(p)),
                body
            ]
            .spacing(6),
        )
            .padding(8)
            .height(Length::Fill)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_branch(
        name: &str,
        is_head: bool,
        upstream: Option<&str>,
        ahead: usize,
        behind: usize,
        upstream_gone: bool,
        worktree_path: Option<&str>,
    ) -> BranchInfo {
        BranchInfo {
            name: name.into(),
            is_head,
            is_remote: false,
            upstream: upstream.map(str::to_string),
            ahead,
            behind,
            upstream_gone,
            worktree_path: worktree_path.map(str::to_string),
        }
    }

    #[test]
    fn builds_nested_branch_tree_with_counts_head_and_upstream_hint() {
        let branches = vec![
            mk_branch("main", true, Some("origin/main"), 0, 0, false, None),
            mk_branch(
                "feat/agent-voice",
                false,
                Some("origin/feat/agent-voice"),
                3,
                0,
                false,
                None,
            ),
            mk_branch(
                "review/main",
                false,
                Some("origin/review/main"),
                0,
                69,
                false,
                None,
            ),
            mk_branch(
                "feat/support_streamable_http",
                false,
                None,
                0,
                0,
                false,
                Some("/repo/.claude/worktrees/admiring"),
            ),
            mk_branch(
                "test/nested/exp",
                false,
                Some("upstream/other"),
                2,
                5,
                false,
                None,
            ),
            mk_branch(
                "stale",
                false,
                Some("origin/stale"),
                0,
                0,
                true,
                None,
            ),
        ];

        let mut root = BranchTreeNode::default();
        for b in &branches {
            root.insert(&b.name, b);
        }

        assert_eq!(root.count(), 6);
        assert!(root.contains_head());
        assert_eq!(root.leaves.len(), 2);
        assert_eq!(root.folders["feat"].count(), 2);
        assert!(!root.folders["feat"].contains_head());
        assert_eq!(root.folders["test"].folders["nested"].leaves[0].0, "exp");
        assert_eq!(format_upstream_hint(&branches[0]).as_deref(), Some("= origin"));
        assert_eq!(format_upstream_hint(&branches[1]).as_deref(), Some("> origin"));
        assert_eq!(format_upstream_hint(&branches[2]).as_deref(), Some("< origin"));
        assert_eq!(format_upstream_hint(&branches[3]), None);
        assert_eq!(
            format_upstream_hint(&branches[4]).as_deref(),
            Some("<> upstream/other")
        );
        assert_eq!(
            format_upstream_hint(&branches[5]).as_deref(),
            Some("? origin")
        );
        assert_eq!(
            classify_upstream_hint(&branches[5]).map(|(k, _)| k),
            Some(UpstreamHintKind::Gone)
        );
        assert_eq!(
            format_tracking_state(&branches[5]).as_deref(),
            Some("Remote branch deleted (gone)")
        );
        assert_eq!(
            format_tracking_state(&branches[3]).as_deref(),
            Some("Local only (not tracking a remote)")
        );
        assert_eq!(
            format_tracking_state(&branches[2]).as_deref(),
            Some("69 commits behind")
        );
        assert_eq!(
            linked_worktree_for_branch(&branches[3], &[]),
            Some(("/repo/.claude/worktrees/admiring".to_string(), false))
        );
    }

    #[test]
    fn branch_tree_indentation_shifts_children_inside_groups() {
        // Direct child of a group (depth 1) must be indented noticeably past the group checkbox
        assert!(branch_leaf_indent(1) >= GROUP_CHECKBOX_X + 16.0);
        // Nested folder children (depth 2) indent deeper than direct children (depth 1)
        assert!(branch_leaf_indent(2) > branch_leaf_indent(1));
        assert!(branch_folder_indent(2) > branch_folder_indent(1));
        assert!(branch_leaf_indent(1) > branch_folder_indent(1));
    }
}
