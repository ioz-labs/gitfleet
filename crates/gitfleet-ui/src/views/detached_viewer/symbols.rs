//! Pure-Rust lexical symbol outline extractor (`S7`), live Git line-diff gutter marks (`S5`),
//! and matching-bracket scanner (`S9`) for GitFleet Studio.

use crate::syntax::CodeLanguage;
use gitfleet_git::{DiffBlock, DiffChangeKind, ParsedDiff, diff_two_texts};
use std::collections::HashMap;

// ── S5: Line-Level Git Diff Gutter Marks ─────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineGitMark {
    Added,
    Modified,
    DeletedAbove,
}

/// Extracts 0-based new-file line marks from a `ParsedDiff`.
pub(crate) fn compute_line_git_marks_from_parsed(parsed: &ParsedDiff) -> HashMap<usize, LineGitMark> {
    let mut marks = HashMap::new();
    for block in &parsed.blocks {
        if let DiffBlock::Change(ch) = block {
            match ch.kind {
                DiffChangeKind::Added => {
                    let start = ch.new_start.saturating_sub(1);
                    for i in 0..ch.new_lines.len() {
                        marks.insert(start + i, LineGitMark::Added);
                    }
                }
                DiffChangeKind::Modified => {
                    let start = ch.new_start.saturating_sub(1);
                    for i in 0..ch.new_lines.len() {
                        marks.insert(start + i, LineGitMark::Modified);
                    }
                }
                DiffChangeKind::Removed => {
                    let anchor = ch.new_start.saturating_sub(1);
                    marks.entry(anchor).or_insert(LineGitMark::DeletedAbove);
                }
            }
        }
    }
    marks
}

/// Computes 0-based line marks comparing `base_text` (e.g. `HEAD` or disk snapshot) against
/// `current_text` (live editor buffer). Caps input size at 256 KB so huge files never stall typing.
pub(crate) fn compute_line_git_marks_vs_base(
    base_text: &str,
    current_text: &str,
) -> HashMap<usize, LineGitMark> {
    if base_text == current_text || base_text.len() > 262_144 || current_text.len() > 262_144 {
        return HashMap::new();
    }
    let parsed = diff_two_texts("HEAD", base_text, "Working", current_text);
    compute_line_git_marks_from_parsed(&parsed)
}

// ── S7: Lexical Symbol Outline Extractor ─────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DocumentSymbol {
    pub(crate) name: String,
    pub(crate) kind_badge: &'static str,
    /// 0-based line index
    pub(crate) line_idx: usize,
    pub(crate) indent_level: usize,
}

