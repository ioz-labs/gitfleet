use crate::app::{BottomViewMode, DrawerTab, GitFleetApp, HostLayoutMode, Message};
use crate::views::detached_viewer::DetachedAction;
use crate::views::image_diff::ImageDiffMode;
use iced::keyboard::{self, Key, Modifiers};
use iced::{Point, Subscription, event, mouse, time, window};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

static STREAM_CURSOR_EVENTS: AtomicBool = AtomicBool::new(false);
static PALETTE_NAV_ACTIVE: AtomicBool = AtomicBool::new(false);
static BRANCH_POPOVER_NAV_ACTIVE: AtomicBool = AtomicBool::new(false);
static LAST_CURSOR_POS: OnceLock<Mutex<HashMap<window::Id, Point>>> = OnceLock::new();
static STUDIO_WINDOWS: OnceLock<Mutex<HashSet<window::Id>>> = OnceLock::new();
static QUICK_OPEN_WINDOWS: OnceLock<Mutex<HashSet<window::Id>>> = OnceLock::new();

fn record_cursor_pos(id: window::Id, pos: Point) {
    let map = LAST_CURSOR_POS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut guard) = map.lock() {
        guard.insert(id, pos);
    }
}

pub(crate) fn latest_cursor_pos(id: window::Id) -> Option<Point> {
    let map = LAST_CURSOR_POS.get()?;
    map.lock().ok().and_then(|guard| guard.get(&id).copied())
}

pub(crate) fn set_stream_cursor_events(enabled: bool) {
    STREAM_CURSOR_EVENTS.store(enabled, Ordering::Relaxed);
}

fn sync_window_sets(studio: HashSet<window::Id>, quick_open: HashSet<window::Id>) {
    if let Ok(mut guard) = STUDIO_WINDOWS
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
    {
        *guard = studio;
    }
    if let Ok(mut guard) = QUICK_OPEN_WINDOWS
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
    {
        *guard = quick_open;
    }
}

fn is_studio_window(id: window::Id) -> bool {
    STUDIO_WINDOWS
        .get()
        .and_then(|m| m.lock().ok())
        .is_some_and(|s| s.contains(&id))
}

fn is_quick_open_window(id: window::Id) -> bool {
    QUICK_OPEN_WINDOWS
        .get()
        .and_then(|m| m.lock().ok())
        .is_some_and(|s| s.contains(&id))
}

impl GitFleetApp {
    pub(crate) fn subscription(&self) -> Subscription<Message> {
        let needs_cursor_stream = self.active_resize.is_some()
            || self.repo_drag_candidate.is_some()
            || self.dragging_repo.is_some()
            || self
                .detached_viewers
                .values()
                .any(|v| v.resizing_sidebar || v.resizing_split);
        set_stream_cursor_events(needs_cursor_stream);
        PALETTE_NAV_ACTIVE.store(self.show_palette, Ordering::Relaxed);
        BRANCH_POPOVER_NAV_ACTIVE.store(self.show_branch_popover, Ordering::Relaxed);
        sync_window_sets(
            self.detached_viewers.keys().copied().collect(),
            self.detached_viewers
                .iter()
                .filter_map(|(id, v)| v.quick_open_visible.then_some(*id))
                .collect(),
        );

        let close_sub = window::close_events().map(Message::WindowClosed);
        let tick_sub = time::every(Duration::from_millis(250)).map(|_| Message::BackgroundTick);
        let terminal_capture_keys = self.drawer_tab == Some(DrawerTab::Terminal)
            && self.terminal_focused
            && !self.has_modal_or_menu_open()
            && !self.terminal_tabs.is_empty();

        let mut subs = vec![close_sub, tick_sub];

        if self.drawer_tab == Some(DrawerTab::Terminal) && !self.terminal_tabs.is_empty() {
            subs.push(time::every(Duration::from_millis(33)).map(|_| Message::TerminalTick));
        }

        let host_needs_blink = self.bottom_mode == BottomViewMode::ImageDiff
            && self.image_diff_mode == ImageDiffMode::Blink
            && !self.image_diff_blink_paused
            && self.diff_image.is_some();
        let any_detached_needs_blink = self.detached_viewers.values().any(|v| v.needs_blink_timer());
        if host_needs_blink || any_detached_needs_blink {
            subs.push(time::every(Duration::from_millis(750)).map(|_| Message::ImageDiffBlinkTick));
        }

        if self.update_tracker.is_some() {
            subs.push(time::every(Duration::from_millis(100)).map(|_| Message::UpdateProgressTick));
        }

        if self.active_clone.is_some() {
            subs.push(time::every(Duration::from_millis(100)).map(|_| Message::CloneProgressTick));
        }

        if self
            .blueprint_import_modal
            .as_ref()
            .is_some_and(|m| m.running)
        {
            subs.push(time::every(Duration::from_millis(100)).map(|_| Message::BlueprintImportTick));
        }

        if terminal_capture_keys {
            subs.push(event::listen_with(handle_window_and_terminal_event));
        } else {
            subs.push(event::listen_with(handle_window_event));
        }

        Subscription::batch(subs)
    }
}

