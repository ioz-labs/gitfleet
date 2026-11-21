mod formats;
mod ignore_layers;
mod manifest;
mod models;
mod scanner;
mod vault;

pub use formats::{normalize_repo_rel_path, parse_secret_entries};
pub use manifest::manifest_file_path;
pub use models::{
    IgnoreLayer, IgnoreSnapshot, OversizedFile, SecretFileFormat, SecretFileSpec, SecretFinding,
    SecretsManifest, VaultFileState, VaultFileSummary, VaultKeyInfo, VaultSummary,
};

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::RwLock;

/// Manages `.gitfleet/secrets.age` (age encryption), `.gitfleet/secrets.json` (committed manifest),
/// staged secrets scanner, oversized files, and 3-layer `.gitignore`.
pub struct SecurityService {
    pub(crate) allowed_finding_ids: RwLock<HashSet<String>>,
    pub(crate) allowed_paths: RwLock<HashSet<String>>,
    pub(crate) identity_override: Option<PathBuf>,
}

impl Default for SecurityService {
    fn default() -> Self {
        Self::new()
    }
}

impl SecurityService {
    pub fn new() -> Self {
        Self {
            allowed_finding_ids: RwLock::new(HashSet::new()),
            allowed_paths: RwLock::new(HashSet::new()),
            identity_override: None,
        }
    }

    #[cfg(test)]
    pub fn with_identity_path(path: PathBuf) -> Self {
        Self {
            allowed_finding_ids: RwLock::new(HashSet::new()),
            allowed_paths: RwLock::new(HashSet::new()),
            identity_override: Some(path),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("gitfleet_sec_{tag}_{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn detects_and_allowlists_staged_secrets() {
        let svc = SecurityService::new();
        let diff = "\
+++ b/config.env
@@ -1,0 +1,2 @@
+AWS_KEY=AKIAIOSFODNN7EXAMPLE
+NORMAL_VAR=hello
";
        let findings = svc.scan_staged_diff_secrets(diff);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, "AWS Access Key");

        svc.allow_secret_line(findings[0].id.clone());
        let after = svc.scan_staged_diff_secrets(diff);
        assert!(after.is_empty());
    }

    #[test]
    fn rejects_path_traversal_and_internal_paths() {
        let repo = temp_dir("traversal");
        assert!(normalize_repo_rel_path(&repo, "../outside.env").is_err());
        assert!(normalize_repo_rel_path(&repo, ".git/config").is_err());
        assert!(normalize_repo_rel_path(&repo, ".gitfleet/secrets.age").is_err());
        assert_eq!(
            normalize_repo_rel_path(&repo, r"config\secrets.toml").unwrap(),
            "config/secrets.toml"
        );
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn supports_toml_yaml_env_selection_and_cross_machine_clone_restore() {
        let root = temp_dir("clone_flow");
        let repo_a = root.join("machine_a");
        let repo_b = root.join("machine_b");
        std::fs::create_dir_all(repo_a.join("config")).unwrap();
        std::fs::create_dir_all(repo_a.join("deploy")).unwrap();
        std::fs::create_dir_all(&repo_b).unwrap();

        let key_a = root.join("key_a.txt");
        let key_b = root.join("key_b.txt");
        let svc_a = SecurityService::with_identity_path(key_a.clone());
        let svc_b = SecurityService::with_identity_path(key_b.clone());

        svc_a.create_key().unwrap();

        let env_content = "# local env\nAPI_KEY=env-secret-123\n";
        let toml_content =
            "# DB config\n[database]\nurl = \"postgres://user:pass@localhost/db\"\nport = 5432\n";
        let yaml_content = "auth:\n  jwt_secret: super-yaml-token\n  enabled: true\n";

        std::fs::write(repo_a.join(".env"), env_content).unwrap();
        std::fs::write(repo_a.join("config/secrets.toml"), toml_content).unwrap();
        std::fs::write(repo_a.join("deploy/prod.yaml"), yaml_content).unwrap();

        svc_a.add_manifest_file(&repo_a, ".env", None).unwrap();
        svc_a
            .add_manifest_file(&repo_a, "config/secrets.toml", None)
            .unwrap();
        svc_a
            .add_manifest_file(&repo_a, "deploy/prod.yaml", None)
            .unwrap();

        let count = svc_a.encrypt_selected_files(&repo_a, None).unwrap();
        assert_eq!(count, 3);

        assert_eq!(
            svc_a.reveal_secret(&repo_a, ".env", "API_KEY").unwrap(),
            "env-secret-123"
        );
        assert_eq!(
            svc_a
                .reveal_secret(&repo_a, "config/secrets.toml", "database.url")
                .unwrap(),
            "postgres://user:pass@localhost/db"
        );
        assert_eq!(
            svc_a
                .reveal_secret(&repo_a, "deploy/prod.yaml", "auth.jwt_secret")
                .unwrap(),
            "super-yaml-token"
        );

        svc_a
            .set_secret_value(&repo_a, "config/secrets.toml", "database.token", "tok-999")
            .unwrap();
        assert_eq!(
            svc_a
                .reveal_secret(&repo_a, "config/secrets.toml", "database.token")
                .unwrap(),
            "tok-999"
        );

        std::fs::create_dir_all(repo_b.join(".gitfleet")).unwrap();
        std::fs::copy(
            repo_a.join(".gitfleet/secrets.json"),
            repo_b.join(".gitfleet/secrets.json"),
        )
        .unwrap();
        std::fs::copy(
            repo_a.join(".gitfleet/secrets.age"),
            repo_b.join(".gitfleet/secrets.age"),
        )
        .unwrap();

        let locked_summary = svc_b.vault_summary(&repo_b).unwrap();
        assert!(locked_summary.has_manifest);
        assert!(locked_summary.has_vault);
        assert!(locked_summary.vault_locked);
        assert_eq!(locked_summary.files.len(), 3);
        assert!(
            locked_summary
                .files
                .iter()
                .all(|f| f.state == VaultFileState::Locked)
        );

        let raw_key = std::fs::read_to_string(&key_a).unwrap();
        svc_b.import_key(&raw_key).unwrap();

        let unlocked_summary = svc_b.vault_summary(&repo_b).unwrap();
        assert!(!unlocked_summary.vault_locked);
        assert!(
            unlocked_summary
                .files
                .iter()
                .all(|f| f.state == VaultFileState::MissingOnDisk)
        );

        let restored = svc_b.restore_selected_files(&repo_b, None).unwrap();
        assert_eq!(restored, 3);
        assert_eq!(
            std::fs::read_to_string(repo_b.join(".env")).unwrap(),
            env_content
        );
        let restored_toml = std::fs::read_to_string(repo_b.join("config/secrets.toml")).unwrap();
        assert!(restored_toml.contains("# DB config"));
        assert!(restored_toml.contains("token = \"tok-999\""));
        assert_eq!(
            std::fs::read_to_string(repo_b.join("deploy/prod.yaml")).unwrap(),
            yaml_content
        );

        let synced_summary = svc_b.vault_summary(&repo_b).unwrap();
        assert!(
            synced_summary
                .files
                .iter()
                .all(|f| f.state == VaultFileState::Synced)
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
