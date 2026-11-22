use crate::SecurityService;
use crate::formats::normalize_repo_rel_path;
use crate::models::{DEFAULT_PROFILE, SecretFileFormat, SecretFileSpec, SecretsManifest};
use gitfleet_core::{AppError, AppResult};
use ignore::WalkBuilder;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub fn manifest_file_path(repo_path: &Path) -> PathBuf {
    repo_path.join(".gitfleet").join("secrets.json")
}

/// Validates and normalizes a Secrets Vault profile name into a safe filename-friendly slug
/// (`a-z`, `0-9`, `-`, `_`; spaces collapse to `-`).
pub fn normalize_profile_name(raw: &str) -> AppResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::Security("Profile name cannot be empty.".into()));
    }

    let mut out = String::with_capacity(trimmed.len());
    let mut prev_sep = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_sep = false;
        } else if ch == '-' || ch == '_' || ch.is_whitespace() {
            let mapped = if ch == '_' { '_' } else { '-' };
            if !out.is_empty() && !prev_sep {
                out.push(mapped);
                prev_sep = true;
            }
        } else {
            return Err(AppError::Security(format!(
                "Invalid profile name '{trimmed}'. Use letters, digits, '-' or '_' (e.g. laptop, work-pc, staging)."
            )));
        }
    }

    while out.ends_with('-') || out.ends_with('_') {
        out.pop();
    }

    if out.is_empty() {
        return Err(AppError::Security(format!(
            "Invalid profile name '{trimmed}'. Use letters, digits, '-' or '_'."
        )));
    }
    if out.len() > 40 {
        return Err(AppError::Security(
            "Profile name is too long (maximum 40 characters).".into(),
        ));
    }
    Ok(out)
}

pub(crate) fn resolve_profile_name(profile: Option<&str>) -> AppResult<String> {
    match profile.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(DEFAULT_PROFILE.to_string()),
        Some(p) => normalize_profile_name(p),
    }
}

/// Returns the `.gitfleet/` filename for a given normalized profile:
/// - `"default"` -> `"secrets.age"` (100% backwards-compatible)
/// - `"laptop"`  -> `"secrets.laptop.age"`
pub fn vault_file_name_for_profile(profile: &str) -> String {
    if profile.is_empty() || profile == DEFAULT_PROFILE {
        "secrets.age".to_string()
    } else {
        format!("secrets.{profile}.age")
    }
}

pub(crate) fn vault_file_path_for_profile(repo_path: &Path, profile: &str) -> PathBuf {
    repo_path
        .join(".gitfleet")
        .join(vault_file_name_for_profile(profile))
}

/// Discovers all profile names present in `.gitfleet/secrets.json` or `.gitfleet/secrets.*.age`.
/// Always puts `"default"` first, followed by sorted custom profiles.
pub(crate) fn discover_vault_profiles(repo_path: &Path, manifest: &SecretsManifest) -> Vec<String> {
    let mut custom: BTreeSet<String> = BTreeSet::new();
    for p in &manifest.profiles {
        if let Ok(norm) = normalize_profile_name(p)
            && norm != DEFAULT_PROFILE
        {
            custom.insert(norm);
        }
    }
    for p in manifest.profile_key_names.keys() {
        if let Ok(norm) = normalize_profile_name(p)
            && norm != DEFAULT_PROFILE
        {
            custom.insert(norm);
        }
    }
    let dir = repo_path.join(".gitfleet");
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|ft| ft.is_file()) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(mid) = name
                .strip_prefix("secrets.")
                .and_then(|s| s.strip_suffix(".age"))
                && !mid.is_empty()
                && let Ok(norm) = normalize_profile_name(mid)
                && norm != DEFAULT_PROFILE
            {
                custom.insert(norm);
            }
        }
    }
    let mut out = Vec::with_capacity(custom.len() + 1);
    out.push(DEFAULT_PROFILE.to_string());
    out.extend(custom);
    out
}

pub(crate) fn any_vault_file_exists(repo_path: &Path, manifest: &SecretsManifest) -> bool {
    discover_vault_profiles(repo_path, manifest)
        .iter()
        .any(|p| vault_file_path_for_profile(repo_path, p).exists())
}

impl SecurityService {
    pub fn list_vault_profiles(&self, repo_path: &Path) -> Vec<String> {
        let manifest = self.load_manifest(repo_path).unwrap_or_default();
        discover_vault_profiles(repo_path, &manifest)
    }

