use crate::app::{DrawerTab, GitFleetApp, Message, STATUS_BAR_HEIGHT};
use crate::icons;
use crate::theme::{self, Palette, icon_size, type_scale};
use crate::views::{tip, truncate_chars};
use iced::widget::{Space, button, column, container, horizontal_rule, row, text, tooltip};
use iced::{Alignment, Color, Element, Length};

impl GitFleetApp {
    /// VS Code-style Status Bar: live repo telemetry & status messages on the left,
    /// active file info & Bottom Dock panel toggles on the right (no buttons duplicated from Titlebar).
    pub(crate) fn view_status_bar(
        &self,
        p: Palette,
        _ws_name: &str,
        repo_count: usize,
    ) -> Element<'_, Message> {
        let active_repo = self.current_repo();
        let active_status = self.active_status();

        let branch_label = active_status
            .and_then(|st| st.branch.clone())
            .unwrap_or_else(|| "HEAD".into());
        let behind = active_status.map(|st| st.behind).unwrap_or(0);
        let ahead = active_status.map(|st| st.ahead).unwrap_or(0);
        let conflicts = active_status.map(|st| st.conflict_count).unwrap_or(0);
        let hygiene_warns = self.staged_secrets.len() + self.oversized_files.len();
        let staged_n = active_status.map(|st| st.staged_count).unwrap_or(0);
        let unstaged_n = active_status
            .map(|st| st.unstaged_count + st.untracked_count + st.conflict_count)
            .unwrap_or(0);

        let mut left = row![]
            .spacing(4)
            .height(Length::Fill)
            .align_y(Alignment::Center);

