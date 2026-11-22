use gitfleet_core::{AppEvent, EventBus};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledFetchConfig {
    pub enabled: bool,
    pub interval_secs: u64,
}

impl Default for ScheduledFetchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secs: 300,
        }
    }
}

/// Manages periodic remote fetch and live repository filesystem watching (`notify`).
pub struct SchedulerService {
    config: RwLock<ScheduledFetchConfig>,
    last_fetch: Mutex<Instant>,
    watcher: Mutex<Option<RecommendedWatcher>>,
    watched_path: Mutex<Option<PathBuf>>,
    fs_dirty: Arc<AtomicBool>,
    git_refs_dirty: Arc<AtomicBool>,
    events: EventBus,
}

impl SchedulerService {
    pub fn new(events: EventBus) -> Self {
        Self {
            config: RwLock::new(ScheduledFetchConfig::default()),
            last_fetch: Mutex::new(Instant::now()),
            watcher: Mutex::new(None),
            watched_path: Mutex::new(None),
            fs_dirty: Arc::new(AtomicBool::new(false)),
            git_refs_dirty: Arc::new(AtomicBool::new(false)),
            events,
        }
    }

    pub fn config(&self) -> ScheduledFetchConfig {
        self.config.read().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn set_config(&self, cfg: ScheduledFetchConfig) {
        if let Ok(mut guard) = self.config.write() {
            *guard = cfg;
        }
        if let Ok(mut last) = self.last_fetch.lock() {
            *last = Instant::now();
        }
    }

    /// Starts watching `repo_path` for external file changes using OS-native `notify`.
    pub fn watch_repo(&self, repo_path: &Path) {
        if let Ok(mut wp) = self.watched_path.lock() {
            if wp.as_deref() == Some(repo_path) {
                return;
            }
            *wp = Some(repo_path.to_path_buf());
        }

        let dirty = self.fs_dirty.clone();
        let refs_dirty = self.git_refs_dirty.clone();
        let watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(ev) = res {
                let is_lock_noise = ev.paths.iter().all(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|s| s.ends_with(".lock"))
                });
                if !is_lock_noise && !ev.kind.is_access() {
                    if ev.paths.iter().any(|p| is_git_ref_or_meta_path(p)) {
                        refs_dirty.store(true, Ordering::Relaxed);
                    }
                    dirty.store(true, Ordering::Relaxed);
                }
            }
        });

        if let Ok(mut w) = watcher
            && w.watch(repo_path, RecursiveMode::Recursive).is_ok()
            && let Ok(mut guard) = self.watcher.lock()
        {
            *guard = Some(w);
        }
    }

    pub fn stop_watching(&self) {
        if let Ok(mut guard) = self.watcher.lock() {
            *guard = None;
        }
        if let Ok(mut wp) = self.watched_path.lock() {
            *wp = None;
        }
    }

    /// Returns true once if any external file modification occurred since the last check.
    pub fn take_fs_dirty(&self) -> bool {
        self.fs_dirty.swap(false, Ordering::Relaxed)
    }

    /// Returns true once if any `.git` ref/HEAD/log/stash metadata changed since the last check.
    pub fn take_git_refs_dirty(&self) -> bool {
        self.git_refs_dirty.swap(false, Ordering::Relaxed)
    }

    /// Returns true when scheduled background fetch is enabled and interval has elapsed.
    pub fn take_scheduled_fetch_due(&self) -> bool {
        let cfg = self.config();
        if !cfg.enabled || cfg.interval_secs < 15 {
            return false;
        }
        if let Ok(mut last) = self.last_fetch.lock()
            && last.elapsed() >= Duration::from_secs(cfg.interval_secs)
        {
            *last = Instant::now();
            return true;
        }
        false
    }

    pub fn trigger_remote_refresh_now(&self) {
        if let Ok(mut last) = self.last_fetch.lock() {
            *last = Instant::now();
        }
        self.events.publish(AppEvent::RemoteRefresh {
            running: true,
            updated_repos: 0,
        });
    }
}

/// Returns `true` when `path` touches Git ref/history metadata inside `.git/` (such as `HEAD`,
/// `refs/*`, `packed-refs`, `logs/*`, `FETCH_HEAD`, `ORIG_HEAD`, `MERGE_HEAD`, `REBASE_HEAD`,
/// `CHERRY_PICK_HEAD`, `REVERT_HEAD`, `COMMIT_EDITMSG`, or `worktrees/*`).
pub fn is_git_ref_or_meta_path(path: &Path) -> bool {
    let mut after_git = false;
    for comp in path.components() {
        let Some(name) = comp.as_os_str().to_str() else {
            continue;
        };
        if after_git {
            return matches!(
                name,
                "HEAD"
                    | "refs"
                    | "packed-refs"
                    | "logs"
                    | "FETCH_HEAD"
                    | "ORIG_HEAD"
                    | "MERGE_HEAD"
                    | "REBASE_HEAD"
                    | "CHERRY_PICK_HEAD"
                    | "REVERT_HEAD"
                    | "COMMIT_EDITMSG"
                    | "worktrees"
            );
        }
        if name == ".git" {
            after_git = true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_git_ref_and_meta_paths_cleanly() {
        assert!(is_git_ref_or_meta_path(Path::new("repo/.git/HEAD")));
        assert!(is_git_ref_or_meta_path(Path::new("repo/.git/refs/heads/main")));
        assert!(is_git_ref_or_meta_path(Path::new("repo/.git/refs/tags/v1.0.0")));
        assert!(is_git_ref_or_meta_path(Path::new("repo/.git/packed-refs")));
        assert!(is_git_ref_or_meta_path(Path::new("repo/.git/logs/HEAD")));
        assert!(is_git_ref_or_meta_path(Path::new("repo/.git/FETCH_HEAD")));

        // Index edits & regular working-tree files should NOT trigger full ref/graph reloads on every keystroke
        assert!(!is_git_ref_or_meta_path(Path::new("repo/.git/index")));
        assert!(!is_git_ref_or_meta_path(Path::new("repo/src/main.rs")));
        assert!(!is_git_ref_or_meta_path(Path::new("repo/refs/heads/main")));
    }
}
