//! Far-left Activity Rail, language color badge helper, and collapsible Project Explorer sidebar.

use super::state::{DetachedAction, DetachedViewerState, StudioSidebarPanel};
use crate::app::Message;
use crate::icons;
use crate::syntax::CodeLanguage;
use crate::theme::{self, Palette, ThemeMode, type_scale};
use crate::views::{parent_dir, slim_scroll, tip};
use gitfleet_git::IgnoredDirListing;
use iced::widget::{
    Space, button, column, container, horizontal_rule, mouse_area, row, text, text_input, tooltip,
};
use iced::{Alignment, Color, Element, Length, window};
use std::collections::{BTreeMap, HashMap, HashSet};
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
    lang_tag_styled(lang, theme_mode, false)
}

fn lang_tag_styled<'a>(
    lang: CodeLanguage,
    theme_mode: ThemeMode,
    dimmed: bool,
) -> Element<'a, Message> {
    let base = lang.badge_color(theme_mode);
    let col = if dimmed {
        Color { a: 0.50, ..base }
    } else {
        base
    };
    text(lang.short_badge())
        .size(10)
        .font(theme::CODE_FONT)
        .color(col)
        .wrapping(text::Wrapping::None)
        .into()
}

// ── Sidebar 1: Project Explorer Tree ─────────────────────────────────────────

#[derive(Default)]
pub(crate) struct ExplorerDirNode {
    pub(crate) dirs: BTreeMap<String, ExplorerDirNode>,
    pub(crate) files: Vec<(String, String, bool)>, // (file_name, full_rel_path, is_ignored)
    pub(crate) has_non_ignored: bool,
    pub(crate) has_ignored: bool,
    pub(crate) lazy_ignored_dir: bool,
    pub(crate) lazy_loaded: bool,
    pub(crate) truncated: bool,
}

impl ExplorerDirNode {
    pub(crate) fn is_ignored_only(&self) -> bool {
        self.has_ignored && !self.has_non_ignored
    }

