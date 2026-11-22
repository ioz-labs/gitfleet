use super::io::{compare_local_files_payload, editor_buffer_text, format_editor_text_for_save};
use super::state::DetachedViewMode;
use gitfleet_search::{SearchOptions, replace_all_in_text};
use iced::widget::text_editor;

    #[test]
    fn compares_arbitrary_local_csv_and_text_files_independently() {
        let dir = std::env::temp_dir().join(format!(
            "gitfleet-detached-test-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);

        let left_csv = dir.join("old.csv");
        let right_csv = dir.join("new.csv");
        std::fs::write(&left_csv, "sku,stock\nA1,10\nA2,20\n").unwrap();
        std::fs::write(&right_csv, "sku,stock\nA1,15\nA2,20\n").unwrap();

        let csv_payload = compare_local_files_payload(
            left_csv.clone(),
            right_csv.clone(),
            DetachedViewMode::SideBySideDiff,
        )
        .expect("csv compare should succeed");

        assert_eq!(csv_payload.preferred_mode, DetachedViewMode::CsvDiff);
        let csv_data = csv_payload.csv_diff.expect("csv diff data");
        assert_eq!(csv_data.stats.modified_rows, 1);
        assert_eq!(csv_data.stats.changed_cells, 1);
        assert!(!csv_payload.parsed_diff.blocks.is_empty());

        let left_rs = dir.join("a.rs");
        let right_rs = dir.join("b.rs");
        std::fs::write(&left_rs, "fn main() {\n    println!(\"old\");\n}\n").unwrap();
        std::fs::write(&right_rs, "fn main() {\n    println!(\"new\");\n}\n").unwrap();

        let text_payload = compare_local_files_payload(
            left_rs.clone(),
            right_rs.clone(),
            DetachedViewMode::SideBySideDiff,
        )
        .expect("text compare should succeed");
        assert_eq!(text_payload.preferred_mode, DetachedViewMode::SideBySideDiff);
        assert_eq!(text_payload.parsed_diff.stats.modified_blocks, 1);
        assert!(text_payload.editable_source.is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn editor_buffer_text_and_save_preserve_lines_and_eof_newline() {
        let c1 = text_editor::Content::with_text("line1\nline2");
        assert_eq!(editor_buffer_text(&c1), "line1\nline2");
        assert_eq!(format_editor_text_for_save(&c1, false), "line1\nline2");
        assert_eq!(format_editor_text_for_save(&c1, true), "line1\nline2\n");

        let (replaced, count) = replace_all_in_text(
            &editor_buffer_text(&c1),
            "line",
            "row",
            SearchOptions::default(),
        )
        .unwrap();
        assert_eq!(count, 2);
        let c2 = text_editor::Content::with_text(&replaced);
        assert_eq!(c2.line_count(), 2);
        assert_eq!(format_editor_text_for_save(&c2, true), "row1\nrow2\n");
    }
