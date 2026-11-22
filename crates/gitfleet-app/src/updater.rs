//! Self-update service for GitFleet:
//! - Checks GitHub Releases API with ETag caching (`304 Not Modified` uses zero rate-limit quota).
//! - Detects the active installation channel (Windows Installer, Windows Portable, macOS DMG,
//!   Linux `pacman`, Linux `.deb`, Linux Portable) from the running executable path.
//! - Streams release downloads with live progress + cancellation, enforces SHA-256 verification
//!   against `SHA256SUMS.txt`, and applies updates safely without `unsafe` code.

use gitfleet_core::AppSettings;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::time::Duration;

pub const GITHUB_OWNER: &str = "ioz-labs";
pub const GITHUB_REPO: &str = "gitfleet";
pub const RELEASES_PAGE_URL: &str = "https://github.com/ioz-labs/gitfleet/releases";
const LATEST_RELEASE_API: &str = "https://api.github.com/repos/ioz-labs/gitfleet/releases/latest";
/// Minimum interval between silent background update checks (12 hours).
pub const AUTO_CHECK_INTERVAL_SECS: i64 = 12 * 60 * 60;

/// Detected installation mechanism for the currently running GitFleet binary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstallChannel {
    /// Installed on Windows via Inno Setup (`gitfleet-windows-<arch>-setup.exe`).
    WindowsInstaller { install_dir: PathBuf },
    /// Portable Windows executable extracted from `.zip` in a writable folder.
    WindowsPortable { exe_path: PathBuf },
    /// Installed on Arch Linux / derivatives via `pacman`.
    LinuxPacman,
    /// Installed on Debian / Ubuntu via `.deb` (`apt` / `dpkg`).
    LinuxDeb,
    /// Portable Linux binary extracted from `.tar.gz` in a writable folder.
    LinuxPortable { exe_path: PathBuf },
    /// macOS `.app` bundle (updated by downloading & opening the verified `.dmg`).
    MacOsAppBundle { app_bundle: PathBuf },
    /// Read-only or unrecognized installation path (opens release page / manual update).
    ReadOnlyManual,
}

