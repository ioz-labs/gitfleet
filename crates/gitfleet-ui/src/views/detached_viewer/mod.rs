//! GitFleet Studio — independent multi-tab code editor, Search & Replace, and Diff window.
//! Modularized into `state`, `io`, `tasks`, `update`, `update_local`, sidebars, `workbench`,
//! `find_bar`, and `status_bar`.

pub(crate) mod find_bar;
pub(crate) mod io;
pub(crate) mod sidebar_compare;
pub(crate) mod sidebar_explorer;
pub(crate) mod sidebar_search;
pub(crate) mod smart_edit;
pub(crate) mod state;
pub(crate) mod status_bar;
pub(crate) mod symbols;
pub(crate) mod tasks;
pub(crate) mod update;
pub(crate) mod update_local;
pub(crate) mod workbench;

#[cfg(test)]
mod tests;

pub(crate) use state::{DetachedAction, DetachedViewerState};

use self::sidebar_compare::render_compare_sidebar;
use self::sidebar_explorer::{lang_tag, render_activity_rail, render_explorer_sidebar};
use self::sidebar_search::render_search_sidebar;
use self::state::{StudioCreateEntryKind, StudioSidebarPanel, StudioStatusMenu};
use self::status_bar::render_studio_status_bar;
use self::workbench::render_editor_workbench;
use crate::app::{GitFleetApp, Message};
use crate::icons;
use crate::syntax::CodeLanguage;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use crate::views::overlays::doc_format_modal::{
    build_encoding_menu_card, build_eol_menu_card, build_syntax_menu_card,
};
use crate::views::overlays::file_ops_modal::{build_add_gitignore_modal, build_rename_entry_modal};
use crate::views::overlays::fuzzy::render_fuzzy_text;
use crate::views::{slim_scroll, truncate_chars};
use iced::widget::{
    Space, Stack, button, column, container, horizontal_rule, opaque, row, text, text_input,
    vertical_rule,
};
use iced::{Alignment, Element, Length, window};
use std::path::Path;

