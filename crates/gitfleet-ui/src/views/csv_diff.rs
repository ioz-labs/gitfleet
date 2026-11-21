use crate::app::{GitFleetApp, Message};
use crate::icons;
use crate::theme::{self, Palette, ThemeMode, type_scale};
use gitfleet_git::FileVersionPair;
use iced::widget::{
    Space, button, column, container, horizontal_rule, row, text, text_input,
};
use iced::{Alignment, Background, Border, Color, Element, Length, alignment};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub(crate) enum CsvDiffMsg {
    SetChangesOnly(bool),
    ToggleIgnoreFormat,
    FilterChanged(String),
}

const MAX_RENDERED_CSV_ROWS: usize = 1500;
const ROW_HEIGHT: f32 = 25.0;
const GUTTER_W: f32 = 44.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CsvDiffRow {
    Unchanged {
        old_row_no: usize,
        new_row_no: usize,
        cells: Vec<String>,
    },
    Removed {
        old_row_no: usize,
        cells: Vec<String>,
    },
    Added {
        new_row_no: usize,
        cells: Vec<String>,
    },
    Modified {
        old_row_no: usize,
        new_row_no: usize,
        old_cells: Vec<String>,
        new_cells: Vec<String>,
        changed_cols: Vec<bool>,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CsvDiffStats {
    pub(crate) added_rows: usize,
    pub(crate) removed_rows: usize,
    pub(crate) modified_rows: usize,
    pub(crate) changed_cells: usize,
    pub(crate) unchanged_rows: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct CsvDiffData {
    pub(crate) file_path: String,
    pub(crate) delimiter: char,
    pub(crate) before_label: String,
    pub(crate) after_label: String,
    pub(crate) before_line_count: usize,
    pub(crate) after_line_count: usize,
    pub(crate) before_byte_len: usize,
    pub(crate) after_byte_len: usize,
    pub(crate) headers: Vec<String>,
    pub(crate) old_headers: Option<Vec<String>>,
    pub(crate) header_changed_cols: Vec<bool>,
    pub(crate) col_widths: Vec<f32>,
    pub(crate) rows: Vec<CsvDiffRow>,
    pub(crate) stats: CsvDiffStats,
}

pub(crate) fn is_csv_diff_path(path: &str) -> bool {
    let Some(ext) = path.rsplit('.').next() else {
        return false;
    };
    ext.eq_ignore_ascii_case("csv") || ext.eq_ignore_ascii_case("tsv")
}

fn format_bytes(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

pub(crate) fn detect_delimiter(file_path: &str, sample: &str) -> char {
    if file_path
        .rsplit('.')
        .next()
        .is_some_and(|e| e.eq_ignore_ascii_case("tsv"))
    {
        return '\t';
    }
    let first_line = sample
        .trim_start_matches('\u{feff}')
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");
    let mut commas = 0usize;
    let mut semis = 0usize;
    let mut tabs = 0usize;
    let mut in_quotes = false;
    for ch in first_line.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => commas += 1,
            ';' if !in_quotes => semis += 1,
            '\t' if !in_quotes => tabs += 1,
            _ => {}
        }
    }
    if commas == 0 && semis > 0 && semis >= tabs {
        ';'
    } else if commas == 0 && tabs > 0 {
        '\t'
    } else {
        ','
    }
}

/// RFC-4180 compliant delimited table parser handling quoted fields, escaped `""`, CRLF, and
/// embedded newlines.
pub(crate) fn parse_csv_records(raw: &str, delimiter: char) -> Vec<Vec<String>> {
    let input = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    if input.is_empty() {
        return Vec::new();
    }

    let mut records: Vec<Vec<String>> = Vec::new();
    let mut cur_row: Vec<String> = Vec::new();
    let mut cur_field = String::new();
    let mut in_quotes = false;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cur_field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                cur_field.push(ch);
            }
        } else if ch == '"' && cur_field.is_empty() {
            in_quotes = true;
        } else if ch == delimiter {
            cur_row.push(std::mem::take(&mut cur_field));
        } else if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            cur_row.push(std::mem::take(&mut cur_field));
            records.push(std::mem::take(&mut cur_row));
        } else if ch == '\n' {
            cur_row.push(std::mem::take(&mut cur_field));
            records.push(std::mem::take(&mut cur_row));
        } else {
            cur_field.push(ch);
        }
    }

    if !cur_field.is_empty() || !cur_row.is_empty() {
        cur_row.push(cur_field);
        records.push(cur_row);
    }

    // Drop completely empty trailing lines caused by trailing blank newlines
    while records
        .last()
        .is_some_and(|r| r.len() == 1 && r[0].is_empty())
    {
        records.pop();
    }

    records
}

