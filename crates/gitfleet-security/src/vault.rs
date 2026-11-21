use crate::SecurityService;
use crate::formats::{
    normalize_newlines, normalize_repo_rel_path, parse_secret_entries, rel_to_fs_path,
    update_secret_content, validate_secret_content,
};
use crate::manifest::manifest_file_path;
use crate::models::{
    SecretFileFormat, SecretFileSpec, VaultFileState, VaultFileSummary, VaultKeyInfo, VaultSummary,
};
use age::secrecy::ExposeSecret;
use arboard::Clipboard;
use directories::ProjectDirs;
use gitfleet_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zeroize::Zeroize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct VaultFileEntry {
    pub format: SecretFileFormat,
    pub raw: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct VaultPayloadV2 {
    pub version: u32,
    pub files: BTreeMap<String, VaultFileEntry>,
}

type LegacyVaultPayloadV1 = BTreeMap<String, BTreeMap<String, String>>;

fn default_identity_file_path() -> Option<PathBuf> {
    ProjectDirs::from("org", "gitfleet", "gitfleet")
        .map(|d| d.config_dir().join("secrets-identity.txt"))
}

pub(crate) fn vault_file_path(repo_path: &Path) -> PathBuf {
    repo_path.join(".gitfleet").join("secrets.age")
}

fn parse_age_identity_text(raw: &str) -> AppResult<age::x25519::Identity> {
    let line = raw
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("AGE-SECRET-KEY-"))
        .ok_or_else(|| {
            AppError::Security(
                "Invalid Age identity key. Expected a line starting with 'AGE-SECRET-KEY-'.".into(),
            )
        })?;
    line.parse::<age::x25519::Identity>()
        .map_err(|e| AppError::Security(format!("Failed parsing Age key: {e}")))
}

impl SecurityService {
    fn identity_file_path(&self) -> Option<PathBuf> {
        self.identity_override
            .clone()
            .or_else(default_identity_file_path)
    }

    fn load_identity(&self) -> AppResult<age::x25519::Identity> {
        let path = self
            .identity_file_path()
            .ok_or_else(|| AppError::Security("Cannot resolve config directory".into()))?;
        if !path.exists() {
            return Err(AppError::Security(
                "No Age identity key exists yet. Click 'Create Key' or 'Import Key' first.".into(),
            ));
        }
        let mut raw = std::fs::read_to_string(&path)?;
        let parsed = parse_age_identity_text(&raw);
        raw.zeroize();
        parsed
    }

    pub fn key_info(&self) -> VaultKeyInfo {
        let path = self.identity_file_path();
        if let Ok(id) = self.load_identity() {
            VaultKeyInfo {
                state: "ready".to_string(),
                public_key: Some(id.to_public().to_string()),
                key_path: path,
            }
        } else {
            VaultKeyInfo {
                state: "none".to_string(),
                public_key: None,
                key_path: path,
            }
        }
    }

    pub fn create_key(&self) -> AppResult<VaultKeyInfo> {
        let path = self
            .identity_file_path()
            .ok_or_else(|| AppError::Security("Cannot resolve config directory".into()))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let identity = age::x25519::Identity::generate();
        let pub_key = identity.to_public().to_string();
        let mut content = format!(
            "# GitFleet Age Identity Key\n# public key: {pub_key}\n{}\n",
            identity.to_string().expose_secret()
        );
        std::fs::write(&path, &content)?;
        content.zeroize();
        Ok(VaultKeyInfo {
            state: "ready".to_string(),
            public_key: Some(pub_key),
            key_path: Some(path),
        })
    }

    pub fn import_key(&self, raw_input: &str) -> AppResult<VaultKeyInfo> {
        let identity = parse_age_identity_text(raw_input)?;
        let path = self
            .identity_file_path()
            .ok_or_else(|| AppError::Security("Cannot resolve config directory".into()))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let pub_key = identity.to_public().to_string();
        let mut content = format!(
            "# GitFleet Age Identity Key (imported)\n# public key: {pub_key}\n{}\n",
            identity.to_string().expose_secret()
        );
        std::fs::write(&path, &content)?;
        content.zeroize();
        Ok(VaultKeyInfo {
            state: "ready".to_string(),
            public_key: Some(pub_key),
            key_path: Some(path),
        })
    }

