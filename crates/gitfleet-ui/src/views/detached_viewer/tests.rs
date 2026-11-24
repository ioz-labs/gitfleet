use super::io::{compare_local_files_payload, editor_buffer_text, format_editor_text_for_save};
use super::sidebar_explorer::ExplorerDirNode;
use super::state::{DetachedViewMode, StudioTab};
use gitfleet_git::IgnoredDirListing;
use gitfleet_search::{SearchOptions, replace_all_in_text};
use iced::widget::text_editor;
use std::collections::HashMap;
use std::path::Path;

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

    #[test]
    fn studio_explorer_tree_merges_and_dims_gitignored_files_and_lazy_dirs() {
        let files = vec!["Cargo.toml".into(), "src/main.rs".into()];
        let ignored_files = vec![".env".into(), "src/.env.local".into(), "tmp_ign/secret.txt".into()];
        let ignored_dirs = vec![".vscode".into(), "target".into()];
        let mut cache = HashMap::new();
        cache.insert(
            ".vscode".to_string(),
            IgnoredDirListing {
                files: vec![".vscode/settings.json".into()],
                dirs: Vec::new(),
                truncated: false,
            },
        );

        let tree = ExplorerDirNode::build(&files, &ignored_files, &ignored_dirs, &cache, true);
        assert_eq!(tree.files.len(), 2);
        assert_eq!(tree.files[0], (".env".into(), ".env".into(), true));
        assert_eq!(tree.files[1], ("Cargo.toml".into(), "Cargo.toml".into(), false));

        // Tracked dir containing an ignored file stays non-ignored
        assert!(!tree.dirs["src"].is_ignored_only());
        assert_eq!(tree.dirs["src"].files.len(), 2);

        // Ignored-only dirs are dimmed like VS Code
        assert!(tree.dirs["tmp_ign"].is_ignored_only());
        assert!(tree.dirs["target"].is_ignored_only());
        assert!(tree.dirs["target"].lazy_ignored_dir);
        assert!(!tree.dirs["target"].lazy_loaded);

        // Loaded lazy ignored dir populates its files cleanly
        let vscode = &tree.dirs[".vscode"];
        assert!(vscode.is_ignored_only());
        assert!(vscode.lazy_loaded);
        assert_eq!(
            vscode.files,
            vec![("settings.json".into(), ".vscode/settings.json".into(), true)]
        );

        // Hiding ignored items removes them cleanly
        let hidden = ExplorerDirNode::build(&files, &ignored_files, &ignored_dirs, &cache, false);
        assert_eq!(hidden.files, vec![("Cargo.toml".into(), "Cargo.toml".into(), false)]);
        assert!(!hidden.dirs.contains_key(".vscode"));
        assert!(!hidden.dirs.contains_key("target"));
        assert!(!hidden.dirs.contains_key("tmp_ign"));
    }

    #[test]
    fn studio_tab_disables_preview_for_code_and_hydrates_markdown_lazily() {
        let dir = std::env::temp_dir().join(format!(
            "gitfleet-studio-lazy-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let rs_file = dir.join("main.rs");
        let md_file = dir.join("README.md");
        std::fs::write(&rs_file, "fn main() {\n    println!(\"hi\");\n}\n").unwrap();
        std::fs::write(&md_file, "# Hello\n\nSome markdown.\n").unwrap();

        let docs = gitfleet_docs::DocsService::new();

        // Code files never offer Preview and always stay directly editable in Code mode
        let rs_cap = docs.open_document(&rs_file).unwrap();
        let mut rs_tab = StudioTab::new_empty(1, "main.rs".into());
        rs_tab.mode = DetachedViewMode::Preview;
        assert!(rs_tab.apply_loaded_doc(&rs_file, Some(Path::new(&dir)), Some(rs_cap)));
        assert!(!rs_tab.supports_preview());
        rs_tab.hydrate_active_mode();
        assert_eq!(rs_tab.mode, DetachedViewMode::Edit);
        assert!(rs_tab.editor_hydrated);
        assert!(!rs_tab.preview_hydrated);

        // Rich previewable files (Markdown, SVG, CSV/TSV) hydrate editor & preview lazily
        let md_cap = docs.open_document(&md_file).unwrap();
        let mut md_tab = StudioTab::new_empty(2, "README.md".into());
        md_tab.mode = DetachedViewMode::Edit;
        assert!(md_tab.apply_loaded_doc(&md_file, Some(Path::new(&dir)), Some(md_cap)));
        assert!(md_tab.supports_preview());
        assert!(!md_tab.editor_hydrated);
        assert!(!md_tab.preview_hydrated);

        md_tab.hydrate_active_mode();
        assert!(md_tab.editor_hydrated);
        assert!(!md_tab.preview_hydrated);

        md_tab.mode = DetachedViewMode::Preview;
        md_tab.ensure_preview_hydrated();
        assert!(md_tab.preview_hydrated);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn large_file_opens_directly_in_full_editable_code_mode() {
        let dir = std::env::temp_dir().join(format!(
            "gitfleet-studio-full-edit-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let code_file = dir.join("large_module.rs");

        let mut raw = String::with_capacity(80_000);
        for i in 0..1_200 {
            raw.push_str(&format!("pub fn func_{i}() -> usize {{ {i} }}\n"));
        }
        let orig_lines = raw.lines().count();
        assert_eq!(orig_lines, 1_200);
        std::fs::write(&code_file, &raw).unwrap();

        let docs = gitfleet_docs::DocsService::new();
        let cap = docs.open_document(&code_file).unwrap();

        let mut tab = StudioTab::new_empty(1, "large_module.rs".into());
        tab.mode = DetachedViewMode::Edit;
        assert!(tab.apply_loaded_doc(&code_file, Some(Path::new(&dir)), Some(cap)));
        assert!(!tab.supports_preview());
        tab.hydrate_active_mode();

        // Full file is loaded directly into editor_content and immediately editable (no slice/preview fallback)
        assert!(tab.editor_hydrated);
        assert_eq!(tab.editor_content.line_count(), orig_lines);
        assert_eq!(tab.active_full_text().unwrap(), raw);

        tab.editor_content
            .perform(text_editor::Action::Edit(text_editor::Edit::Insert('/')));
        tab.recompute_dirty_flag();
        assert!(tab.dirty);
        assert!(tab.active_full_text().unwrap().starts_with('/'));

        let _ = std::fs::remove_dir_all(&dir);
    }
