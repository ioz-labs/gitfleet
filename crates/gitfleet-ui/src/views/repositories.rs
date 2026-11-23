use crate::app::{ContextMenuTarget, GitFleetApp, Message, PendingConfirm, RepoFilter};
use crate::icons;
use crate::theme::{self, Palette, ThemeMode, tree, type_scale};
use crate::views::{slim_scroll_h, tip, truncate_chars};
use gitfleet_git::bulk::BulkOperation;
use gitfleet_workspace::RepoEntry;
use iced::widget::canvas::{self, Canvas, Geometry, LineCap, LineJoin, Path as CanvasPath, Stroke};
use iced::widget::{
    Space, button, checkbox, column, container, horizontal_rule, mouse_area, row, text, text_input,
    tooltip,
};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Padding, Point, Rectangle, Renderer,
    Size, Theme, mouse,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RepoDiskTone {
    Missing,
    Conflicted,
    Dirty,
    CleanActive,
    CleanIdle,
}

fn sync_push_color(dark_mode: bool) -> Color {
    if dark_mode {
        Color::from_rgb(1.0, 0.480, 0.150) // crisp high-contrast push orange (#ff7a26)
    } else {
        Color::from_rgb(0.880, 0.345, 0.045)
    }
}

fn sync_pull_color(dark_mode: bool) -> Color {
    if dark_mode {
        Color::from_rgb(0.180, 0.855, 0.455) // crisp high-contrast pull emerald (#2eda74)
    } else {
        Color::from_rgb(0.082, 0.545, 0.265)
    }
}

fn dirty_dot_color(dark_mode: bool) -> Color {
    if dark_mode {
        Color::from_rgb(0.980, 0.800, 0.200) // bright amber-yellow (#facc33)
    } else {
        Color::from_rgb(0.800, 0.540, 0.020)
    }
}

