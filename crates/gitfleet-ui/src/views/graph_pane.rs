use crate::app::{
    ActiveFocusPane, ContextMenuTarget, GitFleetApp, GraphSelection, GraphTab, Message,
    PendingConfirm,
};
use crate::graph;
use crate::icons;
use crate::theme::kit::{ButtonSize, ButtonVariant, SegmentItem, UiButton, pane_focus_dot, segmented_control};
use crate::theme::{self, Palette, type_scale};
use crate::views::slim_scroll_h;
use gitfleet_git::{BranchInfo, ResetMode};
use iced::widget::{
    Space, button, column, container, horizontal_rule, mouse_area, row, text, text_input, tooltip,
};
use iced::{Alignment, Background, Border, Color, Element, Length};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GraphRefPillKind {
    SyncedLocal {
        remote_label: String,
        branch: String,
        is_head: bool,
    },
    Local {
        branch: String,
        is_head: bool,
        ahead: usize,
        behind: usize,
        upstream_gone: bool,
        upstream_remote: Option<String>,
    },
    Remote(String),
    DetachedHead,
    Tag(String),
}

impl GraphRefPillKind {
    fn est_width(&self) -> f32 {
        match self {
            Self::SyncedLocal {
                remote_label,
                branch,
                is_head,
            } => {
                theme::estimate_text_width(&format!("{remote_label} ="), type_scale::BADGE, false)
                    + theme::estimate_text_width(branch, type_scale::BADGE, false)
                    + if *is_head { 40.0 } else { 28.0 }
            }
            Self::Local {
                branch,
                is_head,
                ahead,
                behind,
                upstream_gone,
                upstream_remote,
            } => {
                let mut extra = if *is_head { 26.0 } else { 14.0 };
                if *upstream_gone {
                    let rem = upstream_remote.as_deref().unwrap_or("origin");
                    extra += theme::estimate_text_width(
                        &format!("? {rem}"),
                        type_scale::BADGE,
                        true,
                    ) + 4.0;
                } else {
                    if *ahead > 0 {
                        extra += theme::estimate_text_width(
                            &format!("+{ahead}"),
                            type_scale::BADGE,
                            true,
                        ) + 4.0;
                    }
                    if *behind > 0 {
                        extra += theme::estimate_text_width(
                            &format!("-{behind}"),
                            type_scale::BADGE,
                            true,
                        ) + 4.0;
                    }
                }
                theme::estimate_text_width(branch, type_scale::BADGE, false) + extra
            }
            Self::Remote(name) => theme::estimate_text_width(name, type_scale::BADGE, false) + 16.0,
            Self::DetachedHead => 56.0,
            Self::Tag(name) => {
                theme::estimate_text_width(&format!("tag: {name}"), type_scale::BADGE, false) + 16.0
            }
        }
    }
}

