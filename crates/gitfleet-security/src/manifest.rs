use crate::SecurityService;
use crate::formats::normalize_repo_rel_path;
use crate::models::{SecretFileFormat, SecretFileSpec, SecretsManifest};
use crate::vault::vault_file_path;
use gitfleet_core::{AppError, AppResult};
use ignore::WalkBuilder;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn manifest_file_path(repo_path: &Path) -> PathBuf {
    repo_path.join(".gitfleet").join("secrets.json")
}

impl SecurityService {
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
        Ok(manifest)
    }

    pub fn save_manifest(&self, repo_path: &Path, manifest: &SecretsManifest) -> AppResult<()> {
        let mut dedup: BTreeMap<String, SecretFileFormat> = BTreeMap::new();
        for item in &manifest.files {
            let norm = normalize_repo_rel_path(repo_path, &item.path)?;
            dedup.insert(norm, item.format);
        }
        let normalized = SecretsManifest {
            version: 1,
            key_name: manifest
                .key_name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
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

        let vault_path = vault_file_path(repo_path);
        if remove_from_vault
            && vault_path.exists()
            && let Ok((mut payload, matched_key)) = self.decrypt_payload_with_key(repo_path, None)
        {
            let removed = payload.files.remove(&rel).is_some();
            if payload.files.is_empty() && manifest.files.is_empty() {
                let _ = std::fs::remove_file(&vault_path);
            } else if removed {
                let target_id = matched_key.as_ref().map(|k| k.id.as_str());
                let _ = self.encrypt_payload_with_key(repo_path, &payload, target_id);
            }
        }

        if manifest.files.is_empty() && !vault_path.exists() {
            let mpath = manifest_file_path(repo_path);
            if mpath.exists() {
                let _ = std::fs::remove_file(&mpath);
            }
            if let Some(dir) = mpath.parent() {
                let _ = std::fs::remove_dir(dir);
            }
            manifest.key_name = None;
        } else {
            self.save_manifest(repo_path, &manifest)?;
        }

        Ok(manifest)
    }

    /// Completely removes this repository's Secrets Vault artifacts (`.gitfleet/secrets.age` and
    /// `.gitfleet/secrets.json`, plus `.gitfleet/` if empty). Works even when the vault is locked.
    /// Never modifies or deletes local plaintext secret files (`.env`, `.toml`, `.yaml`) on disk.
    pub fn destroy_repo_vault(&self, repo_path: &Path) -> AppResult<usize> {
        let mut removed = 0usize;
        let vault_path = vault_file_path(repo_path);
        if vault_path.exists() {
            std::fs::remove_file(&vault_path)?;
            removed += 1;
        }
        let mpath = manifest_file_path(repo_path);
        if mpath.exists() {
            std::fs::remove_file(&mpath)?;
            removed += 1;
        }
        if let Some(dir) = mpath.parent()
            && dir.exists()
        {
            let _ = std::fs::remove_dir(dir);
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
