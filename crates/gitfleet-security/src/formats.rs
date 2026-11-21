use crate::models::SecretFileFormat;
use gitfleet_core::{AppError, AppResult};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// Normalizes and validates a repo-relative secret file path.
/// Rejects path traversal (`..`), paths escaping `repo_path`, and `.git/` or `.gitfleet/` internals.
pub fn normalize_repo_rel_path(repo_path: &Path, input: &str) -> AppResult<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AppError::Security("Secret file path cannot be empty".into()));
    }

    let raw_path = Path::new(trimmed);
    let rel_buf: PathBuf = if raw_path.is_absolute() {
        if let Ok(stripped) = raw_path.strip_prefix(repo_path) {
            stripped.to_path_buf()
        } else {
            let canon_repo =
                std::fs::canonicalize(repo_path).unwrap_or_else(|_| repo_path.to_path_buf());
            let canon_in =
                std::fs::canonicalize(raw_path).unwrap_or_else(|_| raw_path.to_path_buf());
            canon_in
                .strip_prefix(&canon_repo)
                .map(Path::to_path_buf)
                .map_err(|_| {
                    AppError::Security(format!(
                        "Selected file '{}' must be inside the repository",
                        raw_path.display()
                    ))
                })?
        }
    } else {
        PathBuf::from(trimmed.replace('\\', "/"))
    };

    let mut parts: Vec<String> = Vec::new();
    for comp in rel_buf.components() {
        match comp {
            Component::Normal(os) => {
                let s = os.to_string_lossy().to_string();
                if !s.is_empty() {
                    parts.push(s);
                }
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(AppError::Security(format!(
                    "Invalid secret file path '{trimmed}': must stay inside repository root"
                )));
            }
        }
    }

    if parts.is_empty() {
        return Err(AppError::Security("Secret file path cannot be empty".into()));
    }

    let first_lower = parts[0].to_lowercase();
    if first_lower == ".git" || first_lower == ".gitfleet" {
        return Err(AppError::Security(format!(
            "Cannot track internal metadata path '{}'",
            parts.join("/")
        )));
    }

    Ok(parts.join("/"))
}

pub(crate) fn rel_to_fs_path(repo_path: &Path, normalized_rel: &str) -> PathBuf {
    let mut full = repo_path.to_path_buf();
    for seg in normalized_rel.split('/') {
        full.push(seg);
    }
    full
}

pub(crate) fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n")
}

pub(crate) fn validate_secret_content(
    format: SecretFileFormat,
    path: &str,
    raw: &str,
) -> AppResult<()> {
    match format {
        SecretFileFormat::Env => Ok(()),
        SecretFileFormat::Toml => {
            raw.parse::<toml_edit::DocumentMut>()
                .map_err(|e| AppError::Security(format!("Invalid TOML in '{path}': {e}")))?;
            Ok(())
        }
        SecretFileFormat::Yaml => {
            serde_yaml_ng::from_str::<serde_yaml_ng::Value>(raw)
                .map_err(|e| AppError::Security(format!("Invalid YAML in '{path}': {e}")))?;
            Ok(())
        }
    }
}

pub fn parse_secret_entries(
    format: SecretFileFormat,
    raw: &str,
) -> AppResult<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    match format {
        SecretFileFormat::Env => {
            for line in raw.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                let stripped = trimmed.strip_prefix("export ").unwrap_or(trimmed);
                if let Some((k, v)) = stripped.split_once('=') {
                    let key = k.trim();
                    if !key.is_empty() {
                        out.insert(key.to_string(), v.trim().to_string());
                    }
                }
            }
        }
        SecretFileFormat::Toml => {
            let doc = raw
                .parse::<toml_edit::DocumentMut>()
                .map_err(|e| AppError::Security(format!("Invalid TOML: {e}")))?;
            flatten_toml_table("", doc.as_table(), &mut out);
        }
        SecretFileFormat::Yaml => {
            if raw.trim().is_empty() {
                return Ok(out);
            }
            let val: serde_yaml_ng::Value = serde_yaml_ng::from_str(raw)
                .map_err(|e| AppError::Security(format!("Invalid YAML: {e}")))?;
            flatten_yaml_value("", &val, &mut out);
        }
    }
    Ok(out)
}

pub(crate) fn update_secret_content(
    format: SecretFileFormat,
    raw: &str,
    key: &str,
    value: &str,
) -> AppResult<String> {
    match format {
        SecretFileFormat::Env => {
            let mut lines: Vec<String> = Vec::new();
            let mut replaced = false;
            for line in raw.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('#') {
                    let stripped = trimmed.strip_prefix("export ").unwrap_or(trimmed);
                    if let Some((k, _)) = stripped.split_once('=')
                        && k.trim() == key
                    {
                        let prefix = if trimmed.starts_with("export ") {
                            "export "
                        } else {
                            ""
                        };
                        lines.push(format!("{prefix}{key}={value}"));
                        replaced = true;
                        continue;
                    }
                }
                lines.push(line.to_string());
            }
            if !replaced {
                lines.push(format!("{key}={value}"));
            }
            let mut out = lines.join("\n");
            out.push('\n');
            Ok(out)
        }
        SecretFileFormat::Toml => {
            let mut doc = if raw.trim().is_empty() {
                toml_edit::DocumentMut::new()
            } else {
                raw.parse::<toml_edit::DocumentMut>()
                    .map_err(|e| AppError::Security(format!("Invalid TOML: {e}")))?
            };
            let segments: Vec<&str> = key.split('.').filter(|s| !s.is_empty()).collect();
            if segments.is_empty() {
                return Err(AppError::Security("Invalid TOML key".into()));
            }
            let mut table = doc.as_table_mut();
            for seg in &segments[..segments.len() - 1] {
                if !table.contains_key(seg) || !table[*seg].is_table() {
                    table[*seg] = toml_edit::Item::Table(toml_edit::Table::new());
                }
                table = table[*seg].as_table_mut().ok_or_else(|| {
                    AppError::Security(format!("Key segment '{seg}' is not a table"))
                })?;
            }
            let leaf = segments[segments.len() - 1];
            table[leaf] = toml_edit::value(value);
            Ok(doc.to_string())
        }
        SecretFileFormat::Yaml => {
            let mut root = if raw.trim().is_empty() {
                serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::new())
            } else {
                serde_yaml_ng::from_str::<serde_yaml_ng::Value>(raw)
                    .map_err(|e| AppError::Security(format!("Invalid YAML: {e}")))?
            };
            let segments: Vec<&str> = key.split('.').filter(|s| !s.is_empty()).collect();
            if segments.is_empty() {
                return Err(AppError::Security("Invalid YAML key".into()));
            }
            set_yaml_nested(&mut root, &segments, value)?;
            serde_yaml_ng::to_string(&root)
                .map_err(|e| AppError::Security(format!("YAML serialize error: {e}")))
        }
    }
}