/// Parses Git `%D` decorations and enriches them with `BranchInfo` ahead/behind/upstream data
/// to produce full, untruncated SmartGit-style graph pills (`[origin = ▶ develop]`,
/// `[-10 ▶ develop]`, `[+3 ▶ develop]`, `[origin/develop]`, tags).
pub(crate) fn build_commit_ref_pills(
    refs: &str,
    branches: &[BranchInfo],
    status_branch: Option<&str>,
    status_upstream: Option<&str>,
    status_ahead: usize,
    status_behind: usize,
) -> Vec<GraphRefPillKind> {
    if refs.trim().is_empty() {
        return Vec::new();
    }

    let mut local_map: HashMap<&str, &BranchInfo> = HashMap::new();
    let mut known_remotes: HashSet<&str> = HashSet::new();
    let mut remote_prefixes: HashSet<&str> = HashSet::new();
    for b in branches {
        if b.is_remote {
            known_remotes.insert(b.name.as_str());
            if let Some((pref, _)) = b.name.split_once('/') {
                remote_prefixes.insert(pref);
            }
        } else {
            local_map.insert(b.name.as_str(), b);
        }
    }
    if remote_prefixes.is_empty() {
        remote_prefixes.insert("origin");
        remote_prefixes.insert("upstream");
    }

    let mut head_branch: Option<String> = None;
    let mut detached_head = false;
    let mut local_branches: Vec<String> = Vec::new();
    let mut remote_branches: Vec<String> = Vec::new();
    let mut tags: Vec<String> = Vec::new();

    for raw_tok in refs.split(',') {
        let tok = raw_tok.trim();
        if tok.is_empty() {
            continue;
        }
        if let Some((lhs, rhs)) = tok.split_once(" -> ") {
            if lhs.trim() == "HEAD" {
                let target = rhs.trim();
                if !target.is_empty() {
                    head_branch = Some(target.to_string());
                }
            }
            // Skip symbolic remote HEAD pointers such as `origin/HEAD -> origin/develop`
            continue;
        }
        if tok == "HEAD" {
            detached_head = true;
            continue;
        }
        if tok.ends_with("/HEAD") {
            continue;
        }
        if let Some(tag) = tok.strip_prefix("tag:") {
            let t = tag.trim();
            if !t.is_empty() {
                tags.push(t.to_string());
            }
            continue;
        }
        let is_remote = known_remotes.contains(tok)
            || (!local_map.contains_key(tok)
                && tok
                    .split_once('/')
                    .is_some_and(|(pref, _)| remote_prefixes.contains(pref)));
        if is_remote {
            remote_branches.push(tok.to_string());
        } else {
            local_branches.push(tok.to_string());
        }
    }

    let mut pills: Vec<GraphRefPillKind> = Vec::new();
    let mut consumed_remotes: HashSet<String> = HashSet::new();

    let mut push_local = |pills: &mut Vec<GraphRefPillKind>, name: String, is_head: bool| {
        let info = local_map.get(name.as_str()).copied();
        let (upstream, ahead, behind, gone) = if let Some(b) = info {
            (b.upstream.as_deref(), b.ahead, b.behind, b.upstream_gone)
        } else if is_head && status_branch == Some(name.as_str()) {
            (status_upstream, status_ahead, status_behind, false)
        } else {
            (None, 0, 0, false)
        };

        let upstream_remote = upstream
            .and_then(|u| u.split_once('/').map(|(r, _)| r.to_string()));

        // Check if this branch's upstream (or matching `origin/<name>`) is on this exact commit
        let synced_remote = upstream
            .filter(|u| remote_branches.iter().any(|r| r == u))
            .map(|u| u.to_string())
            .or_else(|| {
                if upstream.is_none() {
                    let candidate = format!("origin/{name}");
                    remote_branches
                        .iter()
                        .find(|r| *r == &candidate)
                        .cloned()
                } else {
                    None
                }
            });

        if let Some(rem_full) = synced_remote
            && ahead == 0
            && behind == 0
            && !gone
        {
            consumed_remotes.insert(rem_full.clone());
            let remote_label = rem_full
                .strip_suffix(&format!("/{name}"))
                .unwrap_or(&rem_full)
                .to_string();
            pills.push(GraphRefPillKind::SyncedLocal {
                remote_label,
                branch: name,
                is_head,
            });
        } else {
            pills.push(GraphRefPillKind::Local {
                branch: name,
                is_head,
                ahead,
                behind,
                upstream_gone: gone,
                upstream_remote,
            });
        }
    };

    if let Some(hb) = head_branch {
        push_local(&mut pills, hb, true);
    }
    if detached_head {
        pills.push(GraphRefPillKind::DetachedHead);
    }
    for lb in local_branches {
        push_local(&mut pills, lb, false);
    }
    for rb in remote_branches {
        if !consumed_remotes.contains(&rb) {
            pills.push(GraphRefPillKind::Remote(rb));
        }
    }
    for tg in tags {
        pills.push(GraphRefPillKind::Tag(tg));
    }

    pills
}

