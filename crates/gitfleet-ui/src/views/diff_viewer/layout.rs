//! Diff layout builder, cached syntax/offset table (`DiffLayoutCache`),
//! viewport virtualization slicing, and piecewise scroll synchronization.

use super::inline_diff::{InlineSpan, compute_inline_diff, full_changed_spans};
use super::{DIFF_CONTEXT_LINES, DIFF_HIDDEN_H, DIFF_LINE_H, DIFF_SEP_H};
use crate::app::DiffGapExpansion;
use crate::syntax::{CodeLanguage, DiffSyntaxHighlighter, TokenFormat};
use crate::theme::{self, ThemeMode};
use gitfleet_git::{DiffBlock, DiffChangeKind, ParsedDiff, parse_unified_diff};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) enum SideRow {
    Code {
        line_no: Option<usize>,
        text: String,
        spans: Vec<InlineSpan>,
        kind: Option<DiffChangeKind>,
        is_first: bool,
        is_last: bool,
        top_seam: Option<DiffChangeKind>,
        bottom_seam: Option<DiffChangeKind>,
    },
    Hidden {
        gap_id: usize,
        hidden_count: usize,
        can_expand_top: bool,
        can_expand_bottom: bool,
    },
    Separator(String),
}

impl SideRow {
    #[inline]
    pub(crate) fn height(&self) -> f32 {
        match self {
            Self::Code { .. } => DIFF_LINE_H,
            Self::Hidden { .. } => DIFF_HIDDEN_H,
            Self::Separator(_) => DIFF_SEP_H,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum UnifiedRow {
    HunkHeader {
        header: String,
        patch: String,
        kind: DiffChangeKind,
    },
    Code {
        old_line: Option<usize>,
        new_line: Option<usize>,
        sign: char,
        text: String,
        spans: Vec<InlineSpan>,
        kind: Option<DiffChangeKind>,
    },
    Hidden {
        gap_id: usize,
        hidden_count: usize,
        can_expand_top: bool,
        can_expand_bottom: bool,
    },
    Separator(String),
}

impl UnifiedRow {
    #[inline]
    pub(crate) fn height(&self) -> f32 {
        match self {
            Self::Code { .. } => DIFF_LINE_H,
            Self::Hidden { .. } => DIFF_HIDDEN_H,
            Self::HunkHeader { .. } | Self::Separator(_) => DIFF_SEP_H,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ConnectorSpan {
    pub(crate) kind: DiffChangeKind,
    pub(crate) left_top: f32,
    pub(crate) left_bot: f32,
    pub(crate) right_top: f32,
    pub(crate) right_bot: f32,
    pub(crate) unified_top: f32,
}

#[derive(Debug, Default)]
pub(crate) struct DiffLayout {
    pub(crate) left_rows: Vec<SideRow>,
    pub(crate) right_rows: Vec<SideRow>,
    pub(crate) unified_rows: Vec<UnifiedRow>,
    pub(crate) connectors: Vec<ConnectorSpan>,
    pub(crate) anchors: Vec<(f32, f32)>,
    pub(crate) total_left_h: f32,
    pub(crate) total_right_h: f32,
    pub(crate) total_unified_h: f32,
    pub(crate) max_left_chars: usize,
    pub(crate) max_right_chars: usize,
    pub(crate) max_unified_chars: usize,
}

pub(crate) type SyntaxSpans = Vec<(Range<usize>, TokenFormat)>;

/// Pre-computed diff layout + per-row syntax highlighting spans + cumulative Y offsets
/// for O(log N) viewport virtualization and zero per-frame layout/lexing overhead.
#[derive(Debug)]
pub(crate) struct DiffLayoutCache {
    pub(crate) layout: DiffLayout,
    pub(crate) left_y_offsets: Vec<f32>,
    pub(crate) right_y_offsets: Vec<f32>,
    pub(crate) unified_y_offsets: Vec<f32>,
    pub(crate) left_syntax: Vec<SyntaxSpans>,
    pub(crate) right_syntax: Vec<SyntaxSpans>,
    pub(crate) unified_syntax: Vec<SyntaxSpans>,
    pub(crate) raw_lines: Vec<String>,
    pub(crate) raw_max_chars: usize,
    pub(crate) compact: bool,
    pub(crate) language: CodeLanguage,
    pub(crate) theme_mode: ThemeMode,
}

impl Default for DiffLayoutCache {
    fn default() -> Self {
        Self {
            layout: DiffLayout::default(),
            left_y_offsets: vec![0.0],
            right_y_offsets: vec![0.0],
            unified_y_offsets: vec![0.0],
            left_syntax: Vec::new(),
            right_syntax: Vec::new(),
            unified_syntax: Vec::new(),
            raw_lines: Vec::new(),
            raw_max_chars: 40,
            compact: true,
            language: CodeLanguage::PlainText,
            theme_mode: ThemeMode::Dark,
        }
    }
}

impl DiffLayoutCache {
    pub(crate) fn matches_config(
        &self,
        compact: bool,
        language: CodeLanguage,
        theme_mode: ThemeMode,
    ) -> bool {
        self.compact == compact && self.language == language && self.theme_mode == theme_mode
    }
}

/// Builds a complete cached diff layout + syntax spans + cumulative row Y-offsets.
/// Safe to invoke either on a background blocking thread or synchronously on config change.
pub(crate) fn build_diff_layout_cache(
    parsed: &ParsedDiff,
    compact: bool,
    expanded_gaps: &HashMap<usize, DiffGapExpansion>,
    language: CodeLanguage,
    theme_mode: ThemeMode,
) -> Arc<DiffLayoutCache> {
    if parsed.blocks.is_empty() {
        let mut raw_lines = Vec::new();
        let mut raw_max_chars: usize = 40;
        if !parsed.raw.is_empty() {
            for line in parsed.raw.lines().take(2000) {
                let clean = theme::sanitize_code_line(line);
                raw_max_chars = raw_max_chars.max(clean.chars().count());
                raw_lines.push(clean);
            }
        }
        return Arc::new(DiffLayoutCache {
            raw_lines,
            raw_max_chars,
            compact,
            language,
            theme_mode,
            ..Default::default()
        });
    }

    let layout = build_diff_layout(parsed, compact, expanded_gaps);

    let mut left_y_offsets = Vec::with_capacity(layout.left_rows.len() + 1);
    let mut left_syntax = Vec::with_capacity(layout.left_rows.len());
    let mut left_hl = DiffSyntaxHighlighter::new(language, theme_mode);
    let mut y = 0.0_f32;
    left_y_offsets.push(0.0);
    for row in &layout.left_rows {
        y += row.height();
        left_y_offsets.push(y);
        match row {
            SideRow::Code { text, .. } => left_syntax.push(left_hl.highlight_line(text)),
            SideRow::Hidden { .. } | SideRow::Separator(_) => {
                left_hl.reset();
                left_syntax.push(Vec::new());
            }
        }
    }

    let mut right_y_offsets = Vec::with_capacity(layout.right_rows.len() + 1);
    let mut right_syntax = Vec::with_capacity(layout.right_rows.len());
    let mut right_hl = DiffSyntaxHighlighter::new(language, theme_mode);
    y = 0.0_f32;
    right_y_offsets.push(0.0);
    for row in &layout.right_rows {
        y += row.height();
        right_y_offsets.push(y);
        match row {
            SideRow::Code { text, .. } => right_syntax.push(right_hl.highlight_line(text)),
            SideRow::Hidden { .. } | SideRow::Separator(_) => {
                right_hl.reset();
                right_syntax.push(Vec::new());
            }
        }
    }

    let mut unified_y_offsets = Vec::with_capacity(layout.unified_rows.len() + 1);
    let mut unified_syntax = Vec::with_capacity(layout.unified_rows.len());
    let mut unified_hl = DiffSyntaxHighlighter::new(language, theme_mode);
    y = 0.0_f32;
    unified_y_offsets.push(0.0);
    for row in &layout.unified_rows {
        y += row.height();
        unified_y_offsets.push(y);
        match row {
            UnifiedRow::Code { text, .. } => unified_syntax.push(unified_hl.highlight_line(text)),
            UnifiedRow::HunkHeader { .. }
            | UnifiedRow::Hidden { .. }
            | UnifiedRow::Separator(_) => {
                unified_hl.reset();
                unified_syntax.push(Vec::new());
            }
        }
    }

    Arc::new(DiffLayoutCache {
        layout,
        left_y_offsets,
        right_y_offsets,
        unified_y_offsets,
        left_syntax,
        right_syntax,
        unified_syntax,
        raw_lines: Vec::new(),
        raw_max_chars: 40,
        compact,
        language,
        theme_mode,
    })
}

/// Parses a unified diff and builds its initial `DiffLayoutCache` completely off the UI thread.
pub(crate) fn parse_and_cache_diff(
    raw: &str,
    compact: bool,
    language: CodeLanguage,
    theme_mode: ThemeMode,
) -> (ParsedDiff, Arc<DiffLayoutCache>) {
    let parsed = parse_unified_diff(raw);
    let cache = build_diff_layout_cache(&parsed, compact, &HashMap::new(), language, theme_mode);
    (parsed, cache)
}

/// Computes the half-open row index range `[start, end)` overlapping `[scroll_y - overscan, scroll_y + viewport_h + overscan]`
/// plus `(top_spacer_h, bottom_spacer_h)` in O(log N) time using `y_offsets` (length `rows + 1`).
pub(crate) fn visible_row_range(
    y_offsets: &[f32],
    scroll_y: f32,
    viewport_h: f32,
    overscan_px: f32,
) -> (Range<usize>, f32, f32) {
    let row_count = y_offsets.len().saturating_sub(1);
    if row_count == 0 {
        return (0..0, 0.0, 0.0);
    }
    let total_h = *y_offsets.last().unwrap_or(&0.0);
    let view_top = (scroll_y - overscan_px).max(0.0);
    let view_bot = (scroll_y + viewport_h.max(1.0) + overscan_px).min(total_h);

    // First row whose bottom edge (`y_offsets[i + 1]`) is strictly greater than `view_top`.
    let start = y_offsets[1..]
        .partition_point(|&row_bot| row_bot <= view_top)
        .min(row_count);
    // First row whose top edge (`y_offsets[i]`) is at or past `view_bot`.
    let end = y_offsets[..row_count]
        .partition_point(|&row_top| row_top < view_bot)
        .clamp(start, row_count);

    let top_spacer = y_offsets[start];
    let bottom_spacer = (total_h - y_offsets[end]).max(0.0);
    (start..end, top_spacer, bottom_spacer)
}

fn push_anchor(anchors: &mut Vec<(f32, f32)>, ly: f32, ry: f32) {
    if let Some(&(last_l, last_r)) = anchors.last()
        && (last_l - ly).abs() < 0.01
        && (last_r - ry).abs() < 0.01
    {
        return;
    }
    anchors.push((ly, ry));
}

fn mark_side_seam(
    rows: &mut [SideRow],
    pending_top: &mut Option<DiffChangeKind>,
    kind: DiffChangeKind,
) {
    if let Some(SideRow::Code { bottom_seam, .. }) = rows.last_mut() {
        *bottom_seam = Some(kind);
    } else {
        *pending_top = Some(kind);
    }
}

/// Pure layout generator shared by Side-by-Side view, Unified view, vertical scroll sync,
/// Next/Prev change jump buttons, and unit tests.
pub(crate) fn build_diff_layout(
    parsed: &ParsedDiff,
    compact: bool,
    expanded_gaps: &HashMap<usize, DiffGapExpansion>,
) -> DiffLayout {
    let mut out = DiffLayout {
        anchors: vec![(0.0, 0.0)],
        max_left_chars: 32,
        max_right_chars: 32,
        max_unified_chars: 40,
        ..Default::default()
    };

    let mut left_y = 0.0_f32;
    let mut right_y = 0.0_f32;
    let mut unified_y = 0.0_f32;
    let mut pending_left_top: Option<DiffChangeKind> = None;
    let mut pending_right_top: Option<DiffChangeKind> = None;
    let mut gap_id = 0usize;

    let blocks = &parsed.blocks;
    for (b_idx, block) in blocks.iter().enumerate() {
        match block {
            DiffBlock::Equal(lines) => {
                let has_prev_change = blocks[..b_idx]
                    .iter()
                    .any(|b| matches!(b, DiffBlock::Change(_)));
                let has_next_change = blocks[b_idx + 1..]
                    .iter()
                    .any(|b| matches!(b, DiffBlock::Change(_)));

                let cur_gap_id = gap_id;
                gap_id += 1;

                let exp = expanded_gaps.get(&cur_gap_id).copied().unwrap_or_default();
                let (keep_top, keep_bot, hidden) = if !compact || exp.show_all {
                    (lines.len(), 0usize, 0usize)
                } else {
                    let kt = if has_prev_change {
                        DIFF_CONTEXT_LINES + exp.top_shown
                    } else {
                        exp.top_shown
                    };
                    let kb = if has_next_change {
                        DIFF_CONTEXT_LINES + exp.bottom_shown
                    } else {
                        exp.bottom_shown
                    };
                    // Never collapse 0 or 1 line into a hidden bar
                    if kt + kb + 1 >= lines.len() {
                        (lines.len(), 0usize, 0usize)
                    } else {
                        (kt, kb, lines.len() - kt - kb)
                    }
                };

                for ctx in &lines[..keep_top] {
                    let clean = theme::sanitize_code_line(&ctx.text);
                    let len = clean.chars().count();
                    out.max_left_chars = out.max_left_chars.max(len);
                    out.max_right_chars = out.max_right_chars.max(len);
                    out.max_unified_chars = out.max_unified_chars.max(len);

                    out.left_rows.push(SideRow::Code {
                        line_no: Some(ctx.old_line),
                        text: clean.clone(),
                        spans: Vec::new(),
                        kind: None,
                        is_first: false,
                        is_last: false,
                        top_seam: pending_left_top.take(),
                        bottom_seam: None,
                    });
                    out.right_rows.push(SideRow::Code {
                        line_no: Some(ctx.new_line),
                        text: clean.clone(),
                        spans: Vec::new(),
                        kind: None,
                        is_first: false,
                        is_last: false,
                        top_seam: pending_right_top.take(),
                        bottom_seam: None,
                    });
                    out.unified_rows.push(UnifiedRow::Code {
                        old_line: Some(ctx.old_line),
                        new_line: Some(ctx.new_line),
                        sign: ' ',
                        text: clean,
                        spans: Vec::new(),
                        kind: None,
                    });
                    left_y += DIFF_LINE_H;
                    right_y += DIFF_LINE_H;
                    unified_y += DIFF_LINE_H;
                }
                push_anchor(&mut out.anchors, left_y, right_y);

                if hidden > 0 {
                    out.left_rows.push(SideRow::Hidden {
                        gap_id: cur_gap_id,
                        hidden_count: hidden,
                        can_expand_top: has_prev_change,
                        can_expand_bottom: has_next_change,
                    });
                    out.right_rows.push(SideRow::Hidden {
                        gap_id: cur_gap_id,
                        hidden_count: hidden,
                        can_expand_top: has_prev_change,
                        can_expand_bottom: has_next_change,
                    });
                    out.unified_rows.push(UnifiedRow::Hidden {
                        gap_id: cur_gap_id,
                        hidden_count: hidden,
                        can_expand_top: has_prev_change,
                        can_expand_bottom: has_next_change,
                    });
                    left_y += DIFF_HIDDEN_H;
                    right_y += DIFF_HIDDEN_H;
                    unified_y += DIFF_HIDDEN_H;
                    push_anchor(&mut out.anchors, left_y, right_y);

                    for ctx in &lines[lines.len() - keep_bot..] {
                        let clean = theme::sanitize_code_line(&ctx.text);
                        let len = clean.chars().count();
                        out.max_left_chars = out.max_left_chars.max(len);
                        out.max_right_chars = out.max_right_chars.max(len);
                        out.max_unified_chars = out.max_unified_chars.max(len);

                        out.left_rows.push(SideRow::Code {
                            line_no: Some(ctx.old_line),
                            text: clean.clone(),
                            spans: Vec::new(),
                            kind: None,
                            is_first: false,
                            is_last: false,
                            top_seam: pending_left_top.take(),
                            bottom_seam: None,
                        });
                        out.right_rows.push(SideRow::Code {
                            line_no: Some(ctx.new_line),
                            text: clean.clone(),
                            spans: Vec::new(),
                            kind: None,
                            is_first: false,
                            is_last: false,
                            top_seam: pending_right_top.take(),
                            bottom_seam: None,
                        });
                        out.unified_rows.push(UnifiedRow::Code {
                            old_line: Some(ctx.old_line),
                            new_line: Some(ctx.new_line),
                            sign: ' ',
                            text: clean,
                            spans: Vec::new(),
                            kind: None,
                        });
                        left_y += DIFF_LINE_H;
                        right_y += DIFF_LINE_H;
                        unified_y += DIFF_LINE_H;
                    }
                    push_anchor(&mut out.anchors, left_y, right_y);
                }
            }
            DiffBlock::Change(cb) => {
                let left_top = left_y;
                let right_top = right_y;
                let unified_top = unified_y;

                out.unified_rows.push(UnifiedRow::HunkHeader {
                    header: cb.header.clone(),
                    patch: cb.patch.clone(),
                    kind: cb.kind,
                });
                unified_y += DIFF_SEP_H;

                let old_clean: Vec<String> = cb
                    .old_lines
                    .iter()
                    .map(|l| theme::sanitize_code_line(l))
                    .collect();
                let new_clean: Vec<String> = cb
                    .new_lines
                    .iter()
                    .map(|l| theme::sanitize_code_line(l))
                    .collect();

                // Compute SmartGit-style intra-line highlights for Modified blocks
                let mut old_spans: Vec<Vec<InlineSpan>> = vec![Vec::new(); old_clean.len()];
                let mut new_spans: Vec<Vec<InlineSpan>> = vec![Vec::new(); new_clean.len()];
                if cb.kind == DiffChangeKind::Modified {
                    let paired = old_clean.len().min(new_clean.len());
                    for idx in 0..paired {
                        let (os, ns) = compute_inline_diff(&old_clean[idx], &new_clean[idx]);
                        old_spans[idx] = os;
                        new_spans[idx] = ns;
                    }
                    for idx in paired..old_clean.len() {
                        old_spans[idx] = full_changed_spans(&old_clean[idx]);
                    }
                    for idx in paired..new_clean.len() {
                        new_spans[idx] = full_changed_spans(&new_clean[idx]);
                    }
                }

                if old_clean.is_empty() {
                    mark_side_seam(&mut out.left_rows, &mut pending_left_top, cb.kind);
                } else {
                    let n = old_clean.len();
                    for (idx, clean) in old_clean.into_iter().enumerate() {
                        let len = clean.chars().count();
                        out.max_left_chars = out.max_left_chars.max(len);
                        out.max_unified_chars = out.max_unified_chars.max(len);
                        let ln = cb.old_start + idx;
                        let sp = std::mem::take(&mut old_spans[idx]);

                        out.left_rows.push(SideRow::Code {
                            line_no: Some(ln),
                            text: clean.clone(),
                            spans: sp.clone(),
                            kind: Some(cb.kind),
                            is_first: idx == 0,
                            is_last: idx + 1 == n,
                            top_seam: pending_left_top.take(),
                            bottom_seam: None,
                        });
                        out.unified_rows.push(UnifiedRow::Code {
                            old_line: Some(ln),
                            new_line: None,
                            sign: '-',
                            text: clean,
                            spans: sp,
                            kind: Some(DiffChangeKind::Removed),
                        });
                        left_y += DIFF_LINE_H;
                        unified_y += DIFF_LINE_H;
                    }
                }

                if new_clean.is_empty() {
                    mark_side_seam(&mut out.right_rows, &mut pending_right_top, cb.kind);
                } else {
                    let n = new_clean.len();
                    for (idx, clean) in new_clean.into_iter().enumerate() {
                        let len = clean.chars().count();
                        out.max_right_chars = out.max_right_chars.max(len);
                        out.max_unified_chars = out.max_unified_chars.max(len);
                        let ln = cb.new_start + idx;
                        let sp = std::mem::take(&mut new_spans[idx]);

                        out.right_rows.push(SideRow::Code {
                            line_no: Some(ln),
                            text: clean.clone(),
                            spans: sp.clone(),
                            kind: Some(cb.kind),
                            is_first: idx == 0,
                            is_last: idx + 1 == n,
                            top_seam: pending_right_top.take(),
                            bottom_seam: None,
                        });
                        out.unified_rows.push(UnifiedRow::Code {
                            old_line: None,
                            new_line: Some(ln),
                            sign: '+',
                            text: clean,
                            spans: sp,
                            kind: Some(DiffChangeKind::Added),
                        });
                        right_y += DIFF_LINE_H;
                        unified_y += DIFF_LINE_H;
                    }
                }

                out.connectors.push(ConnectorSpan {
                    kind: cb.kind,
                    left_top,
                    left_bot: left_y,
                    right_top,
                    right_bot: right_y,
                    unified_top,
                });
                push_anchor(&mut out.anchors, left_y, right_y);
            }
            DiffBlock::Separator(hdr) => {
                out.left_rows.push(SideRow::Separator(hdr.clone()));
                out.right_rows.push(SideRow::Separator(hdr.clone()));
                out.unified_rows.push(UnifiedRow::Separator(hdr.clone()));
                left_y += DIFF_SEP_H;
                right_y += DIFF_SEP_H;
                unified_y += DIFF_SEP_H;
                push_anchor(&mut out.anchors, left_y, right_y);
            }
        }
    }

    // Pure-add or pure-delete file: ensure opposite side still renders a visible marker seam
    if let Some(seam) = pending_left_top.take() {
        out.left_rows.push(SideRow::Code {
            line_no: None,
            text: String::new(),
            spans: Vec::new(),
            kind: None,
            is_first: false,
            is_last: false,
            top_seam: Some(seam),
            bottom_seam: None,
        });
        left_y += DIFF_LINE_H;
    }
    if let Some(seam) = pending_right_top.take() {
        out.right_rows.push(SideRow::Code {
            line_no: None,
            text: String::new(),
            spans: Vec::new(),
            kind: None,
            is_first: false,
            is_last: false,
            top_seam: Some(seam),
            bottom_seam: None,
        });
        right_y += DIFF_LINE_H;
    }

    out.total_left_h = left_y;
    out.total_right_h = right_y;
    out.total_unified_h = unified_y;
    out
}

/// Piecewise-linear vertical scroll synchronization across `(left_y, right_y)` anchor pairs.
pub(crate) fn map_scroll_y(y: f32, anchors: &[(f32, f32)], left_to_right: bool) -> f32 {
    if anchors.is_empty() {
        return y.max(0.0);
    }
    let get = |pair: (f32, f32)| -> (f32, f32) {
        if left_to_right {
            (pair.0, pair.1)
        } else {
            (pair.1, pair.0)
        }
    };
    let (s0, d0) = get(anchors[0]);
    if y <= s0 {
        return d0;
    }
    for win in anchors.windows(2) {
        let (sa, da) = get(win[0]);
        let (sb, db) = get(win[1]);
        if y <= sb {
            let span = sb - sa;
            if span.abs() < 0.001 {
                return da;
            }
            let t = ((y - sa) / span).clamp(0.0, 1.0);
            return da + t * (db - da);
        }
    }
    let (s_last, d_last) = get(*anchors.last().unwrap());
    (d_last + (y - s_last)).max(0.0)
}
