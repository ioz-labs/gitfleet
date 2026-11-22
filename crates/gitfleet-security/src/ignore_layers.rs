use crate::SecurityService;
use crate::models::{IgnoreLayer, IgnoreSnapshot};
use gitfleet_core::{AppError, AppResult};
use regex::Regex;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppendIgnoreResult {
    pub added: usize,
    pub skipped_existing: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitignorePreset {
    pub label: String,
    pub pattern: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegexRuleOrigin {
    /// Cleanly translated into equivalent `.gitignore` glob rule(s).
    ExactGlob,
    /// Generalized into approximate `.gitignore` glob rule(s) (e.g. `\d+` -> `[0-9]*`).
    ApproximateGlob,
    /// Expanded directly to the exact workspace paths matched by the regex.
    MatchedPaths,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegexIgnorePreview {
    pub matched_paths: Vec<String>,
    pub total_matches: usize,
    pub rules: Vec<String>,
    pub origin: RegexRuleOrigin,
    pub can_translate_glob: bool,
}

impl SecurityService {
    pub fn ignore_snapshot(&self, repo_path: &Path) -> AppResult<IgnoreSnapshot> {
        let root = repo_path.join(".gitignore");
        let personal = repo_path.join(".git").join("info").join("exclude");
        let global_path = resolve_global_excludes_path();
        let global_content = global_path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default();

        Ok(IgnoreSnapshot {
            root_gitignore: std::fs::read_to_string(root).unwrap_or_default(),
            personal_exclude: std::fs::read_to_string(personal).unwrap_or_default(),
            global_exclude_path: global_path,
            global_exclude: global_content,
        })
    }

    pub fn write_ignore_layer(
        &self,
        repo_path: &Path,
        layer: IgnoreLayer,
        content: &str,
    ) -> AppResult<()> {
        let target = resolve_ignore_layer_path(repo_path, layer)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(target, content)?;
        Ok(())
    }

    pub fn append_gitignore(&self, repo_path: &Path, pattern: &str) -> AppResult<()> {
        let _ = self.append_ignore_rules(
            repo_path,
            IgnoreLayer::RootGitignore,
            &[pattern.to_string()],
        )?;
        Ok(())
    }

    /// Appends one or more `.gitignore` rules to `layer`, deduplicating against existing lines
    /// and within `rules`. Creates parent directories if needed.
    pub fn append_ignore_rules(
        &self,
        repo_path: &Path,
        layer: IgnoreLayer,
        rules: &[String],
    ) -> AppResult<AppendIgnoreResult> {
        let target = resolve_ignore_layer_path(repo_path, layer)?;
        let existing = std::fs::read_to_string(&target).unwrap_or_default();
        let mut seen: HashSet<String> = existing
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect();

        let mut to_add = Vec::new();
        let mut skipped_existing = 0usize;
        for raw in rules {
            for line in raw.lines() {
                let rule = line.trim();
                if rule.is_empty() {
                    continue;
                }
                if seen.insert(rule.to_string()) {
                    to_add.push(rule.to_string());
                } else {
                    skipped_existing += 1;
                }
            }
        }

        if to_add.is_empty() {
            return Ok(AppendIgnoreResult {
                added: 0,
                skipped_existing,
            });
        }

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut next = existing;
        if !next.is_empty() && !next.ends_with('\n') {
            next.push('\n');
        }
        for rule in &to_add {
            next.push_str(rule);
            next.push('\n');
        }
        std::fs::write(target, next)?;
        Ok(AppendIgnoreResult {
            added: to_add.len(),
            skipped_existing,
        })
    }
}

fn resolve_ignore_layer_path(repo_path: &Path, layer: IgnoreLayer) -> AppResult<PathBuf> {
    match layer {
        IgnoreLayer::RootGitignore => Ok(repo_path.join(".gitignore")),
        IgnoreLayer::PersonalExclude => Ok(repo_path.join(".git").join("info").join("exclude")),
        IgnoreLayer::GlobalExclude => resolve_global_excludes_path()
            .ok_or_else(|| AppError::Security("Global excludes path not configured".into())),
    }
}

/// Builds concise, deduplicated `.gitignore` glob presets tailored to a file or folder.
pub fn build_gitignore_presets(rel_path: &str, is_dir: bool) -> Vec<GitignorePreset> {
    let norm = rel_path.replace('\\', "/");
    let trimmed = norm.trim_matches('/');
    if trimmed.is_empty() {
        return Vec::new();
    }
    let (parent, name) = match trimmed.rsplit_once('/') {
        Some((p, n)) => (p, n),
        None => ("", trimmed),
    };

    let mut presets = Vec::new();
    let mut push_unique = |label: String, pattern: String| {
        if !presets.iter().any(|p: &GitignorePreset| p.pattern == pattern) {
            presets.push(GitignorePreset { label, pattern });
        }
    };

    if is_dir {
        push_unique("Exact folder".into(), format!("/{trimmed}/"));
        push_unique(format!("Any '{name}/'"), format!("{name}/"));
        return presets;
    }

    push_unique("Exact file".into(), format!("/{trimmed}"));
    push_unique(format!("Any '{name}'"), name.to_string());

    // Extension presets (skip single-dot dotfiles like `.env`, offer `.env.*` for them instead)
    if let Some(dot) = name.rfind('.') {
        if dot > 0 && dot + 1 < name.len() {
            let ext = &name[dot..]; // includes '.'
            if !parent.is_empty() {
                push_unique(format!("*{ext} in folder"), format!("/{parent}/*{ext}"));
            }
            push_unique(format!("All *{ext}"), format!("*{ext}"));
        } else if dot == 0 && name.len() > 1 {
            push_unique(format!("{name}.* variants"), format!("{name}.*"));
        }
    }

    presets
}

/// Builds regex starter presets tailored to a file or folder.
pub fn build_gitignore_regex_presets(rel_path: &str, is_dir: bool) -> Vec<GitignorePreset> {
    let norm = rel_path.replace('\\', "/");
    let trimmed = norm.trim_matches('/');
    if trimmed.is_empty() {
        return Vec::new();
    }
    let (parent, name) = match trimmed.rsplit_once('/') {
        Some((p, n)) => (p, n),
        None => ("", trimmed),
    };

    let esc_full = escape_regex_literal(trimmed);
    let esc_name = escape_regex_literal(name);
    let esc_parent = escape_regex_literal(parent);

    let mut presets = Vec::new();
    let mut push_unique = |label: String, pattern: String| {
        if !presets.iter().any(|p: &GitignorePreset| p.pattern == pattern) {
            presets.push(GitignorePreset { label, pattern });
        }
    };

    if is_dir {
        push_unique("Exact folder".into(), format!("^{esc_full}(/|$)"));
        push_unique(format!("Any '{name}/'"), format!("(^|/){esc_name}(/|$)"));
        push_unique("All inside".into(), format!("^{esc_full}/.*"));
        return presets;
    }

    push_unique("Exact file".into(), format!("^{esc_full}$"));
    push_unique(format!("Any '{name}'"), format!("(^|/){esc_name}$"));

    if let Some(dot) = name.rfind('.') {
        if dot > 0 && dot + 1 < name.len() {
            let ext = &name[dot..];
            let esc_ext = escape_regex_literal(ext);
            if !parent.is_empty() {
                push_unique(
                    format!("*{ext} in folder"),
                    format!("^{esc_parent}/.*{esc_ext}$"),
                );
            }
            push_unique(format!("All *{ext}"), format!(".*{esc_ext}$"));
        } else if dot == 0 && name.len() > 1 {
            push_unique(format!("{name}.* variants"), format!("(^|/){esc_name}\\..*$"));
        }
    }

    presets
}

/// Default regex expression pre-filled when switching to Regex mode.
pub fn default_regex_for_entry(rel_path: &str, is_dir: bool) -> String {
    build_gitignore_regex_presets(rel_path, is_dir)
        .into_iter()
        .next()
        .map(|p| p.pattern)
        .unwrap_or_default()
}

/// Evaluates `regex_input` against workspace `candidates` and resolves the `.gitignore` rules
/// that will be appended (either cleanly translated glob rules or exact matched workspace paths).
pub fn preview_regex_gitignore(
    regex_input: &str,
    candidates: &[String],
    prefer_matched_paths: bool,
) -> Result<RegexIgnorePreview, String> {
    let trimmed = regex_input.trim();
    if trimmed.is_empty() {
        return Err("Enter a regular expression pattern.".into());
    }
    let re = Regex::new(trimmed).map_err(|e| format!("Invalid regex: {e}"))?;

    let mut matched_paths = Vec::new();
    let mut seen_matches = HashSet::new();
    for raw in candidates {
        let norm = raw.replace('\\', "/");
        let clean = norm.trim_start_matches('/');
        if clean.is_empty() {
            continue;
        }
        let no_slash = clean.trim_end_matches('/');
        let is_match = re.is_match(clean) || (clean != no_slash && re.is_match(no_slash));
        if is_match && seen_matches.insert(clean.to_string()) {
            matched_paths.push(clean.to_string());
        }
    }
    let total_matches = matched_paths.len();

    let translation = translate_regex_to_gitignore_globs(trimmed);
    let can_translate_glob = !matches!(translation, GlobTranslation::NotTranslatable);

    let (rules, origin) = if prefer_matched_paths || !can_translate_glob {
        let exact_rules = matched_paths
            .iter()
            .map(|p| format!("/{p}"))
            .collect::<Vec<_>>();
        (exact_rules, RegexRuleOrigin::MatchedPaths)
    } else {
        match translation {
            GlobTranslation::Exact(globs) => (globs, RegexRuleOrigin::ExactGlob),
            GlobTranslation::Approximate(globs) => (globs, RegexRuleOrigin::ApproximateGlob),
            GlobTranslation::NotTranslatable => unreachable!(),
        }
    };

    Ok(RegexIgnorePreview {
        matched_paths,
        total_matches,
        rules,
        origin,
        can_translate_glob,
    })
}

fn escape_regex_literal(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 4);
    for ch in raw.chars() {
        if matches!(
            ch,
            '.' | '+' | '*' | '?' | '^' | '$' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\'
        ) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GlobTranslation {
    Exact(Vec<String>),
    Approximate(Vec<String>),
    NotTranslatable,
}

fn translate_regex_to_gitignore_globs(pattern: &str) -> GlobTranslation {
    let mut rest = pattern.trim();
    if rest.is_empty() {
        return GlobTranslation::NotTranslatable;
    }

    // Extract common start/end wrappers before expanding alternations so `(^|/)(a|b)(/|$)`
    // expands cleanly into `a/` and `b/` instead of cross-product anchor variants.
    let mut outer_path_boundary = false;
    if let Some(stripped) = rest.strip_prefix("(^|/)") {
        outer_path_boundary = true;
        rest = stripped;
    }
    let mut outer_dir_suffix = false;
    if let Some(stripped) = rest.strip_suffix("(/|$)") {
        outer_dir_suffix = true;
        rest = stripped;
    } else if let Some(stripped) = rest.strip_suffix("(/.*)?$") {
        outer_dir_suffix = true;
        rest = stripped;
    }

    let Some(branches) = expand_alternations(rest, 16) else {
        return GlobTranslation::NotTranslatable;
    };

    let mut rules = Vec::new();
    let mut seen = HashSet::new();
    let mut any_approx = false;

    for branch in branches {
        let mut assembled = String::new();
        if outer_path_boundary {
            assembled.push_str("(^|/)");
        }
        assembled.push_str(&branch);
        if outer_dir_suffix {
            assembled.push_str("(/|$)");
        }
        match translate_single_branch(&assembled) {
            BranchTranslation::Exact(rule) => {
                if seen.insert(rule.clone()) {
                    rules.push(rule);
                }
            }
            BranchTranslation::Approximate(rule) => {
                any_approx = true;
                if seen.insert(rule.clone()) {
                    rules.push(rule);
                }
            }
            BranchTranslation::NotTranslatable => return GlobTranslation::NotTranslatable,
        }
    }

    if rules.is_empty() {
        return GlobTranslation::NotTranslatable;
    }
    if any_approx {
        GlobTranslation::Approximate(rules)
    } else {
        GlobTranslation::Exact(rules)
    }
}

/// Expands top-level `a|b` and simple non-nested `(a|b)` alternations up to `max_branches`.
fn expand_alternations(input: &str, max_branches: usize) -> Option<Vec<String>> {
    // First split by top-level `|` (outside `(...)` and `[...]`)
    let top_parts = split_top_level_pipes(input)?;
    let mut expanded = Vec::new();
    for part in top_parts {
        let sub = expand_inline_groups(&part, max_branches)?;
        for s in sub {
            expanded.push(s);
            if expanded.len() > max_branches {
                return None;
            }
        }
    }
    Some(expanded)
}

fn split_top_level_pipes(input: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = input.chars().collect();
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut depth = 0usize;
    let mut in_class = false;
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\\' && i + 1 < chars.len() {
            cur.push(ch);
            cur.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if in_class {
            if ch == ']' {
                in_class = false;
            }
            cur.push(ch);
            i += 1;
            continue;
        }
        match ch {
            '[' => {
                in_class = true;
                cur.push(ch);
            }
            '(' => {
                depth += 1;
                cur.push(ch);
            }
            ')' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                cur.push(ch);
            }
            '|' if depth == 0 => {
                parts.push(std::mem::take(&mut cur));
            }
            _ => cur.push(ch),
        }
        i += 1;
    }
    if in_class || depth != 0 {
        return None;
    }
    parts.push(cur);
    Some(parts)
}

fn expand_inline_groups(input: &str, max_branches: usize) -> Option<Vec<String>> {
    let chars: Vec<char> = input.chars().collect();
    let mut in_class = false;
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\\' && i + 1 < chars.len() {
            i += 2;
            continue;
        }
        if in_class {
            if ch == ']' {
                in_class = false;
            }
            i += 1;
            continue;
        }
        if ch == '[' {
            in_class = true;
            i += 1;
            continue;
        }
        if ch == '(' {
            // Find matching `)`
            let start = i;
            let mut depth = 1usize;
            let mut j = i + 1;
            let mut inner_class = false;
            while j < chars.len() && depth > 0 {
                let cj = chars[j];
                if cj == '\\' && j + 1 < chars.len() {
                    j += 2;
                    continue;
                }
                if inner_class {
                    if cj == ']' {
                        inner_class = false;
                    }
                    j += 1;
                    continue;
                }
                match cj {
                    '[' => inner_class = true,
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            if depth != 0 {
                return None;
            }
            let close_idx = j - 1;
            // Reject groups with regex flags/lookarounds `(?:...)` is ok, `(?i)` / `(?=...)` is not
            let inner: String = chars[start + 1..close_idx].iter().collect();
            let inner_clean = if let Some(nc) = inner.strip_prefix("?:") {
                nc
            } else if inner.starts_with('?') {
                return None;
            } else {
                inner.as_str()
            };
            // Reject quantified groups like `(foo)+` or `(foo)*` (only allow optional `(foo)?` if no quantifier other than `?`)
            let next_char = chars.get(close_idx + 1).copied();
            if matches!(next_char, Some('*' | '+' | '{')) {
                return None;
            }
            let optional_group = next_char == Some('?');
            let after_idx = if optional_group {
                close_idx + 2
            } else {
                close_idx + 1
            };

            let prefix: String = chars[..start].iter().collect();
            let suffix: String = chars[after_idx..].iter().collect();
            let mut alts = split_top_level_pipes(inner_clean)?;
            if optional_group {
                alts.push(String::new());
            }
            let mut out = Vec::new();
            for alt in alts {
                let combined = format!("{prefix}{alt}{suffix}");
                let sub = expand_inline_groups(&combined, max_branches)?;
                for s in sub {
                    out.push(s);
                    if out.len() > max_branches {
                        return None;
                    }
                }
            }
            return Some(out);
        }
        i += 1;
    }
    Some(vec![input.to_string()])
}

enum BranchTranslation {
    Exact(String),
    Approximate(String),
    NotTranslatable,
}

fn translate_single_branch(branch: &str) -> BranchTranslation {
    let mut s = branch;
    let mut anchored_start = false;
    let mut path_boundary_start = false;
    let mut anchored_end = false;
    let mut dir_suffix = false;

    if let Some(rest) = s.strip_prefix("(^|/)") {
        path_boundary_start = true;
        s = rest;
    } else if let Some(rest) = s.strip_prefix('^') {
        anchored_start = true;
        s = rest;
    }

    if let Some(rest) = s.strip_suffix("(/|$)") {
        dir_suffix = true;
        anchored_end = true;
        s = rest;
    } else if let Some(rest) = s.strip_suffix("(/.*)?$") {
        dir_suffix = true;
        anchored_end = true;
        s = rest;
    } else if let Some(rest) = s.strip_suffix("/?$") {
        dir_suffix = true;
        anchored_end = true;
        s = rest;
    } else if let Some(rest) = s.strip_suffix("/$") {
        dir_suffix = true;
        anchored_end = true;
        s = rest;
    } else if let Some(rest) = s.strip_suffix('$') {
        anchored_end = true;
        s = rest;
    }

    let chars: Vec<char> = s.chars().collect();
    let mut glob = String::new();
    let mut approximate = false;
    let mut i = 0;

    while i < chars.len() {
        // `[^/]*` -> `*`
        if i + 4 < chars.len()
            && chars[i] == '['
            && chars[i + 1] == '^'
            && chars[i + 2] == '/'
            && chars[i + 3] == ']'
            && chars[i + 4] == '*'
        {
            glob.push('*');
            i += 5;
            continue;
        }
        // `[^/]+` -> `?*`
        if i + 4 < chars.len()
            && chars[i] == '['
            && chars[i + 1] == '^'
            && chars[i + 2] == '/'
            && chars[i + 3] == ']'
            && chars[i + 4] == '+'
        {
            glob.push_str("?*");
            i += 5;
            continue;
        }

        // `.*`
        if chars[i] == '.' && chars.get(i + 1) == Some(&'*') {
            if chars.get(i + 2) == Some(&'?') {
                return BranchTranslation::NotTranslatable;
            }
            let remaining: String = chars[i + 2..].iter().collect();
            if glob.is_empty() && !remaining.is_empty() {
                if anchored_start {
                    anchored_start = false;
                }
                if remaining.starts_with('/') {
                    glob.push_str("**");
                    approximate = true;
                } else {
                    glob.push('*');
                }
            } else if glob.ends_with('/') {
                if remaining.is_empty() {
                    glob.push_str("**");
                } else {
                    glob.push_str("**/*");
                }
            } else {
                glob.push('*');
                approximate = true;
            }
            i += 2;
            continue;
        }

        // `.+`
        if chars[i] == '.' && chars.get(i + 1) == Some(&'+') {
            if chars.get(i + 2) == Some(&'?') {
                return BranchTranslation::NotTranslatable;
            }
            let remaining_empty = i + 2 >= chars.len();
            if glob.ends_with('/') && remaining_empty {
                glob.push_str("**");
            } else {
                glob.push_str("?*");
            }
            approximate = true;
            i += 2;
            continue;
        }

        // Single `.`
        if chars[i] == '.' {
            if matches!(chars.get(i + 1), Some('?' | '{')) {
                return BranchTranslation::NotTranslatable;
            }
            glob.push('?');
            approximate = true;
            i += 1;
            continue;
        }

        // Escape sequence
        if chars[i] == '\\' {
            let Some(&esc) = chars.get(i + 1) else {
                return BranchTranslation::NotTranslatable;
            };
            let next_q = chars.get(i + 2).copied();
            match esc {
                'd' => match next_q {
                    Some('+' | '*') => {
                        glob.push_str("[0-9]*");
                        approximate = true;
                        i += 3;
                    }
                    Some('?' | '{') => return BranchTranslation::NotTranslatable,
                    _ => {
                        glob.push_str("[0-9]");
                        i += 2;
                    }
                },
                'w' => match next_q {
                    Some('+' | '*') => {
                        glob.push('*');
                        approximate = true;
                        i += 3;
                    }
                    Some('?' | '{') => return BranchTranslation::NotTranslatable,
                    _ => {
                        glob.push_str("[A-Za-z0-9_]");
                        i += 2;
                    }
                },
                '.' | '-' | '_' | '/' | '+' | '^' | '$' | '(' | ')' | '{' | '}' | '|' | '\\'
                | '[' | ']' | '?' | '*' => {
                    if matches!(next_q, Some('*' | '+' | '?' | '{')) {
                        return BranchTranslation::NotTranslatable;
                    }
                    push_glob_literal(&mut glob, esc);
                    i += 2;
                }
                _ => return BranchTranslation::NotTranslatable,
            }
            continue;
        }

        // Character class `[...]`
        if chars[i] == '[' {
            let mut j = i + 1;
            if chars.get(j) == Some(&'^') {
                j += 1;
            }
            if chars.get(j) == Some(&']') {
                j += 1;
            }
            while j < chars.len() && chars[j] != ']' {
                if chars[j] == '\\' {
                    j += 1;
                }
                j += 1;
            }
            if j >= chars.len() {
                return BranchTranslation::NotTranslatable;
            }
            let raw_class: String = chars[i + 1..j].iter().collect();
            let converted = if let Some(neg) = raw_class.strip_prefix('^') {
                format!("[!{neg}]")
            } else {
                format!("[{raw_class}]")
            };
            match chars.get(j + 1) {
                Some('+' | '*') => {
                    glob.push_str(&converted);
                    glob.push('*');
                    approximate = true;
                    i = j + 2;
                }
                Some('?' | '{') => return BranchTranslation::NotTranslatable,
                _ => {
                    glob.push_str(&converted);
                    i = j + 1;
                }
            }
            continue;
        }

        // Unhandled regex meta characters
        if matches!(chars[i], '(' | ')' | '{' | '}' | '|' | '*' | '+' | '?') {
            return BranchTranslation::NotTranslatable;
        }

        // Plain literal
        if matches!(chars.get(i + 1), Some('*' | '+' | '?' | '{')) {
            return BranchTranslation::NotTranslatable;
        }
        glob.push(chars[i]);
        i += 1;
    }

    if glob.trim_matches('/').is_empty() {
        return BranchTranslation::NotTranslatable;
    }

    if !anchored_start && !path_boundary_start && !glob.starts_with('*') {
        glob.insert(0, '*');
    }
    if !anchored_end && !dir_suffix && !glob.ends_with('*') {
        glob.push('*');
    }
    if dir_suffix && !glob.ends_with('/') {
        glob.push('/');
    }

    let has_internal_slash = glob.trim_matches('/').contains('/');
    let final_rule = if anchored_start {
        if glob.starts_with('/') {
            glob
        } else {
            format!("/{glob}")
        }
    } else if has_internal_slash && !glob.starts_with("**/") {
        format!("**/{}", glob.trim_start_matches('/'))
    } else {
        glob.trim_start_matches('/').to_string()
    };

    // Never auto-emit a blanket rule that ignores the entire repo (e.g. `*` or `/**`)
    let bare = final_rule.trim_matches('/');
    if bare.is_empty() || bare == "*" || bare == "**" {
        return BranchTranslation::NotTranslatable;
    }

    if approximate {
        BranchTranslation::Approximate(final_rule)
    } else {
        BranchTranslation::Exact(final_rule)
    }
}

fn push_glob_literal(out: &mut String, ch: char) {
    if matches!(ch, '*' | '?' | '[' | ']' | '\\') {
        out.push('\\');
    }
    out.push(ch);
}

fn resolve_global_excludes_path() -> Option<PathBuf> {
    let mut cmd = std::process::Command::new("git");
    cmd.args(["config", "--global", "--get", "core.excludesfile"]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    if let Ok(out) = cmd.output()
        && out.status.success()
    {
        let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !raw.is_empty() {
            return Some(PathBuf::from(raw));
        }
    }
    directories::UserDirs::new().map(|u| u.home_dir().join(".gitignore_global"))
}
