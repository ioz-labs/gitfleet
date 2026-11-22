use super::inline_diff::compute_inline_diff;
use super::layout::{
    SideRow, build_diff_layout, build_diff_layout_cache, map_scroll_y, visible_row_range,
};
use super::selection::{extract_selected_diff_text, word_bounds_at};
use super::DIFF_LINE_H;
use crate::app::{DiffGapExpansion, DiffSelectionSide, DiffTextPos, DiffTextSelection};
use crate::syntax::CodeLanguage;
use crate::theme::ThemeMode;
use gitfleet_git::{DiffChangeKind, ParsedDiff, parse_unified_diff};
use std::collections::HashMap;

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

    let compact = build_diff_layout(&parsed, true, &expanded);
    assert!(matches!(
        compact.left_rows[0],
        SideRow::Hidden {
            gap_id: 0,
            hidden_count: 7,
            ..
        }
    ));
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

    let full = build_diff_layout(&parsed, false, &HashMap::new());
    assert!(
        !full
            .left_rows
            .iter()
            .any(|r| matches!(r, SideRow::Hidden { .. }))
    );
    assert_eq!(full.left_rows.len(), 22);
    assert_eq!(full.right_rows.len(), 22);
}

#[test]
fn piecewise_scroll_mapping_syncs_across_add_and_remove_spans() {
    let parsed = sample_full_file_diff();
    let layout = build_diff_layout(&parsed, false, &HashMap::new());

    let y_before = 5.0 * DIFF_LINE_H;
    assert!((map_scroll_y(y_before, &layout.anchors, true) - y_before).abs() < 0.01);

    let y_after_remove_left = 12.0 * DIFF_LINE_H;
    let expected_right = 10.0 * DIFF_LINE_H;
    assert!(
        (map_scroll_y(y_after_remove_left, &layout.anchors, true) - expected_right).abs() < 0.01
    );

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

    assert!(old_hl.contains("db"));
    assert!(!old_hl.contains("RegisterProfileServiceServer"));
    assert!(!old_hl.contains("rabbitMQManager"));
    assert!(new_hl.contains("profileUsecase"));
    assert!(new_hl.contains("emojiTransactionUsecase"));
    assert!(!new_hl.contains("RegisterProfileServiceServer"));
    assert!(!new_hl.contains("rabbitMQManager"));

    let old_rebuilt: String = old_spans.iter().map(|s| s.text.as_str()).collect();
    let new_rebuilt: String = new_spans.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(old_rebuilt, old_line);
    assert_eq!(new_rebuilt, new_line);
}

#[test]
fn extracts_clean_single_and_multiline_diff_selection_without_line_numbers() {
    let parsed = sample_full_file_diff();
    let layout = build_diff_layout(&parsed, false, &HashMap::new());

    let sel_single = DiffTextSelection {
        side: DiffSelectionSide::Left,
        anchor: DiffTextPos { row: 10, col: 0 },
        focus: DiffTextPos { row: 10, col: 7 },
        dragging: false,
    };
    assert_eq!(
        extract_selected_diff_text(&parsed, &layout, sel_single),
        "removed"
    );

    let sel_multi = DiffTextSelection {
        side: DiffSelectionSide::Left,
        anchor: DiffTextPos { row: 11, col: 9 },
        focus: DiffTextPos { row: 9, col: 4 },
        dragging: false,
    };
    assert_eq!(
        extract_selected_diff_text(&parsed, &layout, sel_multi),
        "top_10\nremoved_a\nremoved_b"
    );

    let line = "composer.ComposeProfileGRPCService(*env, db)";
    let (ws, we) = word_bounds_at(line, 12);
    assert_eq!(&line[ws..we], "ComposeProfileGRPCService");
}

#[test]
fn virtualized_row_range_preserves_exact_total_height_and_caches_syntax() {
    let parsed = sample_full_file_diff();
    let cache = build_diff_layout_cache(
        &parsed,
        false,
        &HashMap::new(),
        CodeLanguage::Go,
        ThemeMode::Dark,
    );
    assert_eq!(cache.left_y_offsets.len(), cache.layout.left_rows.len() + 1);
    assert_eq!(cache.left_syntax.len(), cache.layout.left_rows.len());
    assert!((cache.left_y_offsets.last().copied().unwrap_or(0.0) - cache.layout.total_left_h).abs() < 0.01);

    let (range, top_sp, bot_sp) = visible_row_range(&cache.left_y_offsets, 210.0, 100.0, 21.0);
    let rendered_h: f32 = cache.layout.left_rows[range.clone()]
        .iter()
        .map(SideRow::height)
        .sum();
    assert!((top_sp + rendered_h + bot_sp - cache.layout.total_left_h).abs() < 0.01);
    assert!(range.start > 0 && range.end < cache.layout.left_rows.len());
}
