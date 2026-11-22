use crate::app::{EncodingModalMode, GitFleetApp, Message};
use crate::icons;
use crate::syntax::CodeLanguage;
use crate::theme::{self, Palette, ThemeMode, icon_size, space, type_scale};
use crate::views::slim_scroll_overlay;
use gitfleet_docs::{FileEncoding, LineEnding};
use iced::widget::{
    Space, button, column, container, horizontal_rule, opaque, row, text, text_input,
};
use iced::{Alignment, Element, Length, Point};
use std::path::Path;

/// Reusable compact anchored dropdown menu card for selecting End of Line Sequence (`LF` / `CRLF`).
pub(crate) fn build_eol_menu_card<'a>(
    p: Palette,
    current_eol: LineEnding,
    editor_dirty: bool,
    on_select: impl Fn(LineEnding) -> Message + 'a,
) -> Element<'a, Message> {
    let mut items = column![
        container(
            text("End of Line Sequence")
                .size(type_scale::CAPTION)
                .color(p.text_muted),
        )
        .padding([3, 8]),
        horizontal_rule(1).style(theme::subtle_rule(p)),
    ]
    .spacing(2)
    .width(Length::Fill);

    for &eol in LineEnding::all() {
        let is_current = current_eol == eol;
        items = items.push(
            button(
                row![
                    if is_current {
                        Element::from(icons::check(icon_size::XS, p.success_fg))
                    } else {
                        Space::with_width(Length::Fixed(icon_size::XS)).into()
                    },
                    text(eol.label())
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(if is_current {
                            p.accent_fg
                        } else {
                            p.text_primary
                        })
                        .wrapping(text::Wrapping::None),
                    Space::with_width(Length::Fill),
                    text(eol.description())
                        .size(type_scale::BADGE)
                        .color(p.text_muted)
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(space::XS)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding([6, 8])
            .style(theme::context_menu_item(p, false))
            .on_press(on_select(eol)),
        );
    }

    if editor_dirty {
        items = items
            .push(horizontal_rule(1).style(theme::subtle_rule(p)))
            .push(
                container(
                    text("Unsaved buffer — applies on Ctrl+S")
                        .size(type_scale::BADGE)
                        .color(p.warning_fg),
                )
                .padding([3, 8]),
            );
    }

    opaque(
        container(items)
            .padding(6)
            .width(Length::Fixed(248.0))
            .style(theme::context_menu_box(p)),
    )
}

/// Reusable compact anchored dropdown menu card for selecting File Encoding (Save or Reopen).
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_encoding_menu_card<'a>(
    p: Palette,
    current_encoding: FileEncoding,
    mode: EncodingModalMode,
    editor_dirty: bool,
    on_set_mode: impl Fn(EncodingModalMode) -> Message + 'a,
    on_save: impl Fn(FileEncoding) -> Message + 'a,
    on_reopen: impl Fn(FileEncoding) -> Message + 'a,
) -> Element<'a, Message> {
    let is_save = mode == EncodingModalMode::SaveWithEncoding;

    let mode_switch = theme::segmented_group(
        p,
        row![
            button(
                text("Save with")
                    .size(type_scale::BADGE)
                    .wrapping(text::Wrapping::None),
            )
            .padding([4, 8])
            .width(Length::FillPortion(1))
            .style(theme::segmented_btn(p, is_save))
            .on_press(on_set_mode(EncodingModalMode::SaveWithEncoding)),
            button(
                text("Reopen with")
                    .size(type_scale::BADGE)
                    .wrapping(text::Wrapping::None),
            )
            .padding([4, 8])
            .width(Length::FillPortion(1))
            .style(theme::segmented_btn(p, !is_save))
            .on_press(on_set_mode(EncodingModalMode::ReopenWithEncoding)),
        ]
        .spacing(2)
        .width(Length::Fill),
    );

    let mut list = column![].spacing(1).width(Length::Fill);
    for &enc in FileEncoding::all() {
        let is_current = current_encoding == enc;
        let msg = if is_save {
            on_save(enc)
        } else {
            on_reopen(enc)
        };
        list = list.push(
            button(
                row![
                    if is_current {
                        Element::from(icons::check(icon_size::XS, p.success_fg))
                    } else {
                        Space::with_width(Length::Fixed(icon_size::XS)).into()
                    },
                    column![
                        text(enc.label())
                            .size(type_scale::CAPTION)
                            .color(if is_current {
                                p.accent_fg
                            } else {
                                p.text_primary
                            })
                            .wrapping(text::Wrapping::None),
                        text(enc.description())
                            .size(type_scale::BADGE)
                            .color(p.text_muted)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(1),
                    Space::with_width(Length::Fill),
                ]
                .spacing(space::XS)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding([5, 8])
            .style(theme::context_menu_item(p, false))
            .on_press(msg),
        );
    }

    let mut items = column![
        container(
            text("File Encoding")
                .size(type_scale::CAPTION)
                .color(p.text_muted),
        )
        .padding([2, 6]),
        mode_switch,
    ]
    .spacing(5)
    .width(Length::Fill);

    if !is_save && editor_dirty {
        items = items.push(
            container(
                row![
                    icons::alert_triangle(12.0, p.warning_fg),
                    text("Discards unsaved edits")
                        .size(type_scale::BADGE)
                        .color(p.warning_fg),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding([4, 8])
            .width(Length::Fill)
            .style(theme::pill_box(p.warning_bg, p.warning_fg, p.border_subtle)),
        );
    }

    items = items
        .push(horizontal_rule(1).style(theme::subtle_rule(p)))
        .push(
            container(slim_scroll_overlay(list, p).height(Length::Shrink))
                .max_height(280.0)
                .width(Length::Fill),
        );

    opaque(
        container(items)
            .padding(6)
            .width(Length::Fixed(292.0))
            .style(theme::context_menu_box(p)),
    )
}

/// Reusable searchable anchored dropdown menu card for selecting Syntax Highlighting language.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_syntax_menu_card<'a>(
    p: Palette,
    theme_mode: ThemeMode,
    current_lang: CodeLanguage,
    auto_lang: Option<CodeLanguage>,
    filter: &'a str,
    filter_id: text_input::Id,
    on_filter: impl Fn(String) -> Message + 'a,
    on_select: impl Fn(CodeLanguage) -> Message + 'a,
    on_auto: Option<Message>,
) -> Element<'a, Message> {
    let q = filter.trim().to_lowercase();
    let mut header_row = row![
        text("Syntax Highlight")
            .size(type_scale::CAPTION)
            .color(p.text_muted)
            .wrapping(text::Wrapping::None),
        Space::with_width(Length::Fill),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    if let (Some(auto_l), Some(auto_msg)) = (auto_lang, on_auto) {
        header_row = header_row.push(
            button(
                text(format!("Auto ({})", auto_l.label()))
                    .size(type_scale::BADGE)
                    .color(p.accent_fg)
                    .wrapping(text::Wrapping::None),
            )
            .padding([2, 6])
            .style(theme::vscode_icon_btn(p, current_lang == auto_l))
            .on_press(auto_msg),
        );
    }

    let filter_input = text_input("Filter language…", filter)
        .id(filter_id)
        .on_input(on_filter)
        .padding([4, 8])
        .size(type_scale::CAPTION)
        .width(Length::Fill)
        .style(theme::dark_input(p));

    let mut list = column![].spacing(1).width(Length::Fill);
    let mut shown = 0usize;
    for &lang in CodeLanguage::all() {
        if !q.is_empty()
            && !lang.label().to_lowercase().contains(&q)
            && !lang.short_badge().to_lowercase().contains(&q)
        {
            continue;
        }
        shown += 1;
        let is_current = current_lang == lang;
        let is_auto = auto_lang == Some(lang);
        let badge_col = lang.badge_color(theme_mode);

        let mut item_row = row![
            if is_current {
                Element::from(icons::check(icon_size::XS, p.success_fg))
            } else {
                Space::with_width(Length::Fixed(icon_size::XS)).into()
            },
            container(
                text(lang.short_badge())
                    .size(10)
                    .font(theme::CODE_FONT)
                    .color(badge_col)
                    .wrapping(text::Wrapping::None),
            )
            .width(Length::Fixed(22.0)),
            text(lang.label())
                .size(type_scale::CAPTION)
                .color(if is_current {
                    p.accent_fg
                } else {
                    p.text_primary
                })
                .wrapping(text::Wrapping::None),
            Space::with_width(Length::Fill),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        if is_auto {
            item_row = item_row.push(
                text("auto")
                    .size(type_scale::BADGE)
                    .color(p.text_muted)
                    .wrapping(text::Wrapping::None),
            );
        }

        list = list.push(
            button(item_row)
                .width(Length::Fill)
                .padding([5, 8])
                .style(theme::context_menu_item(p, false))
                .on_press(on_select(lang)),
        );
    }

    if shown == 0 {
        list = list.push(
            container(
                text("No matching language")
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
            )
            .padding([8, 8]),
        );
    }

    let items = column![
        container(header_row).padding([2, 6]),
        filter_input,
        horizontal_rule(1).style(theme::subtle_rule(p)),
        container(slim_scroll_overlay(list, p).height(Length::Shrink))
            .max_height(310.0)
            .width(Length::Fill),
    ]
    .spacing(5)
    .width(Length::Fill);

    opaque(
        container(items)
            .padding(6)
            .width(Length::Fixed(262.0))
            .style(theme::context_menu_box(p)),
    )
}

impl GitFleetApp {
    fn effective_doc_menu_anchor(&self) -> Point {
        if self.doc_format_menu_pos.x > 1.0 || self.doc_format_menu_pos.y > 1.0 {
            self.doc_format_menu_pos
        } else {
            Point::new(
                (self.window_size.width - 320.0).max(24.0),
                (self.window_size.height - 44.0).max(40.0),
            )
        }
    }

    pub(crate) fn view_eol_modal(&self, p: Palette) -> Element<'_, Message> {
        let card = build_eol_menu_card(
            p,
            self.open_doc_line_ending,
            self.editor_dirty,
            Message::ChangeDocLineEnding,
        );
        theme::overlay_backdrop(
            theme::popup_at(self.effective_doc_menu_anchor(), card),
            Message::CloseOverlays,
        )
    }

    pub(crate) fn view_encoding_modal(&self, p: Palette) -> Element<'_, Message> {
        let card = build_encoding_menu_card(
            p,
            self.open_doc_encoding,
            self.encoding_modal_mode,
            self.editor_dirty,
            Message::SetEncodingModalMode,
            Message::SaveDocWithEncoding,
            Message::ReopenDocWithEncoding,
        );
        theme::overlay_backdrop(
            theme::popup_at(self.effective_doc_menu_anchor(), card),
            Message::CloseOverlays,
        )
    }

    pub(crate) fn view_syntax_modal(&self, p: Palette) -> Element<'_, Message> {
        let auto_lang = self
            .open_doc_path
            .as_deref()
            .or_else(|| self.active_file_path.as_deref().map(Path::new))
            .map(CodeLanguage::from_path);
        let card = build_syntax_menu_card(
            p,
            self.theme_mode,
            self.open_doc_language,
            auto_lang,
            &self.syntax_filter,
            self.syntax_filter_id.clone(),
            Message::SyntaxFilterChanged,
            Message::SetDocLanguage,
            auto_lang.map(Message::SetDocLanguage),
        );
        theme::overlay_backdrop(
            theme::popup_at(self.effective_doc_menu_anchor(), card),
            Message::CloseOverlays,
        )
    }
}
