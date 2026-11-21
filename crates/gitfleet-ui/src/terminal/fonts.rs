use iced::font::{Family, Style, Weight};
use iced::Font;
use std::collections::{HashMap, HashSet};
#[cfg(target_os = "windows")]
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
#[derive(Debug, Clone)]
pub struct TerminalFontCatalog {
    pub default_font: String,
    pub available: Vec<String>,
}

static FONT_INTERNER: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();

pub fn intern_font_name(name: &str) -> &'static str {
    let map = FONT_INTERNER.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut guard) = map.lock() {
        if let Some(&existing) = guard.get(name) {
            return existing;
        }
        let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
        guard.insert(name.to_string(), leaked);
        return leaked;
    }
    "Consolas"
}

pub fn terminal_font(name: &str, bold: bool, italic: bool) -> Font {
    Font {
        family: Family::Name(intern_font_name(name)),
        weight: if bold { Weight::Bold } else { Weight::Normal },
        style: if italic { Style::Italic } else { Style::Normal },
        ..Font::DEFAULT
    }
}

pub fn cell_metrics(font_name: &str, font_size: f32) -> (f32, f32) {
    let lower = font_name.to_lowercase();
    let ratio = if lower.contains("consolas") {
        0.550
    } else if lower.contains("cascadia") || lower.contains("caskaydia") {
        0.586
    } else if lower.contains("fira") {
        0.610
    } else if lower.contains("iosevka") {
        0.500
    } else {
        // JetBrains Mono, MesloLGS NF, Monaspace, Hack, Menlo, etc.
        0.600
    };
    let cell_w = (font_size * ratio).max(5.5);
    let cell_h = (font_size * 1.28).round().max(12.0);
    (cell_w, cell_h)
}

/// Discovers installed monospace / Nerd Font families from the OS font database and
/// detects the preferred font face from Windows Terminal `settings.json` when available.
pub fn detect_terminal_fonts() -> TerminalFontCatalog {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    let mut all_families: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut candidate_mono: Vec<String> = Vec::new();
    let mut seen_mono: HashSet<String> = HashSet::new();

    for face in db.faces() {
        for (fam, _) in &face.families {
            let trimmed = fam.trim();
            if trimmed.is_empty() {
                continue;
            }
            let key = trimmed.to_lowercase();
            if seen.insert(key.clone()) {
                all_families.push(trimmed.to_string());
            }
            let looks_terminal = face.monospaced
                || key.contains("nerd")
                || key.ends_with(" nf")
                || key.contains(" nf ")
                || key.contains("nfm")
                || key.contains("mono")
                || key.contains("code")
                || key.contains("consolas")
                || key.contains("cascadia")
                || key.contains("caskaydia")
                || key.contains("meslo")
                || key.contains("monaspace")
                || key.contains("hack")
                || key.contains("courier")
                || key.contains("menlo")
                || key.contains("iosevka")
                || key.contains("inconsolata");
            if looks_terminal && seen_mono.insert(key) {
                candidate_mono.push(trimmed.to_string());
            }
        }
    }

    let is_nerd = |name: &str| -> bool {
        let l = name.to_lowercase();
        l.contains("nerd") || l.ends_with(" nf") || l.contains(" nf ") || l.contains("nfm")
    };

    candidate_mono.sort_by(|a, b| {
        let rank = |s: &str| -> u8 {
            let l = s.to_lowercase();
            if l.contains("caskaydia") && is_nerd(s) {
                0
            } else if l.contains("jetbrains") && is_nerd(s) {
                1
            } else if l.contains("meslo") && is_nerd(s) {
                2
            } else if is_nerd(s) {
                3
            } else if l.contains("cascadia") || l.contains("jetbrains") || l.contains("fira") {
                4
            } else if l.contains("consolas") || l.contains("sf mono") || l.contains("menlo") {
                5
            } else {
                6
            }
        };
        rank(a).cmp(&rank(b)).then_with(|| a.cmp(b))
    });

    // Try matching Windows Terminal's configured font face first
    let wt_face = detect_windows_terminal_font_face();
    let matched_wt = wt_face
        .as_deref()
        .and_then(|wanted| match_font_family(wanted, &all_families));

    let default_font = matched_wt
        .or_else(|| candidate_mono.first().cloned())
        .unwrap_or_else(|| {
            if cfg!(target_os = "windows") {
                "Consolas".to_string()
            } else {
                "Monospace".to_string()
            }
        });

    if !candidate_mono
        .iter()
        .any(|f| f.eq_ignore_ascii_case(&default_font))
    {
        candidate_mono.insert(0, default_font.clone());
    }

    TerminalFontCatalog {
        default_font,
        available: candidate_mono,
    }
}