        if let Some(repo) = &active_repo {
            // Accent left badge: active branch (single quick-switch entry Ctrl+B)
            left = left
                .push(tip(
                    button(
                        container(
                            row![
                                icons::branch(icon_size::XS, Color::WHITE),
                                text(truncate_chars(&branch_label, 24))
                                    .size(type_scale::CAPTION)
                                    .color(Color::WHITE)
                                    .wrapping(text::Wrapping::None),
                            ]
                            .spacing(5)
                            .align_y(Alignment::Center),
                        )
                        .height(Length::Fill)
                        .align_y(iced::alignment::Vertical::Center),
                    )
                    .padding([0, 10])
                    .height(Length::Fill)
                    .style(theme::vscode_status_badge(p))
                    .on_press(Message::ToggleBranchPopover),
                    "Switch / Create Branch (Ctrl+B)",
                    tooltip::Position::Top,
                    p,
                ))
                .push(
                    container(
                        text(truncate_chars(&repo.name, 22))
                            .size(type_scale::CAPTION)
                            .color(p.text_primary)
                            .wrapping(text::Wrapping::None),
                    )
                    .padding([0, 6]),
                )
                .push(
                    if active_status.is_some_and(|st| !st.detached && st.upstream.is_none()) {
                        tip(
                            button(
                                row![
                                    icons::push(11.0, p.accent_fg),
                                    text("Publish")
                                        .size(type_scale::CAPTION)
                                        .color(p.accent_fg)
                                        .wrapping(text::Wrapping::None),
                                ]
                                .spacing(4)
                                .align_y(Alignment::Center),
                            )
                            .padding([2, 7])
                            .style(theme::vscode_icon_btn(p, false))
                            .on_press(Message::RequestConfirm(
                                crate::app::PendingConfirm::publish_branch(&branch_label),
                            )),
                            "Local-only branch (no remote upstream yet) — click to publish to origin",
                            tooltip::Position::Top,
                            p,
                        )
                    } else {
                        container(
                            row![
                                text(behind.to_string())
                                    .size(type_scale::CAPTION)
                                    .color(if behind > 0 { p.accent_fg } else { p.text_muted })
                                    .wrapping(text::Wrapping::None),
                                icons::pull(
                                    11.0,
                                    if behind > 0 { p.accent_fg } else { p.text_muted },
                                ),
                                text(ahead.to_string())
                                    .size(type_scale::CAPTION)
                                    .color(if ahead > 0 { p.success_fg } else { p.text_muted })
                                    .wrapping(text::Wrapping::None),
                                icons::push(
                                    11.0,
                                    if ahead > 0 { p.success_fg } else { p.text_muted },
                                ),
                            ]
                            .spacing(3)
                            .align_y(Alignment::Center),
                        )
                        .padding([0, 6])
                        .into()
                    },
                );

            if conflicts > 0 {
                left = left.push(
                    container(
                        row![
                            icons::x_circle(12.0, p.danger_fg),
                            text(format!("{conflicts} conflict"))
                                .size(type_scale::CAPTION)
                                .color(p.danger_fg)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                    )
                    .padding([0, 6]),
                );
            }
        }

        // Single authoritative live status / toast indicator (clipped so it never pushes right buttons off-screen)
        let status_msg = self
            .busy_label
            .as_deref()
            .unwrap_or(self.toast_message.as_str());
        if !status_msg.is_empty() {
            if self.toast_is_error
                && self.busy_label.is_none()
                && self.last_action_error.is_some()
            {
                left = left.push(tip(
                    button(
                        row![
                            icons::alert_triangle(12.0, p.danger_fg),
                            theme::clipped_line(
                                text(truncate_chars(status_msg, 52))
                                    .size(type_scale::CAPTION)
                                    .color(p.danger_fg),
                                Length::Shrink,
                            ),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                    )
                    .padding([2, 8])
                    .style(theme::vscode_status_item(p))
                    .on_press(Message::OpenLastErrorModal),
                    "Click to open full error details popup",
                    tooltip::Position::Top,
                    p,
                ));
            } else {
                left = left.push(
                    container(theme::clipped_line(
                        text(truncate_chars(status_msg, 56))
                            .size(type_scale::CAPTION)
                            .color(if self.toast_is_error {
                                p.danger_fg
                            } else if self.busy_label.is_some() {
                                p.accent_fg
                            } else {
                                p.text_muted
                            }),
                        Length::Shrink,
                    ))
                    .padding([0, 8]),
                );
            }
        }

        // Right side: context telemetry + Bottom Dock panel toggles (Search, Secrets, Terminal, Output)
        let mut right = row![]
            .spacing(2)
            .height(Length::Fill)
            .align_y(Alignment::Center);

        if let Some(file) = &self.active_file_path {
            let base = file.rsplit(['/', '\\']).next().unwrap_or(file);
            right = right.push(
                container(
                    text(truncate_chars(base, 26))
                        .size(type_scale::BADGE)
                        .color(p.text_muted)
                        .wrapping(text::Wrapping::None),
                )
                .padding([0, 4]),
            );
            if self.open_doc_id.is_some() {
                let enc_color = if self.show_encoding_modal {
                    p.accent_fg
                } else {
                    p.text_secondary
                };
                let eol_color = if self.show_eol_modal {
                    p.accent_fg
                } else {
                    p.text_secondary
                };
                right = right
                    .push(tip(
                        button(
                            row![
                                text(self.open_doc_encoding.label())
                                    .size(type_scale::BADGE)
                                    .color(enc_color)
                                    .wrapping(text::Wrapping::None),
                                icons::chevron_down(10.0, p.text_muted),
                            ]
                            .spacing(3)
                            .align_y(Alignment::Center),
                        )
                        .padding([2, 6])
                        .style(theme::vscode_status_item(p))
                        .on_press(Message::ToggleEncodingModal),
                        format!(
                            "Encoding: {} — click to open Encoding menu",
                            self.open_doc_encoding.description()
                        ),
                        tooltip::Position::Top,
                        p,
                    ))
                    .push(tip(
                        button(
                            row![
                                text(self.open_doc_line_ending.label())
                                    .size(type_scale::BADGE)
                                    .color(eol_color)
                                    .wrapping(text::Wrapping::None),
                                icons::chevron_down(10.0, p.text_muted),
                            ]
                            .spacing(3)
                            .align_y(Alignment::Center),
                        )
                        .padding([2, 6])
                        .style(theme::vscode_status_item(p))
                        .on_press(Message::ToggleEolModal),
                        format!(
                            "End of Line Sequence: {} — click to open EOL menu",
                            self.open_doc_line_ending.description()
                        ),
                        tooltip::Position::Top,
                        p,
                    ));
            }
            let lang_col = self.open_doc_language.badge_color(self.theme_mode);
            let syntax_fg = if self.show_syntax_modal {
                p.accent_fg
            } else {
                p.text_secondary
            };
            right = right.push(tip(
                button(
                    row![
                        container(Space::new(Length::Fixed(6.0), Length::Fixed(6.0)))
                            .style(theme::dot_box(lang_col)),
                        text(self.open_doc_language.label())
                            .size(type_scale::BADGE)
                            .color(syntax_fg)
                            .wrapping(text::Wrapping::None),
                        icons::chevron_down(10.0, p.text_muted),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([2, 6])
                .style(theme::vscode_status_item(p))
                .on_press(Message::ToggleSyntaxModal),
                "Syntax Highlighting — click to select language",
                tooltip::Position::Top,
                p,
            ));
        }

        if active_repo.is_some() {
            right = right.push(
                container(
                    text(format!(
                        "Unstaged {unstaged_n} · Staged {staged_n} · {repo_count} repo(s)"
                    ))
                    .size(type_scale::BADGE)
                    .color(p.text_muted)
                    .wrapping(text::Wrapping::None),
                )
                .padding([0, 6]),
            );
        }

        // Active background/modal Git clone indicator (isolated from `busy_label` so watcher refreshes never flicker it)
        if let Some(active) = &self.active_clone {
            let is_cancelling = active.snapshot.is_cancelled;
            let fg = if is_cancelling {
                p.warning_fg
            } else {
                p.accent_fg
            };
            let pill_label = if is_cancelling {
                format!("Cancelling {}", truncate_chars(&active.folder_name, 16))
            } else {
                format!(
                    "Cloning {} {:>2}%",
                    truncate_chars(&active.folder_name, 16),
                    active.snapshot.overall_percent
                )
            };
            right = right.push(tip(
                button(
                    row![
                        icons::cloud_download(11.0, fg),
                        text(pill_label)
                            .size(type_scale::BADGE)
                            .font(theme::CODE_FONT)
                            .color(fg)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([2, 8])
                .style(theme::filter_pill_btn(p, true))
                .on_press(Message::OpenCloneModal(None)),
                "Click to view live Git clone progress or cancel",
                tooltip::Position::Top,
                p,
            ));
        }

        // Subtle software-update indicator (visible ONLY when a new non-skipped release is ready or downloading)
        let skipped_ver = self.service.settings().skipped_update_version;
        match &self.update_status {
            crate::app::UpdateUiStatus::Available { release }
                if skipped_ver.as_deref() != Some(release.version.as_str()) =>
            {
                let ver_label = format!("v{}", release.version);
                let tip_text = format!(
                    "GitFleet {ver_label} is available — click to review release notes & update"
                );
                right = right.push(tip(
                    button(
                        row![
                            icons::sparkle(11.0, p.accent_fg),
                            text(format!("Update {ver_label}"))
                                .size(type_scale::BADGE)
                                .color(p.accent_fg)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                    )
                    .padding([2, 8])
                    .style(theme::filter_pill_btn(p, true))
                    .on_press(Message::OpenUpdateModal),
                    tip_text,
                    tooltip::Position::Top,
                    p,
                ));
            }
            crate::app::UpdateUiStatus::Applying {
                phase,
                downloaded_bytes,
                total_bytes,
                ..
            } => {
                let label = if *phase == gitfleet_app::updater::UpdateDownloadPhase::Downloading
                    && *total_bytes > 0
                {
                    let pct = ((*downloaded_bytes as f32 / *total_bytes as f32).clamp(0.0, 1.0)
                        * 100.0)
                        .round() as u32;
                    format!("Updating {pct}%")
                } else {
                    "Updating…".to_string()
                };
                right = right.push(tip(
                    button(
                        row![
                            icons::refresh(11.0, p.accent_fg),
                            text(label)
                                .size(type_scale::BADGE)
                                .color(p.accent_fg)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                    )
                    .padding([2, 8])
                    .style(theme::filter_pill_btn(p, true))
                    .on_press(Message::OpenUpdateModal),
                    "Click to view update progress or cancel",
                    tooltip::Position::Top,
                    p,
                ));
            }
            _ => {}
        }

        let search_active = self.drawer_tab == Some(DrawerTab::Search);
        let search_color = if search_active {
            p.accent_fg
        } else {
            p.text_secondary
        };

        let secrets_active = self.drawer_tab == Some(DrawerTab::Secrets);
        let encrypt_needed = self.vault_summary.encrypt_needed_count();
        let restore_needed = self.vault_summary.restore_needed_count();
        let encrypt_paths = self.vault_summary.encrypt_needed_paths();
        let restore_paths = self.vault_summary.restore_needed_paths();
        let active_profile = self.vault_summary.active_profile.as_str();
        let vault_fname = self.vault_summary.vault_file_name.as_str();
        let profile_suffix = if self.vault_summary.is_default_profile() {
            String::new()
        } else {
            format!(" [{active_profile}]")
        };

        let (secrets_status_text, secrets_color, secrets_dot, secrets_tip) = if hygiene_warns > 0 {
            (
                format!("Secrets ({hygiene_warns}!)"),
                p.danger_fg,
                Some(p.danger_fg),
                "Staged secret / oversized hygiene warnings — click to review".to_string(),
            )
        } else if self.vault_summary.vault_locked {
            let key_hint = self
                .vault_summary
                .manifest_key_name
                .as_deref()
                .map(|k| format!(" (key: {k})"))
                .unwrap_or_default();
            (
                format!("Secrets{profile_suffix}: Locked"),
                p.warning_fg,
                Some(p.warning_fg),
                format!(
                    "Secrets profile '{active_profile}' is locked{key_hint} — click to import or select key"
                ),
            )
        } else if encrypt_needed > 0 {
            (
                format!("Secrets{profile_suffix}: {encrypt_needed} modified"),
                p.warning_fg,
                Some(p.warning_fg),
                format!(
                    "Secret file(s) changed on disk ({}) vs profile '{active_profile}' — click Encrypt or open Secrets Vault",
                    encrypt_paths.join(", ")
                ),
            )
        } else if restore_needed > 0 {
            (
                format!("Secrets{profile_suffix}: {restore_needed} missing"),
                p.accent_fg,
                Some(p.accent_fg),
                format!(
                    "Secret file(s) missing on disk ({}) in profile '{active_profile}' — click Restore or open Secrets Vault",
                    restore_paths.join(", ")
                ),
            )
        } else if !self.vault_summary.files.is_empty() {
            (
                format!("Secrets{profile_suffix} ({})", self.vault_summary.files.len()),
                if secrets_active {
                    p.accent_fg
                } else {
                    p.text_secondary
                },
                None,
                format!(
                    "Secrets Vault (.gitfleet/{vault_fname}, profile '{active_profile}') — all files synced"
                ),
            )
        } else {
            (
                format!("Secrets{profile_suffix}"),
                if secrets_active {
                    p.accent_fg
                } else {
                    p.text_secondary
                },
                None,
                format!("Secrets Vault (.gitfleet/secrets.json & {vault_fname})"),
            )
        };

        let term_active = self.drawer_tab == Some(DrawerTab::Terminal);
        let term_color = if term_active {
            p.accent_fg
        } else {
            p.text_secondary
        };
        let active_shell = self
            .terminal_tabs
            .get(self.active_terminal_tab)
            .and_then(|t| t.panes.get(t.focused_pane).or_else(|| t.panes.first()))
            .map(|pane| pane.short_label.clone())
            .unwrap_or_else(|| self.default_terminal_profile_id());

        let log_active = self.drawer_tab == Some(DrawerTab::CommandLog);
        let log_count = self.service.command_log.list().len();
        let log_color = if log_active {
            p.accent_fg
        } else {
            p.text_muted
        };

        let mut secrets_btn_row = row![icons::shield(12.0, secrets_color)]
            .spacing(4)
            .align_y(Alignment::Center);
        if let Some(dot_color) = secrets_dot {
            secrets_btn_row = secrets_btn_row.push(
                container(Space::new(Length::Fixed(6.0), Length::Fixed(6.0))).style(move |_| {
                    container::Style {
                        background: Some(iced::Background::Color(dot_color)),
                        border: iced::Border {
                            radius: 999.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                }),
            );
        }
        secrets_btn_row = secrets_btn_row.push(
            text(secrets_status_text)
                .size(type_scale::CAPTION)
                .color(secrets_color),
        );

        let problems_n = self.total_problems_count();
        let problems_active = self.drawer_tab == Some(DrawerTab::Problems);
        let problems_color = if problems_active {
            p.accent_fg
        } else if problems_n > 0 {
            p.warning_fg
        } else {
            p.text_secondary
        };

        right = right
            .push(tip(
                button(
                    row![
                        icons::alert_triangle(11.0, problems_color),
                        text(format!("Problems ({problems_n})"))
                            .size(type_scale::CAPTION)
                            .color(problems_color),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([2, 7])
                .style(theme::vscode_status_item(p))
                .on_press(Message::ToggleDrawer(DrawerTab::Problems)),
                "Workspace Problems, Conflicts & Hygiene Checks",
                tooltip::Position::Top,
                p,
            ))
            .push(tip(
                button(
                    row![
                        icons::search(12.0, search_color),
                        text("Search").size(type_scale::CAPTION).color(search_color),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([2, 7])
                .style(theme::vscode_status_item(p))
                .on_press(Message::ToggleDrawer(DrawerTab::Search)),
                "Fleet Search across workspace (Ctrl+Shift+F)",
                tooltip::Position::Top,
                p,
            ))
            .push(tip(
                button(secrets_btn_row)
                    .padding([2, 7])
                    .style(theme::vscode_status_item(p))
                    .on_press(Message::ToggleDrawer(DrawerTab::Secrets)),
                secrets_tip,
                tooltip::Position::Top,
                p,
            ));

        if encrypt_needed > 0 && !self.vault_summary.vault_locked {
            let encrypt_tip = format!(
                "Encrypt {} modified secret file(s) ({}) into profile '{active_profile}' (.gitfleet/{vault_fname})",
                encrypt_needed,
                encrypt_paths.join(", ")
            );
            right = right.push(tip(
                button(
                    row![
                        icons::lock(11.0, Color::WHITE),
                        text("Encrypt").size(type_scale::BADGE).color(Color::WHITE),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([2, 8])
                .style(theme::primary_btn(p))
                .on_press(Message::EncryptRepoEnv),
                encrypt_tip,
                tooltip::Position::Top,
                p,
            ));
        } else if restore_needed > 0 && !self.vault_summary.vault_locked {
            let restore_tip = format!(
                "Restore {} missing secret file(s) ({}) from profile '{active_profile}' (.gitfleet/{vault_fname})",
                restore_needed,
                restore_paths.join(", ")
            );
            right = right.push(tip(
                button(
                    row![
                        icons::pull(11.0, p.accent_fg),
                        text("Restore").size(type_scale::BADGE).color(p.accent_fg),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([2, 8])
                .style(theme::subtle_btn(p))
                .on_press(Message::RestoreRepoEnv),
                restore_tip,
                tooltip::Position::Top,
                p,
            ));
        }

        right = right
            .push(tip(
                button(
                    row![
                        icons::terminal(12.0, term_color),
                        text(active_shell)
                            .size(type_scale::CAPTION)
                            .color(term_color),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([2, 7])
                .style(theme::vscode_status_item(p))
                .on_press(Message::ToggleDrawer(DrawerTab::Terminal)),
                "Embedded PTY Terminal (Ctrl+`)",
                tooltip::Position::Top,
                p,
            ))
            .push(tip(
                button(
                    text(if log_count > 0 {
                        format!("Output ({log_count})")
                    } else {
                        "Output".into()
                    })
                    .size(type_scale::CAPTION)
                    .color(log_color),
                )
                .padding([2, 7])
                .style(theme::vscode_status_item(p))
                .on_press(Message::ToggleDrawer(DrawerTab::CommandLog)),
                "Git Command Output Log",
                tooltip::Position::Top,
                p,
            ));

        column![
            horizontal_rule(1).style(theme::subtle_rule(p)),
            container(
                row![left, Space::with_width(Length::Fill), right]
                    .height(Length::Fill)
                    .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fixed(STATUS_BAR_HEIGHT - 1.0))
            .style(theme::vscode_status_bar(p)),
        ]
        .width(Length::Fill)
        .height(Length::Fixed(STATUS_BAR_HEIGHT))
        .into()
    }
}
