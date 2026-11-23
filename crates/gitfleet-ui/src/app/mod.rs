pub(crate) mod message;
pub(crate) mod subscription;
pub(crate) mod tasks;
pub(crate) mod types;
pub(crate) mod update;

pub(crate) use message::Message;
pub(crate) use types::*;

use crate::{graph, preview};
use crate::syntax::CodeLanguage;
use crate::terminal::TerminalFontCatalog;
use crate::theme::{Palette, ThemeMode, ThemePreset};
use crate::views::detached_viewer::DetachedViewerState;
use crate::views::diff_viewer::{DiffLayoutCache, build_diff_layout_cache};
use crate::views::overlays::blueprint_modal::{
    BlueprintExportModalState, BlueprintImportModalState,
};
use crate::views::overlays::file_ops_modal::{AddGitignoreModalState, RenameEntryModalState};
use gitfleet_app::{AppService, DetectedTool};
use gitfleet_core::{DocumentId, RepoId, SessionId};
use gitfleet_docs::{FileEncoding, LineEnding};
use gitfleet_git::{
    AmendTarget, BranchInfo, CommitSummary, FileChange, ForecastResult, GitFlowKind, GitFlowState,
    OrphanGitlink, ParsedConflictFile, ParsedDiff, ReflogEntry, ReleaseTrigger, RepoStatusSummary,
    StashEntry, SubmoduleInfo, TagInfo, WorktreeInfo, has_conflict_markers,
};
use gitfleet_search::SearchMatch;
use gitfleet_security::{
    IgnoreLayer, IgnoreSnapshot, OversizedFile, SecretFileSpec, SecretFinding, VaultSummary,
};
use gitfleet_workspace::{PrefixRelocation, RepoEntry};
use iced::widget::{scrollable, text_editor, text_input};
use iced::{Point, Size, Task, Theme, window};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;
pub(crate) struct GitFleetApp {
    pub(crate) service: Arc<AppService>,
    pub(crate) windows: BTreeMap<window::Id, WindowKind>,
    pub(crate) detached_viewers: BTreeMap<window::Id, DetachedViewerState>,
    pub(crate) pending_detached_viewers: VecDeque<DetachedViewerState>,
    pub(crate) theme_preset: ThemePreset,
    pub(crate) theme_mode: ThemeMode,
    pub(crate) host_layout: HostLayoutMode,
    pub(crate) pre_zen_layout: HostLayoutMode,

    // Top-Left: Repositories
    pub(crate) filter_query: String,
    pub(crate) repo_filter: RepoFilter,
    pub(crate) selected_repo: Option<RepoId>,
    pub(crate) active_submodule: Option<(RepoId, String)>,
    pub(crate) repo_submodules: HashMap<RepoId, Vec<SubmoduleInfo>>,
    pub(crate) collapsed_repo_submodules: HashSet<RepoId>,
    pub(crate) bulk_selected: HashSet<RepoId>,
    pub(crate) statuses: HashMap<RepoId, RepoStatusSummary>,

    // Bottom-Left: Branches / Stashes / Tags / Submodules / Worktrees
    pub(crate) bottom_left_tab: BottomLeftTab,
    pub(crate) branches: Vec<BranchInfo>,
    pub(crate) selected_branch: Option<String>,
    pub(crate) last_branch_click: Option<(String, Instant)>,
    pub(crate) last_file_click: Option<(String, Instant)>,
    pub(crate) graph_checked_branches: HashSet<String>,
    pub(crate) toggled_branch_nodes: HashSet<String>,
    pub(crate) stashes: Vec<StashEntry>,
    pub(crate) tags: Vec<TagInfo>,
    pub(crate) submodules: Vec<SubmoduleInfo>,
    pub(crate) worktrees: Vec<WorktreeInfo>,
    pub(crate) orphan_gitlinks: Vec<OrphanGitlink>,
    pub(crate) release_triggers: Vec<ReleaseTrigger>,
    pub(crate) new_tag_name: String,
    pub(crate) new_submodule_url: String,
    pub(crate) new_submodule_path: String,
    pub(crate) new_worktree_branch: String,

    // Center-Top: Graph / Reflog
    pub(crate) graph_tab: GraphTab,
    pub(crate) graph_all_branches: bool,
    pub(crate) graph_topo_order: bool,
    pub(crate) commit_filter: String,
    pub(crate) graph_selection: GraphSelection,
    pub(crate) commits: Vec<CommitSummary>,
    pub(crate) graph_rows: Vec<graph::GraphRow>,
    pub(crate) graph_max_lanes: usize,
    pub(crate) reflog: Vec<ReflogEntry>,

    // Right-Top: Commit & Files
    pub(crate) commit_summary: String,
    pub(crate) commit_summary_id: text_input::Id,
    pub(crate) commit_editor: text_editor::Content,
    pub(crate) amend_commit: bool,
    pub(crate) amend_target: Option<AmendTarget>,

    pub(crate) files_mode: FilesViewMode,
    pub(crate) show_ignored_files: bool,
    pub(crate) file_filter: String,
    pub(crate) selected_dir_filter: Option<String>,
    pub(crate) selected_all_dir: String,
    pub(crate) expanded_all_dirs: HashSet<String>,
    pub(crate) commit_files: Vec<FileChange>,
    pub(crate) all_repo_files: Vec<String>,
    pub(crate) ignored_repo_files: Vec<String>,
    pub(crate) ignored_repo_dirs: Vec<String>,
    pub(crate) ignored_dir_children: HashMap<String, gitfleet_git::IgnoredDirListing>,
    pub(crate) ignored_entries_loaded: bool,
    pub(crate) ignored_entries_stale: bool,

    // Bottom: Diff / Preview / Edit Viewer
    pub(crate) bottom_mode: BottomViewMode,
    pub(crate) last_text_diff_mode: BottomViewMode,
    pub(crate) csv_prefer_table: bool,
    pub(crate) diff_request_id: u64,
    pub(crate) diff_image: Option<crate::views::image_diff::ImageDiffData>,
    pub(crate) image_diff_mode: crate::views::image_diff::ImageDiffMode,
    pub(crate) image_diff_zoom: crate::views::image_diff::ImageDiffZoom,
    pub(crate) image_diff_swapped: bool,
    pub(crate) image_diff_swipe: f32,
    pub(crate) image_diff_onion: f32,
    pub(crate) image_diff_blink_after: bool,
    pub(crate) image_diff_blink_paused: bool,
    pub(crate) diff_csv: Option<crate::views::csv_diff::CsvDiffData>,
    pub(crate) csv_diff_changes_only: bool,
    pub(crate) csv_diff_ignore_format: bool,
    pub(crate) csv_diff_filter: String,
    pub(crate) active_file_path: Option<String>,
    pub(crate) selected_changed_file: Option<FileChange>,
    pub(crate) selected_changed_files: HashSet<(String, bool)>,
    pub(crate) file_selection_anchor: Option<FileChange>,
    pub(crate) focus_pane: ActiveFocusPane,
    pub(crate) keyboard_modifiers: iced::keyboard::Modifiers,
    pub(crate) diff_title: String,
    pub(crate) diff_parsed_path: Option<String>,
    pub(crate) diff_parsed: ParsedDiff,
    pub(crate) diff_cache: Arc<DiffLayoutCache>,
    pub(crate) diff_is_working_file: bool,
    pub(crate) diff_is_staged: bool,
    pub(crate) diff_compact: bool,
    pub(crate) diff_expanded_gaps: HashMap<usize, DiffGapExpansion>,
    pub(crate) diff_left_scroll_id: scrollable::Id,
    pub(crate) diff_unified_scroll_id: scrollable::Id,
    pub(crate) diff_left_scroll_y: f32,
    pub(crate) diff_unified_scroll_y: f32,
    pub(crate) diff_left_scroll_x: f32,
    pub(crate) diff_left_viewport_h: f32,
    pub(crate) diff_unified_viewport_h: f32,
    pub(crate) diff_active_change: usize,
    pub(crate) diff_selection: Option<DiffTextSelection>,
    pub(crate) diff_last_click: Option<(Instant, DiffSelectionSide, usize, usize, u8)>,

