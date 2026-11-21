use crate::app::{GitFleetApp, Message, TerminalMenuSubmenu};
use crate::terminal::TerminalCanvasAction;
use iced::{Task, clipboard};
use std::path::PathBuf;

impl GitFleetApp {
    pub(crate) fn try_update_terminal(&mut self, message: Message) -> Result<Task<Message>, Message> {
        let mut unhandled: Option<Message> = None;
        let task = (|| -> Task<Message> {
            match message {
            Message::TerminalTick => {
                self.sync_terminal_snapshots();
                Task::none()
            }
            Message::TerminalNewTab(profile_id) => {
                self.spawn_terminal_pane(profile_id, false);
                Task::none()
            }
            Message::TerminalSplitPane(profile_id) => {
                let split = !self.terminal_tabs.is_empty();
                self.spawn_terminal_pane(profile_id, split);
                Task::none()
            }
            Message::TerminalOpenExternalWindow(profile_id) => {
                self.show_terminal_menu = false;
                self.terminal_menu_submenu = TerminalMenuSubmenu::None;
                let cwd = self
                    .current_repo()
                    .map(|r| r.path)
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
                let target_prof = profile_id.or_else(|| self.service.settings().default_terminal_profile);
                match self
                    .service
                    .terminal
                    .open_external_window(&cwd, target_prof.as_deref())
                {
                    Ok(_) => {
                        self.toast_is_error = false;
                        self.toast_message =
                            format!("Opened external terminal in {}", cwd.display());
                    }
                    Err(e) => {
                        self.toast_is_error = true;
                        self.toast_message = format!("External terminal failed: {e}");
                    }
                }
                Task::none()
            }
            Message::TerminalSelectTab(idx) => {
                if idx < self.terminal_tabs.len() {
                    self.active_terminal_tab = idx;
                    self.terminal_focused = true;
                    self.sync_terminal_snapshots();
                }
                Task::none()
            }
            Message::TerminalFocusPane(tab_idx, pane_idx) => {
                if let Some(tab) = self.terminal_tabs.get_mut(tab_idx)
                    && pane_idx < tab.panes.len()
                {
                    self.active_terminal_tab = tab_idx;
                    tab.focused_pane = pane_idx;
                    self.terminal_focused = true;
                    self.sync_terminal_snapshots();
                }
                Task::none()
            }
            Message::TerminalCloseTab(idx) => {
                if idx < self.terminal_tabs.len() {
                    let tab = self.terminal_tabs.remove(idx);
                    for pane in tab.panes {
                        let _ = self.service.terminal.close(&pane.session_id);
                    }
                    if self.active_terminal_tab >= self.terminal_tabs.len() {
                        self.active_terminal_tab = self.terminal_tabs.len().saturating_sub(1);
                    }
                    if self.terminal_tabs.is_empty() {
                        self.terminal_focused = false;
                    }
                }
                Task::none()
            }
            Message::TerminalClosePane(tab_idx, pane_idx) => {
                if let Some(tab) = self.terminal_tabs.get_mut(tab_idx)
                    && pane_idx < tab.panes.len()
                {
                    let pane = tab.panes.remove(pane_idx);
                    let _ = self.service.terminal.close(&pane.session_id);
                    if tab.panes.is_empty() {
                        self.terminal_tabs.remove(tab_idx);
                        if self.active_terminal_tab >= self.terminal_tabs.len() {
                            self.active_terminal_tab = self.terminal_tabs.len().saturating_sub(1);
                        }
                        if self.terminal_tabs.is_empty() {
                            self.terminal_focused = false;
                        }
                    } else if tab.focused_pane >= tab.panes.len() {
                        tab.focused_pane = tab.panes.len().saturating_sub(1);
                    }
                }
                Task::none()
            }
            Message::TerminalKillFocusedPane => {
                if let Some(tab) = self.terminal_tabs.get(self.active_terminal_tab) {
                    let p_idx = tab.focused_pane.min(tab.panes.len().saturating_sub(1));
                    return self.update(Message::TerminalClosePane(
                        self.active_terminal_tab,
                        p_idx,
                    ));
                }
                Task::none()
            }
            Message::TerminalCanvas(action) => match action {
                TerminalCanvasAction::Focus(sid) => {
                    self.terminal_focused = true;
                    self.show_terminal_menu = false;
                    self.terminal_menu_submenu = TerminalMenuSubmenu::None;
                    if let Some(tab) = self.terminal_tabs.get_mut(self.active_terminal_tab)
                        && let Some(idx) = tab.panes.iter().position(|p| p.session_id == sid)
                    {
                        tab.focused_pane = idx;
                    }
                    Task::none()
                }
                TerminalCanvasAction::Resize(sid, cols, rows) => {
                    for tab in &mut self.terminal_tabs {
                        if let Some(pane) = tab.panes.iter_mut().find(|p| p.session_id == sid)
                            && (pane.cols != cols || pane.rows != rows)
                        {
                            pane.cols = cols;
                            pane.rows = rows;
                            let _ = self.service.terminal.resize(&sid, cols, rows);
                            if let Some(snap) = self.service.terminal.screen_snapshot(&sid) {
                                pane.snapshot = snap;
                            }
                        }
                    }
                    Task::none()
                }
                TerminalCanvasAction::Scroll(sid, delta) => {
                    self.service.terminal.scroll_by(&sid, delta);
                    self.sync_terminal_snapshots();
                    Task::none()
                }
            },
            Message::TerminalKeyPressed {
                key,
                modifiers,
                text,
            } => {
                if let Some(sid) = self.focused_terminal_session_id()
                    && let Some(bytes) =
                        crate::terminal::key_to_pty_bytes(&key, modifiers, text.as_deref())
                {
                    let _ = self.service.terminal.write(&sid, &bytes);
                    self.sync_terminal_snapshots();
                }
                Task::none()
            }
            Message::TerminalPasteClipboard => {
                self.show_terminal_menu = false;
                self.terminal_menu_submenu = TerminalMenuSubmenu::None;
                if let Some(sid) = self.focused_terminal_session_id()
                    && let Ok(mut cb) = arboard::Clipboard::new()
                    && let Ok(clip) = cb.get_text()
                {
                    let normalized = clip.replace("\r\n", "\r").replace('\n', "\r");
                    let _ = self.service.terminal.write(&sid, &normalized);
                    self.sync_terminal_snapshots();
                }
                Task::none()
            }
            Message::TerminalCopyScreen => {
                self.show_terminal_menu = false;
                self.terminal_menu_submenu = TerminalMenuSubmenu::None;
                if let Some(sid) = self.focused_terminal_session_id() {
                    let screen = self.service.terminal.screen_text(&sid);
                    self.toast_is_error = false;
                    self.toast_message = "Copied terminal screen to clipboard".into();
                    return clipboard::write(screen);
                }
                Task::none()
            }
            Message::TerminalSendCtrlC => {
                self.show_terminal_menu = false;
                self.terminal_menu_submenu = TerminalMenuSubmenu::None;
                if let Some(sid) = self.focused_terminal_session_id() {
                    let _ = self.service.terminal.write(&sid, "\x03");
                    self.sync_terminal_snapshots();
                }
                Task::none()
            }
            Message::ToggleTerminalMenu => {
                self.show_terminal_menu = !self.show_terminal_menu;
                self.terminal_menu_submenu = TerminalMenuSubmenu::None;
                Task::none()
            }
            Message::ToggleTerminalSidebar => {
                self.show_terminal_sidebar = !self.show_terminal_sidebar;
                Task::none()
            }
            Message::SetTerminalMenuSubmenu(sub) => {
                self.terminal_menu_submenu = if self.terminal_menu_submenu == sub {
                    TerminalMenuSubmenu::None
                } else {
                    sub
                };
                Task::none()
            }
            Message::ToggleTerminalExpanded => {
                self.terminal_expanded = !self.terminal_expanded;
                self.drawer_height = if self.terminal_expanded {
                    (self.window_size.height * 0.60).clamp(440.0, 660.0)
                } else {
                    crate::theme::layout::DOCK_DEFAULT_HEIGHT
                };
                Task::none()
            }
            Message::StartResize(target) => {
                self.active_resize = Some(target);
                Task::none()
            }
            Message::EndResize => {
                self.active_resize = None;
                if let Some(sel) = &mut self.diff_selection {
                    sel.dragging = false;
                }
                if let Some((repo_id, _)) = self.dragging_repo.take() {
                    self.repo_drag_candidate = None;
                    if let Some((_, target_group)) = self.drag_hover_group.take() {
                        return self.update(Message::AssignRepoGroup(repo_id, target_group));
                    }
                    return Task::none();
                }
                if let Some((repo_id, _, _)) = self.repo_drag_candidate.take() {
                    self.drag_hover_group = None;
                    return self.update(Message::SelectRepo(repo_id));
                }
                self.drag_hover_group = None;
                Task::none()
            }
            Message::ToggleBranchTreeNode(key) => {
                if !self.toggled_branch_nodes.remove(&key) {
                    self.toggled_branch_nodes.insert(key);
                }
                Task::none()
            }
            Message::SetDefaultTerminalProfile(profile_id) => {
                let mut s = self.service.settings();
                s.default_terminal_profile = Some(profile_id.clone());
                self.service.update_settings(s);
                let label = self
                    .service
                    .terminal
                    .profile_by_id(&profile_id)
                    .map(|p| p.label)
                    .unwrap_or(profile_id);
                self.show_terminal_menu = false;
                self.terminal_menu_submenu = TerminalMenuSubmenu::None;
                self.toast_is_error = false;
                self.toast_message = format!("Default terminal profile set to '{label}'");
                Task::none()
            }
            Message::SetTerminalFontFamily(font_opt) => {
                let mut s = self.service.settings();
                s.terminal_font_family = font_opt.clone();
                self.service.update_settings(s);
                let display = font_opt.unwrap_or_else(|| {
                    format!("Auto ({})", self.terminal_fonts.default_font)
                });
                self.toast_is_error = false;
                self.toast_message = format!("Terminal font set to {display}");
                Task::none()
            }
            Message::SetTerminalFontSize(sz) => {
                let mut s = self.service.settings();
                s.terminal_font_size = sz.clamp(12, 24);
                self.service.update_settings(s);
                Task::none()
            }
                other => {
                    unhandled = Some(other);
                    Task::none()
                }
            }
        })();
        match unhandled {
            Some(msg) => Err(msg),
            None => Ok(task),
        }
    }
}
