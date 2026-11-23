//! Portable Group & Workspace Blueprint (`.gitfleet-pack.json`) schema, path-traversal validator,
//! `Group = Folder` materializer path resolver, and `SSH <-> HTTPS` Git URL protocol rewriter.

use crate::normalize_group_path;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const BLUEPRINT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum UrlProtocolMode {
    #[default]
    KeepOriginal,
    ForceHttps,
    ForceSsh,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlueprintRemote {
    pub name: String,
    pub fetch_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub push_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlueprintRepo {
    pub name: String,
    pub folder_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_path: Option<String>,
    pub origin_url: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_remotes: Vec<BlueprintRemote>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default)]
    pub has_submodules: bool,
    #[serde(default)]
    pub has_secrets_vault: bool,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupBlueprint {
    pub version: u32,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_group: Option<String>,
    pub exported_at_utc: String,
    #[serde(default)]
    pub groups: Vec<String>,
    pub repos: Vec<BlueprintRepo>,
}

/// Strictly validates a single directory segment (group segment or repo `folder_name`) so
/// untrusted `.gitfleet-pack.json` files can NEVER escape `target_base_dir` via `..`, drive
/// letters, UNC prefixes, or Windows reserved device names.
pub fn validate_safe_rel_segment(seg: &str) -> Result<(), String> {
    let trimmed = seg.trim();
    if trimmed.is_empty() {
        return Err("Directory segment cannot be empty".into());
    }
    if trimmed == "." || trimmed == ".." {
        return Err(format!("Unsafe relative segment '{trimmed}' is not allowed"));
    }
    if seg.ends_with('.') || seg.ends_with(' ') {
        return Err(format!(
            "Segment '{seg}' cannot end with a trailing dot or space"
        ));
    }
    for ch in seg.chars() {
        if matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || ch.is_control() {
            return Err(format!("Segment '{seg}' contains invalid character '{ch}'"));
        }
    }
    let stem = trimmed
        .split('.')
        .next()
        .unwrap_or(trimmed)
        .to_ascii_uppercase();
    const RESERVED_WIN: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED_WIN.contains(&stem.as_str()) {
        return Err(format!(
            "Segment '{seg}' is a reserved OS device name and cannot be used as a folder"
        ));
    }
    Ok(())
}

/// Validates an entire normalized group path (`"company/backend/api"`).
pub fn validate_group_hierarchy(group_path: &str) -> Result<String, String> {
    let norm = normalize_group_path(group_path);
    if norm.is_empty() {
        return Ok(String::new());
    }
    for seg in norm.split('/') {
        validate_safe_rel_segment(seg)?;
    }
    Ok(norm)
}

/// Computes the exported group path for a repo (or subgroup) when exporting either:
/// - Full workspace / full hierarchy (`relative_to_selected_group == false`), e.g. `"company/backend/api"`
/// - Relative from the selected group (`relative_to_selected_group == true` for selected `"company/backend"`),
///   keeping the selected group's leaf folder `"backend/api"`!
pub fn compute_export_group_path(
    repo_group: Option<&str>,
    selected_group: Option<&str>,
    relative_to_selected_group: bool,
) -> Option<String> {
    let norm_repo = normalize_group_path(repo_group.unwrap_or(""));
    if norm_repo.is_empty() {
        return None;
    }
    if !relative_to_selected_group {
        return Some(norm_repo);
    }
    let Some(sel_raw) = selected_group else {
        return Some(norm_repo);
    };
    let norm_sel = normalize_group_path(sel_raw);
    if norm_sel.is_empty() {
        return Some(norm_repo);
    }
    // Strip the parent prefix above `selected_group` so exporting `company/backend` yields `backend/...`
    if let Some((parent_prefix, _leaf)) = norm_sel.rsplit_once('/') {
        if let Some(stripped) = norm_repo
            .strip_prefix(parent_prefix)
            .and_then(|s| s.strip_prefix('/'))
            && !stripped.is_empty()
        {
            return Some(stripped.to_string());
        }
    }
    Some(norm_repo)
}

/// Resolves the exact `(parent_dir, repo_dir, final_workspace_group)` for a `BlueprintRepo` inside
/// `target_base_dir`, enforcing a strict 1:1 `Group === Folder` mapping.
///
/// Example:
/// - `target_base_dir = D:\Projects`
/// - `parent_group_prefix = None`
/// - `blueprint_group = Some("company/backend/api")`
/// - `folder_name = "payment-svc"`
/// -> `parent_dir = D:\Projects\company\backend\api`
/// -> `repo_dir   = D:\Projects\company\backend\api\payment-svc`
/// -> `final_group = Some("company/backend/api")`
pub fn resolve_materialized_repo_dest(
    target_base_dir: &Path,
    blueprint_group: Option<&str>,
    parent_group_prefix: Option<&str>,
    folder_name: &str,
) -> Result<(PathBuf, PathBuf, Option<String>), String> {
    validate_safe_rel_segment(folder_name)?;

    let prefix_norm = match parent_group_prefix {
        Some(p) if !p.trim().is_empty() => validate_group_hierarchy(p)?,
        _ => String::new(),
    };
    let bp_norm = match blueprint_group {
        Some(g) if !g.trim().is_empty() => validate_group_hierarchy(g)?,
        _ => String::new(),
    };

    let combined_group = match (prefix_norm.is_empty(), bp_norm.is_empty()) {
        (true, true) => None,
        (false, true) => Some(prefix_norm),
        (true, false) => Some(bp_norm),
        (false, false) => Some(format!("{prefix_norm}/{bp_norm}")),
    };

    let mut parent_dir = target_base_dir.to_path_buf();
    if let Some(ref grp) = combined_group {
        for seg in grp.split('/') {
            parent_dir.push(seg);
        }
    }
    let repo_dir = parent_dir.join(folder_name.trim());
    Ok((parent_dir, repo_dir, combined_group))
}

/// Rewrites Git remote URLs between SSH (`git@host:org/repo.git` / `ssh://git@host/org/repo.git`)
/// and HTTPS (`https://host/org/repo.git`) for seamless cross-machine onboarding.
pub fn rewrite_git_url_protocol(url: &str, mode: UrlProtocolMode) -> String {
    let trimmed = url.trim();
    match mode {
        UrlProtocolMode::KeepOriginal => trimmed.to_string(),
        UrlProtocolMode::ForceHttps => {
            if let Some(rest) = trimmed.strip_prefix("git@")
                && let Some((host, path)) = rest.split_once(':')
                && !host.is_empty()
                && !path.is_empty()
            {
                let clean_path = path.trim_start_matches('/');
                return format!("https://{host}/{clean_path}");
            }
            if let Some(rest) = trimmed.strip_prefix("ssh://") {
                let without_user = rest.rsplit_once('@').map(|(_, h)| h).unwrap_or(rest);
                if let Some((host_port, path)) = without_user.split_once('/') {
                    let host = host_port.split(':').next().unwrap_or(host_port);
                    if !host.is_empty() && !path.is_empty() {
                        return format!("https://{host}/{path}");
                    }
                }
            }
            trimmed.to_string()
        }
        UrlProtocolMode::ForceSsh => {
            let stripped = trimmed
                .strip_prefix("https://")
                .or_else(|| trimmed.strip_prefix("http://"));
            if let Some(rest) = stripped {
                let without_user = rest.rsplit_once('@').map(|(_, h)| h).unwrap_or(rest);
                if let Some((host_port, path)) = without_user.split_once('/') {
                    let host = host_port.split(':').next().unwrap_or(host_port);
                    if !host.is_empty() && !path.is_empty() {
                        return format!("git@{host}:{path}");
                    }
                }
            }
            trimmed.to_string()
        }
    }
}

impl GroupBlueprint {
    pub fn to_pretty_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize Group Blueprint: {e}"))
    }

    pub fn from_json_str(raw: &str) -> Result<Self, String> {
        let mut bp: Self = serde_json::from_str(raw)
            .map_err(|e| format!("Invalid .gitfleet-pack.json file: {e}"))?;
        if bp.version == 0 || bp.version > BLUEPRINT_SCHEMA_VERSION {
            return Err(format!(
                "Unsupported blueprint version {} (expected <= {})",
                bp.version, BLUEPRINT_SCHEMA_VERSION
            ));
        }
        let mut clean_groups = Vec::new();
        for g in &bp.groups {
            let norm = validate_group_hierarchy(g)?;
            if !norm.is_empty() && !clean_groups.contains(&norm) {
                clean_groups.push(norm);
            }
        }
        bp.groups = clean_groups;

        for repo in &mut bp.repos {
            validate_safe_rel_segment(&repo.folder_name)?;
            repo.folder_name = repo.folder_name.trim().to_string();
            if let Some(ref g) = repo.group_path {
                let norm = validate_group_hierarchy(g)?;
                repo.group_path = if norm.is_empty() { None } else { Some(norm) };
            }
            if repo.origin_url.trim().is_empty() {
                return Err(format!(
                    "Repository '{}' has an empty origin_url",
                    repo.name
                ));
            }
        }
        Ok(bp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_path_traversal_and_reserved_windows_names() {
        assert!(validate_safe_rel_segment("..").is_err());
        assert!(validate_safe_rel_segment(".").is_err());
        assert!(validate_safe_rel_segment("foo/bar").is_err());
        assert!(validate_safe_rel_segment("foo\\bar").is_err());
        assert!(validate_safe_rel_segment("C:evil").is_err());
        assert!(validate_safe_rel_segment("CON").is_err());
        assert!(validate_safe_rel_segment("nul.txt").is_err());
        assert!(validate_safe_rel_segment("trailing.").is_err());

        assert!(validate_safe_rel_segment("gitfleet-core").is_ok());
        assert!(validate_safe_rel_segment("service_v2.0").is_ok());
    }

    #[test]
    fn resolves_group_equals_folder_destination_and_relative_subgroup_export() {
        let full = compute_export_group_path(
            Some("company/backend/api"),
            Some("company/backend"),
            false,
        );
        assert_eq!(full.as_deref(), Some("company/backend/api"));

        let rel = compute_export_group_path(
            Some("company/backend/api"),
            Some("company/backend"),
            true,
        );
        assert_eq!(rel.as_deref(), Some("backend/api"));

        let base = Path::new("D:/Work");
        let (parent_dir, repo_dir, final_group) = resolve_materialized_repo_dest(
            base,
            rel.as_deref(),
            Some("imported"),
            "orders-api",
        )
        .expect("valid destination");

        assert_eq!(final_group.as_deref(), Some("imported/backend/api"));
        assert_eq!(parent_dir, base.join("imported").join("backend").join("api"));
        assert_eq!(
            repo_dir,
            base.join("imported")
                .join("backend")
                .join("api")
                .join("orders-api")
        );
    }

    #[test]
    fn rewrites_ssh_and_https_urls_cleanly() {
        assert_eq!(
            rewrite_git_url_protocol(
                "git@github.com:ioz-labs/gitfleet.git",
                UrlProtocolMode::ForceHttps
            ),
            "https://github.com/ioz-labs/gitfleet.git"
        );
        assert_eq!(
            rewrite_git_url_protocol(
                "ssh://git@gitlab.company.io:2222/platform/auth.git",
                UrlProtocolMode::ForceHttps
            ),
            "https://gitlab.company.io/platform/auth.git"
        );
        assert_eq!(
            rewrite_git_url_protocol(
                "https://github.com/ioz-labs/gitfleet.git",
                UrlProtocolMode::ForceSsh
            ),
            "git@github.com:ioz-labs/gitfleet.git"
        );
    }

    #[test]
    fn blueprint_json_roundtrips_and_rejects_malicious_paths() {
        let bp = GroupBlueprint {
            version: BLUEPRINT_SCHEMA_VERSION,
            title: "Group: company/backend".into(),
            source_group: Some("company/backend".into()),
            exported_at_utc: "2026-11-23T15:00:00Z".into(),
            groups: vec!["company/backend".into(), "company/backend/empty-sub".into()],
            repos: vec![BlueprintRepo {
                name: "auth-service".into(),
                folder_name: "auth-service".into(),
                group_path: Some("company/backend".into()),
                origin_url: "git@github.com:acme/auth-service.git".into(),
                extra_remotes: vec![],
                branch: Some("main".into()),
                has_submodules: false,
                has_secrets_vault: true,
                pinned: true,
            }],
        };
        let json = bp.to_pretty_json().expect("serialize");
        let parsed = GroupBlueprint::from_json_str(&json).expect("parse");
        assert_eq!(parsed, bp);

        let bad_json = json.replace("\"auth-service\"", "\"../../etc\"");
        assert!(GroupBlueprint::from_json_str(&bad_json).is_err());
    }
}
