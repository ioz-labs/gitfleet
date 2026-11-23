use gitfleet_core::{AppEvent, AppResult, EventBus, RepoId};
use gitfleet_docs::{detect_and_decode, detect_line_ending, encode_text};
use ignore::WalkBuilder;
use ignore::overrides::{Override, OverrideBuilder};
pub use regex::Regex;
use regex::{NoExpand, RegexBuilder};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub case_sensitive: bool,
    #[serde(default)]
    pub whole_word: bool,
    pub regex: bool,
    #[serde(default)]
    pub include_glob: String,
    #[serde(default)]
    pub exclude_glob: String,
    pub targets: Vec<(RepoId, String, PathBuf)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMatch {
    pub repo_id: RepoId,
    pub repo_name: String,
    pub file_path: String,
    pub line_number: usize,
    pub line_preview: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopedSearchRequest {
    pub root: PathBuf,
    pub query: String,
    pub options: SearchOptions,
    pub include_patterns: String,
    pub exclude_patterns: String,
    pub use_gitignore: bool,
    pub max_matches: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineSearchMatch {
    /// 1-based line number.
    pub line_number: usize,
    /// 0-based character offset in the line where the match starts.
    pub col_start: usize,
    /// 0-based character offset in the line where the match ends (exclusive).
    pub col_end: usize,
    /// Full line text (without trailing CR/LF).
    pub line_text: String,
    /// Exact matched substring.
    pub match_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSearchGroup {
    /// Forward-slash path relative to search root.
    pub file_path: String,
    pub abs_path: PathBuf,
    pub matches: Vec<LineSearchMatch>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScopedSearchResult {
    /// Files whose relative path or filename matches the query (or all filtered files when query is empty).
    pub matched_files: Vec<(String, PathBuf)>,
    /// Line-level content matches grouped by file.
    pub groups: Vec<FileSearchGroup>,
    pub total_matches: usize,
    pub files_scanned: usize,
    pub truncated: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplaceReport {
    pub files_modified: usize,
    pub replacements: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InFileMatch {
    /// 0-based line index.
    pub line_index: usize,
    /// 0-based character offset inside `line_index`.
    pub col_start: usize,
    /// 0-based character offset inside `line_index` (exclusive).
    pub col_end: usize,
    /// 0-based byte offset inside the normalized (`\n`) document string.
    pub byte_start: usize,
    /// 0-based byte offset inside the normalized (`\n`) document string (exclusive).
    pub byte_end: usize,
    /// Trimmed line preview for quick navigation UI.
    pub line_preview: String,
}

/// Builds a compiled regex from a user query and VS Code-style search options.
pub fn build_search_regex(query: &str, opts: SearchOptions) -> Result<Regex, String> {
    if query.is_empty() {
        return Err("Search query is empty".into());
    }
    let core = if opts.regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    let pattern = if opts.whole_word {
        format!(r"\b(?:{core})\b")
    } else {
        core
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(!opts.case_sensitive)
        .build()
        .map_err(|e| format!("Invalid search pattern: {e}"))
}

fn has_glob_meta(pat: &str) -> bool {
    pat.contains('*') || pat.contains('?') || pat.contains('[') || pat.contains('{')
}

fn normalize_glob_token(raw: &str) -> String {
    let mut s = raw.trim().replace('\\', "/");
    while let Some(stripped) = s.strip_prefix("./") {
        s = stripped.to_string();
    }
    while let Some(stripped) = s.strip_prefix('/') {
        s = stripped.to_string();
    }
    while let Some(stripped) = s.strip_suffix('/') {
        s = stripped.to_string();
    }
    s
}

fn add_expanded_pattern(
    builder: &mut OverrideBuilder,
    token: &str,
    negate: bool,
) -> Result<(), String> {
    let prefix = if negate { "!" } else { "" };
    if has_glob_meta(token) {
        builder
            .add(&format!("{prefix}{token}"))
            .map_err(|e| format!("Invalid pattern '{token}': {e}"))?;
        // If user wrote e.g. `src/*` or `crates/*/src`, also allow matching directory subtrees when no trailing `**`
        if token.ends_with("/*") {
            let dir_base = token.trim_end_matches("/*");
            if !dir_base.is_empty() {
                builder
                    .add(&format!("{prefix}{dir_base}/**"))
                    .map_err(|e| format!("Invalid pattern '{token}': {e}"))?;
            }
        }
        return Ok(());
    }

    // Bare file or folder name/path: match both the exact path and any subtree beneath it.
    builder
        .add(&format!("{prefix}{token}"))
        .map_err(|e| format!("Invalid pattern '{token}': {e}"))?;
    builder
        .add(&format!("{prefix}{token}/**"))
        .map_err(|e| format!("Invalid pattern '{token}': {e}"))?;
    if !token.contains('/') {
        builder
            .add(&format!("{prefix}**/{token}"))
            .map_err(|e| format!("Invalid pattern '{token}': {e}"))?;
        builder
            .add(&format!("{prefix}**/{token}/**"))
            .map_err(|e| format!("Invalid pattern '{token}': {e}"))?;
    }
    Ok(())
}

/// Builds `ignore::overrides::Override` from VS Code-style comma-separated include/exclude lists.
pub fn build_scope_overrides(
    root: &Path,
    include_patterns: &str,
    exclude_patterns: &str,
) -> Result<Option<Override>, String> {
    let mut builder = OverrideBuilder::new(root);
    let mut has_any = false;

    for raw in include_patterns.split([',', ';', '\n']) {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(neg) = trimmed.strip_prefix('!') {
            let tok = normalize_glob_token(neg);
            if !tok.is_empty() {
                add_expanded_pattern(&mut builder, &tok, true)?;
                has_any = true;
            }
        } else {
            let tok = normalize_glob_token(trimmed);
            if !tok.is_empty() {
                add_expanded_pattern(&mut builder, &tok, false)?;
                has_any = true;
            }
        }
    }

    for raw in exclude_patterns.split([',', ';', '\n']) {
        let trimmed = raw.trim().trim_start_matches('!');
        let tok = normalize_glob_token(trimmed);
        if tok.is_empty() {
            continue;
        }
        add_expanded_pattern(&mut builder, &tok, true)?;
        has_any = true;
    }

    if !has_any {
        return Ok(None);
    }

    builder
        .build()
        .map(Some)
        .map_err(|e| format!("Failed to compile include/exclude patterns: {e}"))
}

fn byte_to_char_offset(s: &str, byte_idx: usize) -> usize {
    s[..byte_idx.min(s.len())].chars().count()
}

/// Searches both filenames and file contents within a directory, respecting VS Code-style
/// `include_patterns`, `exclude_patterns`, `.gitignore`, case/whole-word/regex toggles.
pub fn search_in_directory(req: &ScopedSearchRequest) -> ScopedSearchResult {
    const MAX_MATCHED_FILES: usize = 200;
    const MAX_FILE_SIZE: u64 = 4 * 1024 * 1024;

    if !req.root.is_dir() {
        return ScopedSearchResult {
            error: Some(format!("Search folder does not exist: {}", req.root.display())),
            ..Default::default()
        };
    }

    let overrides = match build_scope_overrides(&req.root, &req.include_patterns, &req.exclude_patterns) {
        Ok(ov) => ov,
        Err(err) => {
            return ScopedSearchResult {
                error: Some(err),
                ..Default::default()
            };
        }
    };

    let query = req.query.as_str();
    let content_re = if query.is_empty() {
        None
    } else {
        match build_search_regex(query, req.options) {
            Ok(re) => Some(re),
            Err(err) => {
                return ScopedSearchResult {
                    error: Some(err),
                    ..Default::default()
                };
            }
        }
    };

    // Filename matcher: when regex mode is on, reuse compiled regex; otherwise case-insensitive substring
    // (or case-sensitive if user checked Aa).
    let query_lower = query.to_lowercase();

    let mut walk = WalkBuilder::new(&req.root);
    walk.hidden(false)
        .git_ignore(req.use_gitignore)
        .git_global(req.use_gitignore)
        .git_exclude(req.use_gitignore)
        .ignore(req.use_gitignore)
        .parents(req.use_gitignore)
        .max_filesize(Some(MAX_FILE_SIZE))
        .sort_by_file_path(|a, b| a.cmp(b))
        .filter_entry(|e| e.file_name().to_str().is_none_or(|n| n != ".git"));
    if let Some(ov) = overrides {
        walk.overrides(ov);
    }

    let max_matches = req.max_matches.max(1);
    let mut out = ScopedSearchResult::default();

    for entry in walk.build().flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        out.files_scanned += 1;

        let rel = path
            .strip_prefix(&req.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");

        // 1. Filename / relative-path matching
        let file_matches = if query.is_empty() {
            true
        } else if req.options.regex {
            content_re.as_ref().is_some_and(|re| re.is_match(&rel))
        } else if req.options.case_sensitive {
            rel.contains(query)
        } else {
            rel.to_lowercase().contains(&query_lower)
        };

        if file_matches && out.matched_files.len() < MAX_MATCHED_FILES {
            out.matched_files.push((rel.clone(), path.to_path_buf()));
        }

        // 2. Content matching (only when query is non-empty and under limit)
        let Some(re) = &content_re else {
            continue;
        };
        if out.total_matches >= max_matches {
            out.truncated = true;
            continue;
        }

        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Ok((_enc, decoded)) = detect_and_decode(&bytes) else {
            continue;
        };

        let mut file_matches_vec = Vec::new();
        for (idx, raw_line) in decoded.lines().enumerate() {
            let line = raw_line.trim_end_matches('\r');
            for m in re.find_iter(line) {
                if m.start() == m.end() {
                    continue;
                }
                let col_start = byte_to_char_offset(line, m.start());
                let col_end = byte_to_char_offset(line, m.end());
                file_matches_vec.push(LineSearchMatch {
                    line_number: idx + 1,
                    col_start,
                    col_end,
                    line_text: line.to_string(),
                    match_text: m.as_str().to_string(),
                });
                out.total_matches += 1;
                if out.total_matches >= max_matches {
                    out.truncated = true;
                    break;
                }
            }
            if out.truncated {
                break;
            }
        }

        if !file_matches_vec.is_empty() {
            out.groups.push(FileSearchGroup {
                file_path: rel,
                abs_path: path.to_path_buf(),
                matches: file_matches_vec,
            });
        }
    }

    out
}

/// Lists relative forward-slash file paths inside `root` for lightweight Explorer trees,
/// respecting `.gitignore` when `use_gitignore` is true and always skipping `.git`.
pub fn list_directory_files(root: &Path, use_gitignore: bool, max_files: usize) -> Vec<String> {
    if !root.is_dir() {
        return Vec::new();
    }
    let mut walk = WalkBuilder::new(root);
    walk.hidden(false)
        .git_ignore(use_gitignore)
        .git_global(use_gitignore)
        .git_exclude(use_gitignore)
        .ignore(use_gitignore)
        .parents(use_gitignore)
        .sort_by_file_path(|a, b| a.cmp(b))
        .filter_entry(|e| e.file_name().to_str().is_none_or(|n| n != ".git"));

    let limit = max_files.max(1);
    let mut files = Vec::new();
    for entry in walk.build().flatten() {
        let path = entry.path();
        if path == root {
            continue;
        }
        if path.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            files.push(rel);
            if files.len() >= limit {
                break;
            }
        } else if path.is_dir()
            && std::fs::read_dir(path).is_ok_and(|mut it| it.next().is_none())
        {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            if !rel.is_empty() {
                files.push(format!("{rel}/"));
            }
        }
    }
    files
}

/// Replaces matches inside a single file on disk while preserving its encoding and EOL sequence.
/// When `target_line` is `Some(line_1based)`, only matches on that 1-based line are replaced.
pub fn replace_in_file(
    path: &Path,
    query: &str,
    replacement: &str,
    options: SearchOptions,
    target_line: Option<usize>,
) -> Result<usize, String> {
    let re = build_search_regex(query, options)?;
    let bytes = std::fs::read(path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    let (encoding, decoded) = detect_and_decode(&bytes)
        .map_err(|e| format!("Cannot decode {}: {e}", path.display()))?;
    let line_ending = detect_line_ending(&decoded);
    let normalized = decoded.replace("\r\n", "\n");

    let (updated, count) = match target_line {
        None => replace_with_regex(&normalized, &re, replacement, options.regex),
        Some(line_1based) => {
            let mut out = String::with_capacity(normalized.len());
            let mut total_replaced = 0usize;
            for (idx, seg) in normalized.split_inclusive('\n').enumerate() {
                let (line_body, nl) = match seg.strip_suffix('\n') {
                    Some(b) => (b, "\n"),
                    None => (seg, ""),
                };
                if idx + 1 == line_1based {
                    let (new_line, n) = replace_with_regex(line_body, &re, replacement, options.regex);
                    total_replaced += n;
                    out.push_str(&new_line);
                    out.push_str(nl);
                } else {
                    out.push_str(seg);
                }
            }
            (out, total_replaced)
        }
    };

    if count == 0 {
        return Ok(0);
    }

    let (encoded, _lossy) = encode_text(&updated, line_ending, encoding);
    std::fs::write(path, encoded)
        .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
    Ok(count)
}

/// Replaces matches across multiple files, preserving each file's encoding and line endings.
pub fn replace_in_files(
    paths: &[PathBuf],
    query: &str,
    replacement: &str,
    options: SearchOptions,
) -> ReplaceReport {
    let mut report = ReplaceReport::default();
    if let Err(err) = build_search_regex(query, options) {
        report.errors.push(err);
        return report;
    }
    for path in paths {
        match replace_in_file(path, query, replacement, options, None) {
            Ok(0) => {}
            Ok(n) => {
                report.files_modified += 1;
                report.replacements += n;
            }
            Err(err) => report.errors.push(err),
        }
    }
    report
}

fn replace_with_regex(text: &str, re: &Regex, replacement: &str, is_regex: bool) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut last_end = 0usize;
    let mut count = 0usize;

    for caps in re.captures_iter(text) {
        let Some(m) = caps.get(0) else {
            continue;
        };
        if m.start() == m.end() {
            continue;
        }
        out.push_str(&text[last_end..m.start()]);
        if is_regex {
            caps.expand(replacement, &mut out);
        } else {
            out.push_str(replacement);
        }
        last_end = m.end();
        count += 1;
    }

    if count == 0 {
        return (text.to_string(), 0);
    }
    out.push_str(&text[last_end..]);
    (out, count)
}

/// Finds all matches inside an in-memory text buffer (normalized to `\n` internally).
pub fn find_in_text(
    text: &str,
    query: &str,
    options: SearchOptions,
) -> Result<Vec<InFileMatch>, String> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let re = build_search_regex(query, options)?;
    let mut matches = Vec::new();
    let mut byte_offset = 0usize;

    for (line_idx, seg) in text.split_inclusive('\n').enumerate() {
        let line = seg.trim_end_matches(['\r', '\n']);
        for m in re.find_iter(line) {
            if m.start() == m.end() {
                continue;
            }
            matches.push(InFileMatch {
                line_index: line_idx,
                col_start: byte_to_char_offset(line, m.start()),
                col_end: byte_to_char_offset(line, m.end()),
                byte_start: byte_offset + m.start(),
                byte_end: byte_offset + m.end(),
                line_preview: line.trim().chars().take(120).collect(),
            });
        }
        byte_offset += seg.len();
    }

    Ok(matches)
}

/// Replaces a single `InFileMatch` inside `text`, returning the updated text.
pub fn replace_single_match_in_text(
    text: &str,
    target: &InFileMatch,
    query: &str,
    replacement: &str,
    options: SearchOptions,
) -> Result<String, String> {
    if target.byte_end > text.len() || target.byte_start > target.byte_end {
        return Err("Match span is out of bounds".into());
    }
    let slice = &text[target.byte_start..target.byte_end];
    let expanded = if options.regex {
        let re = build_search_regex(query, options)?;
        re.replace(slice, replacement).into_owned()
    } else {
        let re = build_search_regex(query, options)?;
        re.replace(slice, NoExpand(replacement)).into_owned()
    };
    let mut out = String::with_capacity(text.len() + expanded.len());
    out.push_str(&text[..target.byte_start]);
    out.push_str(&expanded);
    out.push_str(&text[target.byte_end..]);
    Ok(out)
}

/// Replaces all occurrences inside an in-memory text buffer, returning `(new_text, count)`.
pub fn replace_all_in_text(
    text: &str,
    query: &str,
    replacement: &str,
    options: SearchOptions,
) -> Result<(String, usize), String> {
    let re = build_search_regex(query, options)?;
    Ok(replace_with_regex(text, &re, replacement, options.regex))
}

/// Cross-repository Fleet Search engine backed by `ignore::WalkBuilder` + `regex`.
pub struct SearchService {
    events: EventBus,
    active_runs: RwLock<HashMap<String, Arc<AtomicBool>>>,
}

impl SearchService {
    pub fn new(events: EventBus) -> Self {
        Self {
            events,
            active_runs: RwLock::new(HashMap::new()),
        }
    }

    pub fn new_cancel_token(&self) -> (String, Arc<AtomicBool>) {
        let run_id = Uuid::new_v4().to_string();
        let flag = Arc::new(AtomicBool::new(false));
        if let Ok(mut guard) = self.active_runs.write() {
            guard.insert(run_id.clone(), flag.clone());
        }
        (run_id, flag)
    }

    pub async fn execute(&self, request: SearchRequest) -> AppResult<(String, Vec<SearchMatch>)> {
        let (run_id, cancelled) = self.new_cancel_token();
        let events = self.events.clone();
        let rid = run_id.clone();

        let matches = tokio::task::spawn_blocking(move || {
            let opts = SearchOptions {
                case_sensitive: request.case_sensitive,
                whole_word: request.whole_word,
                regex: request.regex,
            };
            let Ok(re) = build_search_regex(&request.query, opts) else {
                return Vec::new();
            };

            let mut results = Vec::new();
            const MAX_MATCHES: usize = 2000;

            'outer: for (repo_id, repo_name, root) in request.targets {
                if cancelled.load(Ordering::Relaxed) || !root.is_dir() {
                    break;
                }
                let mut wb = WalkBuilder::new(&root);
                wb.hidden(false)
                    .git_ignore(true)
                    .git_exclude(true)
                    .max_filesize(Some(2 * 1024 * 1024))
                    .filter_entry(|e| {
                        e.file_name()
                            .to_str()
                            .is_none_or(|n| n != ".git" && n != "node_modules" && n != "target")
                    });
                if let Ok(Some(ov)) =
                    build_scope_overrides(&root, &request.include_glob, &request.exclude_glob)
                {
                    wb.overrides(ov);
                }
                let walker = wb.build();

                for entry in walker.flatten() {
                    if cancelled.load(Ordering::Relaxed) {
                        break 'outer;
                    }
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }
                    let Ok(content) = std::fs::read_to_string(path) else {
                        continue;
                    };
                    let rel = path
                        .strip_prefix(&root)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .replace('\\', "/");

                    for (idx, line) in content.lines().enumerate() {
                        if re.is_match(line) {
                            let preview: String = line.trim().chars().take(120).collect();
                            results.push(SearchMatch {
                                repo_id: repo_id.clone(),
                                repo_name: repo_name.clone(),
                                file_path: rel.clone(),
                                line_number: idx + 1,
                                line_preview: preview.clone(),
                            });
                            events.publish(AppEvent::SearchResult {
                                run_id: rid.clone(),
                                repo_id: repo_id.clone(),
                                file: rel.clone(),
                                line: idx + 1,
                                preview,
                            });
                            if results.len() >= MAX_MATCHES {
                                break 'outer;
                            }
                        }
                    }
                }
            }
            events.publish(AppEvent::SearchFinished {
                run_id: rid,
                total_matches: results.len(),
            });
            results
        })
        .await
        .unwrap_or_default();

        if let Ok(mut guard) = self.active_runs.write() {
            guard.remove(&run_id);
        }
        Ok((run_id, matches))
    }

    pub fn cancel(&self, run_id: &str) {
        if let Ok(mut guard) = self.active_runs.write()
            && let Some(flag) = guard.remove(run_id)
        {
            flag.store(true, Ordering::Relaxed);
        }
    }

    pub fn cancel_all(&self) {
        if let Ok(mut guard) = self.active_runs.write() {
            for (_, flag) in guard.drain() {
                flag.store(true, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitfleet_docs::{FileEncoding, LineEnding};

    fn make_temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gitfleet-search-{tag}-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scoped_search_supports_include_exclude_whole_word_and_filenames() {
        let root = make_temp_dir("scoped");
        std::fs::create_dir_all(root.join("src/ui")).unwrap();
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();

        std::fs::write(root.join("src/main.rs"), "fn alpha() {}\nfn alpha_beta() {}\n").unwrap();
        std::fs::write(root.join("src/ui/alpha_view.rs"), "// ALPHA header\nlet alpha = 1;\n").unwrap();
        std::fs::write(root.join("dist/bundle.rs"), "let alpha = 999;\n").unwrap();
        std::fs::write(root.join("docs/alpha.md"), "alpha documentation\n").unwrap();

        // Include `src`, exclude `dist`, whole word + case sensitive
        let res = search_in_directory(&ScopedSearchRequest {
            root: root.clone(),
            query: "alpha".into(),
            options: SearchOptions {
                case_sensitive: true,
                whole_word: true,
                regex: false,
            },
            include_patterns: "src/**, *.md".into(),
            exclude_patterns: "dist, docs".into(),
            use_gitignore: false,
            max_matches: 500,
        });
        assert!(res.error.is_none(), "unexpected error: {:?}", res.error);
        // Should match src/main.rs (line 1 only, not alpha_beta) and src/ui/alpha_view.rs (line 2 only, not ALPHA)
        assert_eq!(res.groups.len(), 2);
        assert_eq!(res.total_matches, 2);
        assert_eq!(res.groups[0].file_path, "src/main.rs");
        assert_eq!(res.groups[0].matches[0].line_number, 1);
        assert_eq!(res.groups[1].file_path, "src/ui/alpha_view.rs");
        assert_eq!(res.groups[1].matches[0].line_number, 2);
        // Filename match finds src/ui/alpha_view.rs
        assert_eq!(res.matched_files.len(), 1);
        assert_eq!(res.matched_files[0].0, "src/ui/alpha_view.rs");

        // Bare folder name in include (`docs`) + empty query lists files in `docs`
        let docs_only = search_in_directory(&ScopedSearchRequest {
            root: root.clone(),
            query: String::new(),
            options: SearchOptions::default(),
            include_patterns: "docs".into(),
            exclude_patterns: String::new(),
            use_gitignore: false,
            max_matches: 100,
        });
        assert_eq!(docs_only.matched_files.len(), 1);
        assert_eq!(docs_only.matched_files[0].0, "docs/alpha.md");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn replace_in_file_preserves_bom_and_crlf_and_supports_single_line() {
        let root = make_temp_dir("replace");
        let file = root.join("sample.txt");
        let (bytes, _) = encode_text(
            "foo one\nfoo two\nfoo three\n",
            LineEnding::Crlf,
            FileEncoding::Utf8Bom,
        );
        std::fs::write(&file, bytes).unwrap();

        // Replace only line 2 with literal `$1` (non-regex must NOT expand `$1`)
        let n = replace_in_file(
            &file,
            "foo",
            "$1bar",
            SearchOptions {
                case_sensitive: false,
                whole_word: true,
                regex: false,
            },
            Some(2),
        )
        .unwrap();
        assert_eq!(n, 1);

        let raw_after = std::fs::read(&file).unwrap();
        assert!(raw_after.starts_with(&[0xEF, 0xBB, 0xBF]), "UTF-8 BOM must be preserved");
        let (enc, decoded) = detect_and_decode(&raw_after).unwrap();
        assert_eq!(enc, FileEncoding::Utf8Bom);
        assert_eq!(detect_line_ending(&decoded), LineEnding::Crlf);
        assert_eq!(decoded, "foo one\r\n$1bar two\r\nfoo three\r\n");

        // Regex replace across entire file
        let report = replace_in_files(
            std::slice::from_ref(&file),
            r"foo (\w+)",
            "baz_$1",
            SearchOptions {
                case_sensitive: false,
                whole_word: false,
                regex: true,
            },
        );
        assert_eq!(report.files_modified, 1);
        assert_eq!(report.replacements, 2);
        assert!(report.errors.is_empty());

        let (_, decoded2) = detect_and_decode(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(decoded2, "baz_one\r\n$1bar two\r\nbaz_three\r\n");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn in_file_find_and_replace_helpers_work_accurately() {
        let text = "hello world\nHELLO again\nhello_world\n";
        let opts = SearchOptions {
            case_sensitive: false,
            whole_word: true,
            regex: false,
        };
        let matches = find_in_text(text, "hello", opts).unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].line_index, 0);
        assert_eq!(matches[0].col_start, 0);
        assert_eq!(matches[0].col_end, 5);
        assert_eq!(matches[1].line_index, 1);

        let replaced_one =
            replace_single_match_in_text(text, &matches[1], "hello", "hi", opts).unwrap();
        assert_eq!(replaced_one, "hello world\nhi again\nhello_world\n");

        let (replaced_all, count) = replace_all_in_text(text, "hello", "hi", opts).unwrap();
        assert_eq!(count, 2);
        assert_eq!(replaced_all, "hi world\nhi again\nhello_world\n");
    }
}