    pub fn load_manifest(&self, repo_path: &Path) -> AppResult<SecretsManifest> {
        let path = manifest_file_path(repo_path);
        if !path.exists() {
            return Ok(SecretsManifest::default());
        }
        let raw = std::fs::read_to_string(&path)?;
        let mut manifest: SecretsManifest = serde_json::from_str(&raw)
            .map_err(|e| AppError::Security(format!("Invalid .gitfleet/secrets.json: {e}")))?;

        let mut dedup: BTreeMap<String, SecretFileFormat> = BTreeMap::new();
        for item in manifest.files.drain(..) {
            if let Ok(norm) = normalize_repo_rel_path(repo_path, &item.path) {
                dedup.insert(norm, item.format);
            }
        }
        manifest.files = dedup
            .into_iter()
            .map(|(path, format)| SecretFileSpec { path, format })
            .collect();

        let mut norm_profiles: BTreeSet<String> = BTreeSet::new();
        for p in manifest.profiles.drain(..) {
            if let Ok(norm) = normalize_profile_name(&p)
                && norm != DEFAULT_PROFILE
            {
                norm_profiles.insert(norm);
            }
        }
        manifest.profiles = norm_profiles.into_iter().collect();

        let mut norm_key_names: BTreeMap<String, String> = BTreeMap::new();
        for (p, k) in std::mem::take(&mut manifest.profile_key_names) {
            let trimmed = k.trim();
            if !trimmed.is_empty()
                && let Ok(norm) = normalize_profile_name(&p)
                && norm != DEFAULT_PROFILE
            {
                norm_key_names.insert(norm, trimmed.to_string());
            }
        }
        manifest.profile_key_names = norm_key_names;
        Ok(manifest)
    }

    pub fn save_manifest(&self, repo_path: &Path, manifest: &SecretsManifest) -> AppResult<()> {
        let mut dedup: BTreeMap<String, SecretFileFormat> = BTreeMap::new();
        for item in &manifest.files {
            let norm = normalize_repo_rel_path(repo_path, &item.path)?;
            dedup.insert(norm, item.format);
        }
        let mut norm_profiles: BTreeSet<String> = BTreeSet::new();
        for p in &manifest.profiles {
            if let Ok(norm) = normalize_profile_name(p)
                && norm != DEFAULT_PROFILE
            {
                norm_profiles.insert(norm);
            }
        }
        let mut norm_key_names: BTreeMap<String, String> = BTreeMap::new();
        for (p, k) in &manifest.profile_key_names {
            let trimmed = k.trim();
            if !trimmed.is_empty()
                && let Ok(norm) = normalize_profile_name(p)
                && norm != DEFAULT_PROFILE
            {
                norm_key_names.insert(norm, trimmed.to_string());
            }
        }

        let normalized = SecretsManifest {
            version: 1,
            key_name: manifest
                .key_name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            profile_key_names: norm_key_names,
            profiles: norm_profiles.into_iter().collect(),
            files: dedup
                .into_iter()
                .map(|(path, format)| SecretFileSpec { path, format })
                .collect(),
        };
        let target = manifest_file_path(repo_path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut json = serde_json::to_string_pretty(&normalized)?;
        json.push('\n');
        std::fs::write(target, json)?;
        Ok(())
    }

    pub fn add_manifest_file(
        &self,
        repo_path: &Path,
        input_path: &str,
        format_override: Option<SecretFileFormat>,
    ) -> AppResult<SecretsManifest> {
        let rel = normalize_repo_rel_path(repo_path, input_path)?;
        let format = format_override
            .or_else(|| SecretFileFormat::detect_from_path(&rel))
            .ok_or_else(|| {
                AppError::Security(format!(
                    "Unsupported secret file '{rel}'. Supported formats: .env*, .toml, .yaml, .yml"
                ))
            })?;

        let mut manifest = self.load_manifest(repo_path)?;
        if let Some(existing) = manifest.files.iter_mut().find(|f| f.path == rel) {
            existing.format = format;
        } else {
            manifest.files.push(SecretFileSpec { path: rel, format });
        }
        manifest.files.sort_by(|a, b| a.path.cmp(&b.path));
        self.save_manifest(repo_path, &manifest)?;
        Ok(manifest)
    }

    pub fn remove_manifest_file(
        &self,
        repo_path: &Path,
        rel_path: &str,
        remove_from_vault: bool,
    ) -> AppResult<SecretsManifest> {
        let rel = normalize_repo_rel_path(repo_path, rel_path)?;
        let mut manifest = self.load_manifest(repo_path)?;
        manifest.files.retain(|f| f.path != rel);

        let profiles = discover_vault_profiles(repo_path, &manifest);
        if remove_from_vault {
            for prof in &profiles {
                let vpath = vault_file_path_for_profile(repo_path, prof);
                if vpath.exists()
                    && let Ok((mut payload, matched_key)) =
                        self.decrypt_payload_for_profile(repo_path, prof, None)
                {
                    let removed = payload.files.remove(&rel).is_some();
                    if payload.files.is_empty() && manifest.files.is_empty() {
                        let _ = std::fs::remove_file(&vpath);
                    } else if removed {
                        let target_id = matched_key.as_ref().map(|k| k.id.as_str());
                        let _ =
                            self.encrypt_payload_for_profile(repo_path, prof, &payload, target_id);
                    }
                }
            }
        }

        if manifest.files.is_empty() && !any_vault_file_exists(repo_path, &manifest) {
            let mpath = manifest_file_path(repo_path);
            if mpath.exists() {
                let _ = std::fs::remove_file(&mpath);
            }
            if let Some(dir) = mpath.parent() {
                let _ = std::fs::remove_dir(dir);
            }
            manifest.key_name = None;
            manifest.profile_key_names.clear();
            manifest.profiles.clear();
        } else {
            self.save_manifest(repo_path, &manifest)?;
        }

        Ok(manifest)
    }

    /// Completely removes this repository's Secrets Vault artifacts (`.gitfleet/secrets.age`,
    /// `.gitfleet/secrets.*.age`, and `.gitfleet/secrets.json`, plus `.gitfleet/` if empty).
    /// Works even when vaults are locked. Never modifies or deletes local plaintext secret files on disk.
    pub fn destroy_repo_vault(&self, repo_path: &Path) -> AppResult<usize> {
        let mut removed = 0usize;
        let dir = repo_path.join(".gitfleet");
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if !entry.file_type().is_ok_and(|ft| ft.is_file()) {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                let is_vault = name == "secrets.age"
                    || (name.starts_with("secrets.") && name.ends_with(".age"));
                if is_vault {
                    std::fs::remove_file(entry.path())?;
                    removed += 1;
                }
            }
        }
        let mpath = manifest_file_path(repo_path);
        if mpath.exists() {
            std::fs::remove_file(&mpath)?;
            removed += 1;
        }
        if dir.exists() {
            let _ = std::fs::remove_dir(&dir);
        }
        Ok(removed)
    }

