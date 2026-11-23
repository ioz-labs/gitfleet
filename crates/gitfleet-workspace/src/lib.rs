pub mod blueprint;

pub use blueprint::{
    BLUEPRINT_SCHEMA_VERSION, BlueprintRemote, BlueprintRepo, GroupBlueprint, UrlProtocolMode,
    compute_export_group_path, resolve_materialized_repo_dest, rewrite_git_url_protocol,
    validate_group_hierarchy, validate_safe_rel_segment,
};

use directories::ProjectDirs;
use gitfleet_core::{
    AppError, AppEvent, AppResult, EventBus, RepoId, canonicalize_clean,
    strip_windows_verbatim_prefix,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoEntry {
    pub id: RepoId,
    pub name: String,
    pub path: PathBuf,
    pub group: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    pub available: bool,
    /// Optional explicit Age key ID override for this repository's Secrets Vault.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_key_id: Option<String>,
    /// Optional active Secrets Vault profile on this machine (e.g. `"laptop"`, `"staging"`).
    /// Stored locally per workspace (never committed to Git); defaults to `"default"` when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceUiState {
    pub selected_repo_id: Option<RepoId>,
    pub collapsed_groups: Vec<String>,
    pub active_tab: Option<String>,
    #[serde(default)]
    pub compact_density: bool,
    #[serde(default)]
    pub light_theme: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme_preset: Option<String>,
    #[serde(default = "default_show_ignored_files")]
    pub show_ignored_files: bool,
}

fn default_show_ignored_files() -> bool {
    true
}

impl Default for WorkspaceUiState {
    fn default() -> Self {
        Self {
            selected_repo_id: None,
            collapsed_groups: Vec::new(),
            active_tab: None,
            compact_density: false,
            light_theme: false,
            theme_preset: None,
            show_ignored_files: default_show_ignored_files(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub name: String,
    #[serde(default)]
    pub file_path: Option<PathBuf>,
    #[serde(default)]
    pub groups: Vec<String>,
    /// Per-group Age key assignments (`normalized group path` -> `key_id`).
    /// Subgroups and repositories automatically inherit from the nearest assigned ancestor group.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub group_secret_keys: HashMap<String, String>,
    #[serde(default)]
    pub repos: Vec<RepoEntry>,
    #[serde(default)]
    pub ui: WorkspaceUiState,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            name: "Home Workspace".to_string(),
            file_path: None,
            groups: Vec::new(),
            group_secret_keys: HashMap::new(),
            repos: Vec::new(),
            ui: WorkspaceUiState::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretKeyBindingSource {
    Repo,
    Group(String),
}

/// Walks up a normalized group path (e.g. `"CompanyA/Backend/Services"` → `"CompanyA/Backend"` → `"CompanyA"`)
/// and returns the nearest assigned `(key_id, matched_group_path)`.
pub fn resolve_group_secret_key(
    group_secret_keys: &HashMap<String, String>,
    group: Option<&str>,
) -> Option<(String, String)> {
    let mut cur = group.map(normalize_group_path).filter(|g| !g.is_empty())?;
    loop {
        if let Some(key_id) = group_secret_keys.get(&cur)
            && !key_id.trim().is_empty()
        {
            return Some((key_id.clone(), cur));
        }
        cur = parent_group_path(&cur)?;
    }
}

/// Resolves the workspace-configured Age key binding for `repo` (`Repo` override takes priority over
/// the nearest assigned ancestor `Group`).
pub fn resolve_repo_secret_key_in(
    cfg: &WorkspaceConfig,
    repo: &RepoEntry,
) -> Option<(String, SecretKeyBindingSource)> {
    if let Some(key_id) = repo
        .secret_key_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some((key_id.to_string(), SecretKeyBindingSource::Repo));
    }
    resolve_group_secret_key(&cfg.group_secret_keys, repo.group.as_deref())
        .map(|(id, grp)| (id, SecretKeyBindingSource::Group(grp)))
}

/// Normalizes a hierarchical group path (e.g. `" work / backend/api "` -> `"work/backend/api"`).
pub fn normalize_group_path(raw: &str) -> String {
    raw.replace('\\', "/")
        .split('/')
        .map(str::trim)
        .filter(|seg| !seg.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// Returns the parent group path for a nested group (e.g. `"a/b/c"` -> `Some("a/b")`).
pub fn parent_group_path(group: &str) -> Option<String> {
    group
        .rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .filter(|p| !p.is_empty())
}

/// Returns `true` if `repo_group` is inside `group` (exact match or nested subgroup).
pub fn group_contains_repo(group: &str, repo_group: Option<&str>) -> bool {
    let norm_group = normalize_group_path(group);
    if norm_group.is_empty() {
        return false;
    }
    let Some(raw_repo_group) = repo_group else {
        return false;
    };
    let norm_repo = normalize_group_path(raw_repo_group);
    if norm_repo.is_empty() {
        return false;
    }
    norm_repo == norm_group
        || norm_repo
            .strip_prefix(&norm_group)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Counts all repositories in `group` and all of its nested subgroups.
pub fn count_repos_in_group(cfg: &WorkspaceConfig, group: &str) -> usize {
    cfg.repos
        .iter()
        .filter(|r| group_contains_repo(group, r.group.as_deref()))
        .count()
}

fn delete_group_in(cfg: &mut WorkspaceConfig, name: &str) -> Vec<RepoId> {
    let norm = normalize_group_path(name);
    if norm.is_empty() {
        return Vec::new();
    }
    let matches_group = |g: &str| -> bool {
        let ng = normalize_group_path(g);
        ng == norm || ng.strip_prefix(&norm).is_some_and(|rest| rest.starts_with('/'))
    };
    cfg.groups.retain(|g| !matches_group(g));
    cfg.group_secret_keys.retain(|g, _| !matches_group(g));
    cfg.ui.collapsed_groups.retain(|g| !matches_group(g));

    let mut removed_ids = Vec::new();
    cfg.repos.retain(|repo| {
        if group_contains_repo(&norm, repo.group.as_deref()) {
            removed_ids.push(repo.id.clone());
            false
        } else {
            true
        }
    });
    if let Some(sel) = &cfg.ui.selected_repo_id
        && removed_ids.contains(sel)
    {
        cfg.ui.selected_repo_id = cfg.repos.first().map(|r| r.id.clone());
    }
    removed_ids
}

fn ensure_group_ancestors(groups: &mut Vec<String>, normalized: &str) {
    let mut acc = String::new();
    for seg in normalized.split('/').filter(|s| !s.is_empty()) {
        if !acc.is_empty() {
            acc.push('/');
        }
        acc.push_str(seg);
        if !groups.contains(&acc) {
            groups.push(acc.clone());
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentWorkspace {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceListItem {
    pub name: String,
    pub path: PathBuf,
    pub repo_count: usize,
    pub is_current: bool,
    pub is_home: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceOwnership {
    pub is_owner: bool,
    pub owner_pid: Option<u32>,
    pub lock_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrefixRelocation {
    pub old_prefix: PathBuf,
    pub new_prefix: PathBuf,
    pub affected_repo_ids: Vec<RepoId>,
}

/// Default recursive folder scan depth when discovering Git repositories.
pub const DEFAULT_SCAN_DEPTH: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanGroupSegment {
    /// Display folder name for this group level (e.g. `"backend"`).
    pub name: String,
    /// Stable raw path key from the scanned root (e.g. `"MyCompany/backend"`),
    /// enabling individual group nodes to be dropped without affecting siblings.
    pub raw_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedRepoCandidate {
    pub name: String,
    pub path: PathBuf,
    /// Display path relative to the scanned root (e.g. `"backend/services/api"` or `"."`).
    pub relative_path: String,
    /// Candidate group segments ordered from outermost (level 0) to innermost.
    /// When `ScanFolderResult::has_root_level` is true, index 0 is the scanned root folder's name.
    pub raw_segments: Vec<ScanGroupSegment>,
    /// True if this repository is already registered in the active workspace.
    pub already_in_workspace: bool,
    /// Normalized existing group in the workspace if `already_in_workspace` is true.
    pub existing_group: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanLevelInfo {
    pub depth: usize,
    pub is_root_folder: bool,
    /// Sorted unique folder names appearing at this depth level.
    pub folder_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ScanFolderResult {
    pub root: PathBuf,
    pub max_depth: usize,
    pub has_root_level: bool,
    pub levels: Vec<ScanLevelInfo>,
    pub default_enabled_levels: Vec<bool>,
    pub repos: Vec<ScannedRepoCandidate>,
}

#[derive(Debug, Clone, Default)]
pub struct ScanImportReport {
    pub added: Vec<RepoEntry>,
    pub regrouped: Vec<RepoEntry>,
    pub groups_created: usize,
}

/// Resolves the final normalized group path for a scanned repository given the enabled depth
/// levels, any individually skipped raw group nodes, and an optional target parent group prefix.
pub fn resolve_scanned_repo_group(
    raw_segments: &[ScanGroupSegment],
    enabled_levels: &[bool],
    skipped_raw_nodes: &HashSet<String>,
    parent_group: Option<&str>,
) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(parent) = parent_group {
        let norm = normalize_group_path(parent);
        if !norm.is_empty() {
            parts.push(norm);
        }
    }
    for (idx, seg) in raw_segments.iter().enumerate() {
        if enabled_levels.get(idx).copied().unwrap_or(true)
            && !skipped_raw_nodes.contains(&seg.raw_path)
        {
            parts.push(seg.name.clone());
        }
    }
    let joined = normalize_group_path(&parts.join("/"));
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

/// Builds a structured scan preview (with folder levels and smart default toggles) from a list
/// of discovered repository paths under `root`.
pub fn build_scan_result(
    root: &Path,
    discovered: Vec<PathBuf>,
    existing_repos: &[RepoEntry],
    max_depth: usize,
) -> ScanFolderResult {
    let canonical_root = canonicalize_clean(root);
    let mut existing_by_key: HashMap<String, Option<String>> =
        HashMap::with_capacity(existing_repos.len());
    for repo in existing_repos {
        let norm_group = repo
            .group
            .as_deref()
            .map(normalize_group_path)
            .filter(|g| !g.is_empty());
        existing_by_key.insert(normalize_path_key(&repo.path), norm_group);
    }

    // Deduplicate and deterministically sort discovered paths
    let mut unique_paths: Vec<PathBuf> = Vec::with_capacity(discovered.len());
    let mut seen_keys: HashSet<String> = HashSet::with_capacity(discovered.len());
    let mut sorted_discovered = discovered;
    sorted_discovered.sort();
    for p in sorted_discovered {
        let key = normalize_path_key(&p);
        if seen_keys.insert(key) {
            unique_paths.push(p);
        }
    }

    let root_folder_name: Option<String> = root
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let root_is_repo = unique_paths.len() == 1 && same_workspace_path(&unique_paths[0], root);
    let has_root_level = root_folder_name.is_some() && !root_is_repo && !unique_paths.is_empty();

    let mut repos = Vec::with_capacity(unique_paths.len());
    for repo_path in unique_paths {
        let rel_owned: PathBuf = repo_path
            .strip_prefix(root)
            .map(Path::to_path_buf)
            .ok()
            .or_else(|| {
                canonicalize_clean(&repo_path)
                    .strip_prefix(&canonical_root)
                    .ok()
                    .map(Path::to_path_buf)
            })
            .unwrap_or_default();

        let components: Vec<String> = rel_owned
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => {
                    let trimmed = s.to_string_lossy().trim().to_string();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed)
                    }
                }
                _ => None,
            })
            .collect();

        let (relative_path, parent_dirs): (String, &[String]) = if components.is_empty() {
            (".".to_string(), &[])
        } else {
            let rel_str = components.join("/");
            (rel_str, &components[..components.len().saturating_sub(1)])
        };

        let mut raw_segments = Vec::new();
        let mut raw_acc = String::new();
        if has_root_level
            && let Some(root_name) = &root_folder_name
        {
            raw_acc.push_str(root_name);
            raw_segments.push(ScanGroupSegment {
                name: root_name.clone(),
                raw_path: raw_acc.clone(),
            });
        }
        for dir_seg in parent_dirs {
            if !raw_acc.is_empty() {
                raw_acc.push('/');
            }
            raw_acc.push_str(dir_seg);
            raw_segments.push(ScanGroupSegment {
                name: dir_seg.clone(),
                raw_path: raw_acc.clone(),
            });
        }

        let name = repo_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("repo")
            .to_string();
        let path_key = normalize_path_key(&repo_path);
        let (already_in_workspace, existing_group) = match existing_by_key.get(&path_key) {
            Some(grp) => (true, grp.clone()),
            None => (false, None),
        };

        repos.push(ScannedRepoCandidate {
            name,
            path: repo_path,
            relative_path,
            raw_segments,
            already_in_workspace,
            existing_group,
        });
    }

    let max_level_count = repos
        .iter()
        .map(|r| r.raw_segments.len())
        .max()
        .unwrap_or(0);
    let mut levels = Vec::with_capacity(max_level_count);
    for depth in 0..max_level_count {
        let mut names = BTreeSet::new();
        for r in &repos {
            if let Some(seg) = r.raw_segments.get(depth) {
                names.insert(seg.name.clone());
            }
        }
        levels.push(ScanLevelInfo {
            depth,
            is_root_folder: has_root_level && depth == 0,
            folder_names: names.into_iter().collect(),
        });
    }

    let mut default_enabled_levels = vec![true; levels.len()];
    if has_root_level && levels.len() > 1 {
        let has_direct_child_repo = repos.iter().any(|r| r.raw_segments.len() <= 1);
        if !has_direct_child_repo {
            // Every discovered repo already lives inside a subfolder (e.g. scanning D:\Projects
            // containing client-a/api, client-b/web): omit the redundant outer container folder
            // by default while keeping it one click away as Level 0.
            default_enabled_levels[0] = false;
        }
    }

    ScanFolderResult {
        root: root.to_path_buf(),
        max_depth,
        has_root_level,
        levels,
        default_enabled_levels,
        repos,
    }
}

pub fn config_dir() -> Option<PathBuf> {
    ProjectDirs::from("org", "gitfleet", "gitfleet").map(|dirs| dirs.config_dir().to_path_buf())
}

pub fn home_workspace_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("home-workspace.json"))
}

pub fn workspaces_dir() -> Option<PathBuf> {
    config_dir().map(|d| d.join("workspaces"))
}

fn slugify_workspace_name(name: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
    }
    let trimmed = slug.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "workspace".to_string()
    } else {
        trimmed
    }
}

fn unique_workspace_path_in(dir: &Path, name: &str) -> PathBuf {
    let base = slugify_workspace_name(name);
    let candidate = dir.join(format!("{base}.gitfleet.json"));
    if !candidate.exists() {
        return candidate;
    }
    for n in 2..=999 {
        let next = dir.join(format!("{base}-{n}.gitfleet.json"));
        if !next.exists() {
            return next;
        }
    }
    dir.join(format!(
        "{base}-{}.gitfleet.json",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    ))
}

fn recent_workspaces_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("recent-workspaces.json"))
}

fn lock_file_for(workspace_path: &Path) -> PathBuf {
    let mut p = workspace_path.as_os_str().to_os_string();
    p.push(".lock");
    PathBuf::from(p)
}

fn normalize_path_key(p: &Path) -> String {
    let resolved = canonicalize_clean(p);
    let s = resolved.to_string_lossy();
    #[cfg(windows)]
    {
        s.replace('/', "\\").to_lowercase()
    }
    #[cfg(not(windows))]
    {
        s.into_owned()
    }
}

/// Strips legacy Windows `\\?\` / `\\?\UNC\` verbatim prefixes persisted in older workspace files
/// and refreshes `available` flags. Returns `true` if any stored path was cleaned.
fn sanitize_loaded_workspace_paths(cfg: &mut WorkspaceConfig) -> bool {
    let mut cleaned_any = false;
    for repo in &mut cfg.repos {
        let cleaned = strip_windows_verbatim_prefix(&repo.path);
        if cleaned != repo.path {
            repo.path = cleaned;
            cleaned_any = true;
        }
        repo.available = repo.path.exists();
    }
    cleaned_any
}

fn same_workspace_path(a: &Path, b: &Path) -> bool {
    normalize_path_key(a) == normalize_path_key(b)
}

fn read_recent_workspaces_file(path: &Path) -> Vec<RecentWorkspace> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn rename_workspace_file_in(
    path: &Path,
    new_name: &str,
    is_home: bool,
    recent_path: Option<&Path>,
) -> AppResult<String> {
    let trimmed = new_name.trim().to_string();
    if trimmed.is_empty() {
        return Err(AppError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Workspace name cannot be empty.",
        )));
    }
    let mut cfg: WorkspaceConfig = if path.exists() {
        let raw = std::fs::read_to_string(path)?;
        serde_json::from_str(&raw)?
    } else if is_home {
        WorkspaceConfig::default()
    } else {
        return Err(AppError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Workspace file not found: {}", path.display()),
        )));
    };
    cfg.name = trimmed.clone();
    cfg.file_path = Some(path.to_path_buf());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(&cfg)?;
    std::fs::write(path, json)?;

    if let Some(rp) = recent_path {
        let mut list = read_recent_workspaces_file(rp);
        let mut changed = false;
        for rec in &mut list {
            if same_workspace_path(&rec.path, path) && rec.name != trimmed {
                rec.name = trimmed.clone();
                changed = true;
            }
        }
        if changed && let Ok(rec_json) = serde_json::to_string_pretty(&list) {
            if let Some(parent) = rp.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(rp, rec_json);
        }
    }

    Ok(trimmed)
}

fn list_workspaces_in(
    cur: &WorkspaceConfig,
    home_path: Option<PathBuf>,
    ws_dir: Option<PathBuf>,
    cfg_dir: Option<PathBuf>,
    recents: Vec<RecentWorkspace>,
) -> Vec<WorkspaceListItem> {
    let cur_path = cur.file_path.clone().or_else(|| home_path.clone());
    let mut seen: Vec<String> = Vec::new();
    let mut items: Vec<WorkspaceListItem> = Vec::new();

    let mut push_path = |path: PathBuf, fallback_name: Option<String>| {
        let key = normalize_path_key(&path);
        if seen.iter().any(|s| s == &key) {
            return;
        }
        let is_current = cur_path
            .as_deref()
            .is_some_and(|cp| same_workspace_path(cp, &path));
        let is_home = home_path
            .as_deref()
            .is_some_and(|hp| same_workspace_path(hp, &path));

        if is_current {
            seen.push(key);
            items.push(WorkspaceListItem {
                name: cur.name.clone(),
                path,
                repo_count: cur.repos.len(),
                is_current: true,
                is_home,
            });
            return;
        }

        if let Ok(raw) = std::fs::read_to_string(&path)
            && let Ok(parsed) = serde_json::from_str::<WorkspaceConfig>(&raw)
        {
            seen.push(key);
            items.push(WorkspaceListItem {
                name: parsed.name,
                path,
                repo_count: parsed.repos.len(),
                is_current: false,
                is_home,
            });
        } else if is_home {
            seen.push(key);
            items.push(WorkspaceListItem {
                name: fallback_name.unwrap_or_else(|| "Home Workspace".to_string()),
                path,
                repo_count: 0,
                is_current: false,
                is_home: true,
            });
        }
    };

    // 1. Always show the currently active workspace first
    if let Some(cp) = cur_path.clone() {
        push_path(cp, Some(cur.name.clone()));
    }

    // 2. Recent workspaces (in most-recently-used order)
    for rec in recents {
        push_path(rec.path, Some(rec.name));
    }

    // 3. Home Workspace (always switchable even if recent-workspaces.json was empty)
    if let Some(hp) = home_path.clone() {
        push_path(hp, Some("Home Workspace".to_string()));
    }

    // 4. All saved workspaces inside `<config_dir>/workspaces/`
    if let Some(dir) = ws_dir
        && let Ok(entries) = std::fs::read_dir(&dir)
    {
        let mut extra_paths: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && matches!(
                        p.extension().and_then(|e| e.to_str()),
                        Some("json" | "gitfleet")
                    )
            })
            .collect();
        extra_paths.sort();
        for p in extra_paths {
            push_path(p, None);
        }
    }

    // 5. Any `*.gitfleet.json` or `*.gitfleet` workspace snapshots saved directly in `<config_dir>`
    if let Some(dir) = cfg_dir
        && let Ok(entries) = std::fs::read_dir(&dir)
    {
        let mut root_paths: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                if !p.is_file() {
                    return false;
                }
                let fname = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                fname.ends_with(".gitfleet.json") || fname.ends_with(".gitfleet")
            })
            .collect();
        root_paths.sort();
        for p in root_paths {
            push_path(p, None);
        }
    }

    drop(push_path);
    if items.is_empty() {
        items.push(WorkspaceListItem {
            name: cur.name.clone(),
            path: PathBuf::from("home-workspace.json"),
            repo_count: cur.repos.len(),
            is_current: true,
            is_home: true,
        });
    }

    items
}

