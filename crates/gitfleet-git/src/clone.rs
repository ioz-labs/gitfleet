use crate::{GitService, runner};
use chrono::Utc;
use gitfleet_core::{AppError, AppEvent, AppResult, CommandLogEntry};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::sync::Notify;

const MAX_RECENT_LOG_LINES: usize = 4;

/// High-level lifecycle stage of a `git clone --progress` operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClonePhase {
    #[default]
    Connecting,
    Counting,
    Receiving,
    Resolving,
    CheckingOut,
    Submodules,
    Cancelling,
}

impl ClonePhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Connecting => "Connecting to remote…",
            Self::Counting => "Counting & compressing objects…",
            Self::Receiving => "Receiving objects…",
            Self::Resolving => "Resolving deltas…",
            Self::CheckingOut => "Checking out files…",
            Self::Submodules => "Cloning submodules…",
            Self::Cancelling => "Cancelling clone & cleaning up…",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Connecting => "Connecting",
            Self::Counting => "Counting",
            Self::Receiving => "Receiving",
            Self::Resolving => "Resolving",
            Self::CheckingOut => "Checkout",
            Self::Submodules => "Submodules",
            Self::Cancelling => "Cancelling",
        }
    }

    /// Returns step index `0..=3` for the 4-stage pipeline indicator:
    /// `0 = Remote`, `1 = Receiving`, `2 = Resolving`, `3 = Checkout`.
    pub fn step_index(self) -> usize {
        match self {
            Self::Connecting | Self::Counting => 0,
            Self::Receiving => 1,
            Self::Resolving => 2,
            Self::CheckingOut | Self::Submodules => 3,
            Self::Cancelling => 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CloneOptions {
    pub url: String,
    pub parent_dir: PathBuf,
    pub folder_name: String,
    pub branch: Option<String>,
    pub depth_one: bool,
    pub recurse_submodules: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloneOutcome {
    Completed(PathBuf),
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct CloneProgressSnapshot {
    pub phase: ClonePhase,
    /// Exact percentage (`0..=100`) reported by Git for the current active phase, if available.
    pub stage_percent: Option<u8>,
    /// Monotonic overall percentage (`0..=100`) across the clone lifecycle.
    pub overall_percent: u8,
    /// Compact object count & transfer speed summary (e.g. `"523/1245 · 15.20 MiB | 9.10 MiB/s"`).
    pub transfer_detail: String,
    /// Latest cleaned progress line emitted by `git clone --progress`.
    pub status_line: String,
    /// Last few lines of Git progress output for live console display.
    pub recent_lines: Vec<String>,
    pub elapsed_secs: u64,
    pub is_cancelled: bool,
}

impl Default for CloneProgressSnapshot {
    fn default() -> Self {
        Self {
            phase: ClonePhase::Connecting,
            stage_percent: None,
            overall_percent: 2,
            transfer_detail: String::new(),
            status_line: "Starting git clone…".to_string(),
            recent_lines: Vec::new(),
            elapsed_secs: 0,
            is_cancelled: false,
        }
    }
}

#[derive(Debug)]
struct CloneProgressInner {
    phase: ClonePhase,
    stage_percent: Option<u8>,
    overall_percent: u8,
    in_submodule: bool,
    transfer_detail: String,
    status_line: String,
    recent_lines: VecDeque<String>,
    last_ephemeral: bool,
}

impl Default for CloneProgressInner {
    fn default() -> Self {
        Self {
            phase: ClonePhase::Connecting,
            stage_percent: None,
            overall_percent: 2,
            in_submodule: false,
            transfer_detail: String::new(),
            status_line: "Starting git clone…".to_string(),
            recent_lines: VecDeque::with_capacity(MAX_RECENT_LOG_LINES),
            last_ephemeral: false,
        }
    }
}

/// Thread-safe progress & instant cancellation handle shared between `GitService::clone_repo_with_progress`
/// and the Iced UI layer.
pub struct CloneProgressTracker {
    started_at: Instant,
    cancelled: AtomicBool,
    cancel_notify: Notify,
    inner: Mutex<CloneProgressInner>,
}

impl Default for CloneProgressTracker {
    fn default() -> Self {
        Self {
            started_at: Instant::now(),
            cancelled: AtomicBool::new(false),
            cancel_notify: Notify::new(),
            inner: Mutex::new(CloneProgressInner::default()),
        }
    }
}

impl std::fmt::Debug for CloneProgressTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloneProgressTracker")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

impl CloneProgressTracker {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn cancel(&self) {
        if !self.cancelled.swap(true, Ordering::SeqCst) {
            if let Ok(mut guard) = self.inner.lock() {
                guard.phase = ClonePhase::Cancelling;
                guard.status_line = "Cancelling git clone and cleaning up partial files…".to_string();
            }
            self.cancel_notify.notify_waiters();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub async fn cancelled_notified(&self) {
        let notified = self.cancel_notify.notified();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }

    pub fn snapshot(&self) -> CloneProgressSnapshot {
        let is_cancelled = self.is_cancelled();
        let elapsed_secs = self.started_at.elapsed().as_secs();
        let Ok(guard) = self.inner.lock() else {
            return CloneProgressSnapshot {
                phase: if is_cancelled {
                    ClonePhase::Cancelling
                } else {
                    ClonePhase::Connecting
                },
                is_cancelled,
                elapsed_secs,
                ..Default::default()
            };
        };
        CloneProgressSnapshot {
            phase: if is_cancelled {
                ClonePhase::Cancelling
            } else {
                guard.phase
            },
            stage_percent: guard.stage_percent,
            overall_percent: guard.overall_percent,
            transfer_detail: guard.transfer_detail.clone(),
            status_line: guard.status_line.clone(),
            recent_lines: guard.recent_lines.iter().cloned().collect(),
            elapsed_secs,
            is_cancelled,
        }
    }

    fn apply_stderr_segment(&self, raw_segment: &str, was_carriage_return: bool) {
        let clean = strip_ansi_and_trim(raw_segment);
        if clean.is_empty() || self.is_cancelled() {
            return;
        }
        let parsed = parse_git_progress_line(&clean);
        let Ok(mut guard) = self.inner.lock() else {
            return;
        };

        if clean.starts_with("Submodule ") || (guard.overall_percent >= 85 && clean.starts_with("Cloning into ")) {
            guard.in_submodule = true;
        }

        if let Some(mut next_phase) = parsed.phase {
            if guard.in_submodule && next_phase == ClonePhase::Connecting {
                next_phase = ClonePhase::Submodules;
            }
            guard.phase = next_phase;
        } else if guard.in_submodule && guard.phase != ClonePhase::Cancelling {
            guard.phase = ClonePhase::Submodules;
        }

        if parsed.phase.is_some() {
            guard.stage_percent = parsed.percent;
            if !parsed.detail.is_empty() {
                guard.transfer_detail = parsed.detail;
            } else if parsed.percent.is_none() {
                guard.transfer_detail.clear();
            }
            let mapped = compute_overall_percent(guard.phase, guard.stage_percent, guard.in_submodule);
            if mapped > guard.overall_percent {
                guard.overall_percent = mapped;
            }
        }

        guard.status_line = clean.clone();

        // Replace the trailing ephemeral progress line in `recent_lines` when Git overwrites with `\r`
        let is_ephemeral = was_carriage_return || parsed.is_progress_counter;
        if guard.last_ephemeral && is_ephemeral && !guard.recent_lines.is_empty() {
            if let Some(back) = guard.recent_lines.back_mut() {
                *back = clean;
            }
        } else {
            if guard.recent_lines.len() >= MAX_RECENT_LOG_LINES {
                guard.recent_lines.pop_front();
            }
            guard.recent_lines.push_back(clean);
        }
        guard.last_ephemeral = is_ephemeral && !raw_segment.trim_end().ends_with(", done.");
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ParsedProgress {
    phase: Option<ClonePhase>,
    percent: Option<u8>,
    detail: String,
    is_progress_counter: bool,
}

fn compute_overall_percent(phase: ClonePhase, stage_pct: Option<u8>, in_submodule: bool) -> u8 {
    if in_submodule {
        let sub_frac = stage_pct.unwrap_or(50) as f32 / 100.0;
        return (92.0 + 7.0 * sub_frac).round().clamp(92.0, 99.0) as u8;
    }
    let f = stage_pct.unwrap_or(0) as f32 / 100.0;
    let raw: f32 = match phase {
        ClonePhase::Connecting => 4.0,
        ClonePhase::Counting => {
            if stage_pct.is_some() {
                6.0 + 10.0 * f
            } else {
                8.0
            }
        }
        ClonePhase::Receiving => 16.0 + 62.0 * f,
        ClonePhase::Resolving => 78.0 + 14.0 * f,
        ClonePhase::CheckingOut => 92.0 + 7.0 * f,
        ClonePhase::Submodules => 95.0,
        ClonePhase::Cancelling => 0.0,
    };
    raw.round().clamp(2.0, 99.0) as u8
}

fn strip_ansi_and_trim(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if chars.peek() == Some(&'[') {
                let _ = chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else if !ch.is_control() {
            out.push(ch);
        }
    }
    out.trim().to_string()
}

fn parse_git_progress_line(line: &str) -> ParsedProgress {
    let body = line.strip_prefix("remote:").map(str::trim).unwrap_or(line);

    let phase = if body.starts_with("Cloning into ") {
        Some(ClonePhase::Connecting)
    } else if body.starts_with("Submodule ") {
        Some(ClonePhase::Submodules)
    } else if body.starts_with("Enumerating objects:")
        || body.starts_with("Counting objects:")
        || body.starts_with("Compressing objects:")
    {
        Some(ClonePhase::Counting)
    } else if body.starts_with("Receiving objects:") {
        Some(ClonePhase::Receiving)
    } else if body.starts_with("Resolving deltas:") {
        Some(ClonePhase::Resolving)
    } else if body.starts_with("Updating files:") || body.starts_with("Checking out files:") {
        Some(ClonePhase::CheckingOut)
    } else {
        None
    };

    let Some(colon_idx) = body.find(':') else {
        return ParsedProgress {
            phase,
            percent: None,
            detail: String::new(),
            is_progress_counter: false,
        };
    };

    if phase.is_none() || matches!(phase, Some(ClonePhase::Connecting | ClonePhase::Submodules)) {
        return ParsedProgress {
            phase,
            percent: None,
            detail: String::new(),
            is_progress_counter: false,
        };
    }

    let after_colon = body[colon_idx + 1..].trim().trim_end_matches(", done.").trim();

    // Extract `<N>%` if present
    let mut percent: Option<u8> = None;
    if let Some(pct_pos) = after_colon.find('%') {
        let digits: String = after_colon[..pct_pos]
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        if let Ok(val) = digits.parse::<u16>() {
            percent = Some(val.min(100) as u8);
        }
    }

    // Extract `(cur/total)` and optional `, 12.34 MiB | 5.67 MiB/s`
    let mut parts: Vec<String> = Vec::new();
    if let (Some(lp), Some(rp)) = (after_colon.find('('), after_colon.find(')'))
        && lp + 1 < rp
    {
        let counts = after_colon[lp + 1..rp].trim();
        if !counts.is_empty() {
            parts.push(counts.to_string());
        }
        let tail = after_colon[rp + 1..]
            .trim()
            .trim_start_matches(',')
            .trim_end_matches(", done.")
            .trim_end_matches("done.")
            .trim_end_matches(',')
            .trim();
        if !tail.is_empty() {
            parts.push(tail.to_string());
        }
    } else if percent.is_none() && !after_colon.is_empty() {
        parts.push(after_colon.to_string());
    }

    ParsedProgress {
        phase,
        percent,
        detail: parts.join(" · "),
        is_progress_counter: true,
    }
}

fn is_noise_progress_for_error(line: &str) -> bool {
    let parsed = parse_git_progress_line(line);
    parsed.is_progress_counter
        || line.starts_with("Cloning into ")
        || line.starts_with("remote: Total ")
}

/// Infers a clean target repository folder name from an HTTPS, SSH (`ssh://` or `git@host:org/repo.git`),
/// `file://`, or local filesystem Git URL.
pub fn infer_repo_folder_from_url(raw_url: &str) -> Option<String> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Strip query string / URL fragment if copied from a browser bar
    let without_query = trimmed.split(['?', '#']).next().unwrap_or(trimmed);
    let cleaned = without_query.trim_end_matches(['/', '\\']).trim();
    if cleaned.is_empty() {
        return None;
    }

    // Guard against bare scheme+host URLs with no repository path (e.g. `https://github.com`)
    if let Some(after_scheme) = cleaned.split_once("://").map(|(_, rest)| rest) {
        let after_host = after_scheme.split_once('/').map(|(_, p)| p).unwrap_or("");
        if after_host.trim_matches('/').is_empty() {
            return None;
        }
    } else if let Some((left, right)) = cleaned.split_once(':') {
        // SCP-style `git@github.com:org/repo.git` vs Windows drive `D:\repos\my-repo`
        let is_windows_drive = left.len() == 1 && left.chars().all(|c| c.is_ascii_alphabetic());
        if !is_windows_drive && right.trim_matches(['/', '\\']).is_empty() {
            return None;
        }
    }

    let tail = cleaned.rsplit(['/', '\\', ':']).next()?.trim();
    let base = tail
        .strip_suffix(".git")
        .or_else(|| tail.strip_suffix(".GIT"))
        .unwrap_or(tail)
        .trim_matches(['.', ' ']);

    if base.is_empty() {
        None
    } else {
        Some(base.to_string())
    }
}

async fn kill_process_tree(child: &mut Child) {
    if let Some(pid) = child.id() {
        #[cfg(target_os = "windows")]
        {
            let mut killer = Command::new("taskkill");
            killer.args(["/F", "/T", "/PID", &pid.to_string()]);
            killer.stdout(Stdio::null()).stderr(Stdio::null());
            killer.creation_flags(runner::CREATE_NO_WINDOW);
            let _ = killer.status().await;
        }
        #[cfg(unix)]
        {
            let neg_pgid = format!("-{pid}");
            let _ = Command::new("kill")
                .args(["-TERM", &neg_pgid])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
            tokio::time::sleep(Duration::from_millis(40)).await;
            let _ = Command::new("kill")
                .args(["-KILL", &neg_pgid])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
}

async fn cleanup_cancelled_clone_dir(dest: &Path, dest_existed_before: bool) {
    if dest_existed_before {
        let dot_git = dest.join(".git");
        for _ in 0..4 {
            if !dot_git.exists() || tokio::fs::remove_dir_all(&dot_git).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(60)).await;
        }
    } else {
        for _ in 0..4 {
            if !dest.exists() || tokio::fs::remove_dir_all(dest).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(60)).await;
        }
    }
}

impl GitService {
    /// Clones a repository with real-time `--progress` stderr streaming and instant process-tree
    /// cancellation + partial directory cleanup.
    pub async fn clone_repo_with_progress(
        &self,
        opts: CloneOptions,
        tracker: Arc<CloneProgressTracker>,
    ) -> AppResult<CloneOutcome> {
        let url = opts.url.trim().to_string();
        let folder_name = opts.folder_name.trim().to_string();
        if url.is_empty() {
            return Err(AppError::Git("Repository URL cannot be empty.".into()));
        }
        if folder_name.is_empty() {
            return Err(AppError::Git("Target folder name cannot be empty.".into()));
        }

        if tracker.is_cancelled() {
            return Ok(CloneOutcome::Cancelled);
        }

        std::fs::create_dir_all(&opts.parent_dir)?;
        let dest = opts.parent_dir.join(&folder_name);
        if dest.is_file() {
            return Err(AppError::Git(format!(
                "Destination '{}' already exists as a file.",
                dest.display()
            )));
        }
        if dest.is_dir() && std::fs::read_dir(&dest)?.next().is_some() {
            return Err(AppError::Git(format!(
                "Destination folder '{}' already exists and is not empty.",
                dest.display()
            )));
        }
        let dest_existed_before = dest.exists();
        let dest_str = dest.to_string_lossy().to_string();

        let mut args: Vec<String> = vec!["clone".into(), "--progress".into()];
        if opts.recurse_submodules {
            args.push("--recurse-submodules".into());
        }
        if opts.depth_one {
            args.push("--depth".into());
            args.push("1".into());
        }
        if let Some(branch) = opts
            .branch
            .as_deref()
            .map(str::trim)
            .filter(|b| !b.is_empty())
        {
            args.push("--branch".into());
            args.push(branch.to_string());
        }
        args.push("--".into());
        args.push(url);
        args.push(dest_str);

        let start = Instant::now();
        let mut cmd = Command::new("git");
        cmd.current_dir(&opts.parent_dir).args(&args);
        cmd.env("GIT_TERMINAL_PROMPT", "0");
        cmd.env("LC_MESSAGES", "C");
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        #[cfg(target_os = "windows")]
        {
            cmd.creation_flags(runner::CREATE_NO_WINDOW);
        }
        #[cfg(unix)]
        {
            cmd.process_group(0);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| AppError::Git(format!("Failed to execute git clone: {e}")))?;

        let stdout_task = child.stdout.take().map(|mut stdout| {
            tokio::spawn(async move {
                let mut out = Vec::new();
                let _ = stdout.read_to_end(&mut out).await;
                String::from_utf8_lossy(&out).into_owned()
            })
        });

        let mut stderr_pipe = child.stderr.take();
        let mut pending_bytes: Vec<u8> = Vec::with_capacity(256);
        let mut error_lines: Vec<String> = Vec::new();
        let mut read_buf = [0u8; 512];
        let mut cancelled_during_stream = false;

        if let Some(stderr) = &mut stderr_pipe {
            loop {
                tokio::select! {
                    biased;
                    _ = tracker.cancelled_notified() => {
                        cancelled_during_stream = true;
                        break;
                    }
                    read_res = stderr.read(&mut read_buf) => {
                        match read_res {
                            Ok(0) => break,
                            Ok(n) => {
                                for &b in &read_buf[..n] {
                                    if b == b'\r' || b == b'\n' {
                                        if !pending_bytes.is_empty() {
                                            let seg = String::from_utf8_lossy(&pending_bytes).into_owned();
                                            pending_bytes.clear();
                                            let clean = strip_ansi_and_trim(&seg);
                                            if !clean.is_empty() && !is_noise_progress_for_error(&clean) {
                                                error_lines.push(clean);
                                            }
                                            tracker.apply_stderr_segment(&seg, b == b'\r');
                                        }
                                    } else {
                                        pending_bytes.push(b);
                                    }
                                }
                            }
                            Err(_) => break,
                        }
                    }
                }
            }
        }

        if cancelled_during_stream || tracker.is_cancelled() {
            kill_process_tree(&mut child).await;
            if let Some(h) = stdout_task {
                h.abort();
            }
            cleanup_cancelled_clone_dir(&dest, dest_existed_before).await;
            self.record_clone_command_log(
                &opts.parent_dir,
                &args,
                None,
                start.elapsed().as_millis() as u64,
                Some("[cancelled by user]".to_string()),
            );
            return Ok(CloneOutcome::Cancelled);
        }

        if !pending_bytes.is_empty() {
            let seg = String::from_utf8_lossy(&pending_bytes).into_owned();
            let clean = strip_ansi_and_trim(&seg);
            if !clean.is_empty() && !is_noise_progress_for_error(&clean) {
                error_lines.push(clean);
            }
            tracker.apply_stderr_segment(&seg, false);
        }

        let status = tokio::select! {
            biased;
            _ = tracker.cancelled_notified() => {
                kill_process_tree(&mut child).await;
                if let Some(h) = stdout_task {
                    h.abort();
                }
                cleanup_cancelled_clone_dir(&dest, dest_existed_before).await;
                self.record_clone_command_log(
                    &opts.parent_dir,
                    &args,
                    None,
                    start.elapsed().as_millis() as u64,
                    Some("[cancelled by user]".to_string()),
                );
                return Ok(CloneOutcome::Cancelled);
            }
            res = child.wait() => {
                res.map_err(|e| AppError::Git(format!("Failed waiting for git clone: {e}")))?
            }
        };

        let stdout_str = match stdout_task {
            Some(h) => h.await.unwrap_or_default(),
            None => String::new(),
        };

        if tracker.is_cancelled() {
            cleanup_cancelled_clone_dir(&dest, dest_existed_before).await;
            self.record_clone_command_log(
                &opts.parent_dir,
                &args,
                None,
                start.elapsed().as_millis() as u64,
                Some("[cancelled by user]".to_string()),
            );
            return Ok(CloneOutcome::Cancelled);
        }

        let duration_ms = start.elapsed().as_millis() as u64;
        let exit_code = status.code().unwrap_or(-1);
        let err_joined = error_lines.join("\n");

        self.record_clone_command_log(
            &opts.parent_dir,
            &args,
            Some(exit_code),
            duration_ms,
            if exit_code != 0 && !err_joined.trim().is_empty() {
                Some(err_joined.trim().chars().take(180).collect())
            } else {
                None
            },
        );

        if exit_code != 0 {
            let msg = if !err_joined.trim().is_empty() {
                err_joined.trim().to_string()
            } else if !stdout_str.trim().is_empty() {
                stdout_str.trim().to_string()
            } else {
                format!("git clone exited with code {exit_code}")
            };
            return Err(AppError::Git(msg));
        }

        Ok(CloneOutcome::Completed(dest))
    }

    fn record_clone_command_log(
        &self,
        cwd: &Path,
        args: &[String],
        exit_code: Option<i32>,
        duration_ms: u64,
        stderr_preview: Option<String>,
    ) {
        let entry = CommandLogEntry {
            id: format!("{}", Utc::now().timestamp_nanos_opt().unwrap_or_default()),
            timestamp: Utc::now(),
            repo_path: Some(cwd.display().to_string()),
            command: format!(
                "git {}",
                args.iter()
                    .map(|a| runner::format_command_arg(a))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            exit_code,
            duration_ms,
            stderr_preview,
        };
        self.command_log.push(entry.clone());
        self.events.publish(AppEvent::CommandLogAppended(entry));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitfleet_core::{CommandLogStore, EventBus};

    #[test]
    fn infers_repo_folder_from_https_ssh_and_local_urls() {
        assert_eq!(
            infer_repo_folder_from_url("https://github.com/ioz-labs/gitfleet.git").as_deref(),
            Some("gitfleet")
        );
        assert_eq!(
            infer_repo_folder_from_url("https://github.com/ioz-labs/gitfleet/").as_deref(),
            Some("gitfleet")
        );
        assert_eq!(
            infer_repo_folder_from_url("https://github.com/org/my-service.git?tab=readme")
                .as_deref(),
            Some("my-service")
        );
        assert_eq!(
            infer_repo_folder_from_url("git@github.com:ioz-labs/gitfleet.git").as_deref(),
            Some("gitfleet")
        );
        assert_eq!(
            infer_repo_folder_from_url("git@gitlab.internal:platform-core.git").as_deref(),
            Some("platform-core")
        );
        assert_eq!(
            infer_repo_folder_from_url("ssh://git@github.com:22/org/engine.git").as_deref(),
            Some("engine")
        );
        assert_eq!(
            infer_repo_folder_from_url(r"D:\repos\sample-app.git").as_deref(),
            Some("sample-app")
        );

        // Incomplete host-only strings must return None
        assert_eq!(infer_repo_folder_from_url(""), None);
        assert_eq!(infer_repo_folder_from_url("https://github.com"), None);
        assert_eq!(infer_repo_folder_from_url("https://github.com/"), None);
        assert_eq!(infer_repo_folder_from_url("git@github.com:"), None);
    }

    #[test]
    fn parses_git_clone_progress_phases_percentages_and_speeds() {
        let p_recv = parse_git_progress_line(
            "Receiving objects:  42% (596/1420), 18.45 MiB | 11.20 MiB/s",
        );
        assert_eq!(p_recv.phase, Some(ClonePhase::Receiving));
        assert_eq!(p_recv.percent, Some(42));
        assert_eq!(p_recv.detail, "596/1420 · 18.45 MiB | 11.20 MiB/s");

        let p_done = parse_git_progress_line(
            "Receiving objects: 100% (1420/1420), 42.10 MiB | 12.05 MiB/s, done.",
        );
        assert_eq!(p_done.phase, Some(ClonePhase::Receiving));
        assert_eq!(p_done.percent, Some(100));
        assert_eq!(p_done.detail, "1420/1420 · 42.10 MiB | 12.05 MiB/s");

        let p_comp = parse_git_progress_line("remote: Compressing objects:  75% (300/400)");
        assert_eq!(p_comp.phase, Some(ClonePhase::Counting));
        assert_eq!(p_comp.percent, Some(75));
        assert_eq!(p_comp.detail, "300/400");

        let p_delta = parse_git_progress_line("Resolving deltas:  68% (612/900)");
        assert_eq!(p_delta.phase, Some(ClonePhase::Resolving));
        assert_eq!(p_delta.percent, Some(68));
        assert_eq!(p_delta.detail, "612/900");

        let p_checkout = parse_git_progress_line("Updating files:  90% (450/500)");
        assert_eq!(p_checkout.phase, Some(ClonePhase::CheckingOut));
        assert_eq!(p_checkout.percent, Some(90));
        assert_eq!(p_checkout.detail, "450/500");

        // Error lines must NOT be discarded as progress noise
        assert!(!is_noise_progress_for_error(
            "fatal: repository 'https://example.invalid/nope.git' not found"
        ));
        assert!(is_noise_progress_for_error(
            "Receiving objects:  42% (596/1420), 18.45 MiB | 11.20 MiB/s"
        ));
    }

    #[tokio::test]
    async fn clones_local_repo_and_respects_cancellation() {
        let tmp = std::env::temp_dir().join(format!(
            "gitfleet-clone-test-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let src = tmp.join("source-repo");
        let parent = tmp.join("clones");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&parent).unwrap();

        let svc = GitService::new(EventBus::default(), Arc::new(CommandLogStore::new()));
        svc.init_repo(&src).await.unwrap();
        std::fs::write(src.join("README.md"), "# Test\n").unwrap();
        runner::run_git_ok(&src, &["config", "user.email", "test@example.com"], None, None, false)
            .await
            .unwrap();
        runner::run_git_ok(&src, &["config", "user.name", "Test"], None, None, false)
            .await
            .unwrap();
        runner::run_git_ok(&src, &["add", "."], None, None, false)
            .await
            .unwrap();
        runner::run_git_ok(&src, &["commit", "-m", "init"], None, None, false)
            .await
            .unwrap();

        // 1. Successful clone
        let tracker = CloneProgressTracker::new();
        let outcome = svc
            .clone_repo_with_progress(
                CloneOptions {
                    url: src.to_string_lossy().to_string(),
                    parent_dir: parent.clone(),
                    folder_name: "cloned-ok".into(),
                    branch: None,
                    depth_one: false,
                    recurse_submodules: false,
                },
                tracker,
            )
            .await
            .unwrap();
        assert_eq!(outcome, CloneOutcome::Completed(parent.join("cloned-ok")));
        assert!(parent.join("cloned-ok").join(".git").exists());

        // 2. Cancelled clone leaves no destination directory
        let cancel_tracker = CloneProgressTracker::new();
        cancel_tracker.cancel();
        let cancelled = svc
            .clone_repo_with_progress(
                CloneOptions {
                    url: src.to_string_lossy().to_string(),
                    parent_dir: parent.clone(),
                    folder_name: "cloned-cancelled".into(),
                    branch: None,
                    depth_one: false,
                    recurse_submodules: false,
                },
                cancel_tracker,
            )
            .await
            .unwrap();
        assert_eq!(cancelled, CloneOutcome::Cancelled);
        assert!(!parent.join("cloned-cancelled").exists());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
