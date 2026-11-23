//! `F0` — Portable Group / Workspace Blueprint Export & Batch Auto-Clone Import UI (`Group = Folder`).

use crate::app::{DrawerTab, GitFleetApp, Message};
use crate::icons;
use crate::theme::kit::{
    ButtonSize, ButtonVariant, ModalShell, ModalSize, SegmentItem, UiButton, segmented_control,
    status_pill,
};
use crate::theme::{self, Palette, icon_size, pad, space, type_scale};
use crate::views::{slim_scroll, truncate_chars};
use gitfleet_core::RepoId;
use gitfleet_git::{CloneProgressSnapshot, CloneProgressTracker};
use gitfleet_workspace::{
    BlueprintRemote, BlueprintRepo, GroupBlueprint, UrlProtocolMode, compute_export_group_path,
    resolve_materialized_repo_dest, rewrite_git_url_protocol,
};
use iced::widget::{Space, checkbox, column, container, horizontal_rule, row, text, text_input};
use iced::{Alignment, Background, Border, Element, Length};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

#[derive(Debug, Clone)]
pub(crate) struct BlueprintExportRepoItem {
    #[allow(dead_code)]
    pub(crate) repo_id: RepoId,
    pub(crate) name: String,
    pub(crate) folder_name: String,
    pub(crate) raw_group: Option<String>,
    pub(crate) origin_url: Option<String>,
    pub(crate) extra_remotes: Vec<BlueprintRemote>,
    pub(crate) branch: Option<String>,
    pub(crate) has_submodules: bool,
    pub(crate) has_secrets_vault: bool,
    pub(crate) pinned: bool,
    pub(crate) selected: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct BlueprintExportModalState {
    pub(crate) title: String,
    pub(crate) selected_group: Option<String>,
    pub(crate) relative_to_selected_group: bool,
    pub(crate) loading: bool,
    pub(crate) groups: Vec<String>,
    pub(crate) items: Vec<BlueprintExportRepoItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BlueprintItemStatus {
    ReadyToClone,
    ExistingGitLink,
    FolderConflict(String),
    Cloning,
    Completed { linked_existing: bool },
    Failed(String),
    Skipped,
}

#[derive(Debug, Clone)]
pub(crate) struct BlueprintImportItem {
    pub(crate) repo: BlueprintRepo,
    pub(crate) selected: bool,
    pub(crate) status: BlueprintItemStatus,
    pub(crate) tracker: Option<Arc<CloneProgressTracker>>,
    pub(crate) snapshot: Option<CloneProgressSnapshot>,
}

#[derive(Debug, Clone)]
pub(crate) struct BlueprintImportModalState {
    #[allow(dead_code)]
    pub(crate) source_file: PathBuf,
    pub(crate) blueprint: GroupBlueprint,
    pub(crate) target_base_dir: String,
    pub(crate) parent_group_prefix: String,
    pub(crate) protocol_mode: UrlProtocolMode,
    pub(crate) recurse_submodules: bool,
    pub(crate) items: Vec<BlueprintImportItem>,
    pub(crate) running: bool,
    pub(crate) finished_once: bool,
    pub(crate) cancel_flag: Option<Arc<AtomicBool>>,
}

impl BlueprintImportModalState {
    pub(crate) fn refresh_preflight(&mut self) {
        if self.running {
            return;
        }
        let base_str = self.target_base_dir.trim();
        let base_path = Path::new(base_str);
        let prefix = {
            let t = self.parent_group_prefix.trim();
            if t.is_empty() { None } else { Some(t) }
        };

        for item in &mut self.items {
            if matches!(item.status, BlueprintItemStatus::Completed { .. }) {
                continue;
            }
            if base_str.is_empty() {
                item.status = BlueprintItemStatus::FolderConflict(
                    "Select a Target Base Directory above".into(),
                );
                continue;
            }
            match resolve_materialized_repo_dest(
                base_path,
                item.repo.group_path.as_deref(),
                prefix,
                &item.repo.folder_name,
            ) {
                Ok((_parent, repo_dir, _grp)) => {
                    if repo_dir.join(".git").exists() {
                        item.status = BlueprintItemStatus::ExistingGitLink;
                    } else if repo_dir.exists() {
                        let non_empty = std::fs::read_dir(&repo_dir)
                            .ok()
                            .and_then(|mut it| it.next())
                            .is_some();
                        if non_empty {
                            item.status = BlueprintItemStatus::FolderConflict(
                                "Target folder exists & is not a Git repo".into(),
                            );
                        } else {
                            item.status = BlueprintItemStatus::ReadyToClone;
                        }
                    } else {
                        item.status = BlueprintItemStatus::ReadyToClone;
                    }
                }
                Err(err) => {
                    item.status = BlueprintItemStatus::FolderConflict(err);
                }
            }
        }
    }
}

impl GitFleetApp {
    pub(crate) fn view_blueprint_export_modal(
        &self,
        p: Palette,
        modal: &BlueprintExportModalState,
    ) -> Element<'_, Message> {
        let valid_selected = modal
            .items
            .iter()
            .filter(|i| i.selected && i.origin_url.is_some())
            .count();
        let missing_remote_cnt = modal.items.iter().filter(|i| i.origin_url.is_none()).count();
        let vault_cnt = modal
            .items
            .iter()
            .filter(|i| i.selected && i.has_secrets_vault)
            .count();

        let has_nested_group = modal
            .selected_group
            .as_deref()
            .is_some_and(|g| g.contains('/'));

        let mut top_controls = column![].spacing(space::SM);
        if let Some(sel_grp) = &modal.selected_group
            && has_nested_group
        {
            let leaf = sel_grp.rsplit('/').next().unwrap_or(sel_grp);
            let mode_row = row![
                text("Exported Folder Structure:")
                    .size(type_scale::CAPTION)
                    .color(p.text_secondary),
                Space::with_width(Length::Fill),
                segmented_control(
                    p,
                    [
                        SegmentItem::new(false, "Full Hierarchy"),
                        SegmentItem::new(true, "From Selected Group"),
                    ],
                    modal.relative_to_selected_group,
                    Message::SetBlueprintExportRelative,
                ),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center);

            let example_path = if modal.relative_to_selected_group {
                format!("<TargetDir>/{leaf}/<repo>")
            } else {
                format!("<TargetDir>/{sel_grp}/<repo>")
            };
            top_controls = top_controls.push(mode_row).push(
                text(format!("Destination preview on target machine: {example_path}"))
                    .size(type_scale::CAPTION)
                    .font(theme::CODE_FONT)
                    .color(p.accent_fg),
            );
        }

        if missing_remote_cnt > 0 {
            top_controls = top_controls.push(
                container(
                    row![
                        icons::alert_triangle(icon_size::XS, p.warning_fg),
                        text(format!(
                            "{missing_remote_cnt} local repository(s) have no Git remote URL and cannot be auto-cloned on another machine."
                        ))
                        .size(type_scale::CAPTION)
                        .color(p.warning_fg),
                    ]
                    .spacing(space::XS)
                    .align_y(Alignment::Center),
                )
                .padding([6, 10])
                .width(Length::Fill)
                .style(theme::pill_box(p.warning_bg, p.warning_fg, p.border_default)),
            );
        }

        let mut list_col = column![].spacing(4);
        if modal.loading {
            list_col = list_col.push(
                text("Inspecting repository remotes, active branches, submodules, and Secrets Vaults…")
                    .size(type_scale::BODY)
                    .color(p.text_muted),
            );
        } else {
            for (idx, item) in modal.items.iter().enumerate() {
                let effective_group = compute_export_group_path(
                    item.raw_group.as_deref(),
                    modal.selected_group.as_deref(),
                    modal.relative_to_selected_group,
                );
                let rel_dest = match &effective_group {
                    Some(g) => format!("{g}/{}", item.folder_name),
                    None => item.folder_name.clone(),
                };
                let has_origin = item.origin_url.is_some();

                let mut badges = row![].spacing(4).align_y(Alignment::Center);
                if let Some(br) = &item.branch {
                    badges = badges.push(status_pill(br.clone(), p.accent_fg, p.fill_selected));
                }
                if item.has_submodules {
                    badges = badges.push(status_pill("submodules", p.violet_fg, p.surface_card));
                }
                if item.has_secrets_vault {
                    badges = badges.push(status_pill("vault.age", p.success_fg, p.success_bg));
                }
                if !has_origin {
                    badges = badges.push(status_pill("no remote", p.warning_fg, p.warning_bg));
                }

                let url_line = item
                    .origin_url
                    .as_deref()
                    .unwrap_or("Local-only repository (add 'origin' remote to include)");

                let item_row = container(
                    row![
                        checkbox("", item.selected && has_origin)
                            .size(14)
                            .style(theme::tree_checkbox(p))
                            .on_toggle_maybe(
                                has_origin.then_some(move |v| Message::ToggleBlueprintExportRepo(idx, v))
                            ),
                        column![
                            row![
                                icons::folder(icon_size::XS, p.accent_fg),
                                text(rel_dest)
                                    .size(type_scale::BODY)
                                    .font(theme::CODE_FONT)
                                    .color(if has_origin { p.text_primary } else { p.text_muted })
                                    .width(Length::Fill),
                                badges,
                            ]
                            .spacing(space::XS)
                            .align_y(Alignment::Center),
                            text(truncate_chars(url_line, 78))
                                .size(type_scale::CAPTION)
                                .font(theme::CODE_FONT)
                                .color(p.text_muted),
                        ]
                        .spacing(2)
                        .width(Length::Fill),
                    ]
                    .spacing(space::SM)
                    .align_y(Alignment::Center),
                )
                .padding([6, 10])
                .width(Length::Fill)
                .style(theme::card_box(p));

                list_col = list_col.push(item_row);
            }
        }

        let summary_line = format!(
            "{valid_selected} repo(s) ready · {} group(s){}",
            modal.groups.len(),
            if vault_cnt > 0 {
                format!(" · {vault_cnt} with encrypted Secrets Vault")
            } else {
                String::new()
            }
        );

        let body = column![
            top_controls,
            row![
                text(summary_line)
                    .size(type_scale::CAPTION)
                    .color(p.text_secondary)
                    .width(Length::Fill),
            ],
            container(slim_scroll(list_col, p).height(Length::Fixed(310.0))).width(Length::Fill),
        ]
        .spacing(space::SM);

        let footer = row![
            UiButton::new(p, "Cancel")
                .variant(ButtonVariant::Subtle)
                .size(ButtonSize::Md)
                .on_press(Message::CloseOverlays),
            UiButton::new(p, format!("Export Blueprint ({valid_selected})…"))
                .variant(ButtonVariant::Primary)
                .size(ButtonSize::Md)
                .icon(icons::save(icon_size::XS, theme::on_accent_ink(p.accent_primary)))
                .on_press_maybe((!modal.loading && valid_selected > 0).then_some(Message::PickSaveBlueprintFile)),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center);

        ModalShell::new(p, modal.title.clone(), body)
            .subtitle("Export a portable .gitfleet-pack.json manifest preserving exact Group = Folder hierarchy.")
            .icon(Element::from(icons::repo_stack(icon_size::MD, p.accent_fg)))
            .size(ModalSize::Lg)
            .scrollable_body(false)
            .footer(footer)
            .on_close(Message::CloseOverlays)
            .into_element()
    }

    pub(crate) fn view_blueprint_import_modal(
        &self,
        p: Palette,
        modal: &BlueprintImportModalState,
    ) -> Element<'_, Message> {
        let base_str = modal.target_base_dir.trim();
        let base_path = Path::new(base_str);
        let prefix = {
            let t = modal.parent_group_prefix.trim();
            if t.is_empty() { None } else { Some(t) }
        };

        let selected_ready_cnt = modal
            .items
            .iter()
            .filter(|i| {
                i.selected
                    && matches!(
                        i.status,
                        BlueprintItemStatus::ReadyToClone | BlueprintItemStatus::ExistingGitLink
                    )
            })
            .count();
        let completed_cnt = modal
            .items
            .iter()
            .filter(|i| matches!(i.status, BlueprintItemStatus::Completed { .. }))
            .count();
        let failed_cnt = modal
            .items
            .iter()
            .filter(|i| matches!(i.status, BlueprintItemStatus::Failed(_)))
            .count();
        let vault_completed_cnt = modal
            .items
            .iter()
            .filter(|i| {
                i.repo.has_secrets_vault
                    && matches!(i.status, BlueprintItemStatus::Completed { .. })
            })
            .count();

        let target_dir_row = column![
            text("Target Base Directory (Group = Subfolder inside this path)")
                .size(type_scale::CAPTION)
                .font(theme::UI_FONT_MEDIUM)
                .color(p.text_secondary),
            row![
                text_input("e.g. D:\\Projects or ~/work…", &modal.target_base_dir)
                    .on_input_maybe((!modal.running).then_some(Message::BlueprintTargetDirChanged))
                    .padding(pad::INPUT_COMPACT)
                    .size(type_scale::BODY)
                    .font(theme::CODE_FONT)
                    .width(Length::Fill)
                    .style(theme::dark_input(p)),
                UiButton::new(p, "Browse…")
                    .variant(ButtonVariant::Secondary)
                    .size(ButtonSize::Sm)
                    .icon(icons::folder_open(icon_size::XS, p.text_secondary))
                    .on_press_maybe((!modal.running).then_some(Message::PickBlueprintTargetDir)),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        ]
        .spacing(4);

        let options_row = row![
            column![
                text("Optional Parent Group Prefix")
                    .size(type_scale::BADGE)
                    .color(p.text_muted),
                text_input("Root (or e.g. Work/TeamA)…", &modal.parent_group_prefix)
                    .on_input_maybe((!modal.running).then_some(Message::BlueprintParentPrefixChanged))
                    .padding(pad::INPUT_COMPACT)
                    .size(type_scale::CAPTION)
                    .width(Length::Fixed(210.0))
                    .style(theme::dark_input(p)),
            ]
            .spacing(3),
            column![
                text("Remote URL Protocol")
                    .size(type_scale::BADGE)
                    .color(p.text_muted),
                segmented_control(
                    p,
                    [
                        SegmentItem::new(UrlProtocolMode::KeepOriginal, "Original"),
                        SegmentItem::new(UrlProtocolMode::ForceHttps, "HTTPS"),
                        SegmentItem::new(UrlProtocolMode::ForceSsh, "SSH"),
                    ],
                    modal.protocol_mode,
                    Message::SetBlueprintProtocolMode,
                ),
            ]
            .spacing(3),
            Space::with_width(Length::Fill),
            checkbox("Recurse submodules", modal.recurse_submodules)
                .size(14)
                .text_size(type_scale::CAPTION)
                .style(theme::tree_checkbox(p))
                .on_toggle_maybe((!modal.running).then_some(Message::ToggleBlueprintSubmodules)),
        ]
        .spacing(space::MD)
        .align_y(Alignment::End);

        let mut list_col = column![].spacing(5);
        for (idx, item) in modal.items.iter().enumerate() {
            let rewritten_url = rewrite_git_url_protocol(&item.repo.origin_url, modal.protocol_mode);
            let resolved = resolve_materialized_repo_dest(
                base_path,
                item.repo.group_path.as_deref(),
                prefix,
                &item.repo.folder_name,
            );
            let (dest_str, group_badge_str) = match &resolved {
                Ok((_parent, repo_dir, grp)) => (
                    repo_dir.display().to_string(),
                    grp.clone().unwrap_or_else(|| "(root)".into()),
                ),
                Err(_) => (item.repo.folder_name.clone(), "invalid".into()),
            };

            let status_badge: Element<'_, Message> = match &item.status {
                BlueprintItemStatus::ReadyToClone => {
                    status_pill("Ready to Clone", p.accent_fg, p.fill_selected)
                }
                BlueprintItemStatus::ExistingGitLink => {
                    status_pill("Existing .git -> Link & Fetch", p.success_fg, p.success_bg)
                }
                BlueprintItemStatus::FolderConflict(_) => {
                    status_pill("Conflict", p.warning_fg, p.warning_bg)
                }
                BlueprintItemStatus::Cloning => {
                    let pct = item
                        .snapshot
                        .as_ref()
                        .and_then(|s| s.stage_percent.or(Some(s.overall_percent)))
                        .unwrap_or(5);
                    let phase = item
                        .snapshot
                        .as_ref()
                        .map(|s| s.phase.short_label())
                        .unwrap_or("Cloning");
                    status_pill(format!("{phase} {pct}%"), p.accent_fg, p.fill_selected)
                }
                BlueprintItemStatus::Completed { linked_existing } => status_pill(
                    if *linked_existing { "Linked & Fetched" } else { "Cloned" },
                    p.success_fg,
                    p.success_bg,
                ),
                BlueprintItemStatus::Failed(_) => status_pill("Failed", p.danger_fg, p.danger_bg),
                BlueprintItemStatus::Skipped => status_pill("Skipped", p.text_muted, p.surface_panel),
            };

            let mut detail_col = column![
                row![
                    icons::folder(icon_size::XS, p.accent_fg),
                    text(truncate_chars(&dest_str, 68))
                        .size(type_scale::BODY)
                        .font(theme::CODE_FONT)
                        .color(p.text_primary)
                        .width(Length::Fill),
                    status_pill(format!("Group: {group_badge_str}"), p.text_secondary, p.surface_panel),
                    status_badge,
                ]
                .spacing(space::XS)
                .align_y(Alignment::Center),
                row![
                    text(truncate_chars(&rewritten_url, 68))
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(p.text_muted)
                        .width(Length::Fill),
                    if let Some(br) = &item.repo.branch {
                        Element::from(
                            text(format!("branch: {br}"))
                                .size(type_scale::BADGE)
                                .font(theme::CODE_FONT)
                                .color(p.accent_fg),
                        )
                    } else {
                        Space::with_width(Length::Shrink).into()
                    },
                ]
                .spacing(space::XS)
                .align_y(Alignment::Center),
            ]
            .spacing(3)
            .width(Length::Fill);

            if let BlueprintItemStatus::Cloning = &item.status
                && let Some(snap) = &item.snapshot
            {
                let pct = snap.stage_percent.unwrap_or(snap.overall_percent).clamp(3, 100) as u16;
                let rem = 100u16.saturating_sub(pct);
                let mut bar = row![
                    container(Space::new(Length::Fill, 4.0))
                        .width(Length::FillPortion(pct))
                        .style(move |_| container::Style {
                            background: Some(Background::Color(p.accent_primary)),
                            border: Border {
                                radius: 2.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                ];
                if rem > 0 {
                    bar = bar.push(Space::with_width(Length::FillPortion(rem)));
                }
                detail_col = detail_col.push(bar);
                if !snap.transfer_detail.is_empty() {
                    detail_col = detail_col.push(
                        text(snap.transfer_detail.clone())
                            .size(type_scale::BADGE)
                            .font(theme::CODE_FONT)
                            .color(p.text_muted),
                    );
                }
            } else if let BlueprintItemStatus::Failed(err)
            | BlueprintItemStatus::FolderConflict(err) = &item.status
            {
                detail_col = detail_col.push(
                    text(truncate_chars(err, 96))
                        .size(type_scale::CAPTION)
                        .color(p.danger_fg),
                );
            }

            list_col = list_col.push(
                container(
                    row![
                        checkbox("", item.selected)
                            .size(14)
                            .style(theme::tree_checkbox(p))
                            .on_toggle_maybe(
                                (!modal.running)
                                    .then_some(move |v| Message::ToggleBlueprintImportRepo(idx, v))
                            ),
                        detail_col,
                    ]
                    .spacing(space::SM)
                    .align_y(Alignment::Center),
                )
                .padding([7, 10])
                .width(Length::Fill)
                .style(theme::card_box(p)),
            );
        }

        let mut vault_banner: Element<'_, Message> = Space::with_height(0.0).into();
        if vault_completed_cnt > 0 && !modal.running {
            vault_banner = container(
                row![
                    icons::shield(icon_size::SM, p.success_fg),
                    text(format!(
                        "{vault_completed_cnt} cloned repository(s) contain encrypted .gitfleet/secrets.age vaults ready to restore."
                    ))
                    .size(type_scale::CAPTION)
                    .color(p.success_fg)
                    .width(Length::Fill),
                    UiButton::new(p, "Open Secrets Vault")
                        .variant(ButtonVariant::Secondary)
                        .size(ButtonSize::Xs)
                        .on_press(Message::ToggleDrawer(DrawerTab::Secrets)),
                ]
                .spacing(space::SM)
                .align_y(Alignment::Center),
            )
            .padding([6, 10])
            .width(Length::Fill)
            .style(theme::pill_box(p.success_bg, p.success_fg, p.border_default))
            .into();
        }

        let body = column![
            target_dir_row,
            options_row,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            row![
                text(format!(
                    "Blueprint '{}' · {} total · {completed_cnt} completed{}",
                    modal.blueprint.title,
                    modal.items.len(),
                    if failed_cnt > 0 {
                        format!(" · {failed_cnt} failed")
                    } else {
                        String::new()
                    }
                ))
                .size(type_scale::CAPTION)
                .color(p.text_secondary)
                .width(Length::Fill),
            ],
            container(slim_scroll(list_col, p).height(Length::Fixed(270.0))).width(Length::Fill),
            vault_banner,
        ]
        .spacing(space::SM);

        let mut footer = row![].spacing(space::SM).align_y(Alignment::Center);
        if modal.running {
            footer = footer.push(
                UiButton::new(p, "Cancel Batch Clone")
                    .variant(ButtonVariant::Destructive)
                    .size(ButtonSize::Md)
                    .on_press(Message::CancelBlueprintImport),
            );
        } else {
            footer = footer.push(
                UiButton::new(p, if modal.finished_once { "Done" } else { "Cancel" })
                    .variant(ButtonVariant::Subtle)
                    .size(ButtonSize::Md)
                    .on_press(Message::CloseOverlays),
            );
            if failed_cnt > 0 {
                footer = footer.push(
                    UiButton::new(p, format!("Retry Failed ({failed_cnt})"))
                        .variant(ButtonVariant::OutlineAccent)
                        .size(ButtonSize::Md)
                        .icon(icons::refresh(icon_size::XS, p.accent_fg))
                        .on_press(Message::RetryFailedBlueprintImport),
                );
            }
            if selected_ready_cnt > 0 {
                footer = footer.push(
                    UiButton::new(p, format!("Clone & Materialize ({selected_ready_cnt})"))
                        .variant(ButtonVariant::Primary)
                        .size(ButtonSize::Md)
                        .icon(icons::cloud_download(
                            icon_size::XS,
                            theme::on_accent_ink(p.accent_primary),
                        ))
                        .on_press(Message::StartBlueprintImport),
                );
            }
        }

        ModalShell::new(p, "Import Group Blueprint & Auto-Clone", body)
            .subtitle("Creates matching Group = Folder hierarchy inside Target Directory and clones repositories in parallel.")
            .icon(icons::cloud_download(icon_size::MD, p.accent_fg))
            .size(ModalSize::Xl)
            .scrollable_body(false)
            .footer(footer)
            .on_close(Message::CloseOverlays)
            .into_element()
    }
}
