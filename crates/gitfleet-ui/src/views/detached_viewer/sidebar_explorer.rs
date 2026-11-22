//! Far-left Activity Rail, language color badge helper, and collapsible Project Explorer sidebar.

use super::state::{DetachedAction, DetachedViewerState, StudioSidebarPanel};
use crate::app::Message;
use crate::icons;
use crate::syntax::CodeLanguage;
use crate::theme::{self, Palette, ThemeMode, type_scale};
use crate::views::{parent_dir, slim_scroll, tip};
use iced::widget::{
    Space, button, column, container, horizontal_rule, row, text, text_input, tooltip,
};
use iced::{Alignment, Element, Length, window};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

// ── Activity Rail (Far-Left Minimal Icon Strip) ──────────────────────────────

pub(crate) fn render_activity_rail<'a>(
    win_id: window::Id,
    v: &'a DetachedViewerState,
    p: Palette,
) -> Element<'a, Message> {
    let rail_item = |icon: Element<'a, Message>,
                     active: bool,
                     hint: &'static str,
                     act: DetachedAction|
     -> Element<'a, Message> {
        tip(
            button(
                container(icon)
                    .width(Length::Fixed(28.0))
                    .height(Length::Fixed(28.0))
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center),
            )
            .padding(2)
            .style(theme::studio_activity_btn(p, active))
            .on_press(Message::Detached(win_id, act)),
            hint,
            tooltip::Position::Right,
            p,
        )
    };

    let exp_active = v.sidebar_open && v.sidebar_panel == StudioSidebarPanel::Explorer;
    let search_active = v.sidebar_open && v.sidebar_panel == StudioSidebarPanel::Search;
    let cmp_active = v.sidebar_open && v.sidebar_panel == StudioSidebarPanel::Compare;

    let search_icon: Element<'a, Message> = if v.search_result.total_matches > 0 {
        column![
            icons::search(15.0, if search_active { p.accent_fg } else { p.text_secondary }),
            text(format!("{}", v.search_result.total_matches.min(999)))
                .size(10)
                .font(theme::CODE_FONT)
                .color(p.accent_fg)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(1)
        .align_x(Alignment::Center)
        .into()
    } else {
        icons::search(15.0, if search_active { p.accent_fg } else { p.text_secondary }).into()
    };

    container(
        column![
            rail_item(
                icons::folder_tree(
                    15.0,
                    if exp_active { p.accent_fg } else { p.text_secondary }
                )
                .into(),
                exp_active,
                "Explorer (Ctrl+Shift+E)",
                DetachedAction::SetSidebarPanel(StudioSidebarPanel::Explorer),
            ),
            rail_item(
                search_icon,
                search_active,
                "Search & Replace (Ctrl+Shift+F)",
                DetachedAction::SetSidebarPanel(StudioSidebarPanel::Search),
            ),
            rail_item(
                icons::file_diff(
                    15.0,
                    if cmp_active { p.accent_fg } else { p.text_secondary }
                )
                .into(),
                cmp_active,
                "Compare Local Files",
                DetachedAction::SetSidebarPanel(StudioSidebarPanel::Compare),
            ),
            Space::with_height(Length::Fill),
            rail_item(
                icons::folder_open(15.0, p.text_muted).into(),
                false,
                "Open File (Ctrl+O)",
                DetachedAction::PickSingleFile,
            ),
            rail_item(
                icons::folder_plus(15.0, p.text_muted).into(),
                false,
                "Open Folder…",
                DetachedAction::PickSearchRoot,
            ),
        ]
        .spacing(6)
        .align_x(Alignment::Center)
        .height(Length::Fill),
    )
    .padding([8, 4])
    .width(Length::Fixed(42.0))
    .height(Length::Fill)
    .style(theme::studio_activity_rail(p))
    .into()
}

// ── Language File Tag Helper ─────────────────────────────────────────────────

pub(crate) fn lang_tag<'a>(lang: CodeLanguage, theme_mode: ThemeMode) -> Element<'a, Message> {
    let col = lang.badge_color(theme_mode);
    text(lang.short_badge())
        .size(10)
        .font(theme::CODE_FONT)
        .color(col)
        .wrapping(text::Wrapping::None)
        .into()
}