    /// Discovers candidate `.env*`, `.toml`, `.yaml`, `.yml` files in the repo (including gitignored files)
    /// so the user can choose which ones to add to `.gitfleet/secrets.json`.
    pub fn discover_candidate_files(&self, repo_path: &Path) -> Vec<SecretFileSpec> {
        let mut out = Vec::new();
        let walker = WalkBuilder::new(repo_path)
            .hidden(false)
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .ignore(false)
            .max_depth(Some(4))
            .filter_entry(|e| {
                if e.depth() == 0 {
                    return true;
                }
                let name = e.file_name().to_string_lossy();
                if e.file_type().is_some_and(|ft| ft.is_dir()) {
                    return !matches!(
                        name.as_ref(),
                        ".git"
                            | ".gitfleet"
                            | "node_modules"
                            | "target"
                            | "dist"
                            | "build"
                            | "out"
                            | ".next"
                            | ".venv"
                            | "venv"
                            | "vendor"
                            | "__pycache__"
                    );
                }
                true
            })
            .build();

        for entry in walker.flatten() {
            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }
            let Ok(rel_os) = entry.path().strip_prefix(repo_path) else {
                continue;
            };
            let rel = rel_os.to_string_lossy().replace('\\', "/");
            let lower = rel.to_lowercase();
            let file_name = lower.rsplit('/').next().unwrap_or(&lower);
            if file_name.ends_with(".example")
                || file_name.ends_with(".sample")
                || file_name == "cargo.toml"
                || file_name == "cargo.lock"
                || file_name == "rust-toolchain.toml"
                || file_name == "pnpm-lock.yaml"
            {
                continue;
            }
            if let Some(format) = SecretFileFormat::detect_from_path(&rel) {
                out.push(SecretFileSpec { path: rel, format });
            }
            if out.len() >= 80 {
                break;
            }
        }

        out.sort_by(|a, b| {
            let score = |s: &SecretFileSpec| -> u8 {
                let l = s.path.to_lowercase();
                if s.format == SecretFileFormat::Env {
                    0
                } else if l.contains("secret") || l.contains("credential") || l.contains(".local") {
                    1
                } else if l.contains("config") || l.contains("env") || l.contains("prod") {
                    2
                } else {
                    3
                }
            };
            score(a).cmp(&score(b)).then_with(|| a.path.cmp(&b.path))
        });
        out
    }
}
