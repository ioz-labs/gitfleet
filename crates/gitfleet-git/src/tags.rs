use crate::{GitService, runner};
use gitfleet_core::AppResult;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagInfo {
    pub name: String,
    pub target_oid: String,
    pub annotated: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseTrigger {
    pub workflow_file: String,
    pub tag_patterns: Vec<String>,
}

impl GitService {
    pub async fn get_tags(&self, repo_path: &Path) -> AppResult<Vec<TagInfo>> {
        let out = runner::run_git_ok(
            repo_path,
            &[
                "tag",
                "-l",
                "--sort=-creatordate",
                "--format=%(refname:short)%09%(objectname:short)%09%(objecttype)%09%(contents:subject)",
            ],
            Some(&self.command_log),
            Some(&self.events),
            false,
        )
        .await?;

        let mut tags = Vec::new();
        for line in out.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 4 {
                tags.push(TagInfo {
                    name: parts[0].to_string(),
                    target_oid: parts[1].to_string(),
                    annotated: parts[2] == "tag",
                    message: if parts[3].is_empty() {
                        None
                    } else {
                        Some(parts[3].to_string())
                    },
                });
            }
        }
        Ok(tags)
    }

    pub async fn create_tag(
        &self,
        repo_path: &Path,
        name: &str,
        target: Option<&str>,
        message: Option<&str>,
    ) -> AppResult<String> {
        let mut args = vec!["tag"];
        if let Some(m) = message
            && !m.trim().is_empty()
        {
            args.extend(["-a", name, "-m", m]);
        } else {
            args.push(name);
        }
        if let Some(t) = target
            && !t.trim().is_empty()
        {
            args.push(t);
        }
        runner::run_git_ok(
            repo_path,
            &args,
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    pub async fn delete_tag(
        &self,
        repo_path: &Path,
        name: &str,
        remote: Option<&str>,
    ) -> AppResult<String> {
        if let Some(rem) = remote {
            let refspec = format!(":refs/tags/{name}");
            runner::run_git_ok(
                repo_path,
                &["push", rem, &refspec],
                Some(&self.command_log),
                Some(&self.events),
                true,
            )
            .await
        } else {
            runner::run_git_ok(
                repo_path,
                &["tag", "-d", name],
                Some(&self.command_log),
                Some(&self.events),
                true,
            )
            .await
        }
    }

    /// Push explicitly named tags only (never `--tags`).
    pub async fn push_tags(
        &self,
        repo_path: &Path,
        remote: &str,
        names: &[String],
    ) -> AppResult<String> {
        let mut args = vec!["push", remote];
        let refs: Vec<String> = names.iter().map(|n| format!("refs/tags/{n}")).collect();
        let ref_slices: Vec<&str> = refs.iter().map(String::as_str).collect();
        args.extend(ref_slices);
        runner::run_git_ok(
            repo_path,
            &args,
            Some(&self.command_log),
            Some(&self.events),
            true,
        )
        .await
    }

    /// Scans `.github/workflows/*.yml` to detect tag push release triggers.
    pub fn read_release_triggers(&self, repo_path: &Path) -> Vec<ReleaseTrigger> {
        let wf_dir = repo_path.join(".github").join("workflows");
        let Ok(entries) = std::fs::read_dir(&wf_dir) else {
            return Vec::new();
        };
        let mut triggers = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
            if ext != "yml" && ext != "yaml" {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let mut patterns = Vec::new();
            let mut in_tags = false;
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("tags:") {
                    in_tags = true;
                    let inline = trimmed.trim_start_matches("tags:").trim();
                    if inline.starts_with('[') && inline.ends_with(']') {
                        for item in inline.trim_matches(['[', ']']).split(',') {
                            let clean = item.trim().trim_matches(['"', '\'']);
                            if !clean.is_empty() {
                                patterns.push(clean.to_string());
                            }
                        }
                        in_tags = false;
                    }
                } else if in_tags {
                    if let Some(item) = trimmed.strip_prefix("- ") {
                        patterns.push(item.trim().trim_matches(['"', '\'']).to_string());
                    } else if !trimmed.is_empty() && !trimmed.starts_with('#') {
                        in_tags = false;
                    }
                }
            }
            if !patterns.is_empty() {
                triggers.push(ReleaseTrigger {
                    workflow_file: entry.file_name().to_string_lossy().to_string(),
                    tag_patterns: patterns,
                });
            }
        }
        triggers
    }
}
