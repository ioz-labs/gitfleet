//! `D6` — Workspace `Problems & Hygiene` Dock Tab: centralizes merge conflicts, staged secrets,
//! oversized files, Secrets Vault drift, and dirty/uninitialized submodules across the workspace.

use crate::app::{BottomViewMode, GitFleetApp, Message};
use crate::icons;
use crate::theme::kit::{ButtonSize, ButtonVariant, UiButton, status_pill};
use crate::theme::{self, Palette, icon_size, pad, space, type_scale};
use crate::views::slim_scroll;
use iced::widget::{Space, column, container, row, text};
use iced::{Alignment, Element, Length};

impl GitFleetApp {
    pub(crate) fn total_problems_count(&self) -> usize {
        let ws_conflicts: usize = self
            .statuses
            .iter()
            .filter(|(id, _)| !Self::is_submodule_repo_id(id))
            .map(|(_, st)| st.conflict_count)
            .sum();
        let secrets_n = self.staged_secrets.len();
        let big_n = self.oversized_files.len();
        let vault_drift = usize::from(self.vault_summary.vault_locked)
            + self.vault_summary.encrypt_needed_count()
            + self.vault_summary.restore_needed_count();
        let sub_issues = self
            .submodules
            .iter()
            .filter(|s| !s.initialized || s.dirty)
            .count()
            + self.orphan_gitlinks.len();
        ws_conflicts + secrets_n + big_n + vault_drift + sub_issues
    }

    pub(crate) fn view_problems_drawer(&self, p: Palette) -> Element<'_, Message> {
        let ws = self.service.workspace.current();
        let mut items = column![].spacing(6).width(Length::Fill);
        let mut count = 0usize;

        // 1. Workspace Merge Conflicts
        for repo in &ws.repos {
            if let Some(st) = self.statuses.get(&repo.id)
                && st.conflict_count > 0
            {
                count += st.conflict_count;
                let is_active = self.selected_repo.as_ref() == Some(&repo.id);
                let action_msg = if is_active {
                    Message::SetBottomViewMode(BottomViewMode::ResolveConflicts)
                } else {
                    Message::SelectRepo(repo.id.clone())
                };
                items = items.push(
                    container(
                        row![
                            icons::alert_triangle(icon_size::SM, p.danger_fg),
                            status_pill("CONFLICT", p.danger_fg, p.danger_bg),
                            text(format!(
                                "{}: {} conflicted file(s) requiring resolution",
                                repo.name, st.conflict_count
                            ))
                            .size(type_scale::BODY)
                            .color(p.text_primary)
                            .width(Length::Fill),
                            UiButton::new(
                                p,
                                if is_active {
                                    "Open Conflict Solver"
                                } else {
                                    "Switch to Repo"
                                },
                            )
                            .variant(ButtonVariant::Destructive)
                            .size(ButtonSize::Xs)
                            .on_press(action_msg),
                        ]
                        .spacing(space::SM)
                        .align_y(Alignment::Center),
                    )
                    .padding([6, 10])
                    .width(Length::Fill)
                    .style(theme::card_box(p)),
                );
            }
        }

        // 2. Staged Secret Leak Findings
        for finding in &self.staged_secrets {
            count += 1;
            items = items.push(
                container(
                    row![
                        icons::shield(icon_size::SM, p.danger_fg),
                        status_pill("SECRET", p.danger_fg, p.danger_bg),
                        text(format!(
                            "{} detected in {}:{}",
                            finding.rule, finding.file_path, finding.line
                        ))
                        .size(type_scale::CODE)
                        .font(theme::CODE_FONT)
                        .color(p.text_primary)
                        .width(Length::Fill),
                        if let Some(repo_id) = &self.selected_repo {
                            Element::from(
                                UiButton::new(p, "Inspect Line")
                                    .variant(ButtonVariant::Secondary)
                                    .size(ButtonSize::Xs)
                                    .on_press(Message::JumpToSearchMatch(
                                        repo_id.clone(),
                                        finding.file_path.clone(),
                                        finding.line,
                                    )),
                            )
                        } else {
                            Space::with_width(0.0).into()
                        },
                        UiButton::new(p, "Allow")
                            .variant(ButtonVariant::Subtle)
                            .size(ButtonSize::Xs)
                            .on_press(Message::AllowSecretLine(finding.id.clone())),
                    ]
                    .spacing(space::SM)
                    .align_y(Alignment::Center),
                )
                .padding([6, 10])
                .width(Length::Fill)
                .style(theme::card_box(p)),
            );
        }

        // 3. Oversized Files
        for big in &self.oversized_files {
            count += 1;
            let mb = big.bytes as f64 / (1024.0 * 1024.0);
            items = items.push(
                container(
                    row![
                        icons::alert_triangle(icon_size::SM, p.warning_fg),
                        status_pill(format!("{mb:.1} MB"), p.warning_fg, p.warning_bg),
                        text(format!("Oversized working-tree file: {}", big.file_path))
                            .size(type_scale::CODE)
                            .font(theme::CODE_FONT)
                            .color(p.text_primary)
                            .width(Length::Fill),
                        UiButton::new(p, "Add to .gitignore")
                            .variant(ButtonVariant::Subtle)
                            .size(ButtonSize::Xs)
                            .on_press(Message::IgnoreOversizedFile(big.file_path.clone())),
                    ]
                    .spacing(space::SM)
                    .align_y(Alignment::Center),
                )
                .padding([6, 10])
                .width(Length::Fill)
                .style(theme::card_box(p)),
            );
        }