    pub(crate) doc_request_id: u64,
    pub(crate) open_doc_raw_text: Option<String>,
    pub(crate) pending_jump_line: Option<usize>,
    pub(crate) preview_hydrated: bool,
    pub(crate) editor_hydrated: bool,
    pub(crate) preview_state: preview::PreviewState,
    pub(crate) preview_editor: text_editor::Content,
    pub(crate) preview_scroll: crate::views::code_editor::CodeScrollState,
    pub(crate) preview_raw_mode: bool,
    pub(crate) open_doc_id: Option<DocumentId>,
    pub(crate) open_doc_version: Option<String>,
    pub(crate) open_doc_path: Option<PathBuf>,
    pub(crate) open_doc_line_ending: LineEnding,
    pub(crate) open_doc_encoding: FileEncoding,
    pub(crate) open_doc_language: CodeLanguage,
    pub(crate) editor_content: text_editor::Content,
    pub(crate) editor_scroll: crate::views::code_editor::CodeScrollState,
    pub(crate) editor_dirty: bool,
    pub(crate) show_eol_modal: bool,
    pub(crate) show_encoding_modal: bool,
    pub(crate) show_syntax_modal: bool,
    pub(crate) syntax_filter: String,
    pub(crate) syntax_filter_id: text_input::Id,
    pub(crate) doc_format_menu_pos: Point,
    pub(crate) encoding_modal_mode: EncodingModalMode,

    // Overlays
    pub(crate) show_palette: bool,
    pub(crate) palette_input_id: text_input::Id,
    pub(crate) palette_query: String,
    pub(crate) palette_selected_idx: usize,
    pub(crate) show_branch_popover: bool,
    pub(crate) branch_filter_id: text_input::Id,
    pub(crate) branch_filter_query: String,
    pub(crate) branch_popover_selected_idx: usize,
    pub(crate) branch_create_base: Option<String>,
    pub(crate) branch_create_checkout: bool,
    pub(crate) workspace_modal_tab: WorkspaceModalTab,
    pub(crate) blueprint_export_modal: Option<BlueprintExportModalState>,
    pub(crate) blueprint_import_modal: Option<BlueprintImportModalState>,

    // Merge/Rebase Forecast Modal
    pub(crate) show_merge_modal: bool,
    pub(crate) merge_branch_filter: String,
    pub(crate) merge_target_branch: Option<String>,
    pub(crate) merge_forecast: Option<ForecastResult>,

    // Rename Branch Modal
    pub(crate) rename_branch_modal: Option<RenameBranchModalState>,
    pub(crate) rename_branch_input_id: text_input::Id,

    // Rename Workspace Modal
    pub(crate) rename_workspace_modal: Option<RenameWorkspaceModalState>,
    pub(crate) rename_workspace_input_id: text_input::Id,

    // Rename Group Modal
    pub(crate) rename_group_modal: Option<RenameGroupModalState>,
    pub(crate) rename_group_input_id: text_input::Id,

    // Rename File/Folder & Add to .gitignore Modals
    pub(crate) rename_entry_modal: Option<RenameEntryModalState>,
    pub(crate) rename_entry_input_id: text_input::Id,
    pub(crate) add_gitignore_modal: Option<AddGitignoreModalState>,
    pub(crate) add_gitignore_input_id: text_input::Id,

    // Commit Popup Modal (right-click Repo -> Commit...)
    pub(crate) commit_modal: Option<CommitModalState>,
    pub(crate) commit_modal_editor: text_editor::Content,

    // Scan Folder & Auto-Group Preview Modal
    pub(crate) scan_modal: Option<ScanModalState>,
    pub(crate) scan_request_id: u64,

    // Interactive Conflict Solver
    pub(crate) conflict_file_path: Option<String>,
    pub(crate) conflict_parsed: ParsedConflictFile,
    pub(crate) conflict_active_hunk: Option<usize>,
    pub(crate) conflict_editing_hunk: Option<usize>,
    pub(crate) conflict_hunk_editor: text_editor::Content,
    pub(crate) conflict_expanded_segments: HashSet<usize>,
    pub(crate) conflict_scroll_id: scrollable::Id,

    // Git Flow Modal
    pub(crate) show_git_flow_modal: bool,
    pub(crate) git_flow_editing_config: bool,
    pub(crate) git_flow_state: GitFlowState,
    pub(crate) git_flow_main_input: String,
    pub(crate) git_flow_develop_input: String,
    pub(crate) git_flow_feature_prefix: String,
    pub(crate) git_flow_release_prefix: String,
    pub(crate) git_flow_hotfix_prefix: String,
    pub(crate) git_flow_bugfix_prefix: String,
    pub(crate) git_flow_name_input: String,
    pub(crate) git_flow_kind: GitFlowKind,

    // Command log, Drawer & status
    pub(crate) drawer_tab: Option<DrawerTab>,
    pub(crate) bulk_cancel: Option<Arc<AtomicBool>>,

    // Milestone 3: Workspace Manager, Groups, Clone & Add Repo Modals
    pub(crate) show_workspace_modal: bool,
    pub(crate) show_inline_group_input: bool,
    pub(crate) new_group_parent: Option<String>,
    pub(crate) new_group_input: String,
    pub(crate) repo_drag_candidate: Option<(RepoId, String, Point)>,
    pub(crate) dragging_repo: Option<(RepoId, String)>,
    pub(crate) drag_hover_group: Option<(String, Option<String>)>,
    pub(crate) new_workspace_name: String,
    pub(crate) last_repo_parent_dir: Option<PathBuf>,
    pub(crate) show_clone_modal: bool,
    pub(crate) clone_modal: CloneModalState,
    pub(crate) clone_url_input_id: text_input::Id,
    pub(crate) active_clone: Option<ActiveCloneState>,
    pub(crate) add_repo_modal: Option<AddRepoModalState>,
    pub(crate) add_repo_path_input_id: text_input::Id,
    pub(crate) add_repo_name_input_id: text_input::Id,
    pub(crate) pending_relocation: Option<PrefixRelocation>,

    // Milestone 4: Pre-Commit Hygiene, Secrets Vault, 3-Layer Ignore
    pub(crate) staged_secrets: Vec<SecretFinding>,
    pub(crate) oversized_files: Vec<OversizedFile>,
    pub(crate) vault_summary: VaultSummary,
    pub(crate) secret_candidates: Vec<SecretFileSpec>,
    pub(crate) show_secret_picker: bool,
    pub(crate) secret_add_path_input: String,
    pub(crate) show_key_manager: bool,
    pub(crate) new_key_name_input: String,
    pub(crate) show_import_key_bar: bool,
    pub(crate) import_key_name_input: String,
    pub(crate) import_key_input: String,
    pub(crate) renaming_key: Option<(String, String)>,
    pub(crate) show_create_profile_bar: bool,
    pub(crate) new_profile_name_input: String,
    pub(crate) clone_from_active_profile: bool,
    pub(crate) renaming_profile: Option<(String, String)>,
    pub(crate) revealed_secrets: HashMap<(String, String), String>,
    pub(crate) secret_file_input: String,
    pub(crate) secret_key_filter: String,
    pub(crate) secret_key_input: String,
    pub(crate) secret_val_input: String,
    pub(crate) ignore_layer: IgnoreLayer,
    pub(crate) ignore_snapshot: IgnoreSnapshot,
    pub(crate) ignore_editor: text_editor::Content,
    pub(crate) ignore_test_path: String,
    pub(crate) ignore_test_result: Option<Option<String>>,

    // Milestone 5: Fleet Search & Multi-Tab Split PTY Terminal
    pub(crate) search_query: String,
    pub(crate) search_case_sensitive: bool,
    pub(crate) search_whole_word: bool,
    pub(crate) search_regex: bool,
    pub(crate) search_show_advanced: bool,
    pub(crate) search_replace_text: String,
    pub(crate) search_include_glob: String,
    pub(crate) search_exclude_glob: String,
    pub(crate) collapsed_search_repos: HashSet<RepoId>,
    pub(crate) search_running: bool,
    pub(crate) search_results: Vec<SearchMatch>,
    pub(crate) command_log_filter: CommandLogFilter,
    pub(crate) command_log_query: String,
    pub(crate) selected_command_log_id: Option<String>,
    pub(crate) terminal_tabs: Vec<TerminalTab>,
    pub(crate) active_terminal_tab: usize,
    pub(crate) terminal_focused: bool,
    pub(crate) terminal_expanded: bool,
    pub(crate) terminal_search_open: bool,
    pub(crate) terminal_search_query: String,
    pub(crate) dock_position: DockPosition,
    pub(crate) drawer_height: f32,
    pub(crate) drawer_width: f32,
    pub(crate) active_resize: Option<ResizeTarget>,
    pub(crate) sidebar_width: f32,
    pub(crate) left_split_ratio: f32,
    pub(crate) right_split_ratio: f32,
    pub(crate) top_right_split_ratio: f32,
    pub(crate) commit_files_split_ratio: f32,
    pub(crate) files_staged_split_ratio: f32,
    pub(crate) show_terminal_sidebar: bool,
    pub(crate) show_terminal_menu: bool,
    pub(crate) terminal_menu_submenu: TerminalMenuSubmenu,
    pub(crate) terminal_fonts: TerminalFontCatalog,

