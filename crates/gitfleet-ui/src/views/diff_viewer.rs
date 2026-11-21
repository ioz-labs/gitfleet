use crate::app::{
    BottomViewMode, DiffGapExpandMode, DiffGapExpansion, DiffSelectionSide, DiffTextSelection,
    GitFleetApp, Message, PendingConfirm,
};
use crate::icons;
use crate::preview::{PreviewHighlightSettings, PreviewHighlighter, PreviewState};
use crate::theme::{self, Palette};
use crate::views::{slim_scroll_h, tip};
use gitfleet_git::{DiffBlock, DiffChangeKind, ParsedDiff};
use iced::advanced::layout as adv_layout;
use iced::advanced::text as adv_text;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, overlay, renderer};
use iced::widget::canvas::{self, Canvas, Geometry, Path as CanvasPath, Stroke};
use iced::widget::{
    Space, button, column, container, horizontal_rule, rich_text, row, span, text, text_editor,
    tooltip,
};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Padding, Point, Rectangle, Renderer,
    Shadow, Size, Theme, Vector, alignment, event, mouse,
};
use std::collections::HashMap;
use std::sync::OnceLock;

pub(crate) const DIFF_LINE_H: f32 = 19.0;
pub(crate) const DIFF_HIDDEN_H: f32 = 22.0;
pub(crate) const DIFF_SEP_H: f32 = 24.0;
const DIFF_CONTEXT_LINES: usize = 3;
const DIFF_EXPAND_STEP: usize = 20;
const LINE_NUM_W: f32 = 42.0;
const CONNECTOR_W: f32 = 28.0;
const OVERVIEW_BAR_W: f32 = 5.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InlineSpan {
    pub(crate) text: String,
    pub(crate) highlighted: bool,
}

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
    pub(crate) max_left_chars: usize,
    pub(crate) max_right_chars: usize,
    pub(crate) max_unified_chars: usize,
}

fn tokenize_code(s: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let ch = s[i..].chars().next().unwrap();
        if ch.is_alphanumeric() || ch == '_' {
            i += ch.len_utf8();
            while i < bytes.len() {
                let c = s[i..].chars().next().unwrap();
                if c.is_alphanumeric() || c == '_' {
                    i += c.len_utf8();
                } else {
                    break;
                }
            }
        } else if ch.is_whitespace() {
            i += ch.len_utf8();
            while i < bytes.len() {
                let c = s[i..].chars().next().unwrap();
                if c.is_whitespace() {
                    i += c.len_utf8();
                } else {
                    break;
                }
            }
        } else {
            i += ch.len_utf8();
        }
        tokens.push(&s[start..i]);
    }
    tokens
}

fn token_weight(tok: &str) -> usize {
    let first = tok.chars().next().unwrap_or(' ');
    if first.is_alphanumeric() || first == '_' {
        tok.chars().count().max(1) * 3
    } else if first.is_whitespace() {
        1
    } else {
        2
    }
}

fn coalesce_spans(tokens: &[&str], changed: &[bool]) -> Vec<InlineSpan> {
    let mut out: Vec<InlineSpan> = Vec::new();
    for (&tok, &is_chg) in tokens.iter().zip(changed.iter()) {
        if let Some(last) = out.last_mut()
            && last.highlighted == is_chg
        {
            last.text.push_str(tok);
        } else {
            out.push(InlineSpan {
                text: tok.to_string(),
                highlighted: is_chg,
            });
        }
    }
    out
}

fn full_changed_spans(line: &str) -> Vec<InlineSpan> {
    if line.is_empty() {
        return Vec::new();
    }
    let trimmed_start = line.len() - line.trim_start().len();
    if trimmed_start > 0 && trimmed_start < line.len() {
        vec![
            InlineSpan {
                text: line[..trimmed_start].to_string(),
                highlighted: false,
            },
            InlineSpan {
                text: line[trimmed_start..].to_string(),
                highlighted: true,
            },
        ]
    } else {
        vec![InlineSpan {
            text: line.to_string(),
            highlighted: true,
        }]
    }
}