// ── Sidebar 1: Project Explorer Tree ─────────────────────────────────────────

#[derive(Default)]
struct ExplorerDirNode {
    dirs: BTreeMap<String, ExplorerDirNode>,
    files: Vec<(String, String)>, // (file_name, full_rel_path)
}

impl ExplorerDirNode {
    fn insert(&mut self, rel: &str) {
        let parts: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return;
        }
        let mut cur = self;
        for dir in &parts[..parts.len() - 1] {
            cur = cur.dirs.entry((*dir).to_string()).or_default();
        }
        cur.files
            .push((parts[parts.len() - 1].to_string(), rel.to_string()));
    }
}

pub(crate) fn render_explorer_sidebar<'a>(
    win_id: window::Id,
    v: &'a DetachedViewerState,
    theme_mode: ThemeMode,
    p: Palette,
) -> Element<'a, Message> {
    let root_name = v
        .search_root_name
        .as_deref()
        .unwrap_or("No folder open");

    let header = row![
        theme::clipped_line(
            text(root_name)
                .size(type_scale::CAPTION)
                .color(p.text_primary)
                .wrapping(text::Wrapping::None),
            Length::Fill,
        ),
        tip(
            button(icons::refresh(12.0, p.text_muted))
                .padding([3, 5])
                .style(theme::vscode_icon_btn(p, false))
                .on_press(Message::Detached(win_id, DetachedAction::RefreshExplorer)),
            "Refresh file tree",
            tooltip::Position::Bottom,
            p,
        ),
        tip(
            button(icons::folder_open(12.0, p.text_muted))
                .padding([3, 5])
                .style(theme::vscode_icon_btn(p, false))
                .on_press(Message::Detached(win_id, DetachedAction::PickSearchRoot)),
            "Open different folder",
            tooltip::Position::Bottom,
            p,
        ),
    ]
    .spacing(3)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let filter_box = text_input("Filter files…", &v.explorer_filter)
        .id(v.explorer_filter_id.clone())
        .on_input(move |s| Message::Detached(win_id, DetachedAction::ExplorerFilterChanged(s)))
        .padding([4, 8])
        .size(type_scale::CAPTION)
        .width(Length::Fill)
        .style(theme::dark_input(p));

    let active_rel = v.active_tab().and_then(|t| t.rel_path.as_deref());
    let mut list_col = column![].spacing(1).width(Length::Fill);

    if v.search_root.is_none() {
        list_col = list_col.push(
            container(
                column![
                    text("Open a folder to browse files.")
                        .size(type_scale::CAPTION)
                        .color(p.text_muted),
                    button(
                        text("Open Folder…")
                            .size(type_scale::CAPTION)
                            .wrapping(text::Wrapping::None),
                    )
                    .padding([5, 10])
                    .style(theme::primary_btn(p))
                    .on_press(Message::Detached(win_id, DetachedAction::PickSearchRoot)),
                ]
                .spacing(8),
            )
            .padding([12, 6]),
        );
    } else if v.explorer_loading && v.explorer_files.is_empty() {
        list_col = list_col.push(
            container(
                text("Scanning folder…")
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
            )
            .padding([10, 6]),
        );
    } else {
        let q = v.explorer_filter.trim().to_lowercase();
        if !q.is_empty() {
            // Flat filtered results for instant jumping
            let mut shown = 0usize;
            for rel in &v.explorer_files {
                if !rel.to_lowercase().contains(&q) {
                    continue;
                }
                let fname = rel.rsplit('/').next().unwrap_or(rel);
                let dir = parent_dir(rel);
                let is_active = active_rel == Some(rel.as_str());
                let lang = CodeLanguage::from_path(Path::new(rel));

                let mut inner = row![
                    container(lang_tag(lang, theme_mode)).width(Length::Fixed(20.0)),
                    text(fname)
                        .size(type_scale::CAPTION)
                        .color(if is_active { p.accent_fg } else { p.text_primary })
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(4)
                .align_y(Alignment::Center);

                if !dir.is_empty() {
                    inner = inner.push(
                        text(dir)
                            .size(type_scale::BADGE)
                            .color(p.text_muted)
                            .wrapping(text::Wrapping::None),
                    );
                }

                list_col = list_col.push(
                    button(theme::clipped_line(inner, Length::Fill))
                        .padding([3, 6])
                        .width(Length::Fill)
                        .style(theme::repo_row_btn(p, is_active))
                        .on_press(Message::Detached(
                            win_id,
                            DetachedAction::OpenExplorerFile(rel.clone()),
                        )),
                );
                shown += 1;
                if shown >= 200 {
                    break;
                }
            }
            if shown == 0 {
                list_col = list_col.push(
                    container(
                        text("No matching files")
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                    )
                    .padding([10, 6]),
                );
            }
        } else {
            let mut root_node = ExplorerDirNode::default();
            for rel in &v.explorer_files {
                root_node.insert(rel);
            }
            let mut rows = Vec::new();
            render_explorer_node(
                win_id,
                &root_node,
                "",
                0,
                &v.explorer_expanded_dirs,
                active_rel,
                theme_mode,
                p,
                &mut rows,
            );
            list_col = column(rows).spacing(1).width(Length::Fill);
        }
    }

    container(
        column![
            header,
            filter_box,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            slim_scroll(list_col, p).height(Length::Fill),
        ]
        .spacing(6)
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .padding(8)
    .width(Length::Fixed(264.0))
    .height(Length::Fill)
    .style(theme::sidebar_box(p))
    .into()
}

#[allow(clippy::too_many_arguments)]
fn render_explorer_node<'a>(
    win_id: window::Id,
    node: &ExplorerDirNode,
    parent_path: &str,
    depth: usize,
    expanded: &HashSet<String>,
    active_rel: Option<&str>,
    theme_mode: ThemeMode,
    p: Palette,
    out: &mut Vec<Element<'a, Message>>,
) {
    let indent = (depth as f32) * 14.0;

    for (dir_name, child) in &node.dirs {
        let full = if parent_path.is_empty() {
            dir_name.clone()
        } else {
            format!("{parent_path}/{dir_name}")
        };
        let is_open = expanded.contains(&full);
        let chev: Element<'a, Message> = if is_open {
            icons::chevron_down(11.0, p.text_muted).into()
        } else {
            icons::chevron_right(11.0, p.text_muted).into()
        };
        let folder_ic: Element<'a, Message> = if is_open {
            icons::folder_open(12.0, p.accent_fg).into()
        } else {
            icons::folder(12.0, p.text_secondary).into()
        };

        let label = row![
            Space::with_width(Length::Fixed(indent)),
            chev,
            folder_ic,
            text(dir_name.clone())
                .size(type_scale::CAPTION)
                .color(p.text_primary)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(4)
        .align_y(Alignment::Center);

        out.push(
            button(theme::clipped_line(label, Length::Fill))
                .padding([3, 5])
                .width(Length::Fill)
                .style(theme::repo_row_btn(p, false))
                .on_press(Message::Detached(
                    win_id,
                    DetachedAction::ToggleExplorerDir(full.clone()),
                ))
                .into(),
        );

        if is_open {
            render_explorer_node(
                win_id,
                child,
                &full,
                depth + 1,
                expanded,
                active_rel,
                theme_mode,
                p,
                out,
            );
        }
    }

    for (fname, full_rel) in &node.files {
        let is_active = active_rel == Some(full_rel.as_str());
        let lang = CodeLanguage::from_path(Path::new(full_rel));
        let label = row![
            Space::with_width(Length::Fixed(indent + 12.0)),
            container(lang_tag(lang, theme_mode)).width(Length::Fixed(20.0)),
            text(fname.clone())
                .size(type_scale::CAPTION)
                .color(if is_active { p.accent_fg } else { p.text_secondary })
                .wrapping(text::Wrapping::None),
        ]
        .spacing(3)
        .align_y(Alignment::Center);

        out.push(
            button(theme::clipped_line(label, Length::Fill))
                .padding([3, 5])
                .width(Length::Fill)
                .style(theme::repo_row_btn(p, is_active))
                .on_press(Message::Detached(
                    win_id,
                    DetachedAction::OpenExplorerFile(full_rel.clone()),
                ))
                .into(),
        );
    }
}