    // Milestone 6: Settings & Diagnostics Modal
    pub(crate) show_settings_modal: bool,
    pub(crate) settings_tab: SettingsTab,
    pub(crate) settings_font_filter: String,
    pub(crate) git_version: String,
    pub(crate) detected_tools: Vec<DetectedTool>,

    // Software Updater
    pub(crate) show_update_modal: bool,
    pub(crate) update_status: UpdateUiStatus,
    pub(crate) update_release_markdown: Vec<iced::widget::markdown::Item>,
    pub(crate) update_tracker: Option<Arc<gitfleet_app::updater::UpdateDownloadTracker>>,
    pub(crate) update_confirm_dirty: bool,

    // Context Menus, Confirmations, Error Popup & Clipboard
    pub(crate) cursor_pos: Point,
    pub(crate) window_size: Size,
    pub(crate) context_menu: Option<ContextMenuState>,
    pub(crate) pending_confirm: Option<PendingConfirm>,
    pub(crate) action_error: Option<ActionErrorState>,
    pub(crate) last_action_error: Option<ActionErrorState>,
    pub(crate) cherry_pick_modal: Option<CherryPickModalState>,
    pub(crate) discard_modal: Option<DiscardModalState>,
    pub(crate) inline_group_input_id: text_input::Id,

    pub(crate) busy_label: Option<String>,
    pub(crate) toast_message: String,
    pub(crate) toast_is_error: bool,
}

pub(crate) fn app_window_icon() -> Option<window::Icon> {
    window::icon::from_file_data(include_bytes!("../../../../assets/icon-64.png"), None).ok()
}