fn handle_studio_key_pressed(
    key: Key,
    modifiers: Modifiers,
    status: event::Status,
    id: window::Id,
) -> Option<Message> {
    if !modifiers.command() && !modifiers.alt() && !modifiers.shift() {
        if is_quick_open_window(id) {
            match key.as_ref() {
                Key::Named(keyboard::key::Named::ArrowUp) => {
                    return Some(Message::Detached(
                        id,
                        DetachedAction::QuickOpenMoveSelection(-1),
                    ));
                }
                Key::Named(keyboard::key::Named::ArrowDown) => {
                    return Some(Message::Detached(
                        id,
                        DetachedAction::QuickOpenMoveSelection(1),
                    ));
                }
                _ => {}
            }
        }
        match key.as_ref() {
            Key::Named(keyboard::key::Named::F5) => {
                return Some(Message::Detached(id, DetachedAction::ReloadFiles));
            }
            Key::Named(keyboard::key::Named::Escape) => {
                return Some(Message::ShortcutEscape(id));
            }
            _ => {}
        }
    }

    if modifiers.command() && modifiers.shift() && !modifiers.alt() {
        return match key.as_ref() {
            Key::Character("s") | Key::Character("S") => {
                Some(Message::Detached(id, DetachedAction::SaveTabAs))
            }
            Key::Character("f") | Key::Character("F") => Some(Message::ShortcutSearchFiles(id)),
            Key::Character("e") | Key::Character("E") => Some(Message::ShortcutExplorer(id)),
            Key::Character("p") | Key::Character("P") => {
                Some(Message::Detached(id, DetachedAction::ToggleQuickOpen))
            }
            Key::Character("o") | Key::Character("O") => {
                Some(Message::Detached(id, DetachedAction::OpenSymbolOutline))
            }
            Key::Character("t") | Key::Character("T") => {
                Some(Message::Detached(id, DetachedAction::ReopenClosedTab))
            }
            Key::Character("z") | Key::Character("Z") if status == event::Status::Ignored => {
                Some(Message::Detached(id, DetachedAction::Redo))
            }
            Key::Character("=") | Key::Character("+") => Some(Message::ShortcutZoomFont(id, 1.0)),
            _ => None,
        };
    }

    if modifiers.command() && modifiers.alt() && !modifiers.shift() {
        return match key.as_ref() {
            Key::Character("s") | Key::Character("S") => {
                Some(Message::Detached(id, DetachedAction::SaveAllTabs))
            }
            _ => None,
        };
    }

    if modifiers.command() && !modifiers.shift() && !modifiers.alt() {
        return match key.as_ref() {
            Key::Character("p")
            | Key::Character("P")
            | Key::Character("k")
            | Key::Character("K") => Some(Message::Detached(id, DetachedAction::ToggleQuickOpen)),
            Key::Character("g") | Key::Character("G") => {
                Some(Message::Detached(id, DetachedAction::OpenGoToLine))
            }
            Key::Character("n") | Key::Character("N") => {
                Some(Message::Detached(id, DetachedAction::NewUntitledTab))
            }
            Key::Character("s") | Key::Character("S") => Some(Message::ShortcutSave(id)),
            Key::Character("f") | Key::Character("F") => Some(Message::ShortcutFind(id)),
            Key::Character("h") | Key::Character("H") => Some(Message::ShortcutReplace(id)),
            Key::Character("b") | Key::Character("B") => Some(Message::ShortcutToggleSidebar(id)),
            Key::Character("w") | Key::Character("W") => Some(Message::ShortcutCloseTab(id)),
            Key::Character("o") | Key::Character("O") => Some(Message::ShortcutOpenFile(id)),
            Key::Character("\\") => {
                Some(Message::Detached(id, DetachedAction::ToggleSplitPreview))
            }
            Key::Character("=") | Key::Character("+") => Some(Message::ShortcutZoomFont(id, 1.0)),
            Key::Character("-") => Some(Message::ShortcutZoomFont(id, -1.0)),
            Key::Character("z") | Key::Character("Z") if status == event::Status::Ignored => {
                Some(Message::Detached(id, DetachedAction::Undo))
            }
            Key::Character("y") | Key::Character("Y") if status == event::Status::Ignored => {
                Some(Message::Detached(id, DetachedAction::Redo))
            }
            Key::Character("c") | Key::Character("C") if status == event::Status::Ignored => {
                Some(Message::CopyDiffSelection(id))
            }
            Key::Character("a") | Key::Character("A") if status == event::Status::Ignored => {
                Some(Message::SelectAllFocused(id))
            }
            _ => None,
        };
    }

    if modifiers.alt() && !modifiers.command() && !modifiers.shift() {
        if matches!(key.as_ref(), Key::Character("z") | Key::Character("Z")) {
            return Some(Message::ShortcutToggleWrap(id));
        }
    }

    None
}