fn sanitize_cell(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\r' => {}
            '\n' => out.push_str(" ↵ "),
            '\t' => out.push_str("  "),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

fn pad_row(cells: &[String], col_count: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(col_count);
    for idx in 0..col_count {
        out.push(
            cells
                .get(idx)
                .map(|s| sanitize_cell(s))
                .unwrap_or_default(),
        );
    }
    out
}

fn row_similarity(a: &[String], b: &[String]) -> (usize, usize) {
    let n = a.len().max(b.len());
    let mut same = 0usize;
    let mut non_empty = 0usize;
    for i in 0..n {
        let sa = a.get(i).map(String::as_str).unwrap_or("");
        let sb = b.get(i).map(String::as_str).unwrap_or("");
        if !sa.is_empty() || !sb.is_empty() {
            non_empty += 1;
            if sa == sb {
                same += 1;
            }
        }
    }
    (same, non_empty)
}

fn emit_changed_hunk(
    old_items: &[(usize, Vec<String>)],
    new_items: &[(usize, Vec<String>)],
    col_count: usize,
    out: &mut Vec<CsvDiffRow>,
    stats: &mut CsvDiffStats,
) {
    if old_items.is_empty() {
        for (n_no, cells) in new_items {
            out.push(CsvDiffRow::Added {
                new_row_no: *n_no,
                cells: pad_row(cells, col_count),
            });
            stats.added_rows += 1;
        }
        return;
    }
    if new_items.is_empty() {
        for (o_no, cells) in old_items {
            out.push(CsvDiffRow::Removed {
                old_row_no: *o_no,
                cells: pad_row(cells, col_count),
            });
            stats.removed_rows += 1;
        }
        return;
    }

    // Match modified pairs inside this hunk:
    // 1) Exact first-column key match when unique inside the hunk (e.g. SKU / ID column)
    // 2) Best cell-similarity match (>= 1 matching non-empty cell and >= 30% overlap)
    let mut old_to_new: Vec<Option<usize>> = vec![None; old_items.len()];
    let mut new_used: Vec<bool> = vec![false; new_items.len()];

    let mut old_key_counts: HashMap<&str, usize> = HashMap::new();
    let mut new_key_counts: HashMap<&str, usize> = HashMap::new();
    for (_, r) in old_items {
        if let Some(k) = r.first().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            *old_key_counts.entry(k).or_insert(0) += 1;
        }
    }
    for (_, r) in new_items {
        if let Some(k) = r.first().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            *new_key_counts.entry(k).or_insert(0) += 1;
        }
    }

    for (oi, (_, orow)) in old_items.iter().enumerate() {
        let Some(ok) = orow.first().map(|s| s.trim()).filter(|s| !s.is_empty()) else {
            continue;
        };
        if old_key_counts.get(ok) == Some(&1)
            && new_key_counts.get(ok) == Some(&1)
            && let Some(ni) = new_items
                .iter()
                .position(|(_, nrow)| nrow.first().map(|s| s.trim()) == Some(ok))
            && !new_used[ni]
        {
            old_to_new[oi] = Some(ni);
            new_used[ni] = true;
        }
    }

    // Second pass: greedy similarity matching for unmatched rows (e.g., when column 0 itself changed)
    for (oi, (_, orow)) in old_items.iter().enumerate() {
        if old_to_new[oi].is_some() {
            continue;
        }
        let mut best: Option<(usize, usize)> = None; // (new_idx, matching_cells)
        for (ni, (_, nrow)) in new_items.iter().enumerate() {
            if new_used[ni] {
                continue;
            }
            let (same, non_empty) = row_similarity(orow, nrow);
            if same > 0
                && same * 10 >= non_empty.max(1) * 3
                && best.is_none_or(|(_, best_same)| same > best_same)
            {
                best = Some((ni, same));
            }
        }
        if let Some((ni, _)) = best {
            old_to_new[oi] = Some(ni);
            new_used[ni] = true;
        }
    }

    // Emit rows in stable interleaved order so added rows land near their logical neighbors
    let mut next_unmatched_new = 0usize;
    for (oi, (o_no, orow)) in old_items.iter().enumerate() {
        if let Some(ni) = old_to_new[oi] {
            while next_unmatched_new < ni {
                if !new_used[next_unmatched_new] {
                    let (n_no, nrow) = &new_items[next_unmatched_new];
                    out.push(CsvDiffRow::Added {
                        new_row_no: *n_no,
                        cells: pad_row(nrow, col_count),
                    });
                    stats.added_rows += 1;
                    new_used[next_unmatched_new] = true;
                }
                next_unmatched_new += 1;
            }
            let (n_no, nrow) = &new_items[ni];
            let old_p = pad_row(orow, col_count);
            let new_p = pad_row(nrow, col_count);
            let changed_cols: Vec<bool> = old_p
                .iter()
                .zip(new_p.iter())
                .map(|(a, b)| a != b)
                .collect();
            stats.changed_cells += changed_cols.iter().filter(|&&c| c).count();
            stats.modified_rows += 1;
            out.push(CsvDiffRow::Modified {
                old_row_no: *o_no,
                new_row_no: *n_no,
                old_cells: old_p,
                new_cells: new_p,
                changed_cols,
            });
            if next_unmatched_new == ni {
                next_unmatched_new += 1;
            }
        } else {
            out.push(CsvDiffRow::Removed {
                old_row_no: *o_no,
                cells: pad_row(orow, col_count),
            });
            stats.removed_rows += 1;
        }
    }

    for (ni, (n_no, nrow)) in new_items.iter().enumerate() {
        if !new_used[ni] {
            out.push(CsvDiffRow::Added {
                new_row_no: *n_no,
                cells: pad_row(nrow, col_count),
            });
            stats.added_rows += 1;
        }
    }
}

