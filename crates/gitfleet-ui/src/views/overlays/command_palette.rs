use super::fuzzy::{fuzzy_match, render_fuzzy_text};
use crate::app::{
    BottomViewMode, DrawerTab, FilesViewMode, GitFleetApp, HostLayoutMode, Message, PendingConfirm,
};
use crate::icons;
use crate::theme::kit::{focus_accent_bar, kbd_pill};
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use crate::views::slim_scroll;
use gitfleet_workspace::RepoEntry;
use iced::widget::{
    button, column, container, horizontal_rule, opaque, row, text, text_input,
};
use iced::{Alignment, Element, Length, Padding};

pub(crate) struct PaletteEntry {
    pub(crate) label: String,
    pub(crate) shortcut: &'static str,
    pub(crate) meta: Option<String>,
    pub(crate) kind: PaletteEntryKind,
    pub(crate) msg: Message,
}

#[derive(Clone, Copy)]
pub(crate) enum PaletteEntryKind {
    Command,
    Blueprint,
    Workspace,
    Repo,
}

impl GitFleetApp {
    pub(crate) fn build_filtered_palette_entries(
        &self,
        repos: &[RepoEntry],
    ) -> Vec<(PaletteEntry, Vec<usize>)> {
        let base_commands: Vec<(&str, &'static str, PaletteEntryKind, Message)> = vec![
            (
                "Blueprint: Export Workspace Blueprint (.gitfleet-pack.json)…",
                "",
                PaletteEntryKind::Blueprint,
                Message::OpenExportBlueprintModal(None),
            ),
            (
                "Blueprint: Import Group Blueprint & Auto-Clone Repos…",
                "",
                PaletteEntryKind::Blueprint,
                Message::PickImportBlueprintFile(None),
            ),
            (
                "Workspace: Manage Workspaces & Groups…",
                "",
                PaletteEntryKind::Command,
                Message::ToggleWorkspaceModal,
            ),
            (
                "Workspace: Rename Current Workspace…",
                "",
                PaletteEntryKind::Command,
                Message::OpenRenameWorkspaceModal(None),
            ),
            (
                "Repository: Clone Repository…",
                "",
                PaletteEntryKind::Command,
                Message::OpenCloneModal(None),
            ),
            (
                "Repository: Add / Create Local Repository…",
                "",
                PaletteEntryKind::Command,
                Message::OpenAddRepoModal(None),
            ),
            (
                "Workspace: Scan Folder & Auto-Group…",
                "",
                PaletteEntryKind::Command,
                Message::PickScanFolder(None),
            ),
            (
                "Workspace: Open Active Repo in Explorer",
                "",
                PaletteEntryKind::Command,
                Message::RevealSelectedInExplorer,
            ),
            (
                "Git: Refresh Selected",
                "F5",
                PaletteEntryKind::Command,
                Message::RefreshSelected,
            ),
            (
                "Git: Refresh All",
                "Ctrl+Shift+R",
                PaletteEntryKind::Command,
                Message::RefreshAll,
            ),
            ("Git: Fetch", "", PaletteEntryKind::Command, Message::FetchSelected),
            ("Git: Pull", "", PaletteEntryKind::Command, Message::PullSelected),
            (
                "Git: Push…",
                "",
                PaletteEntryKind::Command,
                Message::RequestConfirm(PendingConfirm::push_selected(
                    self.active_status()
                        .and_then(|s| s.branch.as_deref())
                        .unwrap_or("HEAD"),
                    self.active_status().map(|s| s.ahead).unwrap_or(0),
                )),
            ),
            ("Git: Stage All", "Ctrl+T", PaletteEntryKind::Command, Message::StageAll),
            (
                "Git: Discard Local or Staged Changes…",
                "Ctrl+Z",
                PaletteEntryKind::Command,
                Message::OpenDiscardModal(None),
            ),
            (
                "Git: Quick Switch Branch",
                "Ctrl+B",
                PaletteEntryKind::Command,
                Message::ToggleBranchPopover,
            ),
            (
                "Git: Merge / Rebase + Live Forecast",
                "Ctrl+M",
                PaletteEntryKind::Command,
                Message::ToggleMergeModal,
            ),
            (
                "Git: Git Flow",
                "",
                PaletteEntryKind::Command,
                Message::ToggleGitFlowModal,
            ),
            (
                "Git: Push Preflight (Orphan Submodules)",
                "",
                PaletteEntryKind::Command,
                Message::RunPushPreflight,
            ),
            (
                "Layout: Cockpit 4-Pane Mode",
                "Alt+1",
                PaletteEntryKind::Command,
                Message::SetHostLayout(HostLayoutMode::Cockpit),
            ),
            (
                "Layout: Changes & Full-Height Diff Mode",
                "Alt+2",
                PaletteEntryKind::Command,
                Message::SetHostLayout(HostLayoutMode::Changes),
            ),
            (
                "Layout: History & Commit Archaeology Mode",
                "Alt+3",
                PaletteEntryKind::Command,
                Message::SetHostLayout(HostLayoutMode::History),
            ),
            (
                "Layout: Toggle Zen Diff Maximize",
                "Alt+Z",
                PaletteEntryKind::Command,
                Message::ToggleZenDiff,
            ),
            (
                "Tools: Fleet Search (Cross-Repo)",
                "Ctrl+Shift+F",
                PaletteEntryKind::Command,
                Message::ToggleDrawer(DrawerTab::Search),
            ),
            (
                "Tools: Embedded PTY Terminal",
                "Ctrl+`",
                PaletteEntryKind::Command,
                Message::ToggleDrawer(DrawerTab::Terminal),
            ),
            (
                "Terminal: New Terminal (Default Profile)",
                "Ctrl+Shift+`",
                PaletteEntryKind::Command,
                Message::TerminalNewTab(None),
            ),
            (
                "Terminal: Split Terminal",
                "Ctrl+Shift+5",
                PaletteEntryKind::Command,
                Message::TerminalSplitPane(None),
            ),
            (
                "Terminal: New External Terminal Window",
                "",
                PaletteEntryKind::Command,
                Message::TerminalOpenExternalWindow(None),
            ),
            (
                "Tools: Secrets Vault (.gitfleet/secrets.age & secrets.json)",
                "",
                PaletteEntryKind::Command,
                Message::ToggleDrawer(DrawerTab::Secrets),
            ),
            (
                "Secrets: Manage Age Keys & Repo/Group Assignment",
                "",
                PaletteEntryKind::Command,
                Message::OpenSecretsDrawer { show_keys: true },
            ),
            (
                "Secrets: Encrypt All Modified Files",
                "",
                PaletteEntryKind::Command,
                Message::EncryptRepoEnv,
            ),
            (
                "Secrets: Restore All Files from Vault",
                "",
                PaletteEntryKind::Command,
                Message::RestoreRepoEnv,
            ),
            (
                "Secrets: Delete Vault from Active Repository…",
                "",
                PaletteEntryKind::Command,
                Message::RequestConfirm(PendingConfirm::destroy_repo_vault(
                    self.current_repo()
                        .as_ref()
                        .map(|r| r.name.as_str())
                        .unwrap_or("repository"),
                )),
            ),
            (
                "Tools: 3-Layer Ignore Editor",
                "",
                PaletteEntryKind::Command,
                Message::ToggleDrawer(DrawerTab::Ignore),
            ),
            (
                "Tools: Command Log",
                "",
                PaletteEntryKind::Command,
                Message::ToggleDrawer(DrawerTab::CommandLog),
            ),
            (
                "Tools: Settings & Theme Presets",
                "",
                PaletteEntryKind::Command,
                Message::ToggleSettingsModal,
            ),
            (
                "App: Check for Updates…",
                "",
                PaletteEntryKind::Command,
                Message::CheckForUpdates { manual: true },
            ),
            (
                "Files: Flat Changed Mode",
                "",
                PaletteEntryKind::Command,
                Message::SetFilesViewMode(FilesViewMode::Flat),
            ),
            (
                "Files: Dir-Filtered Changes Mode",
                "",
                PaletteEntryKind::Command,
                Message::SetFilesViewMode(FilesViewMode::Dir),
            ),
            (
                "Files: All Files Explorer",
                "",
                PaletteEntryKind::Command,
                Message::SetFilesViewMode(FilesViewMode::AllFiles),
            ),
            (
                if self.show_ignored_files {
                    "Files: Hide Gitignored Files in All Files"
                } else {
                    "Files: Show Gitignored Files in All Files (.env, etc.)"
                },
                "",
                PaletteEntryKind::Command,
                Message::ToggleShowIgnoredFiles,
            ),
            (
                "Files: Search & Replace in Files (GitFleet Studio)",
                "",
                PaletteEntryKind::Command,
                Message::OpenDetachedSearch(None),
            ),
            (
                "Window: Open GitFleet Studio (File, Search & Diff)",
                "",
                PaletteEntryKind::Command,
                Message::OpenDetachedViewer,
            ),
            (
                "View: Side by Side Diff",
                "",
                PaletteEntryKind::Command,
                Message::SetBottomViewMode(BottomViewMode::SideBySideDiff),
            ),
            (
                "View: Unified Diff",
                "",
                PaletteEntryKind::Command,
                Message::SetBottomViewMode(BottomViewMode::UnifiedDiff),
            ),
            (
                "View: Rich Preview (.md / img / svg / json / csv)",
                "",
                PaletteEntryKind::Command,
                Message::SetBottomViewMode(BottomViewMode::Preview),
            ),
            (
                "View: Live File Editor",
                "",
                PaletteEntryKind::Command,
                Message::SetBottomViewMode(BottomViewMode::Edit),
            ),
            (
                "File: Change End of Line Sequence (LF / CRLF)",
                "",
                PaletteEntryKind::Command,
                Message::ToggleEolModal,
            ),
            (
                "File: Change File Encoding (Save / Reopen)",
                "",
                PaletteEntryKind::Command,
                Message::ToggleEncodingModal,
            ),
            (
                "File: Select Syntax Highlighting Language",
                "",
                PaletteEntryKind::Command,
                Message::ToggleSyntaxModal,
            ),
            (
                "View: Toggle Dark / Light Polarity",
                "",
                PaletteEntryKind::Command,
                Message::ToggleTheme,
            ),
        ];

        let mut all: Vec<PaletteEntry> = base_commands
            .into_iter()
            .map(|(lbl, sc, kind, msg)| PaletteEntry {
                label: lbl.to_string(),
                shortcut: sc,
                meta: None,
                kind,
                msg,
            })
            .collect();

        for item in self.service.workspace.list_workspaces() {
            if item.is_current {
                continue;
            }
            let msg = if item.is_home && !item.path.exists() {
                Message::OpenHomeWorkspace
            } else {
                Message::OpenWorkspacePath(Some(item.path.clone()))
            };
            all.push(PaletteEntry {
                label: format!("Workspace: Switch to '{}'", item.name),
                shortcut: "",
                meta: Some(format!("{} repo(s)", item.repo_count)),
                kind: PaletteEntryKind::Workspace,
                msg,
            });
        }

        for repo in repos {
            all.push(PaletteEntry {
                label: format!("Repo: {}", repo.name),
                shortcut: "",
                meta: Some(repo.path.display().to_string()),
                kind: PaletteEntryKind::Repo,
                msg: Message::SelectRepo(repo.id.clone()),
            });
        }

        let q = self.palette_query.trim();
        if q.is_empty() {
            return all.into_iter().map(|e| (e, Vec::new())).collect();
        }

        let mut scored: Vec<(i32, usize, PaletteEntry, Vec<usize>)> = all
            .into_iter()
            .enumerate()
            .filter_map(|(order, entry)| {
                fuzzy_match(q, &entry.label)
                    .map(|(score, indices)| (score, order, entry, indices))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        scored
            .into_iter()
            .map(|(_, _, entry, indices)| (entry, indices))
            .collect()
    }

    pub(crate) fn view_command_palette(
        &self,
        p: Palette,
        repos: &[RepoEntry],
    ) -> Element<'_, Message> {
        let filtered = self.build_filtered_palette_entries(repos);
        let active_idx = if filtered.is_empty() {
            0
        } else {
            self.palette_selected_idx.min(filtered.len() - 1)
        };

        let mut items = column![].spacing(2);
        if filtered.is_empty() {
            items = items.push(
                container(
                    text("No matching commands, workspaces, or repositories.")
                        .size(type_scale::BODY)
                        .color(p.text_muted),
                )
                .padding([18, 12]),
            );
        } else {
            for (idx, (entry, matched)) in filtered.into_iter().enumerate() {
                let is_sel = idx == active_idx;
                let (lead_icon, base_fg) = match entry.kind {
                    PaletteEntryKind::Blueprint => {
                        (icons::repo_stack(icon_size::XS, p.accent_fg), p.text_primary)
                    }
                    PaletteEntryKind::Command => {
                        (icons::command(icon_size::XS, p.text_muted), p.text_primary)
                    }
                    PaletteEntryKind::Workspace => {
                        (icons::folder_open(icon_size::XS, p.success_fg), p.success_fg)
                    }
                    PaletteEntryKind::Repo => {
                        (icons::folder(icon_size::XS, p.accent_fg), p.text_primary)
                    }
                };

                let mut r = row![
                    focus_accent_bar(p, is_sel, true, 16.0),
                    lead_icon,
                    theme::clipped_line(
                        render_fuzzy_text(&entry.label, &matched, base_fg, p.accent_fg),
                        Length::Fill,
                    ),
                ]
                .spacing(space::SM)
                .align_y(Alignment::Center);

                if let Some(meta) = entry.meta {
                    r = r.push(
                        text(meta)
                            .size(type_scale::BADGE)
                            .font(theme::CODE_FONT)
                            .color(p.text_muted),
                    );
                }
                if !entry.shortcut.is_empty() {
                    r = r.push(kbd_pill(p, entry.shortcut));
                }

                items = items.push(
                    button(r)
                        .width(Length::Fill)
                        .padding([6, 10])
                        .style(theme::repo_row_btn(p, is_sel))
                        .on_press(Message::PaletteExecuteEntry(Box::new(entry.msg))),
                );
            }
        }

        let modal = opaque(
            container(
                column![
                    row![
                        icons::search(icon_size::MD, p.accent_fg),
                        text_input(
                            "Fuzzy search commands, blueprints, workspaces, or repos…",
                            &self.palette_query
                        )
                        .id(self.palette_input_id.clone())
                        .on_input(Message::PaletteQueryChanged)
                        .on_submit(Message::PaletteConfirmSelected)
                        .padding(pad::INPUT)
                        .size(type_scale::SUBHEADING)
                        .style(theme::dark_input(p)),
                        kbd_pill(p, "↑↓"),
                        kbd_pill(p, "Enter"),
                        kbd_pill(p, "Esc"),
                        button(icons::close_x(icon_size::SM, p.text_secondary))
                            .padding(pad::BTN_ICON)
                            .style(theme::vscode_icon_btn(p, false))
                            .on_press(Message::CloseOverlays),
                    ]
                    .spacing(space::XS)
                    .align_y(Alignment::Center),
                    horizontal_rule(1).style(theme::subtle_rule(p)),
                    slim_scroll(items, p).height(Length::Fixed(390.0)),
                ]
                .spacing(space::SM),
            )
            .padding(pad::CARD + 2)
            .width(Length::Fixed(layout::MODAL_MD + 60.0))
            .style(theme::modal_box(p)),
        );

        let positioned = container(modal)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .padding(Padding {
                top: 56.0,
                right: 24.0,
                bottom: 24.0,
                left: 24.0,
            })
            .style(theme::backdrop_box);

        theme::overlay_backdrop(positioned, Message::CloseOverlays)
    }
}