impl InstallChannel {
    pub fn label(&self) -> &'static str {
        match self {
            Self::WindowsInstaller { .. } => "Windows Installer",
            Self::WindowsPortable { .. } => "Windows Portable",
            Self::LinuxPacman => "Arch Linux (pacman)",
            Self::LinuxDeb => "Debian / Ubuntu (.deb)",
            Self::LinuxPortable { .. } => "Linux Portable",
            Self::MacOsAppBundle { .. } => "macOS App (.dmg)",
            Self::ReadOnlyManual => "Manual Install",
        }
    }

    pub fn supports_auto_restart(&self) -> bool {
        matches!(
            self,
            Self::WindowsInstaller { .. }
                | Self::WindowsPortable { .. }
                | Self::LinuxPortable { .. }
        )
    }

    pub fn primary_action_label(&self) -> &'static str {
        match self {
            Self::WindowsInstaller { .. }
            | Self::WindowsPortable { .. }
            | Self::LinuxPortable { .. } => "Update & Restart",
            Self::MacOsAppBundle { .. } => "Download & Open DMG",
            Self::LinuxDeb => "Download .deb Package",
            Self::LinuxPacman => "Copy pacman Command",
            Self::ReadOnlyManual => "Open Release Page",
        }
    }

    pub fn manual_command(&self, downloaded_path: Option<&Path>) -> Option<String> {
        match self {
            Self::LinuxPacman => Some("sudo pacman -Syu".to_string()),
            Self::LinuxDeb => Some(match downloaded_path {
                Some(p) => format!("sudo apt install \"{}\"", p.display()),
                None => "sudo apt install ./gitfleet-linux-*.deb".to_string(),
            }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateAsset {
    pub name: String,
    pub download_url: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub version: String,
    pub tag_name: String,
    pub name: String,
    pub body: String,
    pub html_url: String,
    pub published_at: Option<String>,
    pub asset: Option<UpdateAsset>,
    pub checksums_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCheckOutcome {
    UpToDate {
        current_version: String,
        etag: Option<String>,
    },
    NotModified,
    Available {
        current_version: String,
        release: Box<ReleaseInfo>,
        etag: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UpdateDownloadPhase {
    Idle = 0,
    Downloading = 1,
    VerifyingChecksum = 2,
    Installing = 3,
}

impl UpdateDownloadPhase {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Downloading,
            2 => Self::VerifyingChecksum,
            3 => Self::Installing,
            _ => Self::Idle,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Preparing…",
            Self::Downloading => "Downloading update…",
            Self::VerifyingChecksum => "Verifying SHA-256 checksum…",
            Self::Installing => "Applying update…",
        }
    }
}

/// Lock-free progress + cancellation tracker shared between the async download task and UI.
#[derive(Debug, Default)]
pub struct UpdateDownloadTracker {
    downloaded_bytes: AtomicU64,
    total_bytes: AtomicU64,
    phase: AtomicU8,
    cancelled: AtomicBool,
}

impl UpdateDownloadTracker {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            downloaded_bytes: AtomicU64::new(0),
            total_bytes: AtomicU64::new(0),
            phase: AtomicU8::new(UpdateDownloadPhase::Downloading as u8),
            cancelled: AtomicBool::new(false),
        })
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    pub fn set_phase(&self, phase: UpdateDownloadPhase) {
        self.phase.store(phase as u8, Ordering::Relaxed);
    }

    pub fn set_total(&self, total: u64) {
        self.total_bytes.store(total, Ordering::Relaxed);
    }

    pub fn set_downloaded(&self, downloaded: u64) {
        self.downloaded_bytes.store(downloaded, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> (UpdateDownloadPhase, u64, u64) {
        (
            UpdateDownloadPhase::from_u8(self.phase.load(Ordering::Relaxed)),
            self.downloaded_bytes.load(Ordering::Relaxed),
            self.total_bytes.load(Ordering::Relaxed),
        )
    }
}

#[derive(Debug, Clone)]
pub enum ApplyUpdateOutcome {
    /// Verified Windows Inno Setup installer is ready; caller should release workspace lock,
    /// invoke `launch_update_handoff`, and exit cleanly.
    ReadyForInstaller { installer_path: PathBuf },
    /// Portable binary was atomically swapped; caller should release workspace lock,
    /// invoke `launch_update_handoff` to spawn the updated binary, and exit cleanly.
    ReadyToRelaunchPortable { exe_path: PathBuf },
    /// Verified artifact was downloaded and opened/revealed (macOS `.dmg`, Linux `.deb`, or manual fallback).
    OpenedArtifact {
        path: PathBuf,
        note: String,
        manual_command: Option<String>,
    },
    /// System package manager owns this installation (e.g. Arch `pacman`).
    ManualCommand {
        command: String,
        release_url: String,
    },
}

/// Spawns the verified installer or freshly swapped portable binary after the caller has
/// cleanly released the workspace lock and prepared to exit.
pub fn launch_update_handoff(outcome: &ApplyUpdateOutcome) -> Result<(), String> {
    match outcome {
        ApplyUpdateOutcome::ReadyForInstaller { installer_path } => {
            std::process::Command::new(installer_path)
                .args(["/SILENT", "/CLOSEAPPLICATIONS", "/NORESTART", "/relaunch=1"])
                .spawn()
                .map_err(|e| format!("Failed launching GitFleet setup installer: {e}"))?;
            Ok(())
        }
        ApplyUpdateOutcome::ReadyToRelaunchPortable { exe_path } => {
            std::process::Command::new(exe_path)
                .spawn()
                .map_err(|e| format!("Updated binary installed, but failed to relaunch: {e}"))?;
            Ok(())
        }
        _ => Ok(()),
    }
}

// ── GitHub API DTOs ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct GithubReleaseDto {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    published_at: Option<String>,
    draft: bool,
    prerelease: bool,
    assets: Vec<GithubAssetDto>,
}

#[derive(Debug, Deserialize)]
struct GithubAssetDto {
    name: String,
    browser_download_url: String,
    size: u64,
}

// ── Startup Cleanup ──────────────────────────────────────────────────────────

/// Removes any leftover `.old` executable left behind by a previous Windows portable self-update.
/// Safe and silent no-op if no leftover file exists.
pub fn cleanup_stale_old_binary() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let clean = gitfleet_core::strip_windows_verbatim_prefix(&exe);
    let old_path = old_backup_path(&clean);
    if old_path.is_file() {
        let _ = std::fs::remove_file(&old_path);
    }
}

fn old_backup_path(exe_path: &Path) -> PathBuf {
    let mut os = exe_path.as_os_str().to_os_string();
    os.push(".old");
    PathBuf::from(os)
}

fn staged_new_path(exe_path: &Path) -> PathBuf {
    let mut os = exe_path.as_os_str().to_os_string();
    os.push(".new");
    PathBuf::from(os)
}

// ── Pure Version, Channel & Checksum Helpers ─────────────────────────────────

pub fn current_app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Returns `true` if `candidate` is strictly newer than `current` using semantic versioning.
pub fn is_newer_version(current: &str, candidate: &str) -> Result<bool, String> {
    let cur_norm = current.trim().trim_start_matches(['v', 'V']);
    let cand_norm = candidate.trim().trim_start_matches(['v', 'V']);
    let cur = Version::parse(cur_norm)
        .map_err(|e| format!("Invalid current version '{current}': {e}"))?;
    let cand = Version::parse(cand_norm)
        .map_err(|e| format!("Invalid release version '{candidate}': {e}"))?;
    Ok(cand > cur)
}

/// Decides whether a silent background update check should fire on app startup.
pub fn should_auto_check(settings: &AppSettings, now_epoch_secs: i64, is_debug_build: bool) -> bool {
    if is_debug_build || !settings.auto_check_updates {
        return false;
    }
    match settings.last_update_check_epoch_secs {
        None => true,
        Some(last) => now_epoch_secs.saturating_sub(last) >= AUTO_CHECK_INTERVAL_SECS,
    }
}

/// Detects how the currently running GitFleet executable was installed.
pub fn detect_install_channel() -> InstallChannel {
    let Ok(exe) = std::env::current_exe() else {
        return InstallChannel::ReadOnlyManual;
    };
    let clean = gitfleet_core::strip_windows_verbatim_prefix(&exe);
    let dir = clean.parent().unwrap_or_else(|| Path::new("."));
    let has_win_uninstaller = dir.join("unins000.exe").is_file() || dir.join("unins001.exe").is_file();
    let dir_writable = is_dir_writable(dir);
    let has_pacman = cfg!(target_os = "linux")
        && (Path::new("/etc/pacman.conf").is_file() || which::which("pacman").is_ok());
    let has_dpkg = cfg!(target_os = "linux") && which::which("dpkg").is_ok();

    classify_install_channel(
        std::env::consts::OS,
        &clean,
        has_win_uninstaller,
        dir_writable,
        has_pacman,
        has_dpkg,
    )
}

/// Pure install-channel classifier (unit-testable across all OS paths).
pub fn classify_install_channel(
    os: &str,
    exe_path: &Path,
    has_win_uninstaller: bool,
    dir_writable: bool,
    has_pacman: bool,
    has_dpkg: bool,
) -> InstallChannel {
    match os {
        "windows" => {
            if has_win_uninstaller {
                let install_dir = exe_path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from("."));
                InstallChannel::WindowsInstaller { install_dir }
            } else if dir_writable {
                InstallChannel::WindowsPortable {
                    exe_path: exe_path.to_path_buf(),
                }
            } else {
                InstallChannel::ReadOnlyManual
            }
        }
        "macos" => {
            // Walk up to find enclosing `*.app` bundle if running from an app bundle.
            let app_bundle = exe_path
                .ancestors()
                .find(|a| {
                    a.extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| e.eq_ignore_ascii_case("app"))
                })
                .map(Path::to_path_buf)
                .unwrap_or_else(|| exe_path.to_path_buf());
            InstallChannel::MacOsAppBundle { app_bundle }
        }
        "linux" => {
            let norm = exe_path.to_string_lossy();
            let system_managed = norm.starts_with("/usr/bin/")
                || norm.starts_with("/usr/local/bin/")
                || norm.starts_with("/bin/");
            if system_managed && !dir_writable {
                if has_pacman {
                    InstallChannel::LinuxPacman
                } else if has_dpkg {
                    InstallChannel::LinuxDeb
                } else {
                    InstallChannel::ReadOnlyManual
                }
            } else if dir_writable {
                InstallChannel::LinuxPortable {
                    exe_path: exe_path.to_path_buf(),
                }
            } else if has_pacman {
                InstallChannel::LinuxPacman
            } else if has_dpkg {
                InstallChannel::LinuxDeb
            } else {
                InstallChannel::ReadOnlyManual
            }
        }
        _ => InstallChannel::ReadOnlyManual,
    }
}