fn render_graph_ref_pill<'a>(
    pill: &GraphRefPillKind,
    lane_c: Color,
    p: Palette,
) -> Element<'a, Message> {
    match pill {
        GraphRefPillKind::SyncedLocal {
            remote_label,
            branch,
            is_head,
        } => {
            let border_c = if *is_head { p.accent_fg } else { lane_c };
            let mut right_row = row![].spacing(4).align_y(Alignment::Center);
            if *is_head {
                right_row = right_row.push(icons::play_triangle(9.0, p.accent_fg));
            }
            right_row = right_row.push(
                text(branch.clone())
                    .size(type_scale::BADGE)
                    .color(if *is_head { p.text_primary } else { lane_c })
                    .wrapping(text::Wrapping::None),
            );
            let left_part = container(
                text(format!("{remote_label} ="))
                    .size(type_scale::BADGE)
                    .color(p.text_secondary)
                    .wrapping(text::Wrapping::None),
            )
            .padding([1, 6])
            .style(move |_| container::Style {
                background: Some(Background::Color(p.surface_card)),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: iced::border::Radius {
                        top_left: 3.0,
                        top_right: 0.0,
                        bottom_right: 0.0,
                        bottom_left: 3.0,
                    },
                },
                ..Default::default()
            });
            let right_part = container(right_row)
                .padding([1, 6])
                .style(move |_| container::Style {
                    background: Some(Background::Color(p.fill_selected)),
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: iced::border::Radius {
                            top_left: 0.0,
                            top_right: 3.0,
                            bottom_right: 3.0,
                            bottom_left: 0.0,
                        },
                    },
                    ..Default::default()
                });
            container(row![left_part, right_part].align_y(Alignment::Center))
                .style(move |_| container::Style {
                    border: Border {
                        color: Color { a: 0.85, ..border_c },
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                })
                .into()
        }
        GraphRefPillKind::Local {
            branch,
            is_head,
            ahead,
            behind,
            upstream_gone,
            upstream_remote,
        } => {
            let border_c = if *is_head { p.accent_fg } else { lane_c };
            let mut inner = row![].spacing(4).align_y(Alignment::Center);
            if *upstream_gone {
                let rem = upstream_remote.as_deref().unwrap_or("origin");
                inner = inner.push(
                    text(format!("? {rem}"))
                        .size(type_scale::BADGE)
                        .font(theme::CODE_FONT)
                        .color(p.danger_fg)
                        .wrapping(text::Wrapping::None),
                );
            } else {
                if *ahead > 0 {
                    inner = inner.push(
                        text(format!("+{ahead}"))
                            .size(type_scale::BADGE)
                            .font(theme::CODE_FONT)
                            .color(p.success_fg)
                            .wrapping(text::Wrapping::None),
                    );
                }
                if *behind > 0 {
                    inner = inner.push(
                        text(format!("-{behind}"))
                            .size(type_scale::BADGE)
                            .font(theme::CODE_FONT)
                            .color(p.warning_fg)
                            .wrapping(text::Wrapping::None),
                    );
                }
            }
            if *is_head {
                inner = inner.push(icons::play_triangle(9.0, p.accent_fg));
            }
            inner = inner.push(
                text(branch.clone())
                    .size(type_scale::BADGE)
                    .color(if *is_head { p.text_primary } else { lane_c })
                    .wrapping(text::Wrapping::None),
            );
            container(inner)
                .padding([1, 6])
                .style(theme::pill_box(
                    p.fill_selected,
                    if *is_head { p.text_primary } else { lane_c },
                    Color { a: 0.85, ..border_c },
                ))
                .into()
        }
        GraphRefPillKind::Remote(name) => container(
            text(name.clone())
                .size(type_scale::BADGE)
                .color(p.violet_fg)
                .wrapping(text::Wrapping::None),
        )
        .padding([1, 6])
        .style(theme::pill_box(
            p.surface_card,
            p.violet_fg,
            Color {
                a: 0.78,
                ..p.violet_fg
            },
        ))
        .into(),
        GraphRefPillKind::DetachedHead => container(
            row![
                icons::play_triangle(9.0, p.danger_fg),
                text("HEAD")
                    .size(type_scale::BADGE)
                    .color(p.danger_fg)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        )
        .padding([1, 6])
        .style(theme::pill_box(p.danger_bg, p.danger_fg, p.danger_fg))
        .into(),
        GraphRefPillKind::Tag(name) => container(
            text(format!("tag: {name}"))
                .size(type_scale::BADGE)
                .color(p.warning_fg)
                .wrapping(text::Wrapping::None),
        )
        .padding([1, 6])
        .style(theme::pill_box(
            p.warning_bg,
            p.warning_fg,
            Color {
                a: 0.75,
                ..p.warning_fg
            },
        ))
        .into(),
    }
}

fn commit_has_head_decoration(refs: &str) -> bool {
    refs.split(',').any(|raw| {
        let tok = raw.trim();
        tok == "HEAD" || tok.starts_with("HEAD -> ")
    })
}