fn clean_ident(s: &str) -> Option<String> {
    let token: String = s
        .trim_start()
        .chars()
        .take_while(|c| c.is_alphanumeric() || matches!(*c, '_' | '$' | '-' | ':' | '.'))
        .collect();
    let trimmed = token.trim_matches(|c| c == ':' || c == '.');
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn strip_rust_visibility(mut s: &str) -> &str {
    s = s.trim_start();
    if let Some(rest) = s.strip_prefix("pub") {
        let rest = rest.trim_start();
        if let Some(after_paren) = rest.strip_prefix('(') {
            if let Some((_, tail)) = after_paren.split_once(')') {
                return tail.trim_start();
            }
        } else if rest.len() < s.len() - 3 {
            return rest;
        }
    }
    s
}

/// Extracts document symbols (functions, structs, classes, headings, sections) for `Ctrl+Shift+O`
/// and `@symbol` Quick Open navigation.
pub(crate) fn extract_document_symbols(text: &str, lang: CodeLanguage) -> Vec<DocumentSymbol> {
    let mut out = Vec::new();
    for (line_idx, raw_line) in text.lines().enumerate() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let leading_spaces = raw_line
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .map(|c| if c == '\t' { 4 } else { 1 })
            .sum::<usize>();
        let indent_level = leading_spaces / 4;

        match lang {
            CodeLanguage::Rust => {
                if trimmed.starts_with("//") || trimmed.starts_with('#') {
                    continue;
                }
                let mut rest = strip_rust_visibility(trimmed);
                for kw in ["default ", "const ", "async ", "unsafe "] {
                    if let Some(r) = rest.strip_prefix(kw) {
                        rest = r.trim_start();
                    }
                }
                if let Some(r) = rest.strip_prefix("fn ") {
                    if let Some(name) = clean_ident(r) {
                        out.push(DocumentSymbol {
                            name,
                            kind_badge: "fn",
                            line_idx,
                            indent_level,
                        });
                    }
                } else if let Some(r) = rest.strip_prefix("struct ") {
                    if let Some(name) = clean_ident(r) {
                        out.push(DocumentSymbol {
                            name,
                            kind_badge: "struct",
                            line_idx,
                            indent_level,
                        });
                    }
                } else if let Some(r) = rest.strip_prefix("enum ") {
                    if let Some(name) = clean_ident(r) {
                        out.push(DocumentSymbol {
                            name,
                            kind_badge: "enum",
                            line_idx,
                            indent_level,
                        });
                    }
                } else if let Some(r) = rest.strip_prefix("trait ") {
                    if let Some(name) = clean_ident(r) {
                        out.push(DocumentSymbol {
                            name,
                            kind_badge: "trait",
                            line_idx,
                            indent_level,
                        });
                    }
                } else if let Some(r) = rest.strip_prefix("impl") {
                    let after = r.trim_start();
                    let after_gen = if after.starts_with('<') {
                        after
                            .split_once('>')
                            .map(|(_, t)| t.trim_start())
                            .unwrap_or(after)
                    } else {
                        after
                    };
                    let target = after_gen
                        .split('{')
                        .next()
                        .unwrap_or(after_gen)
                        .split("where")
                        .next()
                        .unwrap_or(after_gen)
                        .trim();
                    if !target.is_empty() {
                        out.push(DocumentSymbol {
                            name: target.to_string(),
                            kind_badge: "impl",
                            line_idx,
                            indent_level,
                        });
                    }
                } else if let Some(r) = rest.strip_prefix("mod ") {
                    if let Some(name) = clean_ident(r) {
                        out.push(DocumentSymbol {
                            name,
                            kind_badge: "mod",
                            line_idx,
                            indent_level,
                        });
                    }
                } else if let Some(r) = rest.strip_prefix("type ") {
                    if let Some(name) = clean_ident(r) {
                        out.push(DocumentSymbol {
                            name,
                            kind_badge: "type",
                            line_idx,
                            indent_level,
                        });
                    }
                }
            }
            CodeLanguage::TypeScript | CodeLanguage::JavaScript => {
                if trimmed.starts_with("//") || trimmed.starts_with('*') {
                    continue;
                }
                let mut rest = trimmed;
                for kw in ["export ", "default ", "declare ", "abstract ", "async "] {
                    if let Some(r) = rest.strip_prefix(kw) {
                        rest = r.trim_start();
                    }
                }
                for (prefix, badge) in [
                    ("function ", "fn"),
                    ("class ", "class"),
                    ("interface ", "iface"),
                    ("type ", "type"),
                    ("enum ", "enum"),
                    ("const ", "const"),
                ] {
                    if let Some(r) = rest.strip_prefix(prefix) {
                        if let Some(name) = clean_ident(r) {
                            out.push(DocumentSymbol {
                                name,
                                kind_badge: badge,
                                line_idx,
                                indent_level,
                            });
                        }
                        break;
                    }
                }
            }
            CodeLanguage::Python => {
                if trimmed.starts_with('#') {
                    continue;
                }
                let rest = trimmed.strip_prefix("async ").unwrap_or(trimmed).trim_start();
                if let Some(r) = rest.strip_prefix("def ") {
                    if let Some(name) = clean_ident(r) {
                        out.push(DocumentSymbol {
                            name,
                            kind_badge: "def",
                            line_idx,
                            indent_level,
                        });
                    }
                } else if let Some(r) = rest.strip_prefix("class ")
                    && let Some(name) = clean_ident(r)
                {
                    out.push(DocumentSymbol {
                        name,
                        kind_badge: "class",
                        line_idx,
                        indent_level,
                    });
                }
            }
            CodeLanguage::Go => {
                if let Some(r) = trimmed.strip_prefix("func ") {
                    let after_recv = if r.trim_start().starts_with('(') {
                        r.split_once(')').map(|(_, t)| t.trim_start()).unwrap_or(r)
                    } else {
                        r
                    };
                    if let Some(name) = clean_ident(after_recv) {
                        out.push(DocumentSymbol {
                            name,
                            kind_badge: "func",
                            line_idx,
                            indent_level,
                        });
                    }
                } else if let Some(r) = trimmed.strip_prefix("type ")
                    && let Some(name) = clean_ident(r)
                {
                    out.push(DocumentSymbol {
                        name,
                        kind_badge: "type",
                        line_idx,
                        indent_level,
                    });
                }
            }
            CodeLanguage::C
            | CodeLanguage::Cpp
            | CodeLanguage::CSharp
            | CodeLanguage::Java
            | CodeLanguage::Kotlin => {
                if trimmed.starts_with("//") || trimmed.starts_with('*') || trimmed.starts_with('#')
                {
                    continue;
                }
                for (kw, badge) in [
                    ("class ", "class"),
                    ("struct ", "struct"),
                    ("interface ", "iface"),
                    ("enum ", "enum"),
                    ("fun ", "fun"),
                    ("namespace ", "ns"),
                ] {
                    if let Some(idx) = trimmed.find(kw)
                        && (idx == 0 || trimmed.as_bytes()[idx - 1].is_ascii_whitespace())
                        && let Some(name) = clean_ident(&trimmed[idx + kw.len()..])
                    {
                        out.push(DocumentSymbol {
                            name,
                            kind_badge: badge,
                            line_idx,
                            indent_level,
                        });
                        break;
                    }
                }
            }
            CodeLanguage::Markdown => {
                if trimmed.starts_with('#') {
                    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
                    if (1..=5).contains(&hashes) {
                        let heading = trimmed[hashes..].trim();
                        if !heading.is_empty() {
                            let badge = match hashes {
                                1 => "H1",
                                2 => "H2",
                                3 => "H3",
                                _ => "H4",
                            };
                            out.push(DocumentSymbol {
                                name: heading.to_string(),
                                kind_badge: badge,
                                line_idx,
                                indent_level: hashes.saturating_sub(1),
                            });
                        }
                    }
                }
            }
            CodeLanguage::Toml | CodeLanguage::Ini => {
                if trimmed.starts_with('[') && trimmed.ends_with(']') {
                    let sec = trimmed.trim_matches(|c| c == '[' || c == ']').trim();
                    if !sec.is_empty() {
                        out.push(DocumentSymbol {
                            name: sec.to_string(),
                            kind_badge: "sec",
                            line_idx,
                            indent_level: 0,
                        });
                    }
                }
            }
            CodeLanguage::Json | CodeLanguage::Yaml => {
                if indent_level <= 1 {
                    if let Some((k, _)) = trimmed.split_once(':') {
                        let key = k.trim().trim_matches('"').trim_matches('\'');
                        if !key.is_empty() && !key.starts_with('#') && !key.starts_with('{') {
                            out.push(DocumentSymbol {
                                name: key.to_string(),
                                kind_badge: "key",
                                line_idx,
                                indent_level,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

// ── S9: Matching Bracket Scanner ─────────────────────────────────────────────

fn bracket_partner(ch: char) -> Option<(char, bool)> {
    match ch {
        '(' => Some((')', true)),
        '[' => Some((']', true)),
        '{' => Some(('}', true)),
        ')' => Some(('(', false)),
        ']' => Some(('[', false)),
        '}' => Some(('{', false)),
        _ => None,
    }
}

/// Finds a bracket at `(cursor_line, cursor_col)` (or immediately left at `cursor_col - 1`) and
/// scans up to `max_scan_lines` lines to locate its matching partner.
/// Returns `Some(((line_a, char_col_a), (line_b, char_col_b)))`.
pub(crate) fn find_matching_bracket_in_lines<S: AsRef<str>>(
    lines: &[S],
    cursor_line: usize,
    cursor_col: usize,
    max_scan_lines: usize,
) -> Option<((usize, usize), (usize, usize))> {
    let cur_str = lines.get(cursor_line)?.as_ref();
    let cur_chars: Vec<char> = cur_str.chars().collect();

    let (anchor_col, anchor_ch, target_ch, forward) = if let Some(&ch) = cur_chars.get(cursor_col)
        && let Some((partner, fwd)) = bracket_partner(ch)
    {
        (cursor_col, ch, partner, fwd)
    } else if cursor_col > 0
        && let Some(&ch) = cur_chars.get(cursor_col - 1)
        && let Some((partner, fwd)) = bracket_partner(ch)
    {
        (cursor_col - 1, ch, partner, fwd)
    } else {
        return None;
    };

    let mut depth: i32 = 0;
    if forward {
        let end_line = (cursor_line + max_scan_lines).min(lines.len().saturating_sub(1));
        for ln in cursor_line..=end_line {
            let chars: Vec<char> = lines[ln].as_ref().chars().collect();
            let start_c = if ln == cursor_line { anchor_col } else { 0 };
            for (col, &c) in chars.iter().enumerate().skip(start_c) {
                if c == anchor_ch {
                    depth += 1;
                } else if c == target_ch {
                    depth -= 1;
                    if depth == 0 {
                        return Some(((cursor_line, anchor_col), (ln, col)));
                    }
                }
            }
        }
    } else {
        let min_line = cursor_line.saturating_sub(max_scan_lines);
        for ln in (min_line..=cursor_line).rev() {
            let chars: Vec<char> = lines[ln].as_ref().chars().collect();
            let end_c = if ln == cursor_line {
                anchor_col + 1
            } else {
                chars.len()
            };
            for col in (0..end_c.min(chars.len())).rev() {
                let c = chars[col];
                if c == anchor_ch {
                    depth += 1;
                } else if c == target_ch {
                    depth -= 1;
                    if depth == 0 {
                        return Some(((ln, col), (cursor_line, anchor_col)));
                    }
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_rust_and_markdown_symbols_cleanly() {
        let rs = concat!(
            "pub struct Config {\n",
            "    pub port: u16,\n",
            "}\n",
            "impl Config {\n",
            "    pub(crate) async fn connect(&self) {}\n",
            "}\n",
        );
        let syms = extract_document_symbols(rs, CodeLanguage::Rust);
        assert_eq!(syms.len(), 3);
        assert_eq!((syms[0].kind_badge, syms[0].name.as_str(), syms[0].line_idx), ("struct", "Config", 0));
        assert_eq!((syms[1].kind_badge, syms[1].name.as_str(), syms[1].line_idx), ("impl", "Config", 3));
        assert_eq!((syms[2].kind_badge, syms[2].name.as_str(), syms[2].line_idx), ("fn", "connect", 4));

        let md = "# Title\ntext\n## Section Two\n";
        let md_syms = extract_document_symbols(md, CodeLanguage::Markdown);
        assert_eq!(md_syms.len(), 2);
        assert_eq!((md_syms[0].kind_badge, md_syms[0].name.as_str()), ("H1", "Title"));
        assert_eq!((md_syms[1].kind_badge, md_syms[1].name.as_str()), ("H2", "Section Two"));
    }

    #[test]
    fn computes_git_line_marks_for_added_modified_and_deleted_lines() {
        let base = "alpha\nbeta\ngamma\ndelta\n";
        let edited = "alpha\nbeta_mod\ndelta\nepsilon\n";
        let marks = compute_line_git_marks_vs_base(base, edited);
        assert_eq!(marks.get(&1), Some(&LineGitMark::Modified));
        assert_eq!(marks.get(&3), Some(&LineGitMark::Added));
    }

    #[test]
    fn finds_matching_brackets_forward_and_backward() {
        let lines = vec!["fn demo(x: (i32, i32)) {", "    if x.0 > 0 {", "    }", "}"];
        assert_eq!(
            find_matching_bracket_in_lines(&lines, 0, 23, 50),
            Some(((0, 23), (3, 0)))
        );
        assert_eq!(
            find_matching_bracket_in_lines(&lines, 3, 1, 50),
            Some(((0, 23), (3, 0)))
        );
    }
}