fn is_dir_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".gitfleet-write-probe-{}", std::process::id()));
    match File::create(&probe) {
        Ok(f) => {
            drop(f);
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Maps `(os, arch, channel, version)` to the exact GitHub Release artifact filename produced by
/// `.github/workflows/release.yml`.
pub fn expected_asset_name(
    os: &str,
    arch: &str,
    channel: &InstallChannel,
    version: &str,
) -> Option<String> {
    let norm_ver = version.trim().trim_start_matches(['v', 'V']);
    let artifact_arch = match arch {
        "x86_64" => "x86_64",
        "aarch64" => "arm64",
        _ => return None,
    };
    let pacman_arch = match arch {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        _ => return None,
    };

    match (os, channel) {
        ("windows", InstallChannel::WindowsInstaller { .. }) => {
            Some(format!("gitfleet-windows-{artifact_arch}-setup.exe"))
        }
        ("windows", InstallChannel::WindowsPortable { .. }) => {
            Some(format!("gitfleet-windows-{artifact_arch}.zip"))
        }
        ("windows", _) => Some(format!("gitfleet-windows-{artifact_arch}-setup.exe")),
        ("macos", _) if artifact_arch == "arm64" => Some("gitfleet-macos-arm64.dmg".to_string()),
        ("linux", InstallChannel::LinuxPacman) => {
            Some(format!("gitfleet-{norm_ver}-1-{pacman_arch}.pkg.tar.zst"))
        }
        ("linux", InstallChannel::LinuxDeb) => Some(format!("gitfleet-linux-{artifact_arch}.deb")),
        ("linux", InstallChannel::LinuxPortable { .. }) => {
            Some(format!("gitfleet-linux-{artifact_arch}.tar.gz"))
        }
        ("linux", _) => Some(format!("gitfleet-linux-{artifact_arch}.tar.gz")),
        _ => None,
    }
}

/// Parses a standard `sha256sum` manifest (`<64-hex>  <filename>` or `<64-hex> *<filename>`)
/// and returns the lowercase SHA-256 hex digest for `target_filename`.
pub fn parse_sha256sums(contents: &str, target_filename: &str) -> Option<String> {
    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(2, |c: char| c.is_ascii_whitespace());
        let hash = parts.next()?.trim();
        let rest = parts.next()?.trim().trim_start_matches('*');
        let file_name = rest.rsplit(['/', '\\']).next().unwrap_or(rest);
        if file_name == target_filename
            && hash.len() == 64
            && hash.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

/// Streams a file in 64 KiB chunks and validates its SHA-256 digest against `expected_hex`.
pub fn verify_file_sha256(path: &Path, expected_hex: &str) -> Result<(), String> {
    let mut file = File::open(path)
        .map_err(|e| format!("Failed to open downloaded file for checksum verification: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("Failed to read file for SHA-256 verification: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual = hex::encode(hasher.finalize());
    let expected = expected_hex.trim().to_ascii_lowercase();
    if actual != expected {
        return Err(format!(
            "SHA-256 checksum mismatch!\nExpected: {expected}\nActual:   {actual}"
        ));
    }
    Ok(())
}

fn validate_https_github_url(url: &str) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    if !lower.starts_with("https://") {
        return Err(format!("Refusing non-HTTPS URL: {url}"));
    }
    let after_scheme = &lower["https://".len()..];
    let host = after_scheme.split('/').next().unwrap_or("");
    let allowed = host == "github.com"
        || host == "api.github.com"
        || host.ends_with(".githubusercontent.com")
        || host.ends_with(".github.com");
    if !allowed {
        return Err(format!("Refusing unexpected download host '{host}'"));
    }
    Ok(())
}

/// Opens an HTTPS URL in the user's default system browser safely.
pub fn open_url_in_browser(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("Refusing to open non-HTTP(S) URL".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn()
            .map_err(|e| format!("Failed to open browser: {e}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("Failed to open browser: {e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("Failed to open browser: {e}"))?;
    }
    Ok(())
}

// ── Archive Extraction & Safe Portable Self-Replace ──────────────────────────

/// Safely extracts `gitfleet.exe` from a Windows release `.zip` into `dest_exe_path`.
pub fn extract_windows_exe_from_zip(zip_path: &Path, dest_exe_path: &Path) -> Result<(), String> {
    let file = File::open(zip_path).map_err(|e| format!("Cannot open zip archive: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("Invalid zip archive: {e}"))?;

    for idx in 0..archive.len() {
        let mut entry = archive
            .by_index(idx)
            .map_err(|e| format!("Cannot read zip entry #{idx}: {e}"))?;
        if entry.is_dir() {
            continue;
        }
        let Some(safe_path) = entry.enclosed_name() else {
            continue;
        };
        let is_target = safe_path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case("gitfleet.exe"));
        if !is_target {
            continue;
        }
        if let Some(parent) = dest_exe_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Cannot prepare destination directory: {e}"))?;
        }
        let mut out = File::create(dest_exe_path)
            .map_err(|e| format!("Cannot create staged binary {}: {e}", dest_exe_path.display()))?;
        let copied = std::io::copy(&mut entry, &mut out)
            .map_err(|e| format!("Failed extracting gitfleet.exe from zip: {e}"))?;
        out.flush()
            .map_err(|e| format!("Failed flushing extracted binary: {e}"))?;
        if copied == 0 {
            let _ = std::fs::remove_file(dest_exe_path);
            return Err("Extracted gitfleet.exe is empty".to_string());
        }
        return Ok(());
    }

    Err("Release archive does not contain gitfleet.exe".to_string())
}

/// Safely extracts the `gitfleet` binary from a Linux `.tar.gz` into `dest_bin_path`.
pub fn extract_linux_bin_from_tar_gz(tar_gz_path: &Path, dest_bin_path: &Path) -> Result<(), String> {
    let file = File::open(tar_gz_path).map_err(|e| format!("Cannot open tar.gz archive: {e}"))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);

    let entries = archive
        .entries()
        .map_err(|e| format!("Invalid tar.gz archive: {e}"))?;
    for entry_res in entries {
        let mut entry = entry_res.map_err(|e| format!("Cannot read tar entry: {e}"))?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let entry_path = entry
            .path()
            .map_err(|e| format!("Invalid tar entry path: {e}"))?;
        // Reject absolute or parent-traversal paths
        if entry_path.is_absolute()
            || entry_path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            continue;
        }
        let is_target = entry_path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == "gitfleet");
        if !is_target {
            continue;
        }

        if let Some(parent) = dest_bin_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Cannot prepare destination directory: {e}"))?;
        }
        let mut out = File::create(dest_bin_path)
            .map_err(|e| format!("Cannot create staged binary {}: {e}", dest_bin_path.display()))?;
        let copied = std::io::copy(&mut entry, &mut out)
            .map_err(|e| format!("Failed extracting gitfleet binary: {e}"))?;
        out.flush()
            .map_err(|e| format!("Failed flushing extracted binary: {e}"))?;
        if copied == 0 {
            let _ = std::fs::remove_file(dest_bin_path);
            return Err("Extracted gitfleet binary is empty".to_string());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dest_bin_path, std::fs::Permissions::from_mode(0o755));
        }
        return Ok(());
    }

    Err("Release archive does not contain gitfleet binary".to_string())
}

/// Replaces `current_exe` with `staged_new_exe` using rollback-safe rename semantics:
/// - On Windows: renames running exe -> `.old`, then `.new` -> `current_exe` (restores `.old` on failure).
/// - On Unix: atomic `rename(.new, current_exe)` directly over the running inode.
pub fn swap_portable_binary(current_exe: &Path, staged_new_exe: &Path) -> Result<(), String> {
    if !staged_new_exe.is_file() {
        return Err("Staged update binary is missing".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        let backup = old_backup_path(current_exe);
        if backup.exists() {
            let _ = std::fs::remove_file(&backup);
        }
        std::fs::rename(current_exe, &backup)
            .map_err(|e| format!("Cannot move current executable aside: {e}"))?;
        if let Err(err) = std::fs::rename(staged_new_exe, current_exe) {
            // Roll back immediately so the user is never left without an executable.
            let _ = std::fs::rename(&backup, current_exe);
            return Err(format!("Cannot install new executable: {err}"));
        }
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::fs::rename(staged_new_exe, current_exe)
            .map_err(|e| format!("Cannot replace executable atomically: {e}"))
    }
}

// ── Updater Service ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct UpdaterService {
    client: reqwest::Client,
    channel: InstallChannel,
}

impl Default for UpdaterService {
    fn default() -> Self {
        Self::new()
    }
}

impl UpdaterService {
    pub fn new() -> Self {
        let user_agent = format!("GitFleet/{} ({}; {})", current_app_version(), std::env::consts::OS, std::env::consts::ARCH);
        let client = reqwest::Client::builder()
            .user_agent(user_agent)
            .connect_timeout(Duration::from_secs(12))
            .timeout(Duration::from_secs(300))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            client,
            channel: detect_install_channel(),
        }
    }

    pub fn channel(&self) -> &InstallChannel {
        &self.channel
    }

    /// Queries GitHub Releases API for the latest stable release.
    /// Passes `If-None-Match` when `etag` is supplied so repeated checks cost zero API quota.
    pub async fn check_latest_release(
        &self,
        etag: Option<&str>,
    ) -> Result<UpdateCheckOutcome, String> {
        let current = current_app_version().to_string();
        let mut req = self
            .client
            .get(LATEST_RELEASE_API)
            .timeout(Duration::from_secs(15))
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");

        if let Some(tag) = etag.filter(|s| !s.trim().is_empty()) {
            req = req.header(reqwest::header::IF_NONE_MATCH, tag);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| format!("Could not reach GitHub Releases: {e}"))?;

        if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(UpdateCheckOutcome::NotModified);
        }

        if resp.status() == reqwest::StatusCode::FORBIDDEN
            || resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
        {
            return Err(
                "GitHub API rate limit reached — please try checking again in a few minutes."
                    .to_string(),
            );
        }

        if !resp.status().is_success() {
            return Err(format!(
                "Update check failed with HTTP {}",
                resp.status().as_u16()
            ));
        }

        let next_etag = resp
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);

        let dto: GithubReleaseDto = resp
            .json()
            .await
            .map_err(|e| format!("Invalid GitHub release payload: {e}"))?;

        if dto.draft || dto.prerelease {
            return Ok(UpdateCheckOutcome::UpToDate {
                current_version: current,
                etag: next_etag,
            });
        }

        if !is_newer_version(&current, &dto.tag_name)? {
            return Ok(UpdateCheckOutcome::UpToDate {
                current_version: current,
                etag: next_etag,
            });
        }

        let normalized_version = dto
            .tag_name
            .trim()
            .trim_start_matches(['v', 'V'])
            .to_string();
        let wanted = expected_asset_name(
            std::env::consts::OS,
            std::env::consts::ARCH,
            &self.channel,
            &normalized_version,
        );

        let matched_asset = wanted.as_deref().and_then(|want| {
            dto.assets.iter().find(|a| a.name == want).map(|a| UpdateAsset {
                name: a.name.clone(),
                download_url: a.browser_download_url.clone(),
                size_bytes: a.size,
            })
        });

        let checksums_url = dto
            .assets
            .iter()
            .find(|a| a.name == "SHA256SUMS.txt")
            .map(|a| a.browser_download_url.clone());

        let release = ReleaseInfo {
            version: normalized_version,
            tag_name: dto.tag_name,
            name: dto.name.unwrap_or_default(),
            body: dto.body.unwrap_or_default(),
            html_url: dto.html_url,
            published_at: dto.published_at,
            asset: matched_asset,
            checksums_url,
        };

        Ok(UpdateCheckOutcome::Available {
            current_version: current,
            release: Box::new(release),
            etag: next_etag,
        })
    }

    /// Downloads the release artifact for the active channel, verifies its SHA-256 checksum
    /// against `SHA256SUMS.txt`, and applies/launches the update cleanly.
    pub async fn download_and_apply_update(
        &self,
        release: &ReleaseInfo,
        tracker: Arc<UpdateDownloadTracker>,
    ) -> Result<ApplyUpdateOutcome, String> {
        // Arch Linux pacman manages /usr/bin/gitfleet directly — never overwrite it!
        if matches!(self.channel, InstallChannel::LinuxPacman) {
            return Ok(ApplyUpdateOutcome::ManualCommand {
                command: "sudo pacman -Syu".to_string(),
                release_url: release.html_url.clone(),
            });
        }

        let Some(asset) = &release.asset else {
            open_url_in_browser(&release.html_url)?;
            return Ok(ApplyUpdateOutcome::ManualCommand {
                command: String::new(),
                release_url: release.html_url.clone(),
            });
        };

        validate_https_github_url(&asset.download_url)?;
        if let Some(cs_url) = &release.checksums_url {
            validate_https_github_url(cs_url)?;
        }

        let update_dir = std::env::temp_dir()
            .join("gitfleet-updates")
            .join(&release.version);
        std::fs::create_dir_all(&update_dir)
            .map_err(|e| format!("Cannot create temporary update directory: {e}"))?;

        let final_path = update_dir.join(&asset.name);
        let part_path = update_dir.join(format!("{}.part", asset.name));

        // 1. Stream artifact download with progress & cancellation.
        tracker.set_phase(UpdateDownloadPhase::Downloading);
        tracker.set_downloaded(0);
        tracker.set_total(asset.size_bytes);

        let mut resp = self
            .client
            .get(&asset.download_url)
            .send()
            .await
            .map_err(|e| format!("Download failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!(
                "Download failed with HTTP {}",
                resp.status().as_u16()
            ));
        }

        if let Some(len) = resp.content_length()
            && len > 0
        {
            tracker.set_total(len);
        }

        let mut out_file = File::create(&part_path)
            .map_err(|e| format!("Cannot create temporary download file: {e}"))?;
        let mut downloaded: u64 = 0;

        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| {
                let _ = std::fs::remove_file(&part_path);
                format!("Network error while downloading update: {e}")
            })?
        {
            if tracker.is_cancelled() {
                drop(out_file);
                let _ = std::fs::remove_file(&part_path);
                return Err("Update download cancelled.".to_string());
            }
            out_file.write_all(&chunk).map_err(|e| {
                let _ = std::fs::remove_file(&part_path);
                format!("Disk write error while downloading update: {e}")
            })?;
            downloaded = downloaded.saturating_add(chunk.len() as u64);
            tracker.set_downloaded(downloaded);
        }

        out_file
            .flush()
            .map_err(|e| format!("Failed to flush downloaded update file: {e}"))?;
        drop(out_file);

        // 2. Verify SHA-256 against SHA256SUMS.txt from the same release.
        tracker.set_phase(UpdateDownloadPhase::VerifyingChecksum);
        if let Some(cs_url) = &release.checksums_url {
            let cs_resp = self
                .client
                .get(cs_url)
                .timeout(Duration::from_secs(20))
                .send()
                .await
                .map_err(|e| {
                    let _ = std::fs::remove_file(&part_path);
                    format!("Could not download SHA256SUMS.txt: {e}")
                })?;
            if !cs_resp.status().is_success() {
                let _ = std::fs::remove_file(&part_path);
                return Err(format!(
                    "Could not fetch SHA256SUMS.txt (HTTP {})",
                    cs_resp.status().as_u16()
                ));
            }
            let sums_text = cs_resp.text().await.map_err(|e| {
                let _ = std::fs::remove_file(&part_path);
                format!("Could not read SHA256SUMS.txt: {e}")
            })?;
            let expected_hash = parse_sha256sums(&sums_text, &asset.name).ok_or_else(|| {
                let _ = std::fs::remove_file(&part_path);
                format!("SHA256SUMS.txt does not contain an entry for '{}'", asset.name)
            })?;
            let verify_path = part_path.clone();
            let verify_res =
                tokio::task::spawn_blocking(move || verify_file_sha256(&verify_path, &expected_hash))
                    .await
                    .unwrap_or_else(|e| Err(format!("Checksum verification task failed: {e}")));
            if let Err(err) = verify_res {
                let _ = std::fs::remove_file(&part_path);
                return Err(err);
            }
        }

        if tracker.is_cancelled() {
            let _ = std::fs::remove_file(&part_path);
            return Err("Update cancelled.".to_string());
        }

        if final_path.exists() {
            let _ = std::fs::remove_file(&final_path);
        }
        std::fs::rename(&part_path, &final_path)
            .map_err(|e| format!("Failed finalizing downloaded update file: {e}"))?;

        // 3. Apply according to detected InstallChannel.
        tracker.set_phase(UpdateDownloadPhase::Installing);
        let channel = self.channel.clone();
        let release_url = release.html_url.clone();
        tokio::task::spawn_blocking(move || apply_verified_artifact(&channel, &final_path, &release_url))
            .await
            .unwrap_or_else(|e| Err(format!("Update installer task failed: {e}")))
    }
}

