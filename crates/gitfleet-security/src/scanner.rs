use crate::SecurityService;
use crate::models::{OversizedFile, SecretFinding};
use regex::Regex;
use std::path::Path;

impl SecurityService {
    pub fn allow_secret_line(&self, finding_id: String) {
        if let Ok(mut guard) = self.allowed_finding_ids.write() {
            guard.insert(finding_id);
        }
    }

    pub fn allow_secret_path(&self, path: String) {
        if let Ok(mut guard) = self.allowed_paths.write() {
            guard.insert(path);
        }
    }

    /// Scans added lines of a staged unified diff for leaked credentials/private keys.
    pub fn scan_staged_diff_secrets(&self, staged_diff: &str) -> Vec<SecretFinding> {
        let rules: [(&str, &str); 6] = [
            ("AWS Access Key", r"AKIA[0-9A-Z]{16}"),
            (
                "GitHub Token",
                r"(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9_]{20,}|github_pat_[A-Za-z0-9_]{20,}",
            ),
            ("Private Key", r"-----BEGIN [A-Z ]*PRIVATE KEY-----"),
            (
                "OpenAI / Stripe Key",
                r"sk-(?:live|test|proj|ant-api)[A-Za-z0-9_-]{16,}",
            ),
            ("Slack Token", r"xox[baprs]-[0-9A-Za-z-]{10,}"),
            (
                "Hardcoded Secret",
                r#"(?i)(?:api[_-]?key|secret[_-]?key|access[_-]?token|password)\s*[:=]\s*["'][A-Za-z0-9_\-/+=]{12,}["']"#,
            ),
        ];

        let compiled: Vec<(&str, Regex)> = rules
            .iter()
            .filter_map(|(name, pat)| Regex::new(pat).ok().map(|r| (*name, r)))
            .collect();

        let allowed_ids = self.allowed_finding_ids.read().ok();
        let allowed_paths = self.allowed_paths.read().ok();

        let mut findings = Vec::new();
        let mut current_file = String::new();
        let mut line_no = 0usize;

        for line in staged_diff.lines() {
            if let Some(f) = line.strip_prefix("+++ b/") {
                current_file = f.to_string();
                line_no = 0;
                continue;
            }
            if line.starts_with("@@") {
                line_no = 0;
                continue;
            }
            if let Some(added) = line.strip_prefix('+') {
                if line.starts_with("+++") {
                    continue;
                }
                line_no += 1;
                if allowed_paths
                    .as_ref()
                    .is_some_and(|s| s.contains(&current_file))
                {
                    continue;
                }
                for (rule_name, re) in &compiled {
                    if re.is_match(added) {
                        let id = format!("{current_file}:{line_no}:{rule_name}");
                        if allowed_ids.as_ref().is_some_and(|s| s.contains(&id)) {
                            continue;
                        }
                        findings.push(SecretFinding {
                            id,
                            file_path: current_file.clone(),
                            line: line_no,
                            rule: (*rule_name).to_string(),
                            preview: added.trim().chars().take(64).collect(),
                        });
                        break;
                    }
                }
            }
        }
        findings
    }

    pub fn scan_oversized_staged(
        &self,
        repo_path: &Path,
        staged_paths: &[String],
        threshold_bytes: u64,
    ) -> Vec<OversizedFile> {
        let mut out = Vec::new();
        for rel in staged_paths {
            let full = repo_path.join(rel);
            if let Ok(meta) = std::fs::metadata(&full)
                && meta.len() >= threshold_bytes
            {
                out.push(OversizedFile {
                    file_path: rel.clone(),
                    bytes: meta.len(),
                });
            }
        }
        out
    }
}
