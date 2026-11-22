pub mod updater;

use directories::ProjectDirs;
use gitfleet_core::{AppResult, AppSettings, CommandLogStore, EventBus, UndoStack};
use gitfleet_docs::DocsService;
use gitfleet_git::GitService;
use gitfleet_scheduler::{ScheduledFetchConfig, SchedulerService};
use gitfleet_search::SearchService;
use gitfleet_security::SecurityService;
use gitfleet_terminal::TerminalService;
use gitfleet_workspace::WorkspaceService;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use updater::UpdaterService;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedTool {
    pub name: String,
    pub command: String,
    pub path: PathBuf,
}

fn settings_file_path() -> Option<PathBuf> {
    ProjectDirs::from("org", "gitfleet", "gitfleet").map(|d| d.config_dir().join("settings.json"))
}

/// Unified application service container (replaces Electron main process + IPC bridge).
pub struct AppService {
    pub events: EventBus,
    pub settings: RwLock<AppSettings>,
    pub command_log: Arc<CommandLogStore>,
    pub undo: UndoStack,
    pub workspace: WorkspaceService,
    pub git: GitService,
    pub security: SecurityService,
    pub search: SearchService,
    pub terminal: TerminalService,
    pub docs: DocsService,
    pub scheduler: SchedulerService,
    pub updater: UpdaterService,
}

impl AppService {
    pub fn new() -> Arc<Self> {
        let events = EventBus::default();
        let command_log = Arc::new(CommandLogStore::new());
        let loaded_settings = settings_file_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|raw| serde_json::from_str::<AppSettings>(&raw).ok())
            .unwrap_or_default();

        let scheduler = SchedulerService::new(events.clone());
        scheduler.set_config(ScheduledFetchConfig {
            enabled: loaded_settings.scheduled_fetch_enabled,
            interval_secs: loaded_settings.auto_fetch_interval_secs.unwrap_or(300),
        });

        Arc::new(Self {
            workspace: WorkspaceService::new(events.clone()),
            git: GitService::new(events.clone(), command_log.clone()),
            security: SecurityService::new(),
            search: SearchService::new(events.clone()),
            terminal: TerminalService::new(events.clone()),
            docs: DocsService::new(),
            scheduler,
            updater: UpdaterService::new(),
            events,
            settings: RwLock::new(loaded_settings),
            command_log,
            undo: UndoStack::new(),
        })
    }

    pub fn settings(&self) -> AppSettings {
        self.settings.read().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn update_settings(&self, next: AppSettings) {
        self.scheduler.set_config(ScheduledFetchConfig {
            enabled: next.scheduled_fetch_enabled,
            interval_secs: next.auto_fetch_interval_secs.unwrap_or(300),
        });
        if let Some(p) = settings_file_path() {
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string_pretty(&next) {
                let _ = std::fs::write(p, json);
            }
        }
        if let Ok(mut guard) = self.settings.write() {
            *guard = next;
        }
    }

    /// Scans PATH for common external diff / merge / editor tools.
    pub fn scan_diff_tools(&self) -> Vec<DetectedTool> {
        let candidates = [
            ("VS Code", "code"),
            ("Beyond Compare", "bcomp"),
            ("WinMerge", "WinMergeU"),
            ("KDiff3", "kdiff3"),
            ("Meld", "meld"),
            ("Zed", "zed"),
        ];
        let mut found = Vec::new();
        for (name, bin) in candidates {
            if let Ok(path) = which::which(bin) {
                found.push(DetectedTool {
                    name: name.to_string(),
                    command: bin.to_string(),
                    path,
                });
            }
        }
        found
    }

    pub fn reveal_in_file_manager(&self, path: &Path) -> AppResult<()> {
        let mut target = gitfleet_core::strip_windows_verbatim_prefix(path);
        let mut is_file = target.is_file();
        if !target.exists() {
            is_file = false;
            while let Some(parent) = target.parent() {
                if parent.exists() {
                    target = parent.to_path_buf();
                    break;
                }
                target = parent.to_path_buf();
            }
        }

        #[cfg(target_os = "windows")]
        {
            let win_path = target.to_string_lossy().replace('/', "\\");
            let mut cmd = std::process::Command::new("explorer.exe");
            if is_file {
                cmd.arg("/select,").arg(&win_path);
            } else {
                cmd.arg(&win_path);
            }
            let _ = cmd.spawn()?;
        }
        #[cfg(target_os = "macos")]
        {
            let mut cmd = std::process::Command::new("open");
            if is_file {
                cmd.arg("-R").arg(&target);
            } else {
                cmd.arg(&target);
            }
            let _ = cmd.spawn()?;
        }
        #[cfg(target_os = "linux")]
        {
            let open_dir = if is_file {
                target.parent().unwrap_or(&target)
            } else {
                target.as_path()
            };
            let _ = std::process::Command::new("xdg-open")
                .arg(open_dir)
                .spawn()?;
        }
        Ok(())
    }

    pub fn build_diagnostics_json(&self, git_version: &str) -> String {
        let ws = self.workspace.current();
        let diag = serde_json::json!({
            "app": "GitFleet",
            "version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "git_version": git_version,
            "workspace_name": ws.name,
            "repo_count": ws.repos.len(),
            "group_count": ws.groups.len(),
            "settings": self.settings(),
            "detected_tools": self.scan_diff_tools(),
            "recent_command_log": self.command_log.list(),
        });
        serde_json::to_string_pretty(&diag).unwrap_or_default()
    }
}