/// Renders an ahead (`↑N`) or behind (`↓N`) sync counter using a crisp vector SVG arrow
/// paired with pure ASCII `CODE_FONT` digits inside a fixed `18px` vertically-centered box so
/// it aligns on the exact same horizontal line as the adjacent uncommitted-changes pill.
fn sync_count_badge<'a>(is_push: bool, count: usize, color: Color) -> Element<'a, Message> {
    let arrow = if is_push {
        icons::push(10.0, color)
    } else {
        icons::pull(10.0, color)
    };
    container(
        row![
            arrow,
            text(format!("{count}"))
                .size(type_scale::BADGE)
                .font(theme::CODE_FONT)
                .line_height(text::LineHeight::Relative(1.0))
                .color(color)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(2)
        .align_y(Alignment::Center),
    )
    .height(Length::Fixed(18.0))
    .align_y(iced::alignment::Vertical::Center)
    .into()
}

/// Renders the uncommitted/conflicted changes counter pill using the exact same `18px` height,
/// `CODE_FONT` metrics, and vertical centering as `sync_count_badge`.
fn changed_count_badge<'a>(count: usize, fg: Color, bg: Color) -> Element<'a, Message> {
    container(
        text(format!("{count}"))
            .size(type_scale::BADGE)
            .font(theme::CODE_FONT)
            .line_height(text::LineHeight::Relative(1.0))
            .color(fg)
            .wrapping(text::Wrapping::None),
    )
    .height(Length::Fixed(18.0))
    .padding([0, 5])
    .align_y(iced::alignment::Vertical::Center)
    .style(theme::pill_box(bg, fg, Color { a: 0.42, ..fg }))
    .into()
}

/// SmartGit-style 3-layer cool-slate repository disk stack with high-contrast overlaid
/// Push (`^` orange) / Pull (`v` green) chevrons and a distinct bottom-right amber dirty badge.
struct RepoSyncIcon {
    tone: RepoDiskTone,
    ahead: usize,
    behind: usize,
    dark_mode: bool,
    p: Palette,
}

impl canvas::Program<Message> for RepoSyncIcon {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;
        let sx = w / 16.0;
        let sy = h / 16.0;

        // Keep disk bars cool neutral slate/silver so orange Push `^`, green Pull `v`, and yellow
        // dirty dot all pop with maximum contrast and never blend into each other.
        let (bar_stroke, bar_fill) = match self.tone {
            RepoDiskTone::Missing => (
                Color {
                    a: 0.40,
                    ..self.p.text_muted
                },
                Color {
                    a: 0.08,
                    ..self.p.text_muted
                },
            ),
            RepoDiskTone::Conflicted => (
                self.p.danger_fg,
                Color {
                    a: 0.22,
                    ..self.p.danger_fg
                },
            ),
            RepoDiskTone::CleanActive | RepoDiskTone::Dirty => {
                if self.dark_mode {
                    (
                        Color::from_rgba(0.640, 0.710, 0.800, 0.88),
                        Color::from_rgba(0.550, 0.630, 0.730, 0.16),
                    )
                } else {
                    (
                        Color::from_rgba(0.300, 0.365, 0.455, 0.85),
                        Color::from_rgba(0.300, 0.365, 0.455, 0.14),
                    )
                }
            }
            RepoDiskTone::CleanIdle => {
                if self.dark_mode {
                    (
                        Color::from_rgba(0.460, 0.525, 0.610, 0.72),
                        Color::from_rgba(0.460, 0.525, 0.610, 0.12),
                    )
                } else {
                    (
                        Color::from_rgba(0.420, 0.480, 0.560, 0.68),
                        Color::from_rgba(0.420, 0.480, 0.560, 0.10),
                    )
                }
            }
        };

        // 3 stacked horizontal disk bars
        for y0 in [1.8_f32, 6.4, 11.0] {
            let rect = CanvasPath::rounded_rectangle(
                Point::new(1.4 * sx, y0 * sy),
                Size::new(13.2 * sx, 3.0 * sy),
                (1.1 * sx).into(),
            );
            frame.fill(&rect, bar_fill);
            frame.stroke(
                &rect,
                Stroke::default()
                    .with_color(bar_stroke)
                    .with_width((1.0 * sx).max(0.9)),
            );
        }

        let halo = if self.dark_mode {
            Color::from_rgba(0.050, 0.065, 0.090, 0.94)
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.95)
        };
        let push_c = sync_push_color(self.dark_mode);
        let pull_c = sync_pull_color(self.dark_mode);

        let draw_chevron = |frame: &mut canvas::Frame,
                            p0: (f32, f32),
                            p1: (f32, f32),
                            p2: (f32, f32),
                            color: Color,
                            width: f32,
                            halo_w: f32| {
            let chevron = CanvasPath::new(|b| {
                b.move_to(Point::new(p0.0 * sx, p0.1 * sy));
                b.line_to(Point::new(p1.0 * sx, p1.1 * sy));
                b.line_to(Point::new(p2.0 * sx, p2.1 * sy));
            });
            frame.stroke(
                &chevron,
                Stroke {
                    width: halo_w * sx,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Stroke::default()
                }
                .with_color(halo),
            );
            frame.stroke(
                &chevron,
                Stroke {
                    width: width * sx,
                    line_cap: LineCap::Round,
                    line_join: LineJoin::Round,
                    ..Stroke::default()
                }
                .with_color(color),
            );
        };

        let is_dirty = self.tone == RepoDiskTone::Dirty;
        let is_conflicted = self.tone == RepoDiskTone::Conflicted;

        match (self.ahead > 0, self.behind > 0) {
            (true, false) => {
                // Needs Push: crisp coral-orange upward chevron
                draw_chevron(
                    &mut frame,
                    (3.4, 9.2),
                    (8.0, 3.7),
                    (12.6, 9.2),
                    push_c,
                    2.4,
                    4.4,
                );
            }
            (false, true) => {
                // Needs Pull: crisp emerald downward chevron
                draw_chevron(
                    &mut frame,
                    (3.4, 4.3),
                    (8.0, 9.8),
                    (12.6, 4.3),
                    pull_c,
                    2.4,
                    4.4,
                );
            }
            (true, true) => {
                // Diverged: top orange Push + bottom emerald Pull (shifted slightly left if dirty dot)
                let cx = if is_dirty || is_conflicted { 6.8 } else { 8.0 };
                draw_chevron(
                    &mut frame,
                    (cx - 3.6, 6.5),
                    (cx, 2.4),
                    (cx + 3.6, 6.5),
                    push_c,
                    2.0,
                    3.8,
                );
                draw_chevron(
                    &mut frame,
                    (cx - 3.6, 9.5),
                    (cx, 13.6),
                    (cx + 3.6, 9.5),
                    pull_c,
                    2.0,
                    3.8,
                );
            }
            (false, false) => {}
        }

        // Distinct bottom-right status badge dot for uncommitted local changes / conflicts
        if is_dirty || is_conflicted {
            let dot_c = if is_conflicted {
                self.p.danger_fg
            } else {
                dirty_dot_color(self.dark_mode)
            };
            let center = Point::new(13.0 * sx, 13.0 * sy);
            frame.fill(&CanvasPath::circle(center, 3.4 * sx), halo);
            frame.fill(&CanvasPath::circle(center, 2.3 * sx), dot_c);
        }

        vec![frame.into_geometry()]
    }
}