    pub fn reset_key(&self) -> AppResult<()> {
        if let Some(path) = self.identity_file_path()
            && path.exists()
        {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    pub fn copy_public_key(&self) -> AppResult<String> {
        let info = self.key_info();
        let pk = info
            .public_key
            .ok_or_else(|| AppError::Security("No key created yet".into()))?;
        let mut cb =
            Clipboard::new().map_err(|e| AppError::Security(format!("Clipboard error: {e}")))?;
        cb.set_text(pk.clone())
            .map_err(|e| AppError::Security(format!("Clipboard error: {e}")))?;
        Ok(pk)
    }

    pub fn copy_private_key(&self) -> AppResult<()> {
        let identity = self.load_identity()?;
        let mut secret = identity.to_string().expose_secret().to_string();
        let mut cb =
            Clipboard::new().map_err(|e| AppError::Security(format!("Clipboard error: {e}")))?;
        let res = cb
            .set_text(secret.clone())
            .map_err(|e| AppError::Security(format!("Clipboard error: {e}")));
        secret.zeroize();
        res
    }

    pub(crate) fn encrypt_payload(&self, repo_path: &Path, payload: &VaultPayloadV2) -> AppResult<()> {
        let identity = self.load_identity()?;
        let recipient = identity.to_public();
        let mut plaintext = serde_json::to_vec_pretty(payload)?;

        let encryptor =
            age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
                .map_err(|e| AppError::Security(format!("Encryptor init failed: {e}")))?;
        let mut encrypted = Vec::new();
        let mut writer = encryptor
            .wrap_output(&mut encrypted)
            .map_err(|e| AppError::Security(format!("Age wrap failed: {e}")))?;
        writer.write_all(&plaintext)?;
        writer.finish()?;
        plaintext.zeroize();

        let target = vault_file_path(repo_path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(target, encrypted)?;
        Ok(())
    }

    pub(crate) fn decrypt_payload(&self, repo_path: &Path) -> AppResult<VaultPayloadV2> {
        let target = vault_file_path(repo_path);
        if !target.exists() {
            return Ok(VaultPayloadV2 {
                version: 2,
                files: BTreeMap::new(),
            });
        }
        let identity = self.load_identity()?;
        let ciphertext = std::fs::read(&target)?;
        let decryptor = age::Decryptor::new(&ciphertext[..])
            .map_err(|e| AppError::Security(format!("Age decryptor error: {e}")))?;
        let mut decrypted = Vec::new();
        let mut reader = decryptor
            .decrypt(std::iter::once(&identity as &dyn age::Identity))
            .map_err(|e| AppError::Security(format!("Age decrypt failed (wrong key?): {e}")))?;
        reader.read_to_end(&mut decrypted)?;

        let parsed = if let Ok(v2) = serde_json::from_slice::<VaultPayloadV2>(&decrypted)
            && v2.version >= 2
        {
            Ok(v2)
        } else if let Ok(v1) = serde_json::from_slice::<LegacyVaultPayloadV1>(&decrypted) {
            let mut files = BTreeMap::new();
            for (name, kvs) in v1 {
                let mut raw = String::new();
                for (k, v) in kvs {
                    raw.push_str(&format!("{k}={v}\n"));
                }
                let format =
                    SecretFileFormat::detect_from_path(&name).unwrap_or(SecretFileFormat::Env);
                files.insert(name, VaultFileEntry { format, raw });
            }
            Ok(VaultPayloadV2 { version: 2, files })
        } else {
            Err(AppError::Security(
                "Unrecognized encrypted vault format in .gitfleet/secrets.age".into(),
            ))
        };
        decrypted.zeroize();
        parsed
    }

    pub fn vault_summary(&self, repo_path: &Path) -> AppResult<VaultSummary> {
        let has_manifest = manifest_file_path(repo_path).exists();
        let has_vault = vault_file_path(repo_path).exists();
        let manifest = self.load_manifest(repo_path).unwrap_or_default();

        let (payload_opt, vault_locked, lock_error) = if has_vault {
            match self.decrypt_payload(repo_path) {
                Ok(p) => (Some(p), false, None),
                Err(e) => (None, true, Some(e.to_string())),
            }
        } else {
            (None, false, None)
        };

        let mut specs: BTreeMap<String, (SecretFileFormat, bool, bool)> = BTreeMap::new();
        for f in &manifest.files {
            specs.insert(f.path.clone(), (f.format, true, false));
        }
        if let Some(payload) = &payload_opt {
            for (path, entry) in &payload.files {
                let Ok(norm) = normalize_repo_rel_path(repo_path, path) else {
                    continue;
                };
                specs
                    .entry(norm)
                    .and_modify(|e| e.2 = true)
                    .or_insert((entry.format, false, true));
            }
        }

        let mut files = Vec::with_capacity(specs.len());
        for (path, (format, in_manifest, in_vault)) in specs {
            let disk_path = rel_to_fs_path(repo_path, &path);
            let disk_raw = std::fs::read_to_string(&disk_path).ok();

            let (state, keys) = if vault_locked {
                let disk_keys = disk_raw
                    .as_deref()
                    .and_then(|r| parse_secret_entries(format, r).ok())
                    .map(|m| m.into_keys().collect())
                    .unwrap_or_default();
                (VaultFileState::Locked, disk_keys)
            } else if let Some(vault_entry) = payload_opt.as_ref().and_then(|p| p.files.get(&path))
            {
                let keys = parse_secret_entries(vault_entry.format, &vault_entry.raw)
                    .map(|m| m.into_keys().collect())
                    .unwrap_or_default();
                let state = match &disk_raw {
                    None => VaultFileState::MissingOnDisk,
                    Some(d) if normalize_newlines(d) == normalize_newlines(&vault_entry.raw) => {
                        VaultFileState::Synced
                    }
                    Some(_) => VaultFileState::Modified,
                };
                (state, keys)
            } else {
                match &disk_raw {
                    Some(d) => {
                        let keys = parse_secret_entries(format, d)
                            .map(|m| m.into_keys().collect())
                            .unwrap_or_default();
                        (VaultFileState::NotEncrypted, keys)
                    }
                    None => (VaultFileState::MissingBoth, Vec::new()),
                }
            };

            files.push(VaultFileSummary {
                path,
                format,
                state,
                keys,
                in_manifest,
                in_vault,
            });
        }

        Ok(VaultSummary {
            has_manifest,
            has_vault,
            vault_locked,
            lock_error,
            files,
        })
    }

    pub fn encrypt_selected_files(
        &self,
        repo_path: &Path,
        only_paths: Option<&[String]>,
    ) -> AppResult<usize> {
        let mut manifest = self.load_manifest(repo_path)?;

        if manifest.files.is_empty() && only_paths.is_none() {
            if let Ok(entries) = std::fs::read_dir(repo_path) {
                for entry in entries.flatten() {
                    if !entry.path().is_file() {
                        continue;
                    }
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name == ".env"
                        || (name.starts_with(".env.")
                            && !name.ends_with(".example")
                            && !name.ends_with(".sample"))
                    {
                        manifest.files.push(SecretFileSpec {
                            path: name,
                            format: SecretFileFormat::Env,
                        });
                    }
                }
            }
            if !manifest.files.is_empty() {
                manifest.files.sort_by(|a, b| a.path.cmp(&b.path));
                self.save_manifest(repo_path, &manifest)?;
            }
        }

        let targets: Vec<SecretFileSpec> = if let Some(filter) = only_paths {
            let mut list = Vec::new();
            for p in filter {
                let norm = normalize_repo_rel_path(repo_path, p)?;
                let fmt = manifest
                    .files
                    .iter()
                    .find(|f| f.path == norm)
                    .map(|f| f.format)
                    .or_else(|| SecretFileFormat::detect_from_path(&norm))
                    .ok_or_else(|| {
                        AppError::Security(format!("Unsupported secret file format: '{norm}'"))
                    })?;
                if !manifest.files.iter().any(|f| f.path == norm) {
                    manifest.files.push(SecretFileSpec {
                        path: norm.clone(),
                        format: fmt,
                    });
                }
                list.push(SecretFileSpec {
                    path: norm,
                    format: fmt,
                });
            }
            self.save_manifest(repo_path, &manifest)?;
            list
        } else {
            manifest.files.clone()
        };

        if targets.is_empty() {
            return Err(AppError::Security(
                "No secret files selected yet. Click '+ Add File' to choose .env, .toml, or .yaml files."
                    .into(),
            ));
        }

        let mut payload = if vault_file_path(repo_path).exists() {
            self.decrypt_payload(repo_path)?
        } else {
            VaultPayloadV2 {
                version: 2,
                files: BTreeMap::new(),
            }
        };

        let mut encrypted_count = 0usize;
        for spec in &targets {
            let full = rel_to_fs_path(repo_path, &spec.path);
            if !full.is_file() {
                continue;
            }
            let raw = std::fs::read_to_string(&full)?;
            validate_secret_content(spec.format, &spec.path, &raw)?;
            payload.files.insert(
                spec.path.clone(),
                VaultFileEntry {
                    format: spec.format,
                    raw,
                },
            );
            encrypted_count += 1;
        }

        if encrypted_count == 0 {
            return Err(AppError::Security(
                "None of the selected secret files currently exist on disk to encrypt.".into(),
            ));
        }

        self.encrypt_payload(repo_path, &payload)?;
        Ok(encrypted_count)
    }

    pub fn restore_selected_files(
        &self,
        repo_path: &Path,
        only_paths: Option<&[String]>,
    ) -> AppResult<usize> {
        let payload = self.decrypt_payload(repo_path)?;
        if payload.files.is_empty() {
            return Err(AppError::Security(
                "No encrypted files found in .gitfleet/secrets.age.".into(),
            ));
        }

        let filter_set: Option<HashSet<String>> = only_paths
            .map(|list| {
                list.iter()
                    .map(|p| normalize_repo_rel_path(repo_path, p))
                    .collect::<AppResult<HashSet<_>>>()
            })
            .transpose()?;

        let mut restored = 0usize;
        let mut manifest = self.load_manifest(repo_path).unwrap_or_default();
        let mut manifest_changed = false;

        for (rel_raw, entry) in &payload.files {
            let norm = normalize_repo_rel_path(repo_path, rel_raw)?;
            if let Some(set) = &filter_set
                && !set.contains(&norm)
            {
                continue;
            }
            let target = rel_to_fs_path(repo_path, &norm);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&target, &entry.raw)?;
            restored += 1;

            if !manifest.files.iter().any(|f| f.path == norm) {
                manifest.files.push(SecretFileSpec {
                    path: norm,
                    format: entry.format,
                });
                manifest_changed = true;
            }
        }

        if manifest_changed && !manifest_file_path(repo_path).exists() {
            let _ = self.save_manifest(repo_path, &manifest);
        }

        Ok(restored)
    }

    pub fn encrypt_env_files(&self, repo_path: &Path) -> AppResult<usize> {
        self.encrypt_selected_files(repo_path, None)
    }

    pub fn restore_env_files(&self, repo_path: &Path) -> AppResult<usize> {
        self.restore_selected_files(repo_path, None)
    }

    pub fn reveal_secret(&self, repo_path: &Path, file: &str, key: &str) -> AppResult<String> {
        let norm = normalize_repo_rel_path(repo_path, file)?;
        let payload = self.decrypt_payload(repo_path)?;
        let entry = payload
            .files
            .get(&norm)
            .ok_or_else(|| AppError::Security(format!("Secret file '{norm}' not found in vault")))?;
        let entries = parse_secret_entries(entry.format, &entry.raw)?;
        entries
            .get(key)
            .cloned()
            .ok_or_else(|| AppError::Security(format!("Secret key '{key}' not found in '{norm}'")))
    }

    pub fn set_secret_value(
        &self,
        repo_path: &Path,
        file: &str,
        key: &str,
        value: &str,
    ) -> AppResult<()> {
        let norm = normalize_repo_rel_path(repo_path, file)?;
        let key = key.trim();
        if key.is_empty() {
            return Err(AppError::Security("Secret key cannot be empty".into()));
        }

        let mut manifest = self.load_manifest(repo_path)?;
        let format = manifest
            .files
            .iter()
            .find(|f| f.path == norm)
            .map(|f| f.format)
            .or_else(|| SecretFileFormat::detect_from_path(&norm))
            .ok_or_else(|| {
                AppError::Security(format!(
                    "Unsupported file '{norm}'. Use .env*, .toml, .yaml, or .yml"
                ))
            })?;

        let mut payload = if vault_file_path(repo_path).exists() {
            self.decrypt_payload(repo_path)?
        } else {
            VaultPayloadV2 {
                version: 2,
                files: BTreeMap::new(),
            }
        };

        let current_raw = payload
            .files
            .get(&norm)
            .map(|e| e.raw.clone())
            .or_else(|| std::fs::read_to_string(rel_to_fs_path(repo_path, &norm)).ok())
            .unwrap_or_default();

        let updated_raw = update_secret_content(format, &current_raw, key, value)?;
        payload.files.insert(
            norm.clone(),
            VaultFileEntry {
                format,
                raw: updated_raw,
            },
        );
        self.encrypt_payload(repo_path, &payload)?;

        if !manifest.files.iter().any(|f| f.path == norm) {
            manifest.files.push(SecretFileSpec { path: norm, format });
            self.save_manifest(repo_path, &manifest)?;
        }
        Ok(())
    }

    pub fn copy_secret_value(&self, repo_path: &Path, file: &str, key: &str) -> AppResult<()> {
        let mut val = self.reveal_secret(repo_path, file, key)?;
        let mut cb =
            Clipboard::new().map_err(|e| AppError::Security(format!("Clipboard error: {e}")))?;
        let res = cb
            .set_text(val.clone())
            .map_err(|e| AppError::Security(format!("Clipboard error: {e}")));
        val.zeroize();
        res
    }
}
