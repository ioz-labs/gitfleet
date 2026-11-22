//! Character-accurate selection geometry, double-click word bounds, and clean
//! clipboard text extraction for Unified, Side-by-Side, and Raw diff views.

use super::layout::{DiffLayout, SideRow, UnifiedRow};
use crate::app::{DiffSelectionSide, DiffTextSelection};
use crate::theme::{self, type_scale};
use gitfleet_git::ParsedDiff;
use iced::advanced::text as adv_text;
use iced::widget::text;
use iced::{Renderer, Size, alignment};
use std::sync::OnceLock;

pub(crate) fn diff_code_char_width() -> f32 {
    static CHAR_W: OnceLock<f32> = OnceLock::new();
    *CHAR_W.get_or_init(|| {
        use adv_text::{Paragraph as _, Renderer as TextRenderer};
        type Para = <Renderer as TextRenderer>::Paragraph;
        let sample = "MMMMMMMMMM";
        let para = Para::with_text(adv_text::Text {
            content: sample,
            bounds: Size::INFINITY,
            size: iced::Pixels(type_scale::CODE),
            line_height: text::LineHeight::default(),
            font: theme::CODE_FONT,
            horizontal_alignment: alignment::Horizontal::Left,
            vertical_alignment: alignment::Vertical::Top,
            shaping: text::Shaping::Basic,
            wrapping: text::Wrapping::None,
        });
        let w = para.min_width() / sample.len() as f32;
        if w.is_finite() && w > 3.0 && w < 20.0 {
            w
        } else {
            7.8
        }
    })
}

fn measure_code_prefix(prefix: &str) -> f32 {
    if prefix.is_empty() {
        return 0.0;
    }
    use adv_text::{Paragraph as _, Renderer as TextRenderer};
    type Para = <Renderer as TextRenderer>::Paragraph;
    let para = Para::with_text(adv_text::Text {
        content: prefix,
        bounds: Size::INFINITY,
        size: iced::Pixels(type_scale::CODE),
        line_height: text::LineHeight::default(),
        font: theme::CODE_FONT,
        horizontal_alignment: alignment::Horizontal::Left,
        vertical_alignment: alignment::Vertical::Top,
        shaping: text::Shaping::Advanced,
        wrapping: text::Wrapping::None,
    });
    para.min_width()
}

