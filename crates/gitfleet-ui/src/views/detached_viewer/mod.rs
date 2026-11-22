//! GitFleet Studio — independent multi-tab code editor, Search & Replace, and Diff window.
//! Modularized into `state`, `io`, `tasks`, `update`, `update_local`, sidebars, `workbench`,
//! `find_bar`, and `status_bar`.

pub(crate) mod find_bar;
pub(crate) mod io;
pub(crate) mod sidebar_compare;
pub(crate) mod sidebar_explorer;
pub(crate) mod sidebar_search;
pub(crate) mod state;
pub(crate) mod status_bar;
pub(crate) mod tasks;
pub(crate) mod update;
pub(crate) mod update_local;
pub(crate) mod workbench;

#[cfg(test)]
mod tests;

pub(crate) use state::{DetachedAction, DetachedViewerState};

use self::sidebar_compare::render_compare_sidebar;
use self::sidebar_explorer::{render_activity_rail, render_explorer_sidebar};
use self::sidebar_search::render_search_sidebar;
use self::state::{StudioSidebarPanel, StudioStatusMenu};
use self::status_bar::render_studio_status_bar;
use self::workbench::render_editor_workbench;
use crate::app::{GitFleetApp, Message};
use crate::icons;
use crate::syntax::CodeLanguage;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use crate::views::overlays::doc_format_modal::{
    build_encoding_menu_card, build_eol_menu_card, build_syntax_menu_card,
};
use crate::views::truncate_chars;
use iced::widget::{
    Space, Stack, button, column, container, horizontal_rule, opaque, row, text, vertical_rule,
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
            body_row = body_row
                .push(sidebar)
                .push(vertical_rule(1).style(theme::subtle_rule(p)));
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

        let has_overlay =
            v.status_menu.is_some() || v.explorer_menu.is_some() || v.pending_delete.is_some();
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
            };
            layers.push(theme::overlay_backdrop(
                theme::popup_at(anchor, card),
                Message::Detached(win_id, DetachedAction::CloseStatusMenu),
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
                            .color(if danger { p.danger_fg } else { p.text_primary }),
                    ]
                    .spacing(space::SM)
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding([6, 10])
                .style(theme::context_menu_item(p, danger))
                .on_press(Message::Detached(win_id, act))
                .into()
            };

            let mut items = column![
                container(
                    text(truncate_chars(name, 28))
                        .size(type_scale::CAPTION)
                        .color(p.text_muted),
                )
                .padding([2, 10]),
            ]
            .spacing(2);

            if menu.is_dir {
                items = items.push(ctx_btn(
                    icons::folder_open(12.0, p.accent_fg).into(),
                    "Toggle Expand / Collapse",
                    DetachedAction::ToggleExplorerDir(menu.rel_path.clone()),
                    false,
                ));
            } else {
                items = items.push(ctx_btn(
                    icons::file_code(12.0, p.accent_fg).into(),
                    "Open in Editor",
                    DetachedAction::OpenExplorerFile(menu.rel_path.clone()),
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

        Stack::with_children(layers)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