impl GitFleetApp {
    pub(crate) fn new() -> (Self, Task<Message>) {
        let service = AppService::new();
        let ws = service.workspace.current();
        let (host_id, open_task) = window::open(window::Settings {
            size: iced::Size::new(1440.0, 900.0),
            min_size: Some(iced::Size::new(1024.0, 660.0)),
            icon: app_window_icon(),
            ..Default::default()
        });

        let mut windows = BTreeMap::new();
        windows.insert(host_id, WindowKind::Host);

        let initial_repo = ws
            .ui
            .selected_repo_id
            .clone()
            .filter(|id| ws.repos.iter().any(|r| &r.id == id))
            .or_else(|| ws.repos.first().map(|r| r.id.clone()));

        let loaded_app_settings = service.settings();
        if let Some(custom_path) = loaded_app_settings.custom_theme_path.as_deref()
            && let Ok((_name, custom_pal)) = crate::theme::load_custom_theme_from_path(custom_path)
        {
            crate::theme::set_active_custom_palette(custom_pal);
        }
        let initial_preset = if let Some(preset_id) = ws.ui.theme_preset.as_deref() {
            ThemePreset::from_id(preset_id)
        } else if !loaded_app_settings.theme.trim().is_empty()
            && loaded_app_settings.theme != "dark"
        {
            ThemePreset::from_id(&loaded_app_settings.theme)
        } else if ws.ui.light_theme {
            ThemePreset::DaylightCrisp
        } else {
            ThemePreset::PrecisionObsidian
        };

        let mut app = Self {
            theme_preset: initial_preset,
            theme_mode: initial_preset.mode(),
            host_layout: HostLayoutMode::Cockpit,
            pre_zen_layout: HostLayoutMode::Cockpit,
            selected_repo: initial_repo.clone(),
            active_submodule: None,
            repo_submodules: HashMap::new(),
            collapsed_repo_submodules: HashSet::new(),
            service,
            windows,
            detached_viewers: BTreeMap::new(),
            pending_detached_viewers: VecDeque::new(),
            filter_query: String::new(),
            repo_filter: RepoFilter::All,
            bulk_selected: HashSet::new(),
            statuses: HashMap::new(),
            bottom_left_tab: BottomLeftTab::Branches,
            branches: Vec::new(),
            selected_branch: None,
            last_branch_click: None,
            last_file_click: None,
            graph_checked_branches: HashSet::new(),
            toggled_branch_nodes: HashSet::new(),
            stashes: Vec::new(),
            tags: Vec::new(),
            submodules: Vec::new(),
            worktrees: Vec::new(),
            orphan_gitlinks: Vec::new(),
            release_triggers: Vec::new(),
            new_tag_name: String::new(),
            new_submodule_url: String::new(),
            new_submodule_path: String::new(),
            new_worktree_branch: String::new(),
            graph_tab: GraphTab::Graph,
            graph_all_branches: false,
            graph_topo_order: false,
            commit_filter: String::new(),
            graph_selection: GraphSelection::WorkingTree,
            commits: Vec::new(),
            graph_rows: Vec::new(),
            graph_max_lanes: 1,
            reflog: Vec::new(),
            commit_summary: String::new(),
            commit_summary_id: text_input::Id::unique(),
            commit_editor: text_editor::Content::new(),
            amend_commit: false,
            amend_target: None,
            files_mode: FilesViewMode::Flat,
            show_ignored_files: ws.ui.show_ignored_files,
            file_filter: String::new(),
            selected_dir_filter: None,
            selected_all_dir: String::new(),
            expanded_all_dirs: HashSet::new(),
            commit_files: Vec::new(),
            all_repo_files: Vec::new(),
            ignored_repo_files: Vec::new(),
            ignored_repo_dirs: Vec::new(),
            ignored_dir_children: HashMap::new(),
            ignored_entries_loaded: false,
            ignored_entries_stale: false,
            bottom_mode: BottomViewMode::SideBySideDiff,
            last_text_diff_mode: BottomViewMode::SideBySideDiff,
            csv_prefer_table: true,
            diff_request_id: 0,
            diff_image: None,
            image_diff_mode: crate::views::image_diff::ImageDiffMode::Swipe,
            image_diff_zoom: crate::views::image_diff::ImageDiffZoom::Fit,
            image_diff_swapped: false,
            image_diff_swipe: 0.5,
            image_diff_onion: 0.5,
            image_diff_blink_after: false,
            image_diff_blink_paused: false,
            diff_csv: None,
            csv_diff_changes_only: false,
            csv_diff_ignore_format: false,
            csv_diff_filter: String::new(),
            active_file_path: None,
            selected_changed_file: None,
            selected_changed_files: HashSet::new(),
            file_selection_anchor: None,
            focus_pane: ActiveFocusPane::Other,
            keyboard_modifiers: iced::keyboard::Modifiers::empty(),
            diff_title: "No file selected".into(),
            diff_parsed_path: None,
            diff_parsed: ParsedDiff::default(),
            diff_cache: Arc::new(DiffLayoutCache::default()),
            diff_is_working_file: false,
            diff_is_staged: false,
            diff_compact: true,
            diff_expanded_gaps: HashMap::new(),
            diff_left_scroll_id: scrollable::Id::unique(),
            diff_unified_scroll_id: scrollable::Id::unique(),
            diff_left_scroll_y: 0.0,
            diff_unified_scroll_y: 0.0,
            diff_left_scroll_x: 0.0,
            diff_left_viewport_h: 900.0,
            diff_unified_viewport_h: 900.0,
            diff_active_change: 0,
            diff_selection: None,
            diff_last_click: None,
            doc_request_id: 0,
            open_doc_raw_text: None,
            pending_jump_line: None,
            preview_hydrated: false,
            editor_hydrated: false,
            preview_state: preview::PreviewState::Empty,
            preview_editor: text_editor::Content::new(),
            preview_scroll: crate::views::code_editor::CodeScrollState::new(),
            preview_raw_mode: false,
            open_doc_id: None,
            open_doc_version: None,
            open_doc_path: None,
            open_doc_line_ending: LineEnding::Lf,
            open_doc_encoding: FileEncoding::Utf8,
            open_doc_language: CodeLanguage::PlainText,
            editor_content: text_editor::Content::new(),
            editor_scroll: crate::views::code_editor::CodeScrollState::new(),
            editor_dirty: false,
            show_eol_modal: false,
            show_encoding_modal: false,
            show_syntax_modal: false,
            syntax_filter: String::new(),
            syntax_filter_id: text_input::Id::unique(),
            doc_format_menu_pos: Point::ORIGIN,
            encoding_modal_mode: EncodingModalMode::SaveWithEncoding,
            show_palette: false,
            palette_input_id: text_input::Id::unique(),
            palette_query: String::new(),
            palette_selected_idx: 0,
            show_branch_popover: false,
            branch_filter_id: text_input::Id::unique(),
            branch_filter_query: String::new(),
            branch_popover_selected_idx: 0,
            branch_create_base: None,
            branch_create_checkout: true,
            workspace_modal_tab: WorkspaceModalTab::Workspaces,
            blueprint_export_modal: None,
            blueprint_import_modal: None,
            show_merge_modal: false,
            merge_branch_filter: String::new(),
            merge_target_branch: None,
            merge_forecast: None,
            rename_branch_modal: None,
            rename_branch_input_id: text_input::Id::unique(),
            rename_workspace_modal: None,
            rename_workspace_input_id: text_input::Id::unique(),
            rename_group_modal: None,
            rename_group_input_id: text_input::Id::unique(),
            rename_entry_modal: None,
            rename_entry_input_id: text_input::Id::unique(),
            add_gitignore_modal: None,
            add_gitignore_input_id: text_input::Id::unique(),
            commit_modal: None,
            commit_modal_editor: text_editor::Content::new(),
            scan_modal: None,
            scan_request_id: 0,
            conflict_file_path: None,
            conflict_parsed: ParsedConflictFile::default(),
            conflict_active_hunk: None,
            conflict_editing_hunk: None,
            conflict_hunk_editor: text_editor::Content::new(),
            conflict_expanded_segments: HashSet::new(),
            conflict_scroll_id: scrollable::Id::unique(),
            show_git_flow_modal: false,
            git_flow_editing_config: false,
            git_flow_state: GitFlowState::default(),
            git_flow_main_input: "main".into(),
            git_flow_develop_input: "develop".into(),
            git_flow_feature_prefix: "feature/".into(),
            git_flow_release_prefix: "release/".into(),
            git_flow_hotfix_prefix: "hotfix/".into(),
            git_flow_bugfix_prefix: "bugfix/".into(),
            git_flow_name_input: String::new(),
            git_flow_kind: GitFlowKind::Feature,
            drawer_tab: None,
            bulk_cancel: None,
            show_workspace_modal: false,
            show_inline_group_input: false,
            new_group_parent: None,
            new_group_input: String::new(),
            repo_drag_candidate: None,
            dragging_repo: None,
            drag_hover_group: None,
            new_workspace_name: String::new(),
            last_repo_parent_dir: None,
            show_clone_modal: false,
            clone_modal: CloneModalState::default(),
            clone_url_input_id: text_input::Id::unique(),
            active_clone: None,
            add_repo_modal: None,
            add_repo_path_input_id: text_input::Id::unique(),
            add_repo_name_input_id: text_input::Id::unique(),
            pending_relocation: None,
            staged_secrets: Vec::new(),
            oversized_files: Vec::new(),
            vault_summary: VaultSummary::default(),
            secret_candidates: Vec::new(),
            show_secret_picker: false,
            secret_add_path_input: String::new(),
            show_key_manager: false,
            new_key_name_input: String::new(),
            show_import_key_bar: false,
            import_key_name_input: String::new(),
            import_key_input: String::new(),
            renaming_key: None,
            show_create_profile_bar: false,
            new_profile_name_input: String::new(),
            clone_from_active_profile: true,
            renaming_profile: None,
            revealed_secrets: HashMap::new(),
            secret_file_input: ".env".into(),
            secret_key_filter: String::new(),
            secret_key_input: String::new(),
            secret_val_input: String::new(),
            ignore_layer: IgnoreLayer::RootGitignore,
            ignore_snapshot: IgnoreSnapshot::default(),
            ignore_editor: text_editor::Content::new(),
            ignore_test_path: String::new(),
            ignore_test_result: None,
            search_query: String::new(),
            search_case_sensitive: false,
            search_whole_word: false,
            search_regex: false,
            search_show_advanced: false,
            search_replace_text: String::new(),
            search_include_glob: String::new(),
            search_exclude_glob: String::new(),
            collapsed_search_repos: HashSet::new(),
            search_running: false,
            search_results: Vec::new(),
            command_log_filter: CommandLogFilter::All,
            command_log_query: String::new(),
            selected_command_log_id: None,
            terminal_tabs: Vec::new(),
            active_terminal_tab: 0,
            terminal_focused: false,
            terminal_expanded: false,
            terminal_search_open: false,
            terminal_search_query: String::new(),
            dock_position: DockPosition::Bottom,
            drawer_height: crate::theme::layout::DOCK_DEFAULT_HEIGHT,
            drawer_width: crate::theme::layout::DOCK_RIGHT_DEFAULT_WIDTH,
            active_resize: None,
            sidebar_width: 304.0,
            left_split_ratio: 0.52,
            right_split_ratio: 0.54,
            top_right_split_ratio: 0.56,
            commit_files_split_ratio: 0.36,
            files_staged_split_ratio: 0.62,
            show_terminal_sidebar: false,
            show_terminal_menu: false,
            terminal_menu_submenu: TerminalMenuSubmenu::None,
            terminal_fonts: crate::terminal::detect_terminal_fonts(),
            show_settings_modal: false,
            settings_tab: SettingsTab::General,
            settings_font_filter: String::new(),
            git_version: "checking…".into(),
            detected_tools: Vec::new(),
            show_update_modal: false,
            update_status: UpdateUiStatus::Idle,
            update_release_markdown: Vec::new(),
            update_tracker: None,
            update_confirm_dirty: false,
            cursor_pos: Point::ORIGIN,
            window_size: Size::new(1440.0, 900.0),
            context_menu: None,
            pending_confirm: None,
            action_error: None,
            last_action_error: None,
            cherry_pick_modal: None,
            discard_modal: None,
            inline_group_input_id: text_input::Id::unique(),
            busy_label: None,
            toast_message: if ws.repos.is_empty() {
                "Ready — click Clone, Add, or Scan in Repositories to open your repos".into()
            } else {
                format!("Loaded {} repositories", ws.repos.len())
            },
            toast_is_error: false,
        };

        let mut tasks = vec![open_task.discard()];
        {
            let svc = app.service.clone();
            tasks.push(Task::perform(
                async move { svc.git.git_version().await },
                Message::GitVersionLoaded,
            ));
        }
        {
            let cur_settings = app.service.settings();
            if gitfleet_app::updater::should_auto_check(&cur_settings, cfg!(debug_assertions)) {
                let svc = app.service.clone();
                let etag = gitfleet_app::updater::effective_etag_for_version(
                    &cur_settings,
                    gitfleet_app::updater::current_app_version(),
                )
                .map(str::to_string);
                tasks.push(Task::perform(
                    async move { svc.updater.check_latest_release(etag.as_deref()).await },
                    |result| Message::UpdateCheckCompleted {
                        manual: false,
                        result,
                    },
                ));
            }
        }
        for repo in &ws.repos {
            tasks.push(app.load_status_task(repo.clone()));
            tasks.push(app.load_submodules_task(repo.clone()));
        }
        if let Some(repo) = app.current_repo() {
            if app.service.settings().watch_filesystem {
                app.service.scheduler.watch_repo(&repo.path);
            }
            app.refresh_vault_summary();
            tasks.push(app.load_log_task(repo.clone()));
            tasks.push(app.load_branches_task(repo.clone()));
            tasks.push(app.load_stashes_task(repo.clone()));
            tasks.push(app.load_tags_task(repo.clone()));
            tasks.push(app.load_worktrees_task(repo.clone()));
            tasks.push(app.load_all_files_task(repo.clone()));
            if app.show_ignored_files {
                tasks.push(app.load_ignored_entries_task(repo));
            }
        }

        (app, Task::batch(tasks))
    }

