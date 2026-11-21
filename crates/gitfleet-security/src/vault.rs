use crate::SecurityService;
use crate::formats::{
    normalize_newlines, normalize_repo_rel_path, parse_secret_entries, rel_to_fs_path,
    update_secret_content, validate_secret_content,
};
use crate::manifest::manifest_file_path;
use crate::models::{
    SecretFileFormat, SecretFileSpec, VaultFileState, VaultFileSummary, VaultKeyEntry,
    VaultKeyInfo, VaultKeyringSummary, VaultSummary,
};
use age::secrecy::ExposeSecret;
use arboard::Clipboard;
use directories::ProjectDirs;
use gitfleet_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::Zeroize;

static KEY_ID_SEQ: AtomicU64 = AtomicU64::new(1);

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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredAgeKey {
    id: String,
    name: String,
    public_key: String,
    secret_key: String,
}

impl StoredAgeKey {
    fn to_entry(&self, default_id: Option<&str>) -> VaultKeyEntry {
        VaultKeyEntry {
            id: self.id.clone(),
            name: self.name.clone(),
            public_key: self.public_key.clone(),
            is_default: default_id == Some(self.id.as_str()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgeKeyringFile {
    #[serde(default = "default_keyring_version")]
    version: u32,
    #[serde(default)]
    default_key_id: Option<String>,
    #[serde(default)]
    keys: Vec<StoredAgeKey>,
}

fn default_keyring_version() -> u32 {
    1
}

impl Default for AgeKeyringFile {
    fn default() -> Self {
        Self {
            version: default_keyring_version(),
            default_key_id: None,
            keys: Vec::new(),
        }
    }
}

impl AgeKeyringFile {
    fn ensure_valid_default(&mut self) {
        let valid = self
            .default_key_id
            .as_deref()
            .is_some_and(|id| self.keys.iter().any(|k| k.id == id));
        if !valid {
            self.default_key_id = self.keys.first().map(|k| k.id.clone());
        }
    }

    fn resolve_key(&self, preferred_id: Option<&str>) -> Option<&StoredAgeKey> {
        if let Some(id) = preferred_id
            && let Some(found) = self.keys.iter().find(|k| k.id == id)
        {
            return Some(found);
        }
        if let Some(def_id) = self.default_key_id.as_deref()
            && let Some(found) = self.keys.iter().find(|k| k.id == def_id)
        {
            return Some(found);
        }
        self.keys.first()
    }
}

fn next_key_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = KEY_ID_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("key_{nanos:x}_{seq}")
}

fn default_keyring_file_path() -> Option<PathBuf> {
    ProjectDirs::from("org", "gitfleet", "gitfleet")
        .map(|d| d.config_dir().join("secrets-keyring.json"))
}

fn legacy_identity_file_path() -> Option<PathBuf> {
    ProjectDirs::from("org", "gitfleet", "gitfleet")
        .map(|d| d.config_dir().join("secrets-identity.txt"))
}

pub(crate) fn vault_file_path(repo_path: &Path) -> PathBuf {
    repo_path.join(".gitfleet").join("secrets.age")
}

fn parse_age_identity_text(raw: &str) -> AppResult<age::x25519::Identity> {
    if let Ok(kr) = serde_json::from_str::<AgeKeyringFile>(raw)
        && let Some(k) = kr.resolve_key(None)
    {
        return k
            .secret_key
            .trim()
            .parse::<age::x25519::Identity>()
            .map_err(|e| AppError::Security(format!("Failed parsing Age key: {e}")));
    }

    let extracted = raw.lines().find_map(|line| {
        let trimmed = line.trim();
        let start = trimmed.find("AGE-SECRET-KEY-")?;
        let rest = &trimmed[start..];
        let token: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        if token.len() > "AGE-SECRET-KEY-".len() {
            Some(token)
        } else {
            None
        }
    });

    let token = extracted.ok_or_else(|| {
        AppError::Security(
            "Invalid Age identity key. Expected a string starting with 'AGE-SECRET-KEY-'.".into(),
        )
    })?;
    token
        .parse::<age::x25519::Identity>()
        .map_err(|e| AppError::Security(format!("Failed parsing Age key: {e}")))
}

fn default_key_name_for(existing_count: usize, fallback_prefix: &str) -> String {
    if existing_count == 0 {
        fallback_prefix.to_string()
    } else {
        format!("{fallback_prefix} {}", existing_count + 1)
    }
}

impl SecurityService {
    fn keyring_file_path(&self) -> Option<PathBuf> {
        self.identity_override
            .clone()
            .or_else(default_keyring_file_path)
    }

    fn load_keyring(&self) -> AppResult<AgeKeyringFile> {
        let Some(path) = self.keyring_file_path() else {
            return Ok(AgeKeyringFile::default());
        };

        if path.exists() {
            let mut raw = std::fs::read_to_string(&path)?;
            if let Ok(mut kr) = serde_json::from_str::<AgeKeyringFile>(&raw) {
                raw.zeroize();
                kr.ensure_valid_default();
                return Ok(kr);
            }
            // Legacy plain-text Age file at this exact path (e.g. test override or old file)
            let parsed = parse_age_identity_text(&raw);
            raw.zeroize();
            if let Ok(identity) = parsed {
                let id = next_key_id();
                let mut kr = AgeKeyringFile {
                    version: 1,
                    default_key_id: Some(id.clone()),
                    keys: vec![StoredAgeKey {
                        id,
                        name: "Default".to_string(),
                        public_key: identity.to_public().to_string(),
                        secret_key: identity.to_string().expose_secret().to_string(),
                    }],
                };
                kr.ensure_valid_default();
                return Ok(kr);
            }
            return Ok(AgeKeyringFile::default());
        }

        // Lossless one-time migration from legacy single-key `secrets-identity.txt`
        if self.identity_override.is_none()
            && let Some(legacy_path) = legacy_identity_file_path()
            && legacy_path.exists()
            && let Ok(mut raw) = std::fs::read_to_string(&legacy_path)
        {
            let parsed = parse_age_identity_text(&raw);
            raw.zeroize();
            if let Ok(identity) = parsed {
                let id = next_key_id();
                let kr = AgeKeyringFile {
                    version: 1,
                    default_key_id: Some(id.clone()),
                    keys: vec![StoredAgeKey {
                        id,
                        name: "Default".to_string(),
                        public_key: identity.to_public().to_string(),
                        secret_key: identity.to_string().expose_secret().to_string(),
                    }],
                };
                let _ = self.save_keyring(&kr);
                return Ok(kr);
            }
        }

        Ok(AgeKeyringFile::default())
    }

    fn save_keyring(&self, keyring: &AgeKeyringFile) -> AppResult<()> {
        let path = self
            .keyring_file_path()
            .ok_or_else(|| AppError::Security("Cannot resolve config directory".into()))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut json = serde_json::to_string_pretty(keyring)?;
        json.push('\n');
        std::fs::write(&path, &json)?;
        json.zeroize();
        Ok(())
    }

    pub fn keyring_summary(&self) -> VaultKeyringSummary {
        let path = self.keyring_file_path();
        let kr = self.load_keyring().unwrap_or_default();
        let def_id = kr.default_key_id.as_deref();
        let keys = kr.keys.iter().map(|k| k.to_entry(def_id)).collect();
        VaultKeyringSummary {
            keys,
            default_key_id: kr.default_key_id,
            keyring_path: path,
        }
    }

    pub fn key_info(&self) -> VaultKeyInfo {
        let summary = self.keyring_summary();
        if let Some(def) = summary.default_key() {
            VaultKeyInfo {
                state: "ready".to_string(),
                public_key: Some(def.public_key.clone()),
                key_path: summary.keyring_path,
            }
        } else {
            VaultKeyInfo {
                state: "none".to_string(),
                public_key: None,
                key_path: summary.keyring_path,
            }
        }
    }

    pub fn create_named_key(&self, name: &str) -> AppResult<VaultKeyEntry> {
        let mut kr = self.load_keyring()?;
        let trimmed = name.trim();
        let final_name = if trimmed.is_empty() {
            default_key_name_for(kr.keys.len(), "Default")
        } else {
            trimmed.to_string()
        };

        let identity = age::x25519::Identity::generate();
        let pub_key = identity.to_public().to_string();
        let id = next_key_id();
        let stored = StoredAgeKey {
            id: id.clone(),
            name: final_name,
            public_key: pub_key,
            secret_key: identity.to_string().expose_secret().to_string(),
        };
        if kr.default_key_id.is_none() {
            kr.default_key_id = Some(id.clone());
        }
        kr.keys.push(stored);
        kr.ensure_valid_default();
        self.save_keyring(&kr)?;

        let def_id = kr.default_key_id.as_deref();
        kr.keys
            .iter()
            .find(|k| k.id == id)
            .map(|k| k.to_entry(def_id))
            .ok_or_else(|| AppError::Security("Created key missing from keyring".into()))
    }

    pub fn create_key(&self) -> AppResult<VaultKeyInfo> {
        let entry = self.create_named_key("")?;
        Ok(VaultKeyInfo {
            state: "ready".to_string(),
            public_key: Some(entry.public_key),
            key_path: self.keyring_file_path(),
        })
    }

    pub fn import_named_key(&self, name: &str, raw_input: &str) -> AppResult<VaultKeyEntry> {
        let identity = parse_age_identity_text(raw_input)?;
        let pub_key = identity.to_public().to_string();
        let trimmed = name.trim();

        let mut kr = self.load_keyring()?;
        if let Some(existing) = kr.keys.iter_mut().find(|k| k.public_key == pub_key) {
            if !trimmed.is_empty() {
                existing.name = trimmed.to_string();
            }
            let existing_id = existing.id.clone();
            kr.ensure_valid_default();
            self.save_keyring(&kr)?;
            let def_id = kr.default_key_id.as_deref();
            return kr
                .keys
                .iter()
                .find(|k| k.id == existing_id)
                .map(|k| k.to_entry(def_id))
                .ok_or_else(|| AppError::Security("Imported key missing from keyring".into()));
        }

        let final_name = if trimmed.is_empty() {
            default_key_name_for(kr.keys.len(), "Imported Key")
        } else {
            trimmed.to_string()
        };
        let id = next_key_id();
        let stored = StoredAgeKey {
            id: id.clone(),
            name: final_name,
            public_key: pub_key,
            secret_key: identity.to_string().expose_secret().to_string(),
        };
        if kr.default_key_id.is_none() {
            kr.default_key_id = Some(id.clone());
        }
        kr.keys.push(stored);
        kr.ensure_valid_default();
        self.save_keyring(&kr)?;

        let def_id = kr.default_key_id.as_deref();
        kr.keys
            .iter()
            .find(|k| k.id == id)
            .map(|k| k.to_entry(def_id))
            .ok_or_else(|| AppError::Security("Imported key missing from keyring".into()))
    }

    pub fn import_key(&self, raw_input: &str) -> AppResult<VaultKeyInfo> {
        let entry = self.import_named_key("", raw_input)?;
        Ok(VaultKeyInfo {
            state: "ready".to_string(),
            public_key: Some(entry.public_key),
            key_path: self.keyring_file_path(),
        })
    }

    pub fn rename_key(&self, key_id: &str, new_name: &str) -> AppResult<()> {
        let trimmed = new_name.trim();
        if trimmed.is_empty() {
            return Err(AppError::Security("Key name cannot be empty.".into()));
        }
        let mut kr = self.load_keyring()?;
        let Some(k) = kr.keys.iter_mut().find(|k| k.id == key_id) else {
            return Err(AppError::Security("Age key not found in keyring.".into()));
        };
        k.name = trimmed.to_string();
        self.save_keyring(&kr)
    }

    pub fn set_default_key(&self, key_id: &str) -> AppResult<()> {
        let mut kr = self.load_keyring()?;
        if !kr.keys.iter().any(|k| k.id == key_id) {
            return Err(AppError::Security("Age key not found in keyring.".into()));
        }
        kr.default_key_id = Some(key_id.to_string());
        self.save_keyring(&kr)
    }

    pub fn delete_key(&self, key_id: &str) -> AppResult<()> {
        let mut kr = self.load_keyring()?;
        let before = kr.keys.len();
        kr.keys.retain(|k| k.id != key_id);
        if kr.keys.len() == before {
            return Err(AppError::Security("Age key not found in keyring.".into()));
        }
        kr.ensure_valid_default();
        self.save_keyring(&kr)
    }

    pub fn reset_key(&self) -> AppResult<()> {
        if let Some(path) = self.keyring_file_path()
            && path.exists()
        {
            std::fs::remove_file(path)?;
        }
        if self.identity_override.is_none()
            && let Some(legacy) = legacy_identity_file_path()
            && legacy.exists()
        {
            let _ = std::fs::remove_file(legacy);
        }
        Ok(())
    }

    pub fn copy_key_public(&self, key_id: &str) -> AppResult<String> {
        let kr = self.load_keyring()?;
        let k = kr
            .keys
            .iter()
            .find(|k| k.id == key_id)
            .ok_or_else(|| AppError::Security("Age key not found in keyring.".into()))?;
        let mut cb =
            Clipboard::new().map_err(|e| AppError::Security(format!("Clipboard error: {e}")))?;
        cb.set_text(k.public_key.clone())
            .map_err(|e| AppError::Security(format!("Clipboard error: {e}")))?;
        Ok(k.public_key.clone())
    }

    pub fn copy_key_private(&self, key_id: &str) -> AppResult<()> {
        let kr = self.load_keyring()?;
        let k = kr
            .keys
            .iter()
            .find(|k| k.id == key_id)
            .ok_or_else(|| AppError::Security("Age key not found in keyring.".into()))?;
        let mut secret = k.secret_key.clone();
        let mut cb =
            Clipboard::new().map_err(|e| AppError::Security(format!("Clipboard error: {e}")))?;
        let res = cb
            .set_text(secret.clone())
            .map_err(|e| AppError::Security(format!("Clipboard error: {e}")));
        secret.zeroize();
        res
    }

    pub fn copy_public_key(&self) -> AppResult<String> {
        let kr = self.load_keyring()?;
        let k = kr
            .resolve_key(None)
            .ok_or_else(|| AppError::Security("No key created yet".into()))?;
        self.copy_key_public(&k.id)
    }

    pub fn copy_private_key(&self) -> AppResult<()> {
        let kr = self.load_keyring()?;
        let k = kr
            .resolve_key(None)
            .ok_or_else(|| AppError::Security("No key created yet".into()))?;
        self.copy_key_private(&k.id)
    }

    pub(crate) fn encrypt_payload_with_key(
        &self,
        repo_path: &Path,
        payload: &VaultPayloadV2,
        target_key_id: Option<&str>,
    ) -> AppResult<VaultKeyEntry> {
        let kr = self.load_keyring()?;
        let chosen = kr.resolve_key(target_key_id).ok_or_else(|| {
            AppError::Security(
                "No Age key exists yet. Create or import a key in Secrets → Keys first.".into(),
            )
        })?;
        let identity = chosen
            .secret_key
            .trim()
            .parse::<age::x25519::Identity>()
            .map_err(|e| AppError::Security(format!("Stored Age key invalid: {e}")))?;
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
        Ok(chosen.to_entry(kr.default_key_id.as_deref()))
    }

    pub(crate) fn decrypt_payload_with_key(
        &self,
        repo_path: &Path,
        preferred_key_id: Option<&str>,
    ) -> AppResult<(VaultPayloadV2, Option<VaultKeyEntry>)> {
        let target = vault_file_path(repo_path);
        if !target.exists() {
            return Ok((
                VaultPayloadV2 {
                    version: 2,
                    files: BTreeMap::new(),
                },
                None,
            ));
        }

        let kr = self.load_keyring()?;
        let manifest_hint = self
            .load_manifest(repo_path)
            .ok()
            .and_then(|m| m.key_name);
        if kr.keys.is_empty() {
            let msg = match manifest_hint {
                Some(name) => format!(
                    "No Age key in keyring (repo expects key '{name}'). Import or create a key first."
                ),
                None => "No Age key in keyring yet. Click 'Keys' to create or import a key.".into(),
            };
            return Err(AppError::Security(msg));
        }

        let ciphertext = std::fs::read(&target)?;
        // Order candidates: preferred key first, then default key, then all remaining keys
        let mut ordered: Vec<&StoredAgeKey> = Vec::with_capacity(kr.keys.len());
        if let Some(pid) = preferred_key_id
            && let Some(k) = kr.keys.iter().find(|k| k.id == pid)
        {
            ordered.push(k);
        }
        if let Some(did) = kr.default_key_id.as_deref()
            && let Some(k) = kr.keys.iter().find(|k| k.id == did)
            && !ordered.iter().any(|o| o.id == k.id)
        {
            ordered.push(k);
        }
        for k in &kr.keys {
            if !ordered.iter().any(|o| o.id == k.id) {
                ordered.push(k);
            }
        }

        for candidate in ordered {
            let Ok(identity) = candidate.secret_key.trim().parse::<age::x25519::Identity>() else {
                continue;
            };
            let decryptor = age::Decryptor::new(&ciphertext[..])
                .map_err(|e| AppError::Security(format!("Age decryptor error: {e}")))?;
            let Ok(mut reader) = decryptor.decrypt(std::iter::once(&identity as &dyn age::Identity))
            else {
                continue;
            };
            let mut decrypted = Vec::new();
            reader.read_to_end(&mut decrypted)?;
            let parsed = parse_vault_payload(&decrypted);
            decrypted.zeroize();
            return Ok((parsed?, Some(candidate.to_entry(kr.default_key_id.as_deref()))));
        }

        let msg = match manifest_hint {
            Some(name) => format!(
                "No matching key in keyring (repo expects key '{name}'). Import that Age secret key."
            ),
            None => "Age decrypt failed — none of the keys in your keyring match this vault.".into(),
        };
        Err(AppError::Security(msg))
    }

    pub fn vault_summary_with_key(
        &self,
        repo_path: &Path,
        preferred_key_id: Option<&str>,
    ) -> AppResult<VaultSummary> {
        let has_manifest = manifest_file_path(repo_path).exists();
        let has_vault = vault_file_path(repo_path).exists();
        let manifest = self.load_manifest(repo_path).unwrap_or_default();
        let manifest_key_name = manifest.key_name.clone();

        let (payload_opt, unlocked_key, vault_locked, lock_error) = if has_vault {
            match self.decrypt_payload_with_key(repo_path, preferred_key_id) {
                Ok((p, matched)) => (Some(p), matched, false, None),
                Err(e) => {
                    let msg = match e {
                        AppError::Security(s) => s,
                        other => other.to_string(),
                    };
                    (None, None, true, Some(msg))
                }
            }
        } else {
            (None, None, false, None)
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
            unlocked_by_key_id: unlocked_key.as_ref().map(|k| k.id.clone()),
            unlocked_by_key_name: unlocked_key.map(|k| k.name),
            manifest_key_name,
            files,
        })
    }

    pub fn vault_summary(&self, repo_path: &Path) -> AppResult<VaultSummary> {
        self.vault_summary_with_key(repo_path, None)
    }

    pub fn encrypt_selected_files_with_key(
        &self,
        repo_path: &Path,
        only_paths: Option<&[String]>,
        preferred_key_id: Option<&str>,
    ) -> AppResult<(usize, VaultKeyEntry)> {
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
            list
        } else {
            manifest.files.clone()
        };

        if targets.is_empty() {
            return Err(AppError::Security(
                "No secret files selected yet. Click 'Choose Files' to pick .env, .toml, or .yaml files."
                    .into(),
            ));
        }

        let (mut payload, matched_existing) = if vault_file_path(repo_path).exists() {
            self.decrypt_payload_with_key(repo_path, preferred_key_id)?
        } else {
            (
                VaultPayloadV2 {
                    version: 2,
                    files: BTreeMap::new(),
                },
                None,
            )
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

        // Priority: explicit repo/group assignment > key that already unlocked existing vault > default key
        let effective_key_id = preferred_key_id
            .or_else(|| matched_existing.as_ref().map(|k| k.id.as_str()));
        let used_key = self.encrypt_payload_with_key(repo_path, &payload, effective_key_id)?;

        manifest.key_name = Some(used_key.name.clone());
        self.save_manifest(repo_path, &manifest)?;
        Ok((encrypted_count, used_key))
    }

    pub fn encrypt_selected_files(
        &self,
        repo_path: &Path,
        only_paths: Option<&[String]>,
    ) -> AppResult<usize> {
        self.encrypt_selected_files_with_key(repo_path, only_paths, None)
            .map(|(n, _)| n)
    }

    /// Re-encrypts an existing `.gitfleet/secrets.age` vault under a different key from the local
    /// keyring without requiring the plaintext files to exist on disk.
    pub fn rekey_vault(&self, repo_path: &Path, target_key_id: &str) -> AppResult<VaultKeyEntry> {
        if !vault_file_path(repo_path).exists() {
            return Err(AppError::Security(
                "No .gitfleet/secrets.age vault exists in this repository yet.".into(),
            ));
        }
        let (payload, _) = self.decrypt_payload_with_key(repo_path, None)?;
        let used_key = self.encrypt_payload_with_key(repo_path, &payload, Some(target_key_id))?;
        if manifest_file_path(repo_path).exists()
            && let Ok(mut manifest) = self.load_manifest(repo_path)
        {
            manifest.key_name = Some(used_key.name.clone());
            let _ = self.save_manifest(repo_path, &manifest);
        }
        Ok(used_key)
    }

    pub fn restore_selected_files_with_key(
        &self,
        repo_path: &Path,
        only_paths: Option<&[String]>,
        preferred_key_id: Option<&str>,
    ) -> AppResult<usize> {
        let (payload, _) = self.decrypt_payload_with_key(repo_path, preferred_key_id)?;
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

    pub fn restore_selected_files(
        &self,
        repo_path: &Path,
        only_paths: Option<&[String]>,
    ) -> AppResult<usize> {
        self.restore_selected_files_with_key(repo_path, only_paths, None)
    }

    pub fn encrypt_env_files(&self, repo_path: &Path) -> AppResult<usize> {
        self.encrypt_selected_files(repo_path, None)
    }

    pub fn restore_env_files(&self, repo_path: &Path) -> AppResult<usize> {
        self.restore_selected_files(repo_path, None)
    }

    pub fn reveal_secret(&self, repo_path: &Path, file: &str, key: &str) -> AppResult<String> {
        let norm = normalize_repo_rel_path(repo_path, file)?;
        let (payload, _) = self.decrypt_payload_with_key(repo_path, None)?;
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

    pub fn set_secret_value_with_key(
        &self,
        repo_path: &Path,
        file: &str,
        key: &str,
        value: &str,
        preferred_key_id: Option<&str>,
    ) -> AppResult<VaultKeyEntry> {
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

        let (mut payload, matched_existing) = if vault_file_path(repo_path).exists() {
            self.decrypt_payload_with_key(repo_path, preferred_key_id)?
        } else {
            (
                VaultPayloadV2 {
                    version: 2,
                    files: BTreeMap::new(),
                },
                None,
            )
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

        let effective_key_id = preferred_key_id
            .or_else(|| matched_existing.as_ref().map(|k| k.id.as_str()));
        let used_key = self.encrypt_payload_with_key(repo_path, &payload, effective_key_id)?;

        if !manifest.files.iter().any(|f| f.path == norm) {
            manifest.files.push(SecretFileSpec { path: norm, format });
        }
        manifest.key_name = Some(used_key.name.clone());
        self.save_manifest(repo_path, &manifest)?;
        Ok(used_key)
    }

    pub fn set_secret_value(
        &self,
        repo_path: &Path,
        file: &str,
        key: &str,
        value: &str,
    ) -> AppResult<()> {
        self.set_secret_value_with_key(repo_path, file, key, value, None)
            .map(|_| ())
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

fn parse_vault_payload(decrypted: &[u8]) -> AppResult<VaultPayloadV2> {
    if let Ok(v2) = serde_json::from_slice::<VaultPayloadV2>(decrypted)
        && v2.version >= 2
    {
        Ok(v2)
    } else if let Ok(v1) = serde_json::from_slice::<LegacyVaultPayloadV1>(decrypted) {
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
    }
}
