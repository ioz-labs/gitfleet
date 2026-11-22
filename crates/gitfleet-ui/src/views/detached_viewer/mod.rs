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
use crate::theme::{self, Palette, icon_size};
use crate::views::overlays::doc_format_modal::{
    build_encoding_menu_card, build_eol_menu_card, build_syntax_menu_card,
};
use iced::widget::{Stack, column, container, horizontal_rule, row, vertical_rule};
use iced::{Element, Length, window};
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

        let has_menu = v.status_menu.is_some();
        let mut layers: Vec<Element<'_, Message>> = vec![theme::inert_when(base, has_menu)];

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

        Stack::with_children(layers)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
