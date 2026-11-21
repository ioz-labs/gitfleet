use crate::app::{GitFleetApp, Message, PendingConfirm};
use crate::icons;
use crate::theme::{self, Palette, icon_size, pad, space, type_scale};
use crate::views::{slim_scroll, tip, truncate_chars};
use gitfleet_security::{SecretFileFormat, VaultFileState};
use iced::widget::{Space, button, column, container, horizontal_rule, row, text, text_input, tooltip};
use iced::{Alignment, Color, Element, Length};
use std::collections::HashSet;

impl GitFleetApp {
    pub(crate) fn view_secrets_drawer(&self, p: Palette) -> Element<'_, Message> {
        let key_info = self.service.security.key_info();
        let key_ready = key_info.state == "ready";
        let tracked_paths: HashSet<&str> = self
            .vault_summary
            .files
            .iter()
            .map(|f| f.path.as_str())
            .collect();

        // 1. Unified full-bleed Dock Toolbar (matches Search / Ignore / Output cadence)
        let key_status: Element<'_, Message> = if key_ready {
            row![
                icons::key(icon_size::SM, p.success_fg),
                theme::badge("Age Ready", p.success_bg, p.success_fg, p.border_subtle, false),
                text(truncate_chars(
                    key_info.public_key.as_deref().unwrap_or(""),
                    20,
                ))
                .size(type_scale::CODE)
                .font(theme::CODE_FONT)
                .color(p.text_secondary),
            ]
            .spacing(space::XS)
            .align_y(Alignment::Center)
            .into()
        } else {
            row![
                icons::key(icon_size::SM, p.warning_fg),
                theme::badge(
                    "No Age Key",
                    p.warning_bg,
                    p.warning_fg,
                    p.border_subtle,
                    false
                ),
            ]
            .spacing(space::XS)
            .align_y(Alignment::Center)
            .into()
        };

        let key_actions: Element<'_, Message> = if key_ready {
            row![
                button(
                    row![
                        icons::copy(icon_size::XS, p.text_secondary),
                        text("PubKey").size(type_scale::BODY),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding(pad::BTN_SM)
                .style(theme::subtle_btn(p))
                .on_press(Message::CopyVaultPublicKey),
                tip(
                    button(
                        row![
                            icons::key(icon_size::XS, p.text_secondary),
                            text("Backup Key").size(type_scale::BODY),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                    )
                    .padding(pad::BTN_SM)
                    .style(theme::subtle_btn(p))
                    .on_press(Message::CopyVaultPrivateKey),
                    "Copy AGE-SECRET-KEY to clipboard so another machine cloning this repo can decrypt",
                    tooltip::Position::Top,
                    p,
                ),
                button(text("Import Key").size(type_scale::BODY))
                    .padding(pad::BTN_SM)
                    .style(theme::filter_pill_btn(p, self.show_import_key_bar))
                    .on_press(Message::ToggleImportVaultKey),
                button(
                    row![
                        icons::lock(icon_size::XS, Color::WHITE),
                        text("Encrypt All").size(type_scale::BODY),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding(pad::BTN_SM)
                .style(theme::primary_btn(p))
                .on_press(Message::EncryptRepoEnv),
                button(text("Restore All").size(type_scale::BODY))
                    .padding(pad::BTN_SM)
                    .style(theme::subtle_btn(p))
                    .on_press(Message::RestoreRepoEnv),
                button(text("Reset").size(type_scale::BODY).color(p.danger_fg))
                    .padding(pad::BTN_SM)
                    .style(theme::subtle_btn(p))
                    .on_press(Message::RequestConfirm(PendingConfirm::reset_vault_key())),
            ]
            .spacing(space::XS)
            .align_y(Alignment::Center)
            .into()
        } else {
            row![
                button(
                    row![
                        icons::key(icon_size::XS, Color::WHITE),
                        text("Create Age Key").size(type_scale::BODY),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding(pad::BTN_SM)
                .style(theme::primary_btn(p))
                .on_press(Message::CreateVaultKey),
                button(text("Import Age Key").size(type_scale::BODY))
                    .padding(pad::BTN_SM)
                    .style(theme::subtle_btn(p))
                    .on_press(Message::ToggleImportVaultKey),
            ]
            .spacing(space::XS)
            .align_y(Alignment::Center)
            .into()
        };

        let toolbar = theme::dock_toolbar(
            p,
            row![
                key_status,
                key_actions,
                Space::with_width(Length::Fill),
                button(
                    row![
                        icons::plus(
                            icon_size::XS,
                            if self.show_secret_picker {
                                p.accent_fg
                            } else {
                                p.text_secondary
                            }
                        ),
                        text(if self.show_secret_picker {
                            "Hide File Picker"
                        } else {
                            "Choose Files (.env / .toml / .yaml)"
                        })
                        .size(type_scale::BODY),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding(pad::BTN_SM)
                .style(theme::filter_pill_btn(p, self.show_secret_picker))
                .on_press(Message::ToggleSecretFilePicker),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        );

        // 2. Scrollable content area with generous Design System spacing
        let mut content = column![].spacing(space::MD).width(Length::Fill);

        if self.show_import_key_bar {
            let import_row = row![
                text_input(
                    "Paste AGE-SECRET-KEY-1... from another machine",
                    &self.import_key_input
                )
                .on_input(Message::ImportVaultKeyInputChanged)
                .on_submit(Message::ImportVaultKeySubmitted)
                .padding(pad::INPUT_COMPACT)
                .size(type_scale::BODY)
                .width(Length::Fill)
                .style(theme::dark_input(p)),
                button(text("Import Key").size(type_scale::BODY))
                    .padding(pad::BTN_SM)
                    .style(theme::primary_btn(p))
                    .on_press(Message::ImportVaultKeySubmitted),
                button(text("Cancel").size(type_scale::BODY))
                    .padding(pad::BTN_SM)
                    .style(theme::subtle_btn(p))
                    .on_press(Message::ToggleImportVaultKey),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center);

            content = content.push(theme::section_card(
                p,
                "Import Age Private Key",
                Some("Paste the Age secret key from the machine that encrypted this repository."),
                None,
                import_row,
            ));
        }

        if self.show_secret_picker {
            let add_bar = row![
                button(
                    row![
                        icons::folder_open(icon_size::XS, Color::WHITE),
                        text("Browse Repo File…").size(type_scale::BODY),
                    ]
                    .spacing(space::XS)
                    .align_y(Alignment::Center),
                )
                .padding(pad::BTN_SM)
                .style(theme::primary_btn(p))
                .on_press(Message::PickSecretFile),
                text_input(
                    "Or type repo-relative path (e.g. config/secrets.toml, deploy/prod.yaml, .env.local)",
                    &self.secret_add_path_input
                )
                .on_input(Message::SecretAddPathChanged)
                .on_submit(Message::AddSecretManifestFile(
                    self.secret_add_path_input.clone()
                ))
                .padding(pad::INPUT_COMPACT)
                .size(type_scale::BODY)
                .width(Length::Fill)
                .style(theme::dark_input(p)),
                button(text("Add File").size(type_scale::BODY))
                    .padding(pad::BTN_SM)
                    .style(theme::subtle_btn(p))
                    .on_press(Message::AddSecretManifestFile(
                        self.secret_add_path_input.clone()
                    )),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center);

            let untracked: Vec<_> = self
                .secret_candidates
                .iter()
                .filter(|c| !tracked_paths.contains(c.path.as_str()))
                .take(12)
                .collect();

            let mut picker_col = column![add_bar].spacing(space::SM);
            if untracked.is_empty() {
                picker_col = picker_col.push(
                    text("No additional untracked .env / .toml / .yaml files detected in this repo.")
                        .size(type_scale::CAPTION)
                        .color(p.text_muted),
                );
            } else {
                picker_col = picker_col.push(theme::field_label(p, "Detected in repository (click to track):"));
                let mut grid = column![].spacing(space::XS);
                for chunk in untracked.chunks(4) {
                    let mut chip_row = row![].spacing(space::XS).align_y(Alignment::Center);
                    for cand in chunk {
                        chip_row = chip_row.push(
                            button(
                                row![
                                    icons::plus(icon_size::XS, p.accent_fg),
                                    text(format!("[{}] {}", cand.format.label(), cand.path))
                                        .size(type_scale::CAPTION)
                                        .font(theme::CODE_FONT),
                                ]
                                .spacing(5)
                                .align_y(Alignment::Center),
                            )
                            .padding([4, 9])
                            .style(theme::subtle_btn(p))
                            .on_press(Message::AddSecretManifestFile(cand.path.clone())),
                        );
                    }
                    grid = grid.push(chip_row);
                }
                picker_col = picker_col.push(grid);
            }

            content = content.push(theme::section_card(
                p,
                "Track Secret Files (.env / .toml / .yaml)",
                Some("Tracked paths are saved to .gitfleet/secrets.json — commit it alongside .gitfleet/secrets.age so every clone inherits your selection."),
                None,
                picker_col,
            ));
        }

        if self.vault_summary.vault_locked {
            let err_msg = self
                .vault_summary
                .lock_error
                .as_deref()
                .unwrap_or("Import the Age private key from the machine that encrypted this repo.");
            content = content.push(
                container(
                    row![
                        icons::lock(icon_size::MD, p.warning_fg),
                        text(format!("Vault locked: {err_msg}"))
                            .size(type_scale::BODY)
                            .color(p.warning_fg),
                        Space::with_width(Length::Fill),
                        button(text("Import Key").size(type_scale::BODY))
                            .padding(pad::BTN_SM)
                            .style(theme::primary_btn(p))
                            .on_press(Message::ToggleImportVaultKey),
                    ]
                    .spacing(space::SM)
                    .align_y(Alignment::Center),
                )
                .padding([8, 12])
                .width(Length::Fill)
                .style(theme::pill_box(p.warning_bg, p.warning_fg, p.warning_fg)),
            );
        }

        // Quick Set key/value bar (comfortable 12px inputs matching main UI)
        let quick_set = container(
            row![
                text("Quick Set")
                    .size(type_scale::BODY)
                    .color(p.text_secondary),
                text_input(".env / secrets.toml / prod.yaml", &self.secret_file_input)
                    .on_input(Message::SecretFileInputChanged)
                    .padding(pad::INPUT_COMPACT)
                    .size(type_scale::BODY)
                    .width(Length::Fixed(190.0))
                    .style(theme::dark_input(p)),
                text_input("KEY (or section.key)", &self.secret_key_input)
                    .on_input(Message::SecretKeyInputChanged)
                    .padding(pad::INPUT_COMPACT)
                    .size(type_scale::BODY)
                    .width(Length::Fixed(190.0))
                    .style(theme::dark_input(p)),
                text_input("Secret value…", &self.secret_val_input)
                    .on_input(Message::SecretValInputChanged)
                    .on_submit(Message::SetSecretSubmitted)
                    .padding(pad::INPUT_COMPACT)
                    .size(type_scale::BODY)
                    .width(Length::Fill)
                    .style(theme::dark_input(p)),
                button(text("Set Secret").size(type_scale::BODY))
                    .padding(pad::BTN_SM)
                    .style(theme::primary_btn(p))
                    .on_press(Message::SetSecretSubmitted),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        )
        .padding([8, 12])
        .width(Length::Fill)
        .style(theme::section_panel_box(p));
        content = content.push(quick_set);

        if self.vault_summary.files.is_empty() {
            let cta = button(
                row![
                    icons::plus(icon_size::XS, Color::WHITE),
                    text("Choose Files (.env / .toml / .yaml)").size(type_scale::BODY),
                ]
                .spacing(space::XS)
                .align_y(Alignment::Center),
            )
            .padding(pad::BTN_MD)
            .style(theme::primary_btn(p))
            .on_press(Message::ToggleSecretFilePicker);

            content = content.push(
                container(theme::empty_state(
                    p,
                    icons::shield(icon_size::LG, p.accent_fg),
                    "No secret files tracked in this repository",
                    "Pick .env, .toml, or .yaml files to encrypt with Age. Selection is stored in .gitfleet/secrets.json for cross-machine clones.",
                    Some(cta.into()),
                ))
                .width(Length::Fill)
                .height(Length::Fixed(150.0)),
            );
        } else {
            for f in &self.vault_summary.files {
                let (fmt_bg, fmt_fg) = match f.format {
                    SecretFileFormat::Env => (p.fill_selected, p.accent_fg),
                    SecretFileFormat::Toml => (p.success_bg, p.success_fg),
                    SecretFileFormat::Yaml => (p.warning_bg, p.violet_fg),
                };
                let (st_bg, st_fg) = match f.state {
                    VaultFileState::Synced => (p.success_bg, p.success_fg),
                    VaultFileState::Modified | VaultFileState::MissingOnDisk => {
                        (p.warning_bg, p.warning_fg)
                    }
                    VaultFileState::NotEncrypted => (p.fill_selected, p.accent_fg),
                    VaultFileState::Locked | VaultFileState::MissingBoth => {
                        (p.danger_bg, p.danger_fg)
                    }
                };

                let mut file_hdr = row![
                    theme::badge(f.format.label(), fmt_bg, fmt_fg, p.border_subtle, true),
                    text(f.path.clone())
                        .size(type_scale::SUBHEADING)
                        .font(theme::CODE_FONT)
                        .color(p.text_primary),
                    theme::badge(f.state.label(), st_bg, st_fg, p.border_subtle, false),
                    text(format!("{} key(s)", f.keys.len()))
                        .size(type_scale::CAPTION)
                        .color(p.text_muted),
                    Space::with_width(Length::Fill),
                ]
                .spacing(space::SM)
                .align_y(Alignment::Center);

                if matches!(
                    f.state,
                    VaultFileState::NotEncrypted | VaultFileState::Modified
                ) && key_ready
                {
                    file_hdr = file_hdr.push(
                        button(text("Encrypt").size(type_scale::CAPTION))
                            .padding([4, 10])
                            .style(theme::primary_btn(p))
                            .on_press(Message::EncryptSingleSecretFile(f.path.clone())),
                    );
                }
                if matches!(
                    f.state,
                    VaultFileState::MissingOnDisk | VaultFileState::Modified
                ) && f.in_vault
                    && !self.vault_summary.vault_locked
                {
                    file_hdr = file_hdr.push(
                        button(text("Restore").size(type_scale::CAPTION))
                            .padding([4, 10])
                            .style(theme::subtle_btn(p))
                            .on_press(Message::RestoreSingleSecretFile(f.path.clone())),
                    );
                }
                file_hdr = file_hdr
                    .push(
                        button(text("Use in Quick Set").size(type_scale::CAPTION))
                            .padding([4, 10])
                            .style(theme::subtle_btn(p))
                            .on_press(Message::SelectSecretTargetFile(f.path.clone())),
                    )
                    .push(
                        tip(
                            button(icons::trash(icon_size::XS, p.danger_fg))
                                .padding(pad::BTN_ICON)
                                .style(theme::subtle_btn(p))
                                .on_press(Message::RemoveSecretManifestFile(f.path.clone())),
                            "Stop tracking this file in .gitfleet/secrets.json",
                            tooltip::Position::Top,
                            p,
                        ),
                    );

                let mut file_col = column![file_hdr].spacing(space::XS);
                if f.in_vault && !self.vault_summary.vault_locked && !f.keys.is_empty() {
                    file_col = file_col.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                    for key in &f.keys {
                        let revealed = self
                            .revealed_secrets
                            .get(&(f.path.clone(), key.clone()))
                            .cloned();
                        file_col = file_col.push(
                            row![
                                Space::with_width(Length::Fixed(4.0)),
                                text(key.clone())
                                    .size(type_scale::CODE)
                                    .font(theme::CODE_FONT)
                                    .color(p.accent_fg),
                                text("=")
                                    .size(type_scale::CODE)
                                    .font(theme::CODE_FONT)
                                    .color(p.text_muted),
                                text(revealed.unwrap_or_else(|| "••••••••••••".into()))
                                    .size(type_scale::CODE)
                                    .font(theme::CODE_FONT)
                                    .color(p.success_fg),
                                Space::with_width(Length::Fill),
                                button(
                                    row![
                                        icons::eye(icon_size::XS, p.text_secondary),
                                        text("Reveal").size(type_scale::CAPTION),
                                    ]
                                    .spacing(5)
                                    .align_y(Alignment::Center),
                                )
                                .padding([4, 9])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::RevealSecret(f.path.clone(), key.clone())),
                                button(
                                    row![
                                        icons::copy(icon_size::XS, Color::WHITE),
                                        text("Copy").size(type_scale::CAPTION),
                                    ]
                                    .spacing(5)
                                    .align_y(Alignment::Center),
                                )
                                .padding([4, 9])
                                .style(theme::primary_btn(p))
                                .on_press(Message::CopySecret(f.path.clone(), key.clone())),
                            ]
                            .spacing(space::SM)
                            .align_y(Alignment::Center),
                        );
                    }
                }
                content = content.push(
                    container(file_col)
                        .padding(pad::CARD)
                        .width(Length::Fill)
                        .style(theme::section_panel_box(p)),
                );
            }
        }

        let body = container(slim_scroll(content, p).height(Length::Fill))
            .padding(pad::DOCK_BODY)
            .width(Length::Fill)
            .height(Length::Fill);

        column![toolbar, body]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
