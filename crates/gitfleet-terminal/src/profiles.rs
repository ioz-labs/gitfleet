use gitfleet_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[cfg(target_os = "windows")]
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalProfile {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub short_label: String,
    pub command: String,
    pub args: Vec<String>,
}

pub fn detect_profiles() -> Vec<TerminalProfile> {
    let mut out = Vec::new();

    #[cfg(target_os = "windows")]
    {
        // 1. PowerShell 7+ (pwsh.exe)
        if let Some(pwsh) = find_pwsh_windows() {
            out.push(TerminalProfile {
                id: "pwsh".into(),
                label: "PowerShell".into(),
                short_label: "pwsh".into(),
                command: pwsh.to_string_lossy().into_owned(),
                args: vec!["-NoLogo".into()],
            });
        }

        // 2. Windows PowerShell 5.1 (powershell.exe)
        let win_ps = which::which("powershell")
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "powershell.exe".into());
        out.push(TerminalProfile {
            id: "powershell".into(),
            label: "Windows PowerShell".into(),
            short_label: "ps5".into(),
            command: win_ps,
            args: vec!["-NoLogo".into()],
        });

        // 3. Git Bash
        if let Some(git_bash) = find_git_bash_windows() {
            out.push(TerminalProfile {
                id: "git-bash".into(),
                label: "Git Bash".into(),
                short_label: "bash".into(),
                command: git_bash.to_string_lossy().into_owned(),
                args: vec!["--login".into(), "-i".into()],
            });
        }

        // 4. Command Prompt
        out.push(TerminalProfile {
            id: "cmd".into(),
            label: "Command Prompt".into(),
            short_label: "cmd".into(),
            command: "cmd.exe".into(),
            args: vec![],
        });

        // 5. WSL distros (e.g., Ubuntu-24.04)
        for distro in detect_wsl_distros_windows() {
            out.push(TerminalProfile {
                id: format!("wsl:{distro}"),
                label: format!("{distro} (WSL)"),
                short_label: "wsl".into(),
                command: "wsl.exe".into(),
                args: vec!["-d".into(), distro],
            });
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut seen = std::collections::HashSet::new();
        if let Ok(default_shell) = std::env::var("SHELL")
            && !default_shell.trim().is_empty()
        {
            let name = Path::new(&default_shell)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "sh".into());
            seen.insert(name.clone());
            out.push(TerminalProfile {
                id: name.clone(),
                label: format!("{name} (System)"),
                short_label: name,
                command: default_shell,
                args: vec![],
            });
        }
        for (id, label, bin) in [
            ("zsh", "Zsh", "zsh"),
            ("bash", "Bash", "bash"),
            ("fish", "Fish", "fish"),
            ("nu", "Nushell", "nu"),
            ("pwsh", "PowerShell", "pwsh"),
            ("sh", "POSIX sh", "sh"),
        ] {
            if seen.contains(id) {
                continue;
            }
            if let Ok(path) = which::which(bin) {
                seen.insert(id.to_string());
                out.push(TerminalProfile {
                    id: id.into(),
                    label: label.into(),
                    short_label: id.into(),
                    command: path.to_string_lossy().into_owned(),
                    args: vec![],
                });
            }
        }
    }

    if out.is_empty() {
        out.push(TerminalProfile {
            id: "default-shell".into(),
            label: "Default Shell".into(),
            short_label: "sh".into(),
            command: if cfg!(target_os = "windows") {
                "cmd.exe".into()
            } else {
                "/bin/sh".into()
            },
            args: vec![],
        });
    }

    out
}