    pub(crate) fn palette(&self) -> Palette {
        self.theme_preset.palette()
    }

    pub(crate) fn composed_commit_message(&self) -> String {
        let sum = self.commit_summary.trim();
        let body_raw = self.commit_editor.text();
        let body = body_raw.trim();
        match (sum.is_empty(), body.is_empty()) {
            (true, true) => String::new(),
            (false, true) => sum.to_string(),
            (true, false) => body.to_string(),
            (false, false) => format!("{sum}\n\n{body}"),
        }
    }

    pub(crate) fn set_full_commit_message(&mut self, full: &str) {
        let normalized = full.replace("\r\n", "\n");
        let trimmed = normalized.trim();
        if let Some((first, rest)) = trimmed.split_once('\n') {
            self.commit_summary = first.trim().to_string();
            let rest_clean = rest.trim_start_matches('\n').trim();
            self.commit_editor = if rest_clean.is_empty() {
                text_editor::Content::new()
            } else {
                text_editor::Content::with_text(rest_clean)
            };
        } else {
            self.commit_summary = trimmed.to_string();
            self.commit_editor = text_editor::Content::new();
        }
    }

    pub(crate) fn clear_commit_composer(&mut self) {
        self.commit_summary.clear();
        self.commit_editor = text_editor::Content::new();
    }

    pub(crate) fn apply_theme_preset(&mut self, preset: ThemePreset) {
        self.theme_preset = preset;
        self.theme_mode = preset.mode();
        self.rebuild_diff_cache();
        for viewer in self.detached_viewers.values_mut() {
            for tab in &mut viewer.tabs {
                tab.rebuild_diff_cache(self.theme_mode);
            }
        }
        let mut s = self.service.settings();
        s.theme = preset.id().to_string();
        self.service.update_settings(s);
        self.persist_ui_prefs();
    }

    pub(crate) fn persist_ui_prefs(&self) {
        let mut ui = self.service.workspace.current().ui;
        ui.selected_repo_id = self.selected_repo.clone();
        ui.light_theme = self.theme_mode == ThemeMode::Light;
        ui.theme_preset = Some(self.theme_preset.id().to_string());
        ui.show_ignored_files = self.show_ignored_files;
        let _ = self.service.workspace.set_ui_state(ui);
    }

    pub(crate) fn submodule_repo_id(parent_id: &RepoId, sub_path: &str) -> RepoId {
        RepoId(format!("{}::sub::{}", parent_id.0, sub_path))
    }

    pub(crate) fn is_submodule_repo_id(id: &RepoId) -> bool {
        id.0.contains("::sub::")
    }

    pub(crate) fn active_repo_key(&self) -> Option<RepoId> {
        if let Some((parent_id, sub_rel)) = &self.active_submodule {
            return Some(Self::submodule_repo_id(parent_id, sub_rel));
        }
        self.selected_repo.clone()
    }

    pub(crate) fn active_status(&self) -> Option<&RepoStatusSummary> {
        let key = self.active_repo_key()?;
        self.statuses.get(&key)
    }

    pub(crate) fn current_repo(&self) -> Option<RepoEntry> {
        let id = self.selected_repo.as_ref()?;
        let parent = self
            .service
            .workspace
            .current()
            .repos
            .into_iter()
            .find(|r| &r.id == id)?;
        if let Some((parent_id, sub_rel)) = &self.active_submodule
            && parent_id == &parent.id
        {
            let sub_path = parent.path.join(sub_rel);
            let sub_leaf = sub_rel
                .rsplit('/')
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or(sub_rel.as_str());
            return Some(RepoEntry {
                id: Self::submodule_repo_id(&parent.id, sub_rel),
                name: format!("{} / {}", parent.name, sub_leaf),
                available: sub_path.exists(),
                path: sub_path,
                group: parent.group,
                pinned: parent.pinned,
                secret_key_id: parent.secret_key_id,
                secret_profile: parent.secret_profile,
            });
        }
        Some(parent)
    }

    /// Returns the locally selected Secrets Vault profile for the active repository on this machine
    /// (`None` means the base `"default"` profile).
    pub(crate) fn active_repo_secret_profile(&self) -> Option<String> {
        let id = self.selected_repo.as_ref()?;
        self.service.workspace.repo_secret_profile(id)
    }

    /// Returns the workspace-assigned secret key binding (`Repo` override or nearest ancestor `Group`)
    /// for the currently selected repository (submodules inherit from their parent repository).
    pub(crate) fn active_repo_secret_key_binding(
        &self,
    ) -> Option<(String, gitfleet_workspace::SecretKeyBindingSource)> {
        let id = self.selected_repo.as_ref()?;
        self.service.workspace.resolve_repo_secret_key(id)
    }

    /// Resolves the effective Age key entry & human-readable source label for the currently active repo.
    pub(crate) fn resolve_active_repo_effective_key(
        &self,
    ) -> Option<(gitfleet_security::VaultKeyEntry, String)> {
        let keyring = self.service.security.keyring_summary();
        if let Some((key_id, source)) = self.active_repo_secret_key_binding()
            && let Some(entry) = keyring.find(&key_id)
        {
            let label = match source {
                gitfleet_workspace::SecretKeyBindingSource::Repo => "Repo".to_string(),
                gitfleet_workspace::SecretKeyBindingSource::Group(g) => format!("Group: {g}"),
            };
            return Some((entry.clone(), label));
        }
        if let Some(unlocked_id) = self.vault_summary.unlocked_by_key_id.as_deref()
            && let Some(entry) = keyring.find(unlocked_id)
        {
            return Some((entry.clone(), "Vault".to_string()));
        }
        keyring
            .default_key()
            .map(|entry| (entry.clone(), "Default".to_string()))
    }

    pub(crate) fn filtered_repos(&self, repos: &[RepoEntry]) -> Vec<RepoEntry> {
        let q = self.filter_query.to_lowercase();
        repos
            .iter()
            .filter(|repo| {
                if !q.is_empty() && !repo.name.to_lowercase().contains(&q) {
                    return false;
                }
                let st = self.statuses.get(&repo.id);
                match self.repo_filter {
                    RepoFilter::All => true,
                    RepoFilter::Dirty => st.is_some_and(|s| !s.is_clean),
                    RepoFilter::Sync => st.is_some_and(|s| s.ahead > 0 || s.behind > 0),
                }
            })
            .cloned()
            .collect()
    }


    pub(crate) fn suggested_repo_parent_dir(&self) -> Option<PathBuf> {
        if let Some(last) = &self.last_repo_parent_dir
            && last.is_dir()
        {
            return Some(last.clone());
        }
        if let Some(cur) = self.current_repo()
            && !Self::is_submodule_repo_id(&cur.id)
            && let Some(parent) = cur.path.parent()
            && parent.is_dir()
        {
            return Some(parent.to_path_buf());
        }
        self.service
            .workspace
            .current()
            .repos
            .iter()
            .find_map(|r| r.path.parent().filter(|p| p.is_dir()).map(PathBuf::from))
    }

    pub(crate) fn has_modal_or_menu_open(&self) -> bool {
        self.show_palette
            || self.show_branch_popover
            || self.show_merge_modal
            || self.show_git_flow_modal
            || self.show_workspace_modal
            || self.show_clone_modal
            || self.show_settings_modal
            || self.show_update_modal
            || self.show_eol_modal
            || self.show_encoding_modal
            || self.show_syntax_modal
            || self.show_terminal_menu
            || self.context_menu.is_some()
            || self.pending_confirm.is_some()
            || self.action_error.is_some()
            || self.cherry_pick_modal.is_some()
            || self.discard_modal.is_some()
            || self.rename_branch_modal.is_some()
            || self.rename_workspace_modal.is_some()
            || self.rename_group_modal.is_some()
            || self.rename_entry_modal.is_some()
            || self.add_gitignore_modal.is_some()
            || self.commit_modal.is_some()
            || self.add_repo_modal.is_some()
            || self.scan_modal.is_some()
            || self.blueprint_export_modal.is_some()
            || self.blueprint_import_modal.is_some()
    }

