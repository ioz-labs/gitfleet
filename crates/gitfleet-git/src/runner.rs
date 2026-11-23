use chrono::Utc;
use gitfleet_core::{AppError, AppEvent, AppResult, CommandLogEntry, CommandLogStore, EventBus};
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[cfg(target_os = "windows")]
pub(crate) const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct GitOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub(crate) fn format_command_arg(arg: &str) -> String {
    if arg.is_empty() || arg.chars().any(|c| c.is_whitespace() || c == '"') {
        format!("\"{}\"", arg.replace('"', "\\\""))
    } else {
        arg.to_string()
    }
}

pub async fn run_git(
    repo_path: &Path,
    args: &[&str],
    log_store: Option<&Arc<CommandLogStore>>,
    events: Option<&EventBus>,
    record_in_log: bool,
) -> AppResult<GitOutput> {
    run_git_with_stdin(repo_path, args, None, log_store, events, record_in_log).await
}

pub async fn run_git_with_stdin(
    repo_path: &Path,
    args: &[&str],
    stdin_bytes: Option<&[u8]>,
    log_store: Option<&Arc<CommandLogStore>>,
    events: Option<&EventBus>,
    record_in_log: bool,
) -> AppResult<GitOutput> {
    let start = Instant::now();
    let mut cmd = Command::new("git");
    cmd.current_dir(repo_path).args(args);
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.env("LC_MESSAGES", "C");
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    if stdin_bytes.is_some() {
        cmd.stdin(Stdio::piped());
    }

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::Git(format!("Failed to execute git: {e}")))?;

    if let Some(bytes) = stdin_bytes
        && let Some(mut stdin) = child.stdin.take()
    {
        stdin
            .write_all(bytes)
            .await
            .map_err(|e| AppError::Git(format!("Failed writing stdin to git: {e}")))?;
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| AppError::Git(format!("Failed waiting for git: {e}")))?;

    let duration_ms = start.elapsed().as_millis() as u64;
    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    if record_in_log {
        let entry = CommandLogEntry {
            id: format!("{}", Utc::now().timestamp_nanos_opt().unwrap_or_default()),
            timestamp: Utc::now(),
            repo_path: Some(repo_path.display().to_string()),
            command: format!(
                "git {}",
                args.iter()
                    .map(|a| format_command_arg(a))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            exit_code: Some(exit_code),
            duration_ms,
            stderr_preview: if stderr.trim().is_empty() {
                None
            } else {
                Some(stderr.trim().chars().take(180).collect())
            },
        };
        if let Some(store) = log_store {
            store.push(entry.clone());
        }
        if let Some(bus) = events {
            bus.publish(AppEvent::CommandLogAppended(entry));
        }
    }

    Ok(GitOutput {
        stdout,
        stderr,
        exit_code,
    })
}

pub async fn run_git_ok(
    repo_path: &Path,
    args: &[&str],
    log_store: Option<&Arc<CommandLogStore>>,
    events: Option<&EventBus>,
    record_in_log: bool,
) -> AppResult<String> {
    let out = run_git(repo_path, args, log_store, events, record_in_log).await?;
    if out.exit_code != 0 {
        let msg = if out.stderr.trim().is_empty() {
            out.stdout.trim().to_string()
        } else {
            out.stderr.trim().to_string()
        };
        return Err(AppError::Git(msg));
    }
    Ok(out.stdout)
}

/// Runs `git` and returns the raw unmodified `stdout` bytes (safe for binary blobs such as
/// PNG/JPEG/WebP/SVG images and exact CSV byte contents).
pub async fn run_git_bytes(repo_path: &Path, args: &[&str]) -> AppResult<Vec<u8>> {
    let mut cmd = Command::new("git");
    cmd.current_dir(repo_path).args(args);
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.env("LC_MESSAGES", "C");
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = cmd
        .output()
        .await
        .map_err(|e| AppError::Git(format!("Failed to execute git: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AppError::Git(if stderr.is_empty() {
            format!("git {} failed", args.join(" "))
        } else {
            stderr
        }));
    }

    Ok(output.stdout)
}
