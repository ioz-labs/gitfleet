use crate::app::{BottomViewMode, GitFleetApp, Message, PendingConfirm};
use crate::icons;
use crate::theme::{self, Palette, icon_size, pad, space, type_scale};
use crate::views::{slim_scroll, tip, truncate_chars};
use gitfleet_security::{SecretFileFormat, VaultFileState, VaultFileSummary};
use iced::widget::{
    Space, button, checkbox, column, container, horizontal_rule, row, text, text_input, tooltip,
    vertical_rule,
};
use iced::{Alignment, Background, Border, Color, Element, Length};
use std::collections::HashSet;

fn format_badge_colors(fmt: SecretFileFormat, p: Palette) -> (Color, Color) {
    match fmt {
        SecretFileFormat::Env => (p.fill_selected, p.accent_fg),
        SecretFileFormat::Toml => (p.success_bg, p.success_fg),
        SecretFileFormat::Yaml => (p.warning_bg, p.violet_fg),
    }
}

fn state_colors(state: VaultFileState, p: Palette) -> (Color, Color, Color) {
    match state {
        VaultFileState::Synced => (p.success_bg, p.success_fg, p.success_fg),
        VaultFileState::Modified => (p.warning_bg, p.warning_fg, p.warning_fg),
        VaultFileState::MissingOnDisk => (p.fill_selected, p.accent_fg, p.accent_fg),
        VaultFileState::NotEncrypted => (p.surface_card, p.text_secondary, p.accent_fg),
        VaultFileState::Locked | VaultFileState::MissingBoth => {
            (p.danger_bg, p.danger_fg, p.danger_fg)
        }
    }
}

impl GitFleetApp {
    /// Redesigned 2-column Secrets Vault Workbench:
    /// - Left Rail (`surface_sidebar`): Profiles, Tracked Files, and Age Key footer.
    /// - Right Workbench (`surface_panel`): Focused single-view inspector (Active File Key-Value
    ///   Table, Track Files Picker, or Age Keyring & Vault Settings) — zero stacked card soup,
    ///   zero duplicated controls.
    pub(crate) fn view_secrets_drawer(&self, p: Palette) -> Element<'_, Message> {
        if self.selected_repo.is_none() {
            return container(theme::empty_state(
                p,
                icons::shield(icon_size::LG, p.text_muted),
                "No repository selected",
                "Select a repository from the sidebar to manage its encrypted Secrets Vault.",
                None,
            ))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_y(Length::Fill)
            .style(move |_| container::Style {
                background: Some(Background::Color(p.surface_panel)),
                text_color: Some(p.text_primary),
                ..Default::default()
            })
            .into();
        }

        let keyring = self.service.security.keyring_summary();
        let key_ready = !keyring.keys.is_empty();
        let effective_key = self.resolve_active_repo_effective_key();
        let ws = self.service.workspace.current();
        let parent_repo = self
            .selected_repo
            .as_ref()
            .and_then(|id| ws.repos.iter().find(|r| &r.id == id));

        let show_keys_panel =
            self.show_key_manager || !key_ready || self.show_import_key_bar;
        let show_picker = !show_keys_panel && self.show_secret_picker;

        let rekey_target = if self.vault_summary.has_vault && !self.vault_summary.vault_locked {
            match (
                effective_key.as_ref(),
                self.vault_summary.unlocked_by_key_id.as_deref(),
            ) {
                (Some((eff, _)), Some(unlocked_id)) if eff.id != unlocked_id => Some((
                    eff.id.clone(),
                    eff.name.clone(),
                    self.vault_summary
                        .unlocked_by_key_name
                        .clone()
                        .unwrap_or_else(|| "previous key".into()),
                )),
                _ => None,
            }
        } else {
            None
        };

        let selected_file: Option<&VaultFileSummary> = self
            .vault_summary
            .files
            .iter()
            .find(|f| f.path == self.secret_file_input)
            .or_else(|| self.vault_summary.files.first());

        let left_rail = self.view_secrets_left_rail(
            p,
            &keyring,
            key_ready,
            effective_key.as_ref(),
            rekey_target.is_some(),
            show_keys_panel,
            show_picker,
            selected_file.map(|f| f.path.as_str()),
        );

