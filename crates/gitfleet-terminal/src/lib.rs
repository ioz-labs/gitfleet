mod profiles;
mod screen;

pub use profiles::TerminalProfile;
pub use screen::{TermColor, TermRow, TermRun, TermScreenSnapshot, extract_screen_snapshot};

use gitfleet_core::{AppError, AppEvent, AppResult, EventBus, RepoId, SessionId};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use profiles::{detect_profiles, launch_external_terminal};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

struct LiveSession {
    #[allow(dead_code)]
    repo_id: RepoId,
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    parser: Arc<Mutex<vt100::Parser>>,
    dirty_seq: Arc<AtomicU64>,
    exited: Arc<AtomicBool>,
}

/// Embedded PTY session manager powered by `portable-pty` + `vt100`, with multi-profile discovery
/// (PowerShell 7, Windows PowerShell, Git Bash, CMD, WSL distros, POSIX shells) and TrueColor snapshots.
pub struct TerminalService {
    events: EventBus,
    profiles: RwLock<Vec<TerminalProfile>>,
    sessions: RwLock<HashMap<SessionId, LiveSession>>,
}

impl TerminalService {
    pub fn new(events: EventBus) -> Self {
        Self {
            events,
            profiles: RwLock::new(detect_profiles()),
            sessions: RwLock::new(HashMap::new()),
        }
    }

    pub fn profiles(&self) -> Vec<TerminalProfile> {
        self.profiles
            .read()
            .map(|g| g.clone())
            .unwrap_or_else(|_| detect_profiles())
    }

    pub fn refresh_profiles(&self) -> Vec<TerminalProfile> {
        let next = detect_profiles();
        if let Ok(mut guard) = self.profiles.write() {
            *guard = next.clone();
        }
        next
    }

    pub fn profile_by_id(&self, id: &str) -> Option<TerminalProfile> {
        self.profiles().into_iter().find(|p| p.id == id)
    }

    pub fn resolve_default_profile_id(&self, configured: Option<&str>) -> String {
        let all = self.profiles();
        if let Some(pref) = configured
            && all.iter().any(|p| p.id == pref)
        {
            return pref.to_string();
        }
        all.first()
            .map(|p| p.id.clone())
            .unwrap_or_else(|| "pwsh".to_string())
    }

    pub fn open_external_window(&self, cwd: &Path, profile_id: Option<&str>) -> AppResult<()> {
        let profile = if let Some(pid) = profile_id {
            self.profile_by_id(pid)
        } else {
            let resolved = self.resolve_default_profile_id(None);
            self.profile_by_id(&resolved)
        };
        launch_external_terminal(cwd, profile.as_ref())
    }

    pub fn create_in_dir(
        &self,
        repo_id: RepoId,
        cwd: &Path,
        profile_id: &str,
        cols: u16,
        rows: u16,
    ) -> AppResult<SessionId> {
        let profiles = self.profiles();
        let profile = profiles
            .iter()
            .find(|p| p.id == profile_id)
            .or_else(|| profiles.first())
            .ok_or_else(|| AppError::Terminal("No terminal profile available".into()))?;

        let cols = cols.max(30);
        let rows = rows.max(6);

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| AppError::Terminal(format!("Failed opening PTY: {e}")))?;