impl GitFleetApp {
    pub(crate) fn view_graph_pane(&self, p: Palette) -> Element<'_, Message> {
        let scope_label = if !self.graph_checked_branches.is_empty() {
            format!("Selected ({})", self.graph_checked_branches.len())
        } else if self.graph_all_branches {
            "All Branches".to_string()
        } else {
            "Current Branch".to_string()
        };
        let order_label = if self.graph_topo_order { "Topo" } else { "Date" };
        let graph_focused = matches!(
            self.focus_pane,
            ActiveFocusPane::Graph | ActiveFocusPane::FilesCommit
        );

        let header = row![
            pane_focus_dot(p, graph_focused),
            segmented_control(
                p,
                [
                    SegmentItem::new(GraphTab::Graph, "Graph"),
                    SegmentItem::new(GraphTab::Reflog, "Reflog"),
                ],
                self.graph_tab,
                Message::SetGraphTab,
            ),
            UiButton::new(p, scope_label)
                .variant(ButtonVariant::Subtle)
                .size(ButtonSize::Xs)
                .active(!self.graph_checked_branches.is_empty() || self.graph_all_branches)
                .on_press(Message::ToggleGraphAllBranches),
            UiButton::new(p, order_label)
                .variant(ButtonVariant::Subtle)
                .size(ButtonSize::Xs)
                .active(!self.graph_topo_order)
                .on_press(Message::ToggleGraphOrder),
            Space::with_width(Length::Fill),
            container(
                text_input("Filter commits…", &self.commit_filter)
                    .on_input(Message::CommitFilterChanged)
                    .padding([4, 8])
                    .size(type_scale::CAPTION)
                    .width(Length::Fill)
                    .style(theme::dark_input(p)),
            )
            .width(Length::Fill)
            .max_width(190.0),
        ]
        .spacing(5)
        .align_y(Alignment::Center);