/// Computes SmartGit-style intra-line token diff highlights for a modified `(old, new)` line pair
/// using prefix/suffix trimming + identifier-weighted LCS.
pub(crate) fn compute_inline_diff(old: &str, new: &str) -> (Vec<InlineSpan>, Vec<InlineSpan>) {
    if old == new {
        return (Vec::new(), Vec::new());
    }
    let a = tokenize_code(old);
    let b = tokenize_code(new);
    if a.is_empty() || b.is_empty() {
        return (full_changed_spans(old), full_changed_spans(new));
    }

    let mut prefix = 0;
    while prefix < a.len() && prefix < b.len() && a[prefix] == b[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < a.len() - prefix
        && suffix < b.len() - prefix
        && a[a.len() - 1 - suffix] == b[b.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let a_mid = &a[prefix..a.len() - suffix];
    let b_mid = &b[prefix..b.len() - suffix];

    let mut a_chg = vec![false; a.len()];
    let mut b_chg = vec![false; b.len()];

    if a_mid.is_empty() || b_mid.is_empty() || a_mid.len() > 160 || b_mid.len() > 160 {
        for flag in &mut a_chg[prefix..a.len() - suffix] {
            *flag = true;
        }
        for flag in &mut b_chg[prefix..b.len() - suffix] {
            *flag = true;
        }
    } else {
        let m = a_mid.len();
        let n = b_mid.len();
        let cols = n + 1;
        let mut dp = vec![0usize; (m + 1) * cols];
        for i in (0..m).rev() {
            for j in (0..n).rev() {
                dp[i * cols + j] = if a_mid[i] == b_mid[j] {
                    token_weight(a_mid[i]) + dp[(i + 1) * cols + (j + 1)]
                } else {
                    dp[(i + 1) * cols + j].max(dp[i * cols + (j + 1)])
                };
            }
        }

        // Backtrack to find unmatched tokens in `a_mid` and `b_mid`
        let mut i = 0;
        let mut j = 0;
        let mut matched_weight = 0usize;
        while i < m && j < n {
            if a_mid[i] == b_mid[j]
                && dp[i * cols + j] == token_weight(a_mid[i]) + dp[(i + 1) * cols + (j + 1)]
            {
                matched_weight += token_weight(a_mid[i]);
                i += 1;
                j += 1;
            } else if dp[(i + 1) * cols + j] >= dp[i * cols + (j + 1)] {
                a_chg[prefix + i] = true;
                i += 1;
            } else {
                b_chg[prefix + j] = true;
                j += 1;
            }
        }
        while i < m {
            a_chg[prefix + i] = true;
            i += 1;
        }
        while j < n {
            b_chg[prefix + j] = true;
            j += 1;
        }

        // If the entire line shares almost no meaningful tokens, highlight full changed middle cleanly
        let total_weight: usize = a.iter().map(|t| token_weight(t)).sum::<usize>()
            + b.iter().map(|t| token_weight(t)).sum::<usize>();
        let prefix_suffix_weight: usize = a[..prefix]
            .iter()
            .chain(&a[a.len() - suffix..])
            .map(|t| token_weight(t))
            .sum::<usize>()
            * 2;
        if total_weight > 0 && (prefix_suffix_weight + matched_weight * 2) * 4 < total_weight {
            return (full_changed_spans(old), full_changed_spans(new));
        }

        // Smooth single punctuation/space tokens trapped directly between two changed spans
        let smooth = |tokens: &[&str], flags: &mut [bool]| {
            for k in 1..flags.len().saturating_sub(1) {
                if !flags[k]
                    && flags[k - 1]
                    && flags[k + 1]
                    && tokens[k]
                        .chars()
                        .all(|c| c.is_whitespace() || c == ',' || c == '*')
                {
                    flags[k] = true;
                }
            }
        };
        smooth(&a, &mut a_chg);
        smooth(&b, &mut b_chg);
    }

    (coalesce_spans(&a, &a_chg), coalesce_spans(&b, &b_chg))
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

fn change_colors(kind: DiffChangeKind, p: Palette) -> (Color, Color, Color) {
    // Returns (soft_bg, accent_border, text_fg)
    match kind {
        DiffChangeKind::Added => (p.success_bg, p.success_fg, p.text_primary),
        DiffChangeKind::Removed => (p.danger_bg, p.danger_fg, p.text_primary),
        DiffChangeKind::Modified => (p.warning_bg, p.warning_fg, p.text_primary),
    }
}

fn diff_code_char_width() -> f32 {
    static CHAR_W: OnceLock<f32> = OnceLock::new();
    *CHAR_W.get_or_init(|| {
        use adv_text::{Paragraph as _, Renderer as TextRenderer};
        type Para = <Renderer as TextRenderer>::Paragraph;
        let sample = "MMMMMMMMMM";
        let para = Para::with_text(adv_text::Text {
            content: sample,
            bounds: Size::INFINITY,
            size: iced::Pixels(12.0),
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
            7.2
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
        size: iced::Pixels(12.0),
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

fn col_to_x(line: &str, col: usize, char_w: f32) -> f32 {
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

fn x_to_col(line: &str, rel_x: f32, char_w: f32) -> usize {
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

struct DiffRowSelConfig {
    side: DiffSelectionSide,
    row_idx: usize,
    code_start_x: f32,
    line_text: String,
    sel_range: Option<(usize, usize, bool)>,
    is_dragging_same_side: bool,
    sel_bg: Color,
    sel_overlay: Color,
}

/// Single fixed-height diff row widget that paints selective top/bottom borders, 2px SmartGit
/// insertion/deletion seam hairlines, and character-accurate drag-to-select highlights.
#[allow(clippy::too_many_arguments)]
fn diff_code_row_box<'a>(
    bg: Color,
    top_border: Option<Color>,
    bottom_border: Option<Color>,
    top_seam: Option<Color>,
    bottom_seam: Option<Color>,
    gutter_x: f32,
    gutter_border: Color,
    sel: DiffRowSelConfig,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    struct DiffLineBox<'a> {
        bg: Color,
        top_border: Option<Color>,
        bottom_border: Option<Color>,
        top_seam: Option<Color>,
        bottom_seam: Option<Color>,
        gutter_x: f32,
        gutter_border: Color,
        sel: DiffRowSelConfig,
        content: Element<'a, Message>,
    }

    impl<'a> Widget<Message, Theme, Renderer> for DiffLineBox<'a> {
        fn tag(&self) -> tree::Tag {
            tree::Tag::stateless()
        }

        fn state(&self) -> tree::State {
            tree::State::None
        }

        fn children(&self) -> Vec<Tree> {
            vec![Tree::new(&self.content)]
        }

        fn diff(&self, tree: &mut Tree) {
            tree.diff_children(std::slice::from_ref(&self.content));
        }

        fn size(&self) -> Size<Length> {
            Size::new(Length::Fill, Length::Fixed(DIFF_LINE_H))
        }

        fn layout(
            &self,
            tree: &mut Tree,
            renderer: &Renderer,
            limits: &adv_layout::Limits,
        ) -> adv_layout::Node {
            let max_w = if limits.max().width.is_finite() {
                limits.max().width
            } else {
                4096.0
            };
            let child_limits = adv_layout::Limits::new(
                Size::new(max_w, DIFF_LINE_H),
                Size::new(max_w, DIFF_LINE_H),
            );
            let child = self
                .content
                .as_widget()
                .layout(&mut tree.children[0], renderer, &child_limits);
            adv_layout::Node::with_children(Size::new(max_w, DIFF_LINE_H), vec![child])
        }

        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            use iced::advanced::Renderer as _;

            let b = layout.bounds();
            let flat_quad = |bounds: Rectangle| renderer::Quad {
                bounds,
                border: Border::default(),
                shadow: Shadow::default(),
            };

            if self.bg != Color::TRANSPARENT {
                renderer.fill_quad(flat_quad(b), Background::Color(self.bg));
            }
            if self.gutter_x > 0.0 {
                renderer.fill_quad(
                    flat_quad(Rectangle {
                        x: b.x + self.gutter_x,
                        y: b.y,
                        width: 1.0,
                        height: b.height,
                    }),
                    Background::Color(self.gutter_border),
                );
            }
            if let Some(c) = self.top_border {
                renderer.fill_quad(
                    flat_quad(Rectangle {
                        x: b.x,
                        y: b.y,
                        width: b.width,
                        height: 1.0,
                    }),
                    Background::Color(Color { a: 0.72, ..c }),
                );
            }
            if let Some(c) = self.bottom_border {
                renderer.fill_quad(
                    flat_quad(Rectangle {
                        x: b.x,
                        y: b.y + b.height - 1.0,
                        width: b.width,
                        height: 1.0,
                    }),
                    Background::Color(Color { a: 0.72, ..c }),
                );
            }
            if let Some(c) = self.top_seam {
                renderer.fill_quad(
                    flat_quad(Rectangle {
                        x: b.x,
                        y: b.y,
                        width: b.width,
                        height: 2.0,
                    }),
                    Background::Color(c),
                );
            }
            if let Some(c) = self.bottom_seam {
                renderer.fill_quad(
                    flat_quad(Rectangle {
                        x: b.x,
                        y: b.y + b.height - 2.0,
                        width: b.width,
                        height: 2.0,
                    }),
                    Background::Color(c),
                );
            }

            let sel_rect = self.sel.sel_range.and_then(|(s_col, e_col, extends_eol)| {
                let char_w = diff_code_char_width();
                let x0 = b.x + self.sel.code_start_x + col_to_x(&self.sel.line_text, s_col, char_w);
                let x1 = b.x
                    + self.sel.code_start_x
                    + col_to_x(&self.sel.line_text, e_col, char_w)
                    + if extends_eol { char_w } else { 0.0 };
                let w = (x1 - x0).max(if extends_eol { char_w } else { 0.0 });
                (w > 0.5).then_some(Rectangle {
                    x: x0,
                    y: b.y,
                    width: w,
                    height: b.height,
                })
            });

            if let Some(rect) = sel_rect {
                renderer.fill_quad(flat_quad(rect), Background::Color(self.sel.sel_bg));
            }

            if let Some(child_layout) = layout.children().next() {
                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    child_layout,
                    cursor,
                    viewport,
                );
            }

            if let Some(rect) = sel_rect {
                renderer.fill_quad(flat_quad(rect), Background::Color(self.sel.sel_overlay));
            }
        }

        fn on_event(
            &mut self,
            tree: &mut Tree,
            event: iced::Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &Renderer,
            clipboard: &mut dyn Clipboard,
            shell: &mut Shell<'_, Message>,
            viewport: &Rectangle,
        ) -> event::Status {
            let b = layout.bounds();
            match &event {
                iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                    if let Some(pos) = cursor.position_in(b)
                        && cursor
                            .position()
                            .is_some_and(|abs| viewport.contains(abs))
                    {
                        let char_w = diff_code_char_width();
                        let col = x_to_col(&self.sel.line_text, pos.x - self.sel.code_start_x, char_w);
                        shell.publish(Message::DiffSelStart {
                            side: self.sel.side,
                            row: self.sel.row_idx,
                            col,
                        });
                        return event::Status::Captured;
                    }
                }
                iced::Event::Mouse(mouse::Event::CursorMoved { .. })
                    if self.sel.is_dragging_same_side =>
                {
                    if let Some(abs_pos) = cursor.position()
                        && abs_pos.y >= b.y
                        && abs_pos.y < b.y + b.height
                        && abs_pos.y >= viewport.y
                        && abs_pos.y <= viewport.y + viewport.height
                    {
                        let char_w = diff_code_char_width();
                        let col = x_to_col(
                            &self.sel.line_text,
                            abs_pos.x - b.x - self.sel.code_start_x,
                            char_w,
                        );
                        shell.publish(Message::DiffSelDrag {
                            side: self.sel.side,
                            row: self.sel.row_idx,
                            col,
                        });
                    }
                }
                _ => {}
            }

            let Some(child_layout) = layout.children().next() else {
                return event::Status::Ignored;
            };
            self.content.as_widget_mut().on_event(
                &mut tree.children[0],
                event,
                child_layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            )
        }

        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            renderer: &Renderer,
        ) -> mouse::Interaction {
            let b = layout.bounds();
            if let Some(pos) = cursor.position_in(b)
                && pos.x >= self.sel.code_start_x - 4.0
            {
                return mouse::Interaction::Text;
            }
            let Some(child_layout) = layout.children().next() else {
                return mouse::Interaction::None;
            };
            self.content.as_widget().mouse_interaction(
                &tree.children[0],
                child_layout,
                cursor,
                viewport,
                renderer,
            )
        }

        fn operate(
            &self,
            tree: &mut Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            operation: &mut dyn iced::advanced::widget::Operation,
        ) {
            if let Some(child_layout) = layout.children().next() {
                self.content.as_widget().operate(
                    &mut tree.children[0],
                    child_layout,
                    renderer,
                    operation,
                );
            }
        }

        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            translation: Vector,
        ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
            let child_layout = layout.children().next()?;
            self.content.as_widget_mut().overlay(
                &mut tree.children[0],
                child_layout,
                renderer,
                translation,
            )
        }
    }

    Element::new(DiffLineBox {
        bg,
        top_border,
        bottom_border,
        top_seam,
        bottom_seam,
        gutter_x,
        gutter_border,
        sel,
        content: content.into(),
    })
}

/// Center Bezier connector strip connecting Left (Old) and Right (New) diff blocks like SmartGit.
struct DiffConnectorCanvas {
    spans: Vec<ConnectorSpan>,
    left_scroll_y: f32,
    right_scroll_y: f32,
    p: Palette,
}

impl canvas::Program<Message> for DiffConnectorCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;

        // Subtle center divider backdrop
        frame.fill_rectangle(
            Point::ORIGIN,
            bounds.size(),
            self.p.surface_titlebar,
        );
        frame.fill_rectangle(
            Point::new(0.0, 0.0),
            Size::new(1.0, h),
            self.p.border_subtle,
        );
        frame.fill_rectangle(
            Point::new((w - 1.0).max(0.0), 0.0),
            Size::new(1.0, h),
            self.p.border_subtle,
        );

        let mid_x = w * 0.5;
        for span in &self.spans {
            let mut ly0 = span.left_top - self.left_scroll_y;
            let mut ly1 = span.left_bot - self.left_scroll_y;
            let mut ry0 = span.right_top - self.right_scroll_y;
            let mut ry1 = span.right_bot - self.right_scroll_y;

            if ly1.max(ry1) < -6.0 || ly0.min(ry0) > h + 6.0 {
                continue;
            }
            if (ly1 - ly0).abs() < 2.0 {
                ly0 -= 1.0;
                ly1 += 1.0;
            }
            if (ry1 - ry0).abs() < 2.0 {
                ry0 -= 1.0;
                ry1 += 1.0;
            }

            let (_, accent, _) = change_colors(span.kind, self.p);
            let fill = Color { a: 0.24, ..accent };
            let stroke_color = Color { a: 0.78, ..accent };

            let ribbon = CanvasPath::new(|b| {
                b.move_to(Point::new(0.0, ly0));
                b.bezier_curve_to(
                    Point::new(mid_x, ly0),
                    Point::new(mid_x, ry0),
                    Point::new(w, ry0),
                );
                b.line_to(Point::new(w, ry1));
                b.bezier_curve_to(
                    Point::new(mid_x, ry1),
                    Point::new(mid_x, ly1),
                    Point::new(0.0, ly1),
                );
                b.close();
            });
            frame.fill(&ribbon, fill);

            let top_edge = CanvasPath::new(|b| {
                b.move_to(Point::new(0.0, ly0));
                b.bezier_curve_to(
                    Point::new(mid_x, ly0),
                    Point::new(mid_x, ry0),
                    Point::new(w, ry0),
                );
            });
            let bot_edge = CanvasPath::new(|b| {
                b.move_to(Point::new(0.0, ly1));
                b.bezier_curve_to(
                    Point::new(mid_x, ly1),
                    Point::new(mid_x, ry1),
                    Point::new(w, ry1),
                );
            });
            let stroke = Stroke::default().with_color(stroke_color).with_width(1.2);
            frame.stroke(&top_edge, stroke);
            frame.stroke(&bot_edge, stroke);
        }

        vec![frame.into_geometry()]
    }
}

/// Thin SmartGit edge minimap strip highlighting where added/removed/modified blocks live.
struct DiffOverviewBar {
    spans: Vec<ConnectorSpan>,
    is_left: bool,
    total_h: f32,
    p: Palette,
}

impl canvas::Program<Message> for DiffOverviewBar {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), self.p.surface_titlebar);
        if self.total_h <= 1.0 || bounds.height <= 1.0 {
            return vec![frame.into_geometry()];
        }
        let scale = bounds.height / self.total_h;
        for span in &self.spans {
            let (top, bot) = if self.is_left {
                (span.left_top, span.left_bot)
            } else {
                (span.right_top, span.right_bot)
            };
            let (_, accent, _) = change_colors(span.kind, self.p);
            let y = (top * scale).clamp(0.0, (bounds.height - 3.0).max(0.0));
            let h = ((bot - top) * scale).max(3.0).min(bounds.height - y);
            frame.fill_rectangle(
                Point::new(1.0, y),
                Size::new((bounds.width - 2.0).max(2.0), h),
                accent,
            );
        }
        vec![frame.into_geometry()]
    }
}