        let clean_cwd = gitfleet_core::strip_windows_verbatim_prefix(cwd);
        let mut cmd = CommandBuilder::new(&profile.command);
        for arg in &profile.args {
            cmd.arg(arg);
        }
        if clean_cwd.exists() {
            cmd.cwd(&clean_cwd);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "GitFleet");
        #[cfg(target_os = "windows")]
        cmd.env("WT_SESSION", "gitfleet-embedded");

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| AppError::Terminal(format!("Failed spawning shell: {e}")))?;

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| AppError::Terminal(format!("Failed cloning PTY reader: {e}")))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| AppError::Terminal(format!("Failed taking PTY writer: {e}")))?;

        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 2000)));
        let parser_bg = parser.clone();
        let dirty_seq = Arc::new(AtomicU64::new(1));
        let dirty_bg = dirty_seq.clone();
        let exited = Arc::new(AtomicBool::new(false));
        let exited_bg = exited.clone();
        let session_id = SessionId::new();
        let sid_bg = session_id.clone();
        let events_bg = self.events.clone();

        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        exited_bg.store(true, Ordering::Release);
                        dirty_bg.fetch_add(1, Ordering::Release);
                        events_bg.publish(AppEvent::TerminalExit {
                            session_id: sid_bg,
                            exit_code: Some(0),
                        });
                        break;
                    }
                    Ok(n) => {
                        if let Ok(mut p) = parser_bg.lock() {
                            p.process(&buf[..n]);
                        }
                        dirty_bg.fetch_add(1, Ordering::Release);
                        let chunk = String::from_utf8_lossy(&buf[..n]).into_owned();
                        events_bg.publish(AppEvent::TerminalData {
                            session_id: sid_bg.clone(),
                            chunk,
                        });
                    }
                    Err(_) => {
                        exited_bg.store(true, Ordering::Release);
                        dirty_bg.fetch_add(1, Ordering::Release);
                        events_bg.publish(AppEvent::TerminalExit {
                            session_id: sid_bg,
                            exit_code: None,
                        });
                        break;
                    }
                }
            }
        });

        let session = LiveSession {
            repo_id,
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            child: Mutex::new(child),
            parser,
            dirty_seq,
            exited,
        };
        if let Ok(mut guard) = self.sessions.write() {
            guard.insert(session_id.clone(), session);
        }
        Ok(session_id)
    }

    pub fn write(&self, session_id: &SessionId, data: &str) -> AppResult<()> {
        let guard = self
            .sessions
            .read()
            .map_err(|_| AppError::Terminal("Lock poisoned".into()))?;
        let session = guard
            .get(session_id)
            .ok_or_else(|| AppError::Terminal("Terminal session not found".into()))?;
        if let Ok(mut parser) = session.parser.lock()
            && parser.screen().scrollback() > 0
        {
            parser.set_scrollback(0);
            session.dirty_seq.fetch_add(1, Ordering::Release);
        }
        let mut writer = session
            .writer
            .lock()
            .map_err(|_| AppError::Terminal("Writer lock poisoned".into()))?;
        writer
            .write_all(data.as_bytes())
            .map_err(|e| AppError::Terminal(format!("Write failed: {e}")))?;
        let _ = writer.flush();
        Ok(())
    }

    /// Pastes clipboard text into the PTY, honoring DEC private mode 2004 (bracketed paste)
    /// when enabled by the active shell or application (e.g. zsh ZLE, bash 5.1+, fish, vim).
    pub fn paste(&self, session_id: &SessionId, text: &str) -> AppResult<()> {
        if text.is_empty() {
            return Ok(());
        }
        let guard = self
            .sessions
            .read()
            .map_err(|_| AppError::Terminal("Lock poisoned".into()))?;
        let session = guard
            .get(session_id)
            .ok_or_else(|| AppError::Terminal("Terminal session not found".into()))?;
        let bracketed = if let Ok(mut parser) = session.parser.lock() {
            let bp = parser.screen().bracketed_paste();
            if parser.screen().scrollback() > 0 {
                parser.set_scrollback(0);
                session.dirty_seq.fetch_add(1, Ordering::Release);
            }
            bp
        } else {
            false
        };
        let payload = format_paste_payload(text, bracketed);
        let mut writer = session
            .writer
            .lock()
            .map_err(|_| AppError::Terminal("Writer lock poisoned".into()))?;
        writer
            .write_all(payload.as_bytes())
            .map_err(|e| AppError::Terminal(format!("Write failed: {e}")))?;
        let _ = writer.flush();
        Ok(())
    }

    pub fn dirty_seq(&self, session_id: &SessionId) -> u64 {
        let Ok(guard) = self.sessions.read() else {
            return 0;
        };
        guard
            .get(session_id)
            .map(|s| s.dirty_seq.load(Ordering::Acquire))
            .unwrap_or(0)
    }

    pub fn scroll_by(&self, session_id: &SessionId, delta: isize) {
        let Ok(guard) = self.sessions.read() else {
            return;
        };
        let Some(session) = guard.get(session_id) else {
            return;
        };
        if let Ok(mut parser) = session.parser.lock() {
            let cur = parser.screen().scrollback();
            let next = if delta >= 0 {
                cur.saturating_add(delta as usize)
            } else {
                cur.saturating_sub(delta.unsigned_abs())
            };
            parser.set_scrollback(next);
            session.dirty_seq.fetch_add(1, Ordering::Release);
        }
    }

    pub fn screen_snapshot(&self, session_id: &SessionId) -> Option<TermScreenSnapshot> {
        let guard = self.sessions.read().ok()?;
        let session = guard.get(session_id)?;
        let parser = session.parser.lock().ok()?;
        let seq = session.dirty_seq.load(Ordering::Acquire);
        let exited = session.exited.load(Ordering::Acquire);
        Some(extract_screen_snapshot(parser.screen(), seq, exited))
    }

    pub fn screen_text(&self, session_id: &SessionId) -> String {
        let Ok(guard) = self.sessions.read() else {
            return String::new();
        };
        let Some(session) = guard.get(session_id) else {
            return String::new();
        };
        let Ok(parser) = session.parser.lock() else {
            return String::new();
        };
        parser.screen().contents()
    }

    pub fn resize(&self, session_id: &SessionId, cols: u16, rows: u16) -> AppResult<()> {
        let cols = cols.max(30);
        let rows = rows.max(6);
        let guard = self
            .sessions
            .read()
            .map_err(|_| AppError::Terminal("Lock poisoned".into()))?;
        let session = guard
            .get(session_id)
            .ok_or_else(|| AppError::Terminal("Terminal session not found".into()))?;
        if let Ok(m) = session.master.lock() {
            let _ = m.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
        if let Ok(mut p) = session.parser.lock() {
            p.set_size(rows, cols);
        }
        session.dirty_seq.fetch_add(1, Ordering::Release);
        Ok(())
    }

    pub fn close(&self, session_id: &SessionId) -> AppResult<()> {
        let removed = if let Ok(mut guard) = self.sessions.write() {
            guard.remove(session_id)
        } else {
            None
        };
        if let Some(session) = removed
            && let Ok(mut child) = session.child.lock()
        {
            let _ = child.kill();
        }
        Ok(())
    }
}

