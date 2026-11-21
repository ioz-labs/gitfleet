use crate::app::{BottomViewMode, DrawerTab, FilesViewMode, GitFleetApp, Message, PendingConfirm};
use crate::icons;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use crate::views::slim_scroll;
use gitfleet_workspace::RepoEntry;
use iced::widget::{
    Space, button, column, container, horizontal_rule, opaque, row, text, text_input,
};
use iced::{Alignment, Element, Length, Padding};

impl GitFleetApp {
    pub(crate) fn view_command_palette(
        &self,
        p: Palette,
        repos: &[RepoEntry],
    ) -> Element<'_, Message> {
        let q = self.palette_query.to_lowercase();
        let mut items = column![].spacing(3);
        let commands: Vec<(&str, &str, Message)> = vec![
            ("Workspace: Manage Workspaces, Groups, Clone & Create…", "", Message::ToggleWorkspaceModal),
            ("Workspace: Add Repository…", "", Message::PickAddRepo),
            ("Workspace: Scan Folder…", "", Message::PickScanFolder),
            ("Workspace: Open Active Repo in Explorer", "", Message::RevealSelectedInExplorer),
            ("Git: Refresh Selected", "F5", Message::RefreshSelected),
            ("Git: Refresh All", "Ctrl+Shift+R", Message::RefreshAll),
            ("Git: Fetch", "", Message::FetchSelected),
            ("Git: Pull", "", Message::PullSelected),
            (
                "Git: Push…",
                "",
                Message::RequestConfirm(PendingConfirm::push_selected(
                    self.active_status()
                        .and_then(|s| s.branch.as_deref())
                        .unwrap_or("HEAD"),
                    self.active_status().map(|s| s.ahead).unwrap_or(0),
                )),
            ),
            ("Git: Stage All", "", Message::StageAll),
            ("Git: Discard Local or Staged Changes…", "", Message::OpenDiscardModal(None)),
            ("Git: Quick Switch Branch", "Ctrl+B", Message::ToggleBranchPopover),
            ("Git: Merge / Rebase + Live Forecast", "", Message::ToggleMergeModal),
            ("Git: Git Flow", "", Message::ToggleGitFlowModal),
            ("Git: Push Preflight (Orphan Submodules)", "", Message::RunPushPreflight),
            ("Tools: Fleet Search (Cross-Repo)", "Ctrl+Shift+F", Message::ToggleDrawer(DrawerTab::Search)),
            ("Tools: Embedded PTY Terminal", "Ctrl+`", Message::ToggleDrawer(DrawerTab::Terminal)),
            ("Terminal: New Terminal (Default Profile)", "Ctrl+Shift+`", Message::TerminalNewTab(None)),
            ("Terminal: Split Terminal", "Ctrl+Shift+5", Message::TerminalSplitPane(None)),
            ("Terminal: New External Terminal Window", "", Message::TerminalOpenExternalWindow(None)),
            ("Tools: Secrets Vault (.gitfleet/secrets.age & secrets.json)", "", Message::ToggleDrawer(DrawerTab::Secrets)),
            ("Tools: 3-Layer Ignore Editor", "", Message::ToggleDrawer(DrawerTab::Ignore)),
            ("Tools: Command Log", "", Message::ToggleDrawer(DrawerTab::CommandLog)),
            ("Tools: Settings & Diagnostics", "", Message::ToggleSettingsModal),
            ("Files: Flat Changed Mode", "", Message::SetFilesViewMode(FilesViewMode::Flat)),
            ("Files: Dir-Filtered Changes Mode", "", Message::SetFilesViewMode(FilesViewMode::Dir)),
            ("Files: All Files Explorer (VS Code)", "", Message::SetFilesViewMode(FilesViewMode::AllFiles)),
            ("View: Side by Side Diff", "", Message::SetBottomViewMode(BottomViewMode::SideBySideDiff)),
            ("View: Unified Diff", "", Message::SetBottomViewMode(BottomViewMode::UnifiedDiff)),
            ("View: Rich Preview (.md / img / svg / json / csv)", "", Message::SetBottomViewMode(BottomViewMode::Preview)),
            ("View: Live File Editor", "", Message::SetBottomViewMode(BottomViewMode::Edit)),
            ("View: Toggle Dark / Light Theme", "", Message::ToggleTheme),
        ];