impl GitFleetApp {
    pub(crate) fn view_detached_viewer(
        &self,
        win_id: window::Id,
        p: Palette,
    ) -> Element<'_, Message> {
        let Some(v) = self.detached_viewers.get(&win_id) else {
            return container(theme::empty_state(
                p,
                icons::file_code(icon_size::LG, p.text_muted),
                "GitFleet Studio",
                "Initializing editor workspace…",
                None,
            ))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::app_bg(p))
            .into();
        };

        let activity_rail = render_activity_rail(win_id, v, p);
        let editor_area = render_editor_workbench(win_id, v, self.theme_mode, p);

        let mut body_row = row![
            activity_rail,
            vertical_rule(1).style(theme::subtle_rule(p)),
        ]
        .spacing(0)
        .width(Length::Fill)
        .height(Length::Fill);

        if v.sidebar_open {
            let sidebar = match v.sidebar_panel {
                StudioSidebarPanel::Explorer => {
                    render_explorer_sidebar(win_id, v, self.theme_mode, p)
                }
                StudioSidebarPanel::Search => render_search_sidebar(win_id, v, self.theme_mode, p),
                StudioSidebarPanel::Compare => render_compare_sidebar(win_id, v, p),
            };
            body_row = body_row.push(sidebar).push(theme::kit::hairline_v_splitter(
                p,
                v.resizing_sidebar,
                Message::Detached(win_id, DetachedAction::StartResizeSidebar),
            ));
        }

        body_row = body_row.push(editor_area);

        let status_bar = render_studio_status_bar(win_id, v, self.theme_mode, p);

        let base: Element<'_, Message> = container(
            column![
                body_row,
                horizontal_rule(1).style(theme::subtle_rule(p)),
                status_bar,
            ]
            .spacing(0)
            .width(Length::Fill)
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::app_bg(p))
        .into();

        let has_overlay = v.status_menu.is_some()
            || v.explorer_menu.is_some()
            || v.tab_menu.is_some()
            || v.breadcrumb_menu.is_some()
            || v.pending_delete.is_some()
            || v.create_modal.is_some()
            || v.rename_modal.is_some()
            || v.gitignore_modal.is_some()
            || v.quick_open_visible
            || v.goto_line_visible;
        let mut layers: Vec<Element<'_, Message>> = vec![theme::inert_when(base, has_overlay)];

        if let Some((menu_kind, anchor)) = v.status_menu
            && let Some(tab) = v.active_tab()
        {
            let card = match menu_kind {
                StudioStatusMenu::Eol => build_eol_menu_card(
                    p,
                    tab.line_ending,
                    tab.dirty,
                    move |eol| Message::Detached(win_id, DetachedAction::SetEditorEol(eol)),
                ),
                StudioStatusMenu::Encoding => build_encoding_menu_card(
                    p,
                    tab.encoding,
                    v.encoding_menu_mode,
                    tab.dirty,
                    move |m| Message::Detached(win_id, DetachedAction::SetEncodingMenuMode(m)),
                    move |enc| {
                        Message::Detached(win_id, DetachedAction::SaveEditorWithEncoding(enc))
                    },
                    move |enc| {
                        Message::Detached(win_id, DetachedAction::ReopenEditorWithEncoding(enc))
                    },
                ),
                StudioStatusMenu::Syntax => {
                    let auto_lang = tab
                        .file_path
                        .as_deref()
                        .or_else(|| tab.rel_path.as_deref().map(Path::new))
                        .map(CodeLanguage::from_path);
                    build_syntax_menu_card(
                        p,
                        self.theme_mode,
                        tab.language,
                        auto_lang,
                        &v.syntax_filter,
                        v.syntax_filter_id.clone(),
                        move |s| Message::Detached(win_id, DetachedAction::SyntaxFilterChanged(s)),
                        move |lang| Message::Detached(win_id, DetachedAction::SetTabLanguage(lang)),
                        auto_lang.map(|al| {
                            Message::Detached(win_id, DetachedAction::SetTabLanguage(al))
                        }),
                    )
                }
                StudioStatusMenu::Indent => {
                    let indent_opt = |label: &'static str, spaces: usize, use_tabs: bool| -> Element<'_, Message> {
                        let selected = if use_tabs {
                            tab.indent_use_tabs
                        } else {
                            !tab.indent_use_tabs && tab.indent_spaces == spaces
                        };
                        button(
                            row![
                                text(label)
                                    .size(type_scale::BODY)
                                    .color(if selected { p.accent_fg } else { p.text_primary }),
                                Space::with_width(Length::Fill),
                                if selected {
                                    Element::from(icons::check(12.0, p.accent_fg))
                                } else {
                                    Space::with_width(Length::Shrink).into()
                                },
                            ]
                            .align_y(Alignment::Center),
                        )
                        .width(Length::Fill)
                        .padding([5, 10])
                        .style(theme::repo_row_btn(p, selected))
                        .on_press(Message::Detached(
                            win_id,
                            DetachedAction::SetTabIndent { spaces, use_tabs },
                        ))
                        .into()
                    };
                    opaque(
                        container(
                            column![
                                container(
                                    text("Tab Indentation")
                                        .size(type_scale::CAPTION)
                                        .color(p.text_muted),
                                )
                                .padding([4, 10]),
                                indent_opt("Spaces: 2", 2, false),
                                indent_opt("Spaces: 4", 4, false),
                                indent_opt("Spaces: 8", 8, false),
                                horizontal_rule(1).style(theme::subtle_rule(p)),
                                indent_opt("Tab Characters", tab.indent_spaces, true),
                            ]
                            .spacing(2),
                        )
                        .padding(6)
                        .width(Length::Fixed(188.0))
                        .style(theme::context_menu_box(p)),
                    )
                    .into()
                }
            };
            layers.push(theme::overlay_backdrop(
                theme::popup_at(anchor, card),
                Message::Detached(win_id, DetachedAction::CloseStatusMenu),
            ));
        }

        if let Some(tmenu) = &v.tab_menu
            && let Some(tab) = v.tabs.get(tmenu.tab_idx)
        {
            let tidx = tmenu.tab_idx;
            let has_path = tab.file_path.is_some() || tab.rel_path.is_some();
            let menu_btn = |lbl: &'static str, kbd: &'static str, act: DetachedAction| -> Element<'_, Message> {
                let mut r = row![
                    text(lbl).size(type_scale::BODY).color(p.text_primary),
                    Space::with_width(Length::Fill),
                ]
                .align_y(Alignment::Center);
                if !kbd.is_empty() {
                    r = r.push(theme::kit::kbd_pill(p, kbd));
                }
                button(r)
                    .width(Length::Fill)
                    .padding([5, 10])
                    .style(theme::context_menu_item(p, false))
                    .on_press(Message::Detached(win_id, act))
                    .into()
            };

            let mut col = column![
                container(
                    text(truncate_chars(&tab.title, 28))
                        .size(type_scale::CAPTION)
                        .color(p.text_muted),
                )
                .padding([4, 10]),
                menu_btn("Close", "Ctrl+W", DetachedAction::CloseTab(tidx)),
                menu_btn("Close Others", "", DetachedAction::CloseOtherTabs(tidx)),
                menu_btn("Close Saved", "", DetachedAction::CloseSavedTabs),
                menu_btn("Close All", "", DetachedAction::CloseAllTabs),
            ]
            .spacing(2);

            if !v.closed_tabs.is_empty() {
                col = col.push(menu_btn(
                    "Reopen Closed Tab",
                    "Ctrl+Shift+T",
                    DetachedAction::ReopenClosedTab,
                ));
            }
            if has_path {
                col = col
                    .push(horizontal_rule(1).style(theme::subtle_rule(p)))
                    .push(menu_btn(
                        "Copy Path",
                        "",
                        DetachedAction::CopyTabPath {
                            tab_idx: tidx,
                            relative: false,
                        },
                    ))
                    .push(menu_btn(
                        "Copy Relative Path",
                        "",
                        DetachedAction::CopyTabPath {
                            tab_idx: tidx,
                            relative: true,
                        },
                    ));
            }

            let card = opaque(
                container(col)
                    .padding(6)
                    .width(Length::Fixed(228.0))
                    .style(theme::context_menu_box(p)),
            );
            layers.push(theme::overlay_backdrop(
                theme::popup_at(tmenu.position, card),
                Message::Detached(win_id, DetachedAction::CloseTabMenu),
            ));
        }

        if let Some(bmenu) = &v.breadcrumb_menu {
            let prefix = if bmenu.parent_dir.is_empty() {
                String::new()
            } else {
                format!("{}/", bmenu.parent_dir)
            };
            let mut sib_files: Vec<&str> = v
                .explorer_files
                .iter()
                .map(String::as_str)
                .filter(|rel| {
                    if let Some(rest) = rel.strip_prefix(&prefix) {
                        !rest.is_empty() && !rest.contains('/')
                    } else {
                        false
                    }
                })
                .take(40)
                .collect();
            sib_files.sort_unstable();

            let mut list_col = column![].spacing(2);
            if sib_files.is_empty() {
                list_col = list_col.push(
                    container(
                        text("No sibling files in folder")
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                    )
                    .padding([6, 10]),
                );
            } else {
                let active_rel = v.active_tab().and_then(|t| t.rel_path.as_deref());
                for rel in sib_files {
                    let fname = rel.rsplit('/').next().unwrap_or(rel);
                    let is_cur = active_rel == Some(rel);
                    let lang = CodeLanguage::from_path(Path::new(rel));
                    let rel_owned = rel.to_string();
                    list_col = list_col.push(
                        button(
                            row![
                                lang_tag(lang, self.theme_mode),
                                text(fname)
                                    .size(type_scale::CAPTION)
                                    .color(if is_cur { p.accent_fg } else { p.text_primary })
                                    .wrapping(text::Wrapping::None),
                            ]
                            .spacing(6)
                            .align_y(Alignment::Center),
                        )
                        .width(Length::Fill)
                        .padding([4, 8])
                        .style(theme::repo_row_btn(p, is_cur))
                        .on_press(Message::Detached(
                            win_id,
                            DetachedAction::QuickOpenSelectRelPath(rel_owned),
                        )),
                    );
                }
            }

            let card = opaque(
                container(slim_scroll(list_col, p).height(Length::Shrink))
                    .max_height(260.0)
                    .padding(6)
                    .width(Length::Fixed(240.0))
                    .style(theme::context_menu_box(p)),
            );
            layers.push(theme::overlay_backdrop(
                theme::popup_at(bmenu.position, card),
                Message::Detached(win_id, DetachedAction::CloseBreadcrumbMenu),
            ));
        }

        if let Some(menu) = &v.explorer_menu {
            let name = menu
                .rel_path
                .rsplit('/')
                .next()
                .unwrap_or(&menu.rel_path);
            let ctx_btn = |icon: Element<'static, Message>,
                           label: &'static str,
                           act: DetachedAction,
                           danger: bool|
             -> Element<'_, Message> {
                button(
                    row![
                        icon,
                        text(label)
                            .size(type_scale::BODY)
                            .line_height(text::LineHeight::Relative(1.15))
                            .color(if danger { p.danger_fg } else { p.text_primary }),
                    ]
                    .spacing(space::SM)
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding([5, 10])
                .style(theme::context_menu_item(p, danger))
                .on_press(Message::Detached(win_id, act))
                .into()
            };

            let mut items = column![
                container(
                    text(truncate_chars(name, 28))
                        .size(type_scale::CAPTION)
                        .line_height(text::LineHeight::Relative(1.15))
                        .color(p.text_muted),
                )
                .padding([5, 10]),
            ]
            .spacing(2);

            if menu.is_dir {
                items = items
                    .push(ctx_btn(
                        icons::folder_open(12.0, p.accent_fg).into(),
                        "Toggle Expand / Collapse",
                        DetachedAction::ToggleExplorerDir(menu.rel_path.clone()),
                        false,
                    ))
                    .push(ctx_btn(
                        icons::file_plus(12.0, p.text_secondary).into(),
                        "New File Here…",
                        DetachedAction::OpenExplorerCreate {
                            parent_rel: menu.rel_path.clone(),
                            is_dir: false,
                        },
                        false,
                    ))
                    .push(ctx_btn(
                        icons::folder_plus(12.0, p.text_secondary).into(),
                        "New Folder Here…",
                        DetachedAction::OpenExplorerCreate {
                            parent_rel: menu.rel_path.clone(),
                            is_dir: true,
                        },
                        false,
                    ));
            } else {
                let parent_dir = menu
                    .rel_path
                    .rsplit_once('/')
                    .map(|(p, _)| p.to_string())
                    .unwrap_or_default();
                items = items
                    .push(ctx_btn(
                        icons::file_code(12.0, p.accent_fg).into(),
                        "Open in Editor",
                        DetachedAction::OpenExplorerFile(menu.rel_path.clone()),
                        false,
                    ))
                    .push(ctx_btn(
                        icons::file_plus(12.0, p.text_secondary).into(),
                        "New File in Folder…",
                        DetachedAction::OpenExplorerCreate {
                            parent_rel: parent_dir,
                            is_dir: false,
                        },
                        false,
                    ))
                    .push(ctx_btn(
                        icons::copy(12.0, p.text_secondary).into(),
                        "Duplicate File…",
                        DetachedAction::OpenExplorerDuplicate {
                            src_rel: menu.rel_path.clone(),
                        },
                        false,
                    ));
            }

            items = items
                .push(ctx_btn(
                    icons::folder_open(12.0, p.text_secondary).into(),
                    "Open in System Explorer",
                    DetachedAction::RevealExplorerEntry(menu.rel_path.clone()),
                    false,
                ))
                .push(ctx_btn(
                    icons::copy(12.0, p.text_secondary).into(),
                    "Copy Relative Path",
                    DetachedAction::CopyExplorerPath(menu.rel_path.clone()),
                    false,
                ))
                .push(ctx_btn(
                    icons::pencil(12.0, p.text_secondary).into(),
                    "Rename…",
                    DetachedAction::OpenExplorerRename {
                        rel_path: menu.rel_path.clone(),
                        is_dir: menu.is_dir,
                    },
                    false,
                ))
                .push(ctx_btn(
                    icons::eye_off(12.0, p.text_secondary).into(),
                    "Add to .gitignore…",
                    DetachedAction::OpenExplorerGitignore {
                        rel_path: menu.rel_path.clone(),
                        is_dir: menu.is_dir,
                    },
                    false,
                ))
                .push(horizontal_rule(1).style(theme::subtle_rule(p)))
                .push(ctx_btn(
                    icons::trash(12.0, p.danger_fg).into(),
                    if menu.is_dir {
                        "Delete Folder…"
                    } else {
                        "Delete File…"
                    },
                    DetachedAction::RequestDeleteExplorerEntry {
                        rel_path: menu.rel_path.clone(),
                        is_dir: menu.is_dir,
                    },
                    true,
                ));

            let card = opaque(
                container(items)
                    .padding(6)
                    .width(Length::Fixed(236.0))
                    .style(theme::context_menu_box(p)),
            );
            layers.push(theme::overlay_backdrop(
                theme::popup_at(menu.position, card),
                Message::Detached(win_id, DetachedAction::CloseExplorerMenu),
            ));
        }

        if let Some(pending) = &v.pending_delete {
            let title = if pending.is_dir {
                "Delete Folder Permanently?"
            } else {
                "Delete File Permanently?"
            };
            let body = if pending.is_dir {
                "Permanently deletes this folder and all files inside it from disk. This action cannot be undone."
            } else {
                "Permanently deletes this file from disk. This action cannot be undone."
            };
            let detail = if pending.is_dir {
                format!("{}/", pending.rel_path)
            } else {
                pending.rel_path.clone()
            };
            let confirm_lbl = if pending.is_dir {
                "Delete Folder"
            } else {
                "Delete File"
            };

            let modal = opaque(
                container(
                    column![
                        row![
                            icons::alert_triangle(icon_size::MD, p.danger_fg),
                            text(title)
                                .size(type_scale::HEADING)
                                .color(p.text_primary),
                        ]
                        .spacing(space::SM)
                        .align_y(Alignment::Center),
                        text(body)
                            .size(type_scale::BODY)
                            .color(p.text_secondary),
                        container(
                            text(detail)
                                .size(type_scale::CAPTION)
                                .font(theme::CODE_FONT)
                                .color(p.text_primary),
                        )
                        .padding([6, 10])
                        .width(Length::Fill)
                        .style(theme::section_panel_box(p)),
                        row![
                            Space::with_width(Length::Fill),
                            button(text("Cancel").size(type_scale::BODY))
                                .padding([6, 14])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::Detached(
                                    win_id,
                                    DetachedAction::CancelDeleteExplorerEntry,
                                )),
                            button(text(confirm_lbl).size(type_scale::BODY))
                                .padding([6, 16])
                                .style(theme::danger_btn(p))
                                .on_press(Message::Detached(
                                    win_id,
                                    DetachedAction::ConfirmDeleteExplorerEntry,
                                )),
                        ]
                        .spacing(space::SM)
                        .align_y(Alignment::Center),
                    ]
                    .spacing(space::MD),
                )
                .padding(pad::CARD + 6)
                .width(Length::Fixed(layout::MODAL_SM))
                .style(theme::modal_box(p)),
            );

            let centered = container(modal)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .style(theme::backdrop_box);

            layers.push(theme::overlay_backdrop(
                centered,
                Message::Detached(win_id, DetachedAction::CancelDeleteExplorerEntry),
            ));
        }

        if let Some(modal) = &v.create_modal {
            let (title, subtitle, placeholder, submit_lbl) = match &modal.kind {
                StudioCreateEntryKind::File { parent_rel } => (
                    "New File",
                    if parent_rel.is_empty() {
                        "Create inside workspace root (nested paths like src/foo.rs supported)"
                            .to_string()
                    } else {
                        format!("Create inside '{parent_rel}/'")
                    },
                    "e.g. components/Card.tsx",
                    "Create File",
                ),
                StudioCreateEntryKind::Folder { parent_rel } => (
                    "New Folder",
                    if parent_rel.is_empty() {
                        "Create folder inside workspace root".to_string()
                    } else {
                        format!("Create folder inside '{parent_rel}/'")
                    },
                    "e.g. utils",
                    "Create Folder",
                ),
                StudioCreateEntryKind::Duplicate { src_rel } => (
                    "Duplicate File",
                    format!("Copy '{src_rel}' into same directory as:"),
                    "New file name…",
                    "Duplicate",
                ),
            };

            let mut body_col = column![
                text_input(placeholder, &modal.input)
                    .id(v.create_input_id.clone())
                    .on_input(move |s| {
                        Message::Detached(win_id, DetachedAction::ExplorerCreateInputChanged(s))
                    })
                    .on_submit(Message::Detached(
                        win_id,
                        DetachedAction::SubmitExplorerCreate,
                    ))
                    .padding([7, 10])
                    .size(type_scale::BODY)
                    .font(theme::CODE_FONT)
                    .style(theme::dark_input(p)),
            ]
            .spacing(space::SM);

            if let Some(err) = &modal.error {
                body_col = body_col.push(
                    text(err)
                        .size(type_scale::CAPTION)
                        .color(p.danger_fg),
                );
            }

            let footer = row![
                Space::with_width(Length::Fill),
                button(text("Cancel").size(type_scale::BODY))
                    .padding([6, 14])
                    .style(theme::subtle_btn(p))
                    .on_press(Message::Detached(
                        win_id,
                        DetachedAction::CancelExplorerCreate,
                    )),
                button(text(submit_lbl).size(type_scale::BODY))
                    .padding([6, 16])
                    .style(theme::primary_btn(p))
                    .on_press(Message::Detached(
                        win_id,
                        DetachedAction::SubmitExplorerCreate,
                    )),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center);

            layers.push(
                theme::kit::ModalShell::new(p, title, body_col)
                    .subtitle(subtitle)
                    .size(theme::kit::ModalSize::Sm)
                    .scrollable_body(false)
                    .on_close(Message::Detached(
                        win_id,
                        DetachedAction::CancelExplorerCreate,
                    ))
                    .footer(footer)
                    .into_element(),
            );
        }

        if v.quick_open_visible {
            let candidates = v.quick_open_candidates(60);
            let q_trimmed = v.quick_open_query.trim();
            let is_line_mode = q_trimmed.starts_with(':');
            let is_symbol_mode = q_trimmed.starts_with('@');
            let symbol_items = if is_symbol_mode {
                v.quick_open_symbols(80)
            } else {
                Vec::new()
            };

            let input = text_input(
                "Type file name, @symbol to jump to symbol, or :line[:col]…",
                &v.quick_open_query,
            )
            .id(v.quick_open_input_id.clone())
            .on_input(move |s| Message::Detached(win_id, DetachedAction::QuickOpenQueryChanged(s)))
            .on_submit(Message::Detached(
                win_id,
                DetachedAction::QuickOpenConfirmSelected,
            ))
            .padding([8, 12])
            .size(type_scale::BODY)
            .style(theme::dark_input(p));

            let list_body: Element<'_, Message> = if is_symbol_mode {
                if symbol_items.is_empty() {
                    container(
                        text("No matching symbols in active document.")
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                    )
                    .padding([12, 14])
                    .into()
                } else {
                    let mut rows_col = column![].spacing(2);
                    let sel_idx = v.quick_open_selected.min(symbol_items.len() - 1);
                    for (idx, (sym, positions)) in symbol_items.into_iter().enumerate() {
                        let is_sel = idx == sel_idx;
                        let line_idx = sym.line_idx;
                        let sym_row = row![
                            theme::kit::focus_accent_bar(p, is_sel, true, 16.0),
                            Space::with_width(Length::Fixed((sym.indent_level as f32) * 10.0)),
                            theme::kit::status_pill(
                                sym.kind_badge,
                                p.accent_fg,
                                p.fill_selected,
                            ),
                            theme::clipped_line(
                                render_fuzzy_text(
                                    &sym.name,
                                    &positions,
                                    if is_sel { p.text_primary } else { p.text_secondary },
                                    p.accent_fg,
                                ),
                                Length::Fill,
                            ),
                            text(format!("Ln {}", line_idx + 1))
                                .size(type_scale::BADGE)
                                .font(theme::CODE_FONT)
                                .color(p.text_muted),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center);

                        rows_col = rows_col.push(
                            button(sym_row)
                                .width(Length::Fill)
                                .padding([6, 10])
                                .style(theme::repo_row_btn(p, is_sel))
                                .on_press(Message::Detached(
                                    win_id,
                                    DetachedAction::QuickOpenSelectSymbol(line_idx),
                                )),
                        );
                    }
                    slim_scroll(rows_col, p).height(Length::Shrink).into()
                }
            } else if is_line_mode {
                let total = v
                    .active_tab()
                    .map(|t| t.editor_content.line_count())
                    .unwrap_or(0);
                container(
                    row![
                        icons::file_code(13.0, p.accent_fg),
                        text(format!(
                            "Press Enter to jump to {} (1..={})",
                            q_trimmed,
                            total.max(1)
                        ))
                        .size(type_scale::BODY)
                        .color(p.text_primary),
                    ]
                    .spacing(space::SM)
                    .align_y(Alignment::Center),
                )
                .padding([10, 12])
                .into()
            } else if candidates.is_empty() {
                container(
                    text(if v.search_root.is_none() && v.tabs.is_empty() {
                        "Open a folder or file first to browse workspace files."
                    } else {
                        "No matching files."
                    })
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
                )
                .padding([12, 14])
                .into()
            } else {
                let mut rows_col = column![].spacing(2);
                let sel_idx = v.quick_open_selected.min(candidates.len().saturating_sub(1));
                for (idx, item) in candidates.into_iter().enumerate() {
                    let is_sel = idx == sel_idx;
                    let lang = CodeLanguage::from_path(Path::new(&item.rel_path));
                    let normal_col = if item.is_ignored {
                        p.text_muted
                    } else if is_sel {
                        p.text_primary
                    } else {
                        p.text_secondary
                    };
                    let mut line_row = row![
                        theme::kit::focus_accent_bar(p, is_sel, true, 16.0),
                        lang_tag(lang, self.theme_mode),
                        theme::clipped_line(
                            render_fuzzy_text(
                                &item.rel_path,
                                &item.positions,
                                normal_col,
                                p.accent_fg,
                            ),
                            Length::Fill,
                        ),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center);

                    if item.is_open_tab {
                        line_row = line_row.push(theme::kit::status_pill(
                            "open",
                            p.accent_fg,
                            p.fill_selected,
                        ));
                    } else if item.is_ignored {
                        line_row = line_row.push(
                            text("ignored")
                                .size(type_scale::BADGE)
                                .color(p.text_muted),
                        );
                    }

                    let rel_target = item.rel_path.clone();
                    rows_col = rows_col.push(
                        button(line_row)
                            .width(Length::Fill)
                            .padding([6, 10])
                            .style(theme::repo_row_btn(p, is_sel))
                            .on_press(Message::Detached(
                                win_id,
                                DetachedAction::QuickOpenSelectRelPath(rel_target),
                            )),
                    );
                }
                slim_scroll(rows_col, p)
                    .height(Length::Shrink)
                    .into()
            };

            let card = opaque(
                container(
                    column![
                        input,
                        horizontal_rule(1).style(theme::subtle_rule(p)),
                        container(list_body).max_height(340.0).width(Length::Fill),
                        horizontal_rule(1).style(theme::subtle_rule(p)),
                        row![
                            theme::kit::kbd_pill(p, "↑↓"),
                            text("navigate").size(type_scale::BADGE).color(p.text_muted),
                            theme::kit::kbd_pill(p, "Enter"),
                            text("open").size(type_scale::BADGE).color(p.text_muted),
                            theme::kit::kbd_pill(p, "@"),
                            text("symbols").size(type_scale::BADGE).color(p.text_muted),
                            theme::kit::kbd_pill(p, ":42"),
                            text("go to line").size(type_scale::BADGE).color(p.text_muted),
                            Space::with_width(Length::Fill),
                            theme::kit::kbd_pill(p, "Esc"),
                        ]
                        .spacing(6)
                        .align_y(Alignment::Center)
                        .padding([2, 4]),
                    ]
                    .spacing(6),
                )
                .padding(10)
                .width(Length::Fixed(layout::MODAL_MD))
                .style(theme::modal_box(p)),
            );

            let top_overlay = container(card)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(iced::Padding {
                    top: 48.0,
                    right: 16.0,
                    bottom: 16.0,
                    left: 16.0,
                })
                .align_x(iced::alignment::Horizontal::Center)
                .style(theme::backdrop_box);

            layers.push(theme::overlay_backdrop(
                top_overlay,
                Message::Detached(win_id, DetachedAction::CloseQuickOpen),
            ));
        }

        if v.goto_line_visible {
            let total_lines = v
                .active_tab()
                .map(|t| t.editor_content.line_count())
                .unwrap_or(1)
                .max(1);
            let card = opaque(
                container(
                    column![
                        row![
                            text("Go to Line / Column")
                                .size(type_scale::CAPTION)
                                .color(p.text_primary),
                            Space::with_width(Length::Fill),
                            text(format!("1..={total_lines}"))
                                .size(type_scale::BADGE)
                                .font(theme::CODE_FONT)
                                .color(p.text_muted),
                        ]
                        .align_y(Alignment::Center),
                        text_input(
                            &format!("Enter line[:col] (1..={total_lines})…"),
                            &v.goto_line_input,
                        )
                        .id(v.goto_line_input_id.clone())
                        .on_input(move |s| {
                            Message::Detached(win_id, DetachedAction::GoToLineInputChanged(s))
                        })
                        .on_submit(Message::Detached(win_id, DetachedAction::SubmitGoToLine))
                        .padding([7, 10])
                        .size(type_scale::BODY)
                        .font(theme::CODE_FONT)
                        .style(theme::dark_input(p)),
                    ]
                    .spacing(6),
                )
                .padding(10)
                .width(Length::Fixed(360.0))
                .style(theme::modal_box(p)),
            );

            let top_overlay = container(card)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(iced::Padding {
                    top: 52.0,
                    right: 16.0,
                    bottom: 16.0,
                    left: 16.0,
                })
                .align_x(iced::alignment::Horizontal::Center)
                .style(theme::backdrop_box);

            layers.push(theme::overlay_backdrop(
                top_overlay,
                Message::Detached(win_id, DetachedAction::CloseGoToLine),
            ));
        }

        if let Some(modal) = &v.rename_modal {
            layers.push(build_rename_entry_modal(
                p,
                modal,
                v.rename_input_id.clone(),
                move |s| Message::Detached(win_id, DetachedAction::ExplorerRenameInputChanged(s)),
                Message::Detached(win_id, DetachedAction::SubmitExplorerRename),
                Message::Detached(win_id, DetachedAction::CancelExplorerRename),
            ));
        }

        if let Some(modal) = &v.gitignore_modal {
            layers.push(build_add_gitignore_modal(
                p,
                modal,
                v.gitignore_input_id.clone(),
                move |use_re| {
                    Message::Detached(win_id, DetachedAction::ExplorerGitignoreModeChanged(use_re))
                },
                move |s| Message::Detached(win_id, DetachedAction::ExplorerGitignoreGlobChanged(s)),
                move |s| {
                    Message::Detached(win_id, DetachedAction::ExplorerGitignoreRegexChanged(s))
                },
                move |exp| {
                    Message::Detached(
                        win_id,
                        DetachedAction::ExplorerGitignoreExpandMatchesToggled(exp),
                    )
                },
                move |layer| {
                    Message::Detached(win_id, DetachedAction::ExplorerGitignoreLayerChanged(layer))
                },
                Message::Detached(win_id, DetachedAction::SubmitExplorerGitignore),
                Message::Detached(win_id, DetachedAction::CancelExplorerGitignore),
            ));
        }

        Stack::with_children(layers)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
