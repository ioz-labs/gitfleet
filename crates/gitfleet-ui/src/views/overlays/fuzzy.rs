//! Fast Unicode-safe fuzzy subsequence matcher and highlighted `rich_text` renderer for
//! `Command Palette (Ctrl+K)` and `Quick Branch Switcher (Ctrl+B)`.

use crate::theme::{UI_FONT, UI_FONT_SEMIBOLD, type_scale};
use iced::widget::{rich_text, span, text};
use iced::{Color, Element, Font};

/// Computes a fuzzy match score (`higher = better`) and matched char indices into `candidate`.
/// Returns `None` if `query` is not a case-insensitive subsequence of `candidate`.
pub fn fuzzy_match(query: &str, candidate: &str) -> Option<(i32, Vec<usize>)> {
    let q_chars: Vec<char> = query
        .trim()
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if q_chars.is_empty() {
        return Some((0, Vec::new()));
    }

    let cand_chars: Vec<char> = candidate.chars().collect();
    let cand_lower: Vec<char> = cand_chars
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();

    let mut matched = Vec::with_capacity(q_chars.len());
    let mut score: i32 = 0;
    let mut qi = 0usize;
    let mut prev_match_idx: Option<usize> = None;

    for (ci, &cl) in cand_lower.iter().enumerate() {
        if qi < q_chars.len() && cl == q_chars[qi] {
            matched.push(ci);
            let mut bonus = 10;
            if ci == 0 {
                bonus += 24;
            } else {
                let prev_ch = cand_chars[ci - 1];
                if matches!(prev_ch, '/' | ':' | '-' | '_' | ' ' | '.')
                    || (prev_ch.is_lowercase() && cand_chars[ci].is_uppercase())
                {
                    bonus += 18;
                }
            }
            if let Some(prev) = prev_match_idx {
                if prev + 1 == ci {
                    bonus += 22;
                } else {
                    let gap = (ci - prev - 1) as i32;
                    bonus -= gap.min(12);
                }
            }
            score += bonus;
            prev_match_idx = Some(ci);
            qi += 1;
        }
    }

    if qi == q_chars.len() {
        // Slightly prefer shorter, tighter labels on ties
        score -= (cand_chars.len() as i32 / 4).min(15);
        Some((score, matched))
    } else {
        None
    }
}

/// Renders `label` with matched character positions highlighted in `highlight_fg` + `UI_FONT_SEMIBOLD`.
pub fn render_fuzzy_text<'a, M: Clone + 'static>(
    label: &str,
    matched_indices: &[usize],
    base_fg: Color,
    highlight_fg: Color,
) -> Element<'a, M> {
    if matched_indices.is_empty() {
        return text(label.to_string())
            .size(type_scale::BODY)
            .font(UI_FONT)
            .color(base_fg)
            .wrapping(text::Wrapping::None)
            .into();
    }

    let chars: Vec<char> = label.chars().collect();
    let mut spans: Vec<text::Span<'a, M, Font>> = Vec::new();
    let mut buf = String::new();
    let mut cur_hl = false;
    let mut match_cursor = 0usize;

    for (idx, ch) in chars.into_iter().enumerate() {
        while match_cursor < matched_indices.len() && matched_indices[match_cursor] < idx {
            match_cursor += 1;
        }
        let is_hl = match_cursor < matched_indices.len() && matched_indices[match_cursor] == idx;
        if idx > 0 && is_hl != cur_hl && !buf.is_empty() {
            spans.push(
                span(std::mem::take(&mut buf))
                    .size(type_scale::BODY)
                    .font(if cur_hl { UI_FONT_SEMIBOLD } else { UI_FONT })
                    .color(if cur_hl { highlight_fg } else { base_fg }),
            );
        }
        cur_hl = is_hl;
        buf.push(ch);
    }
    if !buf.is_empty() {
        spans.push(
            span(buf)
                .size(type_scale::BODY)
                .font(if cur_hl { UI_FONT_SEMIBOLD } else { UI_FONT })
                .color(if cur_hl { highlight_fg } else { base_fg }),
        );
    }

    rich_text(spans)
        .size(type_scale::BODY)
        .font(UI_FONT)
        .wrapping(text::Wrapping::None)
        .into()
}

#[cfg(test)]
mod tests {
    use super::fuzzy_match;

    #[test]
    fn ranks_prefix_and_word_boundary_matches_above_scattered_subsequences() {
        let (s_prefix, m_prefix) =
            fuzzy_match("gfs", "Git: Fetch Selected").expect("matches word boundaries");
        let (s_scatter, _) =
            fuzzy_match("gfs", "Toggle Fullscreen Settings").expect("matches scattered");
        assert!(
            s_prefix > s_scatter,
            "expected boundary match ({s_prefix}) > scattered ({s_scatter})"
        );
        assert_eq!(m_prefix, vec![0, 5, 11]);
    }

    #[test]
    fn handles_vietnamese_diacritics_and_non_matches_safely() {
        let res = fuzzy_match("dựán", "Repo: Dự Án Backend");
        assert!(res.is_some());
        assert!(fuzzy_match("xyz123", "Git: Pull").is_none());
    }
}