fn hidden_gap_bar<'a>(
    gap_id: usize,
    hidden_count: usize,
    can_expand_top: bool,
    can_expand_bottom: bool,
    p: Palette,
) -> Element<'a, Message> {
    let mut bar = row![
        icons::chevron_up(10.0, p.text_muted),
        text(format!(
            "{hidden_count} {} hidden",
            if hidden_count == 1 { "line" } else { "lines" }
        ))
        .size(11)
        .color(p.text_muted)
        .wrapping(text::Wrapping::None),
    ]
    .spacing(5)
    .align_y(Alignment::Center);

    if hidden_count > DIFF_EXPAND_STEP && can_expand_top {
        bar = bar.push(
            button(
                row![
                    icons::chevron_down(9.0, p.accent_fg),
                    text(format!("+{DIFF_EXPAND_STEP}")).size(10).color(p.accent_fg),
                ]
                .spacing(2)
                .align_y(Alignment::Center),
            )
            .padding([1, 5])
            .style(theme::subtle_btn(p))
            .on_press(Message::ExpandDiffGap {
                gap_id,
                mode: DiffGapExpandMode::Top(DIFF_EXPAND_STEP),
            }),
        );
    }

    bar = bar.push(
        button(
            row![
                icons::chevron_up(9.0, p.text_secondary),
                text("show").size(10.5).color(p.text_primary),
                icons::chevron_down(9.0, p.text_secondary),
            ]
            .spacing(3)
            .align_y(Alignment::Center),
        )
        .padding([1, 7])
        .style(theme::subtle_btn(p))
        .on_press(Message::ExpandDiffGap {
            gap_id,
            mode: DiffGapExpandMode::All,
        }),
    );

    if hidden_count > DIFF_EXPAND_STEP && can_expand_bottom {
        bar = bar.push(
            button(
                row![
                    icons::chevron_up(9.0, p.accent_fg),
                    text(format!("+{DIFF_EXPAND_STEP}")).size(10).color(p.accent_fg),
                ]
                .spacing(2)
                .align_y(Alignment::Center),
            )
            .padding([1, 5])
            .style(theme::subtle_btn(p))
            .on_press(Message::ExpandDiffGap {
                gap_id,
                mode: DiffGapExpandMode::Bottom(DIFF_EXPAND_STEP),
            }),
        );
    }

    container(bar)
        .width(Length::Fill)
        .height(Length::Fixed(DIFF_HIDDEN_H))
        .padding([0, 10])
        .align_y(alignment::Vertical::Center)
        .style(move |_| container::Style {
            background: Some(Background::Color(p.surface_card)),
            text_color: Some(p.text_muted),
            border: Border {
                color: p.border_subtle,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn render_highlighted_code<'a>(
    code: &str,
    spans: &[InlineSpan],
    fg: Color,
    accent: Option<Color>,
) -> Element<'a, Message> {
    if spans.is_empty() || !spans.iter().any(|s| s.highlighted) {
        return text(code.to_string())
            .size(12)
            .font(theme::CODE_FONT)
            .color(fg)
            .wrapping(text::Wrapping::None)
            .into();
    }
    let hl_accent = accent.unwrap_or(fg);
    let hl_bg = Color {
        a: 0.34,
        ..hl_accent
    };
    let hl_border = Border {
        color: Color {
            a: 0.50,
            ..hl_accent
        },
        width: 1.0,
        radius: 2.0.into(),
    };
    let rich_spans: Vec<text::Span<'a, Message, iced::Font>> = spans
        .iter()
        .map(|seg| {
            let s = span(seg.text.clone())
                .font(theme::CODE_FONT)
                .size(12)
                .color(fg);
            if seg.highlighted {
                s.background(hl_bg).border(hl_border).padding([1, 0])
            } else {
                s
            }
        })
        .collect();
    rich_text(rich_spans)
        .size(12)
        .font(theme::CODE_FONT)
        .wrapping(text::Wrapping::None)
        .into()
}

fn render_side_row<'a>(
    item: &SideRow,
    side: DiffSelectionSide,
    row_idx: usize,
    selection: Option<DiffTextSelection>,
    p: Palette,
) -> Element<'a, Message> {
    match item {
        SideRow::Code {
            line_no,
            text: code,
            spans,
            kind,
            is_first,
            is_last,
            top_seam,
            bottom_seam,
        } => {
            let (bg, border_c, fg) = match kind {
                Some(k) => {
                    let (b, a, t) = change_colors(*k, p);
                    (b, Some(a), t)
                }
                None => (Color::TRANSPARENT, None, p.text_secondary),
            };
            let top_border = if *is_first { border_c } else { None };
            let bottom_border = if *is_last { border_c } else { None };
            let top_seam_c = top_seam.map(|k| change_colors(k, p).1);
            let bot_seam_c = bottom_seam.map(|k| change_colors(k, p).1);

            let ln_str = line_no.map(|n| n.to_string()).unwrap_or_default();
            let gutter = container(
                text(ln_str)
                    .size(11)
                    .font(theme::CODE_FONT)
                    .color(p.text_muted)
                    .wrapping(text::Wrapping::None),
            )
            .width(Length::Fixed(LINE_NUM_W))
            .height(Length::Fixed(DIFF_LINE_H))
            .padding([0, 6])
            .align_x(alignment::Horizontal::Right)
            .align_y(alignment::Vertical::Center);

            let code_cell = container(render_highlighted_code(code, spans, fg, border_c))
                .width(Length::Fill)
                .height(Length::Fixed(DIFF_LINE_H))
                .padding(Padding {
                    top: 0.0,
                    right: 8.0,
                    bottom: 0.0,
                    left: 8.0,
                })
                .align_y(alignment::Vertical::Center);

            let char_len = code.chars().count();
            let sel_cfg = DiffRowSelConfig {
                side,
                row_idx,
                code_start_x: LINE_NUM_W + 8.0,
                line_text: code.clone(),
                sel_range: row_selection_range(selection, side, row_idx, char_len),
                is_dragging_same_side: selection
                    .is_some_and(|s| s.dragging && s.side == side),
                sel_bg: Color {
                    a: 0.42,
                    ..p.accent_primary
                },
                sel_overlay: Color {
                    a: 0.16,
                    ..p.accent_hover
                },
            };

            diff_code_row_box(
                bg,
                top_border,
                bottom_border,
                top_seam_c,
                bot_seam_c,
                LINE_NUM_W,
                p.border_subtle,
                sel_cfg,
                row![gutter, code_cell].align_y(Alignment::Center),
            )
        }
        SideRow::Hidden {
            gap_id,
            hidden_count,
            can_expand_top,
            can_expand_bottom,
        } => hidden_gap_bar(*gap_id, *hidden_count, *can_expand_top, *can_expand_bottom, p),
        SideRow::Separator(hdr) => container(
            text(theme::sanitize_code_line(hdr))
                .size(11)
                .font(theme::CODE_FONT)
                .color(p.accent_fg)
                .wrapping(text::Wrapping::None),
        )
        .width(Length::Fill)
        .height(Length::Fixed(DIFF_SEP_H))
        .padding([0, 8])
        .align_y(alignment::Vertical::Center)
        .style(theme::diff_line_box(p.fill_selected, p.accent_fg))
        .into(),
    }
}