pub fn launch_external_terminal(cwd: &Path, profile: Option<&TerminalProfile>) -> AppResult<()> {
    #[cfg(target_os = "windows")]
    {
        if let Ok(wt) = which::which("wt") {
            let mut cmd = std::process::Command::new(wt);
            if cwd.exists() {
                cmd.args(["-d", &cwd.to_string_lossy()]);
            }
            if let Some(prof) = profile {
                cmd.arg("--");
                cmd.arg(&prof.command);
                cmd.args(&prof.args);
            }
            cmd.spawn()
                .map_err(|e| AppError::Terminal(format!("Failed opening Windows Terminal: {e}")))?;
            return Ok(());
        }

        let prof = profile
            .ok_or_else(|| AppError::Terminal("No terminal profile available".into()))?;
        let mut cmd = std::process::Command::new("cmd.exe");
        cmd.args(["/C", "start", "", &prof.command]);
        cmd.args(&prof.args);
        if cwd.exists() {
            cmd.current_dir(cwd);
        }
        cmd.spawn()
            .map_err(|e| AppError::Terminal(format!("Failed opening external terminal: {e}")))?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        let _ = profile;
        std::process::Command::new("open")
            .args(["-a", "Terminal"])
            .arg(cwd)
            .spawn()
            .map_err(|e| AppError::Terminal(format!("Failed opening Terminal.app: {e}")))?;
        Ok(())
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        let _ = profile;
        for bin in ["x-terminal-emulator", "gnome-terminal", "konsole", "alacritty", "kitty"] {
            if let Ok(path) = which::which(bin) {
                let mut cmd = std::process::Command::new(path);
                if cwd.exists() {
                    cmd.current_dir(cwd);
                }
                cmd.spawn().map_err(|e| {
                    AppError::Terminal(format!("Failed opening external terminal: {e}"))
                })?;
                return Ok(());
            }
        }
        Err(AppError::Terminal("No external terminal emulator found on PATH".into()))
    }
}

#[cfg(target_os = "windows")]
fn find_pwsh_windows() -> Option<PathBuf> {
    if let Ok(p) = which::which("pwsh") {
        return Some(p);
    }
    let candidates = [
        PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe"),
        PathBuf::from(r"C:\Program Files\PowerShell\7-preview\pwsh.exe"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

#[cfg(target_os = "windows")]
fn find_git_bash_windows() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(pf) = std::env::var("ProgramFiles") {
        candidates.push(PathBuf::from(&pf).join("Git").join("bin").join("bash.exe"));
    }
    if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
        candidates.push(PathBuf::from(&pf86).join("Git").join("bin").join("bash.exe"));
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        candidates.push(
            PathBuf::from(&local)
                .join("Programs")
                .join("Git")
                .join("bin")
                .join("bash.exe"),
        );
    }
    if let Ok(git_exe) = which::which("git")
        && let Some(git_root) = git_exe.parent().and_then(|p| p.parent())
    {
        candidates.push(git_root.join("bin").join("bash.exe"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

#[cfg(target_os = "windows")]
fn detect_wsl_distros_windows() -> Vec<String> {
    use std::os::windows::process::CommandExt;
    let Ok(wsl) = which::which("wsl") else {
        return Vec::new();
    };
    let mut cmd = std::process::Command::new(wsl);
    cmd.args(["-l", "-q"]);
    cmd.creation_flags(0x0800_0000);
    let Ok(out) = cmd.output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    parse_wsl_distro_list(&out.stdout)
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn parse_wsl_distro_list(raw: &[u8]) -> Vec<String> {
    // wsl.exe -l -q usually outputs UTF-16LE on Windows
    let decoded = if raw.len() >= 2 && raw.iter().skip(1).step_by(2).take(8).any(|&b| b == 0) {
        let u16s: Vec<u16> = raw
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&u16s)
    } else {
        String::from_utf8_lossy(raw).into_owned()
    };

    decoded
        .lines()
        .map(|l| l.trim_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}' || ch == '\0'))
        .filter(|name| {
            !name.is_empty()
                && !name.eq_ignore_ascii_case("docker-desktop")
                && !name.eq_ignore_ascii_case("docker-desktop-data")
        })
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_utf16le_wsl_distros_and_filters_docker() {
        let text = "Ubuntu-24.04\r\ndocker-desktop\r\ndocker-desktop-data\r\nDebian\r\n";
        let mut bytes = Vec::new();
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let distros = parse_wsl_distro_list(&bytes);
        assert_eq!(distros, vec!["Ubuntu-24.04", "Debian"]);
    }
}
