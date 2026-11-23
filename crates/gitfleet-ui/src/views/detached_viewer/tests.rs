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
    fn studio_tab_hydrates_editor_and_preview_lazily() {
        let dir = std::env::temp_dir().join(format!(
            "gitfleet-studio-lazy-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join(".env");
        std::fs::write(&file, "API_KEY=demo\nPORT=8080\n").unwrap();

        let docs = gitfleet_docs::DocsService::new();
        let cap = docs.open_document(&file).unwrap();

        let mut tab = StudioTab::new_empty(1, ".env".into());
        tab.mode = DetachedViewMode::Edit;
        assert!(tab.apply_loaded_doc(&file, Some(Path::new(&dir)), Some(cap)));
        assert!(!tab.editor_hydrated);
        assert!(!tab.preview_hydrated);

        // Hydrating Edit mode shapes editor only — leaves Preview untouched
        tab.hydrate_active_mode();
        assert!(tab.editor_hydrated);
        assert!(!tab.preview_hydrated);

        // Switching to Preview hydrates preview on demand
        tab.mode = DetachedViewMode::Preview;
        tab.ensure_preview_hydrated();
        assert!(tab.preview_hydrated);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn large_cargo_lock_opens_virtualized_and_splices_edit_window_cleanly() {
        let dir = std::env::temp_dir().join(format!(
            "gitfleet-studio-cargolock-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let lock_file = dir.join("Cargo.lock");

        let mut raw = String::with_capacity(190_000);
        for pkg in 0..1_000 {
            raw.push_str("[[package]]\n");
            raw.push_str(&format!("name = \"crate-{pkg}\"\n"));
            raw.push_str("version = \"0.1.0\"\n");
            raw.push_str("source = \"registry+https://github.com/rust-lang/crates.io-index\"\n");
            raw.push_str("checksum = \"abcdef0123456789abcdef0123456789\"\n\n");
        }
        let orig_lines = raw.lines().count();
        assert!(orig_lines >= 6_000);
        std::fs::write(&lock_file, &raw).unwrap();

        let docs = gitfleet_docs::DocsService::new();
        let cap = docs.open_document(&lock_file).unwrap();
        let prebuilt = crate::preview::PreviewState::from_file_prewarmed(
            &lock_file,
            Some(&cap.content),
            crate::theme::ThemeMode::Dark,
        );

        let mut tab = StudioTab::new_empty(1, "Cargo.lock".into());
        tab.mode = DetachedViewMode::Edit;
        let t0 = std::time::Instant::now();
        assert!(tab.apply_loaded_doc_with_preview(
            &lock_file,
            Some(Path::new(&dir)),
            Some(cap),
            Some(prebuilt),
        ));
        tab.hydrate_active_mode();

        // Must enter fast virtualized mode without feeding 6,000+ lines into `cosmic_text::Editor`
        assert!(tab.is_large_file);
        assert!(tab.is_virtualized_code_mode());
        assert_eq!(tab.total_line_count(), orig_lines);
        assert!(
            t0.elapsed() < std::time::Duration::from_millis(50),
            "hydrating 6,000-line Cargo.lock should be O(1), took {:?}",
            t0.elapsed()
        );

        // Open a 300-line edit window around line 3,001, insert a comment, commit slice, and verify full file
        tab.open_edit_window_around(3_000);
        assert!(!tab.is_virtualized_code_mode());
        assert!(tab.editor_content.line_count() <= 305);
        tab.editor_content
            .perform(text_editor::Action::Edit(text_editor::Edit::Insert('#')));
        tab.commit_edit_window(crate::theme::ThemeMode::Dark);
        assert!(tab.is_virtualized_code_mode());
        assert!(tab.dirty);
        let merged = tab.active_full_text().unwrap();
        assert_eq!(merged.lines().count(), orig_lines);
        assert!(merged.lines().nth(3_000).unwrap().starts_with('#'));

        let _ = std::fs::remove_dir_all(&dir);
    }