impl GitFleetApp {
    pub(crate) fn view_bottom_viewer(&self, p: Palette) -> Element<'_, Message> {
        let preview_badge = self.preview_state.kind_badge();
        let is_diff_mode = matches!(
            self.bottom_mode,
            BottomViewMode::UnifiedDiff | BottomViewMode::SideBySideDiff
        );
        let stats = self.diff_parsed.stats;
        let has_changes = stats.added_blocks + stats.removed_blocks + stats.modified_blocks > 0;

        let mut header = row![
            theme::clipped_line(
                text(self.diff_title.clone()).size(12).color(p.text_primary),
                Length::Fill,
            ),
            if self.bottom_mode == BottomViewMode::Preview && !preview_badge.is_empty() {
                Element::from(
                    container(
                        text(preview_badge)
                            .size(10)
                            .color(p.accent_fg)
                            .wrapping(text::Wrapping::None),
                    )
                    .padding([2, 7])
                    .style(theme::pill_box(p.fill_selected, p.accent_fg, p.border_default)),
                )
            } else {
                Space::with_width(Length::Shrink).into()
            },
            if self.editor_dirty {
                Element::from(
                    row![
                        container(Space::new(6, 6)).style(theme::dot_box(p.warning_fg)),
                        text("unsaved")
                            .size(11)
                            .color(p.warning_fg)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
            } else {
                Space::with_width(Length::Shrink).into()
            },
        ]
        .spacing(5)
        .align_y(Alignment::Center);

        if is_diff_mode && has_changes {
            let mut stat_row = row![].spacing(4).align_y(Alignment::Center);
            if stats.added_blocks > 0 {
                stat_row = stat_row.push(
                    text(format!("+{}", stats.added_blocks))
                        .size(11)
                        .font(theme::CODE_FONT)
                        .color(p.success_fg)
                        .wrapping(text::Wrapping::None),
                );
            }
            if stats.removed_blocks > 0 {
                stat_row = stat_row.push(
                    text(format!("-{}", stats.removed_blocks))
                        .size(11)
                        .font(theme::CODE_FONT)
                        .color(p.danger_fg)
                        .wrapping(text::Wrapping::None),
                );
            }
            if stats.modified_blocks > 0 {
                stat_row = stat_row.push(
                    text(format!("~{}", stats.modified_blocks))
                        .size(11)
                        .font(theme::CODE_FONT)
                        .color(p.warning_fg)
                        .wrapping(text::Wrapping::None),
                );
            }
            header = header.push(tip(
                container(stat_row)
                    .padding([2, 6])
                    .style(theme::section_panel_box(p)),
                "Change blocks: +Added  -Removed  ~Modified",
                tooltip::Position::Top,
                p,
            ));
        }

        if self.conflict_file_path.is_some() {
            let unresolved = self.conflict_parsed.unresolved_count();
            let resolve_label = if unresolved > 0 {
                format!("Resolve ({unresolved})")
            } else {
                "Resolve ✓".to_string()
            };
            header = header.push(tip(
                button(
                    text(resolve_label)
                        .size(11)
                        .color(if self.bottom_mode == BottomViewMode::ResolveConflicts {
                            p.accent_fg
                        } else if unresolved > 0 {
                            p.danger_fg
                        } else {
                            p.success_fg
                        })
                        .wrapping(text::Wrapping::None),
                )
                .padding([3, 8])
                .style(theme::filter_pill_btn(
                    p,
                    self.bottom_mode == BottomViewMode::ResolveConflicts,
                ))
                .on_press(Message::SetBottomViewMode(BottomViewMode::ResolveConflicts)),
                "Interactive Conflict Solver (Ours / Theirs / Both / Manual Edit)",
                tooltip::Position::Top,
                p,
            ));
        }

        header = header
            .push(tip(
                button(text("Unified").size(11).wrapping(text::Wrapping::None))
                    .padding([3, 8])
                    .style(theme::filter_pill_btn(
                        p,
                        self.bottom_mode == BottomViewMode::UnifiedDiff,
                    ))
                    .on_press(Message::SetBottomViewMode(BottomViewMode::UnifiedDiff)),
                "Unified Diff with Line Numbers & Hunk Staging",
                tooltip::Position::Top,
                p,
            ))
            .push(tip(
                button(text("Side by Side").size(11).wrapping(text::Wrapping::None))
                    .padding([3, 8])
                    .style(theme::filter_pill_btn(
                        p,
                        self.bottom_mode == BottomViewMode::SideBySideDiff,
                    ))
                    .on_press(Message::SetBottomViewMode(BottomViewMode::SideBySideDiff)),
                "SmartGit-style Side-by-Side Diff (Synced Vertical & Horizontal Scroll + Connectors)",
                tooltip::Position::Top,
                p,
            ));

        if is_diff_mode {
            header = header.push(tip(
                button(text("Compact").size(11).wrapping(text::Wrapping::None))
                    .padding([3, 8])
                    .style(theme::filter_pill_btn(p, self.diff_compact))
                    .on_press(Message::ToggleDiffCompact),
                if self.diff_compact {
                    "Compact mode ON (hides unchanged lines — click to show full file)"
                } else {
                    "Compact mode OFF (showing full file — click to collapse unchanged lines)"
                },
                tooltip::Position::Top,
                p,
            ));
            if has_changes {
                header = header
                    .push(tip(
                        button(icons::push(11.0, p.text_secondary))
                            .padding([4, 6])
                            .style(theme::subtle_btn(p))
                            .on_press(Message::DiffJumpChange(-1)),
                        "Previous Change",
                        tooltip::Position::Top,
                        p,
                    ))
                    .push(tip(
                        button(icons::pull(11.0, p.text_secondary))
                            .padding([4, 6])
                            .style(theme::subtle_btn(p))
                            .on_press(Message::DiffJumpChange(1)),
                        "Next Change",
                        tooltip::Position::Top,
                        p,
                    ));
            }
        }

        header = header
            .push(tip(
                button(text("Preview").size(11).wrapping(text::Wrapping::None))
                    .padding([3, 8])
                    .style(theme::filter_pill_btn(
                        p,
                        self.bottom_mode == BottomViewMode::Preview,
                    ))
                    .on_press(Message::SetBottomViewMode(BottomViewMode::Preview)),
                "Selectable Rich Preview (.md, images, SVG, JSON, CSV, configs, code)",
                tooltip::Position::Top,
                p,
            ))
            .push(tip(
                button(text("Edit").size(11).wrapping(text::Wrapping::None))
                    .padding([3, 8])
                    .style(theme::filter_pill_btn(
                        p,
                        self.bottom_mode == BottomViewMode::Edit,
                    ))
                    .on_press(Message::SetBottomViewMode(BottomViewMode::Edit)),
                "Live File Editor (Ctrl+S to save)",
                tooltip::Position::Top,
                p,
            ));

        if self.bottom_mode == BottomViewMode::Preview {
            if self.preview_state.has_rich_view() {
                header = header.push(tip(
                    button(
                        text(if self.preview_raw_mode {
                            "Rendered"
                        } else {
                            "Select Text"
                        })
                        .size(11)
                        .wrapping(text::Wrapping::None),
                    )
                    .padding([3, 8])
                    .style(theme::filter_pill_btn(p, self.preview_raw_mode))
                    .on_press(Message::TogglePreviewRaw),
                    "Switch between rich rendered view and selectable raw text",
                    tooltip::Position::Top,
                    p,
                ));
            }
            if self.preview_state.copyable_text().is_some() {
                let has_preview_sel = self
                    .preview_editor
                    .selection()
                    .is_some_and(|s| !s.is_empty());
                header = header.push(tip(
                    button(
                        row![
                            icons::copy(
                                11.0,
                                if has_preview_sel {
                                    p.accent_fg
                                } else {
                                    p.text_secondary
                                }
                            ),
                            text(if has_preview_sel { "Copy Sel" } else { "Copy" })
                                .size(11)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                    )
                    .padding([3, 8])
                    .style(theme::filter_pill_btn(p, has_preview_sel))
                    .on_press(Message::CopyPreviewText),
                    "Copy selected preview text (Ctrl+C), or full preview content",
                    tooltip::Position::Top,
                    p,
                ));
            }
        }

        if self.bottom_mode == BottomViewMode::Edit {
            header = header
                .push(
                    button(text("Open File…").size(10))
                        .padding([3, 7])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::PickOpenIgnoredFile),
                )
                .push(
                    button(text("Save As…").size(10))
                        .padding([3, 7])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::PickSaveDocAs),
                )
                .push(
                    button(
                        row![icons::save(12.0, Color::WHITE), text("Save (Ctrl+S)").size(11)]
                            .spacing(4)
                            .align_y(Alignment::Center),
                    )
                    .padding([3, 9])
                    .style(theme::primary_btn(p))
                    .on_press_maybe(self.editor_dirty.then_some(Message::SaveDocument)),
                );
        }

        let body: Element<'_, Message> = match self.bottom_mode {
            BottomViewMode::UnifiedDiff => self.view_unified_diff(p),
            BottomViewMode::SideBySideDiff => self.view_side_by_side_diff(p),
            BottomViewMode::Preview => self.view_file_preview(p),
            BottomViewMode::Edit => self.view_file_editor(p),
            BottomViewMode::ResolveConflicts => self.view_conflict_solver(p),
        };