fn flatten_toml_table(prefix: &str, table: &toml_edit::Table, out: &mut BTreeMap<String, String>) {
    for (k, item) in table.iter() {
        let full = if prefix.is_empty() {
            k.to_string()
        } else {
            format!("{prefix}.{k}")
        };
        flatten_toml_item(&full, item, out);
    }
}

fn flatten_toml_item(key: &str, item: &toml_edit::Item, out: &mut BTreeMap<String, String>) {
    match item {
        toml_edit::Item::Table(t) => flatten_toml_table(key, t, out),
        toml_edit::Item::ArrayOfTables(arr) => {
            for (idx, t) in arr.iter().enumerate() {
                flatten_toml_table(&format!("{key}.{idx}"), t, out);
            }
        }
        toml_edit::Item::Value(v) => flatten_toml_value(key, v, out),
        toml_edit::Item::None => {}
    }
}

fn flatten_toml_value(key: &str, val: &toml_edit::Value, out: &mut BTreeMap<String, String>) {
    match val {
        toml_edit::Value::String(s) => {
            out.insert(key.to_string(), s.value().clone());
        }
        toml_edit::Value::Integer(i) => {
            out.insert(key.to_string(), i.value().to_string());
        }
        toml_edit::Value::Float(f) => {
            out.insert(key.to_string(), f.value().to_string());
        }
        toml_edit::Value::Boolean(b) => {
            out.insert(key.to_string(), b.value().to_string());
        }
        toml_edit::Value::Datetime(dt) => {
            out.insert(key.to_string(), dt.value().to_string());
        }
        toml_edit::Value::InlineTable(t) => {
            for (k, v) in t.iter() {
                flatten_toml_value(&format!("{key}.{k}"), v, out);
            }
        }
        toml_edit::Value::Array(arr) => {
            for (idx, v) in arr.iter().enumerate() {
                flatten_toml_value(&format!("{key}.{idx}"), v, out);
            }
        }
    }
}

fn flatten_yaml_value(
    prefix: &str,
    val: &serde_yaml_ng::Value,
    out: &mut BTreeMap<String, String>,
) {
    match val {
        serde_yaml_ng::Value::Mapping(map) => {
            for (k, v) in map {
                let key_seg = match k {
                    serde_yaml_ng::Value::String(s) => s.clone(),
                    serde_yaml_ng::Value::Number(n) => n.to_string(),
                    serde_yaml_ng::Value::Bool(b) => b.to_string(),
                    _ => continue,
                };
                let full = if prefix.is_empty() {
                    key_seg
                } else {
                    format!("{prefix}.{key_seg}")
                };
                flatten_yaml_value(&full, v, out);
            }
        }
        serde_yaml_ng::Value::Sequence(seq) => {
            for (idx, item) in seq.iter().enumerate() {
                let full = if prefix.is_empty() {
                    idx.to_string()
                } else {
                    format!("{prefix}.{idx}")
                };
                flatten_yaml_value(&full, item, out);
            }
        }
        serde_yaml_ng::Value::String(s) if !prefix.is_empty() => {
            out.insert(prefix.to_string(), s.clone());
        }
        serde_yaml_ng::Value::Number(n) if !prefix.is_empty() => {
            out.insert(prefix.to_string(), n.to_string());
        }
        serde_yaml_ng::Value::Bool(b) if !prefix.is_empty() => {
            out.insert(prefix.to_string(), b.to_string());
        }
        serde_yaml_ng::Value::Null if !prefix.is_empty() => {
            out.insert(prefix.to_string(), String::new());
        }
        serde_yaml_ng::Value::Tagged(tagged) => {
            flatten_yaml_value(prefix, &tagged.value, out);
        }
        _ => {}
    }
}

fn set_yaml_nested(
    current: &mut serde_yaml_ng::Value,
    segments: &[&str],
    value: &str,
) -> AppResult<()> {
    if !current.is_mapping() {
        *current = serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::new());
    }
    let map = current
        .as_mapping_mut()
        .ok_or_else(|| AppError::Security("Expected YAML mapping".into()))?;
    let k = serde_yaml_ng::Value::String(segments[0].to_string());
    if segments.len() == 1 {
        map.insert(k, serde_yaml_ng::Value::String(value.to_string()));
        return Ok(());
    }
    let entry = map
        .entry(k)
        .or_insert_with(|| serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::new()));
    set_yaml_nested(entry, &segments[1..], value)
}