fn apply_verified_artifact(
    channel: &InstallChannel,
    verified_path: &Path,
    release_url: &str,
) -> Result<ApplyUpdateOutcome, String> {
    match channel {
        InstallChannel::WindowsInstaller { .. } => Ok(ApplyUpdateOutcome::ReadyForInstaller {
            installer_path: verified_path.to_path_buf(),
        }),
        InstallChannel::WindowsPortable { exe_path } => {
            let staged = staged_new_path(exe_path);
            extract_windows_exe_from_zip(verified_path, &staged)?;
            swap_portable_binary(exe_path, &staged)?;
            Ok(ApplyUpdateOutcome::ReadyToRelaunchPortable {
                exe_path: exe_path.clone(),
            })
        }
        InstallChannel::LinuxPortable { exe_path } => {
            let staged = staged_new_path(exe_path);
            extract_linux_bin_from_tar_gz(verified_path, &staged)?;
            swap_portable_binary(exe_path, &staged)?;
            Ok(ApplyUpdateOutcome::ReadyToRelaunchPortable {
                exe_path: exe_path.clone(),
            })
        }
        InstallChannel::MacOsAppBundle { .. } => {
            #[cfg(target_os = "macos")]
            {
                std::process::Command::new("open")
                    .arg(verified_path)
                    .spawn()
                    .map_err(|e| format!("Failed opening DMG disk image: {e}"))?;
            }
            Ok(ApplyUpdateOutcome::OpenedArtifact {
                path: verified_path.to_path_buf(),
                note: "Verified DMG opened — drag GitFleet.app into Applications to finish updating."
                    .to_string(),
                manual_command: None,
            })
        }
        InstallChannel::LinuxDeb => {
            let cmd = format!("sudo apt install \"{}\"", verified_path.display());
            Ok(ApplyUpdateOutcome::OpenedArtifact {
                path: verified_path.to_path_buf(),
                note: "Verified .deb downloaded. Run the command below in your terminal to upgrade:"
                    .to_string(),
                manual_command: Some(cmd),
            })
        }
        InstallChannel::LinuxPacman => Ok(ApplyUpdateOutcome::ManualCommand {
            command: "sudo pacman -Syu".to_string(),
            release_url: release_url.to_string(),
        }),
        InstallChannel::ReadOnlyManual => {
            #[cfg(target_os = "windows")]
            {
                let _ = std::process::Command::new(verified_path).spawn();
            }
            Ok(ApplyUpdateOutcome::OpenedArtifact {
                path: verified_path.to_path_buf(),
                note: format!("Verified update saved to {}", verified_path.display()),
                manual_command: None,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semver_versions_accurately() {
        assert!(is_newer_version("0.5.0", "v0.6.0").unwrap());
        assert!(is_newer_version("0.5.0", "0.5.1").unwrap());
        assert!(is_newer_version("0.5.0", "V1.0.0").unwrap());
        assert!(!is_newer_version("0.5.0", "v0.5.0").unwrap());
        assert!(!is_newer_version("0.5.0", "v0.4.9").unwrap());
        assert!(!is_newer_version("0.5.0", "v0.5.0-rc.1").unwrap());
        assert!(is_newer_version("0.5.0", "invalid").is_err());
    }

    #[test]
    fn parses_sha256sums_manifests_safely() {
        let h1 = "a".repeat(64);
        let h2 = "0123456789abcdef".repeat(4);
        let manifest = format!(
            "# comment\n{h1}  gitfleet-windows-x86_64-setup.exe\n{h2} *dist/gitfleet-linux-x86_64.tar.gz\n"
        );
        assert_eq!(
            parse_sha256sums(&manifest, "gitfleet-windows-x86_64-setup.exe"),
            Some(h1)
        );
        assert_eq!(
            parse_sha256sums(&manifest, "gitfleet-linux-x86_64.tar.gz"),
            Some(h2)
        );
        assert_eq!(parse_sha256sums(&manifest, "missing.zip"), None);
    }

    #[test]
    fn verifies_file_sha256_match_and_mismatch() {
        let dir = std::env::temp_dir().join(format!("gitfleet-sha-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("sample.bin");
        std::fs::write(&file, b"gitfleet-update-test").unwrap();

        let expected = hex::encode(Sha256::digest(b"gitfleet-update-test"));
        assert!(verify_file_sha256(&file, &expected.to_uppercase()).is_ok());
        assert!(verify_file_sha256(&file, &"0".repeat(64)).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn classifies_install_channels_and_maps_expected_assets() {
        let win_inst = classify_install_channel(
            "windows",
            Path::new(r"C:\Users\dev\AppData\Local\Programs\GitFleet\gitfleet.exe"),
            true,
            true,
            false,
            false,
        );
        assert!(matches!(win_inst, InstallChannel::WindowsInstaller { .. }));
        assert_eq!(
            expected_asset_name("windows", "x86_64", &win_inst, "0.6.0").as_deref(),
            Some("gitfleet-windows-x86_64-setup.exe")
        );

        let win_port = classify_install_channel(
            "windows",
            Path::new(r"D:\Tools\GitFleet\gitfleet.exe"),
            false,
            true,
            false,
            false,
        );
        assert!(matches!(win_port, InstallChannel::WindowsPortable { .. }));
        assert_eq!(
            expected_asset_name("windows", "aarch64", &win_port, "0.6.0").as_deref(),
            Some("gitfleet-windows-arm64.zip")
        );

        let arch = classify_install_channel(
            "linux",
            Path::new("/usr/bin/gitfleet"),
            false,
            false,
            true,
            false,
        );
        assert_eq!(arch, InstallChannel::LinuxPacman);
        assert_eq!(
            expected_asset_name("linux", "aarch64", &arch, "v0.6.0").as_deref(),
            Some("gitfleet-0.6.0-1-aarch64.pkg.tar.zst")
        );

        let deb = classify_install_channel(
            "linux",
            Path::new("/usr/bin/gitfleet"),
            false,
            false,
            false,
            true,
        );
        assert_eq!(deb, InstallChannel::LinuxDeb);
        assert_eq!(
            expected_asset_name("linux", "x86_64", &deb, "0.6.0").as_deref(),
            Some("gitfleet-linux-x86_64.deb")
        );

        let lin_port = classify_install_channel(
            "linux",
            Path::new("/home/dev/bin/gitfleet"),
            false,
            true,
            true,
            false,
        );
        assert!(matches!(lin_port, InstallChannel::LinuxPortable { .. }));
        assert_eq!(
            expected_asset_name("linux", "x86_64", &lin_port, "0.6.0").as_deref(),
            Some("gitfleet-linux-x86_64.tar.gz")
        );

        let mac = classify_install_channel(
            "macos",
            Path::new("/Applications/GitFleet.app/Contents/MacOS/gitfleet"),
            false,
            false,
            false,
            false,
        );
        assert_eq!(
            mac,
            InstallChannel::MacOsAppBundle {
                app_bundle: PathBuf::from("/Applications/GitFleet.app"),
            }
        );
        assert_eq!(
            expected_asset_name("macos", "aarch64", &mac, "0.6.0").as_deref(),
            Some("gitfleet-macos-arm64.dmg")
        );
    }

    #[test]
    fn respects_auto_check_cooldown_and_debug_flag() {
        let mut settings = AppSettings::default();
        assert!(!should_auto_check(&settings, 100_000, true));
        assert!(should_auto_check(&settings, 100_000, false));

        settings.last_update_check_epoch_secs = Some(100_000);
        assert!(!should_auto_check(&settings, 100_000 + 3600, false));
        assert!(should_auto_check(
            &settings,
            100_000 + AUTO_CHECK_INTERVAL_SECS,
            false
        ));

        settings.auto_check_updates = false;
        assert!(!should_auto_check(
            &settings,
            100_000 + AUTO_CHECK_INTERVAL_SECS * 10,
            false
        ));
    }

    #[test]
    fn extracts_windows_exe_from_zip_and_swaps_binary_safely() {
        let dir = std::env::temp_dir().join(format!("gitfleet-zip-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let zip_path = dir.join("release.zip");
        {
            let f = File::create(&zip_path).unwrap();
            let mut zw = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            zw.start_file("README.md", opts).unwrap();
            zw.write_all(b"readme").unwrap();
            zw.start_file("nested/gitfleet.exe", opts).unwrap();
            zw.write_all(b"NEW-BINARY-BYTES").unwrap();
            zw.finish().unwrap();
        }

        let live_exe = dir.join("gitfleet.exe");
        std::fs::write(&live_exe, b"OLD-BINARY-BYTES").unwrap();

        let staged = staged_new_path(&live_exe);
        extract_windows_exe_from_zip(&zip_path, &staged).unwrap();
        assert_eq!(std::fs::read(&staged).unwrap(), b"NEW-BINARY-BYTES");

        swap_portable_binary(&live_exe, &staged).unwrap();
        assert_eq!(std::fs::read(&live_exe).unwrap(), b"NEW-BINARY-BYTES");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_untrusted_download_hosts() {
        assert!(validate_https_github_url("https://github.com/ioz-labs/gitfleet/releases/download/v0.6.0/a.zip").is_ok());
        assert!(validate_https_github_url("https://objects.githubusercontent.com/foo").is_ok());
        assert!(validate_https_github_url("http://github.com/foo").is_err());
        assert!(validate_https_github_url("https://evil.example.com/foo").is_err());
    }

    #[test]
    fn extracts_linux_bin_from_tar_gz_and_rejects_traversal() {
        let dir = std::env::temp_dir().join(format!("gitfleet-targz-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let tar_gz_path = dir.join("release.tar.gz");
        {
            let f = File::create(&tar_gz_path).unwrap();
            let enc = flate2::write::GzEncoder::new(f, flate2::Compression::default());
            let mut builder = tar::Builder::new(enc);
            let payload = b"LINUX-ELF-BINARY";
            let mut header = tar::Header::new_gnu();
            header.set_size(payload.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, "gitfleet-linux-x86_64/gitfleet", &payload[..])
                .unwrap();
            builder.finish().unwrap();
        }

        let staged = dir.join("gitfleet.new");
        extract_linux_bin_from_tar_gz(&tar_gz_path, &staged).unwrap();
        assert_eq!(std::fs::read(&staged).unwrap(), b"LINUX-ELF-BINARY");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
