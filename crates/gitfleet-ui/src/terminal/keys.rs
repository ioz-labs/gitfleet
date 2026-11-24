use iced::keyboard::key::{Code, Named, Physical};
use iced::keyboard::{self, Key, Modifiers};

/// Matches a logical ASCII letter shortcut robustly across Linux XKB/Wayland/X11 layouts:
/// checks `key`, `modified_key` (including the corresponding ASCII control byte 0x01..=0x1a),
/// and falls back to `physical_key` when the active keyboard layout produces a non-ASCII
/// character or `Key::Unidentified` (without hijacking alternative Latin layouts like Dvorak).
pub fn is_ascii_letter_key(
    key: &Key,
    modified_key: &Key,
    physical_key: Physical,
    letter: char,
    code: Code,
) -> bool {
    let lower = letter.to_ascii_lowercase();
    let ctrl_char = ((lower as u8) - b'a' + 1) as char;

    let matches_char = |k: &Key| match k.as_ref() {
        Key::Character(s) => {
            let mut chars = s.chars();
            match (chars.next(), chars.next()) {
                (Some(ch), None) => ch.to_ascii_lowercase() == lower || ch == ctrl_char,
                _ => false,
            }
        }
        _ => false,
    };

    if matches_char(key) || matches_char(modified_key) {
        return true;
    }

    let is_other_ascii_letter = matches!(
        key.as_ref(),
        Key::Character(s) if s.len() == 1 && s.as_bytes()[0].is_ascii_alphabetic()
    );
    !is_other_ascii_letter && physical_key == code
}

/// Returns true when a terminal keypress should paste from the system clipboard:
/// `Ctrl+V` / `Ctrl+Shift+V` (or `Cmd+V` / `Cmd+Shift+V` on macOS),
/// `Shift+Insert`, or the dedicated `Paste` key.
pub fn is_terminal_paste_shortcut(
    key: &Key,
    modified_key: &Key,
    physical_key: Physical,
    modifiers: Modifiers,
) -> bool {
    if matches!(key.as_ref(), Key::Named(Named::Paste))
        || matches!(modified_key.as_ref(), Key::Named(Named::Paste))
    {
        return true;
    }
    if modifiers.shift()
        && !modifiers.control()
        && !modifiers.alt()
        && !modifiers.logo()
        && (matches!(key.as_ref(), Key::Named(Named::Insert))
            || matches!(modified_key.as_ref(), Key::Named(Named::Insert)))
    {
        return true;
    }
    let command_without_alt =
        modifiers.command() && !modifiers.alt() && (!modifiers.logo() || cfg!(target_os = "macos"));
    command_without_alt && is_ascii_letter_key(key, modified_key, physical_key, 'v', Code::KeyV)
}

// ── Keyboard Event → VT100/PTY Input Bytes ────────────────────────────────────