fn match_font_family(wanted: &str, installed: &[String]) -> Option<String> {
    let w_lower = wanted.trim().to_lowercase();
    if w_lower.is_empty() {
        return None;
    }
    // 1. Exact case-insensitive match
    if let Some(exact) = installed.iter().find(|f| f.to_lowercase() == w_lower) {
        return Some(exact.clone());
    }
    // 2. Normalized match (ignoring spaces & Nerd Font / NF / NFM / Complete suffixes)
    let normalize = |s: &str| -> String {
        s.to_lowercase()
            .replace("windows compatible", "")
            .replace("complete", "")
            .replace("nerd font mono", "nf")
            .replace("nerd font", "nf")
            .replace("nfm", "nf")
            .replace([' ', '-', '_'], "")
    };
    let w_norm = normalize(&w_lower);
    if let Some(m) = installed.iter().find(|f| normalize(f) == w_norm) {
        return Some(m.clone());
    }
    // 3. Prefix / contains match on base name
    installed
        .iter()
        .find(|f| {
            let fn_norm = normalize(f);
            fn_norm.starts_with(&w_norm) || w_norm.starts_with(&fn_norm)
        })
        .cloned()
}

fn detect_windows_terminal_font_face() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        let local_app = std::env::var("LOCALAPPDATA").ok()?;
        let base = PathBuf::from(local_app);
        let candidates = [
            base.join(r"Packages\Microsoft.WindowsTerminal_8wekyb3d8bbwe\LocalState\settings.json"),
            base.join(
                r"Packages\Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe\LocalState\settings.json",
            ),
            base.join(r"Microsoft\Windows Terminal\settings.json"),
        ];
        for path in candidates {
            if let Ok(raw) = std::fs::read_to_string(&path)
                && let Some(face) = extract_wt_font_face(&raw)
            {
                return Some(face);
            }
        }
        None
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

#[cfg(any(target_os = "windows", test))]
fn extract_wt_font_face(raw: &str) -> Option<String> {
    // Windows Terminal settings.json allows JSONC comments, so scan lines for `"face": "..."`
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }
        if let Some(pos) = trimmed.find("\"face\"") {
            let rest = &trimmed[pos + 6..];
            if let Some(colon) = rest.find(':') {
                let after = rest[colon + 1..].trim();
                if let Some(stripped) = after.strip_prefix('"')
                    && let Some(end) = stripped.find('"')
                {
                    let face = stripped[..end].trim();
                    if !face.is_empty() {
                        return Some(face.to_string());
                    }
                }
            }
        }
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_windows_terminal_font_and_matches_installed_family() {
        let sample = r#"
        {
            // comment
            "profiles": {
                "defaults": {
                    "font": {
                        "face": "CaskaydiaCove Nerd Font"
                    }
                }
            }
        }
        "#;
        assert_eq!(
            extract_wt_font_face(sample).as_deref(),
            Some("CaskaydiaCove Nerd Font")
        );

        let installed = vec![
            "Consolas".to_string(),
            "CaskaydiaCove NF".to_string(),
            "Monaspace Xenon NF".to_string(),
        ];
        assert_eq!(
            match_font_family("CaskaydiaCove Nerd Font", &installed).as_deref(),
            Some("CaskaydiaCove NF")
        );
    }

    #[test]
    fn detects_local_system_terminal_fonts() {
        let catalog = detect_terminal_fonts();
        assert!(!catalog.default_font.is_empty());
        assert!(!catalog.available.is_empty());
    }
}