fn handle_window_event(
    ev: iced::Event,
    status: event::Status,
    id: window::Id,
) -> Option<Message> {
    match ev {
        iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
            record_cursor_pos(id, position);
            if STREAM_CURSOR_EVENTS.load(Ordering::Relaxed) {
                Some(Message::CursorMoved(id, position))
            } else {
                None
            }
        }
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            Some(Message::EndResize)
        }
        iced::Event::Window(window::Event::Resized(size)) => Some(Message::WindowResized(id, size)),
        iced::Event::Window(window::Event::Unfocused) => Some(Message::WindowUnfocused),
        iced::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
            Some(Message::ModifiersChanged(modifiers))
        }
        iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
            if is_studio_window(id) {
                return handle_studio_key_pressed(key, modifiers, status, id);
            }

            // ── Host Window Shortcuts ────────────────────────────────────────
            if !modifiers.command() && !modifiers.alt() && !modifiers.shift() {
                if PALETTE_NAV_ACTIVE.load(Ordering::Relaxed) {
                    match key.as_ref() {
                        Key::Named(keyboard::key::Named::ArrowUp) => {
                            return Some(Message::PaletteMoveSelection(-1));
                        }
                        Key::Named(keyboard::key::Named::ArrowDown) => {
                            return Some(Message::PaletteMoveSelection(1));
                        }
                        _ => {}
                    }
                } else if BRANCH_POPOVER_NAV_ACTIVE.load(Ordering::Relaxed) {
                    match key.as_ref() {
                        Key::Named(keyboard::key::Named::ArrowUp) => {
                            return Some(Message::BranchPopoverMoveSelection(-1));
                        }
                        Key::Named(keyboard::key::Named::ArrowDown) => {
                            return Some(Message::BranchPopoverMoveSelection(1));
                        }
                        _ => {}
                    }
                }
                if status == event::Status::Ignored {
                    match key.as_ref() {
                        Key::Named(keyboard::key::Named::F2) => {
                            return Some(Message::OpenRenameBranchModal(None));
                        }
                        Key::Named(keyboard::key::Named::F5) => {
                            return Some(Message::RefreshSelected);
                        }
                        Key::Named(keyboard::key::Named::F7) => {
                            return Some(Message::OpenCreateBranch {
                                base_ref: None,
                                prefix: None,
                            });
                        }
                        _ => {}
                    }
                }
            }

            if modifiers.command() && modifiers.shift() && !modifiers.alt() {
                match key.as_ref() {
                    Key::Character("f") | Key::Character("F") => {
                        return Some(Message::ShortcutSearchFiles(id));
                    }
                    Key::Character("p") | Key::Character("P") => {
                        return Some(Message::ToggleCommandPalette);
                    }
                    Key::Character("`") | Key::Character("~") => {
                        return Some(Message::TerminalNewTab(None));
                    }
                    Key::Character("5") | Key::Character("%") => {
                        return Some(Message::TerminalSplitPane(None));
                    }
                    Key::Character("r") | Key::Character("R") => {
                        return Some(Message::RefreshAll);
                    }
                    Key::Character("t") | Key::Character("T")
                        if status == event::Status::Ignored =>
                    {
                        return Some(Message::UnstageSelectedFiles);
                    }
                    _ => {}
                }
            } else if modifiers.command() && !modifiers.shift() && !modifiers.alt() {
                match key.as_ref() {
                    Key::Character("k")
                    | Key::Character("K")
                    | Key::Character("p")
                    | Key::Character("P") => {
                        return Some(Message::ToggleCommandPalette);
                    }
                    Key::Character("`") => {
                        return Some(Message::ToggleDrawer(DrawerTab::Terminal));
                    }
                    Key::Character("s") | Key::Character("S") => {
                        return Some(Message::ShortcutSave(id));
                    }
                    Key::Character("b") | Key::Character("B") => {
                        return Some(Message::ShortcutToggleSidebar(id));
                    }
                    Key::Character("m") | Key::Character("M")
                        if status == event::Status::Ignored =>
                    {
                        return Some(Message::ShortcutMergeSelectedBranch);
                    }
                    Key::Character("d") | Key::Character("D")
                        if status == event::Status::Ignored =>
                    {
                        return Some(Message::ShortcutRebaseSelectedBranch);
                    }
                    Key::Character("t") | Key::Character("T")
                        if status == event::Status::Ignored =>
                    {
                        return Some(Message::StageSelectedFiles);
                    }
                    Key::Character("z") | Key::Character("Z")
                        if status == event::Status::Ignored =>
                    {
                        return Some(Message::DiscardSelectedFiles);
                    }
                    Key::Character("c") | Key::Character("C")
                        if status == event::Status::Ignored =>
                    {
                        return Some(Message::CopyDiffSelection(id));
                    }
                    Key::Character("a") | Key::Character("A")
                        if status == event::Status::Ignored =>
                    {
                        return Some(Message::SelectAllFocused(id));
                    }
                    _ => {}
                }
            } else if modifiers.alt() && !modifiers.command() && !modifiers.shift() {
                match key.as_ref() {
                    Key::Character("1") => {
                        return Some(Message::SetHostLayout(HostLayoutMode::Cockpit));
                    }
                    Key::Character("2") => {
                        return Some(Message::SetHostLayout(HostLayoutMode::Changes));
                    }
                    Key::Character("3") => {
                        return Some(Message::SetHostLayout(HostLayoutMode::History));
                    }
                    Key::Character("z") | Key::Character("Z") => {
                        return Some(Message::ShortcutToggleWrap(id));
                    }
                    Key::Named(keyboard::key::Named::ArrowUp) => {
                        return Some(Message::SelectPrevRepo);
                    }
                    Key::Named(keyboard::key::Named::ArrowDown) => {
                        return Some(Message::SelectNextRepo);
                    }
                    _ => {}
                }
            } else if matches!(key.as_ref(), Key::Named(keyboard::key::Named::Escape)) {
                return Some(Message::ShortcutEscape(id));
            }
            None
        }
        _ => None,
    }
}