        column![
            header,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            body
        ]
            .spacing(6)
            .height(Length::Fill)
            .into()
    }

    /// Unified Diff with dual `old | new` line numbers, Compact/Full toggle, expandable hidden
    /// sections, and per-change Stage/Unstage/Discard Hunk controls.
    pub(crate) fn view_unified_diff(&self, p: Palette) -> Element<'_, Message> {
        if self.diff_parsed.raw.is_empty() {
            return theme::empty_state(
                p,
                icons::file_code(theme::icon_size::LG, p.text_muted),
                "No diff selected",
                "Select a file from Changed, Dir, or All Files to inspect diff, preview, or edit.",
                None,
            );
        }

        if self.diff_parsed.blocks.is_empty() {
            let mut raw_col = column![].spacing(0);
            let mut max_chars: usize = 40;
            let raw_dragging = self
                .diff_selection
                .is_some_and(|s| s.dragging && s.side == DiffSelectionSide::Raw);
            for (i, line) in self.diff_parsed.raw.lines().take(2000).enumerate() {
                let clean = theme::sanitize_code_line(line);
                let char_len = clean.chars().count();
                max_chars = max_chars.max(char_len);
                let gutter = container(
                    text(format!("{}", i + 1))
                        .size(11)
                        .font(theme::CODE_FONT)
                        .color(p.text_muted)
                        .wrapping(text::Wrapping::None),
                )
                .width(Length::Fixed(LINE_NUM_W))
                .height(Length::Fixed(DIFF_LINE_H))
                .padding([0, 6])
                .align_x(alignment::Horizontal::Right)
                .align_y(alignment::Vertical::Center);
                let code_cell = container(
                    text(clean.clone())
                        .size(12)
                        .font(theme::CODE_FONT)
                        .color(p.text_secondary)
                        .wrapping(text::Wrapping::None),
                )
                .width(Length::Fill)
                .height(Length::Fixed(DIFF_LINE_H))
                .padding([0, 8])
                .align_y(alignment::Vertical::Center);
                let sel_cfg = DiffRowSelConfig {
                    side: DiffSelectionSide::Raw,
                    row_idx: i,
                    code_start_x: LINE_NUM_W + 8.0,
                    line_text: clean,
                    sel_range: row_selection_range(
                        self.diff_selection,
                        DiffSelectionSide::Raw,
                        i,
                        char_len,
                    ),
                    is_dragging_same_side: raw_dragging,
                    sel_bg: Color {
                        a: 0.42,
                        ..p.accent_primary
                    },
                    sel_overlay: Color {
                        a: 0.16,
                        ..p.accent_hover
                    },
                };
                raw_col = raw_col.push(diff_code_row_box(
                    Color::TRANSPARENT,
                    None,
                    None,
                    None,
                    None,
                    LINE_NUM_W,
                    p.border_subtle,
                    sel_cfg,
                    row![gutter, code_cell].align_y(Alignment::Center),
                ));
            }
            let min_w = (max_chars as f32 * 7.5 + LINE_NUM_W + 32.0).clamp(380.0, 2800.0);
            return slim_scroll_h(raw_col, min_w, p);
        }

        let layout = build_diff_layout(
            &self.diff_parsed,
            self.diff_compact,
            &self.diff_expanded_gaps,
        );
        let mut lines_col = column![].spacing(0);
        let gutter_total = LINE_NUM_W * 2.0 + 16.0;
        let unified_dragging = self
            .diff_selection
            .is_some_and(|s| s.dragging && s.side == DiffSelectionSide::Unified);

        for (row_idx, item) in layout.unified_rows.iter().enumerate() {
            match item {
                UnifiedRow::HunkHeader {
                    header,
                    patch,
                    kind,
                } => {
                    let (_, accent, _) = change_colors(*kind, p);
                    let mut bar = row![
                        text(header.clone())
                            .size(11)
                            .font(theme::CODE_FONT)
                            .color(accent)
                            .wrapping(text::Wrapping::None),
                        Space::with_width(Length::Fill),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center);

                    if self.diff_is_working_file {
                        if self.diff_is_staged {
                            bar = bar.push(
                                button(text("Unstage Hunk").size(10))
                                    .padding([2, 7])
                                    .style(theme::subtle_btn(p))
                                    .on_press(Message::ApplyHunk {
                                        patch: patch.clone(),
                                        cached: true,
                                        reverse: true,
                                    }),
                            );
                        } else {
                            bar = bar
                                .push(
                                    button(text("Stage Hunk").size(10))
                                        .padding([2, 7])
                                        .style(theme::primary_btn(p))
                                        .on_press(Message::ApplyHunk {
                                            patch: patch.clone(),
                                            cached: true,
                                            reverse: false,
                                        }),
                                )
                                .push(
                                    button(text("Discard Hunk").size(10).color(p.danger_fg))
                                        .padding([2, 7])
                                        .style(theme::subtle_btn(p))
                                        .on_press(Message::RequestConfirm(
                                            PendingConfirm::discard_hunk(
                                                &self.diff_title,
                                                header,
                                                patch.clone(),
                                            ),
                                        )),
                                );
                        }
                    }

                    lines_col = lines_col.push(
                        container(bar)
                            .width(Length::Fill)
                            .height(Length::Fixed(DIFF_SEP_H))
                            .padding([0, 8])
                            .align_y(alignment::Vertical::Center)
                            .style(theme::diff_line_box(p.fill_selected, accent)),
                    );
                }
                UnifiedRow::Code {
                    old_line,
                    new_line,
                    sign,
                    text: code,
                    spans,
                    kind,
                } => {
                    let (bg, sign_color, fg) = match kind {
                        Some(DiffChangeKind::Added) => (p.success_bg, p.success_fg, p.text_primary),
                        Some(DiffChangeKind::Removed) => (p.danger_bg, p.danger_fg, p.text_primary),
                        Some(DiffChangeKind::Modified) => {
                            (p.warning_bg, p.warning_fg, p.text_primary)
                        }
                        None => (Color::TRANSPARENT, p.text_muted, p.text_secondary),
                    };
                    let hl_accent = kind.map(|k| change_colors(k, p).1);
                    let old_s = old_line.map(|n| n.to_string()).unwrap_or_default();
                    let new_s = new_line.map(|n| n.to_string()).unwrap_or_default();

                    let row_inner = row![
                        container(
                            text(old_s)
                                .size(11)
                                .font(theme::CODE_FONT)
                                .color(p.text_muted)
                                .wrapping(text::Wrapping::None),
                        )
                        .width(Length::Fixed(LINE_NUM_W))
                        .height(Length::Fixed(DIFF_LINE_H))
                        .padding([0, 5])
                        .align_x(alignment::Horizontal::Right)
                        .align_y(alignment::Vertical::Center),
                        container(
                            text(new_s)
                                .size(11)
                                .font(theme::CODE_FONT)
                                .color(p.text_muted)
                                .wrapping(text::Wrapping::None),
                        )
                        .width(Length::Fixed(LINE_NUM_W))
                        .height(Length::Fixed(DIFF_LINE_H))
                        .padding([0, 5])
                        .align_x(alignment::Horizontal::Right)
                        .align_y(alignment::Vertical::Center),
                        container(
                            text(sign.to_string())
                                .size(12)
                                .font(theme::CODE_FONT)
                                .color(sign_color)
                                .wrapping(text::Wrapping::None),
                        )
                        .width(Length::Fixed(16.0))
                        .height(Length::Fixed(DIFF_LINE_H))
                        .align_x(alignment::Horizontal::Center)
                        .align_y(alignment::Vertical::Center),
                        container(render_highlighted_code(code, spans, fg, hl_accent))
                            .width(Length::Fill)
                            .height(Length::Fixed(DIFF_LINE_H))
                            .padding([0, 6])
                            .align_y(alignment::Vertical::Center),
                    ]
                    .align_y(Alignment::Center);

                    let char_len = code.chars().count();
                    let sel_cfg = DiffRowSelConfig {
                        side: DiffSelectionSide::Unified,
                        row_idx,
                        code_start_x: gutter_total + 6.0,
                        line_text: code.clone(),
                        sel_range: row_selection_range(
                            self.diff_selection,
                            DiffSelectionSide::Unified,
                            row_idx,
                            char_len,
                        ),
                        is_dragging_same_side: unified_dragging,
                        sel_bg: Color {
                            a: 0.42,
                            ..p.accent_primary
                        },
                        sel_overlay: Color {
                            a: 0.16,
                            ..p.accent_hover
                        },
                    };

                    lines_col = lines_col.push(diff_code_row_box(
                        bg,
                        None,
                        None,
                        None,
                        None,
                        gutter_total,
                        p.border_subtle,
                        sel_cfg,
                        row_inner,
                    ));
                }
                UnifiedRow::Hidden {
                    gap_id,
                    hidden_count,
                    can_expand_top,
                    can_expand_bottom,
                } => {
                    lines_col = lines_col.push(hidden_gap_bar(
                        *gap_id,
                        *hidden_count,
                        *can_expand_top,
                        *can_expand_bottom,
                        p,
                    ));
                }
                UnifiedRow::Separator(hdr) => {
                    lines_col = lines_col.push(
                        container(
                            text(theme::sanitize_code_line(hdr))
                                .size(11)
                                .font(theme::CODE_FONT)
                                .color(p.accent_fg)
                                .wrapping(text::Wrapping::None),
                        )
                        .width(Length::Fill)
                        .height(Length::Fixed(DIFF_SEP_H))
                        .padding([0, 8])
                        .align_y(alignment::Vertical::Center)
                        .style(theme::diff_line_box(p.fill_selected, p.accent_fg)),
                    );
                }
            }
        }

        let min_w = (layout.max_unified_chars as f32 * 7.5 + gutter_total + 40.0)
            .clamp(420.0, 2800.0);
        theme::responsive_hscroll_with_id(
            lines_col,
            min_w,
            self.diff_unified_scroll_id.clone(),
            p,
        )
    }

    /// SmartGit-style Side-by-Side Diff:
    /// - Two independently horizontally-scrollable panes (Old left, New right) with synced vertical scrolling.
    /// - Line numbers on both sides, red/green/amber block fills & borders, and 2px insertion/deletion seams.
    /// - Smooth cubic Bezier connector ribbons in the center + overview strips on left & right edges.
    /// - Compact mode collapses unchanged runs into expandable `N lines hidden · show` bars; Compact OFF shows full file.
    pub(crate) fn view_side_by_side_diff(&self, p: Palette) -> Element<'_, Message> {
        if self.diff_parsed.blocks.is_empty() {
            return self.view_unified_diff(p);
        }

        let layout = build_diff_layout(
            &self.diff_parsed,
            self.diff_compact,
            &self.diff_expanded_gaps,
        );

        let mut left_col = column![].spacing(0);
        for (idx, item) in layout.left_rows.iter().enumerate() {
            left_col = left_col.push(render_side_row(
                item,
                DiffSelectionSide::Left,
                idx,
                self.diff_selection,
                p,
            ));
        }

        let mut right_col = column![].spacing(0);
        for (idx, item) in layout.right_rows.iter().enumerate() {
            right_col = right_col.push(render_side_row(
                item,
                DiffSelectionSide::Right,
                idx,
                self.diff_selection,
                p,
            ));
        }

        let left_min_w =
            (layout.max_left_chars as f32 * 7.5 + LINE_NUM_W + 30.0).clamp(240.0, 2600.0);
        let right_min_w =
            (layout.max_right_chars as f32 * 7.5 + LINE_NUM_W + 30.0).clamp(240.0, 2600.0);
        // Share the wider canvas width so both horizontal scrollbars have identical range & thumb size
        let shared_min_w = left_min_w.max(right_min_w);

        let left_overview: Element<'_, Message> = Canvas::new(DiffOverviewBar {
            spans: layout.connectors.clone(),
            is_left: true,
            total_h: layout.total_left_h,
            p,
        })
        .width(Length::Fixed(OVERVIEW_BAR_W))
        .height(Length::Fill)
        .into();

        let right_overview: Element<'_, Message> = Canvas::new(DiffOverviewBar {
            spans: layout.connectors.clone(),
            is_left: false,
            total_h: layout.total_right_h,
            p,
        })
        .width(Length::Fixed(OVERVIEW_BAR_W))
        .height(Length::Fill)
        .into();

        let left_pane = container(theme::responsive_hscroll_synced(
            left_col,
            shared_min_w,
            self.diff_left_scroll_id.clone(),
            Message::DiffLeftScrolled,
            p,
        ))
        .width(Length::FillPortion(1))
        .height(Length::Fill);

        let connector: Element<'_, Message> = Canvas::new(DiffConnectorCanvas {
            spans: layout.connectors,
            left_scroll_y: self.diff_left_scroll_y,
            right_scroll_y: self.diff_right_scroll_y,
            p,
        })
        .width(Length::Fixed(CONNECTOR_W))
        .height(Length::Fill)
        .into();

        let right_pane = container(theme::responsive_hscroll_synced(
            right_col,
            shared_min_w,
            self.diff_right_scroll_id.clone(),
            Message::DiffRightScrolled,
            p,
        ))
        .width(Length::FillPortion(1))
        .height(Length::Fill);

        row![left_overview, left_pane, connector, right_pane, right_overview]
            .spacing(0)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub(crate) fn view_file_preview(&self, p: Palette) -> Element<'_, Message> {
        if self.preview_state.uses_selectable_editor(self.preview_raw_mode) {
            let mut col = column![].spacing(6).height(Length::Fill);
            match &self.preview_state {
                PreviewState::Json {
                    valid,
                    error: Some(err),
                    ..
                } if !valid => {
                    col = col.push(
                        container(text(format!("Invalid JSON: {err}")).size(11).color(p.danger_fg))
                            .padding([4, 8])
                            .width(Length::Fill)
                            .style(theme::pill_box(p.danger_bg, p.danger_fg, p.danger_fg)),
                    );
                }
                PreviewState::Json { valid: true, .. } => {
                    col = col.push(
                        text("Valid JSON — formatted (select text & Ctrl+C to copy)")
                            .size(10.5)
                            .color(p.success_fg),
                    );
                }
                PreviewState::Config { kind, content } => {
                    col = col.push(
                        text(format!(
                            "{kind} Preview  •  {} lines  (select & Ctrl+C to copy)",
                            content.lines().count()
                        ))
                        .size(10.5)
                        .color(p.text_muted),
                    );
                }
                PreviewState::Code { lang, content } => {
                    col = col.push(
                        text(format!(
                            "{lang}  •  {} lines  (select & Ctrl+C to copy)",
                            content.lines().count()
                        ))
                        .size(10.5)
                        .color(p.text_muted),
                    );
                }
                _ => {}
            }

            let hl_settings = PreviewHighlightSettings {
                kind: self.preview_state.highlight_kind(),
                theme_mode: self.theme_mode,
            };
            let editor = text_editor(&self.preview_editor)
                .on_action(Message::PreviewEditorAction)
                .highlight_with::<PreviewHighlighter>(hl_settings, |color, _theme| {
                    adv_text::highlighter::Format {
                        color: Some(*color),
                        font: None,
                    }
                })
                .font(theme::CODE_FONT)
                .size(12)
                .height(Length::Fill)
                .style(theme::dark_editor(p));

            return col.push(editor).into();
        }

        let content = self
            .preview_state
            .view(p, self.theme_mode, Message::PreviewLinkClicked);
        slim_scroll_h(content, 380.0, p)
    }

    pub(crate) fn view_file_editor(&self, p: Palette) -> Element<'_, Message> {
        if self.open_doc_id.is_none() {
            return container(
                text("Select any file from 'All Files' or 'Changed' to edit it directly.")
                    .size(12)
                    .color(p.text_muted),
            )
            .padding(10)
            .into();
        }
        text_editor(&self.editor_content)
            .on_action(Message::EditorAction)
            .font(theme::CODE_FONT)
            .size(12)
            .height(Length::Fill)
            .style(theme::dark_editor(p))
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitfleet_git::parse_unified_diff;

    fn sample_full_file_diff() -> ParsedDiff {
        let mut raw = String::from(
            "diff --git a/main.go b/main.go\n--- a/main.go\n+++ b/main.go\n@@ -1,22 +1,22 @@\n",
        );
        for i in 1..=10 {
            raw.push_str(&format!(" ctx_top_{i}\n"));
        }
        raw.push_str("-removed_a\n-removed_b\n");
        for i in 1..=4 {
            raw.push_str(&format!(" mid_{i}\n"));
        }
        raw.push_str("+added_a\n+added_b\n");
        for i in 1..=6 {
            raw.push_str(&format!(" ctx_bot_{i}\n"));
        }
        parse_unified_diff(&raw)
    }

    #[test]
    fn compact_hides_unchanged_lines_and_expands_incrementally_or_full() {
        let parsed = sample_full_file_diff();
        let mut expanded = HashMap::new();

        // Compact ON: top has 10 lines before first change -> keep 3 context, hide 7
        let compact = build_diff_layout(&parsed, true, &expanded);
        assert!(matches!(
            compact.left_rows[0],
            SideRow::Hidden {
                gap_id: 0,
                hidden_count: 7,
                ..
            }
        ));
        // Compact ON: bottom has 6 lines after last change -> keep 3 context, hide 3
        assert!(matches!(
            compact.left_rows.last(),
            Some(SideRow::Hidden {
                hidden_count: 3,
                ..
            })
        ));
        assert_eq!(compact.connectors.len(), 2);
        assert_eq!(compact.connectors[0].kind, DiffChangeKind::Removed);
        assert_eq!(compact.connectors[1].kind, DiffChangeKind::Added);

        // Expand top gap completely -> 0 hidden rows at top
        expanded.insert(
            0,
            DiffGapExpansion {
                show_all: true,
                ..Default::default()
            },
        );
        let after_expand = build_diff_layout(&parsed, true, &expanded);
        assert!(matches!(
            after_expand.left_rows[0],
            SideRow::Code {
                line_no: Some(1),
                ..
            }
        ));

        // Compact OFF -> full file, ZERO hidden bars anywhere!
        let full = build_diff_layout(&parsed, false, &HashMap::new());
        assert!(
            !full
                .left_rows
                .iter()
                .any(|r| matches!(r, SideRow::Hidden { .. }))
        );
        assert_eq!(full.left_rows.len(), 22); // 10 + 2 removed + 4 + 6
        assert_eq!(full.right_rows.len(), 22); // 10 + 4 + 2 added + 6
    }

    #[test]
    fn piecewise_scroll_mapping_syncs_across_add_and_remove_spans() {
        let parsed = sample_full_file_diff();
        let layout = build_diff_layout(&parsed, false, &HashMap::new());

        // Line 10 is before any change -> 1:1 mapping
        let y_before = 5.0 * DIFF_LINE_H;
        assert!((map_scroll_y(y_before, &layout.anchors, true) - y_before).abs() < 0.01);

        // After the 2-line pure removal on Left (lines 11-12), Left is at 12*H while Right is at 10*H
        let y_after_remove_left = 12.0 * DIFF_LINE_H;
        let expected_right = 10.0 * DIFF_LINE_H;
        assert!(
            (map_scroll_y(y_after_remove_left, &layout.anchors, true) - expected_right).abs()
                < 0.01
        );

        // Reverse mapping from Right back to Left after both 2-line remove & 2-line add aligns at bottom
        let y_bot = 20.0 * DIFF_LINE_H;
        assert!((map_scroll_y(y_bot, &layout.anchors, false) - y_bot).abs() < 0.01);
    }

    #[test]
    fn inline_diff_highlights_exact_changed_tokens_in_modified_lines() {
        let old_line =
            "profilepb.RegisterProfileServiceServer(grpcServer, composer.ComposeProfileGRPCService(db, *env, rabbitMQManager))";
        let new_line =
            "profilepb.RegisterProfileServiceServer(grpcServer, composer.ComposeProfileGRPCService(*env, profileUsecase, emojiTransactionUsecase, rabbitMQManager))";
        let (old_spans, new_spans) = compute_inline_diff(old_line, new_line);

        let old_hl: String = old_spans
            .iter()
            .filter(|s| s.highlighted)
            .map(|s| s.text.as_str())
            .collect();
        let new_hl: String = new_spans
            .iter()
            .filter(|s| s.highlighted)
            .map(|s| s.text.as_str())
            .collect();

        // Exact removed arg `db, ` on old side; exact added args on new side; unchanged prefix/suffix stay unhighlighted
        assert!(old_hl.contains("db"));
        assert!(!old_hl.contains("RegisterProfileServiceServer"));
        assert!(!old_hl.contains("rabbitMQManager"));
        assert!(new_hl.contains("profileUsecase"));
        assert!(new_hl.contains("emojiTransactionUsecase"));
        assert!(!new_hl.contains("RegisterProfileServiceServer"));
        assert!(!new_hl.contains("rabbitMQManager"));

        // Reassembled text must equal original lines verbatim
        let old_rebuilt: String = old_spans.iter().map(|s| s.text.as_str()).collect();
        let new_rebuilt: String = new_spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(old_rebuilt, old_line);
        assert_eq!(new_rebuilt, new_line);
    }

    #[test]
    fn extracts_clean_single_and_multiline_diff_selection_without_line_numbers() {
        let parsed = sample_full_file_diff();
        let layout = build_diff_layout(&parsed, false, &HashMap::new());

        // Single-line partial selection on left row 10 ("removed_a")
        let sel_single = DiffTextSelection {
            side: DiffSelectionSide::Left,
            anchor: crate::app::DiffTextPos { row: 10, col: 0 },
            focus: crate::app::DiffTextPos { row: 10, col: 7 },
            dragging: false,
        };
        assert_eq!(
            extract_selected_diff_text(&parsed, &layout, sel_single),
            "removed"
        );

        // Reverse multi-line drag across left rows 11..=9 ("ctx_top_10", "removed_a", "removed_b")
        let sel_multi = DiffTextSelection {
            side: DiffSelectionSide::Left,
            anchor: crate::app::DiffTextPos { row: 11, col: 9 },
            focus: crate::app::DiffTextPos { row: 9, col: 4 },
            dragging: false,
        };
        assert_eq!(
            extract_selected_diff_text(&parsed, &layout, sel_multi),
            "top_10\nremoved_a\nremoved_b"
        );

        // Word bounds helper for double-click word selection
        let line = "composer.ComposeProfileGRPCService(*env, db)";
        let (ws, we) = word_bounds_at(line, 12);
        assert_eq!(&line[ws..we], "ComposeProfileGRPCService");
    }
}