pub struct WorkspaceService {
    state: RwLock<WorkspaceConfig>,
    ownership: RwLock<WorkspaceOwnership>,
    events: EventBus,
}

impl WorkspaceService {
    pub fn new(events: EventBus) -> Self {
        let mut cfg = WorkspaceConfig::default();

        // Resume the most recently active workspace if it still exists on disk; otherwise Home Workspace.
        let mut loaded_startup = false;
        let mut migrated_paths = false;
        if let Some(rp) = recent_workspaces_path() {
            for rec in read_recent_workspaces_file(&rp) {
                if rec.path.exists()
                    && let Ok(raw) = std::fs::read_to_string(&rec.path)
                    && let Ok(mut loaded) = serde_json::from_str::<WorkspaceConfig>(&raw)
                {
                    loaded.file_path = Some(rec.path);
                    migrated_paths = sanitize_loaded_workspace_paths(&mut loaded);
                    cfg = loaded;
                    loaded_startup = true;
                    break;
                }
            }
        }

        if !loaded_startup && let Some(home_path) = home_workspace_path() {
            if home_path.exists()
                && let Ok(raw) = std::fs::read_to_string(&home_path)
                && let Ok(mut loaded) = serde_json::from_str::<WorkspaceConfig>(&raw)
            {
                loaded.file_path = Some(home_path.clone());
                migrated_paths = sanitize_loaded_workspace_paths(&mut loaded);
                cfg = loaded;
            } else {
                cfg.file_path = Some(home_path);
            }
        }
        let own = Self::acquire_lock_for(cfg.file_path.as_deref());
        let svc = Self {
            state: RwLock::new(cfg),
            ownership: RwLock::new(own),
            events,
        };
        if migrated_paths {
            svc.auto_save();
        }
        svc
    }

