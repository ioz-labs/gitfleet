//! Software Update modal: displays current -> latest version, detected install channel,
//! Markdown release notes, live SHA-256 download/verify progress, unsaved-files safeguard,
//! and platform-appropriate update actions.

use crate::app::{GitFleetApp, Message, UpdateUiStatus};
use crate::icons;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use crate::views::slim_scroll;
use gitfleet_app::updater::{
    InstallChannel, RELEASES_PAGE_URL, UpdateDownloadPhase, current_app_version,
};
use iced::widget::{
    Space, button, column, container, horizontal_rule, markdown, opaque, row, text,
};
use iced::{Alignment, Background, Border, Element, Length};

fn format_bytes_human(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    const KIB: f64 = 1024.0;
    if bytes as f64 >= MIB {
        format!("{:.1} MB", bytes as f64 / MIB)
    } else if bytes as f64 >= KIB {
        format!("{:.0} KB", bytes as f64 / KIB)
    } else {
        format!("{bytes} B")
    }
}

fn format_published_date(raw: Option<&str>) -> Option<String> {
    let s = raw?;
    // ISO-8601 `YYYY-MM-DDTHH:MM:SSZ` -> clean `YYYY-MM-DD`
    s.split('T').next().filter(|d| d.len() == 10).map(str::to_string)
}

impl GitFleetApp {
    pub(crate) fn view_update_modal(&self, p: Palette) -> Element<'_, Message> {
        let current_ver = current_app_version();
        let channel = self.service.updater.channel();
        let settings = self.service.settings();
        let is_applying = matches!(self.update_status, UpdateUiStatus::Applying { .. });

        let (header_icon, title_str, subtitle_str) = match &self.update_status {
            UpdateUiStatus::Available { release }
            | UpdateUiStatus::Applying { release, .. }
            | UpdateUiStatus::AppliedManual { release, .. } => (
                icons::sparkle(icon_size::MD, p.accent_fg),
                format!("GitFleet v{} Available", release.version),
                format!("Installed: v{current_ver}  ·  {}", channel.label()),
            ),
            UpdateUiStatus::Checking => (
                icons::refresh(icon_size::MD, p.accent_fg),
                "Checking for Updates…".to_string(),
                format!("Current version: v{current_ver}  ·  {}", channel.label()),
            ),
            UpdateUiStatus::UpToDate => (
                icons::check(icon_size::MD, p.success_fg),
                "GitFleet is Up to Date".to_string(),
                format!("v{current_ver} is the latest release  ·  {}", channel.label()),
            ),
            UpdateUiStatus::Error(_) => (
                icons::alert_triangle(icon_size::MD, p.danger_fg),
                "Update Check Failed".to_string(),
                format!("Current version: v{current_ver}  ·  {}", channel.label()),
            ),
            UpdateUiStatus::Idle => (
                icons::sparkle(icon_size::MD, p.accent_fg),
                "Software Updates".to_string(),
                format!("Current version: v{current_ver}  ·  {}", channel.label()),
            ),
        };

        let mut header = row![
            container(header_icon)
                .padding(7)
                .style(theme::pill_box(p.fill_selected, p.accent_fg, p.border_default)),
            column![
                text(title_str).size(type_scale::TITLE).color(p.text_primary),
                text(subtitle_str)
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
            ]
            .spacing(2),
            Space::with_width(Length::Fill),
        ]
        .spacing(space::MD)
        .align_y(Alignment::Center);

        if !is_applying {
            header = header.push(
                button(icons::close_x(icon_size::SM, p.text_secondary))
                    .padding(pad::BTN_ICON)
                    .style(theme::vscode_icon_btn(p, false))
                    .on_press(Message::CloseUpdateModal),
            );
        }

        let mut body = column![header, horizontal_rule(1).style(theme::subtle_rule(p))]
            .spacing(space::MD)
            .width(Length::Fill);