/// Formats clipboard text for PTY input, normalizing CRLF/LF newlines to CR (`\r`)
/// and wrapping with DEC bracketed-paste markers (`\x1b[200~` ... `\x1b[201~`)
/// when the active terminal screen has enabled mode `?2004h` (e.g. zsh, bash 5.1+, fish, vim).
pub fn format_paste_payload(text: &str, bracketed_paste: bool) -> String {
    if text.is_empty() {
        return String::new();
    }
    let normalized = text.replace("\r\n", "\r").replace('\n', "\r");
    if !bracketed_paste {
        return normalized;
    }
    let mut cleaned = normalized;
    loop {
        let next = cleaned
            .replace("\x1b[200~", "")
            .replace("\x1b[201~", "")
            .replace("\u{009b}200~", "")
            .replace("\u{009b}201~", "");
        if next.len() == cleaned.len() {
            break;
        }
        cleaned = next;
    }
    format!("\x1b[200~{cleaned}\x1b[201~")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_paste_payload_normalizes_line_endings_and_bracketed_mode() {
        assert_eq!(format_paste_payload("", false), "");
        assert_eq!(format_paste_payload("", true), "");
        assert_eq!(
            format_paste_payload("echo hi\r\nls -la\n", false),
            "echo hi\rls -la\r"
        );
        assert_eq!(
            format_paste_payload("echo hi\r\nls -la\n", true),
            "\x1b[200~echo hi\rls -la\r\x1b[201~"
        );
    }

    #[test]
    fn format_paste_payload_strips_nested_bracketed_paste_escape_injection() {
        let malicious = "safe\x1b[201~rm -rf /\x1b[200~done";
        assert_eq!(
            format_paste_payload(malicious, true),
            "\x1b[200~saferm -rf /done\x1b[201~"
        );
        let nested = "a\x1b[20\x1b[201~1~b\u{009b}201~c";
        assert_eq!(format_paste_payload(nested, true), "\x1b[200~abc\x1b[201~");
    }

    #[test]
    fn vt100_tracks_zsh_bracketed_paste_mode() {
        let mut parser = vt100::Parser::new(24, 80, 100);
        assert!(!parser.screen().bracketed_paste());
        parser.process(b"\x1b[?2004h");
        assert!(parser.screen().bracketed_paste());
        parser.process(b"\x1b[?2004l");
        assert!(!parser.screen().bracketed_paste());
    }
}
