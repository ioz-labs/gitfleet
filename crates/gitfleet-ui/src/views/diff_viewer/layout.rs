//! Diff layout builder, cached syntax/offset table (`DiffLayoutCache`),
//! and O(log N) viewport virtualization slicing for Unified and Side-by-Side views.

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
pub(crate) struct SplitCodeCell {
    pub(crate) line_no: Option<usize>,
    pub(crate) text: String,
    pub(crate) spans: Vec<InlineSpan>,
    pub(crate) kind: Option<DiffChangeKind>,
    pub(crate) is_first: bool,
    pub(crate) is_last: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum SplitRow {
    PairedCode {
        left: Option<SplitCodeCell>,
        right: Option<SplitCodeCell>,
        block_is_first: bool,
        block_is_last: bool,
    },
    Hidden {
        gap_id: usize,
        hidden_count: usize,
        can_expand_top: bool,
        can_expand_bottom: bool,
    },
    Separator(String),
}

impl SplitRow {
    #[inline]
    pub(crate) fn height(&self) -> f32 {
        match self {
            Self::PairedCode { .. } => DIFF_LINE_H,
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
    pub(crate) split_top: f32,
    pub(crate) split_bot: f32,
    pub(crate) unified_top: f32,
}

#[derive(Debug, Default)]
pub(crate) struct DiffLayout {
    pub(crate) split_rows: Vec<SplitRow>,
    pub(crate) unified_rows: Vec<UnifiedRow>,
    pub(crate) connectors: Vec<ConnectorSpan>,
    pub(crate) total_split_h: f32,
    pub(crate) total_unified_h: f32,
    pub(crate) max_side_chars: usize,
    pub(crate) max_unified_chars: usize,
}

pub(crate) type SyntaxSpans = Vec<(Range<usize>, TokenFormat)>;

/// Pre-computed diff layout + per-row syntax highlighting spans + cumulative Y offsets
/// for O(log N) viewport virtualization and zero per-frame layout/lexing overhead.
#[derive(Debug)]
pub(crate) struct DiffLayoutCache {
    pub(crate) layout: DiffLayout,
    pub(crate) split_y_offsets: Vec<f32>,
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
            split_y_offsets: vec![0.0],
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

    let mut split_y_offsets = Vec::with_capacity(layout.split_rows.len() + 1);
    let mut left_syntax = Vec::with_capacity(layout.split_rows.len());
    let mut right_syntax = Vec::with_capacity(layout.split_rows.len());
    let mut left_hl = DiffSyntaxHighlighter::new(language, theme_mode);
    let mut right_hl = DiffSyntaxHighlighter::new(language, theme_mode);
    let mut y = 0.0_f32;
    split_y_offsets.push(0.0);
    for row in &layout.split_rows {
        y += row.height();
        split_y_offsets.push(y);
        match row {
            SplitRow::PairedCode { left, right, .. } => {
                match left {
                    Some(cell) => left_syntax.push(left_hl.highlight_line(&cell.text)),
                    None => left_syntax.push(Vec::new()),
                }
                match right {
                    Some(cell) => right_syntax.push(right_hl.highlight_line(&cell.text)),
                    None => right_syntax.push(Vec::new()),
                }
            }
            SplitRow::Hidden { .. } | SplitRow::Separator(_) => {
                left_hl.reset();
                right_hl.reset();
                left_syntax.push(Vec::new());
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
        split_y_offsets,
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

    let start = y_offsets[1..]
        .partition_point(|&row_bot| row_bot <= view_top)
        .min(row_count);
    let end = y_offsets[..row_count]
        .partition_point(|&row_top| row_top < view_bot)
        .clamp(start, row_count);

    let top_spacer = y_offsets[start];
    let bottom_spacer = (total_h - y_offsets[end]).max(0.0);
    (start..end, top_spacer, bottom_spacer)
}

/// Pure layout generator shared by Side-by-Side view, Unified view, Next/Prev change jump buttons,
/// and unit tests. Pairs Left and Right rows onto a single unified vertical grid with shared
/// full-width `Hidden` context expanders.
pub(crate) fn build_diff_layout(
    parsed: &ParsedDiff,
    compact: bool,
    expanded_gaps: &HashMap<usize, DiffGapExpansion>,
) -> DiffLayout {
    let mut out = DiffLayout {
        max_side_chars: 32,
        max_unified_chars: 40,
        ..Default::default()
    };

    let mut split_y = 0.0_f32;
    let mut unified_y = 0.0_f32;
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
                    if kt + kb + 1 >= lines.len() {
                        (lines.len(), 0usize, 0usize)
                    } else {
                        (kt, kb, lines.len() - kt - kb)
                    }
                };

                for ctx in &lines[..keep_top] {
                    let clean = theme::sanitize_code_line(&ctx.text);
                    let len = clean.chars().count();
                    out.max_side_chars = out.max_side_chars.max(len);
                    out.max_unified_chars = out.max_unified_chars.max(len);

                    out.split_rows.push(SplitRow::PairedCode {
                        left: Some(SplitCodeCell {
                            line_no: Some(ctx.old_line),
                            text: clean.clone(),
                            spans: Vec::new(),
                            kind: None,
                            is_first: false,
                            is_last: false,
                        }),
                        right: Some(SplitCodeCell {
                            line_no: Some(ctx.new_line),
                            text: clean.clone(),
                            spans: Vec::new(),
                            kind: None,
                            is_first: false,
                            is_last: false,
                        }),
                        block_is_first: false,
                        block_is_last: false,
                    });
                    out.unified_rows.push(UnifiedRow::Code {
                        old_line: Some(ctx.old_line),
                        new_line: Some(ctx.new_line),
                        sign: ' ',
                        text: clean,
                        spans: Vec::new(),
                        kind: None,
                    });
                    split_y += DIFF_LINE_H;
                    unified_y += DIFF_LINE_H;
                }

                if hidden > 0 {
                    out.split_rows.push(SplitRow::Hidden {
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
                    split_y += DIFF_HIDDEN_H;
                    unified_y += DIFF_HIDDEN_H;

                    for ctx in &lines[lines.len() - keep_bot..] {
                        let clean = theme::sanitize_code_line(&ctx.text);
                        let len = clean.chars().count();
                        out.max_side_chars = out.max_side_chars.max(len);
                        out.max_unified_chars = out.max_unified_chars.max(len);

                        out.split_rows.push(SplitRow::PairedCode {
                            left: Some(SplitCodeCell {
                                line_no: Some(ctx.old_line),
                                text: clean.clone(),
                                spans: Vec::new(),
                                kind: None,
                                is_first: false,
                                is_last: false,
                            }),
                            right: Some(SplitCodeCell {
                                line_no: Some(ctx.new_line),
                                text: clean.clone(),
                                spans: Vec::new(),
                                kind: None,
                                is_first: false,
                                is_last: false,
                            }),
                            block_is_first: false,
                            block_is_last: false,
                        });
                        out.unified_rows.push(UnifiedRow::Code {
                            old_line: Some(ctx.old_line),
                            new_line: Some(ctx.new_line),
                            sign: ' ',
                            text: clean,
                            spans: Vec::new(),
                            kind: None,
                        });
                        split_y += DIFF_LINE_H;
                        unified_y += DIFF_LINE_H;
                    }
                }
            }
            DiffBlock::Change(cb) => {
                let split_top = split_y;
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

                // Unified view: all `-` lines followed by all `+` lines
                for (idx, clean) in old_clean.iter().enumerate() {
                    let len = clean.chars().count();
                    out.max_side_chars = out.max_side_chars.max(len);
                    out.max_unified_chars = out.max_unified_chars.max(len);
                    out.unified_rows.push(UnifiedRow::Code {
                        old_line: Some(cb.old_start + idx),
                        new_line: None,
                        sign: '-',
                        text: clean.clone(),
                        spans: old_spans[idx].clone(),
                        kind: Some(DiffChangeKind::Removed),
                    });
                    unified_y += DIFF_LINE_H;
                }
                for (idx, clean) in new_clean.iter().enumerate() {
                    let len = clean.chars().count();
                    out.max_side_chars = out.max_side_chars.max(len);
                    out.max_unified_chars = out.max_unified_chars.max(len);
                    out.unified_rows.push(UnifiedRow::Code {
                        old_line: None,
                        new_line: Some(cb.new_start + idx),
                        sign: '+',
                        text: clean.clone(),
                        spans: new_spans[idx].clone(),
                        kind: Some(DiffChangeKind::Added),
                    });
                    unified_y += DIFF_LINE_H;
                }

                // Side-by-Side view: pair Left and Right row-by-row on identical Y coordinates
                let old_n = old_clean.len();
                let new_n = new_clean.len();
                let row_len = old_n.max(new_n);
                let mut old_iter = old_clean.into_iter().zip(old_spans).enumerate();
                let mut new_iter = new_clean.into_iter().zip(new_spans).enumerate();

                for row_i in 0..row_len {
                    let left = old_iter.next().map(|(idx, (clean, sp))| SplitCodeCell {
                        line_no: Some(cb.old_start + idx),
                        text: clean,
                        spans: sp,
                        kind: Some(cb.kind),
                        is_first: idx == 0,
                        is_last: idx + 1 == old_n,
                    });
                    let right = new_iter.next().map(|(idx, (clean, sp))| SplitCodeCell {
                        line_no: Some(cb.new_start + idx),
                        text: clean,
                        spans: sp,
                        kind: Some(cb.kind),
                        is_first: idx == 0,
                        is_last: idx + 1 == new_n,
                    });
                    out.split_rows.push(SplitRow::PairedCode {
                        left,
                        right,
                        block_is_first: row_i == 0,
                        block_is_last: row_i + 1 == row_len,
                    });
                    split_y += DIFF_LINE_H;
                }

                out.connectors.push(ConnectorSpan {
                    kind: cb.kind,
                    split_top,
                    split_bot: split_y,
                    unified_top,
                });
            }
            DiffBlock::Separator(hdr) => {
                out.split_rows.push(SplitRow::Separator(hdr.clone()));
                out.unified_rows.push(UnifiedRow::Separator(hdr.clone()));
                split_y += DIFF_SEP_H;
                unified_y += DIFF_SEP_H;
            }
        }
    }

    out.total_split_h = split_y;
    out.total_unified_h = unified_y;
    out
}