        match &self.update_status {
            UpdateUiStatus::Idle | UpdateUiStatus::Checking => {
                body = body.push(
                    container(
                        row![
                            icons::refresh(15.0, p.accent_fg),
                            text("Contacting GitHub Releases (ioz-labs/gitfleet)…")
                                .size(type_scale::BODY)
                                .color(p.text_secondary),
                        ]
                        .spacing(space::SM)
                        .align_y(Alignment::Center),
                    )
                    .padding([18, 14])
                    .width(Length::Fill)
                    .style(theme::section_panel_box(p)),
                );
            }
            UpdateUiStatus::UpToDate => {
                body = body.push(
                    container(
                        column![
                            row![
                                icons::check(15.0, p.success_fg),
                                text(format!(
                                    "You are running the latest version of GitFleet (v{current_ver})."
                                ))
                                .size(type_scale::BODY)
                                .color(p.text_primary),
                            ]
                            .spacing(space::SM)
                            .align_y(Alignment::Center),
                            text("Automatic startup checks use zero-quota ETag caching (304 Not Modified).")
                                .size(type_scale::CAPTION)
                                .color(p.text_muted),
                        ]
                        .spacing(6),
                    )
                    .padding([16, 14])
                    .width(Length::Fill)
                    .style(theme::section_panel_box(p)),
                );
            }
            UpdateUiStatus::Error(err) => {
                body = body.push(
                    container(
                        column![
                            text("Could not complete the update operation:")
                                .size(type_scale::CAPTION)
                                .color(p.danger_fg),
                            text(err.clone())
                                .size(type_scale::BODY)
                                .font(theme::CODE_FONT)
                                .color(p.text_primary),
                        ]
                        .spacing(6),
                    )
                    .padding([14, 14])
                    .width(Length::Fill)
                    .style(theme::pill_box(p.danger_bg, p.danger_fg, p.danger_fg)),
                );
            }
            UpdateUiStatus::Available { release }
            | UpdateUiStatus::Applying { release, .. }
            | UpdateUiStatus::AppliedManual { release, .. } => {
                // Metadata strip: version arrow, date, asset size, SHA-256 badge
                let mut meta = row![
                    theme::badge(
                        format!("v{current_ver} → v{}", release.version),
                        p.fill_selected,
                        p.accent_fg,
                        p.border_default,
                        true,
                    ),
                    theme::badge(
                        channel.label(),
                        p.surface_card,
                        p.text_secondary,
                        p.border_subtle,
                        false,
                    ),
                ]
                .spacing(6)
                .align_y(Alignment::Center);

                if let Some(date) = format_published_date(release.published_at.as_deref()) {
                    meta = meta.push(
                        text(date)
                            .size(type_scale::CAPTION)
                            .color(p.text_muted)
                            .wrapping(text::Wrapping::None),
                    );
                }

                if let Some(asset) = &release.asset {
                    meta = meta.push(
                        text(format!("·  {}", format_bytes_human(asset.size_bytes)))
                            .size(type_scale::CAPTION)
                            .font(theme::CODE_FONT)
                            .color(p.text_muted)
                            .wrapping(text::Wrapping::None),
                    );
                }

                if release.checksums_url.is_some() {
                    meta = meta.push(Space::with_width(Length::Fill)).push(theme::badge(
                        "SHA-256 Verified",
                        p.success_bg,
                        p.success_fg,
                        p.border_subtle,
                        false,
                    ));
                }

                body = body.push(meta);

                // Release Notes card (native Markdown rendering)
                let notes_view: Element<'_, Message> = if self.update_release_markdown.is_empty() {
                    text("No release notes published for this version.")
                        .size(type_scale::BODY)
                        .color(p.text_muted)
                        .into()
                } else {
                    let md_style = markdown::Style::from_palette(self.theme_mode.iced_theme().palette());
                    markdown::view(
                        &self.update_release_markdown,
                        markdown::Settings::default(),
                        md_style,
                    )
                    .map(|url| Message::OpenExternalUrl(url.to_string()))
                };

                body = body.push(
                    container(
                        slim_scroll(
                            container(notes_view).padding([12, 16]).width(Length::Fill),
                            p,
                        )
                        .height(Length::Shrink),
                    )
                    .width(Length::Fill)
                    .max_height(260.0)
                    .style(move |_| container::Style {
                        background: Some(Background::Color(p.surface_input)),
                        border: Border {
                            color: p.border_subtle,
                            width: 1.0,
                            radius: 6.0.into(),
                        },
                        ..Default::default()
                    }),
                );

                // Package-manager command box on Arch Linux pacman
                if let InstallChannel::LinuxPacman = channel {
                    let cmd = "sudo pacman -Syu".to_string();
                    body = body.push(
                        container(
                            column![
                                text("GitFleet is managed by pacman on this system. Upgrade with:")
                                    .size(type_scale::CAPTION)
                                    .color(p.text_secondary),
                                row![
                                    container(
                                        text(cmd.clone())
                                            .size(type_scale::CODE)
                                            .font(theme::CODE_FONT)
                                            .color(p.accent_fg),
                                    )
                                    .padding([6, 10])
                                    .width(Length::Fill)
                                    .style(theme::section_panel_box(p)),
                                    button(
                                        row![
                                            icons::copy(12.0, p.text_primary),
                                            text("Copy").size(type_scale::CAPTION),
                                        ]
                                        .spacing(5)
                                        .align_y(Alignment::Center),
                                    )
                                    .padding([6, 10])
                                    .style(theme::subtle_btn(p))
                                    .on_press(Message::CopyText(
                                        "pacman upgrade command".into(),
                                        cmd,
                                    )),
                                ]
                                .spacing(space::SM)
                                .align_y(Alignment::Center),
                            ]
                            .spacing(6),
                        )
                        .padding([10, 12])
                        .width(Length::Fill)
                        .style(theme::section_panel_box(p)),
                    );
                }

                // Applying progress bar (Downloading / Verifying SHA-256 / Installing)
                if let UpdateUiStatus::Applying {
                    phase,
                    downloaded_bytes,
                    total_bytes,
                    ..
                } = &self.update_status
                {
                    let fraction = if *phase == UpdateDownloadPhase::Downloading {
                        if *total_bytes > 0 {
                            (*downloaded_bytes as f32 / *total_bytes as f32).clamp(0.0, 1.0)
                        } else {
                            0.12
                        }
                    } else {
                        1.0
                    };
                    let pct = (fraction * 100.0).round() as u32;
                    let right_stat = if *phase == UpdateDownloadPhase::Downloading {
                        if *total_bytes > 0 {
                            format!(
                                "{} / {}  ({pct}%)",
                                format_bytes_human(*downloaded_bytes),
                                format_bytes_human(*total_bytes)
                            )
                        } else {
                            format_bytes_human(*downloaded_bytes)
                        }
                    } else {
                        "100%".to_string()
                    };

                    let fill_portion = ((fraction * 1000.0).round() as u16).max(1);
                    let rem_portion = 1000u16.saturating_sub(fill_portion);
                    let mut bar_row = row![
                        container(Space::new(Length::Fill, Length::Fixed(6.0)))
                            .width(Length::FillPortion(fill_portion))
                            .style(move |_| container::Style {
                                background: Some(Background::Color(p.accent_primary)),
                                border: Border {
                                    radius: 999.0.into(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            }),
                    ];
                    if rem_portion > 0 {
                        bar_row = bar_row.push(Space::with_width(Length::FillPortion(rem_portion)));
                    }

                    let progress_track = container(bar_row)
                        .width(Length::Fill)
                        .height(Length::Fixed(6.0))
                        .style(move |_| container::Style {
                            background: Some(Background::Color(p.border_subtle)),
                            border: Border {
                                radius: 999.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        });

                    body = body.push(
                        container(
                            column![
                                row![
                                    text(phase.label())
                                        .size(type_scale::CAPTION)
                                        .color(p.accent_fg),
                                    Space::with_width(Length::Fill),
                                    text(right_stat)
                                        .size(type_scale::CAPTION)
                                        .font(theme::CODE_FONT)
                                        .color(p.text_secondary),
                                ]
                                .align_y(Alignment::Center),
                                progress_track,
                            ]
                            .spacing(6),
                        )
                        .padding([10, 12])
                        .width(Length::Fill)
                        .style(theme::section_panel_box(p)),
                    );
                }

                // Applied-manual result card (macOS DMG opened, Linux .deb downloaded, etc.)
                if let UpdateUiStatus::AppliedManual {
                    note,
                    manual_command,
                    artifact_path,
                    ..
                } = &self.update_status
                {
                    let mut info_col = column![
                        row![
                            icons::check(14.0, p.success_fg),
                            text(note.clone())
                                .size(type_scale::BODY)
                                .color(p.text_primary),
                        ]
                        .spacing(6)
                        .align_y(Alignment::Center),
                    ]
                    .spacing(8);

                    if let Some(cmd) = manual_command {
                        info_col = info_col.push(
                            row![
                                container(
                                    text(cmd.clone())
                                        .size(type_scale::CODE)
                                        .font(theme::CODE_FONT)
                                        .color(p.accent_fg),
                                )
                                .padding([6, 10])
                                .width(Length::Fill)
                                .style(theme::section_panel_box(p)),
                                button(
                                    row![
                                        icons::copy(12.0, p.text_primary),
                                        text("Copy").size(type_scale::CAPTION),
                                    ]
                                    .spacing(5)
                                    .align_y(Alignment::Center),
                                )
                                .padding([6, 10])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::CopyText("upgrade command".into(), cmd.clone())),
                            ]
                            .spacing(space::SM)
                            .align_y(Alignment::Center),
                        );
                    }

                    if let Some(path) = artifact_path {
                        info_col = info_col.push(
                            button(
                                row![
                                    icons::folder_open(13.0, p.text_primary),
                                    text("Show Downloaded File").size(type_scale::CAPTION),
                                ]
                                .spacing(6)
                                .align_y(Alignment::Center),
                            )
                            .padding([5, 10])
                            .style(theme::subtle_btn(p))
                            .on_press(Message::RevealUpdateArtifact(path.clone())),
                        );
                    }

                    body = body.push(
                        container(info_col)
                            .padding([10, 12])
                            .width(Length::Fill)
                            .style(theme::pill_box(p.success_bg, p.success_fg, p.border_subtle)),
                    );
                }

                // Unsaved-editor safeguard banner before auto-restart
                if self.update_confirm_dirty {
                    let dirty_list = self.unsaved_editor_names();
                    let list_str = if dirty_list.is_empty() {
                        "Unsaved changes detected.".to_string()
                    } else {
                        format!("Unsaved file(s): {}", dirty_list.join(", "))
                    };
                    body = body.push(
                        container(
                            column![
                                row![
                                    icons::alert_triangle(14.0, p.warning_fg),
                                    text("Unsaved editor changes will be lost on restart")
                                        .size(type_scale::BODY)
                                        .color(p.warning_fg),
                                ]
                                .spacing(6)
                                .align_y(Alignment::Center),
                                text(list_str)
                                    .size(type_scale::CAPTION)
                                    .font(theme::CODE_FONT)
                                    .color(p.text_secondary),
                                row![
                                    button(text("Cancel & Save First").size(type_scale::CAPTION))
                                        .padding([5, 10])
                                        .style(theme::subtle_btn(p))
                                        .on_press(Message::CloseUpdateModal),
                                    button(text("Update & Restart Anyway").size(type_scale::CAPTION))
                                        .padding([5, 10])
                                        .style(theme::danger_btn(p))
                                        .on_press(Message::StartApplyUpdate { force_dirty: true }),
                                ]
                                .spacing(space::SM),
                            ]
                            .spacing(6),
                        )
                        .padding([10, 12])
                        .width(Length::Fill)
                        .style(theme::pill_box(p.warning_bg, p.warning_fg, p.border_default)),
                    );
                }
            }
        }

        // Footer actions
        let mut footer = row![].spacing(space::SM).align_y(Alignment::Center);

        match &self.update_status {
            UpdateUiStatus::Checking => {
                footer = footer.push(Space::with_width(Length::Fill)).push(
                    button(text("Close").size(type_scale::BODY))
                        .padding(pad::BTN_MD)
                        .style(theme::subtle_btn(p))
                        .on_press(Message::CloseUpdateModal),
                );
            }
            UpdateUiStatus::Idle | UpdateUiStatus::UpToDate | UpdateUiStatus::Error(_) => {
                if settings.skipped_update_version.is_some() {
                    footer = footer.push(
                        button(text("Reset Skipped Version").size(type_scale::CAPTION))
                            .padding(pad::BTN_SM)
                            .style(theme::subtle_btn(p))
                            .on_press(Message::ClearSkippedUpdate),
                    );
                }
                footer = footer
                    .push(
                        button(text("Releases Page").size(type_scale::CAPTION))
                            .padding(pad::BTN_SM)
                            .style(theme::subtle_btn(p))
                            .on_press(Message::OpenExternalUrl(RELEASES_PAGE_URL.to_string())),
                    )
                    .push(Space::with_width(Length::Fill))
                    .push(
                        button(text("Check Again").size(type_scale::BODY))
                            .padding(pad::BTN_MD)
                            .style(theme::primary_btn(p))
                            .on_press(Message::CheckForUpdates { manual: true }),
                    )
                    .push(
                        button(text("Close").size(type_scale::BODY))
                            .padding(pad::BTN_MD)
                            .style(theme::subtle_btn(p))
                            .on_press(Message::CloseUpdateModal),
                    );
            }
            UpdateUiStatus::Applying { phase, .. } => {
                let can_cancel = *phase == UpdateDownloadPhase::Downloading;
                footer = footer.push(Space::with_width(Length::Fill)).push(
                    button(text("Cancel Download").size(type_scale::BODY))
                        .padding(pad::BTN_MD)
                        .style(theme::danger_btn(p))
                        .on_press_maybe(can_cancel.then_some(Message::CancelUpdateDownload)),
                );
            }
            UpdateUiStatus::Available { release } => {
                let release_url = release.html_url.clone();
                let primary_msg = if matches!(channel, InstallChannel::LinuxPacman) {
                    Message::CopyText("pacman upgrade command".into(), "sudo pacman -Syu".into())
                } else if release.asset.is_some() {
                    Message::StartApplyUpdate { force_dirty: false }
                } else {
                    Message::OpenExternalUrl(release_url.clone())
                };
                let primary_label = if release.asset.is_some() || matches!(channel, InstallChannel::LinuxPacman) {
                    channel.primary_action_label()
                } else {
                    "Open Release Page"
                };

                footer = footer
                    .push(
                        button(text("Skip This Version").size(type_scale::CAPTION))
                            .padding(pad::BTN_SM)
                            .style(theme::subtle_btn(p))
                            .on_press(Message::SkipAvailableUpdate),
                    )
                    .push(
                        button(text("Release Page").size(type_scale::CAPTION))
                            .padding(pad::BTN_SM)
                            .style(theme::subtle_btn(p))
                            .on_press(Message::OpenExternalUrl(release_url)),
                    )
                    .push(Space::with_width(Length::Fill))
                    .push(
                        button(text("Later").size(type_scale::BODY))
                            .padding(pad::BTN_MD)
                            .style(theme::subtle_btn(p))
                            .on_press(Message::CloseUpdateModal),
                    )
                    .push(
                        button(text(primary_label).size(type_scale::BODY))
                            .padding(pad::BTN_MD)
                            .style(theme::primary_btn(p))
                            .on_press(primary_msg),
                    );
            }
            UpdateUiStatus::AppliedManual { release, .. } => {
                footer = footer
                    .push(
                        button(text("Release Page").size(type_scale::CAPTION))
                            .padding(pad::BTN_SM)
                            .style(theme::subtle_btn(p))
                            .on_press(Message::OpenExternalUrl(release.html_url.clone())),
                    )
                    .push(Space::with_width(Length::Fill))
                    .push(
                        button(text("Done").size(type_scale::BODY))
                            .padding(pad::BTN_MD)
                            .style(theme::primary_btn(p))
                            .on_press(Message::CloseUpdateModal),
                    );
            }
        }

        body = body
            .push(horizontal_rule(1).style(theme::subtle_rule(p)))
            .push(footer);

        let modal = opaque(
            container(body)
                .width(Length::Fixed(layout::MODAL_MD))
                .max_height(layout::MODAL_MAX_HEIGHT)
                .padding(pad::MODAL)
                .style(theme::modal_box(p)),
        );

        let centered = container(modal)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(theme::backdrop_box);

        theme::overlay_backdrop(centered, Message::CloseUpdateModal)
    }
}