        for (label, shortcut, msg) in commands {
            if q.is_empty() || label.to_lowercase().contains(&q) {
                items = items.push(
                    button(
                        row![
                            text(label).size(type_scale::BODY).color(p.text_primary),
                            Space::with_width(Length::Fill),
                            text(shortcut)
                                .size(type_scale::CAPTION)
                                .font(theme::CODE_FONT)
                                .color(p.text_muted),
                        ]
                        .align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .padding([7, 12])
                    .style(theme::repo_row_btn(p, false))
                    .on_press(msg),
                );
            }
        }
        for item in self.service.workspace.list_workspaces() {
            if item.is_current {
                continue;
            }
            let label = format!("Workspace: Switch to '{}'", item.name);
            if q.is_empty() || label.to_lowercase().contains(&q) {
                let msg = if item.is_home && !item.path.exists() {
                    Message::OpenHomeWorkspace
                } else {
                    Message::OpenWorkspacePath(Some(item.path.clone()))
                };
                items = items.push(
                    button(
                        row![
                            icons::folder_open(icon_size::XS, p.success_fg),
                            text(label).size(type_scale::BODY).color(p.success_fg),
                            Space::with_width(Length::Fill),
                            text(format!("{} repo(s)", item.repo_count))
                                .size(type_scale::CAPTION)
                                .font(theme::CODE_FONT)
                                .color(p.text_muted),
                        ]
                        .spacing(space::XS)
                        .align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .padding([7, 12])
                    .style(theme::repo_row_btn(p, false))
                    .on_press(msg),
                );
            }
        }
        for repo in repos {
            let label = format!("Repo: {}", repo.name);
            if q.is_empty() || label.to_lowercase().contains(&q) {
                items = items.push(
                    button(
                        row![
                            icons::folder(icon_size::XS, p.accent_fg),
                            text(label).size(type_scale::BODY).color(p.accent_fg),
                            Space::with_width(Length::Fill),
                            text(repo.path.display().to_string())
                                .size(type_scale::CAPTION)
                                .font(theme::CODE_FONT)
                                .color(p.text_muted),
                        ]
                        .spacing(space::XS)
                        .align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .padding([7, 12])
                    .style(theme::repo_row_btn(p, false))
                    .on_press(Message::SelectRepo(repo.id.clone())),
                );
            }
        }

        let modal = opaque(
            container(
                column![
                    row![
                        icons::command(icon_size::MD, p.accent_fg),
                        text_input("Type a command or repository…", &self.palette_query)
                            .id(self.palette_input_id.clone())
                            .on_input(Message::PaletteQueryChanged)
                            .padding(pad::INPUT)
                            .size(type_scale::SUBHEADING)
                            .style(theme::dark_input(p)),
                        button(icons::close_x(icon_size::SM, p.text_secondary))
                            .padding(pad::BTN_ICON)
                            .style(theme::vscode_icon_btn(p, false))
                            .on_press(Message::CloseOverlays),
                    ]
                    .spacing(space::SM)
                    .align_y(Alignment::Center),
                    horizontal_rule(1),
                    slim_scroll(items, p).height(Length::Fixed(380.0)),
                ]
                .spacing(space::SM),
            )
            .padding(pad::CARD + 4)
            .width(Length::Fixed(layout::MODAL_MD + 40.0))
            .style(theme::modal_box(p)),
        );

        let positioned = container(modal)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .padding(Padding {
                top: 64.0,
                right: 24.0,
                bottom: 24.0,
                left: 24.0,
            })
            .style(theme::backdrop_box);

        theme::overlay_backdrop(positioned, Message::CloseOverlays)
    }
}