    pub(crate) fn unsaved_editor_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        if self.editor_dirty {
            let label = self
                .active_file_path
                .as_deref()
                .and_then(|p| p.rsplit(['/', '\\']).next())
                .unwrap_or("Host Editor")
                .to_string();
            names.push(label);
        }
        for viewer in self.detached_viewers.values() {
            for tab in &viewer.tabs {
                if tab.dirty {
                    names.push(tab.title.clone());
                }
            }
        }
        names
    }

    pub(crate) fn active_error_context(&self) -> Option<String> {
        let repo = self.current_repo()?;
        let branch = self.active_status().and_then(|s| s.branch.as_deref());
        Some(match branch {
            Some(b) => format!("{}  ·  {}", repo.name, b),
            None => repo.name,
        })
    }

    /// Records an error in the status bar AND displays a clear popup notification modal so
    /// action failures are never silently missed in the bottom status strip.
    pub(crate) fn report_action_error(
        &mut self,
        title: impl Into<String>,
        raw_error: impl Into<String>,
        show_output_shortcut: bool,
    ) {
        let state = build_action_error_state(
            title.into(),
            raw_error.into(),
            self.active_error_context(),
            show_output_shortcut,
        );
        self.toast_is_error = true;
        self.toast_message = format!("{}: {}", state.title, state.summary);
        self.last_action_error = Some(state.clone());
        self.action_error = Some(state);
    }

    /// Records a background/non-interactive error in the status bar while keeping full details
    /// one click away (clicking the error indicator in the status bar opens the Error Modal).
    pub(crate) fn record_background_error(
        &mut self,
        title: impl Into<String>,
        raw_error: impl Into<String>,
        show_output_shortcut: bool,
    ) {
        let state = build_action_error_state(
            title.into(),
            raw_error.into(),
            self.active_error_context(),
            show_output_shortcut,
        );
        self.toast_is_error = true;
        self.toast_message = format!("{}: {}", state.title, state.summary);
        self.last_action_error = Some(state);
    }

    pub(crate) fn default_terminal_profile_id(&self) -> String {
        let configured = self.service.settings().default_terminal_profile;
        self.service
            .terminal
            .resolve_default_profile_id(configured.as_deref())
    }

    pub(crate) fn active_terminal_font(&self) -> String {
        self.service
            .settings()
            .terminal_font_family
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| self.terminal_fonts.default_font.clone())
    }

    pub(crate) fn focused_terminal_session_id(&self) -> Option<SessionId> {
        let tab = self.terminal_tabs.get(self.active_terminal_tab)?;
        let pane = tab
            .panes
            .get(tab.focused_pane)
            .or_else(|| tab.panes.first())?;
        Some(pane.session_id.clone())
    }

    pub(crate) fn spawn_terminal_pane(&mut self, profile_id: Option<String>, split_into_active: bool) {
        let pid = profile_id.unwrap_or_else(|| self.default_terminal_profile_id());
        let prof = self.service.terminal.profile_by_id(&pid).or_else(|| {
            let fallback = self.default_terminal_profile_id();
            self.service.terminal.profile_by_id(&fallback)
        });
        let Some(prof) = prof else {
            self.report_action_error(
                "Terminal Failed",
                "No terminal profile is available on this system.",
                false,
            );
            return;
        };

        let cwd = gitfleet_core::strip_windows_verbatim_prefix(
            &self
                .current_repo()
                .map(|r| r.path)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
        );
        let repo_id = self.active_repo_key().unwrap_or_default();

        // Estimate initial cols/rows based on current window width and split count
        let pane_count = if split_into_active
            && let Some(tab) = self.terminal_tabs.get(self.active_terminal_tab)
        {
            tab.panes.len() + 1
        } else {
            1
        };
        let font_name = self.active_terminal_font();
        let font_size = self.service.settings().terminal_font_size.clamp(12, 24) as f32;
        let (cell_w, cell_h) = crate::terminal::cell_metrics(&font_name, font_size);
        let drawer_h: f32 = self.drawer_height.max(160.0);
        let approx_w = ((self.window_size.width - 36.0) / pane_count as f32).max(260.0);
        let approx_h = (drawer_h - 46.0).max(100.0);
        let cols = (approx_w / cell_w).floor().clamp(40.0, 260.0) as u16;
        let rows = (approx_h / cell_h).floor().clamp(8.0, 60.0) as u16;

        match self
            .service
            .terminal
            .create_in_dir(repo_id, &cwd, &prof.id, cols, rows)
        {
            Ok(sid) => {
                let snapshot = self
                    .service
                    .terminal
                    .screen_snapshot(&sid)
                    .unwrap_or_default();
                let pane = TerminalPane {
                    session_id: sid,
                    profile_label: prof.label.clone(),
                    short_label: if prof.short_label.is_empty() {
                        prof.label.clone()
                    } else {
                        prof.short_label.clone()
                    },
                    cols,
                    rows,
                    snapshot,
                };
                if split_into_active
                    && let Some(tab) = self.terminal_tabs.get_mut(self.active_terminal_tab)
                {
                    tab.panes.push(pane);
                    tab.focused_pane = tab.panes.len().saturating_sub(1);
                } else {
                    self.terminal_tabs.push(TerminalTab {
                        panes: vec![pane],
                        focused_pane: 0,
                    });
                    self.active_terminal_tab = self.terminal_tabs.len().saturating_sub(1);
                }
                self.drawer_tab = Some(DrawerTab::Terminal);
                self.terminal_focused = true;
                self.show_terminal_menu = false;
                self.terminal_menu_submenu = TerminalMenuSubmenu::None;
                self.toast_is_error = false;
                self.toast_message = format!("Started {} in {}", prof.label, cwd.display());
            }
            Err(e) => {
                self.report_action_error("Terminal Failed", e.to_string(), false);
            }
        }
    }

    pub(crate) fn sync_terminal_snapshots(&mut self) {
        let Some(tab) = self.terminal_tabs.get_mut(self.active_terminal_tab) else {
            return;
        };
        for pane in &mut tab.panes {
            let seq = self.service.terminal.dirty_seq(&pane.session_id);
            if seq != pane.snapshot.dirty_seq
                && let Some(snap) = self.service.terminal.screen_snapshot(&pane.session_id)
            {
                pane.snapshot = snap;
            }
        }
    }

    pub(crate) fn sync_preview_editor(&mut self) {
        if let Some(txt) = self.preview_state.copyable_text() {
            self.preview_editor = text_editor::Content::with_text(txt);
        } else {
            self.preview_editor = text_editor::Content::new();
        }
        self.preview_raw_mode = false;
    }

    pub(crate) fn current_host_cursor_pos(&self) -> Point {
        self.windows
            .iter()
            .find_map(|(id, kind)| (*kind == WindowKind::Host).then_some(*id))
            .and_then(crate::app::subscription::latest_cursor_pos)
            .unwrap_or(self.cursor_pos)
    }

    pub(crate) fn rebuild_diff_cache(&mut self) {
        self.diff_cache = build_diff_layout_cache(
            &self.diff_parsed,
            self.diff_compact,
            &self.diff_expanded_gaps,
            self.open_doc_language,
            self.theme_mode,
        );
    }

    pub(crate) fn clear_diff_parsed_and_cache(&mut self) {
        self.diff_parsed_path = None;
        self.diff_parsed = ParsedDiff::default();
        self.diff_cache = Arc::new(DiffLayoutCache::default());
        self.diff_expanded_gaps.clear();
        self.diff_selection = None;
        self.diff_left_scroll_y = 0.0;
        self.diff_unified_scroll_y = 0.0;
        self.diff_left_scroll_x = 0.0;
        self.diff_active_change = 0;
    }

    pub(crate) fn ensure_preview_hydrated(&mut self) {
        if self.preview_hydrated {
            return;
        }
        if let Some(full_path) = self.open_doc_path.clone().or_else(|| {
            self.current_repo().and_then(|r| {
                self.active_file_path
                    .as_deref()
                    .map(|rel| r.path.join(rel))
            })
        }) {
            self.preview_state =
                preview::PreviewState::from_file(&full_path, self.open_doc_raw_text.as_deref());
            self.sync_preview_editor();
        } else {
            self.preview_state = preview::PreviewState::Empty;
            self.preview_editor = text_editor::Content::new();
        }
        self.preview_hydrated = true;
    }

    pub(crate) fn ensure_editor_hydrated(&mut self) {
        if self.editor_hydrated {
            return;
        }
        if let Some(raw) = &self.open_doc_raw_text {
            self.editor_content = text_editor::Content::with_text(raw);
        } else {
            self.editor_content = text_editor::Content::new();
        }
        self.editor_hydrated = true;
    }

    /// Asynchronously loads file metadata/content & scans for conflict markers off the UI thread,
    /// avoiding synchronous disk reads and `cosmic_text` editor/preview shaping when clicking
    /// between files in Diff view.
    pub(crate) fn open_file_for_preview_and_edit_async(&mut self, rel_path: &str) -> Task<Message> {
        self.active_file_path = Some(rel_path.to_string());
        self.open_doc_language = CodeLanguage::from_path(std::path::Path::new(rel_path));
        self.editor_dirty = false;
        self.open_doc_raw_text = None;
        self.preview_hydrated = false;
        self.editor_hydrated = false;
        self.clear_conflict_state();

        let Some(repo) = self.current_repo() else {
            self.open_doc_id = None;
            self.open_doc_version = None;
            self.open_doc_path = None;
            return Task::none();
        };

        self.doc_request_id = self.doc_request_id.wrapping_add(1);
        let req_id = self.doc_request_id;
        let full_path = repo.path.join(rel_path);
        let rel = rel_path.to_string();
        let is_git_conflicted = self
            .active_status()
            .is_some_and(|st| st.conflict_files.iter().any(|f| f.path == rel_path));
        let svc = self.service.clone();

        Task::perform(
            async move {
                let payload = tokio::task::spawn_blocking(move || {
                    let doc = svc.docs.open_document(&full_path).ok();
                    let conflict_parsed = if let Some(cap) = &doc {
                        if is_git_conflicted || has_conflict_markers(&cap.content) {
                            Some(ParsedConflictFile::parse(&cap.content))
                        } else {
                            None
                        }
                    } else if is_git_conflicted {
                        std::fs::read_to_string(&full_path)
                            .ok()
                            .map(|raw| ParsedConflictFile::parse(&raw))
                    } else {
                        None
                    };
                    crate::app::message::DocContextPayload {
                        req_id,
                        rel_path: rel,
                        full_path,
                        doc,
                        conflict_parsed,
                    }
                })
                .await
                .unwrap_or_else(|_| crate::app::message::DocContextPayload {
                    req_id,
                    rel_path: String::new(),
                    full_path: PathBuf::new(),
                    doc: None,
                    conflict_parsed: None,
                });
                Box::new(payload)
            },
            Message::DocContextLoaded,
        )
    }

    pub(crate) fn reload_file_for_preview_and_edit_if_changed(&mut self, rel_path: &str) {
        if self.editor_dirty {
            return;
        }
        let Some(repo) = self.current_repo() else {
            return;
        };
        let full_path = repo.path.join(rel_path);
        let same_path = self.active_file_path.as_deref() == Some(rel_path);
        match self.service.docs.open_document(&full_path) {
            Ok(cap) => {
                if same_path
                    && self.open_doc_version.as_deref() == Some(cap.version.as_str())
                    && self.open_doc_raw_text.is_some()
                {
                    if self.bottom_mode == BottomViewMode::Preview {
                        self.ensure_preview_hydrated();
                    } else if self.bottom_mode == BottomViewMode::Edit {
                        self.ensure_editor_hydrated();
                    }
                    return;
                }
                self.active_file_path = Some(rel_path.to_string());
                self.open_doc_id = Some(cap.document_id);
                self.open_doc_version = Some(cap.version);
                self.open_doc_path = Some(full_path);
                self.open_doc_line_ending = cap.line_ending;
                self.open_doc_encoding = cap.encoding;
                self.open_doc_raw_text = Some(cap.content);
                self.editor_dirty = false;
                self.preview_hydrated = false;
                self.editor_hydrated = false;
                if self.bottom_mode == BottomViewMode::Preview {
                    self.ensure_preview_hydrated();
                } else if self.bottom_mode == BottomViewMode::Edit {
                    self.ensure_editor_hydrated();
                }
            }
            Err(_) => {
                self.active_file_path = Some(rel_path.to_string());
                self.open_doc_id = None;
                self.open_doc_version = None;
                self.open_doc_path = None;
                self.open_doc_raw_text = None;
                self.open_doc_line_ending = LineEnding::Lf;
                self.open_doc_encoding = FileEncoding::Utf8;
                self.editor_dirty = false;
                self.preview_hydrated = false;
                self.editor_hydrated = false;
                if self.bottom_mode == BottomViewMode::Preview {
                    self.ensure_preview_hydrated();
                }
            }
        }
        self.load_conflict_state_for_file(rel_path);
    }

    /// Clears the active diff/preview viewer when the currently selected working-tree file is no
    /// longer changed (e.g. after Discard, Commit, or external revert).
    pub(crate) fn clear_active_diff_viewer(&mut self) {
        self.diff_request_id = self.diff_request_id.wrapping_add(1);
        self.doc_request_id = self.doc_request_id.wrapping_add(1);
        self.selected_changed_file = None;
        self.diff_is_working_file = false;
        self.diff_is_staged = false;
        self.clear_diff_parsed_and_cache();
        self.diff_image = None;
        self.diff_csv = None;
        self.clear_conflict_state();
        if !self.editor_dirty {
            self.active_file_path = None;
            self.diff_title = "No file selected".into();
            self.open_doc_raw_text = None;
            self.preview_hydrated = false;
            self.editor_hydrated = false;
            self.preview_state = preview::PreviewState::Empty;
            self.preview_editor = text_editor::Content::new();
            self.preview_scroll.reset_offsets();
            self.editor_content = text_editor::Content::new();
            self.editor_scroll.reset_offsets();
            self.open_doc_id = None;
            self.open_doc_version = None;
            self.open_doc_path = None;
            self.open_doc_line_ending = LineEnding::Lf;
            self.open_doc_encoding = FileEncoding::Utf8;
            self.show_eol_modal = false;
            self.show_encoding_modal = false;
            self.show_syntax_modal = false;
        }
        if matches!(
            self.bottom_mode,
            BottomViewMode::ImageDiff
                | BottomViewMode::CsvDiff
                | BottomViewMode::ResolveConflicts
        ) {
            self.bottom_mode = self.last_text_diff_mode;
        }
    }

    pub(crate) fn load_conflict_state_for_file(&mut self, rel_path: &str) {
        let Some(repo) = self.current_repo() else {
            self.clear_conflict_state();
            return;
        };
        let is_git_conflicted = self
            .active_status()
            .is_some_and(|st| st.conflict_files.iter().any(|f| f.path == rel_path));
        let full_path = repo.path.join(rel_path);
        let raw_opt = std::fs::read_to_string(&full_path).ok();
        let has_markers = raw_opt.as_deref().is_some_and(has_conflict_markers);

        if is_git_conflicted || has_markers {
            let parsed = raw_opt
                .as_deref()
                .map(ParsedConflictFile::parse)
                .unwrap_or_default();
            let first_hunk = (parsed.conflict_count() > 0).then_some(0);
            self.conflict_file_path = Some(rel_path.to_string());
            self.conflict_parsed = parsed;
            self.conflict_active_hunk = first_hunk;
            self.conflict_editing_hunk = None;
            self.conflict_hunk_editor = text_editor::Content::new();
            self.conflict_expanded_segments.clear();
        } else {
            self.clear_conflict_state();
            if self.bottom_mode == BottomViewMode::ResolveConflicts {
                self.bottom_mode = BottomViewMode::SideBySideDiff;
            }
        }
    }

    pub(crate) fn clear_conflict_state(&mut self) {
        self.conflict_file_path = None;
        self.conflict_parsed = ParsedConflictFile::default();
        self.conflict_active_hunk = None;
        self.conflict_editing_hunk = None;
        self.conflict_hunk_editor = text_editor::Content::new();
        self.conflict_expanded_segments.clear();
    }

    pub(crate) fn active_changed_files(&self) -> Vec<FileChange> {
        match &self.graph_selection {
            GraphSelection::WorkingTree => {
                let Some(st) = self.active_status() else {
                    return Vec::new();
                };
                let mut all = Vec::new();
                all.extend(st.conflict_files.clone());
                all.extend(st.staged_files.clone());
                all.extend(st.unstaged_files.clone());
                all
            }
            GraphSelection::Commit(_)
            | GraphSelection::Range { .. }
            | GraphSelection::Stash(_) => self.commit_files.clone(),
        }
    }

    /// Returns the exact ordered list of visible files in the Files table matching current filters.
    /// In Working Tree mode, `staged_only` selects either the Staged (`true`) or Unstaged (`false`) panel.
    pub(crate) fn visible_files_for_section(
        &self,
        staged_only: bool,
        dir_filter: Option<&str>,
    ) -> Vec<FileChange> {
        let q = self.file_filter.to_lowercase();
        let is_wt = self.graph_selection == GraphSelection::WorkingTree;
        self.active_changed_files()
            .into_iter()
            .filter(|file| {
                if is_wt && file.staged != staged_only {
                    return false;
                }
                if !q.is_empty() && !file.path.to_lowercase().contains(&q) {
                    return false;
                }
                if let Some(target_dir) = dir_filter
                    && crate::views::parent_dir(&file.path) != target_dir
                {
                    return false;
                }
                true
            })
            .collect()
    }

    pub(crate) fn current_files_dir_filter(&self) -> Option<&str> {
        if self.files_mode == FilesViewMode::Dir {
            self.selected_dir_filter.as_deref()
        } else {
            None
        }
    }

    pub(crate) fn clear_ignored_repo_cache(&mut self) {
        self.ignored_repo_files.clear();
        self.ignored_repo_dirs.clear();
        self.ignored_dir_children.clear();
        self.ignored_entries_loaded = false;
        self.ignored_entries_stale = false;
    }

    pub(crate) fn is_path_in_ignored_dir(&self, path: &str) -> bool {
        if path.is_empty() {
            return false;
        }
        self.ignored_repo_dirs.iter().any(|d| {
            path == d
                || path
                    .strip_prefix(d.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    }

    pub(crate) fn is_ignored_repo_file(&self, path: &str) -> bool {
        self.ignored_repo_files
            .binary_search_by(|p| p.as_str().cmp(path))
            .is_ok()
            || self.is_path_in_ignored_dir(path)
    }

    pub(crate) fn selected_unstaged_paths(&self) -> Vec<String> {
        let Some(st) = self.active_status() else {
            return Vec::new();
        };
        let mut valid: HashSet<&str> = HashSet::new();
        for f in st.conflict_files.iter().chain(st.unstaged_files.iter()) {
            valid.insert(f.path.as_str());
        }
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for (path, staged) in &self.selected_changed_files {
            if !*staged && valid.contains(path.as_str()) && seen.insert(path.clone()) {
                out.push(path.clone());
            }
        }
        if out.is_empty()
            && let Some(cur) = &self.selected_changed_file
            && !cur.staged
            && valid.contains(cur.path.as_str())
        {
            out.push(cur.path.clone());
        }
        out.sort();
        out
    }

    pub(crate) fn selected_staged_paths(&self) -> Vec<String> {
        let Some(st) = self.active_status() else {
            return Vec::new();
        };
        let valid: HashSet<&str> = st.staged_files.iter().map(|f| f.path.as_str()).collect();
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for (path, staged) in &self.selected_changed_files {
            if *staged && valid.contains(path.as_str()) && seen.insert(path.clone()) {
                out.push(path.clone());
            }
        }
        if out.is_empty()
            && let Some(cur) = &self.selected_changed_file
            && cur.staged
            && valid.contains(cur.path.as_str())
        {
            out.push(cur.path.clone());
        }
        out.sort();
        out
    }

    pub(crate) fn selected_discard_paths(&self) -> Vec<String> {
        let Some(st) = self.active_status() else {
            return Vec::new();
        };
        let valid: HashSet<(&str, bool)> = st
            .conflict_files
            .iter()
            .chain(st.staged_files.iter())
            .chain(st.unstaged_files.iter())
            .map(|f| (f.path.as_str(), f.staged))
            .collect();
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for (path, staged) in &self.selected_changed_files {
            if valid.contains(&(path.as_str(), *staged)) && seen.insert(path.clone()) {
                out.push(path.clone());
            }
        }
        if out.is_empty()
            && let Some(cur) = &self.selected_changed_file
            && valid.contains(&(cur.path.as_str(), cur.staged))
        {
            out.push(cur.path.clone());
        }
        out.sort();
        out
    }

    pub(crate) fn prune_file_selection(&mut self) {
        if self.graph_selection != GraphSelection::WorkingTree {
            return;
        }
        let Some(st) = self.active_status() else {
            self.selected_changed_files.clear();
            self.file_selection_anchor = None;
            return;
        };
        let valid: HashSet<(String, bool)> = st
            .conflict_files
            .iter()
            .chain(st.staged_files.iter())
            .chain(st.unstaged_files.iter())
            .map(|f| (f.path.clone(), f.staged))
            .collect();
        self.selected_changed_files.retain(|k| valid.contains(k));
        if let Some(anchor) = &self.file_selection_anchor
            && !valid.contains(&(anchor.path.clone(), anchor.staged))
        {
            self.file_selection_anchor = None;
        }
    }

    pub(crate) fn title(&self, window_id: window::Id) -> String {
        match self.windows.get(&window_id) {
            Some(WindowKind::DetachedViewer) => {
                let (label, dirty) = self
                    .detached_viewers
                    .get(&window_id)
                    .map(|v| v.window_title_info())
                    .unwrap_or_else(|| (String::new(), false));
                if label.is_empty() {
                    "GitFleet Studio".to_string()
                } else if dirty {
                    format!("● {label} — GitFleet Studio")
                } else {
                    format!("{label} — GitFleet Studio")
                }
            }
            _ => {
                if let Some(repo) = self.current_repo() {
                    format!("GitFleet — {}", repo.name)
                } else {
                    "GitFleet".to_string()
                }
            }
        }
    }

    pub(crate) fn theme(&self, _window_id: window::Id) -> Theme {
        self.theme_mode.iced_theme()
    }
}

pub(crate) fn build_action_error_state(
    title: String,
    raw_error: String,
    context: Option<String>,
    show_output_shortcut: bool,
) -> ActionErrorState {
    let cleaned = raw_error.trim();
    let stripped = cleaned
        .strip_prefix("Git error: ")
        .or_else(|| cleaned.strip_prefix("git error: "))
        .or_else(|| cleaned.strip_prefix("IO error: "))
        .unwrap_or(cleaned)
        .trim();

    let lines: Vec<&str> = stripped
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();

    if lines.is_empty() {
        return ActionErrorState {
            title,
            summary: "The operation could not be completed.".into(),
            detail: None,
            context,
            show_output_shortcut,
        };
    }

    let first_line = lines[0].to_string();
    let has_multiline_or_long = lines.len() > 1 || stripped.chars().count() > 120;
    if has_multiline_or_long {
        ActionErrorState {
            title,
            summary: first_line,
            detail: Some(stripped.to_string()),
            context,
            show_output_shortcut,
        }
    } else {
        ActionErrorState {
            title,
            summary: first_line,
            detail: None,
            context,
            show_output_shortcut,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_embedded_app_window_icon() {
        let icon = app_window_icon().expect("embedded icon-64.png should decode");
        let (rgba, size) = icon.into_raw();
        assert_eq!(size.width, 64);
        assert_eq!(size.height, 64);
        assert_eq!(rgba.len(), 64 * 64 * 4);
    }

    #[test]
    fn formats_single_and_multiline_action_errors_cleanly() {
        let short = build_action_error_state(
            "Commit Failed".into(),
            "Git error: nothing to commit".into(),
            Some("gitfleet · main".into()),
            true,
        );
        assert_eq!(short.summary, "nothing to commit");
        assert!(short.detail.is_none());

        let multi = build_action_error_state(
            "Pull Failed".into(),
            "Git error: error: Your local changes would be overwritten\n\tsrc/main.rs\nAborting"
                .into(),
            Some("gitfleet · main".into()),
            true,
        );
        assert_eq!(
            multi.summary,
            "error: Your local changes would be overwritten"
        );
        assert!(multi.detail.as_deref().unwrap().contains("src/main.rs"));
    }
}