pub fn key_to_pty_bytes(
    key: &Key,
    modifiers: Modifiers,
    text: Option<&str>,
) -> Option<String> {
    // Ctrl+<letter> (without Alt) -> ASCII control code 0x01..=0x1a
    if modifiers.control() && !modifiers.alt() {
        match key.as_ref() {
            Key::Character(s) => {
                let ch = s.chars().next()?.to_ascii_lowercase();
                if ch.is_ascii_lowercase() {
                    let ctrl_byte = (ch as u8) - b'a' + 1;
                    return Some((ctrl_byte as char).to_string());
                }
                match ch {
                    '[' => return Some("\x1b".into()),
                    '\\' => return Some("\x1c".into()),
                    ']' => return Some("\x1d".into()),
                    '^' | '6' => return Some("\x1e".into()),
                    '_' | '-' => return Some("\x1f".into()),
                    ' ' | '@' => return Some("\x00".into()),
                    _ => {}
                }
            }
            Key::Named(keyboard::key::Named::ArrowLeft) => return Some("\x1b[1;5D".into()),
            Key::Named(keyboard::key::Named::ArrowRight) => return Some("\x1b[1;5C".into()),
            Key::Named(keyboard::key::Named::ArrowUp) => return Some("\x1b[1;5A".into()),
            Key::Named(keyboard::key::Named::ArrowDown) => return Some("\x1b[1;5B".into()),
            Key::Named(keyboard::key::Named::Backspace) => return Some("\x08".into()),
            Key::Named(keyboard::key::Named::Delete) => return Some("\x1b[3;5~".into()),
            Key::Named(keyboard::key::Named::Home) => return Some("\x1b[1;5H".into()),
            Key::Named(keyboard::key::Named::End) => return Some("\x1b[1;5F".into()),
            _ => {}
        }
    }

    match key.as_ref() {
        Key::Named(named) => {
            use keyboard::key::Named;
            let seq = match named {
                Named::Enter => "\r",
                Named::Backspace => "\x7f",
                Named::Tab => {
                    if modifiers.shift() {
                        "\x1b[Z"
                    } else {
                        "\t"
                    }
                }
                Named::Escape => "\x1b",
                Named::Space => " ",
                Named::ArrowUp => "\x1b[A",
                Named::ArrowDown => "\x1b[B",
                Named::ArrowRight => "\x1b[C",
                Named::ArrowLeft => "\x1b[D",
                Named::Home => "\x1b[H",
                Named::End => "\x1b[F",
                Named::Insert => "\x1b[2~",
                Named::Delete => "\x1b[3~",
                Named::PageUp => "\x1b[5~",
                Named::PageDown => "\x1b[6~",
                Named::F1 => "\x1bOP",
                Named::F2 => "\x1bOQ",
                Named::F3 => "\x1bOR",
                Named::F4 => "\x1bOS",
                Named::F5 => "\x1b[15~",
                Named::F6 => "\x1b[17~",
                Named::F7 => "\x1b[18~",
                Named::F8 => "\x1b[19~",
                Named::F9 => "\x1b[20~",
                Named::F10 => "\x1b[21~",
                Named::F11 => "\x1b[23~",
                Named::F12 => "\x1b[24~",
                _ => return None,
            };
            Some(seq.to_string())
        }
        Key::Character(s) => {
            let txt = text.filter(|t| !t.is_empty()).unwrap_or(s);
            if txt.is_empty() {
                return None;
            }
            if modifiers.alt() && !modifiers.control() {
                Some(format!("\x1b{txt}"))
            } else {
                Some(txt.to_string())
            }
        }
        Key::Unidentified => text.filter(|t| !t.is_empty()).map(String::from),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_control_and_navigation_keys_to_vt100() {
        assert_eq!(
            key_to_pty_bytes(&Key::Character("c".into()), Modifiers::CTRL, None).as_deref(),
            Some("\x03")
        );
        assert_eq!(
            key_to_pty_bytes(
                &Key::Named(keyboard::key::Named::Enter),
                Modifiers::empty(),
                None
            )
            .as_deref(),
            Some("\r")
        );
        assert_eq!(
            key_to_pty_bytes(
                &Key::Named(keyboard::key::Named::ArrowUp),
                Modifiers::empty(),
                None
            )
            .as_deref(),
            Some("\x1b[A")
        );
    }

    #[test]
    fn recognizes_terminal_paste_shortcuts_across_layouts() {
        let cmd = Modifiers::COMMAND;
        // Standard Ctrl+V / Cmd+V
        assert!(is_terminal_paste_shortcut(
            &Key::Character("v".into()),
            &Key::Character("v".into()),
            Physical::Code(Code::KeyV),
            cmd,
        ));
        // Ctrl+Shift+V / Cmd+Shift+V
        assert!(is_terminal_paste_shortcut(
            &Key::Character("v".into()),
            &Key::Character("V".into()),
            Physical::Code(Code::KeyV),
            cmd | Modifiers::SHIFT,
        ));
        // X11/Wayland control byte 0x16 in modified_key
        assert!(is_terminal_paste_shortcut(
            &Key::Unidentified,
            &Key::Character("\x16".into()),
            Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
            cmd,
        ));
        // Non-Latin layout (e.g. Cyrillic 'м') falls back to physical KeyV
        assert!(is_terminal_paste_shortcut(
            &Key::Character("м".into()),
            &Key::Character("м".into()),
            Physical::Code(Code::KeyV),
            cmd,
        ));
        // Dvorak layout: physical KeyV mapped to ASCII 'k' must NOT hijack Ctrl+K
        assert!(!is_terminal_paste_shortcut(
            &Key::Character("k".into()),
            &Key::Character("k".into()),
            Physical::Code(Code::KeyV),
            cmd,
        ));
        // Dvorak layout: logical 'v' at physical Period DOES trigger paste
        assert!(is_terminal_paste_shortcut(
            &Key::Character("v".into()),
            &Key::Character("v".into()),
            Physical::Code(Code::Period),
            cmd,
        ));
        // Shift+Insert and dedicated Paste key
        assert!(is_terminal_paste_shortcut(
            &Key::Named(Named::Insert),
            &Key::Named(Named::Insert),
            Physical::Code(Code::Insert),
            Modifiers::SHIFT,
        ));
        assert!(is_terminal_paste_shortcut(
            &Key::Named(Named::Paste),
            &Key::Named(Named::Paste),
            Physical::Code(Code::Paste),
            Modifiers::empty(),
        ));
        // Alt+Ctrl+V should not trigger paste
        assert!(!is_terminal_paste_shortcut(
            &Key::Character("v".into()),
            &Key::Character("v".into()),
            Physical::Code(Code::KeyV),
            cmd | Modifiers::ALT,
        ));
    }
}