        if self.graph_tab == GraphTab::Reflog {
            let mut rf_col = column![].spacing(4);
            for entry in &self.reflog {
                rf_col = rf_col.push(
                    row![
                        text(entry.short_oid.clone())
                            .size(type_scale::CAPTION)
                            .font(theme::CODE_FONT)
                            .color(p.accent_fg),
                        text(entry.selector.clone())
                            .size(type_scale::CAPTION)
                            .font(theme::CODE_FONT)
                            .color(p.text_muted),
                        theme::clipped_line(
                            text(format!("{}: {}", entry.action, entry.message))
                                .size(type_scale::BODY)
                                .color(p.text_primary),
                            Length::Fill,
                        ),
                        text(entry.date_relative.clone())
                            .size(type_scale::CAPTION)
                            .color(p.text_muted)
                            .wrapping(text::Wrapping::None),
                        button(
                            text("Reset")
                                .size(type_scale::CAPTION)
                                .wrapping(text::Wrapping::None),
                        )
                            .padding([3, 7])
                            .style(theme::subtle_btn(p))
                            .on_press(Message::RequestConfirm(PendingConfirm::reset_commit(
                                entry.short_oid.clone(),
                                &entry.short_oid,
                                ResetMode::Mixed,
                            ))),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                );
            }
            if self.reflog.is_empty() {
                rf_col = rf_col.push(
                    text("No reflog entries.")
                        .size(type_scale::CAPTION)
                        .color(p.text_muted),
                );
            }
            let min_reflog_w = 340.0_f32;
            return column![
                header,
                horizontal_rule(1).style(theme::subtle_rule(p)),
                slim_scroll_h(rf_col, min_reflog_w, p)
            ]
                .spacing(6)
                .height(Length::Fill)
                .into();
        }

        let active_st = self.active_status();
        let wt_staged = active_st.map(|s| s.staged_count).unwrap_or(0);
        let wt_unstaged = active_st
            .map(|s| s.unstaged_count + s.untracked_count + s.conflict_count)
            .unwrap_or(0);
        let wt_changes = wt_staged + wt_unstaged;
        let status_branch = active_st.and_then(|s| s.branch.as_deref());
        let status_upstream = active_st.and_then(|s| s.upstream.as_deref());
        let status_ahead = active_st.map(|s| s.ahead).unwrap_or(0);
        let status_behind = active_st.map(|s| s.behind).unwrap_or(0);
        let head_oid = active_st.and_then(|s| s.head_oid.as_deref());

        let q = self.commit_filter.to_lowercase();
        let matches_filter = |c: &gitfleet_git::CommitSummary| -> bool {
            q.is_empty()
                || c.summary.to_lowercase().contains(&q)
                || c.author.to_lowercase().contains(&q)
                || c.short_oid.to_lowercase().contains(&q)
                || c.refs.to_lowercase().contains(&q)
        };

        // Locate the HEAD commit so Working Tree / Index sits right above HEAD (SmartGit style),
        // even when incoming commits on origin/<branch> appear above HEAD in the graph.
        let head_idx = self
            .commits
            .iter()
            .position(|c| head_oid.is_some_and(|h| c.oid == h))
            .or_else(|| {
                self.commits
                    .iter()
                    .position(|c| commit_has_head_decoration(&c.refs))
            })
            .filter(|&idx| self.commits.get(idx).is_some_and(matches_filter));

        let (wt_col, head_col, wt_pass_through, wt_line_color) =
            if let Some(h_idx) = head_idx
                && let Some(h_row) = self.graph_rows.get(h_idx)
            {
                let h_col = h_row.node_col;
                let has_incoming_above = h_row.top_lines.iter().any(|&(col, _)| col == h_col);
                let chosen_wt_col = if has_incoming_above {
                    (0usize..12)
                        .find(|cand| !h_row.top_lines.iter().any(|&(col, _)| col == *cand))
                        .unwrap_or(h_col + 1)
                } else {
                    h_col
                };
                (
                    chosen_wt_col,
                    h_col,
                    h_row.top_lines.clone(),
                    graph::lane_color(h_row.node_color),
                )
            } else {
                (0usize, 0usize, Vec::new(), graph::lane_color(0))
            };
        let canvas_lanes = self.graph_max_lanes.max(wt_col + 1).min(12);

        let wt_selected = self.graph_selection == GraphSelection::WorkingTree;
        let wt_bg = if wt_selected {
            p.fill_selected
        } else {
            p.surface_panel
        };
        let wt_label = match (wt_unstaged, wt_staged) {
            (0, 0) => "Working Tree / Index (clean)".to_string(),
            (u, 0) => format!("Working Tree / Index ({u} changed)"),
            (0, s) => format!("Working Tree / Index ({s} staged)"),
            (u, s) => format!("Working Tree / Index ({u} changed, {s} staged)"),
        };

        let wt_row = button(
            row![
                graph::working_tree_cell(
                    canvas_lanes,
                    wt_col,
                    head_col,
                    wt_pass_through,
                    if wt_changes > 0 { p.warning_fg } else { p.success_fg },
                    wt_line_color,
                    wt_bg,
                    !self.commits.is_empty(),
                ),
                text(wt_label)
                    .size(type_scale::BODY)
                    .color(if wt_changes > 0 {
                        p.warning_fg
                    } else {
                        p.text_primary
                    }),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fixed(graph::ROW_HEIGHT))
        .padding([0, 6])
        .style(theme::repo_row_btn(p, wt_selected))
        .on_press(Message::SelectWorkingTree);

        let wt_area = mouse_area(wt_row)
            .on_right_press(Message::OpenContextMenu(ContextMenuTarget::WorkingTree));
        let mut wt_slot = Some(wt_area);
        let mut rows_col = column![].spacing(0);
        if head_idx.is_none()
            && let Some(wt) = wt_slot.take()
        {
            rows_col = rows_col.push(wt);
        }

        let mut max_ref_w: f32 = 0.0;
        for (idx, c) in self.commits.iter().enumerate() {
            if !matches_filter(c) {
                continue;
            }
            if head_idx == Some(idx)
                && let Some(wt) = wt_slot.take()
            {
                rows_col = rows_col.push(wt);
            }
            let selected = match &self.graph_selection {
                GraphSelection::Commit(oid) => oid == &c.oid,
                GraphSelection::Range { base, head } => base == &c.oid || head == &c.oid,
                _ => false,
            };
            let row_bg = if selected {
                p.fill_selected
            } else {
                p.surface_panel
            };

            let graph_widget: Element<'_, Message> = if let Some(grow) = self.graph_rows.get(idx) {
                graph::graph_cell(grow.clone(), canvas_lanes, row_bg)
            } else {
                Space::with_width(Length::Fixed(graph::graph_canvas_width(canvas_lanes))).into()
            };

            let mut commit_line = row![
                graph_widget,
                text(c.short_oid.clone())
                    .size(type_scale::CAPTION)
                    .font(theme::CODE_FONT)
                    .color(p.text_muted),
            ]
            .spacing(6)
            .align_y(Alignment::Center);

            if !c.refs.is_empty() {
                let ref_color = self
                    .graph_rows
                    .get(idx)
                    .map(|r| graph::lane_color(r.node_color))
                    .unwrap_or(p.accent_fg);
                let pills = build_commit_ref_pills(
                    &c.refs,
                    &self.branches,
                    status_branch,
                    status_upstream,
                    status_ahead,
                    status_behind,
                );
                if !pills.is_empty() {
                    let row_ref_w: f32 = pills.iter().map(|pl| pl.est_width() + 4.0).sum();
                    if row_ref_w > max_ref_w {
                        max_ref_w = row_ref_w;
                    }
                    let mut pills_row = row![].spacing(4).align_y(Alignment::Center);
                    for pill in &pills {
                        pills_row = pills_row.push(render_graph_ref_pill(pill, ref_color, p));
                    }
                    commit_line = commit_line.push(pills_row);
                }
            }

            let (initials, avatar_color) = theme::author_avatar_info(&c.author);
            let avatar_badge = container(
                text(initials.clone())
                    .size(type_scale::BADGE)
                    .color(Color::WHITE)
                    .wrapping(text::Wrapping::None),
            )
            .width(Length::Fixed(26.0))
            .height(Length::Fixed(20.0))
            .center_x(Length::Fixed(26.0))
            .center_y(Length::Fixed(20.0))
            .style(theme::avatar_box(avatar_color));

            let avatar_tip_card = container(
                row![
                    container(text(initials).size(type_scale::CAPTION).color(Color::WHITE))
                        .width(Length::Fixed(30.0))
                        .height(Length::Fixed(30.0))
                        .center_x(Length::Fixed(30.0))
                        .center_y(Length::Fixed(30.0))
                        .style(theme::avatar_box(avatar_color)),
                    column![
                        text(c.author.clone())
                            .size(type_scale::BODY)
                            .color(p.text_primary),
                        text(if c.author_email.is_empty() {
                            "(no email)".to_string()
                        } else {
                            c.author_email.clone()
                        })
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(p.accent_fg),
                        text(format!("{} ({}) · {}", c.date_display, c.date_relative, c.short_oid))
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                    ]
                    .spacing(2),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding([7, 10])
            .style(theme::tooltip_box(p));

            let avatar_with_tooltip = tooltip(avatar_badge, avatar_tip_card, tooltip::Position::Left)
                .gap(6)
                .snap_within_viewport(true);

            commit_line = commit_line
                .push(theme::clipped_line(
                    text(c.summary.clone())
                        .size(type_scale::BODY)
                        .color(p.text_primary),
                    Length::Fill,
                ))
                .push(
                    row![
                        avatar_with_tooltip,
                        container(theme::clipped_line(
                            text(c.date_display.clone())
                                .size(type_scale::CAPTION)
                                .color(p.text_muted),
                            Length::Shrink,
                        ))
                        .width(Length::Fixed(122.0))
                        .align_x(iced::alignment::Horizontal::Left),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                );

            let commit_btn = button(commit_line)
                .width(Length::Fill)
                .height(Length::Fixed(graph::ROW_HEIGHT))
                .padding([0, 6])
                .style(theme::repo_row_btn(p, selected))
                .on_press(Message::SelectCommit(c.oid.clone()));

            rows_col = rows_col.push(mouse_area(commit_btn).on_right_press(
                Message::OpenContextMenu(ContextMenuTarget::Commit {
                    oid: c.oid.clone(),
                    short_oid: c.short_oid.clone(),
                    summary: c.summary.clone(),
                }),
            ));
        }

        let lanes_w = graph::graph_canvas_width(canvas_lanes);
        // Reserve a compact minimum for commit summary (~80px) since clicking any commit
        // opens the full summary & body in the Commit Details pane.
        let min_graph_w = (lanes_w + 56.0 + max_ref_w + 80.0 + 162.0).clamp(320.0, 1400.0);

        let range_banner: Element<'_, Message> =
            if let GraphSelection::Range { base, head } = &self.graph_selection {
                let b_disp = &base[..base.len().min(10)];
                let h_disp = &head[..head.len().min(10)];
                container(
                    row![
                        icons::file_diff(12.0, p.accent_fg),
                        text(format!("Comparing {b_disp} .. {h_disp}"))
                            .size(type_scale::CAPTION)
                            .font(theme::CODE_FONT)
                            .color(p.accent_fg),
                        Space::with_width(Length::Fill),
                        button(text("⇄").size(type_scale::BADGE).color(p.text_secondary))
                            .padding([1, 6])
                            .style(theme::subtle_btn(p))
                            .on_press(Message::CompareRefs(head.clone(), base.clone())),
                        button(icons::close_x(10.0, p.text_secondary))
                            .padding([2, 5])
                            .style(theme::vscode_icon_btn(p, false))
                            .on_press(Message::SelectWorkingTree),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding([3, 8])
                .width(Length::Fill)
                .style(theme::pill_box(p.fill_selected, p.accent_fg, p.border_subtle))
                .into()
            } else {
                Space::with_height(Length::Shrink).into()
            };

        column![
            header,
            range_banner,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            slim_scroll_h(rows_col, min_graph_w, p)
        ]
            .spacing(6)
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
        is_remote: bool,
        upstream: Option<&str>,
        ahead: usize,
        behind: usize,
        upstream_gone: bool,
    ) -> BranchInfo {
        BranchInfo {
            name: name.to_string(),
            is_head,
            is_remote,
            upstream: upstream.map(|s| s.to_string()),
            ahead,
            behind,
            upstream_gone,
            worktree_path: None,
        }
    }

    #[test]
    fn merges_synced_head_and_origin_into_smartgit_capsule_and_drops_origin_head() {
        let branches = vec![
            mk_branch("develop", true, false, Some("origin/develop"), 0, 0, false),
            mk_branch("origin/develop", false, true, None, 0, 0, false),
        ];
        let pills = build_commit_ref_pills(
            "HEAD -> develop, origin/develop, origin/HEAD",
            &branches,
            Some("develop"),
            Some("origin/develop"),
            0,
            0,
        );
        assert_eq!(
            pills,
            vec![GraphRefPillKind::SyncedLocal {
                remote_label: "origin".into(),
                branch: "develop".into(),
                is_head: true,
            }]
        );
    }

    #[test]
    fn displays_full_origin_remote_and_behind_delta_on_head_commit() {
        let branches = vec![
            mk_branch("develop", true, false, Some("origin/develop"), 0, 10, false),
            mk_branch("origin/develop", false, true, None, 0, 0, false),
        ];
        let origin_pills = build_commit_ref_pills(
            "origin/develop, origin/HEAD -> origin/develop",
            &branches,
            Some("develop"),
            Some("origin/develop"),
            0,
            10,
        );
        assert_eq!(
            origin_pills,
            vec![GraphRefPillKind::Remote("origin/develop".into())]
        );

        let head_pills = build_commit_ref_pills(
            "HEAD -> develop",
            &branches,
            Some("develop"),
            Some("origin/develop"),
            0,
            10,
        );
        assert_eq!(
            head_pills,
            vec![GraphRefPillKind::Local {
                branch: "develop".into(),
                is_head: true,
                ahead: 0,
                behind: 10,
                upstream_gone: false,
                upstream_remote: Some("origin".into()),
            }]
        );
    }

    #[test]
    fn preserves_long_untruncated_remote_names_and_tags() {
        let branches = vec![
            mk_branch(
                "feature/refactor-rabbitmq-service",
                true,
                false,
                Some("origin/feature/refactor-rabbitmq-service"),
                2,
                5,
                false,
            ),
            mk_branch(
                "origin/feature/refactor-rabbitmq-service",
                false,
                true,
                None,
                0,
                0,
                false,
            ),
            mk_branch("origin/develop", false, true, None, 0, 0, false),
        ];
        let pills = build_commit_ref_pills(
            "HEAD -> feature/refactor-rabbitmq-service, origin/develop, tag: v2.4.0",
            &branches,
            None,
            None,
            0,
            0,
        );
        assert_eq!(
            pills,
            vec![
                GraphRefPillKind::Local {
                    branch: "feature/refactor-rabbitmq-service".into(),
                    is_head: true,
                    ahead: 2,
                    behind: 5,
                    upstream_gone: false,
                    upstream_remote: Some("origin".into()),
                },
                GraphRefPillKind::Remote("origin/develop".into()),
                GraphRefPillKind::Tag("v2.4.0".into()),
            ]
        );
    }
}
