use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Canonical base profile stored in `.gitfleet/secrets.age` (100% backwards-compatible).
pub const DEFAULT_PROFILE: &str = "default";

fn default_profile_name() -> String {
    DEFAULT_PROFILE.to_string()
}

fn default_vault_file_name() -> String {
    "secrets.age".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretFinding {
    pub id: String,
    pub file_path: String,
    pub line: usize,
    pub rule: String,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OversizedFile {
    pub file_path: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IgnoreLayer {
    RootGitignore,
    PersonalExclude,
    GlobalExclude,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IgnoreSnapshot {
    pub root_gitignore: String,
    pub personal_exclude: String,
    pub global_exclude_path: Option<PathBuf>,
    pub global_exclude: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VaultKeyInfo {
    pub state: String, // "none" | "ready"
    pub public_key: Option<String>,
    pub key_path: Option<PathBuf>,
}

/// Public metadata for one named Age key stored in the local keyring (never includes secret bytes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultKeyEntry {
    pub id: String,
    pub name: String,
    pub public_key: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VaultKeyringSummary {
    pub keys: Vec<VaultKeyEntry>,
    pub default_key_id: Option<String>,
    pub keyring_path: Option<PathBuf>,
}

impl VaultKeyringSummary {
    pub fn find(&self, key_id: &str) -> Option<&VaultKeyEntry> {
        self.keys.iter().find(|k| k.id == key_id)
    }

    pub fn default_key(&self) -> Option<&VaultKeyEntry> {
        self.default_key_id
            .as_deref()
            .and_then(|id| self.find(id))
            .or_else(|| self.keys.first())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SecretFileFormat {
    Env,
    Toml,
    Yaml,
}

impl SecretFileFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Env => "ENV",
            Self::Toml => "TOML",
            Self::Yaml => "YAML",
        }
    }

    pub fn detect_from_path(rel_path: &str) -> Option<Self> {
        let lower = rel_path.to_lowercase();
        if lower.ends_with(".toml") {
            return Some(Self::Toml);
        }
        if lower.ends_with(".yaml") || lower.ends_with(".yml") {
            return Some(Self::Yaml);
        }
        let file_name = lower.rsplit('/').next().unwrap_or(&lower);
        if file_name == ".env" || file_name.starts_with(".env.") || file_name.ends_with(".env") {
            return Some(Self::Env);
        }
        None
    }
}

/// One user-selected secret file persisted in `.gitfleet/secrets.json` (committed with the repo).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretFileSpec {
    /// Repo-relative POSIX path (e.g. `.env`, `config/secrets.toml`, `deploy/prod.yaml`).
    pub path: String,
    pub format: SecretFileFormat,
}

/// Repo-committed manifest (`.gitfleet/secrets.json`) so every machine cloning the repo
/// automatically discovers which files and profiles are managed by the encrypted vault.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretsManifest {
    #[serde(default = "default_manifest_version")]
    pub version: u32,
    /// Optional human-readable name of the Age key last used to encrypt `.gitfleet/secrets.age`
    /// (non-secret hint shown if another machine lacks the matching private key).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_name: Option<String>,
    /// Optional per-profile Age key name hints (`profile -> key_name`) for non-default profiles.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub profile_key_names: BTreeMap<String, String>,
    /// Optional declared custom profiles beyond `"default"` (e.g. `["laptop", "staging"]`).
    /// Profiles are also auto-discovered from `.gitfleet/secrets.<profile>.age`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<String>,
    #[serde(default)]
    pub files: Vec<SecretFileSpec>,
}

fn default_manifest_version() -> u32 {
    1
}

impl Default for SecretsManifest {
    fn default() -> Self {
        Self {
            version: default_manifest_version(),
            key_name: None,
            profile_key_names: BTreeMap::new(),
            profiles: Vec::new(),
            files: Vec::new(),
        }
    }
}

impl SecretsManifest {
    pub fn key_name_for_profile(&self, profile: &str) -> Option<&str> {
        if profile == DEFAULT_PROFILE {
            self.key_name.as_deref()
        } else {
            self.profile_key_names
                .get(profile)
                .map(String::as_str)
                .or(self.key_name.as_deref())
        }
    }

    pub fn set_key_name_for_profile(&mut self, profile: &str, name: Option<String>) {
        let cleaned = name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        if profile == DEFAULT_PROFILE {
            self.key_name = cleaned;
        } else if let Some(val) = cleaned {
            self.profile_key_names.insert(profile.to_string(), val);
        } else {
            self.profile_key_names.remove(profile);
        }
    }
}

/// Summary metadata for one Secrets Vault profile (e.g. `default`, `laptop`, `staging`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultProfileSummary {
    pub name: String,
    pub has_vault: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VaultFileState {
    Synced,
    Modified,
    MissingOnDisk,
    NotEncrypted,
    Locked,
    MissingBoth,
}

impl VaultFileState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Synced => "Synced",
            Self::Modified => "Modified",
            Self::MissingOnDisk => "Missing on disk",
            Self::NotEncrypted => "Not encrypted",
            Self::Locked => "Vault locked",
            Self::MissingBoth => "Not found",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultFileSummary {
    pub path: String,
    pub format: SecretFileFormat,
    pub state: VaultFileState,
    pub keys: Vec<String>,
    pub in_manifest: bool,
    pub in_vault: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultSummary {
    pub has_manifest: bool,
    pub has_vault: bool,
    pub vault_locked: bool,
    pub lock_error: Option<String>,
    /// Active profile inspected in this summary (`"default"`, `"laptop"`, etc.).
    #[serde(default = "default_profile_name")]
    pub active_profile: String,
    /// Filename inside `.gitfleet/` for the active profile (`secrets.age` or `secrets.<profile>.age`).
    #[serde(default = "default_vault_file_name")]
    pub vault_file_name: String,
    /// All known profiles for this repo (`"default"` always first, followed by sorted custom profiles).
    #[serde(default)]
    pub profiles: Vec<VaultProfileSummary>,
    /// Key ID from the local keyring that unlocked the active profile vault (if unlocked).
    #[serde(default)]
    pub unlocked_by_key_id: Option<String>,
    /// Key name from the local keyring that unlocked the active profile vault (if unlocked).
    #[serde(default)]
    pub unlocked_by_key_name: Option<String>,
    /// Key name hint stored in `.gitfleet/secrets.json` for the active profile (if any).
    #[serde(default)]
    pub manifest_key_name: Option<String>,
    pub files: Vec<VaultFileSummary>,
}

impl Default for VaultSummary {
    fn default() -> Self {
        Self {
            has_manifest: false,
            has_vault: false,
            vault_locked: false,
            lock_error: None,
            active_profile: default_profile_name(),
            vault_file_name: default_vault_file_name(),
            profiles: vec![VaultProfileSummary {
                name: DEFAULT_PROFILE.to_string(),
                has_vault: false,
                key_name: None,
            }],
            unlocked_by_key_id: None,
            unlocked_by_key_name: None,
            manifest_key_name: None,
            files: Vec::new(),
        }
    }
}

impl VaultSummary {
    pub fn is_default_profile(&self) -> bool {
        self.active_profile.is_empty() || self.active_profile == DEFAULT_PROFILE
    }
    /// Number of tracked secret files on disk that have unencrypted changes or are not yet sealed.
    pub fn encrypt_needed_count(&self) -> usize {
        self.files
            .iter()
            .filter(|f| {
                matches!(
                    f.state,
                    VaultFileState::Modified | VaultFileState::NotEncrypted
                )
            })
            .count()
    }

    /// Number of encrypted secret files missing from the local working tree that can be restored.
    pub fn restore_needed_count(&self) -> usize {
        self.files
            .iter()
            .filter(|f| f.state == VaultFileState::MissingOnDisk)
            .count()
    }

    pub fn encrypt_needed_paths(&self) -> Vec<&str> {
        self.files
            .iter()
            .filter(|f| {
                matches!(
                    f.state,
                    VaultFileState::Modified | VaultFileState::NotEncrypted
                )
            })
            .map(|f| f.path.as_str())
            .collect()
    }

    pub fn restore_needed_paths(&self) -> Vec<&str> {
        self.files
            .iter()
            .filter(|f| f.state == VaultFileState::MissingOnDisk)
            .map(|f| f.path.as_str())
            .collect()
    }
}
