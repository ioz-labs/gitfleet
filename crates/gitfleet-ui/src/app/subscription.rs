use crate::app::{BottomViewMode, DrawerTab, GitFleetApp, Message};
use crate::views::image_diff::ImageDiffMode;
use iced::keyboard::{self, Key, Modifiers};
use iced::{Subscription, event, mouse, time, window};
use std::time::Duration;

impl GitFleetApp {
    pub(crate) fn subscription(&self) -> Subscription<Message> {
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

        if terminal_capture_keys {
            subs.push(event::listen_with(handle_window_and_terminal_event));
        } else {
            subs.push(keyboard::on_key_press(|key, modifiers| match key.as_ref() {
                Key::Character("k") | Key::Character("p") if modifiers.command() => {
                    Some(Message::ToggleCommandPalette)
                }
                Key::Character("b") if modifiers.command() => Some(Message::ToggleBranchPopover),
                Key::Character("f") if modifiers.command() && modifiers.shift() => {
                    Some(Message::ToggleDrawer(DrawerTab::Search))
                }
                Key::Character("`") | Key::Character("~")
                    if modifiers.command() && modifiers.shift() =>
                {
                    Some(Message::TerminalNewTab(None))
                }
                Key::Character("5") | Key::Character("%")
                    if modifiers.command() && modifiers.shift() =>
                {
                    Some(Message::TerminalSplitPane(None))
                }
                Key::Character("`") if modifiers.command() => {
                    Some(Message::ToggleDrawer(DrawerTab::Terminal))
                }
                Key::Character("s") if modifiers.command() => Some(Message::SaveDocument),
                Key::Character("r") if modifiers.command() && modifiers.shift() => {
                    Some(Message::RefreshAll)
                }
                Key::Named(keyboard::key::Named::F2) => Some(Message::OpenRenameBranchModal(None)),
                Key::Named(keyboard::key::Named::F5) => Some(Message::RefreshSelected),
                Key::Named(keyboard::key::Named::F7) => Some(Message::OpenCreateBranch {
                    base_ref: None,
                    prefix: None,
                }),
                Key::Character("m") | Key::Character("M")
                    if modifiers.command() && !modifiers.shift() && !modifiers.alt() =>
                {
                    Some(Message::ShortcutMergeSelectedBranch)
                }
                Key::Character("d") | Key::Character("D")
                    if modifiers.command() && !modifiers.shift() && !modifiers.alt() =>
                {
                    Some(Message::ShortcutRebaseSelectedBranch)
                }
                Key::Named(keyboard::key::Named::Escape) => Some(Message::CloseOverlays),
                Key::Named(keyboard::key::Named::ArrowUp) if modifiers == Modifiers::ALT => {
                    Some(Message::SelectPrevRepo)
                }
                Key::Named(keyboard::key::Named::ArrowDown) if modifiers == Modifiers::ALT => {
                    Some(Message::SelectNextRepo)
                }
                _ => None,
            }));
            subs.push(event::listen_with(handle_window_event));
        }

        Subscription::batch(subs)
    }
}

fn handle_window_event(
    ev: iced::Event,
    status: event::Status,
    id: window::Id,
) -> Option<Message> {
    match ev {
        iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
            Some(Message::CursorMoved(id, position))
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
            key, modifiers, ..
        }) if status == event::Status::Ignored => {
            if modifiers.command() && modifiers.shift() && !modifiers.alt() {
                match key.as_ref() {
                    Key::Character("t") | Key::Character("T") => {
                        return Some(Message::UnstageSelectedFiles);
                    }
                    _ => {}
                }
            } else if modifiers.command() && !modifiers.shift() && !modifiers.alt() {
                match key.as_ref() {
                    Key::Character("t") | Key::Character("T") => {
                        return Some(Message::StageSelectedFiles);
                    }
                    Key::Character("z") | Key::Character("Z") => {
                        return Some(Message::DiscardSelectedFiles);
                    }
                    Key::Character("c") | Key::Character("C") => {
                        return Some(Message::CopyDiffSelection(id));
                    }
                    Key::Character("a") | Key::Character("A") => {
                        return Some(Message::SelectAllFocused(id));
                    }
                    _ => {}
                }
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
    match ev {
        iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
            Some(Message::CursorMoved(id, position))
        }
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            Some(Message::EndResize)
        }
        iced::Event::Window(window::Event::Resized(size)) => Some(Message::WindowResized(id, size)),
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
