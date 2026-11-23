//! Studio window state, per-tab state, action messages, and host snapshot constructors.

use crate::app::{
    BottomViewMode, DiffGapExpandMode, DiffGapExpansion, DiffSelectionSide, DiffTextSelection,
    EncodingModalMode, GitFleetApp, GraphSelection, Message,
};
use crate::preview::PreviewState;
use crate::syntax::CodeLanguage;
use crate::theme::ThemeMode;
use crate::views::csv_diff::{CsvDiffData, CsvDiffMsg, is_csv_diff_path};
use crate::views::diff_viewer::{DiffLayoutCache, build_diff_layout_cache};
use crate::views::image_diff::{
    ImageDiffData, ImageDiffMode, ImageDiffMsg, ImageDiffViewState, ImageDiffZoom,
    is_image_diff_path,
};
use crate::views::overlays::file_ops_modal::{AddGitignoreModalState, RenameEntryModalState};
use crate::views::overlays::fuzzy::fuzzy_match;
use super::io::{
    editor_buffer_text, file_display_name, format_editor_text_for_save, jump_editor_to_span,
};
use super::sidebar_explorer::ExplorerDirNode;
use super::smart_edit::{EditorSnapshot, MAX_UNDO_HISTORY};
use super::symbols::{
    DocumentSymbol, LineGitMark, compute_line_git_marks_from_parsed,
    compute_line_git_marks_vs_base, extract_document_symbols,
};
use gitfleet_core::DocumentId;
use gitfleet_docs::{FileEncoding, LineEnding, OpenDocumentCapability};
use gitfleet_git::{FileChange, IgnoredDirListing, IgnoredEntries, ParsedDiff};
use gitfleet_search::{
    InFileMatch, ReplaceReport, ScopedSearchResult, SearchOptions, find_in_text,
};
use gitfleet_security::IgnoreLayer;
use iced::widget::{scrollable, text_editor, text_input};
use iced::{Point, Task};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DetachedViewMode {
    Edit,
    Preview,
    SideBySideDiff,
    UnifiedDiff,
    ImageDiff,
    CsvDiff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StudioSidebarPanel {
    Explorer,
    Search,
    Compare,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StudioStatusMenu {
    Eol,
    Encoding,
    Syntax,
    Indent,
}

#[derive(Debug, Clone)]
pub(crate) struct StudioTabMenu {
    pub(crate) tab_idx: usize,
    pub(crate) position: Point,
}

#[derive(Debug, Clone)]
pub(crate) struct StudioBreadcrumbMenu {
    pub(crate) parent_dir: String,
    pub(crate) position: Point,
}

#[derive(Debug, Clone)]
pub(crate) enum DetachedGitDiffTarget {
    WorkingFile {
        repo_path: PathBuf,
        file: FileChange,
    },
    CommitFile {
        repo_path: PathBuf,
        oid: String,
        rel_path: String,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct DetachedComparePayload {
    pub(crate) title: String,
    pub(crate) left_path: Option<PathBuf>,
    pub(crate) right_path: Option<PathBuf>,
    pub(crate) single_file_path: Option<PathBuf>,
    pub(crate) parsed_diff: ParsedDiff,
    pub(crate) image_diff: Option<ImageDiffData>,
    pub(crate) csv_diff: Option<CsvDiffData>,
    pub(crate) preview_source: Option<(PathBuf, Option<String>)>,
    pub(crate) editable_source: Option<(PathBuf, String, FileEncoding, LineEnding)>,
    pub(crate) preferred_mode: DetachedViewMode,
    pub(crate) status_note: String,
}

#[derive(Debug, Clone)]
pub(crate) struct StudioExplorerMenu {
    pub(crate) rel_path: String,
    pub(crate) is_dir: bool,
    pub(crate) position: Point,
}

#[derive(Debug, Clone)]
pub(crate) struct StudioPendingDelete {
    pub(crate) rel_path: String,
    pub(crate) is_dir: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StudioCreateEntryKind {
    File { parent_rel: String },
    Folder { parent_rel: String },
    Duplicate { src_rel: String },
}

#[derive(Debug, Clone)]
pub(crate) struct StudioCreateEntryModal {
    pub(crate) kind: StudioCreateEntryKind,
    pub(crate) input: String,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct QuickOpenCandidate {
    pub(crate) rel_path: String,
    pub(crate) is_ignored: bool,
    pub(crate) is_open_tab: bool,
    pub(crate) positions: Vec<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct LoadedTabFile {
    pub(crate) path: PathBuf,
    pub(crate) doc: Option<OpenDocumentCapability>,
    pub(crate) jump_match: Option<(usize, usize, usize)>,
}

#[derive(Debug, Clone)]
pub(crate) enum DetachedAction {
    // Tabs & Navigation
    SelectTab(usize),
    CloseTab(usize),
    CloseActiveTab,
    NewUntitledTab,
    ReopenClosedTab,
    OpenTabMenu(usize),
    CloseTabMenu,
    CloseOtherTabs(usize),
    CloseSavedTabs,
    CloseAllTabs,
    CopyTabPath {
        tab_idx: usize,
        relative: bool,
    },
    ConfirmCloseTab {
        tab_id: u64,
        save_first: bool,
    },
    CancelCloseTab,
    DismissDiskConflict(u64),
    SetSidebarPanel(StudioSidebarPanel),
    ToggleSidebar,
    ToggleSplitPreview,
    StartResizeSidebar,
    StartResizeSplit,
    OpenBreadcrumbMenu(String),
    CloseBreadcrumbMenu,
    DiffActiveWithHead,
    OpenAndFocusExplorer,
    OpenAndFocusSearchSidebar,

    // Quick Open (Ctrl+P), Symbol Outline (Ctrl+Shift+O) & Go to Line (Ctrl+G)
    ToggleQuickOpen,
    OpenSymbolOutline,
    CloseQuickOpen,
    QuickOpenQueryChanged(String),
    QuickOpenMoveSelection(isize),
    QuickOpenConfirmSelected,
    QuickOpenSelectRelPath(String),
    QuickOpenSelectSymbol(usize),
    OpenGoToLine,
    CloseGoToLine,
    GoToLineInputChanged(String),
    SubmitGoToLine,

    // Explorer Tree
    ExplorerFilterChanged(String),
    ToggleExplorerShowIgnored,
    ToggleExplorerDir(String),
    RefreshExplorer,
    ExplorerFilesLoaded(u64, Vec<String>, IgnoredEntries, HashMap<String, char>),
    ExplorerIgnoredDirLoaded(u64, String, IgnoredDirListing),
    OpenExplorerFile(String),
    OpenExplorerMenu {
        rel_path: String,
        is_dir: bool,
    },
    CloseExplorerMenu,
    RevealExplorerEntry(String),
    CopyExplorerPath(String),
    OpenExplorerCreate {
        parent_rel: String,
        is_dir: bool,
    },
    OpenExplorerDuplicate {
        src_rel: String,
    },
    ExplorerCreateInputChanged(String),
    CancelExplorerCreate,
    SubmitExplorerCreate,
    ExplorerEntryCreated {
        is_dir: bool,
        result: Result<String, String>,
    },
    RequestDeleteExplorerEntry {
        rel_path: String,
        is_dir: bool,
    },
    CancelDeleteExplorerEntry,
    ConfirmDeleteExplorerEntry,
    ExplorerEntryDeleted {
        rel_path: String,
        is_dir: bool,
        abs_path: PathBuf,
        result: Result<bool, String>,
    },
    OpenExplorerRename {
        rel_path: String,
        is_dir: bool,
    },
    ExplorerRenameInputChanged(String),
    CancelExplorerRename,
    SubmitExplorerRename,
    ExplorerEntryRenamed {
        old_rel: String,
        is_dir: bool,
        old_abs: PathBuf,
        result: Result<String, String>,
    },
    OpenExplorerGitignore {
        rel_path: String,
        is_dir: bool,
    },
    ExplorerGitignoreModeChanged(bool),
    ExplorerGitignoreGlobChanged(String),
    ExplorerGitignoreRegexChanged(String),
    ExplorerGitignoreExpandMatchesToggled(bool),
    ExplorerGitignoreLayerChanged(IgnoreLayer),
    CancelExplorerGitignore,
    SubmitExplorerGitignore,

    // View Modes & Diff Controls
    SetMode(DetachedViewMode),
    ToggleCompact,
    ExpandGap {
        gap_id: usize,
        mode: DiffGapExpandMode,
    },
    LeftScrolled(scrollable::Viewport),
    UnifiedScrolled(scrollable::Viewport),
    JumpChange(i32),
    SelStart {
        side: DiffSelectionSide,
        row: usize,
        col: usize,
    },
    SelDrag {
        side: DiffSelectionSide,
        row: usize,
        col: usize,
    },
    CopySelection,
    SelectAll,
    ImageMsg(ImageDiffMsg),
    CsvMsg(CsvDiffMsg),
    PreviewEditorAction(text_editor::Action),
    PreviewScrolled(scrollable::Viewport),
    PreviewLinkClicked(String),
    TogglePreviewRaw,
    CopyPreviewText,

    // File / Compare I/O
    PickSingleFile,
    PickLeftFile,
    PickRightFile,
    SwapCompareFiles,
    ReloadFiles,
    SingleFilePicked(Option<PathBuf>),
    LeftFilePicked(Option<PathBuf>),
    RightFilePicked(Option<PathBuf>),
    PayloadLoaded(u64, Box<DetachedComparePayload>),
    TabFileLoaded(u64, u64, Box<LoadedTabFile>),
    LoadFailed(u64, String),
    GitDiffLoaded(u64, u64, Result<(ParsedDiff, Arc<DiffLayoutCache>), String>),
    GitImageDiffLoaded(u64, u64, ImageDiffData),
    GitCsvDiffLoaded(u64, u64, CsvDiffData),

    // Direct Code Editor
    EditorAction(text_editor::Action),
    EditorScrolled(scrollable::Viewport),
    InsertIndent,
    OutdentSelection,
    SmartEnter,
    ToggleComment,
    MoveLine(isize),
    TypeAutoPairChar(char),
    Undo,
    Redo,
    SaveEditor,
    SaveTabAs,
    SaveTabAsPicked(u64, Option<PathBuf>),
    SaveAllTabs,
    RevertEditor,
    OpenStatusMenu(StudioStatusMenu),
    CloseStatusMenu,
    SetEncodingMenuMode(EncodingModalMode),
    SetEditorEol(LineEnding),
    SaveEditorWithEncoding(FileEncoding),
    ReopenEditorWithEncoding(FileEncoding),
    SyntaxFilterChanged(String),
    SetTabLanguage(CodeLanguage),
    SetTabIndent {
        spaces: usize,
        use_tabs: bool,
    },
    ToggleWordWrap,
    AdjustFontSize(f32),

    // In-File Find & Replace
    OpenInFileFind {
        show_replace: bool,
    },
    CloseInFileFind,
    ToggleInFileReplace,
    InFileQueryChanged(String),
    InFileReplaceChanged(String),
    ToggleInFileCase,
    ToggleInFileWholeWord,
    ToggleInFileRegex,
    InFileJump(isize),
    InFileJumpIndex(usize),
    InFileReplaceCurrent,
    InFileReplaceAll,

    // VS Code-style Folder / Repo Search & Replace Sidebar
    PickSearchRoot,
    SearchRootPicked(Option<PathBuf>),
    SearchQueryChanged(String),
    SearchReplaceChanged(String),
    SearchIncludeChanged(String),
    SearchExcludeChanged(String),
    ToggleSearchReplaceRow,
    ToggleSearchFilters,
    ToggleSearchCase,
    ToggleSearchWholeWord,
    ToggleSearchRegex,
    ToggleSearchGitignore,
    RunSearch,
    SearchCompleted(u64, ScopedSearchResult),
    ToggleSearchGroup(String),
    ToggleSearchFilesSection,
    CollapseExpandAllSearchGroups,
    OpenSearchFile(PathBuf),
    OpenSearchMatch {
        path: PathBuf,
        line_number: usize,
        col_start: usize,
        col_end: usize,
    },
    ReplaceInSearchFile(PathBuf),
    ReplaceSingleSearchMatch {
        path: PathBuf,
        line_number: usize,
    },
    RequestReplaceAllInSearch,
    CancelReplaceAllInSearch,
    ConfirmReplaceAllInSearch,
    SearchReplaceCompleted(u64, ReplaceReport, Vec<PathBuf>),
    EscapePressed,
}

// ── Per-Tab Document / Diff State ────────────────────────────────────────────

pub(crate) struct StudioTab {
    pub(crate) id: u64,
    pub(crate) title: String,
    pub(crate) rel_path: Option<String>,
    pub(crate) mode: DetachedViewMode,
    pub(crate) last_text_diff_mode: DetachedViewMode,

    // Editable local document
    pub(crate) is_untitled: bool,
    pub(crate) file_path: Option<PathBuf>,
    pub(crate) doc_id: Option<DocumentId>,
    pub(crate) doc_version: Option<String>,
    pub(crate) language: CodeLanguage,
    pub(crate) indent_spaces: usize,
    pub(crate) indent_use_tabs: bool,
    pub(crate) encoding: FileEncoding,
    pub(crate) line_ending: LineEnding,
    pub(crate) trailing_newline: bool,
    pub(crate) raw_text: Option<String>,
    pub(crate) editor_hydrated: bool,
    pub(crate) editor_content: text_editor::Content,
    pub(crate) editor_scroll: crate::views::code_editor::CodeScrollState,
    pub(crate) git_line_marks: Arc<HashMap<usize, LineGitMark>>,
    pub(crate) disk_conflict: bool,
    pub(crate) undo_stack: VecDeque<EditorSnapshot>,
    pub(crate) redo_stack: Vec<EditorSnapshot>,
    pub(crate) last_typing_snapshot_at: Option<Instant>,
    pub(crate) dirty: bool,
    pub(crate) loading_file: bool,
    pub(crate) file_load_req_id: u64,

    // Standalone compare / Git diff
    pub(crate) left_path: Option<PathBuf>,
    pub(crate) right_path: Option<PathBuf>,
    pub(crate) git_diff_target: Option<DetachedGitDiffTarget>,
    pub(crate) diff_parsed: ParsedDiff,
    pub(crate) diff_cache: Arc<DiffLayoutCache>,
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
    pub(crate) diff_image: Option<ImageDiffData>,
    pub(crate) image_state: ImageDiffViewState,
    pub(crate) diff_csv: Option<CsvDiffData>,
    pub(crate) csv_changes_only: bool,
    pub(crate) csv_ignore_format: bool,
    pub(crate) csv_filter: String,

    // Preview
    pub(crate) preview_hydrated: bool,
    pub(crate) preview_state: PreviewState,
    pub(crate) preview_editor: text_editor::Content,
    pub(crate) preview_scroll: crate::views::code_editor::CodeScrollState,
    pub(crate) preview_raw_mode: bool,
}

impl StudioTab {
    pub(crate) fn new_empty(id: u64, title: String) -> Self {
        Self {
            id,
            title,
            rel_path: None,
            mode: DetachedViewMode::Edit,
            last_text_diff_mode: DetachedViewMode::SideBySideDiff,
            is_untitled: false,
            file_path: None,
            doc_id: None,
            doc_version: None,
            language: CodeLanguage::PlainText,
            indent_spaces: 4,
            indent_use_tabs: false,
            encoding: FileEncoding::Utf8,
            line_ending: LineEnding::Lf,
            trailing_newline: true,
            raw_text: None,
            editor_hydrated: false,
            editor_content: text_editor::Content::new(),
            editor_scroll: crate::views::code_editor::CodeScrollState::new(),
            git_line_marks: Arc::new(HashMap::new()),
            disk_conflict: false,
            undo_stack: VecDeque::new(),
            redo_stack: Vec::new(),
            last_typing_snapshot_at: None,
            dirty: false,
            loading_file: false,
            file_load_req_id: 0,
            left_path: None,
            right_path: None,
            git_diff_target: None,
            diff_parsed: ParsedDiff::default(),
            diff_cache: Arc::new(DiffLayoutCache::default()),
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
            diff_image: None,
            image_state: ImageDiffViewState {
                mode: ImageDiffMode::Swipe,
                zoom: ImageDiffZoom::Fit,
                swapped: false,
                swipe: 0.5,
                onion: 0.5,
                blink_after: false,
                blink_paused: false,
            },
            diff_csv: None,
            csv_changes_only: false,
            csv_ignore_format: false,
            csv_filter: String::new(),
            preview_hydrated: false,
            preview_state: PreviewState::Empty,
            preview_editor: text_editor::Content::new(),
            preview_scroll: crate::views::code_editor::CodeScrollState::new(),
            preview_raw_mode: false,
        }
    }

    pub(crate) fn new_untitled(id: u64, num: usize) -> Self {
        let mut tab = Self::new_empty(id, format!("Untitled-{num}"));
        tab.is_untitled = true;
        tab.editor_hydrated = true;
        tab.mode = DetachedViewMode::Edit;
        tab
    }

    pub(crate) fn is_editable(&self) -> bool {
        (self.file_path.is_some() && self.doc_id.is_some()) || self.is_untitled
    }

    pub(crate) fn current_snapshot(&self) -> EditorSnapshot {
        let (line, col) = self.editor_content.cursor_position();
        EditorSnapshot {
            text: editor_buffer_text(&self.editor_content),
            line,
            col,
        }
    }

    pub(crate) fn record_undo_snapshot(&mut self, coalesce_typing: bool) {
        let now = Instant::now();
        if coalesce_typing
            && let Some(prev_at) = self.last_typing_snapshot_at
            && now.duration_since(prev_at) <= Duration::from_millis(450)
        {
            self.last_typing_snapshot_at = Some(now);
            self.redo_stack.clear();
            return;
        }
        let snap = self.current_snapshot();
        if self.undo_stack.back().is_none_or(|last| last.text != snap.text) {
            if self.undo_stack.len() >= MAX_UNDO_HISTORY {
                self.undo_stack.pop_front();
            }
            self.undo_stack.push_back(snap);
        }
        self.redo_stack.clear();
        self.last_typing_snapshot_at = if coalesce_typing { Some(now) } else { None };
    }

    pub(crate) fn recompute_dirty_flag(&mut self) {
        if self.is_untitled && self.file_path.is_none() {
            self.dirty = !editor_buffer_text(&self.editor_content).is_empty();
            return;
        }
        if let Some(disk_text) = &self.raw_text {
            let formatted = format_editor_text_for_save(&self.editor_content, self.trailing_newline);
            self.dirty = formatted != *disk_text;
        }
        self.refresh_git_line_marks();
    }

    pub(crate) fn indent_unit_str(&self) -> String {
        if self.indent_use_tabs {
            "\t".to_string()
        } else {
            " ".repeat(self.indent_spaces.max(1))
        }
    }

    pub(crate) fn refresh_git_line_marks(&mut self) {
        let marks = if self.dirty {
            if let Some(base) = &self.raw_text
                && self.editor_hydrated
            {
                compute_line_git_marks_vs_base(base, &editor_buffer_text(&self.editor_content))
            } else {
                HashMap::new()
            }
        } else if !self.diff_parsed.blocks.is_empty() {
            compute_line_git_marks_from_parsed(&self.diff_parsed)
        } else {
            HashMap::new()
        };
        self.git_line_marks = Arc::new(marks);
    }

    pub(crate) fn refresh_live_split_preview(&mut self) {
        let live_text = if self.editor_hydrated {
            Some(editor_buffer_text(&self.editor_content))
        } else {
            self.raw_text.clone()
        };
        let virtual_path = match self.file_path.clone() {
            Some(p) => p,
            None => PathBuf::from(match self.language {
                CodeLanguage::Markdown => "untitled.md",
                CodeLanguage::Json => "untitled.json",
                CodeLanguage::Html => "untitled.html",
                _ => "untitled.txt",
            }),
        };
        self.preview_state = PreviewState::from_file(&virtual_path, live_text.as_deref());
        self.sync_preview_editor();
        self.preview_hydrated = true;
    }

    pub(crate) fn has_preview_available(&self) -> bool {
        self.file_path.is_some()
            || self.is_untitled
            || !matches!(self.preview_state, PreviewState::Empty)
    }

    pub(crate) fn rebuild_diff_cache(&mut self, theme_mode: ThemeMode) {
        self.diff_cache = build_diff_layout_cache(
            &self.diff_parsed,
            self.diff_compact,
            &self.diff_expanded_gaps,
            self.language,
            theme_mode,
        );
    }

    pub(crate) fn sync_preview_editor(&mut self) {
        // Read-only Preview renders via virtualized `HighlightedPreviewDoc` (zero `cosmic_text::Editor` allocation).
        self.preview_editor = text_editor::Content::new();
    }

    /// Lazily shapes `editor_content` only when entering Code/Edit view (avoids redundant cosmic_text work).
    pub(crate) fn ensure_editor_hydrated(&mut self) {
        if self.editor_hydrated {
            return;
        }
        if let Some(raw) = &self.raw_text {
            self.editor_content = text_editor::Content::with_text(raw);
        } else {
            self.editor_content = text_editor::Content::new();
        }
        self.editor_hydrated = true;
    }

    /// Lazily parses `PreviewState` and shapes `preview_editor` only when viewing Preview mode.
    pub(crate) fn ensure_preview_hydrated(&mut self) {
        if self.preview_hydrated {
            return;
        }
        if let Some(path) = self.file_path.as_deref() {
            let live_text = if self.editor_hydrated && self.dirty {
                Some(editor_buffer_text(&self.editor_content))
            } else {
                self.raw_text.clone()
            };
            self.preview_state = PreviewState::from_file(path, live_text.as_deref());
            self.sync_preview_editor();
        } else if self.is_untitled {
            self.refresh_live_split_preview();
            return;
        }
        self.preview_hydrated = true;
    }

    pub(crate) fn hydrate_active_mode(&mut self) {
        match self.mode {
            DetachedViewMode::Edit => self.ensure_editor_hydrated(),
            DetachedViewMode::Preview => self.ensure_preview_hydrated(),
            _ => {}
        }
    }

    pub(crate) fn current_searchable_text(&self) -> Option<String> {
        if self.is_editable() {
            if self.editor_hydrated {
                Some(editor_buffer_text(&self.editor_content))
            } else {
                self.raw_text.clone()
            }
        } else {
            self.preview_state.copyable_text().map(str::to_string)
        }
    }

    /// Applies an asynchronously loaded `OpenDocumentCapability` to this tab and hydrates ONLY
    /// the active view mode (`Edit` or `Preview`), avoiding double `cosmic_text` shaping.
    pub(crate) fn apply_loaded_doc(
        &mut self,
        path: &Path,
        root: Option<&Path>,
        doc: Option<OpenDocumentCapability>,
    ) -> bool {
        self.title = file_display_name(path);
        self.rel_path = root
            .and_then(|r| path.strip_prefix(r).ok())
            .map(|p| p.to_string_lossy().replace('\\', "/"));
        self.language = CodeLanguage::from_path(path);
        self.indent_spaces = self.language.default_indent_spaces();
        self.indent_use_tabs = false;
        self.file_path = Some(path.to_path_buf());
        self.is_untitled = false;
        self.loading_file = false;
        self.dirty = false;
        self.disk_conflict = false;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.last_typing_snapshot_at = None;
        self.editor_hydrated = false;
        self.preview_hydrated = false;

        match doc {
            Some(cap) => {
                self.doc_id = Some(cap.document_id);
                self.doc_version = Some(cap.version);
                self.encoding = cap.encoding;
                self.line_ending = cap.line_ending;
                self.trailing_newline = cap.content.ends_with('\n');
                self.raw_text = Some(cap.content);
                self.preview_state = PreviewState::Empty;
                self.preview_editor = text_editor::Content::new();
                self.refresh_git_line_marks();
                true
            }
            None => {
                self.doc_id = None;
                self.doc_version = None;
                self.raw_text = None;
                self.editor_content = text_editor::Content::new();
                self.editor_hydrated = true;
                false
            }
        }
    }

    pub(crate) fn load_local_file(
        &mut self,
        docs: &gitfleet_docs::DocsService,
        path: &Path,
        root: Option<&Path>,
    ) -> bool {
        if !path.is_file() {
            return false;
        }
        let doc = docs.open_document(path).ok();
        let editable = self.apply_loaded_doc(path, root, doc);
        self.hydrate_active_mode();
        editable
    }

    pub(crate) fn reset_scroll_state(&mut self) {
        self.diff_expanded_gaps.clear();
        self.diff_selection = None;
        self.diff_left_scroll_y = 0.0;
        self.diff_unified_scroll_y = 0.0;
        self.diff_left_scroll_x = 0.0;
        self.diff_active_change = 0;
        self.editor_scroll.reset_offsets();
        self.preview_scroll.reset_offsets();
    }

    pub(crate) fn reset_scroll_task(&self) -> Task<Message> {
        Task::batch([
            scrollable::scroll_to(
                self.diff_left_scroll_id.clone(),
                scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
            ),
            scrollable::scroll_to(
                self.diff_unified_scroll_id.clone(),
                scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
            ),
            scrollable::scroll_to(
                self.editor_scroll.id.clone(),
                scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
            ),
            scrollable::scroll_to(
                self.preview_scroll.id.clone(),
                scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
            ),
        ])
    }
}

// ── GitFleet Studio Window State ─────────────────────────────────────────────

pub(crate) struct DetachedViewerState {
    pub(crate) tabs: Vec<StudioTab>,
    pub(crate) active_tab: Option<usize>,
    pub(crate) next_tab_id: u64,
    pub(crate) next_untitled_num: usize,
    pub(crate) pending_close_tab: Option<u64>,

    // Quick Open (Ctrl+P) & Go to Line (Ctrl+G)
    pub(crate) quick_open_visible: bool,
    pub(crate) quick_open_query: String,
    pub(crate) quick_open_selected: usize,
    pub(crate) quick_open_input_id: text_input::Id,
    pub(crate) goto_line_visible: bool,
    pub(crate) goto_line_input: String,
    pub(crate) goto_line_input_id: text_input::Id,

    pub(crate) request_id: u64,
    pub(crate) loading: bool,
    pub(crate) status_note: String,
    pub(crate) status_is_error: bool,

    // Deferred open tasks
    pub(crate) initial_diff: Option<(u64, DetachedGitDiffTarget)>,
    pub(crate) initial_file_load: Option<(u64, PathBuf)>,
    pub(crate) focus_search_on_open: bool,

    // Editor Preferences (shared across all tabs in this window)
    pub(crate) editor_word_wrap: bool,
    pub(crate) editor_font_size: f32,

    // Left Activity Bar + Resizable Sidebar + Split Live Preview
    pub(crate) sidebar_open: bool,
    pub(crate) sidebar_width: f32,
    pub(crate) resizing_sidebar: bool,
    pub(crate) split_right_preview: bool,
    pub(crate) split_ratio: f32,
    pub(crate) resizing_split: bool,
    pub(crate) window_width: f32,
    pub(crate) closed_tabs: VecDeque<PathBuf>,
    pub(crate) tab_menu: Option<StudioTabMenu>,
    pub(crate) breadcrumb_menu: Option<StudioBreadcrumbMenu>,
    pub(crate) sidebar_panel: StudioSidebarPanel,
    pub(crate) search_root: Option<PathBuf>,
    pub(crate) search_root_name: Option<String>,

    // Explorer Panel
    pub(crate) explorer_files: Vec<String>,
    pub(crate) explorer_git_statuses: HashMap<String, char>,
    pub(crate) explorer_dirty_dirs: HashSet<String>,
    pub(crate) explorer_ignored_files: Vec<String>,
    pub(crate) explorer_ignored_dirs: Vec<String>,
    pub(crate) explorer_ignored_dir_children: HashMap<String, IgnoredDirListing>,
    pub(crate) explorer_tree: ExplorerDirNode,
    pub(crate) explorer_show_ignored: bool,
    pub(crate) explorer_loading: bool,
    pub(crate) explorer_request_id: u64,
    pub(crate) explorer_filter: String,
    pub(crate) explorer_filter_id: text_input::Id,
    pub(crate) explorer_expanded_dirs: HashSet<String>,
    pub(crate) explorer_menu: Option<StudioExplorerMenu>,
    pub(crate) pending_delete: Option<StudioPendingDelete>,
    pub(crate) create_modal: Option<StudioCreateEntryModal>,
    pub(crate) create_input_id: text_input::Id,
    pub(crate) rename_modal: Option<RenameEntryModalState>,
    pub(crate) rename_input_id: text_input::Id,
    pub(crate) gitignore_modal: Option<AddGitignoreModalState>,
    pub(crate) gitignore_input_id: text_input::Id,

    // Compare Panel (persisted left/right handles)
    pub(crate) compare_left: Option<PathBuf>,
    pub(crate) compare_right: Option<PathBuf>,

    // In-File Find & Replace (operates on active tab)
    pub(crate) in_file_find_open: bool,
    pub(crate) in_file_show_replace: bool,
    pub(crate) in_file_query: String,
    pub(crate) in_file_replace: String,
    pub(crate) in_file_options: SearchOptions,
    pub(crate) in_file_matches: Vec<InFileMatch>,
    pub(crate) in_file_active_idx: Option<usize>,
    pub(crate) in_file_error: Option<String>,
    pub(crate) in_file_input_id: text_input::Id,
    pub(crate) in_file_replace_input_id: text_input::Id,

    // VS Code-style Search & Replace Sidebar
    pub(crate) search_query: String,
    pub(crate) search_replace_text: String,
    pub(crate) search_include: String,
    pub(crate) search_exclude: String,
    pub(crate) search_options: SearchOptions,
    pub(crate) search_use_gitignore: bool,
    pub(crate) search_show_replace: bool,
    pub(crate) search_show_filters: bool,
    pub(crate) search_running: bool,
    pub(crate) search_request_id: u64,
    pub(crate) search_has_run: bool,
    pub(crate) search_result: ScopedSearchResult,
    pub(crate) search_collapsed_groups: HashSet<String>,
    pub(crate) search_files_collapsed: bool,
    pub(crate) search_confirm_replace_all: bool,
    pub(crate) search_input_id: text_input::Id,

    // Anchored Status Bar Menus (EOL, Encoding, Syntax Highlight)
    pub(crate) cursor_pos: Point,
    pub(crate) status_menu: Option<(StudioStatusMenu, Point)>,
    pub(crate) encoding_menu_mode: EncodingModalMode,
    pub(crate) syntax_filter: String,
    pub(crate) syntax_filter_id: text_input::Id,
}

impl DetachedViewerState {
    pub(crate) fn empty() -> Self {
        Self {
            tabs: Vec::new(),
            active_tab: None,
            next_tab_id: 1,
            next_untitled_num: 1,
            pending_close_tab: None,
            quick_open_visible: false,
            quick_open_query: String::new(),
            quick_open_selected: 0,
            quick_open_input_id: text_input::Id::unique(),
            goto_line_visible: false,
            goto_line_input: String::new(),
            goto_line_input_id: text_input::Id::unique(),
            request_id: 0,
            loading: false,
            status_note: "Ready".into(),
            status_is_error: false,
            initial_diff: None,
            initial_file_load: None,
            focus_search_on_open: false,
            editor_word_wrap: false,
            editor_font_size: 13.5,
            sidebar_open: true,
            sidebar_width: 264.0,
            resizing_sidebar: false,
            split_right_preview: false,
            split_ratio: 0.50,
            resizing_split: false,
            window_width: 1180.0,
            closed_tabs: VecDeque::new(),
            tab_menu: None,
            breadcrumb_menu: None,
            sidebar_panel: StudioSidebarPanel::Explorer,
            search_root: None,
            search_root_name: None,
            explorer_files: Vec::new(),
            explorer_git_statuses: HashMap::new(),
            explorer_dirty_dirs: HashSet::new(),
            explorer_ignored_files: Vec::new(),
            explorer_ignored_dirs: Vec::new(),
            explorer_ignored_dir_children: HashMap::new(),
            explorer_tree: ExplorerDirNode::default(),
            explorer_show_ignored: true,
            explorer_loading: false,
            explorer_request_id: 0,
            explorer_filter: String::new(),
            explorer_filter_id: text_input::Id::unique(),
            explorer_expanded_dirs: HashSet::new(),
            explorer_menu: None,
            pending_delete: None,
            create_modal: None,
            create_input_id: text_input::Id::unique(),
            rename_modal: None,
            rename_input_id: text_input::Id::unique(),
            gitignore_modal: None,
            gitignore_input_id: text_input::Id::unique(),
            compare_left: None,
            compare_right: None,
            in_file_find_open: false,
            in_file_show_replace: false,
            in_file_query: String::new(),
            in_file_replace: String::new(),
            in_file_options: SearchOptions::default(),
            in_file_matches: Vec::new(),
            in_file_active_idx: None,
            in_file_error: None,
            in_file_input_id: text_input::Id::unique(),
            in_file_replace_input_id: text_input::Id::unique(),
            search_query: String::new(),
            search_replace_text: String::new(),
            search_include: String::new(),
            search_exclude: String::new(),
            search_options: SearchOptions::default(),
            search_use_gitignore: true,
            search_show_replace: true,
            search_show_filters: true,
            search_running: false,
            search_request_id: 0,
            search_has_run: false,
            search_result: ScopedSearchResult::default(),
            search_collapsed_groups: HashSet::new(),
            search_files_collapsed: false,
            search_confirm_replace_all: false,
            search_input_id: text_input::Id::unique(),
            cursor_pos: Point::ORIGIN,
            status_menu: None,
            encoding_menu_mode: EncodingModalMode::SaveWithEncoding,
            syntax_filter: String::new(),
            syntax_filter_id: text_input::Id::unique(),
        }
    }

    pub(crate) fn active_tab(&self) -> Option<&StudioTab> {
        self.active_tab.and_then(|i| self.tabs.get(i))
    }

    pub(crate) fn active_tab_mut(&mut self) -> Option<&mut StudioTab> {
        self.active_tab.and_then(|i| self.tabs.get_mut(i))
    }

    pub(crate) fn window_title_info(&self) -> (String, bool) {
        if let Some(tab) = self.active_tab() {
            let suffix = self
                .search_root_name
                .as_deref()
                .map(|r| format!(" — {r}"))
                .unwrap_or_default();
            (format!("{}{suffix}", tab.title), tab.dirty)
        } else if let Some(name) = &self.search_root_name {
            (name.clone(), false)
        } else {
            (String::new(), false)
        }
    }

    pub(crate) fn alloc_tab_id(&mut self) -> u64 {
        let id = self.next_tab_id;
        self.next_tab_id = self.next_tab_id.wrapping_add(1).max(1);
        id
    }

    pub(crate) fn expand_ancestors_for_rel(&mut self, rel: &str) {
        let mut prefix = String::new();
        let parts: Vec<&str> = rel.split('/').collect();
        if parts.len() <= 1 {
            return;
        }
        for seg in &parts[..parts.len() - 1] {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(seg);
            self.explorer_expanded_dirs.insert(prefix.clone());
        }
    }

    pub(crate) fn is_explorer_path_in_ignored_dir(&self, path: &str) -> bool {
        if path.is_empty() {
            return false;
        }
        self.explorer_ignored_dirs.iter().any(|d| {
            path == d
                || path
                    .strip_prefix(d.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    }

    pub(crate) fn clear_explorer_ignored_cache(&mut self) {
        self.explorer_ignored_files.clear();
        self.explorer_ignored_dirs.clear();
        self.explorer_ignored_dir_children.clear();
    }

    pub(crate) fn rebuild_explorer_tree(&mut self) {
        self.explorer_tree = ExplorerDirNode::build(
            &self.explorer_files,
            &self.explorer_ignored_files,
            &self.explorer_ignored_dirs,
            &self.explorer_ignored_dir_children,
            self.explorer_show_ignored,
        );
    }

    pub(crate) fn close_tabs_under_path(&mut self, abs_path: &Path, is_dir: bool) {
        let mut idx = 0;
        while idx < self.tabs.len() {
            let matches = self.tabs[idx].file_path.as_deref().is_some_and(|p| {
                p == abs_path || (is_dir && p.starts_with(abs_path))
            });
            if matches {
                self.remove_tab_at(idx);
            } else {
                idx += 1;
            }
        }
    }

    pub(crate) fn rename_tabs_under_path(
        &mut self,
        docs: &gitfleet_docs::DocsService,
        old_abs: &Path,
        new_abs: &Path,
        is_dir: bool,
    ) {
        let root = self.search_root.clone();
        for tab in &mut self.tabs {
            let new_path = match tab.file_path.as_deref() {
                Some(p) if p == old_abs => Some(new_abs.to_path_buf()),
                Some(p) if is_dir => p.strip_prefix(old_abs).ok().map(|tail| new_abs.join(tail)),
                _ => None,
            };
            let Some(updated_path) = new_path else {
                continue;
            };
            tab.title = file_display_name(&updated_path);
            tab.rel_path = root
                .as_deref()
                .and_then(|r| updated_path.strip_prefix(r).ok())
                .map(|rel| rel.to_string_lossy().replace('\\', "/"));
            tab.language = CodeLanguage::from_path(&updated_path);
            tab.file_path = Some(updated_path.clone());
            // Rebind DocsService capability handle to the new path so Ctrl+S saves to the new file.
            if let Ok(cap) = docs.open_document(&updated_path) {
                tab.doc_id = Some(cap.document_id);
                tab.doc_version = Some(cap.version);
                if !tab.dirty {
                    tab.encoding = cap.encoding;
                    tab.line_ending = cap.line_ending;
                    tab.raw_text = Some(cap.content);
                }
            }
        }
    }

    pub(crate) fn from_host(app: &GitFleetApp) -> Self {
        let mut state = Self::empty();
        state.explorer_show_ignored = app.show_ignored_files;
        if let Some(repo) = app.current_repo() {
            state.search_root = Some(repo.path.clone());
            state.search_root_name = Some(repo.name.clone());
        }

        let has_content = !app.diff_parsed.raw.is_empty()
            || app.diff_image.is_some()
            || app.diff_csv.is_some()
            || !matches!(app.preview_state, PreviewState::Empty)
            || app.open_doc_path.is_some()
            || app.active_file_path.is_some();

        if !has_content {
            state.sidebar_open = true;
            state.sidebar_panel = StudioSidebarPanel::Explorer;
            return state;
        }

        let tab_id = state.alloc_tab_id();
        let mut tab = StudioTab::new_empty(tab_id, "Untitled".into());
        tab.last_text_diff_mode = match app.last_text_diff_mode {
            BottomViewMode::UnifiedDiff => DetachedViewMode::UnifiedDiff,
            _ => DetachedViewMode::SideBySideDiff,
        };
        tab.diff_parsed = app.diff_parsed.clone();
        tab.diff_cache = Arc::clone(&app.diff_cache);
        tab.diff_compact = app.diff_compact;
        tab.diff_expanded_gaps = app.diff_expanded_gaps.clone();
        tab.diff_image = app.diff_image.clone();
        tab.image_state = ImageDiffViewState {
            mode: app.image_diff_mode,
            zoom: app.image_diff_zoom,
            swapped: app.image_diff_swapped,
            swipe: app.image_diff_swipe,
            onion: app.image_diff_onion,
            blink_after: false,
            blink_paused: app.image_diff_blink_paused,
        };
        tab.diff_csv = app.diff_csv.clone();
        tab.csv_changes_only = app.csv_diff_changes_only;
        tab.csv_ignore_format = app.csv_diff_ignore_format;
        tab.csv_filter = app.csv_diff_filter.clone();
        tab.preview_raw_mode = app.preview_raw_mode;

        let local_path = app.open_doc_path.clone().or_else(|| {
            app.current_repo().and_then(|repo| {
                app.active_file_path
                    .as_deref()
                    .map(|rel| repo.path.join(rel))
                    .filter(|p| p.is_file())
            })
        });

        let mut can_edit = false;
        if let Some(p) = local_path {
            tab.title = file_display_name(&p);
            tab.rel_path = state
                .search_root
                .as_deref()
                .and_then(|r| p.strip_prefix(r).ok())
                .map(|rel| rel.to_string_lossy().replace('\\', "/"));
            tab.language = CodeLanguage::from_path(&p);
            tab.file_path = Some(p.clone());

            // Reuse Host's already-loaded document when available (zero redundant disk I/O!)
            if app.open_doc_path.as_ref() == Some(&p)
                && let (Some(doc_id), Some(ver), Some(raw)) = (
                    app.open_doc_id.clone(),
                    app.open_doc_version.clone(),
                    app.open_doc_raw_text.clone(),
                )
            {
                tab.doc_id = Some(doc_id);
                tab.doc_version = Some(ver);
                tab.encoding = app.open_doc_encoding;
                tab.line_ending = app.open_doc_line_ending;
                tab.trailing_newline = raw.ends_with('\n');
                tab.raw_text = Some(raw);
                can_edit = true;
            } else {
                tab.loading_file = true;
                state.initial_file_load = Some((tab_id, p));
            }
        }

        if tab.title == "Untitled" {
            tab.title = if app.diff_title.is_empty()
                || app.diff_title == "No file selected"
                || app.diff_title == "Select a file to view changes"
            {
                app.active_file_path
                    .as_deref()
                    .map(|s| s.rsplit('/').next().unwrap_or(s).to_string())
                    .unwrap_or_else(|| "Untitled".into())
            } else {
                app.diff_title.clone()
            };
        }
        if tab.rel_path.is_none() {
            tab.rel_path = app.active_file_path.clone();
        }
        if let Some(rel) = tab.rel_path.clone() {
            state.expand_ancestors_for_rel(&rel);
        }
        if app.active_file_path.is_some() || app.open_doc_id.is_some() {
            tab.language = app.open_doc_language;
        }

        tab.mode = match app.bottom_mode {
            BottomViewMode::Edit if can_edit || tab.loading_file => DetachedViewMode::Edit,
            BottomViewMode::Preview => DetachedViewMode::Preview,
            BottomViewMode::UnifiedDiff if !tab.diff_parsed.raw.is_empty() => {
                DetachedViewMode::UnifiedDiff
            }
            BottomViewMode::SideBySideDiff if !tab.diff_parsed.raw.is_empty() => {
                DetachedViewMode::SideBySideDiff
            }
            BottomViewMode::ImageDiff if tab.diff_image.is_some() => DetachedViewMode::ImageDiff,
            BottomViewMode::CsvDiff if tab.diff_csv.is_some() => DetachedViewMode::CsvDiff,
            _ => {
                if tab.diff_image.is_some() {
                    DetachedViewMode::ImageDiff
                } else if tab.diff_csv.is_some() {
                    DetachedViewMode::CsvDiff
                } else if !tab.diff_parsed.raw.is_empty() {
                    tab.last_text_diff_mode
                } else if can_edit || tab.loading_file {
                    DetachedViewMode::Edit
                } else {
                    DetachedViewMode::Preview
                }
            }
        };

        // Hydrate ONLY the active mode (avoids 3x cosmic_text shaping when opening Studio!)
        if tab.mode == DetachedViewMode::Preview
            && tab.file_path.is_none()
            && !matches!(app.preview_state, PreviewState::Empty)
        {
            tab.preview_state = app.preview_state.clone_snapshot();
            tab.sync_preview_editor();
            tab.preview_hydrated = true;
        } else if !tab.loading_file {
            tab.hydrate_active_mode();
        }

        if let (Some(repo), Some(file)) = (app.current_repo(), app.selected_changed_file.clone())
            && app.active_file_path.as_deref() == Some(file.path.as_str())
        {
            tab.git_diff_target = match &app.graph_selection {
                GraphSelection::WorkingTree => Some(DetachedGitDiffTarget::WorkingFile {
                    repo_path: repo.path,
                    file,
                }),
                GraphSelection::Commit(oid) => Some(DetachedGitDiffTarget::CommitFile {
                    repo_path: repo.path,
                    oid: oid.clone(),
                    rel_path: file.path,
                }),
                GraphSelection::Range { .. } | GraphSelection::Stash(_) => None,
            };
        }

        state.tabs.push(tab);
        state.active_tab = Some(0);
        state.sidebar_open = false;
        state.status_note = "Ready".into();
        state
    }

    pub(crate) fn for_repo_file(
        app: &GitFleetApp,
        rel_path: &str,
        changed_file: Option<FileChange>,
        prefer_edit: bool,
    ) -> Self {
        let mut state = Self::empty();
        state.explorer_show_ignored = app.show_ignored_files;
        state.sidebar_open = false;
        state.sidebar_panel = StudioSidebarPanel::Explorer;

        let Some(repo) = app.current_repo() else {
            return state;
        };
        state.search_root = Some(repo.path.clone());
        state.search_root_name = Some(repo.name.clone());
        state.expand_ancestors_for_rel(rel_path);

        let tab_id = state.alloc_tab_id();
        let mut tab = StudioTab::new_empty(
            tab_id,
            rel_path.rsplit('/').next().unwrap_or(rel_path).to_string(),
        );
        tab.rel_path = Some(rel_path.to_string());
        tab.last_text_diff_mode = match app.last_text_diff_mode {
            BottomViewMode::UnifiedDiff => DetachedViewMode::UnifiedDiff,
            _ => DetachedViewMode::SideBySideDiff,
        };

        let full_path = repo.path.join(rel_path);
        tab.language = CodeLanguage::from_path(&full_path);
        let mut can_edit = false;
        if full_path.is_file() {
            tab.file_path = Some(full_path.clone());
            // Reuse Host's already-decoded document if it matches, otherwise load asynchronously on open
            if app.open_doc_path.as_ref() == Some(&full_path)
                && let (Some(doc_id), Some(ver), Some(raw)) = (
                    app.open_doc_id.clone(),
                    app.open_doc_version.clone(),
                    app.open_doc_raw_text.clone(),
                )
            {
                tab.doc_id = Some(doc_id);
                tab.doc_version = Some(ver);
                tab.encoding = app.open_doc_encoding;
                tab.line_ending = app.open_doc_line_ending;
                tab.trailing_newline = raw.ends_with('\n');
                tab.raw_text = Some(raw);
                can_edit = true;
            } else {
                tab.loading_file = true;
                state.initial_file_load = Some((tab_id, full_path));
            }
        }

        if app.active_file_path.as_deref() == Some(rel_path) {
            tab.language = app.open_doc_language;
            if app.diff_parsed_path.as_deref() == Some(rel_path) {
                tab.diff_parsed = app.diff_parsed.clone();
                tab.diff_cache = Arc::clone(&app.diff_cache);
                tab.diff_compact = app.diff_compact;
                tab.diff_expanded_gaps = app.diff_expanded_gaps.clone();
            }
            tab.diff_image = app.diff_image.clone();
            tab.diff_csv = app.diff_csv.clone();
            tab.image_state = ImageDiffViewState {
                mode: app.image_diff_mode,
                zoom: app.image_diff_zoom,
                swapped: app.image_diff_swapped,
                swipe: app.image_diff_swipe,
                onion: app.image_diff_onion,
                blink_after: false,
                blink_paused: app.image_diff_blink_paused,
            };
        }

        let effective_change = changed_file.or_else(|| {
            app.active_status().and_then(|st| {
                st.unstaged_files
                    .iter()
                    .chain(st.staged_files.iter())
                    .find(|f| f.path == rel_path)
                    .cloned()
            })
        });

        let diff_target = match (&app.graph_selection, &effective_change) {
            (GraphSelection::Commit(oid), Some(f)) => Some(DetachedGitDiffTarget::CommitFile {
                repo_path: repo.path.clone(),
                oid: oid.clone(),
                rel_path: f.path.clone(),
            }),
            (GraphSelection::WorkingTree, Some(f)) => Some(DetachedGitDiffTarget::WorkingFile {
                repo_path: repo.path.clone(),
                file: f.clone(),
            }),
            _ => None,
        };

        tab.git_diff_target = diff_target.clone();
        let needs_diff_fetch = diff_target.is_some()
            && tab.diff_parsed.raw.is_empty()
            && tab.diff_image.is_none()
            && tab.diff_csv.is_none();
        if needs_diff_fetch && let Some(target) = diff_target {
            state.initial_diff = Some((tab_id, target));
        }

        let is_img = is_image_diff_path(rel_path);
        let is_csv = is_csv_diff_path(rel_path);
        let has_diff = !tab.diff_parsed.raw.is_empty() || state.initial_diff.is_some();

        tab.mode = if prefer_edit {
            if can_edit || tab.loading_file {
                DetachedViewMode::Edit
            } else {
                DetachedViewMode::Preview
            }
        } else {
            match app.bottom_mode {
                BottomViewMode::Edit if can_edit || tab.loading_file => DetachedViewMode::Edit,
                BottomViewMode::Preview => DetachedViewMode::Preview,
                BottomViewMode::ImageDiff
                    if is_img && (tab.diff_image.is_some() || state.initial_diff.is_some()) =>
                {
                    DetachedViewMode::ImageDiff
                }
                BottomViewMode::CsvDiff
                    if is_csv && (tab.diff_csv.is_some() || state.initial_diff.is_some()) =>
                {
                    DetachedViewMode::CsvDiff
                }
                BottomViewMode::UnifiedDiff if has_diff => DetachedViewMode::UnifiedDiff,
                BottomViewMode::SideBySideDiff if has_diff => DetachedViewMode::SideBySideDiff,
                _ => {
                    if can_edit || tab.loading_file {
                        DetachedViewMode::Edit
                    } else if has_diff {
                        tab.last_text_diff_mode
                    } else {
                        DetachedViewMode::Preview
                    }
                }
            }
        };

        if !tab.loading_file {
            tab.hydrate_active_mode();
        }

        state.tabs.push(tab);
        state.active_tab = Some(0);
        state.status_note = if state.initial_file_load.is_some() {
            format!("Opening {rel_path}…")
        } else {
            format!("Opened {rel_path}")
        };
        state
    }

    pub(crate) fn for_search(app: &GitFleetApp, initial_query: Option<String>) -> Self {
        let mut state = Self::from_host(app);
        state.sidebar_open = true;
        state.sidebar_panel = StudioSidebarPanel::Search;
        state.search_show_replace = true;
        state.search_show_filters = true;
        state.focus_search_on_open = true;
        if let Some(q) = initial_query {
            state.search_query = q;
        }
        state.status_note = match &state.search_root_name {
            Some(name) => format!("Search in {name}"),
            None => "Select a folder to search".into(),
        };
        state
    }

    pub(crate) fn needs_blink_timer(&self) -> bool {
        self.active_tab().is_some_and(|t| {
            t.mode == DetachedViewMode::ImageDiff
                && t.image_state.mode == ImageDiffMode::Blink
                && !t.image_state.blink_paused
                && t.diff_image.is_some()
        })
    }

    pub(crate) fn recompute_in_file_matches(&mut self, preserve_active: bool) {
        if !self.in_file_find_open || self.in_file_query.is_empty() {
            self.in_file_matches.clear();
            self.in_file_active_idx = None;
            self.in_file_error = None;
            return;
        }
        let Some(buf) = self.active_tab().and_then(StudioTab::current_searchable_text) else {
            self.in_file_matches.clear();
            self.in_file_active_idx = None;
            self.in_file_error = None;
            return;
        };

        let prev_idx = self.in_file_active_idx;
        match find_in_text(&buf, &self.in_file_query, self.in_file_options) {
            Ok(list) => {
                self.in_file_error = None;
                self.in_file_active_idx = if list.is_empty() {
                    None
                } else if preserve_active {
                    Some(prev_idx.unwrap_or(0).min(list.len() - 1))
                } else {
                    Some(0)
                };
                self.in_file_matches = list;
            }
            Err(err) => {
                self.in_file_matches.clear();
                self.in_file_active_idx = None;
                self.in_file_error = Some(err);
            }
        }
    }

    pub(crate) fn jump_active_editor_to_match(&mut self) -> Task<Message> {
        let Some(m) = self
            .in_file_active_idx
            .and_then(|i| self.in_file_matches.get(i))
            .cloned()
        else {
            return Task::none();
        };
        let font_size = self.editor_font_size;
        let word_wrap = self.editor_word_wrap;
        if let Some(tab) = self.active_tab_mut() {
            if tab.mode == DetachedViewMode::Edit && tab.is_editable() {
                jump_editor_to_span(
                    &mut tab.editor_content,
                    m.line_index,
                    m.col_start,
                    m.col_end,
                );
                return crate::views::code_editor::scroll_numbered_editor_to_line(
                    &tab.editor_content,
                    &mut tab.editor_scroll,
                    m.line_index,
                    crate::views::code_editor::NumberedEditorConfig::studio(font_size, word_wrap),
                );
            } else if tab.mode == DetachedViewMode::Preview
                && tab.preview_state.uses_selectable_editor(tab.preview_raw_mode)
            {
                jump_editor_to_span(
                    &mut tab.preview_editor,
                    m.line_index,
                    m.col_start,
                    m.col_end,
                );
                return crate::views::code_editor::scroll_numbered_editor_to_line(
                    &tab.preview_editor,
                    &mut tab.preview_scroll,
                    m.line_index,
                    crate::views::code_editor::NumberedEditorConfig::preview(font_size, word_wrap),
                );
            }
        }
        Task::none()
    }

    pub(crate) fn remove_tab_at(&mut self, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }
        let removed = self.tabs.remove(idx);
        if let Some(p) = removed.file_path {
            self.closed_tabs.retain(|x| x != &p);
            if self.closed_tabs.len() >= 20 {
                self.closed_tabs.pop_front();
            }
            self.closed_tabs.push_back(p);
        }
        if self.pending_close_tab == Some(removed.id) {
            self.pending_close_tab = None;
        }
        self.tab_menu = None;
        self.active_tab = if self.tabs.is_empty() {
            None
        } else {
            let cur = self.active_tab.unwrap_or(0);
            if idx < cur {
                Some(cur.saturating_sub(1))
            } else {
                Some(cur.min(self.tabs.len() - 1))
            }
        };
        self.recompute_in_file_matches(false);
    }

    pub(crate) fn set_explorer_git_statuses(&mut self, map: HashMap<String, char>) {
        let mut dirty_dirs = HashSet::new();
        for rel in map.keys() {
            let mut cur = rel.as_str();
            while let Some((parent, _)) = cur.rsplit_once('/') {
                if !parent.is_empty() {
                    dirty_dirs.insert(parent.to_string());
                }
                cur = parent;
            }
        }
        self.explorer_git_statuses = map;
        self.explorer_dirty_dirs = dirty_dirs;
    }

    pub(crate) fn quick_open_symbols(&self, limit: usize) -> Vec<(DocumentSymbol, Vec<usize>)> {
        let q_raw = self.quick_open_query.trim();
        let Some(sym_q) = q_raw.strip_prefix('@') else {
            return Vec::new();
        };
        let Some(tab) = self.active_tab() else {
            return Vec::new();
        };
        let Some(text) = tab.current_searchable_text() else {
            return Vec::new();
        };
        let symbols = extract_document_symbols(&text, tab.language);
        let q = sym_q.trim();
        if q.is_empty() {
            return symbols
                .into_iter()
                .take(limit)
                .map(|s| (s, Vec::new()))
                .collect();
        }
        let mut scored: Vec<(i32, DocumentSymbol, Vec<usize>)> = symbols
            .into_iter()
            .filter_map(|sym| {
                fuzzy_match(q, &sym.name).map(|(score, pos)| (score, sym, pos))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.line_idx.cmp(&b.1.line_idx)));
        scored
            .into_iter()
            .take(limit)
            .map(|(_, sym, pos)| (sym, pos))
            .collect()
    }

    pub(crate) fn quick_open_candidates(&self, limit: usize) -> Vec<QuickOpenCandidate> {
        let q = self.quick_open_query.trim();
        if q.starts_with(':') || q.starts_with('@') {
            return Vec::new();
        }
        let open_rels: HashSet<&str> = self
            .tabs
            .iter()
            .filter_map(|t| t.rel_path.as_deref())
            .collect();

        let mut scored: Vec<(i32, QuickOpenCandidate)> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();

        // 1. Open tabs first
        for tab in &self.tabs {
            if let Some(rel) = tab.rel_path.as_deref()
                && seen.insert(rel)
            {
                if q.is_empty() {
                    scored.push((
                        10_000,
                        QuickOpenCandidate {
                            rel_path: rel.to_string(),
                            is_ignored: false,
                            is_open_tab: true,
                            positions: Vec::new(),
                        },
                    ));
                } else if let Some((score, positions)) = fuzzy_match(q, rel) {
                    scored.push((
                        score + 40,
                        QuickOpenCandidate {
                            rel_path: rel.to_string(),
                            is_ignored: false,
                            is_open_tab: true,
                            positions,
                        },
                    ));
                }
            }
        }

        // 2. Tracked workspace files
        for rel in &self.explorer_files {
            if rel.ends_with('/') || !seen.insert(rel.as_str()) {
                continue;
            }
            let is_open = open_rels.contains(rel.as_str());
            if q.is_empty() {
                if scored.len() < limit {
                    scored.push((
                        0,
                        QuickOpenCandidate {
                            rel_path: rel.clone(),
                            is_ignored: false,
                            is_open_tab: is_open,
                            positions: Vec::new(),
                        },
                    ));
                }
            } else if let Some((score, positions)) = fuzzy_match(q, rel) {
                scored.push((
                    score,
                    QuickOpenCandidate {
                        rel_path: rel.clone(),
                        is_ignored: false,
                        is_open_tab: is_open,
                        positions,
                    },
                ));
            }
        }

        // 3. Ignored files (dimmed, slightly lower priority)
        if self.explorer_show_ignored {
            for rel in &self.explorer_ignored_files {
                if !seen.insert(rel.as_str()) {
                    continue;
                }
                let is_open = open_rels.contains(rel.as_str());
                if q.is_empty() {
                    if scored.len() < limit {
                        scored.push((
                            -10,
                            QuickOpenCandidate {
                                rel_path: rel.clone(),
                                is_ignored: true,
                                is_open_tab: is_open,
                                positions: Vec::new(),
                            },
                        ));
                    }
                } else if let Some((score, positions)) = fuzzy_match(q, rel) {
                    scored.push((
                        score - 20,
                        QuickOpenCandidate {
                            rel_path: rel.clone(),
                            is_ignored: true,
                            is_open_tab: is_open,
                            positions,
                        },
                    ));
                }
            }
        }

        if !q.is_empty() {
            scored.sort_by(|a, b| {
                b.0.cmp(&a.0)
                    .then_with(|| a.1.rel_path.len().cmp(&b.1.rel_path.len()))
                    .then_with(|| a.1.rel_path.cmp(&b.1.rel_path))
            });
        }
        scored.into_iter().take(limit).map(|(_, c)| c).collect()
    }
}