        // 4. Secrets Vault Drift
        if self.vault_summary.vault_locked {
            count += 1;
            items = items.push(
                container(
                    row![
                        icons::lock(icon_size::SM, p.warning_fg),
                        status_pill("VAULT LOCKED", p.warning_fg, p.warning_bg),
                        text("Secrets Vault is encrypted with an Age key not yet unlocked on this machine.")
                            .size(type_scale::BODY)
                            .color(p.text_primary)
                            .width(Length::Fill),
                        UiButton::new(p, "Manage Keys")
                            .variant(ButtonVariant::Primary)
                            .size(ButtonSize::Xs)
                            .on_press(Message::OpenSecretsDrawer { show_keys: true }),
                    ]
                    .spacing(space::SM)
                    .align_y(Alignment::Center),
                )
                .padding([6, 10])
                .width(Length::Fill)
                .style(theme::card_box(p)),
            );
        } else {
            let enc = self.vault_summary.encrypt_needed_count();
            let res = self.vault_summary.restore_needed_count();
            if enc > 0 {
                count += enc;
                items = items.push(
                    container(
                        row![
                            icons::shield(icon_size::SM, p.warning_fg),
                            status_pill("UNSEALED", p.warning_fg, p.warning_bg),
                            text(format!(
                                "{enc} tracked secret file(s) modified on disk and not yet encrypted into .gitfleet/{}",
                                self.vault_summary.vault_file_name
                            ))
                            .size(type_scale::BODY)
                            .color(p.text_primary)
                            .width(Length::Fill),
                            UiButton::new(p, "Encrypt Now")
                                .variant(ButtonVariant::Primary)
                                .size(ButtonSize::Xs)
                                .on_press(Message::EncryptRepoEnv),
                        ]
                        .spacing(space::SM)
                        .align_y(Alignment::Center),
                    )
                    .padding([6, 10])
                    .width(Length::Fill)
                    .style(theme::card_box(p)),
                );
            }
            if res > 0 {
                count += res;
                items = items.push(
                    container(
                        row![
                            icons::shield(icon_size::SM, p.accent_fg),
                            status_pill("MISSING ENV", p.accent_fg, p.fill_selected),
                            text(format!(
                                "{res} secret file(s) available in .gitfleet/{} are missing on disk",
                                self.vault_summary.vault_file_name
                            ))
                            .size(type_scale::BODY)
                            .color(p.text_primary)
                            .width(Length::Fill),
                            UiButton::new(p, "Restore to Disk")
                                .variant(ButtonVariant::Primary)
                                .size(ButtonSize::Xs)
                                .on_press(Message::RestoreRepoEnv),
                        ]
                        .spacing(space::SM)
                        .align_y(Alignment::Center),
                    )
                    .padding([6, 10])
                    .width(Length::Fill)
                    .style(theme::card_box(p)),
                );
            }
        }

        // 5. Submodules & Orphan Gitlinks
        for sub in &self.submodules {
            if !sub.initialized || sub.dirty {
                count += 1;
                items = items.push(
                    container(
                        row![
                            icons::layers(icon_size::SM, p.warning_fg),
                            status_pill(
                                if !sub.initialized { "UNINIT SUB" } else { "DIRTY SUB" },
                                p.warning_fg,
                                p.warning_bg,
                            ),
                            text(format!(
                                "Submodule '{}' {}",
                                sub.path,
                                if !sub.initialized {
                                    "is not initialized on disk"
                                } else {
                                    "has uncommitted local changes"
                                }
                            ))
                            .size(type_scale::BODY)
                            .color(p.text_primary)
                            .width(Length::Fill),
                            UiButton::new(p, "Update Submodules")
                                .variant(ButtonVariant::Subtle)
                                .size(ButtonSize::Xs)
                                .on_press(Message::UpdateSubmodules),
                        ]
                        .spacing(space::SM)
                        .align_y(Alignment::Center),
                    )
                    .padding([6, 10])
                    .width(Length::Fill)
                    .style(theme::card_box(p)),
                );
            }
        }

        for orphan in &self.orphan_gitlinks {
            count += 1;
            items = items.push(
                container(
                    row![
                        icons::alert_triangle(icon_size::SM, p.danger_fg),
                        status_pill("ORPHAN SUB", p.danger_fg, p.danger_bg),
                        text(format!(
                            "Submodule '{}' points to unpushed commit {}",
                            orphan.submodule_path, orphan.commit_oid
                        ))
                        .size(type_scale::CODE)
                        .font(theme::CODE_FONT)
                        .color(p.text_primary)
                        .width(Length::Fill),
                    ]
                    .spacing(space::SM)
                    .align_y(Alignment::Center),
                )
                .padding([6, 10])
                .width(Length::Fill)
                .style(theme::card_box(p)),
            );
        }

        let toolbar = theme::dock_toolbar(
            p,
            row![
                icons::shield(icon_size::SM, if count > 0 { p.warning_fg } else { p.success_fg }),
                text("Problems & Repository Hygiene")
                    .size(type_scale::BODY)
                    .font(theme::UI_FONT_MEDIUM)
                    .color(p.text_primary),
                status_pill(
                    format!("{count} issue(s)"),
                    if count > 0 { p.warning_fg } else { p.success_fg },
                    if count > 0 { p.warning_bg } else { p.success_bg },
                ),
                Space::with_width(Length::Fill),
                UiButton::new(p, "Run Push Preflight")
                    .variant(ButtonVariant::Subtle)
                    .size(ButtonSize::Xs)
                    .on_press(Message::RunPushPreflight),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        );

        let body: Element<'_, Message> = if count == 0 {
            theme::empty_state(
                p,
                icons::check(icon_size::LG, p.success_fg),
                "Workspace hygiene is clean",
                "Zero merge conflicts, staged secrets, oversized files, or unsealed vault drifts detected.",
                None,
            )
        } else {
            container(slim_scroll(items, p))
                .padding(pad::DOCK_BODY)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };

        column![toolbar, body]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