    pub fn release_lock(&self) {
        self.release_current_lock();
    }

    fn release_current_lock(&self) {
        if let Ok(own) = self.ownership.read()
            && own.is_owner
            && let Some(lp) = &own.lock_path
        {
            let _ = std::fs::remove_file(lp);
        }
    }

    fn acquire_lock_for(path: Option<&Path>) -> WorkspaceOwnership {
        let Some(ws_path) = path else {
            return WorkspaceOwnership {
                is_owner: true,
                owner_pid: Some(std::process::id()),
                lock_path: None,
            };
        };
        if let Some(parent) = ws_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let lock_path = lock_file_for(ws_path);
        let my_pid = std::process::id();
        if lock_path.exists()
            && let Ok(txt) = std::fs::read_to_string(&lock_path)
            && let Ok(other_pid) = txt.trim().parse::<u32>()
            && other_pid != my_pid
        {
            // Another PID wrote the lock; still allow claiming via take_ownership
            return WorkspaceOwnership {
                is_owner: false,
                owner_pid: Some(other_pid),
                lock_path: Some(lock_path),
            };
        }
        let _ = std::fs::write(&lock_path, my_pid.to_string());
        WorkspaceOwnership {
            is_owner: true,
            owner_pid: Some(my_pid),
            lock_path: Some(lock_path),
        }
    }

    pub fn take_ownership(&self) -> WorkspaceOwnership {
        let path = self.current().file_path;
        let my_pid = std::process::id();
        let lock_path = path.as_deref().map(lock_file_for);
        if let Some(lp) = &lock_path {
            let _ = std::fs::write(lp, my_pid.to_string());
        }
        let next = WorkspaceOwnership {
            is_owner: true,
            owner_pid: Some(my_pid),
            lock_path,
        };
        if let Ok(mut guard) = self.ownership.write() {
            *guard = next.clone();
        }
        self.events.publish(AppEvent::WorkspaceOwnershipChanged {
            owner_pid: Some(my_pid),
            is_owner: true,
        });
        next
    }

