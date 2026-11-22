use std::path::{Path, PathBuf};

/// Removes Windows Win32 verbatim extended-length path prefixes (`\\?\` and `\\?\UNC\`)
/// returned by `std::fs::canonicalize` on Windows so external tools (PowerShell, CMD,
/// Git Bash, VS Code `code .`, Windows Explorer, Node.js) receive normal paths.
///
/// - `\\?\D:\Project\repo` -> `D:\Project\repo`
/// - `\\?\UNC\server\share\repo` -> `\\server\share\repo`
/// - `\\?\Volume{...}\...` -> kept untouched (device/volume GUID path)
pub fn strip_windows_verbatim_str(raw: &str) -> Option<String> {
    if let Some(unc_tail) = raw
        .strip_prefix(r"\\?\UNC\")
        .or_else(|| raw.strip_prefix(r"\\?\unc\"))
    {
        return Some(format!(r"\\{unc_tail}"));
    }
    if let Some(rest) = raw.strip_prefix(r"\\?\") {
        let bytes = rest.as_bytes();
        // Only strip when followed by a drive specifier (`C:\...` or `C:`)
        if bytes.len() >= 2
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes.len() == 2 || bytes[2] == b'\\' || bytes[2] == b'/')
        {
            return Some(rest.to_string());
        }
    }
    None
}

/// Returns a cleaned `PathBuf` without Windows `\\?\` / `\\?\UNC\` verbatim prefixes.
pub fn strip_windows_verbatim_prefix(path: &Path) -> PathBuf {
    if let Some(s) = path.to_str()
        && let Some(cleaned) = strip_windows_verbatim_str(s)
    {
        return PathBuf::from(cleaned);
    }
    path.to_path_buf()
}

/// Canonicalizes `path` (falling back to `path` if it does not exist yet) and strips
/// Windows verbatim prefixes (`\\?\` / `\\?\UNC\`).
pub fn canonicalize_clean(path: &Path) -> PathBuf {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    strip_windows_verbatim_prefix(&resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_windows_verbatim_drive_and_unc_prefixes_safely() {
        assert_eq!(
            strip_windows_verbatim_str(r"\\?\D:\Project\electrogit\gitfleet"),
            Some(r"D:\Project\electrogit\gitfleet".to_string())
        );
        assert_eq!(
            strip_windows_verbatim_str(r"\\?\C:"),
            Some(r"C:".to_string())
        );
        assert_eq!(
            strip_windows_verbatim_str(r"\\?\UNC\fileserver\repos\gitfleet"),
            Some(r"\\fileserver\repos\gitfleet".to_string())
        );
        // Device / Volume GUID paths must NOT be stripped
        assert_eq!(
            strip_windows_verbatim_str(r"\\?\Volume{1234-5678}\foo"),
            None
        );
        // Normal paths are untouched
        assert_eq!(
            strip_windows_verbatim_str(r"D:\Project\electrogit\gitfleet"),
            None
        );
        assert_eq!(strip_windows_verbatim_str("/home/user/repo"), None);

        let cleaned = strip_windows_verbatim_prefix(Path::new(r"\\?\D:\Project\electrogit\gitfleet"));
        assert_eq!(cleaned, PathBuf::from(r"D:\Project\electrogit\gitfleet"));
    }
}