fn char_to_byte_idx(s: &str, char_idx: usize) -> usize {
    if char_idx == 0 {
        return 0;
    }
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

pub(crate) fn col_to_x(line: &str, col: usize, char_w: f32) -> f32 {
    if col == 0 {
        return 0.0;
    }
    if line.is_ascii() {
        col.min(line.len()) as f32 * char_w
    } else {
        let b = char_to_byte_idx(line, col);
        measure_code_prefix(&line[..b])
    }
}

pub(crate) fn x_to_col(line: &str, rel_x: f32, char_w: f32) -> usize {
    if rel_x <= 0.0 || line.is_empty() {
        return 0;
    }
    if line.is_ascii() {
        (rel_x / char_w).round().clamp(0.0, line.len() as f32) as usize
    } else {
        let char_count = line.chars().count();
        let mut lo = 0usize;
        let mut hi = char_count;
        while lo < hi {
            let mid = (lo + hi) / 2;
            let w = col_to_x(line, mid, char_w);
            if w < rel_x {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            return 0;
        }
        let w_prev = col_to_x(line, lo - 1, char_w);
        let w_cur = col_to_x(line, lo, char_w);
        if (rel_x - w_prev) <= (w_cur - rel_x) {
            lo - 1
        } else {
            lo
        }
    }
}

/// Returns `(start_col, end_col, extends_eol)` for a code row if it intersects `selection`.
pub(crate) fn row_selection_range(
    selection: Option<DiffTextSelection>,
    side: DiffSelectionSide,
    row_idx: usize,
    char_len: usize,
) -> Option<(usize, usize, bool)> {
    let sel = selection?;
    if sel.side != side || sel.is_empty() {
        return None;
    }
    let (start, end) = sel.ordered();
    if row_idx < start.row || row_idx > end.row {
        return None;
    }
    if start.row == end.row {
        let s = start.col.min(char_len);
        let e = end.col.min(char_len);
        if s < e { Some((s, e, false)) } else { None }
    } else if row_idx == start.row {
        Some((start.col.min(char_len), char_len, true))
    } else if row_idx == end.row {
        Some((0, end.col.min(char_len), false))
    } else {
        Some((0, char_len, true))
    }
}

/// Computes `[start_col, end_col)` of the identifier/token at `col` for double-click word selection.
pub(crate) fn word_bounds_at(line: &str, col: usize) -> (usize, usize) {
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        return (0, 0);
    }
    let idx = col.min(chars.len().saturating_sub(1));
    let classify = |c: char| -> u8 {
        if c.is_alphanumeric() || c == '_' {
            1
        } else if c.is_whitespace() {
            2
        } else {
            3
        }
    };
    let cls = classify(chars[idx]);
    let mut start = idx;
    while start > 0 && classify(chars[start - 1]) == cls {
        start -= 1;
    }
    let mut end = idx + 1;
    while end < chars.len() && classify(chars[end]) == cls {
        end += 1;
    }
    (start, end)
}

pub(crate) fn diff_line_text_at(
    parsed: &ParsedDiff,
    layout: &DiffLayout,
    side: DiffSelectionSide,
    row_idx: usize,
) -> Option<String> {
    match side {
        DiffSelectionSide::Left => match layout.left_rows.get(row_idx)? {
            SideRow::Code { text, .. } => Some(text.clone()),
            _ => None,
        },
        DiffSelectionSide::Right => match layout.right_rows.get(row_idx)? {
            SideRow::Code { text, .. } => Some(text.clone()),
            _ => None,
        },
        DiffSelectionSide::Unified => match layout.unified_rows.get(row_idx)? {
            UnifiedRow::Code { text, .. } => Some(text.clone()),
            _ => None,
        },
        DiffSelectionSide::Raw => parsed
            .raw
            .lines()
            .nth(row_idx)
            .map(theme::sanitize_code_line),
    }
}

pub(crate) fn diff_row_count(
    parsed: &ParsedDiff,
    layout: &DiffLayout,
    side: DiffSelectionSide,
) -> usize {
    match side {
        DiffSelectionSide::Left => layout.left_rows.len(),
        DiffSelectionSide::Right => layout.right_rows.len(),
        DiffSelectionSide::Unified => layout.unified_rows.len(),
        DiffSelectionSide::Raw => parsed.raw.lines().take(2000).count(),
    }
}

/// Extracts clean selected code text across a single or multi-line diff selection,
/// stripping line numbers, `+`/`-` diff signs, hidden gap bars, and hunk headers.
pub(crate) fn extract_selected_diff_text(
    parsed: &ParsedDiff,
    layout: &DiffLayout,
    sel: DiffTextSelection,
) -> String {
    if sel.is_empty() {
        return String::new();
    }
    let (start, end) = sel.ordered();
    let mut parts: Vec<String> = Vec::new();
    let total = diff_row_count(parsed, layout, sel.side);
    if total == 0 {
        return String::new();
    }
    let last_row = end.row.min(total.saturating_sub(1));
    for r in start.row..=last_row {
        let Some(line) = diff_line_text_at(parsed, layout, sel.side, r) else {
            continue;
        };
        let char_len = line.chars().count();
        let (s_col, e_col) = if start.row == end.row {
            (start.col.min(char_len), end.col.min(char_len))
        } else if r == start.row {
            (start.col.min(char_len), char_len)
        } else if r == end.row {
            (0, end.col.min(char_len))
        } else {
            (0, char_len)
        };
        let b0 = char_to_byte_idx(&line, s_col);
        let b1 = char_to_byte_idx(&line, e_col);
        if b0 <= b1 {
            parts.push(line[b0..b1].to_string());
        }
    }
    parts.join("\n")
}