    fn record_recent(&self, name: &str, path: &Path) {
        let Some(rp) = recent_workspaces_path() else {
            return;
        };
        let mut list = self.recent_workspaces();
        list.retain(|r| !same_workspace_path(&r.path, path));
        list.insert(
            0,
            RecentWorkspace {
                name: name.to_string(),
                path: path.to_path_buf(),
            },
        );
        list.truncate(20);
        if let Some(parent) = rp.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&list) {
            let _ = std::fs::write(rp, json);
        }
    }

    pub fn recent_workspaces(&self) -> Vec<RecentWorkspace> {
        let Some(rp) = recent_workspaces_path() else {
            return Vec::new();
        };
        read_recent_workspaces_file(&rp)
    }

    /// Returns all available workspaces (current, recent, Home Workspace, and all saved snapshots
    /// in `<config_dir>/workspaces/`) with live repo counts, deduplicated by path.
    pub fn list_workspaces(&self) -> Vec<WorkspaceListItem> {
        let cur = self.current();
        list_workspaces_in(
            &cur,
            home_workspace_path(),
            workspaces_dir(),
            config_dir(),
            self.recent_workspaces(),
        )
    }

    pub fn delete_workspace(&self, path: &Path) -> AppResult<()> {
        let is_home = home_workspace_path()
            .as_deref()
            .is_some_and(|hp| same_workspace_path(hp, path));
        if is_home {
            return Err(AppError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Home Workspace cannot be deleted.",
            )));
        }
        let deleting_current = self
            .current()
            .file_path
            .as_deref()
            .is_some_and(|cp| same_workspace_path(cp, path));

        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
        let lp = lock_file_for(path);
        if lp.exists() {
            let _ = std::fs::remove_file(lp);
        }

        if let Some(rp) = recent_workspaces_path() {
            let mut list = self.recent_workspaces();
            list.retain(|r| !same_workspace_path(&r.path, path));
            if let Ok(json) = serde_json::to_string_pretty(&list) {
                let _ = std::fs::write(rp, json);
            }
        }

        if deleting_current {
            let _ = self.open_home_workspace();
        }
        Ok(())
    }

    fn auto_save(&self) {
        let cfg = self.current();
        let target = cfg.file_path.clone().or_else(home_workspace_path);
        if let Some(path) = target {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string_pretty(&cfg) {
                let _ = std::fs::write(&path, json);
            }
            self.record_recent(&cfg.name, &path);
        }
    }

    pub fn current(&self) -> WorkspaceConfig {
        self.state.read().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn new_workspace(&self, name: String) -> AppResult<WorkspaceConfig> {
        // Persist the outgoing workspace first so it is never lost before switching!
        self.auto_save();
        self.release_current_lock();

        let file_path = workspaces_dir().map(|dir| {
            let _ = std::fs::create_dir_all(&dir);
            unique_workspace_path_in(&dir, &name)
        });
        let cfg = WorkspaceConfig {
            name,
            file_path: file_path.clone(),
            groups: Vec::new(),
            group_secret_keys: HashMap::new(),
            repos: Vec::new(),
            ui: WorkspaceUiState::default(),
        };
        let own = Self::acquire_lock_for(file_path.as_deref());
        if let Ok(mut o) = self.ownership.write() {
            *o = own;
        }
        if let Ok(mut guard) = self.state.write() {
            *guard = cfg.clone();
        }
        self.auto_save();
        Ok(cfg)
    }

    pub fn open_home_workspace(&self) -> AppResult<WorkspaceConfig> {
        if let Some(home) = home_workspace_path()
            && home.exists()
        {
            return self.load_from_file(&home);
        }
        self.release_current_lock();
        let mut cfg = WorkspaceConfig::default();
        cfg.file_path = home_workspace_path();
        let own = Self::acquire_lock_for(cfg.file_path.as_deref());
        if let Ok(mut o) = self.ownership.write() {
            *o = own;
        }
        if let Ok(mut guard) = self.state.write() {
            *guard = cfg.clone();
        }
        self.auto_save();
        Ok(cfg)
    }

    pub fn is_current_home(&self) -> bool {
        let cur_path = self.current().file_path.or_else(home_workspace_path);
        match (cur_path.as_deref(), home_workspace_path().as_deref()) {
            (Some(cp), Some(hp)) => same_workspace_path(cp, hp),
            _ => true,
        }
    }

    pub fn rename_workspace(&self, name: String) -> AppResult<()> {
        let trimmed = name.trim().to_string();
        if trimmed.is_empty() {
            return Err(AppError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Workspace name cannot be empty.",
            )));
        }
        let mut cfg = self.current();
        cfg.name = trimmed.clone();
        let target = cfg.file_path.clone().or_else(home_workspace_path);
        if let Some(path) = target {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            cfg.file_path = Some(path.clone());
            let json = serde_json::to_string_pretty(&cfg)?;
            std::fs::write(&path, json)?;
            if let Ok(mut guard) = self.state.write() {
                guard.name = trimmed;
                guard.file_path = Some(path.clone());
            }
            self.record_recent(&cfg.name, &path);
        } else if let Ok(mut guard) = self.state.write() {
            guard.name = trimmed;
        }
        Ok(())
    }

    pub fn rename_workspace_at(&self, path: &Path, name: String) -> AppResult<String> {
        let trimmed = name.trim().to_string();
        if trimmed.is_empty() {
            return Err(AppError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Workspace name cannot be empty.",
            )));
        }
        let cur_path = self.current().file_path.or_else(home_workspace_path);
        if cur_path
            .as_deref()
            .is_some_and(|cp| same_workspace_path(cp, path))
        {
            self.rename_workspace(trimmed.clone())?;
            return Ok(trimmed);
        }
        let is_home = home_workspace_path()
            .as_deref()
            .is_some_and(|hp| same_workspace_path(hp, path));
        rename_workspace_file_in(path, &trimmed, is_home, recent_workspaces_path().as_deref())
    }

    pub fn add_group(&self, name: String) -> AppResult<()> {
        let normalized = normalize_group_path(&name);
        if normalized.is_empty() {
            return Ok(());
        }
        if let Ok(mut guard) = self.state.write() {
            ensure_group_ancestors(&mut guard.groups, &normalized);
        }
        self.auto_save();
        Ok(())
    }

    pub fn rename_group(&self, old_name: &str, new_name: String) -> AppResult<String> {
        let renamed = if let Ok(mut guard) = self.state.write() {
            rename_group_in(&mut guard, old_name, &new_name)?
        } else {
            normalize_group_path(&new_name)
        };
        self.auto_save();
        Ok(renamed)
    }

    pub fn delete_group(&self, name: &str) -> AppResult<Vec<RepoId>> {
        let removed_ids = if let Ok(mut guard) = self.state.write() {
            delete_group_in(&mut guard, name)
        } else {
            Vec::new()
        };
        self.auto_save();
        Ok(removed_ids)
    }

    pub fn toggle_group_collapsed(&self, group_name: &str) {
        if let Ok(mut guard) = self.state.write() {
            if let Some(pos) = guard
                .ui
                .collapsed_groups
                .iter()
                .position(|g| g == group_name)
            {
                guard.ui.collapsed_groups.remove(pos);
            } else {
                guard.ui.collapsed_groups.push(group_name.to_string());
            }
        }
        self.auto_save();
    }

    pub fn add_repo(&self, path: PathBuf, group: Option<String>) -> AppResult<RepoEntry> {
        let normalized = canonicalize_clean(&path);
        if let Ok(guard) = self.state.read()
            && let Some(existing) = guard
                .repos
                .iter()
                .find(|r| same_workspace_path(&r.path, &normalized))
        {
            return Ok(existing.clone());
        }

        let target_group = group
            .map(|g| normalize_group_path(&g))
            .filter(|g| !g.is_empty());
        let name = normalized
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("repo")
            .to_string();
        let entry = RepoEntry {
            id: RepoId::new(),
            name,
            available: normalized.exists(),
            path: normalized,
            group: target_group.clone(),
            pinned: false,
            secret_key_id: None,
            secret_profile: None,
        };
        if let Ok(mut guard) = self.state.write() {
            if let Some(g) = &target_group {
                ensure_group_ancestors(&mut guard.groups, g);
            }
            guard.repos.push(entry.clone());
        }
        self.auto_save();
        self.events.publish(AppEvent::RepoChanged {
            repo_id: entry.id.clone(),
        });
        Ok(entry)
    }

    pub fn toggle_pin(&self, repo_id: &RepoId) -> AppResult<bool> {
        let mut pinned = false;
        if let Ok(mut guard) = self.state.write()
            && let Some(repo) = guard.repos.iter_mut().find(|r| &r.id == repo_id)
        {
            repo.pinned = !repo.pinned;
            pinned = repo.pinned;
        }
        self.auto_save();
        Ok(pinned)
    }

    pub fn remove_repos(&self, repo_ids: &[RepoId]) -> AppResult<usize> {
        let mut removed = 0;
        if let Ok(mut guard) = self.state.write() {
            let before = guard.repos.len();
            guard.repos.retain(|r| !repo_ids.contains(&r.id));
            removed = before - guard.repos.len();
        }
        self.auto_save();
        Ok(removed)
    }

    pub fn set_repo_group(&self, repo_id: &RepoId, group: Option<String>) -> AppResult<()> {
        let normalized = group
            .map(|g| normalize_group_path(&g))
            .filter(|g| !g.is_empty());
        let mut found = false;
        if let Ok(mut guard) = self.state.write() {
            if let Some(g) = &normalized {
                ensure_group_ancestors(&mut guard.groups, g);
            }
            if let Some(repo) = guard.repos.iter_mut().find(|r| &r.id == repo_id) {
                repo.group = normalized;
                found = true;
            }
        }
        if found {
            self.auto_save();
            Ok(())
        } else {
            Err(AppError::RepoNotFound(repo_id.0.clone()))
        }
    }

    /// Assigns (or clears with `None`) an explicit Age secret key ID on a single repository.
    pub fn set_repo_secret_key(
        &self,
        repo_id: &RepoId,
        key_id: Option<String>,
    ) -> AppResult<()> {
        let cleaned = key_id
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let mut found = false;
        if let Ok(mut guard) = self.state.write()
            && let Some(repo) = guard.repos.iter_mut().find(|r| &r.id == repo_id)
        {
            repo.secret_key_id = cleaned;
            found = true;
        }
        if found {
            self.auto_save();
            Ok(())
        } else {
            Err(AppError::RepoNotFound(repo_id.0.clone()))
        }
    }

    /// Assigns (or clears with `None`) an Age secret key ID on a repository group. All nested
    /// subgroups and repositories inherit this key unless overridden more specifically.
    pub fn set_group_secret_key(
        &self,
        group_name: &str,
        key_id: Option<String>,
    ) -> AppResult<()> {
        let norm_group = normalize_group_path(group_name);
        if norm_group.is_empty() {
            return Ok(());
        }
        let cleaned = key_id
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if let Ok(mut guard) = self.state.write() {
            match cleaned {
                Some(id) => {
                    ensure_group_ancestors(&mut guard.groups, &norm_group);
                    guard.group_secret_keys.insert(norm_group, id);
                }
                None => {
                    guard.group_secret_keys.remove(&norm_group);
                }
            }
        }
        self.auto_save();
        Ok(())
    }

    /// Resolves the assigned secret key for `repo_id` (explicit repo override → nearest ancestor group).
    pub fn resolve_repo_secret_key(
        &self,
        repo_id: &RepoId,
    ) -> Option<(String, SecretKeyBindingSource)> {
        let guard = self.state.read().ok()?;
        let repo = guard.repos.iter().find(|r| &r.id == repo_id)?;
        resolve_repo_secret_key_in(&guard, repo)
    }

    /// Sets (or clears with `None` / `"default"`) the active Secrets Vault profile for `repo_id`
    /// on this machine.
    pub fn set_repo_secret_profile(
        &self,
        repo_id: &RepoId,
        profile: Option<String>,
    ) -> AppResult<()> {
        let cleaned = profile
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty() && s != "default");
        let mut found = false;
        if let Ok(mut guard) = self.state.write()
            && let Some(repo) = guard.repos.iter_mut().find(|r| &r.id == repo_id)
        {
            repo.secret_profile = cleaned;
            found = true;
        }
        if found {
            self.auto_save();
            Ok(())
        } else {
            Err(AppError::RepoNotFound(repo_id.0.clone()))
        }
    }

    /// Returns the locally remembered active Secrets Vault profile for `repo_id` (if non-default).
    pub fn repo_secret_profile(&self, repo_id: &RepoId) -> Option<String> {
        let guard = self.state.read().ok()?;
        let repo = guard.repos.iter().find(|r| &r.id == repo_id)?;
        repo.secret_profile
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != "default")
            .map(str::to_string)
    }

    /// Removes any repo or group secret key bindings referencing a deleted `key_id`.
    pub fn clear_secret_key_bindings(&self, key_id: &str) {
        let mut changed = false;
        if let Ok(mut guard) = self.state.write() {
            let before = guard.group_secret_keys.len();
            guard.group_secret_keys.retain(|_, v| v != key_id);
            if guard.group_secret_keys.len() != before {
                changed = true;
            }
            for repo in &mut guard.repos {
                if repo.secret_key_id.as_deref() == Some(key_id) {
                    repo.secret_key_id = None;
                    changed = true;
                }
            }
        }
        if changed {
            self.auto_save();
        }
    }

    /// Updates a missing repo's path and returns a prefix-relocation suggestion if missing siblings share the same old parent!
    pub fn locate_repo(
        &self,
        repo_id: &RepoId,
        picked_path: PathBuf,
    ) -> AppResult<Option<PrefixRelocation>> {
        let normalized = canonicalize_clean(&picked_path);
        let mut suggestion: Option<PrefixRelocation> = None;

        if let Ok(mut guard) = self.state.write() {
            let old_path = guard
                .repos
                .iter()
                .find(|r| &r.id == repo_id)
                .map(|r| r.path.clone())
                .ok_or_else(|| AppError::RepoNotFound(repo_id.0.clone()))?;

            if let Some(repo) = guard.repos.iter_mut().find(|r| &r.id == repo_id) {
                repo.path = normalized.clone();
                repo.available = normalized.exists();
            }

            if let (Some(old_parent), Some(new_parent)) = (old_path.parent(), normalized.parent())
                && old_parent != new_parent
            {
                let siblings: Vec<RepoId> = guard
                    .repos
                    .iter()
                    .filter(|r| {
                        &r.id != repo_id
                            && !r.available
                            && r.path.starts_with(old_parent)
                            && r.path
                                .strip_prefix(old_parent)
                                .ok()
                                .is_some_and(|rel| new_parent.join(rel).exists())
                    })
                    .map(|r| r.id.clone())
                    .collect();
                if !siblings.is_empty() {
                    suggestion = Some(PrefixRelocation {
                        old_prefix: old_parent.to_path_buf(),
                        new_prefix: new_parent.to_path_buf(),
                        affected_repo_ids: siblings,
                    });
                }
            }
        }
        self.auto_save();
        Ok(suggestion)
    }

    pub fn relocate_prefix(&self, mapping: &PrefixRelocation) -> AppResult<usize> {
        let mut updated = 0;
        if let Ok(mut guard) = self.state.write() {
            for repo in &mut guard.repos {
                if mapping.affected_repo_ids.contains(&repo.id)
                    && let Ok(rel) = repo.path.strip_prefix(&mapping.old_prefix)
                {
                    let candidate = strip_windows_verbatim_prefix(&mapping.new_prefix.join(rel));
                    repo.available = candidate.exists();
                    repo.path = candidate;
                    updated += 1;
                }
            }
        }
        self.auto_save();
        Ok(updated)
    }

    /// Recursively discovers Git repositories up to `DEFAULT_SCAN_DEPTH`, skipping heavy build folders.
    pub fn scan_folder(&self, root: &Path) -> AppResult<Vec<PathBuf>> {
        self.scan_folder_with_depth(root, DEFAULT_SCAN_DEPTH)
    }

    /// Recursively discovers Git repositories up to `max_depth`, skipping heavy build folders.
    pub fn scan_folder_with_depth(&self, root: &Path, max_depth: usize) -> AppResult<Vec<PathBuf>> {
        let mut discovered = Vec::new();
        scan_recursive(root, 0, max_depth, &mut discovered);
        discovered.sort();
        self.events.publish(AppEvent::WorkspaceDiscovery {
            found: discovered.len(),
            finished: true,
        });
        Ok(discovered)
    }

    /// Recursively discovers Git repositories and builds a structured group-level preview against
    /// the current workspace.
    pub fn scan_folder_preview(&self, root: &Path, max_depth: usize) -> AppResult<ScanFolderResult> {
        let discovered = self.scan_folder_with_depth(root, max_depth)?;
        let existing = self.current().repos;
        Ok(build_scan_result(root, discovered, &existing, max_depth))
    }

    /// Batch-imports scanned repositories with their resolved groups (creating ancestor groups
    /// automatically), saving the workspace once. When `update_existing_groups` is true, existing
    /// repositories whose target group differs are moved to the resolved group as well.
    pub fn import_scanned_repos(
        &self,
        items: &[(PathBuf, Option<String>)],
        update_existing_groups: bool,
    ) -> AppResult<ScanImportReport> {
        let (report, changed_ids) = if let Ok(mut guard) = self.state.write() {
            import_scanned_repos_into(&mut guard, items, update_existing_groups)
        } else {
            (ScanImportReport::default(), Vec::new())
        };

        if !changed_ids.is_empty() {
            self.auto_save();
            for repo_id in changed_ids {
                self.events.publish(AppEvent::RepoChanged { repo_id });
            }
        }
        Ok(report)
    }

    pub fn set_ui_state(&self, ui: WorkspaceUiState) -> AppResult<()> {
        if let Ok(mut guard) = self.state.write() {
            guard.ui = ui;
        }
        self.auto_save();
        Ok(())
    }

    pub fn save_to_file(&self, path: &Path) -> AppResult<()> {
        let changed_path = self
            .current()
            .file_path
            .as_deref()
            .is_none_or(|cur| !same_workspace_path(cur, path));
        if changed_path {
            self.release_current_lock();
        }
        if let Ok(mut guard) = self.state.write() {
            guard.file_path = Some(path.to_path_buf());
        }
        let cfg = self.current();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let json = serde_json::to_string_pretty(&cfg)?;
        std::fs::write(path, json)?;
        if changed_path {
            let own = Self::acquire_lock_for(Some(path));
            if let Ok(mut o) = self.ownership.write() {
                *o = own;
            }
        }
        self.record_recent(&cfg.name, path);
        Ok(())
    }

    pub fn load_from_file(&self, path: &Path) -> AppResult<WorkspaceConfig> {
        let raw = std::fs::read_to_string(path)?;
        let mut cfg: WorkspaceConfig = serde_json::from_str(&raw)?;
        cfg.file_path = Some(path.to_path_buf());
        let migrated_paths = sanitize_loaded_workspace_paths(&mut cfg);
        let switching = self
            .current()
            .file_path
            .as_deref()
            .is_none_or(|cur| !same_workspace_path(cur, path));
        if switching {
            self.release_current_lock();
        }
        let own = Self::acquire_lock_for(Some(path));
        if let Ok(mut o) = self.ownership.write() {
            *o = own;
        }
        if let Ok(mut guard) = self.state.write() {
            *guard = cfg.clone();
        }
        if migrated_paths {
            self.auto_save();
        }
        self.record_recent(&cfg.name, path);
        Ok(cfg)
    }

    pub fn ownership(&self) -> WorkspaceOwnership {
        self.ownership
            .read()
            .map(|g| g.clone())
            .unwrap_or(WorkspaceOwnership {
                is_owner: true,
                owner_pid: Some(std::process::id()),
                lock_path: None,
            })
    }

    pub fn lock_status(&self, paths: &[PathBuf]) -> HashMap<PathBuf, bool> {
        paths
            .iter()
            .map(|p| (p.clone(), lock_file_for(p).exists()))
            .collect()
    }
}

