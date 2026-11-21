use crate::SecurityService;
use crate::models::{IgnoreLayer, IgnoreSnapshot};
use gitfleet_core::{AppError, AppResult};
use std::path::{Path, PathBuf};

impl SecurityService {
    pub fn ignore_snapshot(&self, repo_path: &Path) -> AppResult<IgnoreSnapshot> {
        let root = repo_path.join(".gitignore");
        let personal = repo_path.join(".git").join("info").join("exclude");
        let global_path = resolve_global_excludes_path();
        let global_content = global_path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default();

        Ok(IgnoreSnapshot {
            root_gitignore: std::fs::read_to_string(root).unwrap_or_default(),
            personal_exclude: std::fs::read_to_string(personal).unwrap_or_default(),
            global_exclude_path: global_path,
            global_exclude: global_content,
        })
    }

    pub fn write_ignore_layer(
        &self,
        repo_path: &Path,
        layer: IgnoreLayer,
        content: &str,
    ) -> AppResult<()> {
        let target: PathBuf = match layer {
            IgnoreLayer::RootGitignore => repo_path.join(".gitignore"),
            IgnoreLayer::PersonalExclude => repo_path.join(".git").join("info").join("exclude"),
            IgnoreLayer::GlobalExclude => resolve_global_excludes_path()
                .ok_or_else(|| AppError::Security("Global excludes path not configured".into()))?,
        };
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(target, content)?;
        Ok(())
    }

    pub fn append_gitignore(&self, repo_path: &Path, pattern: &str) -> AppResult<()> {
        let path = repo_path.join(".gitignore");
        let mut existing = std::fs::read_to_string(&path).unwrap_or_default();
        if !existing.is_empty() && !existing.ends_with('\n') {
            existing.push('\n');
        }
        existing.push_str(pattern.trim());
        existing.push('\n');
        std::fs::write(path, existing)?;
        Ok(())
    }
}

fn resolve_global_excludes_path() -> Option<PathBuf> {
    let mut cmd = std::process::Command::new("git");
    cmd.args(["config", "--global", "--get", "core.excludesfile"]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    if let Ok(out) = cmd.output()
        && out.status.success()
    {
        let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !raw.is_empty() {
            return Some(PathBuf::from(raw));
        }
    }
    directories::UserDirs::new().map(|u| u.home_dir().join(".gitignore_global"))
}