    pub(crate) fn insert(&mut self, rel: &str, is_ignored: bool) {
        let parts: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return;
        }
        if is_ignored {
            self.has_ignored = true;
        } else {
            self.has_non_ignored = true;
        }
        let mut cur = self;
        for dir in &parts[..parts.len() - 1] {
            cur = cur.dirs.entry((*dir).to_string()).or_default();
            if is_ignored {
                cur.has_ignored = true;
            } else {
                cur.has_non_ignored = true;
            }
        }
        cur.files
            .push((parts[parts.len() - 1].to_string(), rel.to_string(), is_ignored));
    }

    pub(crate) fn insert_ignored_dir(
        &mut self,
        dir_path: &str,
        cache: &HashMap<String, IgnoredDirListing>,
    ) {
        let trimmed = dir_path.trim_matches('/');
        if trimmed.is_empty() {
            return;
        }
        self.has_ignored = true;
        let mut cur = self;
        for seg in trimmed.split('/').filter(|s| !s.is_empty()) {
            cur = cur.dirs.entry(seg.to_string()).or_default();
            cur.has_ignored = true;
        }
        cur.lazy_ignored_dir = true;
        cur.populate_lazy_dir(trimmed, cache);
    }

    fn populate_lazy_dir(&mut self, full_path: &str, cache: &HashMap<String, IgnoredDirListing>) {
        let Some(listing) = cache.get(full_path) else {
            return;
        };
        self.lazy_loaded = true;
        self.truncated = listing.truncated;
        for f in &listing.files {
            let fname = f.rsplit('/').next().unwrap_or(f).to_string();
            self.files.push((fname, f.clone(), true));
        }
        for child_dir in &listing.dirs {
            let child_name = child_dir.rsplit('/').next().unwrap_or(child_dir);
            let child = self.dirs.entry(child_name.to_string()).or_default();
            child.has_ignored = true;
            child.lazy_ignored_dir = true;
            child.populate_lazy_dir(child_dir, cache);
        }
    }

    fn sort_files_recursive(&mut self) {
        self.files.sort_by(|a, b| a.0.cmp(&b.0));
        self.files.dedup_by(|a, b| a.1 == b.1);
        for child in self.dirs.values_mut() {
            child.sort_files_recursive();
        }
    }

    pub(crate) fn build(
        files: &[String],
        ignored_files: &[String],
        ignored_dirs: &[String],
        cache: &HashMap<String, IgnoredDirListing>,
        show_ignored: bool,
    ) -> Self {
        let mut root = Self::default();
        for rel in files {
            root.insert(rel, false);
        }
        if show_ignored {
            for rel in ignored_files {
                root.insert(rel, true);
            }
            for dir in ignored_dirs {
                root.insert_ignored_dir(dir, cache);
            }
        }
        root.sort_files_recursive();
        root
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
            button(icons::file_plus(12.0, p.text_muted))
                .padding([3, 5])
                .style(theme::vscode_icon_btn(p, false))
                .on_press(Message::Detached(
                    win_id,
                    DetachedAction::OpenExplorerCreate {
                        parent_rel: String::new(),
                        is_dir: false,
                    },
                )),
            "New File…",
            tooltip::Position::Bottom,
            p,
        ),
        tip(
            button(icons::folder_plus(12.0, p.text_muted))
                .padding([3, 5])
                .style(theme::vscode_icon_btn(p, false))
                .on_press(Message::Detached(
                    win_id,
                    DetachedAction::OpenExplorerCreate {
                        parent_rel: String::new(),
                        is_dir: true,
                    },
                )),
            "New Folder…",
            tooltip::Position::Bottom,
            p,
        ),
        tip(
            button(if v.explorer_show_ignored {
                icons::eye(12.0, p.accent_fg)
            } else {
                icons::eye_off(12.0, p.text_muted)
            })
            .padding([3, 5])
            .style(theme::vscode_icon_btn(p, v.explorer_show_ignored))
            .on_press(Message::Detached(
                win_id,
                DetachedAction::ToggleExplorerShowIgnored,
            )),
            if v.explorer_show_ignored {
                "Showing gitignored files & folders (dimmed like VS Code) — click to hide"
            } else {
                "Gitignored files & folders hidden — click to show (e.g. .env)"
            },
            tooltip::Position::Bottom,
            p,
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
    } else if v.explorer_loading
        && v.explorer_files.is_empty()
        && v.explorer_ignored_files.is_empty()
        && v.explorer_ignored_dirs.is_empty()
    {
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
            // Flat filtered results for instant jumping (includes gitignored files dimmed)
            let mut matched: Vec<(&str, bool)> = Vec::new();
            let mut seen: HashSet<&str> = HashSet::new();
            for rel in &v.explorer_files {
                if rel.to_lowercase().contains(&q) && seen.insert(rel.as_str()) {
                    matched.push((rel.as_str(), false));
                }
            }
            if v.explorer_show_ignored {
                for rel in &v.explorer_ignored_files {
                    if rel.to_lowercase().contains(&q) && seen.insert(rel.as_str()) {
                        matched.push((rel.as_str(), true));
                    }
                }
                for listing in v.explorer_ignored_dir_children.values() {
                    for rel in &listing.files {
                        if rel.to_lowercase().contains(&q) && seen.insert(rel.as_str()) {
                            matched.push((rel.as_str(), true));
                        }
                    }
                }
            }
            matched.sort_by(|a, b| a.0.cmp(b.0));

            let mut shown = 0usize;
            for (rel, is_ignored) in matched {
                let fname = rel.rsplit('/').next().unwrap_or(rel);
                let dir = parent_dir(rel);
                let is_active = active_rel == Some(rel);
                let lang = CodeLanguage::from_path(Path::new(rel));

                let name_color = if is_active {
                    p.accent_fg
                } else if is_ignored {
                    Color { a: 0.68, ..p.text_muted }
                } else {
                    p.text_primary
                };

                let mut inner = row![
                    container(lang_tag_styled(lang, theme_mode, is_ignored))
                        .width(Length::Fixed(20.0)),
                    text(fname)
                        .size(type_scale::CAPTION)
                        .color(name_color)
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(4)
                .align_y(Alignment::Center);

                if !dir.is_empty() {
                    inner = inner.push(
                        text(dir)
                            .size(type_scale::BADGE)
                            .color(if is_ignored {
                                Color { a: 0.56, ..p.text_muted }
                            } else {
                                p.text_muted
                            })
                            .wrapping(text::Wrapping::None),
                    );
                }

                let btn = button(theme::clipped_line(inner, Length::Fill))
                    .padding([3, 6])
                    .width(Length::Fill)
                    .style(theme::repo_row_btn(p, is_active))
                    .on_press(Message::Detached(
                        win_id,
                        DetachedAction::OpenExplorerFile(rel.to_string()),
                    ));

                list_col = list_col.push(mouse_area(btn).on_right_press(Message::Detached(
                    win_id,
                    DetachedAction::OpenExplorerMenu {
                        rel_path: rel.to_string(),
                        is_dir: false,
                    },
                )));
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
            let mut rows = Vec::new();
            render_explorer_node(
                win_id,
                &v.explorer_tree,
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
        let dimmed = child.is_ignored_only();
        let chev_color = if dimmed {
            Color { a: 0.52, ..p.text_muted }
        } else {
            p.text_muted
        };
        let chev: Element<'a, Message> = if is_open {
            icons::chevron_down(11.0, chev_color).into()
        } else {
            icons::chevron_right(11.0, chev_color).into()
        };
        let folder_color = if dimmed {
            Color { a: 0.56, ..p.text_muted }
        } else if is_open {
            p.accent_fg
        } else {
            p.text_secondary
        };
        let folder_ic: Element<'a, Message> = if is_open {
            icons::folder_open(12.0, folder_color).into()
        } else {
            icons::folder(12.0, folder_color).into()
        };

        let label = row![
            Space::with_width(Length::Fixed(indent)),
            chev,
            folder_ic,
            text(dir_name.clone())
                .size(type_scale::CAPTION)
                .color(if dimmed {
                    Color { a: 0.70, ..p.text_muted }
                } else {
                    p.text_primary
                })
                .wrapping(text::Wrapping::None),
        ]
        .spacing(4)
        .align_y(Alignment::Center);

        let dir_btn = button(theme::clipped_line(label, Length::Fill))
            .padding([3, 5])
            .width(Length::Fill)
            .style(theme::repo_row_btn(p, false))
            .on_press(Message::Detached(
                win_id,
                DetachedAction::ToggleExplorerDir(full.clone()),
            ));

        out.push(
            mouse_area(dir_btn)
                .on_right_press(Message::Detached(
                    win_id,
                    DetachedAction::OpenExplorerMenu {
                        rel_path: full.clone(),
                        is_dir: true,
                    },
                ))
                .into(),
        );

        if is_open {
            if child.lazy_ignored_dir && !child.lazy_loaded {
                out.push(
                    row![
                        Space::with_width(Length::Fixed(indent + 24.0)),
                        text("Loading…")
                            .size(type_scale::BADGE)
                            .color(p.text_muted)
                            .wrapping(text::Wrapping::None),
                    ]
                    .padding([2, 5])
                    .into(),
                );
            } else if child.dirs.is_empty() && child.files.is_empty() {
                out.push(
                    row![
                        Space::with_width(Length::Fixed(indent + 24.0)),
                        text("(empty)")
                            .size(type_scale::BADGE)
                            .color(Color { a: 0.60, ..p.text_muted })
                            .wrapping(text::Wrapping::None),
                    ]
                    .padding([2, 5])
                    .into(),
                );
            } else {
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
                if child.truncated {
                    out.push(
                        row![
                            Space::with_width(Length::Fixed(indent + 24.0)),
                            text("Showing first 500 entries…")
                                .size(type_scale::BADGE)
                                .color(Color { a: 0.65, ..p.text_muted })
                                .wrapping(text::Wrapping::None),
                        ]
                        .padding([2, 5])
                        .into(),
                    );
                }
            }
        }
    }

    for (fname, full_rel, is_ignored) in &node.files {
        let is_active = active_rel == Some(full_rel.as_str());
        let lang = CodeLanguage::from_path(Path::new(full_rel));
        let label = row![
            Space::with_width(Length::Fixed(indent + 12.0)),
            container(lang_tag_styled(lang, theme_mode, *is_ignored)).width(Length::Fixed(20.0)),
            text(fname.clone())
                .size(type_scale::CAPTION)
                .color(if is_active {
                    p.accent_fg
                } else if *is_ignored {
                    Color { a: 0.68, ..p.text_muted }
                } else {
                    p.text_secondary
                })
                .wrapping(text::Wrapping::None),
        ]
        .spacing(3)
        .align_y(Alignment::Center);

        let file_btn = button(theme::clipped_line(label, Length::Fill))
            .padding([3, 5])
            .width(Length::Fill)
            .style(theme::repo_row_btn(p, is_active))
            .on_press(Message::Detached(
                win_id,
                DetachedAction::OpenExplorerFile(full_rel.clone()),
            ));

        out.push(
            mouse_area(file_btn)
                .on_right_press(Message::Detached(
                    win_id,
                    DetachedAction::OpenExplorerMenu {
                        rel_path: full_rel.clone(),
                        is_dir: false,
                    },
                ))
                .into(),
        );
    }
}
