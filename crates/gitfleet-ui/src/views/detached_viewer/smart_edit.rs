//! Pure-Rust smart code editing helpers for GitFleet Studio (`S1`, `S3`, `S4`):
//! - Bounded Undo / Redo snapshot ring (`EditorSnapshot`)
//! - `Go to Line` (`line[:col]`) parser
//! - Smart `Enter` auto-indentation & bracket block expansion
//! - Multi-line `Tab` / `Shift+Tab` block indent & outdent
//! - Language-aware `Ctrl+/` line & block comment toggling across all 20 `CodeLanguage` variants
//! - `Alt+Up` / `Alt+Down` vertical line movement
//! - Context-aware auto-closing brackets/quotes & closing-delimiter skip-over (Rust lifetime safe!)

use super::io::jump_editor_to_span;
use crate::syntax::CodeLanguage;
use iced::widget::text_editor;

pub(crate) const MAX_UNDO_HISTORY: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EditorSnapshot {
    pub(crate) text: String,
    pub(crate) line: usize,
    pub(crate) col: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SmartEnterPlan {
    /// Paste this newline + indentation string directly at cursor.
    Paste(String),
    /// Paste `"\n<inner>\n<outer>"` between an open/close bracket pair, then move cursor to the
    /// end of the middle indented line.
    SplitBrackets(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoPairDecision {
    Normal,
    SkipOverClosing,
    InsertPair(char),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommentSyntax {
    Line(&'static str),
    Wrap(&'static str, &'static str),
}

fn comment_syntax_for(lang: CodeLanguage) -> CommentSyntax {
    match lang {
        CodeLanguage::Rust
        | CodeLanguage::TypeScript
        | CodeLanguage::JavaScript
        | CodeLanguage::Go
        | CodeLanguage::C
        | CodeLanguage::Cpp
        | CodeLanguage::CSharp
        | CodeLanguage::Java
        | CodeLanguage::Kotlin
        | CodeLanguage::Json
        | CodeLanguage::PlainText => CommentSyntax::Line("//"),
        CodeLanguage::Python
        | CodeLanguage::Toml
        | CodeLanguage::Yaml
        | CodeLanguage::Shell
        | CodeLanguage::Ini => CommentSyntax::Line("#"),
        CodeLanguage::Sql => CommentSyntax::Line("--"),
        CodeLanguage::Html | CodeLanguage::Markdown => CommentSyntax::Wrap("<!--", "-->"),
        CodeLanguage::Css => CommentSyntax::Wrap("/*", "*/"),
    }
}

/// Parses `"42"`, `"42:10"`, or `":42:10"` into 0-based `(line_idx, col_idx)` clamped to `total_lines`.
pub(crate) fn parse_goto_line_spec(spec: &str, total_lines: usize) -> Option<(usize, usize)> {
    let cleaned = spec.trim().trim_start_matches(':').trim();
    if cleaned.is_empty() {
        return None;
    }
    let (line_part, col_part) = match cleaned.split_once(':') {
        Some((l, c)) => (l.trim(), Some(c.trim())),
        None => (cleaned, None),
    };
    let line_1 = line_part.parse::<usize>().ok()?;
    if line_1 == 0 {
        return None;
    }
    let max_line = total_lines.max(1);
    let line_idx = line_1.min(max_line).saturating_sub(1);
    let col_idx = col_part
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<usize>().ok())
        .map(|c| c.saturating_sub(1))
        .unwrap_or(0);
    Some((line_idx, col_idx))
}

fn split_at_char_col(line: &str, char_col: usize) -> (&str, &str) {
    match line.char_indices().nth(char_col) {
        Some((byte_idx, _)) => line.split_at(byte_idx),
        None => (line, ""),
    }
}

/// Computes smart `Enter` indentation for `line_text` at `cursor_col` (0-based char column).
pub(crate) fn smart_enter_plan(
    line_text: &str,
    cursor_col: usize,
    indent_spaces: usize,
) -> SmartEnterPlan {
    let (before, after) = split_at_char_col(line_text, cursor_col);
    let base_indent: String = before
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let unit = " ".repeat(indent_spaces.max(1));

    let trimmed_before = before.trim_end();
    let trimmed_after = after.trim_start();
    let last_ch = trimmed_before.chars().last();
    let next_ch = trimmed_after.chars().next();

    let is_matching_pair = matches!(
        (last_ch, next_ch),
        (Some('{'), Some('}')) | (Some('('), Some(')')) | (Some('['), Some(']'))
    );
    if is_matching_pair {
        return SmartEnterPlan::SplitBrackets(format!("\n{base_indent}{unit}\n{base_indent}"));
    }

    let opens_block = matches!(last_ch, Some('{') | Some('(') | Some('[') | Some(':'));
    if opens_block {
        SmartEnterPlan::Paste(format!("\n{base_indent}{unit}"))
    } else {
        SmartEnterPlan::Paste(format!("\n{base_indent}"))
    }
}

/// Indents (`outdent == false`) or outdents (`outdent == true`) lines `first_line..=last_line`.
/// Returns `(updated_text, first_line_col_delta, last_line_char_len)`.
pub(crate) fn block_indent_text(
    full_text: &str,
    first_line: usize,
    last_line: usize,
    indent_spaces: usize,
    outdent: bool,
) -> (String, isize, usize) {
    let mut lines: Vec<String> = full_text.split('\n').map(str::to_string).collect();
    if lines.is_empty() {
        return (String::new(), 0, 0);
    }
    let lo = first_line.min(lines.len() - 1);
    let hi = last_line.min(lines.len() - 1).max(lo);
    let step = indent_spaces.max(1);
    let unit = " ".repeat(step);
    let single_line = lo == hi;
    let mut first_delta: isize = 0;

    for (idx, line) in lines[lo..=hi].iter_mut().enumerate() {
        if outdent {
            let removed = if line.starts_with('\t') {
                line.remove(0);
                1usize
            } else {
                let spaces = line.chars().take_while(|c| *c == ' ').count().min(step);
                if spaces > 0 {
                    line.drain(..spaces);
                }
                spaces
            };
            if idx == 0 {
                first_delta = -(removed as isize);
            }
        } else if single_line || !line.trim().is_empty() {
            line.insert_str(0, &unit);
            if idx == 0 {
                first_delta = step as isize;
            }
        }
    }

    let last_len = lines[hi].chars().count();
    (lines.join("\n"), first_delta, last_len)
}

/// Toggles comments on `first_line..=last_line` according to `lang`.
/// Returns `(updated_text, first_line_col_delta, last_line_char_len)`.
pub(crate) fn toggle_line_comments(
    full_text: &str,
    first_line: usize,
    last_line: usize,
    lang: CodeLanguage,
) -> (String, isize, usize) {
    let mut lines: Vec<String> = full_text.split('\n').map(str::to_string).collect();
    if lines.is_empty() {
        return (String::new(), 0, 0);
    }
    let lo = first_line.min(lines.len() - 1);
    let hi = last_line.min(lines.len() - 1).max(lo);
    let syntax = comment_syntax_for(lang);
    let mut first_delta: isize = 0;

    let non_empty_indices: Vec<usize> = (lo..=hi)
        .filter(|&i| !lines[i].trim().is_empty())
        .collect();

    if non_empty_indices.is_empty() {
        // Empty line: insert empty comment marker at current indentation
        let marker = match syntax {
            CommentSyntax::Line(p) => format!("{p} "),
            CommentSyntax::Wrap(open, close) => format!("{open}  {close}"),
        };
        let delta = match syntax {
            CommentSyntax::Line(p) => (p.chars().count() + 1) as isize,
            CommentSyntax::Wrap(open, _) => (open.chars().count() + 1) as isize,
        };
        lines[lo].push_str(&marker);
        let last_len = lines[hi].chars().count();
        return (lines.join("\n"), delta, last_len);
    }

    match syntax {
        CommentSyntax::Line(prefix) => {
            let all_commented = non_empty_indices
                .iter()
                .all(|&i| lines[i].trim_start().starts_with(prefix));

            if all_commented {
                for &i in &non_empty_indices {
                    let ws_bytes = lines[i].len() - lines[i].trim_start().len();
                    let rest = &lines[i][ws_bytes..];
                    let strip_len = if rest.starts_with(&format!("{prefix} ")) {
                        prefix.len() + 1
                    } else {
                        prefix.len()
                    };
                    let strip_chars = lines[i][ws_bytes..ws_bytes + strip_len].chars().count();
                    lines[i].drain(ws_bytes..ws_bytes + strip_len);
                    if i == lo {
                        first_delta = -(strip_chars as isize);
                    }
                }
            } else {
                let min_indent_bytes = non_empty_indices
                    .iter()
                    .map(|&i| lines[i].len() - lines[i].trim_start().len())
                    .min()
                    .unwrap_or(0);
                let insert_str = format!("{prefix} ");
                let added_chars = insert_str.chars().count() as isize;
                for &i in &non_empty_indices {
                    let pos = min_indent_bytes.min(lines[i].len());
                    lines[i].insert_str(pos, &insert_str);
                    if i == lo {
                        first_delta = added_chars;
                    }
                }
            }
        }
        CommentSyntax::Wrap(open, close) => {
            let all_wrapped = non_empty_indices.iter().all(|&i| {
                let t = lines[i].trim();
                t.starts_with(open) && t.ends_with(close) && t.len() >= open.len() + close.len()
            });

            if all_wrapped {
                for &i in &non_empty_indices {
                    let ws_prefix: String = lines[i]
                        .chars()
                        .take_while(|c| c.is_whitespace())
                        .collect();
                    let trimmed = lines[i].trim();
                    let inner = &trimmed[open.len()..trimmed.len() - close.len()];
                    let inner_clean = inner
                        .strip_prefix(' ')
                        .unwrap_or(inner);
                    let inner_clean = inner_clean
                        .strip_suffix(' ')
                        .unwrap_or(inner_clean);
                    let old_chars = lines[i].chars().count() as isize;
                    lines[i] = format!("{ws_prefix}{inner_clean}");
                    if i == lo {
                        first_delta = lines[i].chars().count() as isize - old_chars;
                    }
                }
            } else {
                for &i in &non_empty_indices {
                    let ws_bytes = lines[i].len() - lines[i].trim_start().len();
                    let ws = &lines[i][..ws_bytes];
                    let body = lines[i][ws_bytes..].trim_end();
                    lines[i] = format!("{ws}{open} {body} {close}");
                    if i == lo {
                        first_delta = (open.chars().count() + 1) as isize;
                    }
                }
            }
        }
    }

    let last_len = lines[hi].chars().count();
    (lines.join("\n"), first_delta, last_len)
}

/// Moves lines `first_line..=last_line` vertically by `delta` (`-1` up, `+1` down).
/// Returns `Some((updated_text, new_first_line, new_last_line))` if moved.
pub(crate) fn move_lines_vertical(
    full_text: &str,
    first_line: usize,
    last_line: usize,
    delta: isize,
) -> Option<(String, usize, usize)> {
    let mut lines: Vec<&str> = full_text.split('\n').collect();
    if lines.len() <= 1 || delta == 0 {
        return None;
    }
    let lo = first_line.min(lines.len() - 1);
    let hi = last_line.min(lines.len() - 1).max(lo);

    if delta < 0 {
        if lo == 0 {
            return None;
        }
        let above = lines.remove(lo - 1);
        lines.insert(hi, above);
        Some((lines.join("\n"), lo - 1, hi - 1))
    } else {
        if hi + 1 >= lines.len() {
            return None;
        }
        let below = lines.remove(hi + 1);
        lines.insert(lo, below);
        Some((lines.join("\n"), lo + 1, hi + 1))
    }
}

pub(crate) fn auto_pair_close_char(open: char) -> Option<char> {
    match open {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        '\'' => Some('\''),
        '`' => Some('`'),
        _ => None,
    }
}

/// Determines whether typing `ch` at `cursor_col` (with no active selection) should skip over an
/// existing closing delimiter, auto-insert a matching closing delimiter, or type normally.
pub(crate) fn decide_auto_pair(line_text: &str, cursor_col: usize, ch: char) -> AutoPairDecision {
    let (before, after) = split_at_char_col(line_text, cursor_col);
    let prev_ch = before.chars().last();
    let next_ch = after.chars().next();

    let is_escaped = prev_ch == Some('\\');
    if is_escaped {
        return AutoPairDecision::Normal;
    }

    // 1. Skip-over closing delimiter when already right in front of cursor
    if matches!(ch, ')' | ']' | '}' | '"' | '\'' | '`') && next_ch == Some(ch) {
        return AutoPairDecision::SkipOverClosing;
    }

    // 2. Auto-insert matching closing delimiter when safe
    if let Some(close) = auto_pair_close_char(ch) {
        let next_allows_pair = match next_ch {
            None => true,
            Some(c) => {
                c.is_whitespace() || matches!(c, ')' | ']' | '}' | ',' | ';' | ':' | '>')
            }
        };
        if !next_allows_pair {
            return AutoPairDecision::Normal;
        }

        if matches!(ch, '"' | '\'' | '`') {
            // Avoid auto-pairing inside words, contractions ("don't"), or Rust lifetimes ("&'a")
            if prev_ch.is_some_and(|p| p.is_alphanumeric() || p == '_' || (ch == '\'' && p == '&'))
            {
                return AutoPairDecision::Normal;
            }
        }
        return AutoPairDecision::InsertPair(close);
    }

    AutoPairDecision::Normal
}

/// Resolves the active `(first_line, last_line, had_multiline_selection, start_col)` on a mutable
/// `text_editor::Content` by inspecting `selection()` and using `Backspace` on a temporary delete
/// only when a selection exists (caller will replace `content` immediately afterward).
pub(crate) fn resolve_selection_line_range(
    editor: &mut text_editor::Content,
) -> (usize, usize, bool, usize) {
    use text_editor::{Action, Edit};
    let Some(sel) = editor.selection().filter(|s| !s.is_empty()) else {
        let (line, col) = editor.cursor_position();
        return (line, line, false, col);
    };
    let newline_count = sel.matches('\n').count();
    editor.perform(Action::Edit(Edit::Backspace));
    let (start_line, start_col) = editor.cursor_position();
    let mut end_line = start_line + newline_count;
    if newline_count > 0 && sel.ends_with('\n') {
        end_line = end_line.saturating_sub(1).max(start_line);
    }
    (start_line, end_line, true, start_col)
}

/// Selects all text from `(first_line, 0)` through `(last_line, last_line_char_len)`.
pub(crate) fn select_line_block(
    editor: &mut text_editor::Content,
    first_line: usize,
    last_line: usize,
    last_line_char_len: usize,
) {
    use text_editor::{Action, Motion};
    jump_editor_to_span(editor, first_line, 0, 0);
    let line_steps = last_line.saturating_sub(first_line);
    for _ in 0..line_steps {
        let prev = editor.cursor_position();
        editor.perform(Action::Select(Motion::Down));
        if editor.cursor_position() == prev {
            break;
        }
    }
    if line_steps > 0 {
        editor.perform(Action::Select(Motion::End));
    } else {
        for _ in 0..last_line_char_len {
            let prev = editor.cursor_position();
            editor.perform(Action::Select(Motion::Right));
            if editor.cursor_position() == prev {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_goto_line_specs_cleanly() {
        assert_eq!(parse_goto_line_spec("1", 100), Some((0, 0)));
        assert_eq!(parse_goto_line_spec("42:8", 100), Some((41, 7)));
        assert_eq!(parse_goto_line_spec(":999:3", 50), Some((49, 2)));
        assert_eq!(parse_goto_line_spec("0", 50), None);
        assert_eq!(parse_goto_line_spec("", 50), None);
    }

    #[test]
    fn computes_smart_enter_indentation_and_bracket_split() {
        assert_eq!(
            smart_enter_plan("    let x = 1;", 14, 4),
            SmartEnterPlan::Paste("\n    ".into())
        );
        assert_eq!(
            smart_enter_plan("    if ready {", 14, 4),
            SmartEnterPlan::Paste("\n        ".into())
        );
        assert_eq!(
            smart_enter_plan("    fn run() {}", 14, 4),
            SmartEnterPlan::SplitBrackets("\n        \n    ".into())
        );
        assert_eq!(
            smart_enter_plan("def greet():", 12, 4),
            SmartEnterPlan::Paste("\n    ".into())
        );
    }

    #[test]
    fn indents_and_outdents_multiline_blocks_cleanly() {
        let src = "fn main() {\n    let a = 1;\n\n    let b = 2;\n}";
        let (indented, delta_in, _) = block_indent_text(src, 1, 3, 4, false);
        assert_eq!(delta_in, 4);
        assert_eq!(
            indented,
            "fn main() {\n        let a = 1;\n\n        let b = 2;\n}"
        );

        let (outdented, delta_out, _) = block_indent_text(&indented, 1, 3, 4, true);
        assert_eq!(delta_out, -4);
        assert_eq!(outdented, src);
    }

    #[test]
    fn toggles_line_and_wrapped_comments_across_languages() {
        let rust_src = "fn a() {\n    let x = 1;\n    let y = 2;\n}";
        let (commented, d1, _) = toggle_line_comments(rust_src, 1, 2, CodeLanguage::Rust);
        assert_eq!(d1, 3);
        assert_eq!(
            commented,
            "fn a() {\n    // let x = 1;\n    // let y = 2;\n}"
        );
        let (uncommented, d2, _) = toggle_line_comments(&commented, 1, 2, CodeLanguage::Rust);
        assert_eq!(d2, -3);
        assert_eq!(uncommented, rust_src);

        let py_src = "  x = 1\n  y = 2";
        let (py_c, _, _) = toggle_line_comments(py_src, 0, 1, CodeLanguage::Python);
        assert_eq!(py_c, "  # x = 1\n  # y = 2");

        let html_src = "  <div>Hello</div>";
        let (html_c, _, _) = toggle_line_comments(html_src, 0, 0, CodeLanguage::Html);
        assert_eq!(html_c, "  <!-- <div>Hello</div> -->");
        let (html_u, _, _) = toggle_line_comments(&html_c, 0, 0, CodeLanguage::Html);
        assert_eq!(html_u, html_src);
    }

    #[test]
    fn moves_lines_up_and_down_preserving_boundaries() {
        let src = "alpha\nbeta\ngamma\ndelta";
        assert_eq!(move_lines_vertical(src, 0, 0, -1), None);
        assert_eq!(move_lines_vertical(src, 3, 3, 1), None);

        let (down, lo, hi) = move_lines_vertical(src, 0, 1, 1).unwrap();
        assert_eq!((lo, hi), (1, 2));
        assert_eq!(down, "gamma\nalpha\nbeta\ndelta");

        let (back_up, lo2, hi2) = move_lines_vertical(&down, 1, 2, -1).unwrap();
        assert_eq!((lo2, hi2), (0, 1));
        assert_eq!(back_up, src);
    }

    #[test]
    fn decides_auto_pair_and_preserves_rust_lifetimes() {
        assert_eq!(
            decide_auto_pair("fn foo", 6, '('),
            AutoPairDecision::InsertPair(')')
        );
        assert_eq!(
            decide_auto_pair("fn foo()", 7, ')'),
            AutoPairDecision::SkipOverClosing
        );
        // Rust lifetime `&'a` must NOT auto-pair a second quote!
        assert_eq!(
            decide_auto_pair("fn ref_str(&", 12, '\''),
            AutoPairDecision::Normal
        );
        // Normal char literal `let c = '` SHOULD auto-pair!
        assert_eq!(
            decide_auto_pair("let c = ", 8, '\''),
            AutoPairDecision::InsertPair('\'')
        );
    }
}