/// Computes a structured spreadsheet-style CSV/TSV diff with key-aware row pairing and exact
/// cell-level change highlights.
pub(crate) fn compute_csv_diff(pair: FileVersionPair) -> CsvDiffData {
    let before_byte_len = pair.before_bytes.as_ref().map(|b| b.len()).unwrap_or(0);
    let after_byte_len = pair.after_bytes.as_ref().map(|b| b.len()).unwrap_or(0);

    let before_text = pair
        .before_bytes
        .as_deref()
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default();
    let after_text = pair
        .after_bytes
        .as_deref()
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default();

    let sample = if !after_text.is_empty() {
        after_text.as_str()
    } else {
        before_text.as_str()
    };
    let delimiter = detect_delimiter(&pair.file_path, sample);

    let before_records = parse_csv_records(&before_text, delimiter);
    let after_records = parse_csv_records(&after_text, delimiter);
    let before_line_count = before_records.len();
    let after_line_count = after_records.len();

    let (before_hdr, before_body) = if before_records.is_empty() {
        (Vec::new(), &before_records[..])
    } else {
        (before_records[0].clone(), &before_records[1..])
    };
    let (after_hdr, after_body) = if after_records.is_empty() {
        (Vec::new(), &after_records[..])
    } else {
        (after_records[0].clone(), &after_records[1..])
    };

    let col_count = before_hdr
        .len()
        .max(after_hdr.len())
        .max(before_body.iter().map(|r| r.len()).max().unwrap_or(0))
        .max(after_body.iter().map(|r| r.len()).max().unwrap_or(0))
        .max(1);

    let before_hdr_p = if before_hdr.is_empty() {
        Vec::new()
    } else {
        pad_row(&before_hdr, col_count)
    };
    let after_hdr_p = if after_hdr.is_empty() {
        Vec::new()
    } else {
        pad_row(&after_hdr, col_count)
    };

    let (headers, old_headers, header_changed_cols) = match (
        before_hdr_p.is_empty(),
        after_hdr_p.is_empty(),
    ) {
        (true, true) => (
            (1..=col_count).map(|i| format!("col_{i}")).collect(),
            None,
            vec![false; col_count],
        ),
        (true, false) => (after_hdr_p, None, vec![false; col_count]),
        (false, true) => (before_hdr_p, None, vec![false; col_count]),
        (false, false) => {
            let chg: Vec<bool> = before_hdr_p
                .iter()
                .zip(after_hdr_p.iter())
                .map(|(a, b)| a != b)
                .collect();
            let any = chg.iter().any(|&c| c);
            (
                after_hdr_p,
                if any { Some(before_hdr_p) } else { None },
                chg,
            )
        }
    };

    // Align identical rows via prefix/suffix trimming + LCS DP (with linear fallback on huge tables)
    let mut prefix = 0usize;
    while prefix < before_body.len()
        && prefix < after_body.len()
        && before_body[prefix] == after_body[prefix]
    {
        prefix += 1;
    }
    let mut suffix = 0usize;
    while suffix < before_body.len() - prefix
        && suffix < after_body.len() - prefix
        && before_body[before_body.len() - 1 - suffix]
            == after_body[after_body.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let mut rows: Vec<CsvDiffRow> = Vec::new();
    let mut stats = CsvDiffStats::default();

    for (idx, row_cells) in after_body.iter().enumerate().take(prefix) {
        rows.push(CsvDiffRow::Unchanged {
            old_row_no: idx + 2,
            new_row_no: idx + 2,
            cells: pad_row(row_cells, col_count),
        });
        stats.unchanged_rows += 1;
    }

    let mid_old = &before_body[prefix..before_body.len() - suffix];
    let mid_new = &after_body[prefix..after_body.len() - suffix];

    if !mid_old.is_empty() || !mid_new.is_empty() {
        let m = mid_old.len();
        let n = mid_new.len();
        if m == 0 || n == 0 {
            let o_items: Vec<(usize, Vec<String>)> = mid_old
                .iter()
                .enumerate()
                .map(|(i, r)| (prefix + i + 2, r.clone()))
                .collect();
            let n_items: Vec<(usize, Vec<String>)> = mid_new
                .iter()
                .enumerate()
                .map(|(j, r)| (prefix + j + 2, r.clone()))
                .collect();
            emit_changed_hunk(&o_items, &n_items, col_count, &mut rows, &mut stats);
        } else if m * n <= 360_000 {
            // Exact LCS alignment on middle rows
            let cols = n + 1;
            let mut dp = vec![0u32; (m + 1) * cols];
            for i in (0..m).rev() {
                for j in (0..n).rev() {
                    dp[i * cols + j] = if mid_old[i] == mid_new[j] {
                        1 + dp[(i + 1) * cols + (j + 1)]
                    } else {
                        dp[(i + 1) * cols + j].max(dp[i * cols + (j + 1)])
                    };
                }
            }
            let mut i = 0usize;
            let mut j = 0usize;
            let mut hunk_old: Vec<(usize, Vec<String>)> = Vec::new();
            let mut hunk_new: Vec<(usize, Vec<String>)> = Vec::new();
            while i < m && j < n {
                if mid_old[i] == mid_new[j] {
                    if !hunk_old.is_empty() || !hunk_new.is_empty() {
                        emit_changed_hunk(&hunk_old, &hunk_new, col_count, &mut rows, &mut stats);
                        hunk_old.clear();
                        hunk_new.clear();
                    }
                    rows.push(CsvDiffRow::Unchanged {
                        old_row_no: prefix + i + 2,
                        new_row_no: prefix + j + 2,
                        cells: pad_row(&mid_new[j], col_count),
                    });
                    stats.unchanged_rows += 1;
                    i += 1;
                    j += 1;
                } else if dp[(i + 1) * cols + j] >= dp[i * cols + (j + 1)] {
                    hunk_old.push((prefix + i + 2, mid_old[i].clone()));
                    i += 1;
                } else {
                    hunk_new.push((prefix + j + 2, mid_new[j].clone()));
                    j += 1;
                }
            }
            while i < m {
                hunk_old.push((prefix + i + 2, mid_old[i].clone()));
                i += 1;
            }
            while j < n {
                hunk_new.push((prefix + j + 2, mid_new[j].clone()));
                j += 1;
            }
            if !hunk_old.is_empty() || !hunk_new.is_empty() {
                emit_changed_hunk(&hunk_old, &hunk_new, col_count, &mut rows, &mut stats);
            }
        } else {
            // Fast key-anchored hunk emit for very large CSV files
            let o_items: Vec<(usize, Vec<String>)> = mid_old
                .iter()
                .enumerate()
                .map(|(i, r)| (prefix + i + 2, r.clone()))
                .collect();
            let n_items: Vec<(usize, Vec<String>)> = mid_new
                .iter()
                .enumerate()
                .map(|(j, r)| (prefix + j + 2, r.clone()))
                .collect();
            emit_changed_hunk(&o_items, &n_items, col_count, &mut rows, &mut stats);
        }
    }

    for k in 0..suffix {
        let oi = before_body.len() - suffix + k;
        let ni = after_body.len() - suffix + k;
        rows.push(CsvDiffRow::Unchanged {
            old_row_no: oi + 2,
            new_row_no: ni + 2,
            cells: pad_row(&after_body[ni], col_count),
        });
        stats.unchanged_rows += 1;
    }

    // Compute balanced per-column display widths
    let mut max_chars: Vec<usize> = headers.iter().map(|h| h.chars().count().max(4)).collect();
    if let Some(oh) = &old_headers {
        for (i, h) in oh.iter().enumerate() {
            if let Some(slot) = max_chars.get_mut(i) {
                *slot = (*slot).max(h.chars().count());
            }
        }
    }
    for r in rows.iter().take(400) {
        let update_widths = |cells: &[String], widths: &mut [usize]| {
            for (i, cell) in cells.iter().enumerate() {
                if let Some(slot) = widths.get_mut(i) {
                    *slot = (*slot).max(cell.chars().count());
                }
            }
        };
        match r {
            CsvDiffRow::Unchanged { cells, .. }
            | CsvDiffRow::Removed { cells, .. }
            | CsvDiffRow::Added { cells, .. } => update_widths(cells, &mut max_chars),
            CsvDiffRow::Modified {
                old_cells,
                new_cells,
                ..
            } => {
                update_widths(old_cells, &mut max_chars);
                update_widths(new_cells, &mut max_chars);
            }
        }
    }
    let col_widths: Vec<f32> = max_chars
        .into_iter()
        .map(|chars| ((chars as f32) * 8.1 + 22.0).clamp(80.0, 280.0))
        .collect();

    CsvDiffData {
        file_path: pair.file_path,
        delimiter,
        before_label: pair.before_label,
        after_label: pair.after_label,
        before_line_count,
        after_line_count,
        before_byte_len,
        after_byte_len,
        headers,
        old_headers,
        header_changed_cols,
        col_widths,
        rows,
        stats,
    }
}

#[derive(Clone, Copy)]
struct CsvColors {
    row_removed_bg: Color,
    row_added_bg: Color,
    cell_removed_bg: Color,
    cell_removed_fg: Color,
    cell_added_bg: Color,
    cell_added_fg: Color,
    bar_removed: Color,
    bar_added: Color,
    grid_line: Color,
}

fn csv_palette(p: Palette, dark_mode: bool) -> CsvColors {
    if dark_mode {
        CsvColors {
            row_removed_bg: Color::from_rgb(0.220, 0.095, 0.102),
            row_added_bg: Color::from_rgb(0.078, 0.204, 0.129),
            cell_removed_bg: Color::from_rgb(0.953, 0.545, 0.545),
            cell_removed_fg: Color::from_rgb(0.110, 0.035, 0.043),
            cell_added_bg: Color::from_rgb(0.514, 0.890, 0.659),
            cell_added_fg: Color::from_rgb(0.035, 0.137, 0.078),
            bar_removed: Color::from_rgb(0.922, 0.369, 0.388),
            bar_added: Color::from_rgb(0.294, 0.788, 0.506),
            grid_line: Color {
                a: 0.55,
                ..p.border_subtle
            },
        }
    } else {
        CsvColors {
            row_removed_bg: Color::from_rgb(0.992, 0.918, 0.918),
            row_added_bg: Color::from_rgb(0.894, 0.976, 0.925),
            cell_removed_bg: Color::from_rgb(0.961, 0.667, 0.667),
            cell_removed_fg: Color::from_rgb(0.220, 0.039, 0.055),
            cell_added_bg: Color::from_rgb(0.580, 0.894, 0.694),
            cell_added_fg: Color::from_rgb(0.039, 0.196, 0.094),
            bar_removed: Color::from_rgb(0.820, 0.235, 0.255),
            bar_added: Color::from_rgb(0.145, 0.588, 0.325),
            grid_line: p.border_subtle,
        }
    }
}

struct CsvLineStyle<'a> {
    row_bg: Color,
    bar_color: Color,
    highlight_cols: Option<(&'a [bool], Color, Color)>,
    default_fg: Color,
    grid_line: Color,
    is_header: bool,
}

fn render_csv_line<'a, M: 'a>(
    row_no: Option<usize>,
    cells: &[String],
    col_widths: &[f32],
    style: CsvLineStyle<'_>,
    p: Palette,
) -> Element<'a, M> {
    let row_h = if style.is_header {
        ROW_HEIGHT + 2.0
    } else {
        ROW_HEIGHT
    };
    let bar_color = style.bar_color;
    let row_bg = style.row_bg;
    let grid_line = style.grid_line;

    let mut r = row![
        // 3px left status accent bar (matches RepoFlock screenshot #2)
        container(Space::new(Length::Fixed(3.0), Length::Fixed(row_h))).style(move |_| {
            container::Style {
                background: Some(Background::Color(bar_color)),
                ..Default::default()
            }
        }),
        // Row number gutter
        container(
            text(row_no.map(|n| n.to_string()).unwrap_or_else(|| {
                if style.is_header {
                    "#".into()
                } else {
                    String::new()
                }
            }))
            .size(type_scale::CAPTION)
            .font(theme::CODE_FONT)
            .color(p.text_muted)
            .wrapping(text::Wrapping::None),
        )
        .width(Length::Fixed(GUTTER_W))
        .height(Length::Fixed(row_h))
        .padding([0, 6])
        .align_x(alignment::Horizontal::Right)
        .align_y(alignment::Vertical::Center),
    ]
    .spacing(0)
    .align_y(Alignment::Center);

    for (idx, &w) in col_widths.iter().enumerate() {
        let val = cells.get(idx).cloned().unwrap_or_default();
        let (cell_bg, cell_fg) = match style.highlight_cols {
            Some((flags, hl_bg, hl_fg)) if flags.get(idx).copied().unwrap_or(false) => {
                (Some(hl_bg), hl_fg)
            }
            _ => (None, style.default_fg),
        };
        let cell_text = theme::clipped_line(
            text(val)
                .size(if style.is_header {
                    type_scale::CAPTION
                } else {
                    type_scale::CODE
                })
                .font(theme::CODE_FONT)
                .color(cell_fg)
                .wrapping(text::Wrapping::None),
            Length::Fill,
        );
        let cell = container(cell_text)
            .width(Length::Fixed(w))
            .height(Length::Fixed(row_h))
            .padding([0, 9])
            .align_y(alignment::Vertical::Center)
            .style(move |_| container::Style {
                background: cell_bg.map(Background::Color),
                ..Default::default()
            });
        r = r.push(cell);
    }

    container(r)
        .width(Length::Fill)
        .height(Length::Fixed(row_h))
        .style(move |_| container::Style {
            background: Some(Background::Color(row_bg)),
            border: Border {
                color: grid_line,
                width: 0.5,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn parse_canonical_number(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let unsigned = t.strip_prefix('-').or_else(|| t.strip_prefix('+')).unwrap_or(t);
    // Preserve leading-zero identifiers like "00123" or "07" as strings, not numbers
    if unsigned.len() > 1 && unsigned.starts_with('0') && !unsigned.starts_with("0.") {
        return None;
    }
    let v: f64 = t.parse().ok()?;
    v.is_finite().then_some(v)
}

fn parse_date_ymd_candidates(s: &str) -> Vec<(u32, u32, u32)> {
    let t = s.trim();
    let sep = if t.contains('-') && !t.contains('/') {
        '-'
    } else if t.contains('/') && !t.contains('-') {
        '/'
    } else {
        return Vec::new();
    };
    let parts: Vec<&str> = t.split(sep).collect();
    if parts.len() != 3 {
        return Vec::new();
    }
    let (Ok(a), Ok(b), Ok(c)) = (
        parts[0].parse::<u32>(),
        parts[1].parse::<u32>(),
        parts[2].parse::<u32>(),
    ) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    // YYYY-MM-DD or YYYY/MM/DD
    if parts[0].len() == 4 && (1900..=2100).contains(&a) && (1..=12).contains(&b) && (1..=31).contains(&c) {
        out.push((a, b, c));
    }
    // MM/DD/YYYY or DD/MM/YYYY (Excel locale reformatting)
    if parts[2].len() == 4 && (1900..=2100).contains(&c) {
        if (1..=12).contains(&a) && (1..=31).contains(&b) {
            out.push((c, a, b));
        }
        if (1..=12).contains(&b) && (1..=31).contains(&a) && a != b {
            out.push((c, b, a));
        }
    }
    out
}

/// Returns `true` when two CSV cell strings represent the same semantic value despite Excel
/// reformatting (e.g. `3448.10` vs `3448.1`, `2026-10-11` vs `10/11/2026`, or outer whitespace).
pub(crate) fn cells_format_equivalent(a: &str, b: &str) -> bool {
    let ta = a.trim();
    let tb = b.trim();
    if ta == tb {
        return true;
    }
    if let (Some(na), Some(nb)) = (parse_canonical_number(ta), parse_canonical_number(tb)) {
        let tol = 1e-12 * na.abs().max(nb.abs()).max(1.0);
        if (na - nb).abs() <= tol {
            return true;
        }
    }
    let da = parse_date_ymd_candidates(ta);
    if !da.is_empty() {
        let db = parse_date_ymd_candidates(tb);
        if da.iter().any(|x| db.contains(x)) {
            return true;
        }
    }
    false
}

pub(crate) fn render_csv_diff<'a>(
    data: Option<&'a CsvDiffData>,
    changes_only: bool,
    ignore_format: bool,
    filter: &'a str,
    p: Palette,
    theme_mode: ThemeMode,
) -> Element<'a, CsvDiffMsg> {
    let Some(data) = data else {
        return theme::empty_state(
            p,
            icons::table_grid(theme::icon_size::LG, p.text_muted),
            "Loading CSV table diff…",
            "Parsing spreadsheet columns and comparing rows.",
            None,
        );
    };

    let dark_mode = theme_mode == ThemeMode::Dark;
    let cp = csv_palette(p, dark_mode);
    let file_name = data
        .file_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&data.file_path);
    let format_tag = if data.delimiter == '\t' { "TSV" } else { "CSV" };
    let delim_label = match data.delimiter {
        '\t' => "Tab",
        ';' => "Semicolon",
        _ => "Comma",
    };

    // Compute effective stats & format-only cell count so Excel noise can be surfaced/filtered
    let mut eff_stats = CsvDiffStats {
        added_rows: data.stats.added_rows,
        removed_rows: data.stats.removed_rows,
        unchanged_rows: data.stats.unchanged_rows,
        modified_rows: 0,
        changed_cells: 0,
    };
    let mut format_only_cells = 0usize;
    for item in &data.rows {
        if let CsvDiffRow::Modified {
            old_cells,
            new_cells,
            changed_cols,
            ..
        } = item
        {
            let mut real_chg = 0usize;
            for ((o, n), &raw_chg) in old_cells
                .iter()
                .zip(new_cells.iter())
                .zip(changed_cols.iter())
            {
                if raw_chg {
                    if cells_format_equivalent(o, n) {
                        format_only_cells += 1;
                        if !ignore_format {
                            real_chg += 1;
                        }
                    } else {
                        real_chg += 1;
                    }
                }
            }
            if real_chg > 0 {
                eff_stats.modified_rows += 1;
                eff_stats.changed_cells += real_chg;
            } else {
                eff_stats.unchanged_rows += 1;
            }
        }
    }

    // 1. Metadata & Filter Toolbar
    let mut toolbar = row![
        container(
            text(format_tag)
                .size(type_scale::BADGE)
                .font(theme::CODE_FONT)
                .color(p.accent_fg)
                .wrapping(text::Wrapping::None)
        )
        .padding([1, 6])
        .style(theme::pill_box(p.fill_selected, p.accent_fg, p.border_default)),
        text(file_name.to_string())
            .size(type_scale::CAPTION)
            .color(p.text_primary)
            .wrapping(text::Wrapping::None),
        text(format!(
            "{} ({} lines · {})  →  {} ({} lines · {})  ·  {} cols ({delim_label})",
            data.before_label,
            data.before_line_count,
            format_bytes(data.before_byte_len),
            data.after_label,
            data.after_line_count,
            format_bytes(data.after_byte_len),
            data.headers.len()
        ))
        .size(type_scale::CAPTION)
        .color(p.text_muted)
        .wrapping(text::Wrapping::None),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    if eff_stats.modified_rows > 0 || eff_stats.added_rows > 0 || eff_stats.removed_rows > 0 {
        let mut pills = row![].spacing(5).align_y(Alignment::Center);
        if eff_stats.modified_rows > 0 {
            pills = pills.push(
                text(format!("~{} rows", eff_stats.modified_rows))
                    .size(type_scale::CAPTION)
                    .font(theme::CODE_FONT)
                    .color(p.warning_fg)
                    .wrapping(text::Wrapping::None),
            );
        }
        if eff_stats.added_rows > 0 {
            pills = pills.push(
                text(format!("+{}", eff_stats.added_rows))
                    .size(type_scale::CAPTION)
                    .font(theme::CODE_FONT)
                    .color(p.success_fg)
                    .wrapping(text::Wrapping::None),
            );
        }
        if eff_stats.removed_rows > 0 {
            pills = pills.push(
                text(format!("-{}", eff_stats.removed_rows))
                    .size(type_scale::CAPTION)
                    .font(theme::CODE_FONT)
                    .color(p.danger_fg)
                    .wrapping(text::Wrapping::None),
            );
        }
        if eff_stats.changed_cells > 0 {
            pills = pills.push(
                text(format!("· {} cells", eff_stats.changed_cells))
                    .size(type_scale::CAPTION)
                    .font(theme::CODE_FONT)
                    .color(p.text_muted)
                    .wrapping(text::Wrapping::None),
            );
        }
        toolbar = toolbar.push(
            container(pills)
                .padding([2, 7])
                .style(theme::section_panel_box(p)),
        );
    }

    let ignore_fmt_label = if !ignore_format && format_only_cells > 0 {
        format!("Ignore format ({format_only_cells})")
    } else {
        "Ignore format".to_string()
    };
    let ignore_fmt_btn = iced::widget::tooltip(
        button(
            text(ignore_fmt_label)
                .size(type_scale::CAPTION)
                .wrapping(text::Wrapping::None),
        )
        .padding([3, 8])
        .style(theme::filter_pill_btn(p, ignore_format))
        .on_press(CsvDiffMsg::ToggleIgnoreFormat),
        container(
            text("Ignore Excel number/date reformatting (e.g. 47.20 → 47.2, 2026-10-11 → 10/11/2026)")
                .size(type_scale::CAPTION)
                .color(p.text_primary),
        )
        .padding([5, 9])
        .style(theme::tooltip_box(p)),
        iced::widget::tooltip::Position::Bottom,
    )
    .gap(5);

    let scope_toggle = theme::segmented_group(
        p,
        row![
            button(
                text("All rows")
                    .size(type_scale::CAPTION)
                    .wrapping(text::Wrapping::None)
            )
            .padding([2, 8])
            .style(theme::segmented_btn(p, !changes_only))
            .on_press(CsvDiffMsg::SetChangesOnly(false)),
            button(
                text("Changes only")
                    .size(type_scale::CAPTION)
                    .wrapping(text::Wrapping::None)
            )
            .padding([2, 8])
            .style(theme::segmented_btn(p, changes_only))
            .on_press(CsvDiffMsg::SetChangesOnly(true)),
        ]
        .spacing(2)
        .align_y(Alignment::Center),
    );

    toolbar = toolbar
        .push(Space::with_width(Length::Fill))
        .push(ignore_fmt_btn)
        .push(scope_toggle)
        .push(container(
            text_input("Filter rows…", filter)
                .on_input(CsvDiffMsg::FilterChanged)
                .padding([4, 8])
                .size(type_scale::CAPTION)
                .width(Length::Fixed(160.0))
                .style(theme::dark_input(p)),
        ));

    // 2. Build Spreadsheet Rows
    let q = filter.trim().to_lowercase();
    let row_matches_q = |cells: &[String]| -> bool {
        q.is_empty() || cells.iter().any(|c| c.to_lowercase().contains(&q))
    };

    let mut table_col = column![].spacing(0);

    // Header row(s)
    if let Some(old_hdr) = &data.old_headers {
        table_col = table_col.push(render_csv_line(
            Some(1),
            old_hdr,
            &data.col_widths,
            CsvLineStyle {
                row_bg: cp.row_removed_bg,
                bar_color: cp.bar_removed,
                highlight_cols: Some((
                    &data.header_changed_cols,
                    cp.cell_removed_bg,
                    cp.cell_removed_fg,
                )),
                default_fg: p.text_primary,
                grid_line: cp.grid_line,
                is_header: true,
            },
            p,
        ));
        table_col = table_col.push(render_csv_line(
            Some(1),
            &data.headers,
            &data.col_widths,
            CsvLineStyle {
                row_bg: cp.row_added_bg,
                bar_color: cp.bar_added,
                highlight_cols: Some((
                    &data.header_changed_cols,
                    cp.cell_added_bg,
                    cp.cell_added_fg,
                )),
                default_fg: p.text_primary,
                grid_line: cp.grid_line,
                is_header: true,
            },
            p,
        ));
    } else {
        table_col = table_col.push(render_csv_line(
            None,
            &data.headers,
            &data.col_widths,
            CsvLineStyle {
                row_bg: p.surface_titlebar,
                bar_color: Color::TRANSPARENT,
                highlight_cols: None,
                default_fg: p.text_primary,
                grid_line: p.border_strong,
                is_header: true,
            },
            p,
        ));
    }

    let mut shown_logical = 0usize;
    let mut total_matching = 0usize;
    for (idx, item) in data.rows.iter().enumerate() {
        let eff_changed: Option<Vec<bool>> = match item {
            CsvDiffRow::Modified {
                old_cells,
                new_cells,
                changed_cols,
                ..
            } if ignore_format => Some(
                old_cells
                    .iter()
                    .zip(new_cells.iter())
                    .zip(changed_cols.iter())
                    .map(|((o, n), &chg)| chg && !cells_format_equivalent(o, n))
                    .collect(),
            ),
            _ => None,
        };
        let is_effectively_unchanged = match item {
            CsvDiffRow::Unchanged { .. } => true,
            CsvDiffRow::Modified { .. } => eff_changed
                .as_ref()
                .is_some_and(|flags| !flags.iter().any(|&c| c)),
            _ => false,
        };
        if changes_only && is_effectively_unchanged {
            continue;
        }
        let matches_filter = match item {
            CsvDiffRow::Unchanged { cells, .. }
            | CsvDiffRow::Removed { cells, .. }
            | CsvDiffRow::Added { cells, .. } => row_matches_q(cells),
            CsvDiffRow::Modified {
                old_cells,
                new_cells,
                ..
            } => row_matches_q(old_cells) || row_matches_q(new_cells),
        };
        if !matches_filter {
            continue;
        }
        total_matching += 1;
        if shown_logical >= MAX_RENDERED_CSV_ROWS {
            continue;
        }
        shown_logical += 1;

        match item {
            CsvDiffRow::Unchanged {
                new_row_no, cells, ..
            } => {
                let bg = if idx % 2 == 0 {
                    p.surface_panel
                } else {
                    Color {
                        a: 0.65,
                        ..p.surface_card
                    }
                };
                table_col = table_col.push(render_csv_line(
                    Some(*new_row_no),
                    cells,
                    &data.col_widths,
                    CsvLineStyle {
                        row_bg: bg,
                        bar_color: Color::TRANSPARENT,
                        highlight_cols: None,
                        default_fg: p.text_secondary,
                        grid_line: cp.grid_line,
                        is_header: false,
                    },
                    p,
                ));
            }
            CsvDiffRow::Removed { old_row_no, cells } => {
                table_col = table_col.push(render_csv_line(
                    Some(*old_row_no),
                    cells,
                    &data.col_widths,
                    CsvLineStyle {
                        row_bg: cp.row_removed_bg,
                        bar_color: cp.bar_removed,
                        highlight_cols: None,
                        default_fg: p.text_primary,
                        grid_line: cp.grid_line,
                        is_header: false,
                    },
                    p,
                ));
            }
            CsvDiffRow::Added { new_row_no, cells } => {
                table_col = table_col.push(render_csv_line(
                    Some(*new_row_no),
                    cells,
                    &data.col_widths,
                    CsvLineStyle {
                        row_bg: cp.row_added_bg,
                        bar_color: cp.bar_added,
                        highlight_cols: None,
                        default_fg: p.text_primary,
                        grid_line: cp.grid_line,
                        is_header: false,
                    },
                    p,
                ));
            }
            CsvDiffRow::Modified {
                old_row_no,
                new_row_no,
                old_cells,
                new_cells,
                changed_cols,
            } => {
                if is_effectively_unchanged {
                    let bg = if idx % 2 == 0 {
                        p.surface_panel
                    } else {
                        Color {
                            a: 0.65,
                            ..p.surface_card
                        }
                    };
                    table_col = table_col.push(render_csv_line(
                        Some(*new_row_no),
                        new_cells,
                        &data.col_widths,
                        CsvLineStyle {
                            row_bg: bg,
                            bar_color: Color::TRANSPARENT,
                            highlight_cols: None,
                            default_fg: p.text_secondary,
                            grid_line: cp.grid_line,
                            is_header: false,
                        },
                        p,
                    ));
                } else {
                    let flags: &[bool] = eff_changed.as_deref().unwrap_or(changed_cols.as_slice());
                    table_col = table_col.push(render_csv_line(
                        Some(*old_row_no),
                        old_cells,
                        &data.col_widths,
                        CsvLineStyle {
                            row_bg: cp.row_removed_bg,
                            bar_color: cp.bar_removed,
                            highlight_cols: Some((flags, cp.cell_removed_bg, cp.cell_removed_fg)),
                            default_fg: p.text_primary,
                            grid_line: cp.grid_line,
                            is_header: false,
                        },
                        p,
                    ));
                    table_col = table_col.push(render_csv_line(
                        Some(*new_row_no),
                        new_cells,
                        &data.col_widths,
                        CsvLineStyle {
                            row_bg: cp.row_added_bg,
                            bar_color: cp.bar_added,
                            highlight_cols: Some((flags, cp.cell_added_bg, cp.cell_added_fg)),
                            default_fg: p.text_primary,
                            grid_line: cp.grid_line,
                            is_header: false,
                        },
                        p,
                    ));
                }
            }
        }
    }

    if total_matching == 0 {
        table_col = table_col.push(
            container(
                text(if changes_only && q.is_empty() {
                    if ignore_format && format_only_cells > 0 {
                        "No semantic value changes — all differences are Excel number/date formatting."
                    } else {
                        "No changed rows in this CSV file."
                    }
                } else {
                    "No rows match the current filter."
                })
                .size(type_scale::BODY)
                .color(p.text_muted),
            )
            .padding([18, 16]),
        );
    } else if total_matching > MAX_RENDERED_CSV_ROWS {
        table_col = table_col.push(
            container(
                text(format!(
                    "Showing first {MAX_RENDERED_CSV_ROWS} of {total_matching} matching rows — enable 'Changes only' or filter rows to inspect specific entries."
                ))
                .size(type_scale::CAPTION)
                .color(p.warning_fg),
            )
            .padding([10, 14])
            .width(Length::Fill)
            .style(theme::section_header_box(p)),
        );
    }

    // Reserve breathing room below the last row so it never clips under the horizontal scrollbar
    table_col = table_col.push(Space::with_height(Length::Fixed(14.0)));

    let total_table_w: f32 = 3.0 + GUTTER_W + data.col_widths.iter().sum::<f32>() + 16.0;
    let scroll_body = theme::responsive_hscroll(table_col, total_table_w.max(420.0), p);

    column![
        toolbar,
        horizontal_rule(1).style(theme::subtle_rule(p)),
        scroll_body,
    ]
    .spacing(5)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

impl GitFleetApp {
    pub(crate) fn view_csv_diff(&self, p: Palette) -> Element<'_, Message> {
        render_csv_diff(
            self.diff_csv.as_ref(),
            self.csv_diff_changes_only,
            self.csv_diff_ignore_format,
            &self.csv_diff_filter,
            p,
            self.theme_mode,
        )
        .map(|msg| match msg {
            CsvDiffMsg::SetChangesOnly(only) => Message::SetCsvDiffChangesOnly(only),
            CsvDiffMsg::ToggleIgnoreFormat => Message::ToggleCsvDiffIgnoreFormat,
            CsvDiffMsg::FilterChanged(q) => Message::CsvDiffFilterChanged(q),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rfc4180_quoted_escaped_and_semicolon_csv() {
        let raw = "\u{feff}sku,product,note\r\nSF-1,\"Canvas, tote\",\"Line 1\nLine \"\"2\"\"\"\r\n";
        let delim = detect_delimiter("items.csv", raw);
        assert_eq!(delim, ',');
        let rows = parse_csv_records(raw, delim);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["sku", "product", "note"]);
        assert_eq!(
            rows[1],
            vec!["SF-1", "Canvas, tote", "Line 1\nLine \"2\""]
        );

        let semi = "a;b;c\n1;2;3\n";
        assert_eq!(detect_delimiter("eu.csv", semi), ';');
        assert_eq!(detect_delimiter("data.tsv", "a\tb\n1\t2\n"), '\t');
    }

    #[test]
    fn pairs_modified_rows_by_key_and_highlights_exact_changed_cells() {
        let before = "\
sku,product,warehouse,stock,price
SF-1001,Canvas tote,Berlin,140,24.00
SF-1002,Linen apron,Berlin,62,32.00
SF-1008,Cotton socks,Lisbon,275,11.00
SF-1009,Travel blanket,Oslo,38,64.00
SF-1010,Desk calendar,Berlin,150,16.00
";
        let after = "\
sku,product,warehouse,stock,price
SF-1001,Canvas tote,Berlin,118,24.00
SF-1002,Linen apron,Berlin,62,32.00
SF-1008,Cotton socks,Berlin,275,11.00
SF-1010,Desk calendar,Berlin,150,16.00
SF-1011,Ceramic mug,Lisbon,90,14.50
";
        let diff = compute_csv_diff(FileVersionPair {
            file_path: "inventory.csv".into(),
            before_label: "september.csv".into(),
            after_label: "october.csv".into(),
            before_bytes: Some(before.as_bytes().to_vec()),
            after_bytes: Some(after.as_bytes().to_vec()),
        });

        assert_eq!(diff.headers, vec!["sku", "product", "warehouse", "stock", "price"]);
        assert!(diff.old_headers.is_none());
        assert_eq!(diff.stats.modified_rows, 2);
        assert_eq!(diff.stats.removed_rows, 1);
        assert_eq!(diff.stats.added_rows, 1);
        assert_eq!(diff.stats.unchanged_rows, 2);
        assert_eq!(diff.stats.changed_cells, 2);

        // SF-1001: only `stock` (col 3) changed
        match &diff.rows[0] {
            CsvDiffRow::Modified {
                old_cells,
                new_cells,
                changed_cols,
                ..
            } => {
                assert_eq!(old_cells[3], "140");
                assert_eq!(new_cells[3], "118");
                assert_eq!(changed_cols, &vec![false, false, false, true, false]);
            }
            other => panic!("expected Modified for SF-1001, got {other:?}"),
        }

        // SF-1008: only `warehouse` (col 2) changed
        match &diff.rows[2] {
            CsvDiffRow::Modified { changed_cols, .. } => {
                assert_eq!(changed_cols, &vec![false, false, true, false, false]);
            }
            other => panic!("expected Modified for SF-1008, got {other:?}"),
        }

        // SF-1009: purely removed
        assert!(matches!(&diff.rows[3], CsvDiffRow::Removed { cells, .. } if cells[0] == "SF-1009"));

        // SF-1011: purely added
        assert!(matches!(&diff.rows[5], CsvDiffRow::Added { cells, .. } if cells[0] == "SF-1011"));
    }

    #[test]
    fn recognizes_excel_number_and_date_reformatting_without_hiding_real_edits() {
        assert!(cells_format_equivalent("3448.10", "3448.1"));
        assert!(cells_format_equivalent("47.20", "47.2"));
        assert!(cells_format_equivalent("2026-10-11", "10/11/2026"));
        assert!(cells_format_equivalent("2026-10-11", "11/10/2026"));
        assert!(!cells_format_equivalent("", "123"));
        assert!(!cells_format_equivalent("00123", "123"));
        assert!(!cells_format_equivalent("2026-10-11", "10/12/2026"));
    }
}
