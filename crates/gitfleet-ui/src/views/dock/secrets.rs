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
        let keyring = self.service.security.keyring_summary();
        let key_ready = !keyring.keys.is_empty();
        let effective_key = self.resolve_active_repo_effective_key();
        let ws = self.service.workspace.current();
        let parent_repo = self
            .selected_repo
            .as_ref()
            .and_then(|id| ws.repos.iter().find(|r| &r.id == id));
        let encrypt_needed = self.vault_summary.encrypt_needed_count();
        let restore_needed = self.vault_summary.restore_needed_count();
        let show_keys_panel =
            self.show_key_manager || !key_ready || self.show_import_key_bar;

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

        let tracked_paths: HashSet<&str> = self
            .vault_summary
            .files
            .iter()
            .map(|f| f.path.as_str())
            .collect();

        // 1. Unified full-bleed Dock Toolbar (matches Search / Ignore / Output cadence)
        let key_status: Element<'_, Message> = if let Some((eff, source_label)) = &effective_key {
            row![
                icons::key(icon_size::SM, p.success_fg),
                theme::badge(
                    truncate_chars(&eff.name, 20),
                    p.success_bg,
                    p.success_fg,
                    p.border_subtle,
                    false
                ),
                theme::badge(
                    truncate_chars(source_label, 22),
                    p.surface_card,
                    p.text_muted,
                    p.border_subtle,
                    false
                ),
                text(truncate_chars(&eff.public_key, 16))
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

        let mut key_actions = row![
            tip(
                button(
                    row![
                        icons::key(
                            icon_size::XS,
                            if show_keys_panel {
                                p.accent_fg
                            } else {
                                p.text_secondary
                            }
                        ),
                        text(format!("Keys ({})", keyring.keys.len())).size(type_scale::BODY),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding(pad::BTN_SM)
                .style(theme::filter_pill_btn(p, show_keys_panel))
                .on_press(Message::ToggleKeyManager),
                "Manage named Age keys & assign keys to Repo or Group",
                tooltip::Position::Top,
                p,
            ),
        ]
        .spacing(space::XS)
        .align_y(Alignment::Center);

        if let Some((target_id, target_name, current_name)) = &rekey_target {
            key_actions = key_actions.push(tip(
                button(
                    row![
                        icons::refresh(icon_size::XS, p.warning_fg),
                        text("Re-key").size(type_scale::BODY).color(p.warning_fg),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding(pad::BTN_SM)
                .style(theme::subtle_btn(p))
                .on_press(Message::RekeyActiveRepoVault(target_id.clone())),
                format!(
                    "Vault is currently encrypted with '{current_name}'. Re-encrypt .gitfleet/secrets.age with '{target_name}'"
                ),
                tooltip::Position::Top,
                p,
            ));
        }

        if key_ready {
            let encrypt_label = if encrypt_needed > 0 {
                format!("Encrypt All ({encrypt_needed})")
            } else {
                "Encrypt All".to_string()
            };
            let restore_label = if restore_needed > 0 {
                format!("Restore All ({restore_needed})")
            } else {
                "Restore All".to_string()
            };
            key_actions = key_actions
                .push(
                    button(
                        row![
                            icons::lock(icon_size::XS, Color::WHITE),
                            text(encrypt_label).size(type_scale::BODY),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                    )
                    .padding(pad::BTN_SM)
                    .style(theme::primary_btn(p))
                    .on_press(Message::EncryptRepoEnv),
                )
                .push(
                    button(text(restore_label).size(type_scale::BODY))
                        .padding(pad::BTN_SM)
                        .style(theme::filter_pill_btn(p, restore_needed > 0))
                        .on_press(Message::RestoreRepoEnv),
                );
        }

        let active_repo_name = self
            .current_repo()
            .map(|r| r.name)
            .unwrap_or_else(|| "repository".to_string());
        let has_repo_vault_artifacts =
            self.vault_summary.has_vault || self.vault_summary.has_manifest;

        if has_repo_vault_artifacts {
            key_actions = key_actions.push(tip(
                button(
                    row![
                        icons::trash(icon_size::XS, p.danger_fg),
                        text("Delete Vault").size(type_scale::BODY).color(p.danger_fg),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding(pad::BTN_SM)
                .style(theme::subtle_btn(p))
                .on_press(Message::RequestConfirm(
                    PendingConfirm::destroy_repo_vault(&active_repo_name),
                )),
                "Completely delete .gitfleet/secrets.age & secrets.json from this repo (local .env files on disk are kept)",
                tooltip::Position::Top,
                p,
            ));
        }

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

        if show_keys_panel {
            let mut key_mgr_col = column![].spacing(space::SM).width(Length::Fill);

            // A. Repo & Group key assignment pills (when keys exist and a repo is selected)
            if key_ready && let Some(repo) = parent_repo {
                let mut assign_col = column![].spacing(space::XS).width(Length::Fill);

                // Helper to build a wrapped grid of key selection pills
                let build_key_pills = |selected_key_id: Option<&str>,
                                       inherit_label: &str,
                                       on_select: &dyn Fn(Option<String>) -> Message|
                 -> Element<'_, Message> {
                    let mut all_pills: Vec<Element<'_, Message>> = Vec::with_capacity(keyring.keys.len() + 1);
                    all_pills.push(
                        button(text(inherit_label.to_string()).size(type_scale::CAPTION))
                            .padding([4, 9])
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
                                        icon_size::XS,
                                        if is_sel { p.accent_fg } else { p.text_muted }
                                    ),
                                    text(truncate_chars(&k.name, 22)).size(type_scale::CAPTION),
                                ]
                                .spacing(4)
                                .align_y(Alignment::Center),
                            )
                            .padding([4, 9])
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
                let repo_inherit_lbl = if let Some(g) = repo.group.as_deref().filter(|g| !g.is_empty()) {
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
                            text(format!("Repo '{}'", truncate_chars(&repo.name, 20)))
                                .size(type_scale::CAPTION)
                                .color(p.text_secondary),
                        )
                        .width(Length::Fixed(165.0)),
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
                                text(format!("Group '{}'", truncate_chars(group_path, 20)))
                                    .size(type_scale::CAPTION)
                                    .color(p.text_secondary),
                            )
                            .width(Length::Fixed(165.0)),
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

                if let Some((target_id, target_name, current_name)) = &rekey_target {
                    assign_col = assign_col.push(
                        container(
                            row![
                                icons::alert_triangle(icon_size::XS, p.warning_fg),
                                text(format!(
                                    "Existing vault was encrypted with '{current_name}', while assigned key is '{target_name}'."
                                ))
                                .size(type_scale::CAPTION)
                                .color(p.warning_fg),
                                Space::with_width(Length::Fill),
                                button(text("Re-key Vault Now").size(type_scale::CAPTION))
                                    .padding([3, 9])
                                    .style(theme::primary_btn(p))
                                    .on_press(Message::RekeyActiveRepoVault(target_id.clone())),
                            ]
                            .spacing(space::SM)
                            .align_y(Alignment::Center),
                        )
                        .padding([6, 10])
                        .width(Length::Fill)
                        .style(theme::pill_box(p.warning_bg, p.warning_fg, p.border_default)),
                    );
                }

                key_mgr_col = key_mgr_col.push(
                    container(assign_col)
                        .padding(pad::CARD)
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
                .padding(pad::INPUT_COMPACT)
                .size(type_scale::BODY)
                .width(Length::Fill)
                .style(theme::dark_input(p)),
                button(
                    row![
                        icons::plus(icon_size::XS, Color::WHITE),
                        text("Create Key").size(type_scale::BODY),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding(pad::BTN_SM)
                .style(theme::primary_btn(p))
                .on_press(Message::CreateNamedVaultKey),
                button(
                    row![
                        icons::key(
                            icon_size::XS,
                            if self.show_import_key_bar {
                                p.accent_fg
                            } else {
                                p.text_secondary
                            }
                        ),
                        text("Import Key…").size(type_scale::BODY),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding(pad::BTN_SM)
                .style(theme::filter_pill_btn(p, self.show_import_key_bar))
                .on_press(Message::ToggleImportVaultKey),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center);
            key_mgr_col = key_mgr_col.push(create_row);

            if self.show_import_key_bar || !key_ready {
                let import_row = row![
                    text_input("Key name (e.g. Team Backend)…", &self.import_key_name_input)
                        .on_input(Message::ImportVaultKeyNameChanged)
                        .padding(pad::INPUT_COMPACT)
                        .size(type_scale::BODY)
                        .width(Length::Fixed(210.0))
                        .style(theme::dark_input(p)),
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
                    button(text("Import").size(type_scale::BODY))
                        .padding(pad::BTN_SM)
                        .style(theme::primary_btn(p))
                        .on_press(Message::ImportVaultKeySubmitted),
                ]
                .spacing(space::SM)
                .align_y(Alignment::Center);
                key_mgr_col = key_mgr_col.push(import_row);
            }

            // C. Keyring entries list
            if !keyring.keys.is_empty() {
                let mut list_col = column![].spacing(space::XS).width(Length::Fill);
                let active_eff_id = effective_key.as_ref().map(|(k, _)| k.id.as_str());

                for k in &keyring.keys {
                    let is_renaming = self
                        .renaming_key
                        .as_ref()
                        .filter(|(id, _)| id == &k.id)
                        .map(|(_, draft)| draft.as_str());
                    let is_eff = active_eff_id == Some(k.id.as_str());

                    let row_inner: Element<'_, Message> = if let Some(draft) = is_renaming {
                        row![
                            icons::key(icon_size::XS, p.accent_fg),
                            text_input("Key name…", draft)
                                .on_input(Message::RenameVaultKeyInputChanged)
                                .on_submit(Message::SubmitRenameVaultKey)
                                .padding(pad::INPUT_COMPACT)
                                .size(type_scale::BODY)
                                .width(Length::Fixed(220.0))
                                .style(theme::dark_input(p)),
                            button(text("Save").size(type_scale::CAPTION))
                                .padding([4, 10])
                                .style(theme::primary_btn(p))
                                .on_press(Message::SubmitRenameVaultKey),
                            button(text("Cancel").size(type_scale::CAPTION))
                                .padding([4, 10])
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
                                icon_size::XS,
                                if is_eff {
                                    p.success_fg
                                } else {
                                    p.text_secondary
                                }
                            ),
                            text(k.name.clone())
                                .size(type_scale::BODY)
                                .color(p.text_primary),
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
                            .push(
                                text(truncate_chars(&k.public_key, 22))
                                    .size(type_scale::CODE)
                                    .font(theme::CODE_FONT)
                                    .color(p.text_muted),
                            )
                            .push(Space::with_width(Length::Fill));

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
                                button(icons::pencil(icon_size::XS, p.text_secondary))
                                    .padding(pad::BTN_ICON)
                                    .style(theme::subtle_btn(p))
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
                                        icons::copy(icon_size::XS, p.text_secondary),
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
                                        icons::key(icon_size::XS, p.text_secondary),
                                        text("Backup").size(type_scale::CAPTION),
                                    ]
                                    .spacing(4)
                                    .align_y(Alignment::Center),
                                )
                                .padding([3, 8])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::CopyNamedKeyPrivate(k.id.clone())),
                                "Copy AGE-SECRET-KEY to share or back up on another machine",
                                tooltip::Position::Top,
                                p,
                            ))
                            .push(tip(
                                button(icons::trash(icon_size::XS, p.danger_fg))
                                    .padding(pad::BTN_ICON)
                                    .style(theme::subtle_btn(p))
                                    .on_press(Message::RequestConfirm(
                                        PendingConfirm::delete_vault_key(k.id.clone(), &k.name),
                                    )),
                                "Delete key from local keyring",
                                tooltip::Position::Top,
                                p,
                            ));
                        r.into()
                    };

                    list_col = list_col.push(
                        container(row_inner)
                            .padding([6, 10])
                            .width(Length::Fill)
                            .style(theme::card_box(p)),
                    );
                }

                key_mgr_col = key_mgr_col.push(list_col);
            }

            let hdr_action: Option<Element<'_, Message>> = if key_ready {
                Some(
                    button(text("Done").size(type_scale::CAPTION))
                        .padding(pad::BTN_SM)
                        .style(theme::subtle_btn(p))
                        .on_press(Message::ToggleKeyManager)
                        .into(),
                )
            } else {
                None
            };

            content = content.push(theme::section_card(
                p,
                "Age Keyring & Repo / Group Assignment",
                Some("Create or import named Age keys (e.g. Personal, Company A) and assign them to specific repositories or groups. Decryption automatically tries all saved keys."),
                hdr_action,
                key_mgr_col,
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
                        tip(
                            button(
                                row![
                                    icons::trash(icon_size::XS, p.danger_fg),
                                    text("Delete Vault…")
                                        .size(type_scale::BODY)
                                        .color(p.danger_fg),
                                ]
                                .spacing(5)
                                .align_y(Alignment::Center),
                            )
                            .padding(pad::BTN_SM)
                            .style(theme::subtle_btn(p))
                            .on_press(Message::RequestConfirm(
                                PendingConfirm::destroy_repo_vault(&active_repo_name),
                            )),
                            "Remove .gitfleet/secrets.age & secrets.json completely (keeps local .env files on disk)",
                            tooltip::Position::Top,
                            p,
                        ),
                        button(text("Import / Manage Keys").size(type_scale::BODY))
                            .padding(pad::BTN_SM)
                            .style(theme::primary_btn(p))
                            .on_press(Message::OpenSecretsDrawer { show_keys: true }),
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
