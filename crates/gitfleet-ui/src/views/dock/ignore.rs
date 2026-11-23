//! `D2` — 2-Column Ignore Studio: 3-Layer Editor + Snippet Templates + Live `git check-ignore` Rule Tester.

use crate::app::{GitFleetApp, Message};
use crate::icons;
use crate::theme::kit::{
    ButtonSize, ButtonVariant, SegmentItem, UiButton, segmented_control, status_pill,
};
use crate::theme::{self, Palette, icon_size, pad, space, type_scale};
use crate::views::slim_scroll;
use gitfleet_security::IgnoreLayer;
use iced::widget::{
    Space, button, column, container, horizontal_rule, row, text, text_editor, text_input,
    vertical_rule,
};
use iced::{Alignment, Element, Length};

impl GitFleetApp {
    pub(crate) fn view_ignore_drawer(&self, p: Palette) -> Element<'_, Message> {
        let mut snippets = row![].spacing(4).align_y(Alignment::Center);
        for (lbl, snip) in [
            ("+ Rust", "\n# Rust\n/target/\n**/*.rs.bk\n"),
            ("+ Node", "\n# Node\nnode_modules/\ndist/\n.next/\n"),
            ("+ Python", "\n# Python\n__pycache__/\n.venv/\n*.pyc\n"),
            ("+ Env", "\n# Local secrets\n.env\n.env.*\n!.env.example\n"),
            ("+ OS/IDE", "\n# OS & IDE\n.DS_Store\nThumbs.db\n.idea/\n"),
        ] {
            snippets = snippets.push(
                button(
                    text(lbl)
                        .size(type_scale::BADGE)
                        .font(theme::CODE_FONT)
                        .color(p.text_secondary),
                )
                .padding([2, 6])
                .style(theme::subtle_btn(p))
                .on_press(Message::AppendIgnoreSnippet(snip)),
            );
        }

        let toolbar = theme::dock_toolbar(
            p,
            row![
                icons::layers(icon_size::SM, p.accent_fg),
                segmented_control(
                    p,
                    [
                        SegmentItem::new(IgnoreLayer::RootGitignore, ".gitignore"),
                        SegmentItem::new(IgnoreLayer::PersonalExclude, ".git/info/exclude"),
                        SegmentItem::new(IgnoreLayer::GlobalExclude, "Global"),
                    ],
                    self.ignore_layer,
                    Message::SelectIgnoreLayer,
                ),
                snippets,
                Space::with_width(Length::Fill),
                UiButton::new(p, "Save Layer")
                    .variant(ButtonVariant::Primary)
                    .size(ButtonSize::Sm)
                    .icon(icons::save(
                        icon_size::XS,
                        theme::on_accent_ink(p.accent_primary)
                    ))
                    .on_press(Message::SaveIgnoreLayer),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        );

        let editor_pane = container(
            text_editor(&self.ignore_editor)
                .on_action(Message::IgnoreEditorAction)
                .font(theme::CODE_FONT)
                .size(type_scale::CODE)
                .height(Length::Fill)
                .style(theme::dark_editor(p)),
        )
        .padding([8, 10])
        .width(Length::FillPortion(58))
        .height(Length::Fill);

        // Right Column: Live `git check-ignore` Rule Tester + Currently Ignored Paths Preview
        let verdict: Element<'_, Message> = if self.ignore_test_path.trim().is_empty() {
            text("Enter any repo-relative path above (and press Enter or Save Layer) to verify which rule matches.")
                .size(type_scale::CAPTION)
                .color(p.text_muted)
                .into()
        } else {
            match &self.ignore_test_result {
                Some(Some(rule)) => row![
                    status_pill("IGNORED", p.warning_fg, p.warning_bg),
                    text(rule.clone())
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(p.text_primary),
                ]
                .spacing(space::XS)
                .align_y(Alignment::Center)
                .into(),
                Some(None) => row![
                    status_pill("NOT IGNORED", p.success_fg, p.success_bg),
                    text("Git will track this path normally.")
                        .size(type_scale::CAPTION)
                        .color(p.text_secondary),
                ]
                .spacing(space::XS)
                .align_y(Alignment::Center)
                .into(),
                None => text("Press Enter to run git check-ignore…")
                    .size(type_scale::CAPTION)
                    .color(p.text_muted)
                    .into(),
            }
        };

        let mut matched_list = column![].spacing(2);
        for d in self.ignored_repo_dirs.iter().take(40) {
            matched_list = matched_list.push(
                row![
                    icons::folder(11.0, p.text_muted),
                    text(format!("{d}/"))
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(p.text_secondary),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            );
        }
        for f in self.ignored_repo_files.iter().take(60) {
            matched_list = matched_list.push(
                row![
                    icons::file_code(11.0, p.text_muted),
                    text(f.clone())
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(p.text_secondary),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            );
        }
        if self.ignored_repo_dirs.is_empty() && self.ignored_repo_files.is_empty() {
            matched_list = matched_list.push(
                text("No gitignored files or folders detected on disk in this repository.")
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
            );
        }

        let tester_pane = container(
            column![
                text("Live Rule Tester (git check-ignore -v)")
                    .size(type_scale::CAPTION)
                    .font(theme::UI_FONT_MEDIUM)
                    .color(p.text_secondary),
                row![
                    text_input(
                        "Test path e.g. target/app.exe or .env.local…",
                        &self.ignore_test_path
                    )
                    .on_input(Message::IgnoreTestPathChanged)
                    .on_submit(Message::RunIgnorePathCheck)
                    .padding(pad::INPUT_COMPACT)
                    .size(type_scale::CAPTION)
                    .font(theme::CODE_FONT)
                    .width(Length::Fill)
                    .style(theme::dark_input(p)),
                    UiButton::new(p, "Test")
                        .variant(ButtonVariant::Secondary)
                        .size(ButtonSize::Xs)
                        .on_press(Message::RunIgnorePathCheck),
                ]
                .spacing(space::XS)
                .align_y(Alignment::Center),
                verdict,
                horizontal_rule(1).style(theme::subtle_rule(p)),
                row![
                    text("Currently Ignored in Repo")
                        .size(type_scale::BADGE)
                        .font(theme::UI_FONT_MEDIUM)
                        .color(p.text_muted),
                    Space::with_width(Length::Fill),
                    status_pill(
                        format!(
                            "{} dirs · {} files",
                            self.ignored_repo_dirs.len(),
                            self.ignored_repo_files.len()
                        ),
                        p.text_muted,
                        p.surface_card,
                    ),
                ]
                .align_y(Alignment::Center),
                slim_scroll(matched_list, p).height(Length::Fill),
            ]
            .spacing(space::XS)
            .height(Length::Fill),
        )
        .padding([8, 12])
        .width(Length::FillPortion(42))
        .height(Length::Fill);

        column![
            toolbar,
            row![
                editor_pane,
                vertical_rule(1).style(theme::subtle_rule(p)),
                tester_pane,
            ]
            .width(Length::Fill)
            .height(Length::Fill),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