fn handle_window_and_terminal_event(
    ev: iced::Event,
    status: event::Status,
    id: window::Id,
) -> Option<Message> {
    if is_studio_window(id) {
        return handle_window_event(ev, status, id);
    }

    match ev {
        iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
            record_cursor_pos(id, position);
            if STREAM_CURSOR_EVENTS.load(Ordering::Relaxed) {
                Some(Message::CursorMoved(id, position))
            } else {
                None
            }
        }
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            Some(Message::EndResize)
        }
        iced::Event::Window(window::Event::Resized(size)) => Some(Message::WindowResized(id, size)),
        iced::Event::Window(window::Event::Unfocused) => Some(Message::WindowUnfocused),
        iced::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
            Some(Message::ModifiersChanged(modifiers))
        }
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modifiers,
            text,
            ..
        }) if status == event::Status::Ignored => {
            if modifiers.command() && modifiers.shift() {
                match key.as_ref() {
                    Key::Character("`") | Key::Character("~") => {
                        return Some(Message::TerminalNewTab(None));
                    }
                    Key::Character("5") | Key::Character("%") => {
                        return Some(Message::TerminalSplitPane(None));
                    }
                    Key::Character("v") | Key::Character("V") => {
                        return Some(Message::TerminalPasteClipboard);
                    }
                    Key::Character("c") | Key::Character("C") => {
                        return Some(Message::TerminalCopyScreen);
                    }
                    Key::Character("f") | Key::Character("F") => {
                        return Some(Message::ToggleDrawer(DrawerTab::Search));
                    }
                    Key::Character("p") | Key::Character("P") => {
                        return Some(Message::ToggleCommandPalette);
                    }
                    _ => {}
                }
            }
            if modifiers.command() && !modifiers.shift() && !modifiers.alt() {
                match key.as_ref() {
                    Key::Character("`") => {
                        return Some(Message::ToggleDrawer(DrawerTab::Terminal));
                    }
                    Key::Character("v") | Key::Character("V") => {
                        return Some(Message::TerminalPasteClipboard);
                    }
                    _ => {}
                }
            }
            Some(Message::TerminalKeyPressed {
                key,
                modifiers,
                text: text.map(|s| s.to_string()),
            })
        }
        _ => None,
    }
}