        let right_workbench: Element<'_, Message> = if show_keys_panel {
            self.view_secrets_keys_workbench(
                p,
                &keyring,
                key_ready,
                effective_key.as_ref(),
                rekey_target.as_ref(),
                parent_repo,
                &ws,
            )
        } else if show_picker {
            self.view_secrets_picker_workbench(p)
        } else if self.vault_summary.vault_locked {
            self.view_secrets_locked_workbench(p)
        } else if let Some(file) = selected_file {
            self.view_secrets_file_inspector(p, file, key_ready)
        } else {
            self.view_secrets_empty_workbench(p)
        };

        row![
            container(left_rail)
                .width(Length::Fixed(248.0))
                .height(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(Background::Color(p.surface_sidebar)),
                    text_color: Some(p.text_primary),
                    ..Default::default()
                }),
            vertical_rule(1).style(theme::subtle_rule(p)),
            container(right_workbench)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(Background::Color(p.surface_panel)),
                    text_color: Some(p.text_primary),
                    ..Default::default()
                }),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    #[allow(clippy::too_many_arguments)]
    fn view_secrets_left_rail<'a>(
        &'a self,
        p: Palette,
        keyring: &gitfleet_security::VaultKeyringSummary,
        key_ready: bool,
        effective_key: Option<&(gitfleet_security::VaultKeyEntry, String)>,
        needs_rekey: bool,
        show_keys_panel: bool,
        show_picker: bool,
        selected_file_path: Option<&str>,
    ) -> Element<'a, Message> {
        let active_profile = self.vault_summary.active_profile.as_str();
        let active_vault_file = self.vault_summary.vault_file_name.as_str();
        let encrypt_needed = self.vault_summary.encrypt_needed_count();
        let restore_needed = self.vault_summary.restore_needed_count();

        // ── Section 1: Profiles ────────────────────────────────────────────────
        let profiles_hdr = row![
            text("Profiles")
                .size(type_scale::CAPTION)
                .color(p.text_muted)
                .wrapping(text::Wrapping::None),
            Space::with_width(Length::Fill),
            tip(
                button(icons::plus(
                    12.0,
                    if self.show_create_profile_bar {
                        p.accent_fg
                    } else {
                        p.text_secondary
                    },
                ))
                .padding([3, 5])
                .style(theme::vscode_icon_btn(p, self.show_create_profile_bar))
                .on_press(Message::ToggleCreateVaultProfile),
                "New profile (e.g. laptop, work-pc, staging)",
                tooltip::Position::Right,
                p,
            ),
        ]
        .spacing(4)
        .padding([0, 4])
        .align_y(Alignment::Center);

        let mut profiles_list = column![].spacing(2).width(Length::Fill);
        for prof in &self.vault_summary.profiles {
            let is_active = prof.name == active_profile;
            let is_custom = prof.name != gitfleet_security::DEFAULT_PROFILE;
            let dot_color = if is_active {
                if self.vault_summary.vault_locked {
                    p.warning_fg
                } else if prof.has_vault {
                    p.success_fg
                } else {
                    p.accent_fg
                }
            } else if prof.has_vault {
                p.text_secondary
            } else {
                p.text_muted
            };
            let prof_file = gitfleet_security::vault_file_name_for_profile(&prof.name);
            let prof_hint = if is_active {
                format!("Active on this machine · .gitfleet/{prof_file}")
            } else if prof.has_vault {
                format!("Switch to '{}' · .gitfleet/{prof_file}", prof.name)
            } else {
                format!("Switch to '{}' (not encrypted yet)", prof.name)
            };

            let mut row_line = row![
                tip(
                    button(
                        row![
                            container(Space::new(6, 6)).style(theme::dot_box(dot_color)),
                            theme::clipped_line(
                                text(prof.name.clone())
                                    .size(type_scale::BODY)
                                    .color(if is_active {
                                        p.text_primary
                                    } else {
                                        p.text_secondary
                                    })
                                    .wrapping(text::Wrapping::None),
                                Length::Fill,
                            ),
                        ]
                        .spacing(7)
                        .align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .padding([5, 8])
                    .style(theme::repo_row_btn(p, is_active))
                    .on_press(Message::SelectVaultProfile(prof.name.clone())),
                    prof_hint,
                    tooltip::Position::Right,
                    p,
                ),
            ]
            .spacing(2)
            .width(Length::Fill)
            .align_y(Alignment::Center);

            if is_active && is_custom && self.renaming_profile.is_none() {
                row_line = row_line
                    .push(tip(
                        button(icons::pencil(11.0, p.text_secondary))
                            .padding([4, 5])
                            .style(theme::vscode_icon_btn(p, false))
                            .on_press(Message::StartRenameVaultProfile(prof.name.clone())),
                        format!("Rename profile '{}'", prof.name),
                        tooltip::Position::Top,
                        p,
                    ))
                    .push(tip(
                        button(icons::trash(11.0, p.danger_fg))
                            .padding([4, 5])
                            .style(theme::vscode_icon_btn(p, false))
                            .on_press(Message::RequestConfirm(
                                PendingConfirm::delete_vault_profile(&prof.name),
                            )),
                        format!("Delete profile '{}' (.gitfleet/{prof_file})", prof.name),
                        tooltip::Position::Top,
                        p,
                    ));
            }
            profiles_list = profiles_list.push(row_line);
        }

        if self.show_create_profile_bar {
            let mut create_box = column![
                text_input("laptop, work, staging…", &self.new_profile_name_input)
                    .on_input(Message::NewVaultProfileNameChanged)
                    .on_submit(Message::CreateVaultProfileSubmitted)
                    .padding([4, 7])
                    .size(type_scale::CAPTION)
                    .width(Length::Fill)
                    .style(theme::dark_input(p)),
            ]
            .spacing(5)
            .width(Length::Fill);

            if self.vault_summary.has_vault && !self.vault_summary.vault_locked {
                create_box = create_box.push(tip(
                    checkbox(
                        format!("Copy from '{active_profile}'"),
                        self.clone_from_active_profile,
                    )
                    .on_toggle(Message::ToggleCloneVaultProfile)
                    .size(13)
                    .spacing(5)
                    .text_size(type_scale::CAPTION)
                    .style(theme::tree_checkbox(p)),
                    "Clone keys from the active profile so you only tweak per-machine differences (e.g. API_HOST)",
                    tooltip::Position::Right,
                    p,
                ));
            }

            create_box = create_box.push(
                row![
                    button(
                        container(text("Create").size(type_scale::CAPTION))
                            .width(Length::Fill)
                            .center_x(Length::Fill),
                    )
                    .width(Length::FillPortion(55))
                    .padding([4, 6])
                    .style(theme::primary_btn(p))
                    .on_press(Message::CreateVaultProfileSubmitted),
                    button(
                        container(text("Cancel").size(type_scale::CAPTION))
                            .width(Length::Fill)
                            .center_x(Length::Fill),
                    )
                    .width(Length::FillPortion(45))
                    .padding([4, 6])
                    .style(theme::subtle_btn(p))
                    .on_press(Message::ToggleCreateVaultProfile),
                ]
                .spacing(4)
                .width(Length::Fill),
            );

            profiles_list = profiles_list.push(
                container(create_box)
                    .padding(7)
                    .width(Length::Fill)
                    .style(theme::card_box(p)),
            );
        } else if let Some((_, draft)) = &self.renaming_profile {
            let rename_box = column![
                text_input("New profile name…", draft)
                    .on_input(Message::RenameVaultProfileInputChanged)
                    .on_submit(Message::SubmitRenameVaultProfile)
                    .padding([4, 7])
                    .size(type_scale::CAPTION)
                    .width(Length::Fill)
                    .style(theme::dark_input(p)),
                row![
                    button(
                        container(text("Save").size(type_scale::CAPTION))
                            .width(Length::Fill)
                            .center_x(Length::Fill),
                    )
                    .width(Length::FillPortion(50))
                    .padding([4, 6])
                    .style(theme::primary_btn(p))
                    .on_press(Message::SubmitRenameVaultProfile),
                    button(
                        container(text("Cancel").size(type_scale::CAPTION))
                            .width(Length::Fill)
                            .center_x(Length::Fill),
                    )
                    .width(Length::FillPortion(50))
                    .padding([4, 6])
                    .style(theme::subtle_btn(p))
                    .on_press(Message::CancelRenameVaultProfile),
                ]
                .spacing(4)
                .width(Length::Fill),
            ]
            .spacing(5)
            .width(Length::Fill);

            profiles_list = profiles_list.push(
                container(rename_box)
                    .padding(7)
                    .width(Length::Fill)
                    .style(theme::card_box(p)),
            );
        }

        // ── Section 2: Tracked Files ───────────────────────────────────────────
        let mut files_hdr = row![
            text("Files")
                .size(type_scale::CAPTION)
                .color(p.text_muted)
                .wrapping(text::Wrapping::None),
            Space::with_width(Length::Fill),
        ]
        .spacing(4)
        .padding([0, 4])
        .align_y(Alignment::Center);

        // Only surface bulk Encrypt/Restore here when >1 files need action (for 1 file, the
        // right-pane header already has the direct primary CTA — zero duplicate controls).
        if key_ready && !self.vault_summary.vault_locked {
            if encrypt_needed > 1 {
                files_hdr = files_hdr.push(tip(
                    button(
                        text(format!("Encrypt {encrypt_needed}"))
                            .size(type_scale::BADGE)
                            .color(Color::WHITE)
                            .wrapping(text::Wrapping::None),
                    )
                    .padding([2, 6])
                    .style(theme::primary_btn(p))
                    .on_press(Message::EncryptRepoEnv),
                    format!(
                        "Encrypt all {encrypt_needed} modified files into profile '{active_profile}' (.gitfleet/{active_vault_file})"
                    ),
                    tooltip::Position::Right,
                    p,
                ));
            } else if restore_needed > 1 {
                files_hdr = files_hdr.push(tip(
                    button(
                        text(format!("Restore {restore_needed}"))
                            .size(type_scale::BADGE)
                            .color(p.accent_fg)
                            .wrapping(text::Wrapping::None),
                    )
                    .padding([2, 6])
                    .style(theme::filter_pill_btn(p, true))
                    .on_press(Message::RestoreRepoEnv),
                    format!(
                        "Restore all {restore_needed} missing files from profile '{active_profile}'"
                    ),
                    tooltip::Position::Right,
                    p,
                ));
            }
        }

        files_hdr = files_hdr.push(tip(
            button(icons::plus(
                12.0,
                if show_picker {
                    p.accent_fg
                } else {
                    p.text_secondary
                },
            ))
            .padding([3, 5])
            .style(theme::vscode_icon_btn(p, show_picker))
            .on_press(Message::ToggleSecretFilePicker),
            "Track .env / .toml / .yaml files in .gitfleet/secrets.json",
            tooltip::Position::Right,
            p,
        ));

        let mut files_list = column![].spacing(2).width(Length::Fill);
        if self.vault_summary.files.is_empty() {
            files_list = files_list.push(
                container(
                    text("No files tracked")
                        .size(type_scale::CAPTION)
                        .color(p.text_muted),
                )
                .padding([4, 8]),
            );
        } else {
            for f in &self.vault_summary.files {
                let is_sel = !show_keys_panel
                    && !show_picker
                    && selected_file_path == Some(f.path.as_str());
                let (_, _, dot_c) = state_colors(f.state, p);

                files_list = files_list.push(tip(
                    button(
                        row![
                            container(Space::new(6, 6)).style(theme::dot_box(dot_c)),
                            theme::clipped_line(
                                text(f.path.clone())
                                    .size(type_scale::CODE)
                                    .font(theme::CODE_FONT)
                                    .color(if is_sel {
                                        p.text_primary
                                    } else {
                                        p.text_secondary
                                    })
                                    .wrapping(text::Wrapping::None),
                                Length::Fill,
                            ),
                            text(f.keys.len().to_string())
                                .size(type_scale::BADGE)
                                .color(p.text_muted)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(7)
                        .align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .padding([5, 8])
                    .style(theme::repo_row_btn(p, is_sel))
                    .on_press(Message::SelectSecretTargetFile(f.path.clone())),
                    format!("{} · {} · {} key(s)", f.path, f.state.label(), f.keys.len()),
                    tooltip::Position::Right,
                    p,
                ));
            }
        }

        let rail_scroll = slim_scroll(
            column![
                profiles_hdr,
                profiles_list,
                Space::with_height(Length::Fixed(4.0)),
                horizontal_rule(1).style(theme::subtle_rule(p)),
                Space::with_height(Length::Fixed(2.0)),
                files_hdr,
                files_list,
            ]
            .spacing(space::XS)
            .width(Length::Fill),
            p,
        )
        .height(Length::Fill);

        // ── Pinned Footer: Age Key & Vault Settings trigger ────────────────────
        let (key_title, key_sub, key_icon_color) = if let Some((eff, src)) = effective_key {
            (
                eff.name.clone(),
                format!("{src} · {} key(s)", keyring.keys.len()),
                if needs_rekey {
                    p.warning_fg
                } else {
                    p.success_fg
                },
            )
        } else {
            (
                "Set up Age Key".to_string(),
                "Required to encrypt".to_string(),
                p.warning_fg,
            )
        };

        let mut key_footer_row = row![
            icons::key(icon_size::SM, key_icon_color),
            column![
                theme::clipped_line(
                    text(key_title)
                        .size(type_scale::CAPTION)
                        .color(p.text_primary)
                        .wrapping(text::Wrapping::None),
                    Length::Fill,
                ),
                theme::clipped_line(
                    text(key_sub)
                        .size(type_scale::BADGE)
                        .color(if needs_rekey || !key_ready {
                            p.warning_fg
                        } else {
                            p.text_muted
                        })
                        .wrapping(text::Wrapping::None),
                    Length::Fill,
                ),
            ]
            .spacing(1)
            .width(Length::Fill),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        if needs_rekey {
            key_footer_row = key_footer_row.push(
                container(Space::new(6, 6)).style(theme::dot_box(p.warning_fg)),
            );
        }
        key_footer_row = key_footer_row.push(icons::chevron_right(12.0, p.text_muted));

        let key_footer = tip(
            button(key_footer_row)
                .width(Length::Fill)
                .padding([7, 8])
                .style(theme::repo_row_btn(p, show_keys_panel))
                .on_press(Message::ToggleKeyManager),
            "Manage Age keys, repo/group key assignments, re-keying, and vault deletion",
            tooltip::Position::Right,
            p,
        );

        column![
            container(rail_scroll)
                .padding([8, 8])
                .width(Length::Fill)
                .height(Length::Fill),
            horizontal_rule(1).style(theme::subtle_rule(p)),
            container(key_footer).padding([6, 8]).width(Length::Fill),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn view_secrets_file_inspector<'a>(
        &'a self,
        p: Palette,
        file: &'a VaultFileSummary,
        key_ready: bool,
    ) -> Element<'a, Message> {
        let active_profile = self.vault_summary.active_profile.as_str();
        let active_vault_file = self.vault_summary.vault_file_name.as_str();
        let (fmt_bg, fmt_fg) = format_badge_colors(file.format, p);
        let (st_bg, st_fg, _) = state_colors(file.state, p);

        // ── 1. Crisp Single-Row File Header ────────────────────────────────────
        let mut hdr_left = row![
            theme::badge(file.format.label(), fmt_bg, fmt_fg, p.border_subtle, true),
            text(file.path.clone())
                .size(type_scale::SUBHEADING)
                .font(theme::CODE_FONT)
                .color(p.text_primary)
                .wrapping(text::Wrapping::None),
            theme::badge(file.state.label(), st_bg, st_fg, p.border_subtle, false),
        ]
        .spacing(space::XS)
        .align_y(Alignment::Center);

        if !self.vault_summary.is_default_profile() {
            hdr_left = hdr_left.push(tip(
                theme::badge(
                    active_profile.to_string(),
                    p.fill_selected,
                    p.accent_fg,
                    p.border_subtle,
                    false,
                ),
                format!("Stored in .gitfleet/{active_vault_file}"),
                tooltip::Position::Bottom,
                p,
            ));
        }

        let mut hdr_right = row![].spacing(space::XS).align_y(Alignment::Center);

        match file.state {
            VaultFileState::Modified => {
                if key_ready {
                    hdr_right = hdr_right.push(tip(
                        button(
                            row![
                                icons::lock(12.0, Color::WHITE),
                                text(format!("Save to '{active_profile}'"))
                                    .size(type_scale::CAPTION)
                                    .wrapping(text::Wrapping::None),
                            ]
                            .spacing(5)
                            .align_y(Alignment::Center),
                        )
                        .padding([4, 10])
                        .style(theme::primary_btn(p))
                        .on_press(Message::EncryptSingleSecretFile(file.path.clone())),
                        format!(
                            "Encrypt local '{}' into profile '{active_profile}' (.gitfleet/{active_vault_file})",
                            file.path
                        ),
                        tooltip::Position::Bottom,
                        p,
                    ));
                }
                if file.in_vault {
                    hdr_right = hdr_right.push(tip(
                        button(
                            text(format!("Apply '{active_profile}'"))
                                .size(type_scale::CAPTION)
                                .wrapping(text::Wrapping::None),
                        )
                        .padding([4, 10])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::RestoreSingleSecretFile(file.path.clone())),
                        format!(
                            "Write '{}' from profile '{active_profile}' to disk (overwrites local file)",
                            file.path
                        ),
                        tooltip::Position::Bottom,
                        p,
                    ));
                }
            }
            VaultFileState::MissingOnDisk => {
                if file.in_vault {
                    hdr_right = hdr_right.push(tip(
                        button(
                            row![
                                icons::pull(12.0, Color::WHITE),
                                text("Restore to disk")
                                    .size(type_scale::CAPTION)
                                    .wrapping(text::Wrapping::None),
                            ]
                            .spacing(5)
                            .align_y(Alignment::Center),
                        )
                        .padding([4, 10])
                        .style(theme::primary_btn(p))
                        .on_press(Message::RestoreSingleSecretFile(file.path.clone())),
                        format!(
                            "Decrypt '{}' from profile '{active_profile}' to working tree",
                            file.path
                        ),
                        tooltip::Position::Bottom,
                        p,
                    ));
                }
            }
            // `NotEncrypted` surfaces a single focal CTA in the body below — no duplicate header CTA.
            VaultFileState::NotEncrypted
            | VaultFileState::Synced
            | VaultFileState::Locked
            | VaultFileState::MissingBoth => {}
        }

        if !matches!(
            file.state,
            VaultFileState::MissingOnDisk | VaultFileState::MissingBoth
        ) {
            hdr_right = hdr_right.push(tip(
                button(icons::file_code(icon_size::XS, p.text_secondary))
                    .padding([5, 6])
                    .style(theme::vscode_icon_btn(p, false))
                    .on_press(Message::OpenRepoFileInMode(
                        file.path.clone(),
                        BottomViewMode::Edit,
                    )),
                format!("Open '{}' in Editor", file.path),
                tooltip::Position::Bottom,
                p,
            ));
        }

        hdr_right = hdr_right.push(tip(
            button(icons::trash(icon_size::XS, p.danger_fg))
                .padding([5, 6])
                .style(theme::vscode_icon_btn(p, false))
                .on_press(Message::RemoveSecretManifestFile(file.path.clone())),
            "Stop tracking this file in .gitfleet/secrets.json (keeps file on disk)",
            tooltip::Position::Bottom,
            p,
        ));

        let header = container(
            row![theme::clipped_line(hdr_left, Length::Fill), hdr_right]
                .spacing(space::SM)
                .width(Length::Fill)
                .align_y(Alignment::Center),
        )
        .padding([8, 14])
        .width(Length::Fill);

        // ── 2. Key-Value Table / Contextual Guidance Body ──────────────────────
        let body: Element<'a, Message> = if !file.in_vault {
            let (title, subtitle, cta): (String, String, Option<Element<'_, Message>>) =
                if file.state == VaultFileState::NotEncrypted {
                    let btn: Option<Element<'_, Message>> = key_ready.then(|| {
                        button(
                            row![
                                icons::lock(icon_size::XS, Color::WHITE),
                                text(format!("Encrypt '{}' to '{active_profile}'", file.path))
                                    .size(type_scale::BODY),
                            ]
                            .spacing(6)
                            .align_y(Alignment::Center),
                        )
                        .padding(pad::BTN_MD)
                        .style(theme::primary_btn(p))
                        .on_press(Message::EncryptSingleSecretFile(file.path.clone()))
                        .into()
                    });
                    (
                        format!("'{}' is not sealed in profile '{active_profile}' yet", file.path),
                        format!(
                            "Detected {} key(s) on disk. Encrypt to seal a snapshot into .gitfleet/{active_vault_file}.",
                            file.keys.len()
                        ),
                        btn,
                    )
                } else {
                    (
                        format!("'{}' not found", file.path),
                        format!(
                            "This file is tracked in .gitfleet/secrets.json, but does not exist on disk or in profile '{active_profile}' yet."
                        ),
                        None,
                    )
                };
            container(theme::empty_state(
                p,
                icons::shield(icon_size::LG, p.accent_fg),
                title,
                subtitle,
                cta,
            ))
            .padding(16)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_y(Length::Fill)
            .into()
        } else if file.keys.is_empty() {
            container(theme::empty_state(
                p,
                icons::file_code(icon_size::LG, p.text_muted),
                format!("No keys inside '{}'", file.path),
                "Add a key below or open the file in the editor to add entries.",
                None,
            ))
            .padding(16)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_y(Length::Fill)
            .into()
        } else {
            let key_q = self.secret_key_filter.trim().to_lowercase();
            let visible_keys: Vec<&String> = file
                .keys
                .iter()
                .filter(|k| key_q.is_empty() || k.to_lowercase().contains(&key_q))
                .collect();

            let filter_bar = container(
                row![
                    icons::search(icon_size::XS, p.text_muted),
                    text_input("Filter KEY names…", &self.secret_key_filter)
                        .on_input(Message::SecretKeyFilterChanged)
                        .padding([4, 8])
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .width(Length::Fixed(220.0))
                        .style(theme::dark_input(p)),
                    Space::with_width(Length::Fill),
                    text(format!("Showing {}/{} keys", visible_keys.len(), file.keys.len()))
                        .size(type_scale::BADGE)
                        .font(theme::CODE_FONT)
                        .color(p.text_muted),
                ]
                .spacing(space::XS)
                .align_y(Alignment::Center),
            )
            .padding([5, 14])
            .width(Length::Fill);

            let table_hdr = container(
                row![
                    container(
                        text("Key")
                            .size(type_scale::BADGE)
                            .color(p.text_muted)
                            .wrapping(text::Wrapping::None),
                    )
                    .width(Length::FillPortion(38)),
                    container(
                        text("Value")
                            .size(type_scale::BADGE)
                            .color(p.text_muted)
                            .wrapping(text::Wrapping::None),
                    )
                    .width(Length::FillPortion(62)),
                    container(
                        text("Actions")
                            .size(type_scale::BADGE)
                            .color(p.text_muted)
                            .wrapping(text::Wrapping::None),
                    )
                    .width(Length::Fixed(88.0))
                    .align_x(iced::alignment::Horizontal::Right),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding([5, 14])
            .width(Length::Fill)
            .style(move |_| container::Style {
                background: Some(Background::Color(p.surface_sidebar)),
                ..Default::default()
            });

            let mut rows_col = column![filter_bar].spacing(0).width(Length::Fill);
            let editing_key = self.secret_key_input.trim();
            for (idx, key) in visible_keys.into_iter().enumerate() {
                if idx > 0 {
                    rows_col = rows_col.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                }
                let revealed = self
                    .revealed_secrets
                    .get(&(file.path.clone(), key.clone()))
                    .cloned();
                let is_revealed = revealed.is_some();
                let is_editing = editing_key == key.as_str();

                let row_inner = row![
                    theme::clipped_line(
                        text(key.clone())
                            .size(type_scale::CODE)
                            .font(theme::CODE_FONT)
                            .color(p.accent_fg)
                            .wrapping(text::Wrapping::None),
                        Length::FillPortion(38),
                    ),
                    theme::clipped_line(
                        text(revealed.unwrap_or_else(|| "••••••••••••".into()))
                            .size(type_scale::CODE)
                            .font(theme::CODE_FONT)
                            .color(if is_revealed {
                                p.success_fg
                            } else {
                                p.text_muted
                            })
                            .wrapping(text::Wrapping::None),
                        Length::FillPortion(62),
                    ),
                    row![
                        tip(
                            button(icons::pencil(
                                12.0,
                                if is_editing {
                                    p.accent_fg
                                } else {
                                    p.text_secondary
                                },
                            ))
                            .padding([4, 6])
                            .style(theme::vscode_icon_btn(p, is_editing))
                            .on_press(Message::SelectSecretTargetKey(
                                file.path.clone(),
                                key.clone(),
                            )),
                            format!("Edit '{key}' in profile '{active_profile}'"),
                            tooltip::Position::Top,
                            p,
                        ),
                        tip(
                            button(if is_revealed {
                                icons::eye_off(12.0, p.accent_fg)
                            } else {
                                icons::eye(12.0, p.text_secondary)
                            })
                            .padding([4, 6])
                            .style(theme::vscode_icon_btn(p, is_revealed))
                            .on_press(Message::RevealSecret(file.path.clone(), key.clone())),
                            if is_revealed { "Hide value" } else { "Reveal value" },
                            tooltip::Position::Top,
                            p,
                        ),
                        tip(
                            button(icons::copy(12.0, p.text_secondary))
                                .padding([4, 6])
                                .style(theme::vscode_icon_btn(p, false))
                                .on_press(Message::CopySecret(file.path.clone(), key.clone())),
                            format!("Copy '{key}' to clipboard"),
                            tooltip::Position::Top,
                            p,
                        ),
                    ]
                    .spacing(2)
                    .width(Length::Fixed(88.0))
                    .align_y(Alignment::Center),
                ]
                .spacing(10)
                .align_y(Alignment::Center);

                rows_col = rows_col.push(
                    container(row_inner)
                        .padding([5, 14])
                        .width(Length::Fill)
                        .style(move |_| container::Style {
                            background: if is_editing {
                                Some(Background::Color(p.fill_selected))
                            } else {
                                None
                            },
                            ..Default::default()
                        }),
                );
            }

            column![
                table_hdr,
                horizontal_rule(1).style(theme::subtle_rule(p)),
                slim_scroll(rows_col, p).height(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        };

        // ── 3. Docked Single-Line Add / Edit Key Bar ───────────────────────────
        let mut root = column![
            header,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            body,
        ]
        .width(Length::Fill)
        .height(Length::Fill);

        if key_ready && !self.vault_summary.vault_locked {
            let trimmed_key = self.secret_key_input.trim();
            let is_updating_existing =
                !trimmed_key.is_empty() && file.keys.iter().any(|k| k == trimmed_key);
            let submit_label = if is_updating_existing {
                "Update"
            } else {
                "Add Key"
            };

            let mut footer_row = row![
                text_input("KEY (e.g. API_HOST)", &self.secret_key_input)
                    .on_input(Message::SecretKeyInputChanged)
                    .padding([5, 9])
                    .size(type_scale::CAPTION)
                    .width(Length::FillPortion(34))
                    .style(theme::dark_input(p)),
                text_input(
                    if is_updating_existing {
                        "New value…"
                    } else {
                        "Secret value…"
                    },
                    &self.secret_val_input
                )
                .on_input(Message::SecretValInputChanged)
                .on_submit(Message::SetSecretSubmitted)
                .padding([5, 9])
                .size(type_scale::CAPTION)
                .width(Length::FillPortion(66))
                .style(theme::dark_input(p)),
                tip(
                    button(
                        text(submit_label)
                            .size(type_scale::CAPTION)
                            .wrapping(text::Wrapping::None),
                    )
                    .padding([5, 11])
                    .style(theme::primary_btn(p))
                    .on_press(Message::SetSecretSubmitted),
                    format!(
                        "Save key into profile '{active_profile}' (.gitfleet/{active_vault_file})"
                    ),
                    tooltip::Position::Top,
                    p,
                ),
            ]
            .spacing(space::XS)
            .width(Length::Fill)
            .align_y(Alignment::Center);

            if !trimmed_key.is_empty() || !self.secret_val_input.is_empty() {
                footer_row = footer_row.push(tip(
                    button(icons::close_x(12.0, p.text_secondary))
                        .padding([5, 6])
                        .style(theme::vscode_icon_btn(p, false))
                        .on_press(Message::ClearSecretQuickEdit),
                    "Clear key input",
                    tooltip::Position::Top,
                    p,
                ));
            }

            root = root
                .push(horizontal_rule(1).style(theme::subtle_rule(p)))
                .push(
                    container(footer_row)
                        .padding([7, 14])
                        .width(Length::Fill)
                        .style(move |_| container::Style {
                            background: Some(Background::Color(p.surface_sidebar)),
                            ..Default::default()
                        }),
                );
        }

        root.into()
    }

    fn view_secrets_locked_workbench(&self, p: Palette) -> Element<'_, Message> {
        let active_profile = self.vault_summary.active_profile.as_str();
        let err_msg = self
            .vault_summary
            .lock_error
            .as_deref()
            .unwrap_or("Import the Age private key from the machine that encrypted this repository.");

        let cta = button(
            row![
                icons::key(icon_size::XS, Color::WHITE),
                text("Open Key Manager").size(type_scale::BODY),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding(pad::BTN_MD)
        .style(theme::primary_btn(p))
        .on_press(Message::OpenSecretsDrawer { show_keys: true });

        container(theme::empty_state(
            p,
            icons::lock(icon_size::LG, p.warning_fg),
            format!("Profile '{active_profile}' is locked"),
            err_msg,
            Some(cta.into()),
        ))
        .padding(20)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }

    fn view_secrets_empty_workbench(&self, p: Palette) -> Element<'_, Message> {
        let cta = button(
            row![
                icons::plus(icon_size::XS, Color::WHITE),
                text("Choose Secret Files…").size(type_scale::BODY),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding(pad::BTN_MD)
        .style(theme::primary_btn(p))
        .on_press(Message::ToggleSecretFilePicker);

        container(theme::empty_state(
            p,
            icons::shield(icon_size::LG, p.accent_fg),
            "No secret files tracked yet",
            "Track .env, .toml, or .yaml files to encrypt them with Age across machines and profiles.",
            Some(cta.into()),
        ))
        .padding(20)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }

    fn view_secrets_picker_workbench(&self, p: Palette) -> Element<'_, Message> {
        let tracked_paths: HashSet<&str> = self
            .vault_summary
            .files
            .iter()
            .map(|f| f.path.as_str())
            .collect();

        let header = container(
            row![
                column![
                    text("Track Secret Files")
                        .size(type_scale::SUBHEADING)
                        .color(p.text_primary),
                    text("Saved in .gitfleet/secrets.json and shared across all profiles")
                        .size(type_scale::CAPTION)
                        .color(p.text_muted),
                ]
                .spacing(1),
                Space::with_width(Length::Fill),
                button(text("Done").size(type_scale::CAPTION))
                    .padding([4, 10])
                    .style(theme::subtle_btn(p))
                    .on_press(Message::ToggleSecretFilePicker),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        )
        .padding([8, 14])
        .width(Length::Fill);

        let add_bar = row![
            button(
                row![
                    icons::folder_open(icon_size::XS, Color::WHITE),
                    text("Browse…").size(type_scale::CAPTION),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding([5, 10])
            .style(theme::primary_btn(p))
            .on_press(Message::PickSecretFile),
            text_input(
                "Repo-relative path (e.g. .env, config/secrets.toml, deploy/prod.yaml)",
                &self.secret_add_path_input
            )
            .on_input(Message::SecretAddPathChanged)
            .on_submit(Message::AddSecretManifestFile(
                self.secret_add_path_input.clone()
            ))
            .padding([5, 9])
            .size(type_scale::CAPTION)
            .width(Length::Fill)
            .style(theme::dark_input(p)),
            button(text("Track").size(type_scale::CAPTION))
                .padding([5, 10])
                .style(theme::subtle_btn(p))
                .on_press(Message::AddSecretManifestFile(
                    self.secret_add_path_input.clone()
                )),
        ]
        .spacing(space::XS)
        .align_y(Alignment::Center);

        let untracked: Vec<_> = self
            .secret_candidates
            .iter()
            .filter(|c| !tracked_paths.contains(c.path.as_str()))
            .take(24)
            .collect();

        let mut list_col = column![add_bar].spacing(space::SM).width(Length::Fill);
        if untracked.is_empty() {
            list_col = list_col.push(
                text("No additional untracked .env / .toml / .yaml files found in this repository.")
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
            );
        } else {
            list_col = list_col.push(theme::field_label(p, "Detected in repository"));
            let mut rows = column![].spacing(0).width(Length::Fill);
            for (idx, cand) in untracked.iter().enumerate() {
                if idx > 0 {
                    rows = rows.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                }
                let (bg, fg) = format_badge_colors(cand.format, p);
                rows = rows.push(
                    container(
                        row![
                            theme::badge(cand.format.label(), bg, fg, p.border_subtle, true),
                            theme::clipped_line(
                                text(cand.path.clone())
                                    .size(type_scale::CODE)
                                    .font(theme::CODE_FONT)
                                    .color(p.text_primary),
                                Length::Fill,
                            ),
                            button(
                                row![
                                    icons::plus(11.0, p.accent_fg),
                                    text("Track").size(type_scale::CAPTION).color(p.accent_fg),
                                ]
                                .spacing(4)
                                .align_y(Alignment::Center),
                            )
                            .padding([3, 9])
                            .style(theme::subtle_btn(p))
                            .on_press(Message::AddSecretManifestFile(cand.path.clone())),
                        ]
                        .spacing(space::SM)
                        .align_y(Alignment::Center),
                    )
                    .padding([6, 8])
                    .width(Length::Fill),
                );
            }
            list_col = list_col.push(
                container(rows)
                    .width(Length::Fill)
                    .style(move |_| container::Style {
                        background: Some(Background::Color(p.surface_sidebar)),
                        border: Border {
                            color: p.border_subtle,
                            width: 1.0,
                            radius: 6.0.into(),
                        },
                        ..Default::default()
                    }),
            );
        }

        column![
            header,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            container(slim_scroll(list_col, p).height(Length::Fill))
                .padding([10, 14])
                .width(Length::Fill)
                .height(Length::Fill),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    #[allow(clippy::too_many_arguments)]
    fn view_secrets_keys_workbench<'a>(
        &'a self,
        p: Palette,
        keyring: &gitfleet_security::VaultKeyringSummary,
        key_ready: bool,
        effective_key: Option<&(gitfleet_security::VaultKeyEntry, String)>,
        rekey_target: Option<&(String, String, String)>,
        parent_repo: Option<&gitfleet_workspace::RepoEntry>,
        ws: &gitfleet_workspace::WorkspaceConfig,
    ) -> Element<'a, Message> {
        let active_profile = self.vault_summary.active_profile.as_str();
        let active_vault_file = self.vault_summary.vault_file_name.as_str();
        let active_repo_name = self
            .current_repo()
            .map(|r| r.name)
            .unwrap_or_else(|| "repository".to_string());
        let has_repo_vault_artifacts = self.vault_summary.has_vault
            || self.vault_summary.has_manifest
            || self.vault_summary.profiles.iter().any(|pr| pr.has_vault);

        let mut hdr_row = row![
            column![
                text("Age Keyring & Vault Settings")
                    .size(type_scale::SUBHEADING)
                    .color(p.text_primary),
                text("Manage named keys, assign keys per repo or group, and auto-unlock vaults.")
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
            ]
            .spacing(1),
            Space::with_width(Length::Fill),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center);

        if key_ready {
            hdr_row = hdr_row.push(
                button(text("Done").size(type_scale::CAPTION))
                    .padding([4, 10])
                    .style(theme::subtle_btn(p))
                    .on_press(Message::ToggleKeyManager),
            );
        }

        let header = container(hdr_row).padding([8, 14]).width(Length::Fill);

        let mut body_col = column![].spacing(space::MD).width(Length::Fill);

        // A. Repo & Group Key Assignment + contextual Re-key alert (single source of truth)
        if key_ready && let Some(repo) = parent_repo {
            let mut assign_col = column![].spacing(space::XS).width(Length::Fill);

            let build_key_pills = |selected_key_id: Option<&str>,
                                   inherit_label: &str,
                                   on_select: &dyn Fn(Option<String>) -> Message|
             -> Element<'_, Message> {
                let mut all_pills: Vec<Element<'_, Message>> =
                    Vec::with_capacity(keyring.keys.len() + 1);
                all_pills.push(
                    button(text(inherit_label.to_string()).size(type_scale::CAPTION))
                        .padding([3, 9])
                        .style(theme::filter_pill_btn(p, selected_key_id.is_none()))
                        .on_press(on_select(None))
                        .into(),
                );
                for k in &keyring.keys {
                    let is_sel = selected_key_id == Some(k.id.as_str());
                    all_pills.push(
                        button(
                            row![
                                icons::key(
                                    11.0,
                                    if is_sel { p.accent_fg } else { p.text_muted }
                                ),
                                text(truncate_chars(&k.name, 20)).size(type_scale::CAPTION),
                            ]
                            .spacing(4)
                            .align_y(Alignment::Center),
                        )
                        .padding([3, 9])
                        .style(theme::filter_pill_btn(p, is_sel))
                        .on_press(on_select(Some(k.id.clone())))
                        .into(),
                    );
                }

                let mut rows_col = column![].spacing(space::XS);
                let mut current_row = row![].spacing(space::XS).align_y(Alignment::Center);
                let mut in_row = 0usize;
                for pill in all_pills {
                    current_row = current_row.push(pill);
                    in_row += 1;
                    if in_row >= 6 {
                        rows_col = rows_col.push(current_row);
                        current_row = row![].spacing(space::XS).align_y(Alignment::Center);
                        in_row = 0;
                    }
                }
                if in_row > 0 {
                    rows_col = rows_col.push(current_row);
                }
                rows_col.into()
            };

            let repo_id_for_cb = repo.id.clone();
            let repo_inherit_lbl = if let Some(g) = repo.group.as_deref().filter(|g| !g.is_empty())
            {
                if let Some((inherited_id, src_group)) =
                    gitfleet_workspace::resolve_group_secret_key(&ws.group_secret_keys, Some(g))
                {
                    let inherited_name = keyring
                        .find(&inherited_id)
                        .map(|k| k.name.as_str())
                        .unwrap_or("key");
                    format!("Inherit ({src_group}: {inherited_name})")
                } else {
                    "Inherit (Default)".to_string()
                }
            } else {
                "Inherit (Default)".to_string()
            };

            assign_col = assign_col.push(
                row![
                    container(
                        text(format!("Repo '{}'", truncate_chars(&repo.name, 18)))
                            .size(type_scale::CAPTION)
                            .color(p.text_secondary),
                    )
                    .width(Length::Fixed(148.0)),
                    build_key_pills(
                        repo.secret_key_id.as_deref(),
                        &repo_inherit_lbl,
                        &move |kid| Message::SetRepoSecretKey(repo_id_for_cb.clone(), kid),
                    ),
                ]
                .spacing(space::SM)
                .align_y(Alignment::Center),
            );

            if let Some(group_path) = repo.group.as_deref().filter(|g| !g.trim().is_empty()) {
                let group_owned = group_path.to_string();
                let explicit_group_key =
                    ws.group_secret_keys.get(group_path).map(String::as_str);
                let group_inherit_lbl = gitfleet_workspace::parent_group_path(group_path)
                    .and_then(|parent| {
                        gitfleet_workspace::resolve_group_secret_key(
                            &ws.group_secret_keys,
                            Some(&parent),
                        )
                    })
                    .and_then(|(kid, src_grp)| {
                        keyring
                            .find(&kid)
                            .map(|k| format!("Inherit ({src_grp}: {})", k.name))
                    })
                    .unwrap_or_else(|| "Inherit (Default)".to_string());
                assign_col = assign_col.push(
                    row![
                        container(
                            text(format!("Group '{}'", truncate_chars(group_path, 18)))
                                .size(type_scale::CAPTION)
                                .color(p.text_secondary),
                        )
                        .width(Length::Fixed(148.0)),
                        build_key_pills(
                            explicit_group_key,
                            &group_inherit_lbl,
                            &move |kid| Message::SetGroupSecretKey(group_owned.clone(), kid),
                        ),
                    ]
                    .spacing(space::SM)
                    .align_y(Alignment::Center),
                );
            }

            if let Some((target_id, target_name, current_name)) = rekey_target {
                assign_col = assign_col.push(
                    container(
                        row![
                            icons::alert_triangle(icon_size::XS, p.warning_fg),
                            theme::clipped_line(
                                text(format!(
                                    "Profile '{active_profile}' uses '{current_name}'; assigned key is '{target_name}'."
                                ))
                                .size(type_scale::CAPTION)
                                .color(p.warning_fg),
                                Length::Fill,
                            ),
                            tip(
                                button(
                                    text(format!("Re-key '{active_profile}'"))
                                        .size(type_scale::CAPTION)
                                        .wrapping(text::Wrapping::None),
                                )
                                .padding([3, 10])
                                .style(theme::primary_btn(p))
                                .on_press(Message::RekeyActiveRepoVault(target_id.clone())),
                                format!(
                                    "Re-encrypt .gitfleet/{active_vault_file} with '{target_name}'"
                                ),
                                tooltip::Position::Top,
                                p,
                            ),
                        ]
                        .spacing(space::SM)
                        .align_y(Alignment::Center),
                    )
                    .padding([6, 10])
                    .width(Length::Fill)
                    .style(theme::pill_box(p.warning_bg, p.warning_fg, p.border_default)),
                );
            }

            body_col = body_col.push(
                container(assign_col)
                    .padding(10)
                    .width(Length::Fill)
                    .style(theme::card_box(p)),
            );
        }

        // B. Create / Import named keys
        let create_row = row![
            text_input(
                "New key name (e.g. Personal, Company A, Prod)…",
                &self.new_key_name_input
            )
            .on_input(Message::NewVaultKeyNameChanged)
            .on_submit(Message::CreateNamedVaultKey)
            .padding([5, 9])
            .size(type_scale::CAPTION)
            .width(Length::Fill)
            .style(theme::dark_input(p)),
            button(
                row![
                    icons::plus(12.0, Color::WHITE),
                    text("Create Key").size(type_scale::CAPTION),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
            )
            .padding([5, 10])
            .style(theme::primary_btn(p))
            .on_press(Message::CreateNamedVaultKey),
            button(
                row![
                    icons::key(
                        12.0,
                        if self.show_import_key_bar {
                            p.accent_fg
                        } else {
                            p.text_secondary
                        }
                    ),
                    text("Import…").size(type_scale::CAPTION),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
            )
            .padding([5, 10])
            .style(theme::filter_pill_btn(p, self.show_import_key_bar))
            .on_press(Message::ToggleImportVaultKey),
        ]
        .spacing(space::XS)
        .align_y(Alignment::Center);

        let mut keys_section = column![create_row].spacing(space::XS).width(Length::Fill);

        if self.show_import_key_bar || !key_ready {
            keys_section = keys_section.push(
                row![
                    text_input("Key name…", &self.import_key_name_input)
                        .on_input(Message::ImportVaultKeyNameChanged)
                        .padding([5, 9])
                        .size(type_scale::CAPTION)
                        .width(Length::Fixed(165.0))
                        .style(theme::dark_input(p)),
                    text_input(
                        "Paste AGE-SECRET-KEY-1… from another machine",
                        &self.import_key_input
                    )
                    .on_input(Message::ImportVaultKeyInputChanged)
                    .on_submit(Message::ImportVaultKeySubmitted)
                    .padding([5, 9])
                    .size(type_scale::CAPTION)
                    .width(Length::Fill)
                    .style(theme::dark_input(p)),
                    button(text("Import").size(type_scale::CAPTION))
                        .padding([5, 10])
                        .style(theme::primary_btn(p))
                        .on_press(Message::ImportVaultKeySubmitted),
                ]
                .spacing(space::XS)
                .align_y(Alignment::Center),
            );
        }

        if !keyring.keys.is_empty() {
            let mut rows = column![].spacing(0).width(Length::Fill);
            let active_eff_id = effective_key.map(|(k, _)| k.id.as_str());

            for (idx, k) in keyring.keys.iter().enumerate() {
                if idx > 0 {
                    rows = rows.push(horizontal_rule(1).style(theme::subtle_rule(p)));
                }
                let is_renaming = self
                    .renaming_key
                    .as_ref()
                    .filter(|(id, _)| id == &k.id)
                    .map(|(_, draft)| draft.as_str());
                let is_eff = active_eff_id == Some(k.id.as_str());

                let row_inner: Element<'_, Message> = if let Some(draft) = is_renaming {
                    row![
                        icons::key(12.0, p.accent_fg),
                        text_input("Key name…", draft)
                            .on_input(Message::RenameVaultKeyInputChanged)
                            .on_submit(Message::SubmitRenameVaultKey)
                            .padding([4, 8])
                            .size(type_scale::CAPTION)
                            .width(Length::Fixed(200.0))
                            .style(theme::dark_input(p)),
                        button(text("Save").size(type_scale::CAPTION))
                            .padding([4, 9])
                            .style(theme::primary_btn(p))
                            .on_press(Message::SubmitRenameVaultKey),
                        button(text("Cancel").size(type_scale::CAPTION))
                            .padding([4, 9])
                            .style(theme::subtle_btn(p))
                            .on_press(Message::CancelRenameVaultKey),
                        Space::with_width(Length::Fill),
                    ]
                    .spacing(space::XS)
                    .align_y(Alignment::Center)
                    .into()
                } else {
                    let mut r = row![
                        icons::key(
                            12.0,
                            if is_eff {
                                p.success_fg
                            } else {
                                p.text_secondary
                            }
                        ),
                        text(k.name.clone())
                            .size(type_scale::BODY)
                            .color(p.text_primary)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(space::XS)
                    .align_y(Alignment::Center);

                    if k.is_default {
                        r = r.push(theme::badge(
                            "Default",
                            p.fill_selected,
                            p.accent_fg,
                            p.border_subtle,
                            false,
                        ));
                    }
                    if is_eff {
                        r = r.push(theme::badge(
                            "Active",
                            p.success_bg,
                            p.success_fg,
                            p.border_subtle,
                            false,
                        ));
                    }

                    r = r
                        .push(theme::clipped_line(
                            text(truncate_chars(&k.public_key, 22))
                                .size(type_scale::CODE)
                                .font(theme::CODE_FONT)
                                .color(p.text_muted),
                            Length::Fill,
                        ));

                    if !k.is_default {
                        r = r.push(
                            button(text("Set Default").size(type_scale::CAPTION))
                                .padding([3, 8])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::SetDefaultVaultKey(k.id.clone())),
                        );
                    }

                    r = r
                        .push(tip(
                            button(icons::pencil(12.0, p.text_secondary))
                                .padding([4, 6])
                                .style(theme::vscode_icon_btn(p, false))
                                .on_press(Message::StartRenameVaultKey(
                                    k.id.clone(),
                                    k.name.clone(),
                                )),
                            "Rename key",
                            tooltip::Position::Top,
                            p,
                        ))
                        .push(tip(
                            button(
                                row![
                                    icons::copy(11.0, p.text_secondary),
                                    text("Pub").size(type_scale::CAPTION),
                                ]
                                .spacing(4)
                                .align_y(Alignment::Center),
                            )
                            .padding([3, 8])
                            .style(theme::subtle_btn(p))
                            .on_press(Message::CopyNamedKeyPublic(k.id.clone())),
                            "Copy Age public key",
                            tooltip::Position::Top,
                            p,
                        ))
                        .push(tip(
                            button(
                                row![
                                    icons::key(11.0, p.text_secondary),
                                    text("Backup").size(type_scale::CAPTION),
                                ]
                                .spacing(4)
                                .align_y(Alignment::Center),
                            )
                            .padding([3, 8])
                            .style(theme::subtle_btn(p))
                            .on_press(Message::CopyNamedKeyPrivate(k.id.clone())),
                            "Copy AGE-SECRET-KEY to back up or import on another machine",
                            tooltip::Position::Top,
                            p,
                        ))
                        .push(tip(
                            button(icons::trash(12.0, p.danger_fg))
                                .padding([4, 6])
                                .style(theme::vscode_icon_btn(p, false))
                                .on_press(Message::RequestConfirm(
                                    PendingConfirm::delete_vault_key(k.id.clone(), &k.name),
                                )),
                            "Delete key from local keyring",
                            tooltip::Position::Top,
                            p,
                        ));
                    r.into()
                };

                rows = rows.push(container(row_inner).padding([7, 10]).width(Length::Fill));
            }

            keys_section = keys_section.push(
                container(rows)
                    .width(Length::Fill)
                    .style(move |_| container::Style {
                        background: Some(Background::Color(p.surface_sidebar)),
                        border: Border {
                            color: p.border_subtle,
                            width: 1.0,
                            radius: 6.0.into(),
                        },
                        ..Default::default()
                    }),
            );
        }

        body_col = body_col.push(keys_section);

        // C. Danger Zone: Delete Vault (single canonical location — never duplicated!)
        if has_repo_vault_artifacts {
            body_col = body_col.push(
                container(
                    row![
                        column![
                            text("Delete Repository Vault")
                                .size(type_scale::BODY)
                                .color(p.danger_fg),
                            text("Removes .gitfleet/secrets*.age and secrets.json from this repo. Local .env / .toml / .yaml files on disk stay untouched.")
                                .size(type_scale::CAPTION)
                                .color(p.text_muted),
                        ]
                        .spacing(2)
                        .width(Length::Fill),
                        button(
                            row![
                                icons::trash(12.0, p.danger_fg),
                                text("Delete Vault")
                                    .size(type_scale::CAPTION)
                                    .color(p.danger_fg)
                                    .wrapping(text::Wrapping::None),
                            ]
                            .spacing(5)
                            .align_y(Alignment::Center),
                        )
                        .padding([5, 10])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::RequestConfirm(
                            PendingConfirm::destroy_repo_vault(&active_repo_name),
                        )),
                    ]
                    .spacing(space::SM)
                    .align_y(Alignment::Center),
                )
                .padding([9, 12])
                .width(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(Background::Color(p.danger_bg)),
                    border: Border {
                        color: Color {
                            a: 0.45,
                            ..p.danger_fg
                        },
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                    ..Default::default()
                }),
            );
        }

        column![
            header,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            container(slim_scroll(body_col, p).height(Length::Fill))
                .padding([10, 14])
                .width(Length::Fill)
                .height(Length::Fill),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