impl Drop for WorkspaceService {
    fn drop(&mut self) {
        if let Ok(own) = self.ownership.read()
            && own.is_owner
            && let Some(lp) = &own.lock_path
        {
            let _ = std::fs::remove_file(lp);
        }
    }
}

fn import_scanned_repos_into(
    cfg: &mut WorkspaceConfig,
    items: &[(PathBuf, Option<String>)],
    update_existing_groups: bool,
) -> (ScanImportReport, Vec<RepoId>) {
    let mut report = ScanImportReport::default();
    let mut changed_ids = Vec::new();
    let groups_before = cfg.groups.len();

    let mut existing_index: HashMap<String, usize> = HashMap::with_capacity(cfg.repos.len());
    for (idx, repo) in cfg.repos.iter().enumerate() {
        existing_index.insert(normalize_path_key(&repo.path), idx);
    }

    for (path, group) in items {
        let normalized_path = canonicalize_clean(path);
        let key = normalize_path_key(&normalized_path);
        let target_group = group
            .as_deref()
            .map(normalize_group_path)
            .filter(|g| !g.is_empty());

        if let Some(&idx) = existing_index.get(&key) {
            if update_existing_groups {
                let current_group = cfg.repos[idx]
                    .group
                    .as_deref()
                    .map(normalize_group_path)
                    .filter(|g| !g.is_empty());
                if current_group != target_group {
                    if let Some(g) = &target_group {
                        ensure_group_ancestors(&mut cfg.groups, g);
                    }
                    cfg.repos[idx].group = target_group;
                    let updated = cfg.repos[idx].clone();
                    changed_ids.push(updated.id.clone());
                    report.regrouped.push(updated);
                }
            }
            continue;
        }

        if let Some(g) = &target_group {
            ensure_group_ancestors(&mut cfg.groups, g);
        }
        let name = normalized_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("repo")
            .to_string();
        let entry = RepoEntry {
            id: RepoId::new(),
            name,
            available: normalized_path.exists(),
            path: normalized_path,
            group: target_group,
            pinned: false,
            secret_key_id: None,
            secret_profile: None,
        };
        let next_idx = cfg.repos.len();
        existing_index.insert(key, next_idx);
        changed_ids.push(entry.id.clone());
        report.added.push(entry.clone());
        cfg.repos.push(entry);
    }

    report.groups_created = cfg.groups.len().saturating_sub(groups_before);
    (report, changed_ids)
}