fn repo_sync_icon<'a>(
    tone: RepoDiskTone,
    ahead: usize,
    behind: usize,
    dark_mode: bool,
    p: Palette,
) -> Element<'a, Message> {
    Canvas::new(RepoSyncIcon {
        tone,
        ahead,
        behind,
        dark_mode,
        p,
    })
    .width(Length::Fixed(16.0))
    .height(Length::Fixed(16.0))
    .into()
}

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

        let dark_mode = self.theme_mode == ThemeMode::Dark;
        let has_overlay = self.has_modal_or_menu_open();
        let ahead = st.map(|s| s.ahead).unwrap_or(0);
        let behind = st.map(|s| s.behind).unwrap_or(0);
        let conflicts = st.map(|s| s.conflict_count).unwrap_or(0);
        let changed = st.map(|s| s.total_changes()).unwrap_or(0);
        let is_dirty = st.is_some_and(|s| !s.is_clean);

        let disk_tone = if !repo.available {
            RepoDiskTone::Missing
        } else if conflicts > 0 {
            RepoDiskTone::Conflicted
        } else if is_dirty {
            RepoDiskTone::Dirty
        } else if selected || ahead > 0 || behind > 0 {
            RepoDiskTone::CleanActive
        } else {
            RepoDiskTone::CleanIdle
        };

        let branch = st.and_then(|s| s.branch.as_deref()).unwrap_or("—");
        let upstream = st.and_then(|s| s.upstream.as_deref());
        let id_check = repo.id.clone();

        let left_x = tree::chevron_x(depth);
        let branch_str = if repo.available {
            format!("({branch})")
        } else {
            "[missing]".to_string()
        };
        let sync_badge_w = if behind > 0 {
            10.0 + 2.0 + theme::estimate_text_width(&format!("{behind}"), type_scale::BADGE, true) + 4.0
        } else {
            0.0
        } + if ahead > 0 {
            10.0 + 2.0 + theme::estimate_text_width(&format!("{ahead}"), type_scale::BADGE, true) + 4.0
        } else {
            0.0
        };
        let changed_badge_w = if changed > 0 {
            theme::estimate_text_width(&format!("{changed}"), type_scale::BADGE, true) + 14.0
        } else {
            0.0
        };
        let badges_gap = if behind > 0 || ahead > 0 || changed > 0 {
            5.0
        } else {
            0.0
        };
        let repo_est_w = tree::content_x(depth)
            + 16.0 // repo_sync_icon
            + 6.0  // gap after icon
            + theme::estimate_text_width(&repo.name, type_scale::BODY, false)
            + 5.0  // gap before branch
            + theme::estimate_text_width(&branch_str, type_scale::CAPTION, false)
            + badges_gap
            + sync_badge_w
            + changed_badge_w
            + tree::ROW_PAD_H
            + if !repo.available { 52.0 } else { 0.0 }
            + 6.0;
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

        let push_c = sync_push_color(dark_mode);
        let pull_c = sync_pull_color(dark_mode);

        let icon_el = repo_sync_icon(disk_tone, ahead, behind, dark_mode, p);
        let icon_with_tip: Element<'a, Message> = if has_overlay {
            icon_el
        } else {
            let mut tip_col = column![
                text(match upstream {
                    Some(up) => format!("{} ({branch} → {up})", repo.name),
                    None => format!("{} ({branch})", repo.name),
                })
                .size(type_scale::CAPTION)
                .color(p.text_primary),
            ]
            .spacing(2);
            if !repo.available {
                tip_col = tip_col.push(
                    text("Repository directory missing on disk")
                        .size(type_scale::CAPTION)
                        .color(p.danger_fg),
                );
            } else {
                if behind > 0 {
                    tip_col = tip_col.push(
                        text(format!(
                            "↓ {behind} commit{} to pull",
                            if behind == 1 { "" } else { "s" }
                        ))
                        .size(type_scale::CAPTION)
                        .color(pull_c),
                    );
                }
                if ahead > 0 {
                    tip_col = tip_col.push(
                        text(format!(
                            "↑ {ahead} commit{} to push",
                            if ahead == 1 { "" } else { "s" }
                        ))
                        .size(type_scale::CAPTION)
                        .color(push_c),
                    );
                }
                if conflicts > 0 {
                    tip_col = tip_col.push(
                        text(format!(
                            "{conflicts} conflicted file{}",
                            if conflicts == 1 { "" } else { "s" }
                        ))
                        .size(type_scale::CAPTION)
                        .color(p.danger_fg),
                    );
                } else if changed > 0 {
                    tip_col = tip_col.push(
                        text(format!(
                            "{changed} uncommitted change{}",
                            if changed == 1 { "" } else { "s" }
                        ))
                        .size(type_scale::CAPTION)
                        .color(p.warning_fg),
                    );
                } else if ahead == 0 && behind == 0 {
                    tip_col = tip_col.push(
                        text("Clean & up to date")
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                    );
                }
            }
            tooltip(
                icon_el,
                container(tip_col)
                    .padding([5, 9])
                    .style(theme::tooltip_box(p)),
                tooltip::Position::Right,
            )
            .gap(6)
            .into()
        };

        let mut right_badges = row![].spacing(5).align_y(Alignment::Center);
        if behind > 0 {
            right_badges = right_badges.push(sync_count_badge(false, behind, pull_c));
        }
        if ahead > 0 {
            right_badges = right_badges.push(sync_count_badge(true, ahead, push_c));
        }
        if changed > 0 {
            let fg = if conflicts > 0 {
                p.danger_fg
            } else {
                p.warning_fg
            };
            let bg = if conflicts > 0 {
                p.danger_bg
            } else {
                p.warning_bg
            };
            right_badges = right_badges.push(changed_count_badge(changed, fg, bg));
        }

        let mut label_and_badges = row![
            text(repo.name.clone())
                .size(type_scale::BODY)
                .color(if is_being_dragged {
                    p.text_muted
                } else {
                    p.text_primary
                }),
            text(branch_str)
                .size(type_scale::CAPTION)
                .color(if repo.available {
                    p.text_muted
                } else {
                    p.danger_fg
                }),
        ]
        .spacing(5)
        .align_y(Alignment::Center);
        if behind > 0 || ahead > 0 || changed > 0 {
            label_and_badges = label_and_badges.push(right_badges);
        }

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
                    icon_with_tip,
                    theme::clipped_line(label_and_badges, Length::Fill),
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
                let sub_conflicts = sub_st.map(|s| s.conflict_count).unwrap_or(0);
                let sub_ahead = sub_st.map(|s| s.ahead).unwrap_or(0);
                let sub_behind = sub_st.map(|s| s.behind).unwrap_or(0);
                let sub_ref = sub_st
                    .and_then(|s| s.branch.as_deref())
                    .or(sub.branch.as_deref())
                    .or(sub.head_oid.as_deref())
                    .unwrap_or(if sub.initialized { "detached" } else { "uninit" });
                let sub_ref_str = format!("({sub_ref})");
                let sub_sync_w = if sub_behind > 0 {
                    10.0 + 2.0
                        + theme::estimate_text_width(&format!("{sub_behind}"), type_scale::BADGE, true)
                        + 4.0
                } else {
                    0.0
                } + if sub_ahead > 0 {
                    10.0 + 2.0
                        + theme::estimate_text_width(&format!("{sub_ahead}"), type_scale::BADGE, true)
                        + 4.0
                } else {
                    0.0
                };
                let sub_changed_w = if sub_changed > 0 {
                    theme::estimate_text_width(&format!("{sub_changed}"), type_scale::BADGE, true)
                        + 14.0
                } else if sub_dirty {
                    12.0
                } else {
                    0.0
                };
                let sub_badges_gap = if sub_behind > 0 || sub_ahead > 0 || sub_changed > 0 || sub_dirty
                {
                    5.0
                } else {
                    0.0
                };
                let sub_est_w = tree::content_x(sub_depth)
                    + theme::estimate_text_width(&sub.path, type_scale::BODY, false)
                    + 5.0
                    + theme::estimate_text_width(&sub_ref_str, type_scale::CAPTION, false)
                    + sub_badges_gap
                    + sub_sync_w
                    + sub_changed_w
                    + tree::ROW_PAD_H
                    + 6.0;
                if sub_est_w > *max_row_w {
                    *max_row_w = sub_est_w;
                }
                let sub_tone = if !sub.initialized {
                    RepoDiskTone::Missing
                } else if sub_conflicts > 0 {
                    RepoDiskTone::Conflicted
                } else if sub_dirty {
                    RepoDiskTone::Dirty
                } else if sub_selected || sub_ahead > 0 || sub_behind > 0 {
                    RepoDiskTone::CleanActive
                } else {
                    RepoDiskTone::CleanIdle
                };

                let mut sub_badges = row![].spacing(5).align_y(Alignment::Center);
                if sub_behind > 0 {
                    sub_badges = sub_badges.push(sync_count_badge(false, sub_behind, pull_c));
                }
                if sub_ahead > 0 {
                    sub_badges = sub_badges.push(sync_count_badge(true, sub_ahead, push_c));
                }
                if sub_changed > 0 {
                    let fg = if sub_conflicts > 0 {
                        p.danger_fg
                    } else {
                        p.warning_fg
                    };
                    let bg = if sub_conflicts > 0 {
                        p.danger_bg
                    } else {
                        p.warning_bg
                    };
                    sub_badges = sub_badges.push(changed_count_badge(sub_changed, fg, bg));
                } else if sub_dirty {
                    sub_badges = sub_badges.push(
                        container(Space::new(7, 7)).style(theme::dot_box(p.warning_fg)),
                    );
                }

                let mut sub_label_row = row![
                    text(sub.path.clone())
                        .size(type_scale::BODY)
                        .color(if sub.initialized {
                            p.text_primary
                        } else {
                            p.text_muted
                        }),
                    text(sub_ref_str)
                        .size(type_scale::CAPTION)
                        .color(if !sub.initialized {
                            p.danger_fg
                        } else {
                            p.text_muted
                        }),
                ]
                .spacing(5)
                .align_y(Alignment::Center);
                if sub_behind > 0 || sub_ahead > 0 || sub_changed > 0 || sub_dirty {
                    sub_label_row = sub_label_row.push(sub_badges);
                }

                let sub_btn = button(
                    row![
                        container(repo_sync_icon(
                            sub_tone, sub_ahead, sub_behind, dark_mode, p
                        ))
                        .width(Length::Fixed(tree::LEAD_W))
                        .center_x(Length::Fixed(tree::LEAD_W)),
                        Space::with_width(Length::Fixed(tree::LABEL_GAP)),
                        theme::clipped_line(sub_label_row, Length::Fill),
                    ]
                    .spacing(0)
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding([tree::ROW_PAD_V, tree::ROW_PAD_H])
                .style(theme::repo_row_btn(p, sub_selected))
                .on_press(Message::SelectSubmodule(repo.id.clone(), sub.path.clone()));

                let sub_body: Element<'a, Message> = if has_overlay {
                    sub_btn.into()
                } else {
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
                    if sub_behind > 0 {
                        sub_tip = sub_tip.push(
                            text(format!(
                                "↓ {sub_behind} commit{} to pull",
                                if sub_behind == 1 { "" } else { "s" }
                            ))
                            .size(type_scale::CAPTION)
                            .color(pull_c),
                        );
                    }
                    if sub_ahead > 0 {
                        sub_tip = sub_tip.push(
                            text(format!(
                                "↑ {sub_ahead} commit{} to push",
                                if sub_ahead == 1 { "" } else { "s" }
                            ))
                            .size(type_scale::CAPTION)
                            .color(push_c),
                        );
                    }
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
                    tooltip(
                        sub_btn,
                        container(sub_tip)
                            .padding([6, 9])
                            .style(theme::tooltip_box(p)),
                        tooltip::Position::Right,
                    )
                    .gap(6)
                    .into()
                };

                out.push(
                    row![Space::with_width(Length::Fixed(sub_btn_left)), sub_body]
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
                + theme::estimate_text_width(seg_name, type_scale::BODY, false)
                + 5.0
                + theme::estimate_text_width(&count_str, type_scale::CAPTION, false)
                + 12.0;
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
        let show_btn_labels = self.sidebar_width >= 260.0;
        let cloning_active = self.active_clone.is_some();
        let clone_btn_label = if let Some(active) = &self.active_clone {
            if active.snapshot.is_cancelled {
                "Cancelling".to_string()
            } else if let Some(pct) = active.snapshot.stage_percent {
                format!("{pct}%")
            } else {
                "Cloning".to_string()
            }
        } else {
            "Clone".to_string()
        };
        let clone_fg = if cloning_active || self.show_clone_modal {
            p.accent_fg
        } else {
            p.text_secondary
        };
        let add_fg = if self.add_repo_modal.is_some() {
            p.accent_fg
        } else {
            p.text_secondary
        };

        let clone_btn_inner: Element<'_, Message> = if show_btn_labels {
            row![
                icons::cloud_download(13.0, clone_fg),
                text(clone_btn_label)
                    .size(type_scale::CAPTION)
                    .color(clone_fg)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(4)
            .align_y(Alignment::Center)
            .into()
        } else {
            icons::cloud_download(15.0, clone_fg).into()
        };

        let add_btn_inner: Element<'_, Message> = if show_btn_labels {
            row![
                icons::folder_plus(13.0, add_fg),
                text("Add")
                    .size(type_scale::CAPTION)
                    .color(add_fg)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(4)
            .align_y(Alignment::Center)
            .into()
        } else {
            icons::folder_plus(15.0, add_fg).into()
        };

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
                button(clone_btn_inner)
                    .padding(if show_btn_labels { [3, 7] } else { [4, 6] })
                    .style({
                        let is_active = cloning_active || self.show_clone_modal;
                        move |t, s| {
                            if is_active {
                                theme::filter_pill_btn(p, true)(t, s)
                            } else if show_btn_labels {
                                theme::subtle_btn(p)(t, s)
                            } else {
                                theme::vscode_icon_btn(p, false)(t, s)
                            }
                        }
                    })
                    .on_press(Message::OpenCloneModal(None)),
                if cloning_active {
                    "View live Git clone progress or cancel"
                } else {
                    "Clone a remote Git repository…"
                },
                tooltip::Position::Bottom,
                p,
            ),
            tip(
                button(add_btn_inner)
                    .padding(if show_btn_labels { [3, 7] } else { [4, 6] })
                    .style({
                        let is_active = self.add_repo_modal.is_some();
                        move |t, s| {
                            if is_active {
                                theme::filter_pill_btn(p, true)(t, s)
                            } else if show_btn_labels {
                                theme::subtle_btn(p)(t, s)
                            } else {
                                theme::vscode_icon_btn(p, false)(t, s)
                            }
                        }
                    })
                    .on_press(Message::OpenAddRepoModal(None)),
                "Add an existing local Git repository or initialize a new one…",
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
        .spacing(4)
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
        if let Some(active) = &self.active_clone {
            let snap = &active.snapshot;
            let is_cancelling = snap.is_cancelled;
            let frac = if is_cancelling {
                (snap.overall_percent as f32 / 100.0).clamp(0.15, 1.0)
            } else if let Some(pct) = snap.stage_percent {
                (pct as f32 / 100.0).clamp(0.04, 1.0)
            } else {
                (snap.overall_percent as f32 / 100.0).clamp(0.06, 1.0)
            };
            let fill_p = ((frac * 1000.0).round() as u16).max(1);
            let rem_p = 1000u16.saturating_sub(fill_p);
            let bar_col = if is_cancelling {
                p.warning_fg
            } else {
                p.accent_primary
            };
            let mut mini_bar = row![
                container(Space::new(Length::Fill, Length::Fixed(4.0)))
                    .width(Length::FillPortion(fill_p))
                    .style(move |_| container::Style {
                        background: Some(Background::Color(bar_col)),
                        border: Border {
                            radius: 999.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
            ];
            if rem_p > 0 {
                mini_bar = mini_bar.push(Space::with_width(Length::FillPortion(rem_p)));
            }
            let status_sub = if is_cancelling {
                "Cancelling…".to_string()
            } else if let Some(pct) = snap.stage_percent {
                format!("{} {pct}%", snap.phase.short_label())
            } else {
                snap.phase.short_label().to_string()
            };

            rows.push(
                container(
                    column![
                        row![
                            icons::cloud_download(
                                13.0,
                                if is_cancelling {
                                    p.warning_fg
                                } else {
                                    p.accent_fg
                                }
                            ),
                            theme::clipped_line(
                                text(format!(
                                    "Cloning '{}' · {status_sub}",
                                    truncate_chars(&active.folder_name, 18)
                                ))
                                .size(type_scale::CAPTION)
                                .color(p.text_primary),
                                Length::Fill,
                            ),
                            button(text("Details").size(type_scale::BADGE))
                                .padding([2, 6])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::OpenCloneModal(None)),
                            button(
                                text(if is_cancelling { "…" } else { "Cancel" })
                                    .size(type_scale::BADGE)
                                    .color(if is_cancelling {
                                        p.text_muted
                                    } else {
                                        p.danger_fg
                                    }),
                            )
                            .padding([2, 6])
                            .style(theme::subtle_btn(p))
                            .on_press_maybe((!is_cancelling).then_some(Message::CancelCloneRepo)),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                        container(mini_bar)
                            .width(Length::Fill)
                            .height(Length::Fixed(4.0))
                            .style(move |_| container::Style {
                                background: Some(Background::Color(p.border_subtle)),
                                border: Border {
                                    radius: 999.0.into(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            }),
                    ]
                    .spacing(5),
                )
                .padding([6, 8])
                .width(Length::Fill)
                .style(theme::card_box(p))
                .into(),
            );
        }
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
        let mut max_row_w: f32 = 120.0;
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
        let min_content_w = max_row_w.clamp(120.0, 960.0);

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
                        text(format!("{bulk_count} Repo"))
                            .size(type_scale::CAPTION)
                            .color(p.accent_fg)
                            .wrapping(text::Wrapping::None),
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