fn scan_recursive(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<PathBuf>) {
    if depth > max_depth || !dir.is_dir() {
        return;
    }
    if dir.join(".git").exists() {
        out.push(dir.to_path_buf());
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.')
            || matches!(
                name_str.as_ref(),
                "node_modules" | "target" | "dist" | "build" | "vendor" | "__pycache__"
            )
        {
            continue;
        }
        scan_recursive(&path, depth + 1, max_depth, out);
    }
}

/// Renames a group (and all its descendant subgroups, assigned repositories, collapsed states,
/// and group secret key bindings) cleanly, ensuring ancestor groups exist and deduplicating lists.
fn rename_group_in(cfg: &mut WorkspaceConfig, old_name: &str, new_name: &str) -> AppResult<String> {
    let old_norm = normalize_group_path(old_name);
    let new_norm = normalize_group_path(new_name);
    if old_norm.is_empty() || new_norm.is_empty() {
        return Err(AppError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Group name cannot be empty.",
        )));
    }
    if new_norm == old_norm {
        return Ok(new_norm);
    }

    let old_prefix = format!("{old_norm}/");
    if new_norm.starts_with(&old_prefix) {
        return Err(AppError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Cannot rename a group into one of its own subgroups.",
        )));
    }

    if cfg.groups.iter().any(|g| g == &new_norm)
        || cfg
            .repos
            .iter()
            .any(|r| r.group.as_deref() == Some(new_norm.as_str()))
    {
        return Err(AppError::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("Group '{new_norm}' already exists."),
        )));
    }

    let rewrite = |val: &str| -> Option<String> {
        if val == old_norm {
            Some(new_norm.clone())
        } else {
            val.strip_prefix(&old_prefix)
                .map(|rest| format!("{new_norm}/{rest}"))
        }
    };

    for g in &mut cfg.groups {
        if let Some(next) = rewrite(g) {
            *g = next;
        }
    }
    ensure_group_ancestors(&mut cfg.groups, &new_norm);
    let mut seen_groups = HashSet::with_capacity(cfg.groups.len());
    cfg.groups.retain(|g| seen_groups.insert(g.clone()));

    for repo in &mut cfg.repos {
        if let Some(cur) = repo.group.as_deref()
            && let Some(next) = rewrite(cur)
        {
            repo.group = Some(next);
        }
    }

    for c in &mut cfg.ui.collapsed_groups {
        if let Some(next) = rewrite(c) {
            *c = next;
        }
    }
    let mut seen_collapsed = HashSet::with_capacity(cfg.ui.collapsed_groups.len());
    cfg.ui
        .collapsed_groups
        .retain(|c| seen_collapsed.insert(c.clone()));

    let rewritten_keys: HashMap<String, String> = cfg
        .group_secret_keys
        .drain()
        .map(|(g, key_id)| (rewrite(&g).unwrap_or(g), key_id))
        .collect();
    cfg.group_secret_keys = rewritten_keys;

    Ok(new_norm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_builds_nested_group_ancestors_with_no_default_group() {
        let cfg = WorkspaceConfig::default();
        assert!(cfg.groups.is_empty());

        assert_eq!(
            normalize_group_path(" goatering-be \\ submodule / libs "),
            "goatering-be/submodule/libs"
        );
        assert_eq!(
            parent_group_path("goatering-be/submodule"),
            Some("goatering-be".to_string())
        );
        assert_eq!(parent_group_path("goatering-be"), None);

        let mut groups = Vec::new();
        ensure_group_ancestors(&mut groups, "goatering-be/submodule/libs");
        assert_eq!(
            groups,
            vec![
                "goatering-be".to_string(),
                "goatering-be/submodule".to_string(),
                "goatering-be/submodule/libs".to_string()
            ]
        );
    }

    #[test]
    fn generates_unique_workspace_file_paths() {
        let dir = std::env::temp_dir().join(format!(
            "gitfleet_ws_slugs_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let p1 = unique_workspace_path_in(&dir, "Goatering BE");
        assert_eq!(p1.file_name().unwrap(), "goatering-be.gitfleet.json");
        std::fs::write(&p1, "{}").unwrap();

        let p2 = unique_workspace_path_in(&dir, "Goatering BE");
        assert_eq!(p2.file_name().unwrap(), "goatering-be-2.gitfleet.json");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lists_and_deduplicates_all_workspaces() {
        let root = std::env::temp_dir().join(format!(
            "gitfleet_ws_list_{}",
            std::process::id()
        ));
        let ws_dir = root.join("workspaces");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&ws_dir).unwrap();

        let home_path = root.join("home-workspace.json");
        let home_cfg = WorkspaceConfig {
            name: "Home Workspace".to_string(),
            file_path: Some(home_path.clone()),
            groups: Vec::new(),
            group_secret_keys: HashMap::new(),
            repos: vec![RepoEntry {
                id: RepoId::new(),
                name: "repo-a".to_string(),
                path: root.join("repo-a"),
                group: None,
                pinned: false,
                available: false,
                secret_key_id: None,
                secret_profile: None,
            }],
            ui: WorkspaceUiState::default(),
        };
        std::fs::write(&home_path, serde_json::to_string(&home_cfg).unwrap()).unwrap();

        let work_path = ws_dir.join("work.gitfleet.json");
        let work_cfg = WorkspaceConfig {
            name: "Work".to_string(),
            file_path: Some(work_path.clone()),
            groups: Vec::new(),
            group_secret_keys: HashMap::new(),
            repos: Vec::new(),
            ui: WorkspaceUiState::default(),
        };
        std::fs::write(&work_path, serde_json::to_string(&work_cfg).unwrap()).unwrap();

        // Simulate switching to `Work` even if `recent-workspaces.json` only has duplicate/partial entries
        let items = list_workspaces_in(
            &work_cfg,
            Some(home_path.clone()),
            Some(ws_dir.clone()),
            Some(root.clone()),
            vec![RecentWorkspace {
                name: "Work".to_string(),
                path: work_path.clone(),
            }],
        );

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].name, "Work");
        assert!(items[0].is_current);
        assert!(!items[0].is_home);

        assert_eq!(items[1].name, "Home Workspace");
        assert!(!items[1].is_current);
        assert!(items[1].is_home);
        assert_eq!(items[1].repo_count, 1);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn renames_saved_workspace_file_and_updates_recents_in_place() {
        let root = std::env::temp_dir().join(format!(
            "gitfleet_ws_rename_{}",
            std::process::id()
        ));
        let ws_dir = root.join("workspaces");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&ws_dir).unwrap();

        let active_path = ws_dir.join("active.gitfleet.json");
        let inactive_path = ws_dir.join("client-a.gitfleet.json");
        let recent_path = root.join("recent-workspaces.json");

        let inactive_cfg = WorkspaceConfig {
            name: "Client A".to_string(),
            file_path: Some(inactive_path.clone()),
            groups: vec!["Backend".to_string()],
            group_secret_keys: HashMap::new(),
            repos: vec![RepoEntry {
                id: RepoId::new(),
                name: "api".to_string(),
                path: root.join("api"),
                group: Some("Backend".to_string()),
                pinned: true,
                available: false,
                secret_key_id: None,
                secret_profile: None,
            }],
            ui: WorkspaceUiState::default(),
        };
        std::fs::write(&inactive_path, serde_json::to_string(&inactive_cfg).unwrap()).unwrap();
        std::fs::write(
            &recent_path,
            serde_json::to_string(&vec![
                RecentWorkspace {
                    name: "Active".to_string(),
                    path: active_path.clone(),
                },
                RecentWorkspace {
                    name: "Client A".to_string(),
                    path: inactive_path.clone(),
                },
            ])
            .unwrap(),
        )
        .unwrap();

        let renamed =
            rename_workspace_file_in(&inactive_path, "  Client B  ", false, Some(&recent_path))
                .unwrap();
        assert_eq!(renamed, "Client B");

        let reloaded: WorkspaceConfig =
            serde_json::from_str(&std::fs::read_to_string(&inactive_path).unwrap()).unwrap();
        assert_eq!(reloaded.name, "Client B");
        assert_eq!(reloaded.groups, vec!["Backend".to_string()]);
        assert_eq!(reloaded.repos.len(), 1);
        assert!(reloaded.repos[0].pinned);

        // MRU order must be preserved (Active stays first, renamed inactive stays second)
        let recents = read_recent_workspaces_file(&recent_path);
        assert_eq!(recents.len(), 2);
        assert_eq!(recents[0].name, "Active");
        assert_eq!(recents[1].name, "Client B");

        // Rejects empty/whitespace-only names
        assert!(
            rename_workspace_file_in(&inactive_path, "   ", false, Some(&recent_path)).is_err()
        );

        // Uninitialized Home Workspace can be renamed and preserves is_home
        let home_path = root.join("home-workspace.json");
        let home_renamed =
            rename_workspace_file_in(&home_path, "Main Workspace", true, Some(&recent_path))
                .unwrap();
        assert_eq!(home_renamed, "Main Workspace");
        assert!(home_path.exists());
        let home_reloaded: WorkspaceConfig =
            serde_json::from_str(&std::fs::read_to_string(&home_path).unwrap()).unwrap();
        assert_eq!(home_reloaded.name, "Main Workspace");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn builds_scan_preview_levels_and_resolves_dropped_levels() {
        let root = std::env::temp_dir().join(format!(
            "gitfleet_scan_preview_{}",
            std::process::id()
        ));
        let scan_root = root.join("MyCompany");
        let _ = std::fs::remove_dir_all(&root);

        let auth = scan_root.join("backend").join("services").join("auth");
        let billing = scan_root.join("backend").join("services").join("billing");
        let web = scan_root.join("frontend").join("web");
        for repo in [&auth, &billing, &web] {
            std::fs::create_dir_all(repo.join(".git")).unwrap();
        }

        let mut discovered = Vec::new();
        scan_recursive(&scan_root, 0, DEFAULT_SCAN_DEPTH, &mut discovered);
        discovered.sort();
        assert_eq!(discovered.len(), 3);

        let preview = build_scan_result(&scan_root, discovered.clone(), &[], DEFAULT_SCAN_DEPTH);
        assert!(preview.has_root_level);
        assert_eq!(preview.levels.len(), 3);
        assert!(preview.levels[0].is_root_folder);
        assert_eq!(preview.levels[0].folder_names, vec!["MyCompany"]);
        assert_eq!(preview.levels[1].folder_names, vec!["backend", "frontend"]);
        assert_eq!(preview.levels[2].folder_names, vec!["services"]);
        // All repos live inside subfolders -> omit redundant outer root folder by default
        assert_eq!(preview.default_enabled_levels, vec![false, true, true]);

        let auth_cand = preview.repos.iter().find(|r| r.name == "auth").unwrap();
        let web_cand = preview.repos.iter().find(|r| r.name == "web").unwrap();

        // Default resolution: backend/services & frontend
        let empty_skip = HashSet::new();
        assert_eq!(
            resolve_scanned_repo_group(
                &auth_cand.raw_segments,
                &preview.default_enabled_levels,
                &empty_skip,
                None,
            ),
            Some("backend/services".to_string())
        );
        assert_eq!(
            resolve_scanned_repo_group(
                &web_cand.raw_segments,
                &preview.default_enabled_levels,
                &empty_skip,
                None,
            ),
            Some("frontend".to_string())
        );

        // Drop level 2 ("services") -> compact 1-level groups
        let one_level = vec![false, true, false];
        assert_eq!(
            resolve_scanned_repo_group(&auth_cand.raw_segments, &one_level, &empty_skip, None),
            Some("backend".to_string())
        );

        // Enable root level + drop individual raw node "MyCompany/backend/services"
        let all_levels = vec![true, true, true];
        let mut skipped = HashSet::new();
        skipped.insert("MyCompany/backend/services".to_string());
        assert_eq!(
            resolve_scanned_repo_group(&auth_cand.raw_segments, &all_levels, &skipped, None),
            Some("MyCompany/backend".to_string())
        );

        // Flat (all levels off) with explicit parent group prefix
        let flat = vec![false, false, false];
        assert_eq!(
            resolve_scanned_repo_group(&auth_cand.raw_segments, &flat, &empty_skip, Some("Work")),
            Some("Work".to_string())
        );
        assert_eq!(
            resolve_scanned_repo_group(&auth_cand.raw_segments, &flat, &empty_skip, None),
            None
        );

        // Direct child repo in root -> root level is ON by default so direct children stay grouped
        let docs = scan_root.join("docs");
        std::fs::create_dir_all(docs.join(".git")).unwrap();
        let mut discovered_mixed = Vec::new();
        scan_recursive(&scan_root, 0, DEFAULT_SCAN_DEPTH, &mut discovered_mixed);
        let preview_mixed = build_scan_result(&scan_root, discovered_mixed, &[], DEFAULT_SCAN_DEPTH);
        assert_eq!(preview_mixed.default_enabled_levels, vec![true, true, true]);

        // Flat folder of direct child repos -> only Level 0 exists and is ON by default
        let flat_root = root.join("goatering");
        std::fs::create_dir_all(flat_root.join("api").join(".git")).unwrap();
        std::fs::create_dir_all(flat_root.join("web").join(".git")).unwrap();
        let mut flat_discovered = Vec::new();
        scan_recursive(&flat_root, 0, DEFAULT_SCAN_DEPTH, &mut flat_discovered);
        let flat_preview = build_scan_result(&flat_root, flat_discovered, &[], DEFAULT_SCAN_DEPTH);
        assert_eq!(flat_preview.levels.len(), 1);
        assert_eq!(flat_preview.default_enabled_levels, vec![true]);

        // Scanning a folder that IS a git repo itself produces 0 group levels
        let single_preview = build_scan_result(&auth, vec![auth.clone()], &[], DEFAULT_SCAN_DEPTH);
        assert!(!single_preview.has_root_level);
        assert!(single_preview.levels.is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn batch_imports_and_regroups_scanned_repos_with_ancestor_groups() {
        let root = std::env::temp_dir().join(format!(
            "gitfleet_scan_import_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let api = root.join("backend").join("services").join("api");
        let web = root.join("frontend").join("web");
        std::fs::create_dir_all(api.join(".git")).unwrap();
        std::fs::create_dir_all(web.join(".git")).unwrap();

        let mut cfg = WorkspaceConfig::default();
        // Pre-add `api` ungrouped at root
        cfg.repos.push(RepoEntry {
            id: RepoId::new(),
            name: "api".to_string(),
            path: api.canonicalize().unwrap(),
            group: None,
            pinned: false,
            available: true,
            secret_key_id: None,
            secret_profile: None,
        });

        // With update_existing_groups = false: `web` is added with group `frontend`, `api` untouched
        let (rep1, _) = import_scanned_repos_into(
            &mut cfg,
            &[
                (api.clone(), Some("backend/services".to_string())),
                (web.clone(), Some("frontend".to_string())),
            ],
            false,
        );
        assert_eq!(rep1.added.len(), 1);
        assert_eq!(rep1.regrouped.len(), 0);
        assert_eq!(rep1.groups_created, 1);
        assert_eq!(cfg.groups, vec!["frontend".to_string()]);
        assert_eq!(cfg.repos[0].group, None);
        assert_eq!(cfg.repos[1].group, Some("frontend".to_string()));

        // With update_existing_groups = true: `api` moves to `backend/services` and ancestor groups created
        let (rep2, _) = import_scanned_repos_into(
            &mut cfg,
            &[
                (api.clone(), Some("backend/services".to_string())),
                (web.clone(), Some("frontend".to_string())),
            ],
            true,
        );
        assert_eq!(rep2.added.len(), 0);
        assert_eq!(rep2.regrouped.len(), 1);
        assert_eq!(rep2.groups_created, 2);
        assert!(cfg.groups.contains(&"backend".to_string()));
        assert!(cfg.groups.contains(&"backend/services".to_string()));
        assert_eq!(cfg.repos[0].group, Some("backend/services".to_string()));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resolves_repo_and_nested_group_secret_key_inheritance() {
        let mut cfg = WorkspaceConfig::default();
        cfg.group_secret_keys
            .insert("company".to_string(), "key_company".to_string());
        cfg.group_secret_keys
            .insert("company/payments".to_string(), "key_payments".to_string());

        let inherited_repo = RepoEntry {
            id: RepoId::new(),
            name: "auth".into(),
            path: PathBuf::from("auth"),
            group: Some("company/backend/services".into()),
            pinned: false,
            available: true,
            secret_key_id: None,
            secret_profile: None,
        };
        assert_eq!(
            resolve_repo_secret_key_in(&cfg, &inherited_repo),
            Some((
                "key_company".to_string(),
                SecretKeyBindingSource::Group("company".to_string())
            ))
        );

        let subgroup_repo = RepoEntry {
            id: RepoId::new(),
            name: "billing".into(),
            path: PathBuf::from("billing"),
            group: Some("company/payments/v2".into()),
            pinned: false,
            available: true,
            secret_key_id: None,
            secret_profile: None,
        };
        assert_eq!(
            resolve_repo_secret_key_in(&cfg, &subgroup_repo),
            Some((
                "key_payments".to_string(),
                SecretKeyBindingSource::Group("company/payments".to_string())
            ))
        );

        let override_repo = RepoEntry {
            id: RepoId::new(),
            name: "special".into(),
            path: PathBuf::from("special"),
            group: Some("company/payments".into()),
            pinned: false,
            available: true,
            secret_key_id: Some("key_special".into()),
            secret_profile: None,
        };
        assert_eq!(
            resolve_repo_secret_key_in(&cfg, &override_repo),
            Some(("key_special".to_string(), SecretKeyBindingSource::Repo))
        );

        let root_repo = RepoEntry {
            id: RepoId::new(),
            name: "personal".into(),
            path: PathBuf::from("personal"),
            group: None,
            pinned: false,
            available: true,
            secret_key_id: None,
            secret_profile: None,
        };
        assert_eq!(resolve_repo_secret_key_in(&cfg, &root_repo), None);
    }

    #[test]
    fn delete_group_removes_subgroups_and_all_child_repos() {
        let mut cfg = WorkspaceConfig {
            groups: vec![
                "work".into(),
                "work/backend".into(),
                "work/backend/api".into(),
                "personal".into(),
            ],
            ..Default::default()
        };
        cfg.group_secret_keys
            .insert("work/backend".into(), "key-1".into());
        cfg.group_secret_keys
            .insert("personal".into(), "key-2".into());
        cfg.ui.collapsed_groups = vec!["work".into(), "work/backend".into(), "personal".into()];

        let r_direct = RepoEntry {
            id: RepoId::new(),
            name: "work-root".into(),
            path: PathBuf::from("work-root"),
            group: Some("work".into()),
            pinned: false,
            available: true,
            secret_key_id: None,
            secret_profile: None,
        };
        let r_nested = RepoEntry {
            id: RepoId::new(),
            name: "auth-service".into(),
            path: PathBuf::from("auth-service"),
            group: Some("work/backend/api".into()),
            pinned: false,
            available: true,
            secret_key_id: None,
            secret_profile: None,
        };
        let r_sibling = RepoEntry {
            id: RepoId::new(),
            name: "notes".into(),
            path: PathBuf::from("notes"),
            group: Some("personal".into()),
            pinned: false,
            available: true,
            secret_key_id: None,
            secret_profile: None,
        };
        let r_flat = RepoEntry {
            id: RepoId::new(),
            name: "gitfleet".into(),
            path: PathBuf::from("gitfleet"),
            group: None,
            pinned: false,
            available: true,
            secret_key_id: None,
            secret_profile: None,
        };
        cfg.ui.selected_repo_id = Some(r_nested.id.clone());
        cfg.repos = vec![
            r_direct.clone(),
            r_nested.clone(),
            r_sibling.clone(),
            r_flat.clone(),
        ];

        assert_eq!(count_repos_in_group(&cfg, "work"), 2);
        assert_eq!(count_repos_in_group(&cfg, "work/backend"), 1);
        assert_eq!(count_repos_in_group(&cfg, "personal"), 1);

        let removed = delete_group_in(&mut cfg, "work");
        assert_eq!(removed.len(), 2);
        assert!(removed.contains(&r_direct.id));
        assert!(removed.contains(&r_nested.id));

        assert_eq!(cfg.groups, vec!["personal".to_string()]);
        assert_eq!(cfg.ui.collapsed_groups, vec!["personal".to_string()]);
        assert!(!cfg.group_secret_keys.contains_key("work/backend"));
        assert!(cfg.group_secret_keys.contains_key("personal"));

        // Child repos must be removed completely — NOT flattened to root!
        let remaining_names: Vec<&str> = cfg.repos.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(remaining_names, vec!["notes", "gitfleet"]);
        assert_eq!(cfg.ui.selected_repo_id, Some(r_sibling.id));
    }

    #[test]
    fn migrates_legacy_windows_verbatim_paths_and_imports_clean_paths() {
        let mut cfg = WorkspaceConfig::default();
        cfg.repos.push(RepoEntry {
            id: RepoId::new(),
            name: "gitfleet".into(),
            path: PathBuf::from(r"\\?\D:\Project\electrogit\gitfleet"),
            group: None,
            pinned: false,
            available: false,
            secret_key_id: None,
            secret_profile: None,
        });

        assert!(sanitize_loaded_workspace_paths(&mut cfg));
        assert_eq!(
            cfg.repos[0].path,
            PathBuf::from(r"D:\Project\electrogit\gitfleet")
        );
        // Second call is a no-op
        assert!(!sanitize_loaded_workspace_paths(&mut cfg));

        // Importing a real temp repo must never persist a `\\?\` prefix
        let root = std::env::temp_dir().join(format!(
            "gitfleet_clean_path_import_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let repo_dir = root.join("demo");
        std::fs::create_dir_all(repo_dir.join(".git")).unwrap();

        let mut imported_cfg = WorkspaceConfig::default();
        let (rep, _) = import_scanned_repos_into(&mut imported_cfg, &[(repo_dir, None)], false);
        assert_eq!(rep.added.len(), 1);
        let stored = imported_cfg.repos[0].path.to_string_lossy();
        assert!(
            !stored.starts_with(r"\\?\"),
            "Stored repo path should not have Windows verbatim prefix: {stored}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn renames_group_subgroups_repos_collapsed_and_secret_keys() {
        let mut cfg = WorkspaceConfig {
            groups: vec![
                "work".into(),
                "work/backend".into(),
                "work/backend/api".into(),
                "personal".into(),
            ],
            ..Default::default()
        };
        cfg.ui.collapsed_groups = vec!["work".into(), "work/backend".into()];
        cfg.group_secret_keys
            .insert("work".into(), "key-work".into());
        cfg.group_secret_keys
            .insert("work/backend".into(), "key-be".into());
        cfg.repos = vec![
            RepoEntry {
                id: RepoId::new(),
                name: "svc".into(),
                path: PathBuf::from("svc"),
                group: Some("work/backend/api".into()),
                pinned: false,
                available: true,
                secret_key_id: None,
                secret_profile: None,
            },
            RepoEntry {
                id: RepoId::new(),
                name: "notes".into(),
                path: PathBuf::from("notes"),
                group: Some("personal".into()),
                pinned: false,
                available: true,
                secret_key_id: None,
                secret_profile: None,
            },
        ];

        // Top-level rename cascades to subgroups, repos, collapsed state, and secret keys
        let renamed = rename_group_in(&mut cfg, "work", "company").unwrap();
        assert_eq!(renamed, "company");
        assert_eq!(
            cfg.groups,
            vec![
                "company".to_string(),
                "company/backend".to_string(),
                "company/backend/api".to_string(),
                "personal".to_string(),
            ]
        );
        assert_eq!(cfg.repos[0].group.as_deref(), Some("company/backend/api"));
        assert_eq!(cfg.repos[1].group.as_deref(), Some("personal"));
        assert_eq!(
            cfg.ui.collapsed_groups,
            vec!["company".to_string(), "company/backend".to_string()]
        );
        assert_eq!(
            cfg.group_secret_keys.get("company").map(String::as_str),
            Some("key-work")
        );
        assert_eq!(
            cfg.group_secret_keys
                .get("company/backend")
                .map(String::as_str),
            Some("key-be")
        );

        // Moving subgroup under a new parent creates ancestor group automatically
        let moved = rename_group_in(&mut cfg, "company/backend", "platform/core").unwrap();
        assert_eq!(moved, "platform/core");
        assert!(cfg.groups.contains(&"platform".to_string()));
        assert!(cfg.groups.contains(&"platform/core".to_string()));
        assert!(cfg.groups.contains(&"platform/core/api".to_string()));
        assert_eq!(cfg.repos[0].group.as_deref(), Some("platform/core/api"));

        // Rejects empty, self-descendant, and colliding group names
        assert!(rename_group_in(&mut cfg, "company", "   ").is_err());
        assert!(rename_group_in(&mut cfg, "company", "company/sub").is_err());
        assert!(rename_group_in(&mut cfg, "company", "personal").is_err());
    }
}
