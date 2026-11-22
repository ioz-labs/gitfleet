//! Pure-Rust stateful multi-language syntax highlighter for GitFleet Studio & Editor views.
//!
//! Supports 19 programming, markup, and config languages with multi-line comment/string tracking
//! and layered in-file search match highlighting on top of semantic syntax tokens.

use crate::theme::{self, ThemeMode};
use gitfleet_search::{Regex, SearchOptions, build_search_regex};
use iced::advanced::text as adv_text;
use iced::{Color, Font, font};
use std::ops::Range;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CodeLanguage {
    Rust,
    TypeScript,
    JavaScript,
    Python,
    Go,
    C,
    Cpp,
    CSharp,
    Java,
    Kotlin,
    Html,
    Css,
    Json,
    Toml,
    Yaml,
    Markdown,
    Sql,
    Shell,
    Ini,
    #[default]
    PlainText,
}

impl CodeLanguage {
    pub const fn all() -> &'static [Self] {
        &[
            Self::Rust,
            Self::TypeScript,
            Self::JavaScript,
            Self::Python,
            Self::Go,
            Self::C,
            Self::Cpp,
            Self::CSharp,
            Self::Java,
            Self::Kotlin,
            Self::Html,
            Self::Css,
            Self::Json,
            Self::Toml,
            Self::Yaml,
            Self::Markdown,
            Self::Sql,
            Self::Shell,
            Self::Ini,
            Self::PlainText,
        ]
    }

    pub fn from_path(path: &Path) -> Self {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        match file_name.as_str() {
            "cargo.lock" => return Self::Toml,
            "dockerfile" | "makefile" | "gnumakefile" | "justfile" => return Self::Shell,
            ".gitignore" | ".gitattributes" | ".gitmodules" | ".editorconfig" | ".env" => {
                return Self::Ini;
            }
            _ => {}
        }
        if file_name.starts_with(".env.") || file_name.starts_with("dockerfile.") {
            return if file_name.starts_with(".env.") {
                Self::Ini
            } else {
                Self::Shell
            };
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        match ext.as_str() {
            "rs" => Self::Rust,
            "ts" | "tsx" | "mts" | "cts" => Self::TypeScript,
            "js" | "jsx" | "mjs" | "cjs" => Self::JavaScript,
            "py" | "pyi" | "pyw" => Self::Python,
            "go" => Self::Go,
            "c" | "h" => Self::C,
            "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" | "ino" => Self::Cpp,
            "cs" => Self::CSharp,
            "java" => Self::Java,
            "kt" | "kts" => Self::Kotlin,
            "html" | "htm" | "xml" | "svg" | "xhtml" | "vue" | "svelte" => Self::Html,
            "css" | "scss" | "sass" | "less" => Self::Css,
            "json" | "jsonc" | "json5" | "arb" => Self::Json,
            "toml" => Self::Toml,
            "yaml" | "yml" => Self::Yaml,
            "md" | "mdx" | "markdown" => Self::Markdown,
            "sql" => Self::Sql,
            "sh" | "bash" | "zsh" | "fish" | "ps1" | "psm1" | "bat" | "cmd" => Self::Shell,
            "ini" | "cfg" | "conf" | "properties" | "env" => Self::Ini,
            _ => Self::PlainText,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::TypeScript => "TypeScript",
            Self::JavaScript => "JavaScript",
            Self::Python => "Python",
            Self::Go => "Go",
            Self::C => "C",
            Self::Cpp => "C++",
            Self::CSharp => "C#",
            Self::Java => "Java",
            Self::Kotlin => "Kotlin",
            Self::Html => "HTML / XML",
            Self::Css => "CSS",
            Self::Json => "JSON",
            Self::Toml => "TOML",
            Self::Yaml => "YAML",
            Self::Markdown => "Markdown",
            Self::Sql => "SQL",
            Self::Shell => "Shell",
            Self::Ini => "Config",
            Self::PlainText => "Plain Text",
        }
    }

    pub const fn short_badge(self) -> &'static str {
        match self {
            Self::Rust => "RS",
            Self::TypeScript => "TS",
            Self::JavaScript => "JS",
            Self::Python => "PY",
            Self::Go => "GO",
            Self::C => "C",
            Self::Cpp => "C+",
            Self::CSharp => "C#",
            Self::Java => "JV",
            Self::Kotlin => "KT",
            Self::Html => "<>",
            Self::Css => "#",
            Self::Json => "{}",
            Self::Toml => "TM",
            Self::Yaml => "YM",
            Self::Markdown => "MD",
            Self::Sql => "SQ",
            Self::Shell => "$_",
            Self::Ini => "CF",
            Self::PlainText => "TX",
        }
    }

    pub const fn default_indent_spaces(self) -> usize {
        match self {
            Self::Json
            | Self::Yaml
            | Self::Html
            | Self::Css
            | Self::TypeScript
            | Self::JavaScript
            | Self::Toml => 2,
            _ => 4,
        }
    }

    /// Distinctive semantic language accent color (used for tab & file-tree file badges).
    pub fn badge_color(self, mode: ThemeMode) -> Color {
        match (self, mode) {
            (Self::Rust, ThemeMode::Dark) => Color::from_rgb(0.95, 0.56, 0.36),
            (Self::Rust, ThemeMode::Light) => Color::from_rgb(0.78, 0.32, 0.12),
            (Self::TypeScript, ThemeMode::Dark) => Color::from_rgb(0.39, 0.72, 1.00),
            (Self::TypeScript, ThemeMode::Light) => Color::from_rgb(0.10, 0.44, 0.78),
            (Self::JavaScript, ThemeMode::Dark) => Color::from_rgb(0.96, 0.84, 0.38),
            (Self::JavaScript, ThemeMode::Light) => Color::from_rgb(0.68, 0.50, 0.02),
            (Self::Python, ThemeMode::Dark) => Color::from_rgb(0.45, 0.80, 0.96),
            (Self::Python, ThemeMode::Light) => Color::from_rgb(0.14, 0.46, 0.68),
            (Self::Go, ThemeMode::Dark) => Color::from_rgb(0.35, 0.86, 0.88),
            (Self::Go, ThemeMode::Light) => Color::from_rgb(0.04, 0.53, 0.58),
            (Self::C | Self::Cpp, ThemeMode::Dark) => Color::from_rgb(0.55, 0.70, 0.98),
            (Self::C | Self::Cpp, ThemeMode::Light) => Color::from_rgb(0.20, 0.36, 0.72),
            (Self::CSharp | Self::Kotlin, ThemeMode::Dark) => Color::from_rgb(0.76, 0.62, 0.98),
            (Self::CSharp | Self::Kotlin, ThemeMode::Light) => Color::from_rgb(0.46, 0.24, 0.78),
            (Self::Java, ThemeMode::Dark) => Color::from_rgb(0.96, 0.58, 0.52),
            (Self::Java, ThemeMode::Light) => Color::from_rgb(0.76, 0.24, 0.18),
            (Self::Html, ThemeMode::Dark) => Color::from_rgb(0.96, 0.54, 0.42),
            (Self::Html, ThemeMode::Light) => Color::from_rgb(0.78, 0.28, 0.16),
            (Self::Css, ThemeMode::Dark) => Color::from_rgb(0.48, 0.78, 0.98),
            (Self::Css, ThemeMode::Light) => Color::from_rgb(0.15, 0.45, 0.74),
            (Self::Json, ThemeMode::Dark) => Color::from_rgb(0.94, 0.82, 0.48),
            (Self::Json, ThemeMode::Light) => Color::from_rgb(0.64, 0.44, 0.05),
            (Self::Toml | Self::Yaml | Self::Ini, ThemeMode::Dark) => {
                Color::from_rgb(0.58, 0.84, 0.68)
            }
            (Self::Toml | Self::Yaml | Self::Ini, ThemeMode::Light) => {
                Color::from_rgb(0.12, 0.50, 0.28)
            }
            (Self::Markdown, ThemeMode::Dark) => Color::from_rgb(0.66, 0.78, 0.90),
            (Self::Markdown, ThemeMode::Light) => Color::from_rgb(0.24, 0.38, 0.52),
            (Self::Sql, ThemeMode::Dark) => Color::from_rgb(0.92, 0.66, 0.86),
            (Self::Sql, ThemeMode::Light) => Color::from_rgb(0.66, 0.22, 0.54),
            (Self::Shell, ThemeMode::Dark) => Color::from_rgb(0.56, 0.86, 0.58),
            (Self::Shell, ThemeMode::Light) => Color::from_rgb(0.16, 0.52, 0.22),
            (Self::PlainText, _) => mode.palette().text_muted,
        }
    }
}

/// Harmonized semantic syntax token colors tuned specifically for GitFleet's Dark & Light themes.
#[derive(Debug, Clone, Copy)]
pub struct SyntaxPalette {
    pub keyword: Color,
    pub type_name: Color,
    pub function: Color,
    pub string: Color,
    pub number: Color,
    pub comment: Color,
    pub doc_comment: Color,
    pub macro_attr: Color,
    pub tag_key: Color,
    pub punctuation: Color,
    pub search_match: Color,
    pub search_active: Color,
}

impl SyntaxPalette {
    pub const fn for_theme(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self {
                keyword: Color::from_rgb(0.776, 0.584, 1.000),      // #c695ff soft violet
                type_name: Color::from_rgb(0.475, 0.753, 1.000),    // #79c0ff crisp sky
                function: Color::from_rgb(0.416, 0.831, 1.000),     // #6ad4ff electric cyan-blue
                string: Color::from_rgb(0.561, 0.839, 0.659),       // #8fd6a8 mint sage
                number: Color::from_rgb(0.949, 0.722, 0.447),       // #f2b872 warm apricot
                comment: Color::from_rgb(0.427, 0.506, 0.580),      // #6d8194 muted slate
                doc_comment: Color::from_rgb(0.525, 0.627, 0.710),  // #86a0b5 brighter doc slate
                macro_attr: Color::from_rgb(1.000, 0.588, 0.569),   // #ff9691 coral rose
                tag_key: Color::from_rgb(0.388, 0.773, 0.855),      // #63c5da ocean teal
                punctuation: Color::from_rgb(0.608, 0.690, 0.761),  // #9bb0c2 silver
                search_match: Color::from_rgb(1.000, 0.839, 0.400), // #ffd666 gold highlight
                search_active: Color::from_rgb(1.000, 0.541, 0.396), // #ff8a65 vivid coral
            },
            ThemeMode::Light => Self {
                keyword: Color::from_rgb(0.486, 0.227, 0.929),      // #7c3aed deep violet
                type_name: Color::from_rgb(0.012, 0.412, 0.631),    // #0369a1 deep ocean
                function: Color::from_rgb(0.114, 0.306, 0.847),     // #1d4ed8 cobalt
                string: Color::from_rgb(0.082, 0.502, 0.239),       // #15803d forest green
                number: Color::from_rgb(0.706, 0.325, 0.035),       // #b45309 warm ochre
                comment: Color::from_rgb(0.420, 0.486, 0.545),      // #6b7c8b slate
                doc_comment: Color::from_rgb(0.314, 0.408, 0.494),  // #50687e doc slate
                macro_attr: Color::from_rgb(0.745, 0.071, 0.235),   // #be123c crimson
                tag_key: Color::from_rgb(0.059, 0.463, 0.431),      // #0f766e deep teal
                punctuation: Color::from_rgb(0.310, 0.380, 0.455),  // #4f6174 slate punct
                search_match: Color::from_rgb(0.706, 0.325, 0.035), // #b45309 amber
                search_active: Color::from_rgb(0.863, 0.149, 0.149), // #dc2626 vivid red
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodeHighlightSettings {
    pub language: CodeLanguage,
    pub theme_mode: ThemeMode,
    pub search_query: String,
    pub search_options: SearchOptions,
    pub active_match_line: Option<usize>,
    pub active_match_cols: Option<(usize, usize)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TokenFormat {
    pub color: Color,
    pub bold: bool,
    pub italic: bool,
}

impl TokenFormat {
    const fn plain(color: Color) -> Self {
        Self {
            color,
            bold: false,
            italic: false,
        }
    }

    const fn bold(color: Color) -> Self {
        Self {
            color,
            bold: true,
            italic: false,
        }
    }

    const fn italic(color: Color) -> Self {
        Self {
            color,
            bold: false,
            italic: true,
        }
    }
}

pub fn format_code_token(tok: &TokenFormat, _theme: &iced::Theme) -> adv_text::highlighter::Format<Font> {
    let font = if tok.bold || tok.italic {
        Some(Font {
            weight: if tok.bold {
                font::Weight::Bold
            } else {
                theme::CODE_FONT.weight
            },
            style: if tok.italic {
                font::Style::Italic
            } else {
                theme::CODE_FONT.style
            },
            ..theme::CODE_FONT
        })
    } else {
        None
    };
    adv_text::highlighter::Format {
        color: Some(tok.color),
        font,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum LineState {
    #[default]
    Normal,
    BlockComment,
    HtmlComment,
    TripleString(u8),
    BacktickString,
    MarkdownFence,
}

/// Iced `Highlighter` combining stateful multi-language syntax coloring with live search highlights.
pub struct CodeSyntaxHighlighter {
    settings: CodeHighlightSettings,
    search_regex: Option<Regex>,
    line_states: Vec<LineState>,
    current_line: usize,
}

impl adv_text::Highlighter for CodeSyntaxHighlighter {
    type Settings = CodeHighlightSettings;
    type Highlight = TokenFormat;
    type Iterator<'a>
        = std::vec::IntoIter<(Range<usize>, TokenFormat)>
    where
        Self: 'a;

    fn new(settings: &Self::Settings) -> Self {
        let search_regex = compile_optional_search(&settings.search_query, settings.search_options);
        Self {
            settings: settings.clone(),
            search_regex,
            line_states: Vec::new(),
            current_line: 0,
        }
    }

    fn update(&mut self, new_settings: &Self::Settings) {
        if self.settings != *new_settings {
            let need_relex = self.settings.language != new_settings.language
                || self.settings.theme_mode != new_settings.theme_mode;
            self.settings = new_settings.clone();
            self.search_regex =
                compile_optional_search(&self.settings.search_query, self.settings.search_options);
            if need_relex {
                self.line_states.clear();
            }
            self.current_line = 0;
        }
    }

    fn change_line(&mut self, line: usize) {
        if line < self.current_line {
            self.current_line = line;
        }
        if line < self.line_states.len() {
            self.line_states.truncate(line);
        }
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        let line_idx = self.current_line;
        self.current_line = self.current_line.saturating_add(1);

        let entry_state = if line_idx == 0 {
            LineState::Normal
        } else {
            self.line_states
                .get(line_idx - 1)
                .copied()
                .unwrap_or(LineState::Normal)
        };

        let sp = SyntaxPalette::for_theme(self.settings.theme_mode);
        let (syntax_spans, exit_state) = lex_line(line, self.settings.language, entry_state, sp);

        if line_idx < self.line_states.len() {
            self.line_states[line_idx] = exit_state;
        } else {
            while self.line_states.len() < line_idx {
                self.line_states.push(LineState::Normal);
            }
            self.line_states.push(exit_state);
        }

        let final_spans = if let Some(re) = &self.search_regex {
            let is_active_line = self.settings.active_match_line == Some(line_idx);
            let mut match_spans = Vec::new();
            for m in re.find_iter(line) {
                if m.start() == m.end() {
                    continue;
                }
                let is_active = if is_active_line
                    && let Some((cs, ce)) = self.settings.active_match_cols
                {
                    let s_ch = line[..m.start()].chars().count();
                    let e_ch = line[..m.end()].chars().count();
                    s_ch == cs && e_ch == ce
                } else {
                    false
                };
                match_spans.push((
                    m.start()..m.end(),
                    TokenFormat::bold(if is_active {
                        sp.search_active
                    } else {
                        sp.search_match
                    }),
                ));
            }
            if match_spans.is_empty() {
                syntax_spans
            } else {
                overlay_spans(syntax_spans, match_spans)
            }
        } else {
            syntax_spans
        };

        final_spans.into_iter()
    }

    fn current_line(&self) -> usize {
        self.current_line
    }
}

fn compile_optional_search(query: &str, opts: SearchOptions) -> Option<Regex> {
    if query.is_empty() {
        None
    } else {
        build_search_regex(query, opts).ok()
    }
}

/// Overlays higher-priority `overlay` ranges on top of sorted non-overlapping `base` ranges,
/// splitting any overlapping base ranges cleanly so the result remains strictly sorted and disjoint.
pub(crate) fn overlay_spans(
    base: Vec<(Range<usize>, TokenFormat)>,
    overlay: Vec<(Range<usize>, TokenFormat)>,
) -> Vec<(Range<usize>, TokenFormat)> {
    let mut out = Vec::with_capacity(base.len() + overlay.len() * 2);
    let mut ov_idx = 0usize;

    for (b_range, b_fmt) in base {
        let mut cursor = b_range.start;
        while ov_idx < overlay.len() && overlay[ov_idx].0.end <= cursor {
            out.push(overlay[ov_idx].clone());
            ov_idx += 1;
        }
        while ov_idx < overlay.len() && overlay[ov_idx].0.start < b_range.end {
            let (ref o_range, o_fmt) = overlay[ov_idx];
            if o_range.start > cursor {
                out.push((cursor..o_range.start, b_fmt));
            }
            out.push((o_range.clone(), o_fmt));
            cursor = o_range.end;
            ov_idx += 1;
            if cursor >= b_range.end {
                break;
            }
        }
        if cursor < b_range.end {
            out.push((cursor..b_range.end, b_fmt));
        }
    }

    while ov_idx < overlay.len() {
        out.push(overlay[ov_idx].clone());
        ov_idx += 1;
    }

    out.sort_by_key(|(r, _)| r.start);
    out
}

/// Lightweight stateful line lexer for Diff panels (Unified & Side-by-Side).
/// Call [`reset`](Self::reset) across collapsed/hidden gaps so multi-line comments don't leak.
pub struct DiffSyntaxHighlighter {
    lang: CodeLanguage,
    palette: SyntaxPalette,
    state: LineState,
}

impl DiffSyntaxHighlighter {
    pub fn new(lang: CodeLanguage, theme_mode: ThemeMode) -> Self {
        Self {
            lang,
            palette: SyntaxPalette::for_theme(theme_mode),
            state: LineState::Normal,
        }
    }

    pub fn reset(&mut self) {
        self.state = LineState::Normal;
    }

    pub fn highlight_line(&mut self, line: &str) -> Vec<(Range<usize>, TokenFormat)> {
        if self.lang == CodeLanguage::PlainText || line.is_empty() {
            return Vec::new();
        }
        let (spans, next_state) = lex_line(line, self.lang, self.state, self.palette);
        self.state = next_state;
        spans
    }
}

// ── Language Dispatch & Lexers ───────────────────────────────────────────────

fn lex_line(
    line: &str,
    lang: CodeLanguage,
    entry: LineState,
    sp: SyntaxPalette,
) -> (Vec<(Range<usize>, TokenFormat)>, LineState) {
    if line.is_empty() {
        return (Vec::new(), entry);
    }
    match lang {
        CodeLanguage::PlainText => (Vec::new(), LineState::Normal),
        CodeLanguage::Json => lex_json(line, entry, sp),
        CodeLanguage::Toml => lex_toml(line, entry, sp),
        CodeLanguage::Yaml => lex_yaml(line, sp),
        CodeLanguage::Ini => lex_ini(line, sp),
        CodeLanguage::Markdown => lex_markdown(line, entry, sp),
        CodeLanguage::Html => lex_html(line, entry, sp),
        CodeLanguage::Css => lex_css(line, entry, sp),
        CodeLanguage::Sql => lex_sql(line, entry, sp),
        CodeLanguage::Shell => lex_shell(line, sp),
        CodeLanguage::Python => lex_python(line, entry, sp),
        CodeLanguage::Rust
        | CodeLanguage::TypeScript
        | CodeLanguage::JavaScript
        | CodeLanguage::Go
        | CodeLanguage::C
        | CodeLanguage::Cpp
        | CodeLanguage::CSharp
        | CodeLanguage::Java
        | CodeLanguage::Kotlin => lex_c_family(line, lang, entry, sp),
    }
}

fn next_char_end(s: &str, i: usize) -> usize {
    s[i..].chars().next().map_or(s.len(), |c| i + c.len_utf8())
}

fn scan_quoted(line: &str, start: usize, quote: u8) -> usize {
    let bytes = line.as_bytes();
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            i = next_char_end(line, i + 1);
            continue;
        }
        if bytes[i] == quote {
            return i + 1;
        }
        i = next_char_end(line, i);
    }
    bytes.len()
}

fn scan_number(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    if i + 1 < bytes.len()
        && bytes[i] == b'0'
        && matches!(bytes[i + 1], b'x' | b'X' | b'b' | b'B' | b'o' | b'O')
    {
        i += 2;
        while i < bytes.len() && (bytes[i].is_ascii_hexdigit() || bytes[i] == b'_') {
            i += 1;
        }
        return i;
    }
    while i < bytes.len()
        && (bytes[i].is_ascii_digit()
            || matches!(bytes[i], b'.' | b'_' | b'e' | b'E')
            || (matches!(bytes[i], b'+' | b'-')
                && i > start
                && matches!(bytes[i - 1], b'e' | b'E')))
    {
        i += 1;
    }
    // Optional type suffix like `u32`, `f64`, `usize`, `px`, `rem`, `%`
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'%') {
        i += 1;
    }
    i
}

fn scan_ident(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    i
}

fn is_followed_by_open_paren(bytes: &[u8], mut i: usize) -> bool {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    // Also handle generic call syntax like `foo::<T>(`
    if i + 2 < bytes.len() && bytes[i] == b':' && bytes[i + 1] == b':' && bytes[i + 2] == b'<' {
        return true;
    }
    i < bytes.len() && bytes[i] == b'('
}

fn looks_like_pascal_type(word: &str) -> bool {
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_uppercase() {
        return false;
    }
    // Distinguish `PascalCase` type names from `ALL_CAPS_CONSTANTS`
    chars.any(|c| c.is_ascii_lowercase())
}

fn looks_like_all_caps_const(word: &str) -> bool {
    word.len() >= 2
        && word
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        && word.bytes().any(|b| b.is_ascii_uppercase())
}

// ── C-Family / Rust / TS / JS / Go / Java / C# / Kotlin ──────────────────────

fn lex_c_family(
    line: &str,
    lang: CodeLanguage,
    mut state: LineState,
    sp: SyntaxPalette,
) -> (Vec<(Range<usize>, TokenFormat)>, LineState) {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;

    // Handle active multi-line block comment or template literal continuation
    match state {
        LineState::BlockComment => {
            if let Some(pos) = line.find("*/") {
                out.push((0..pos + 2, TokenFormat::italic(sp.comment)));
                i = pos + 2;
                state = LineState::Normal;
            } else {
                out.push((0..line.len(), TokenFormat::italic(sp.comment)));
                return (out, LineState::BlockComment);
            }
        }
        LineState::BacktickString => {
            let mut end = None;
            let mut k = 0;
            while k < bytes.len() {
                if bytes[k] == b'\\' && lang != CodeLanguage::Go && k + 1 < bytes.len() {
                    k = next_char_end(line, k + 1);
                    continue;
                }
                if bytes[k] == b'`' {
                    end = Some(k + 1);
                    break;
                }
                k = next_char_end(line, k);
            }
            if let Some(e) = end {
                out.push((0..e, TokenFormat::plain(sp.string)));
                i = e;
                state = LineState::Normal;
            } else {
                out.push((0..line.len(), TokenFormat::plain(sp.string)));
                return (out, LineState::BacktickString);
            }
        }
        _ => {
            state = LineState::Normal;
        }
    }

    // C / C++ preprocessor directive (`#include`, `#define`, `#ifdef`, ...)
    if i == 0 && matches!(lang, CodeLanguage::C | CodeLanguage::Cpp | CodeLanguage::CSharp) {
        let trimmed_start = line.len() - line.trim_start().len();
        if bytes.get(trimmed_start) == Some(&b'#') {
            let mut end_dir = trimmed_start + 1;
            while end_dir < bytes.len() && bytes[end_dir].is_ascii_whitespace() {
                end_dir += 1;
            }
            while end_dir < bytes.len()
                && (bytes[end_dir].is_ascii_alphanumeric() || bytes[end_dir] == b'_')
            {
                end_dir += 1;
            }
            out.push((trimmed_start..end_dir, TokenFormat::plain(sp.macro_attr)));
            i = end_dir;
        }
    }

    while i < bytes.len() {
        let b = bytes[i];

        // Line comment (`//` or doc `///`, `//!`)
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            let is_doc = i + 2 < bytes.len() && matches!(bytes[i + 2], b'/' | b'!');
            let col = if is_doc { sp.doc_comment } else { sp.comment };
            out.push((i..line.len(), TokenFormat::italic(col)));
            break;
        }

        // Block comment (`/* ... */`)
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            let is_doc = i + 2 < bytes.len() && bytes[i + 2] == b'*';
            let col = if is_doc { sp.doc_comment } else { sp.comment };
            if let Some(rel_end) = line[i + 2..].find("*/") {
                let end = i + 2 + rel_end + 2;
                out.push((i..end, TokenFormat::italic(col)));
                i = end;
            } else {
                out.push((i..line.len(), TokenFormat::italic(col)));
                return (out, LineState::BlockComment);
            }
            continue;
        }

        // Rust attributes `#[...]` / `#![...]`
        if lang == CodeLanguage::Rust
            && b == b'#'
            && (line[i..].starts_with("#[") || line[i..].starts_with("#!["))
        {
            let start = i;
            let mut depth = 0usize;
            while i < bytes.len() {
                if bytes[i] == b'[' {
                    depth += 1;
                } else if bytes[i] == b']' {
                    depth = depth.saturating_sub(1);
                    i += 1;
                    if depth == 0 {
                        break;
                    }
                    continue;
                }
                i = next_char_end(line, i);
            }
            out.push((start..i, TokenFormat::plain(sp.macro_attr)));
            continue;
        }

        // Decorators / annotations (`@Component`, `@Override`)
        if b == b'@'
            && matches!(
                lang,
                CodeLanguage::TypeScript
                    | CodeLanguage::JavaScript
                    | CodeLanguage::Java
                    | CodeLanguage::Kotlin
            )
        {
            let start = i;
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'.')
            {
                i += 1;
            }
            out.push((start..i, TokenFormat::plain(sp.macro_attr)));
            continue;
        }

        // Rust raw strings `r#"..."#` / `br#"..."#`
        if lang == CodeLanguage::Rust
            && (b == b'r' || (b == b'b' && bytes.get(i + 1) == Some(&b'r')))
        {
            let mut k = if b == b'b' { i + 2 } else { i + 1 };
            let mut hashes = 0usize;
            while k < bytes.len() && bytes[k] == b'#' {
                hashes += 1;
                k += 1;
            }
            if hashes > 0 && bytes.get(k) == Some(&b'"') {
                let closing = format!("\"{}", "#".repeat(hashes));
                let body_start = k + 1;
                let end = line[body_start..]
                    .find(&closing)
                    .map(|p| body_start + p + closing.len())
                    .unwrap_or(bytes.len());
                out.push((i..end, TokenFormat::plain(sp.string)));
                i = end;
                continue;
            }
        }

        // Double-quoted strings
        if b == b'"' {
            let end = scan_quoted(line, i, b'"');
            out.push((i..end, TokenFormat::plain(sp.string)));
            i = end;
            continue;
        }

        // Single-quoted strings / char literals / Rust lifetimes (`'a`, `'static`)
        if b == b'\'' {
            if lang == CodeLanguage::Rust {
                // Distinguish `'a` lifetime from `'x'` / `'\n'` char literal
                let after = i + 1;
                if after < bytes.len() && bytes[after] != b'\\' {
                    let ident_end = if bytes[after].is_ascii_alphabetic() || bytes[after] == b'_' {
                        scan_ident(bytes, after)
                    } else {
                        after
                    };
                    if ident_end > after && bytes.get(ident_end) != Some(&b'\'') {
                        out.push((i..ident_end, TokenFormat::plain(sp.macro_attr)));
                        i = ident_end;
                        continue;
                    }
                }
            }
            let end = scan_quoted(line, i, b'\'');
            out.push((i..end, TokenFormat::plain(sp.string)));
            i = end;
            continue;
        }

        // Backtick template / raw string (JS, TS, Go)
        if b == b'`'
            && matches!(
                lang,
                CodeLanguage::JavaScript | CodeLanguage::TypeScript | CodeLanguage::Go
            )
        {
            let start = i;
            let mut k = i + 1;
            let mut closed = false;
            while k < bytes.len() {
                if bytes[k] == b'\\' && lang != CodeLanguage::Go && k + 1 < bytes.len() {
                    k = next_char_end(line, k + 1);
                    continue;
                }
                if bytes[k] == b'`' {
                    k += 1;
                    closed = true;
                    break;
                }
                k = next_char_end(line, k);
            }
            out.push((start..k, TokenFormat::plain(sp.string)));
            if !closed {
                return (out, LineState::BacktickString);
            }
            i = k;
            continue;
        }

        // Numbers
        if b.is_ascii_digit() {
            let end = scan_number(bytes, i);
            out.push((i..end, TokenFormat::plain(sp.number)));
            i = end;
            continue;
        }

        // Identifiers, keywords, types, macros, function calls
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            let end = scan_ident(bytes, i);
            let word = &line[start..end];

            if lang == CodeLanguage::Rust && bytes.get(end) == Some(&b'!') {
                out.push((start..end + 1, TokenFormat::plain(sp.macro_attr)));
                i = end + 1;
                continue;
            }

            if is_c_family_keyword(lang, word) {
                out.push((start..end, TokenFormat::plain(sp.keyword)));
            } else if is_c_family_literal(lang, word) {
                out.push((start..end, TokenFormat::plain(sp.number)));
            } else if is_c_family_builtin_type(lang, word) || looks_like_pascal_type(word) {
                out.push((start..end, TokenFormat::plain(sp.type_name)));
            } else if is_followed_by_open_paren(bytes, end) {
                out.push((start..end, TokenFormat::plain(sp.function)));
            } else if looks_like_all_caps_const(word) {
                out.push((start..end, TokenFormat::plain(sp.number)));
            }
            i = end;
            continue;
        }

        // Subtle operators & brackets
        if matches!(
            b,
            b'{' | b'}' | b'[' | b']' | b'(' | b')' | b':' | b';' | b',' | b'=' | b'+' | b'-'
                | b'*' | b'/' | b'<' | b'>' | b'&' | b'|' | b'!' | b'?'
        ) {
            out.push((i..i + 1, TokenFormat::plain(sp.punctuation)));
            i += 1;
            continue;
        }

        i = next_char_end(line, i);
    }

    (out, state)
}

fn is_c_family_keyword(lang: CodeLanguage, word: &str) -> bool {
    match lang {
        CodeLanguage::Rust => matches!(
            word,
            "as" | "async"
                | "await"
                | "break"
                | "const"
                | "continue"
                | "crate"
                | "dyn"
                | "else"
                | "enum"
                | "extern"
                | "fn"
                | "for"
                | "if"
                | "impl"
                | "in"
                | "let"
                | "loop"
                | "match"
                | "mod"
                | "move"
                | "mut"
                | "pub"
                | "ref"
                | "return"
                | "self"
                | "static"
                | "struct"
                | "super"
                | "trait"
                | "type"
                | "unsafe"
                | "use"
                | "where"
                | "while"
        ),
        CodeLanguage::TypeScript | CodeLanguage::JavaScript => matches!(
            word,
            "as" | "async"
                | "await"
                | "break"
                | "case"
                | "catch"
                | "class"
                | "const"
                | "continue"
                | "debugger"
                | "declare"
                | "default"
                | "delete"
                | "do"
                | "else"
                | "enum"
                | "export"
                | "extends"
                | "finally"
                | "for"
                | "from"
                | "function"
                | "get"
                | "if"
                | "implements"
                | "import"
                | "in"
                | "instanceof"
                | "interface"
                | "keyof"
                | "let"
                | "module"
                | "namespace"
                | "new"
                | "of"
                | "private"
                | "protected"
                | "public"
                | "readonly"
                | "return"
                | "set"
                | "static"
                | "super"
                | "switch"
                | "this"
                | "throw"
                | "try"
                | "type"
                | "typeof"
                | "var"
                | "void"
                | "while"
                | "with"
                | "yield"
        ),
        CodeLanguage::Go => matches!(
            word,
            "break"
                | "case"
                | "chan"
                | "const"
                | "continue"
                | "default"
                | "defer"
                | "else"
                | "fallthrough"
                | "for"
                | "func"
                | "go"
                | "goto"
                | "if"
                | "import"
                | "interface"
                | "map"
                | "package"
                | "range"
                | "return"
                | "select"
                | "struct"
                | "switch"
                | "type"
                | "var"
        ),
        CodeLanguage::C | CodeLanguage::Cpp => matches!(
            word,
            "alignas"
                | "alignof"
                | "auto"
                | "break"
                | "case"
                | "catch"
                | "class"
                | "const"
                | "constexpr"
                | "continue"
                | "default"
                | "delete"
                | "do"
                | "else"
                | "enum"
                | "explicit"
                | "extern"
                | "final"
                | "for"
                | "friend"
                | "goto"
                | "if"
                | "inline"
                | "mutable"
                | "namespace"
                | "new"
                | "noexcept"
                | "operator"
                | "override"
                | "private"
                | "protected"
                | "public"
                | "register"
                | "return"
                | "sizeof"
                | "static"
                | "struct"
                | "switch"
                | "template"
                | "this"
                | "throw"
                | "try"
                | "typedef"
                | "typename"
                | "union"
                | "using"
                | "virtual"
                | "volatile"
                | "while"
        ),
        CodeLanguage::CSharp => matches!(
            word,
            "abstract"
                | "as"
                | "async"
                | "await"
                | "base"
                | "break"
                | "case"
                | "catch"
                | "class"
                | "const"
                | "continue"
                | "default"
                | "delegate"
                | "do"
                | "else"
                | "enum"
                | "event"
                | "extern"
                | "finally"
                | "for"
                | "foreach"
                | "get"
                | "if"
                | "in"
                | "interface"
                | "internal"
                | "is"
                | "lock"
                | "namespace"
                | "new"
                | "out"
                | "override"
                | "params"
                | "partial"
                | "private"
                | "protected"
                | "public"
                | "readonly"
                | "record"
                | "ref"
                | "return"
                | "sealed"
                | "set"
                | "static"
                | "struct"
                | "switch"
                | "this"
                | "throw"
                | "try"
                | "typeof"
                | "using"
                | "var"
                | "virtual"
                | "void"
                | "while"
                | "yield"
        ),
        CodeLanguage::Java | CodeLanguage::Kotlin => matches!(
            word,
            "abstract"
                | "as"
                | "break"
                | "case"
                | "catch"
                | "class"
                | "companion"
                | "const"
                | "continue"
                | "data"
                | "default"
                | "do"
                | "else"
                | "enum"
                | "extends"
                | "final"
                | "finally"
                | "for"
                | "fun"
                | "if"
                | "implements"
                | "import"
                | "in"
                | "init"
                | "instanceof"
                | "interface"
                | "internal"
                | "is"
                | "lateinit"
                | "new"
                | "object"
                | "open"
                | "override"
                | "package"
                | "private"
                | "protected"
                | "public"
                | "return"
                | "sealed"
                | "static"
                | "super"
                | "suspend"
                | "switch"
                | "synchronized"
                | "this"
                | "throw"
                | "throws"
                | "try"
                | "val"
                | "var"
                | "void"
                | "when"
                | "while"
        ),
        _ => false,
    }
}

fn is_c_family_literal(_lang: CodeLanguage, word: &str) -> bool {
    matches!(
        word,
        "true" | "false" | "null" | "nullptr" | "nil" | "undefined" | "NaN" | "Infinity" | "iota"
    )
}

fn is_c_family_builtin_type(lang: CodeLanguage, word: &str) -> bool {
    match lang {
        CodeLanguage::Rust => matches!(
            word,
            "bool"
                | "char"
                | "str"
                | "i8"
                | "i16"
                | "i32"
                | "i64"
                | "i128"
                | "isize"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "u128"
                | "usize"
                | "f32"
                | "f64"
                | "Self"
        ),
        CodeLanguage::TypeScript | CodeLanguage::JavaScript => matches!(
            word,
            "string" | "number" | "boolean" | "any" | "unknown" | "never" | "object" | "bigint"
                | "symbol"
        ),
        CodeLanguage::Go => matches!(
            word,
            "bool"
                | "byte"
                | "rune"
                | "string"
                | "error"
                | "any"
                | "int"
                | "int8"
                | "int16"
                | "int32"
                | "int64"
                | "uint"
                | "uint8"
                | "uint16"
                | "uint32"
                | "uint64"
                | "uintptr"
                | "float32"
                | "float64"
                | "complex64"
                | "complex128"
        ),
        CodeLanguage::C | CodeLanguage::Cpp => matches!(
            word,
            "void"
                | "bool"
                | "char"
                | "short"
                | "int"
                | "long"
                | "float"
                | "double"
                | "signed"
                | "unsigned"
                | "size_t"
                | "ssize_t"
                | "int8_t"
                | "int16_t"
                | "int32_t"
                | "int64_t"
                | "uint8_t"
                | "uint16_t"
                | "uint32_t"
                | "uint64_t"
                | "std"
        ),
        CodeLanguage::CSharp => matches!(
            word,
            "bool" | "byte" | "char" | "decimal" | "double" | "float" | "int" | "long" | "object"
                | "sbyte" | "short" | "string" | "uint" | "ulong" | "ushort" | "dynamic"
        ),
        CodeLanguage::Java | CodeLanguage::Kotlin => matches!(
            word,
            "boolean" | "byte" | "char" | "double" | "float" | "int" | "long" | "short"
        ),
        _ => false,
    }
}

// ── Python ───────────────────────────────────────────────────────────────────

fn lex_python(
    line: &str,
    mut state: LineState,
    sp: SyntaxPalette,
) -> (Vec<(Range<usize>, TokenFormat)>, LineState) {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;

    if let LineState::TripleString(q) = state {
        let delim = if q == b'"' { "\"\"\"" } else { "'''" };
        if let Some(pos) = line.find(delim) {
            out.push((0..pos + 3, TokenFormat::plain(sp.string)));
            i = pos + 3;
            state = LineState::Normal;
        } else {
            out.push((0..line.len(), TokenFormat::plain(sp.string)));
            return (out, LineState::TripleString(q));
        }
    }

    while i < bytes.len() {
        let b = bytes[i];
        if b == b'#' {
            out.push((i..line.len(), TokenFormat::italic(sp.comment)));
            break;
        }
        if b == b'@' {
            let start = i;
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'.')
            {
                i += 1;
            }
            out.push((start..i, TokenFormat::plain(sp.macro_attr)));
            continue;
        }
        if (b == b'"' || b == b'\'')
            && i + 2 < bytes.len()
            && bytes[i + 1] == b
            && bytes[i + 2] == b
        {
            let delim = if b == b'"' { "\"\"\"" } else { "'''" };
            if let Some(rel) = line[i + 3..].find(delim) {
                let end = i + 3 + rel + 3;
                out.push((i..end, TokenFormat::plain(sp.string)));
                i = end;
            } else {
                out.push((i..line.len(), TokenFormat::plain(sp.string)));
                return (out, LineState::TripleString(b));
            }
            continue;
        }
        if b == b'"' || b == b'\'' {
            let end = scan_quoted(line, i, b);
            out.push((i..end, TokenFormat::plain(sp.string)));
            i = end;
            continue;
        }
        if b.is_ascii_digit() {
            let end = scan_number(bytes, i);
            out.push((i..end, TokenFormat::plain(sp.number)));
            i = end;
            continue;
        }
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            let end = scan_ident(bytes, i);
            let word = &line[start..end];
            if matches!(
                word,
                "and" | "as"
                    | "assert"
                    | "async"
                    | "await"
                    | "break"
                    | "case"
                    | "class"
                    | "continue"
                    | "def"
                    | "del"
                    | "elif"
                    | "else"
                    | "except"
                    | "finally"
                    | "for"
                    | "from"
                    | "global"
                    | "if"
                    | "import"
                    | "in"
                    | "is"
                    | "lambda"
                    | "match"
                    | "nonlocal"
                    | "not"
                    | "or"
                    | "pass"
                    | "raise"
                    | "return"
                    | "try"
                    | "while"
                    | "with"
                    | "yield"
            ) {
                out.push((start..end, TokenFormat::plain(sp.keyword)));
            } else if matches!(word, "True" | "False" | "None") {
                out.push((start..end, TokenFormat::plain(sp.number)));
            } else if matches!(
                word,
                "self"
                    | "cls"
                    | "int"
                    | "float"
                    | "str"
                    | "bool"
                    | "list"
                    | "dict"
                    | "set"
                    | "tuple"
                    | "bytes"
                    | "object"
            ) || looks_like_pascal_type(word)
            {
                out.push((start..end, TokenFormat::plain(sp.type_name)));
            } else if is_followed_by_open_paren(bytes, end) {
                out.push((start..end, TokenFormat::plain(sp.function)));
            }
            i = end;
            continue;
        }
        i = next_char_end(line, i);
    }

    (out, state)
}

// ── JSON / JSONC ─────────────────────────────────────────────────────────────

fn lex_json(
    line: &str,
    mut state: LineState,
    sp: SyntaxPalette,
) -> (Vec<(Range<usize>, TokenFormat)>, LineState) {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;

    if state == LineState::BlockComment {
        if let Some(pos) = line.find("*/") {
            out.push((0..pos + 2, TokenFormat::italic(sp.comment)));
            i = pos + 2;
            state = LineState::Normal;
        } else {
            out.push((0..line.len(), TokenFormat::italic(sp.comment)));
            return (out, LineState::BlockComment);
        }
    }

    while i < bytes.len() {
        let b = bytes[i];
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            out.push((i..line.len(), TokenFormat::italic(sp.comment)));
            break;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            if let Some(rel) = line[i + 2..].find("*/") {
                let end = i + 4 + rel;
                out.push((i..end, TokenFormat::italic(sp.comment)));
                i = end;
            } else {
                out.push((i..line.len(), TokenFormat::italic(sp.comment)));
                return (out, LineState::BlockComment);
            }
            continue;
        }
        if b == b'"' {
            let end = scan_quoted(line, i, b'"');
            let mut look = end;
            while look < bytes.len() && bytes[look].is_ascii_whitespace() {
                look += 1;
            }
            let color = if bytes.get(look) == Some(&b':') {
                sp.tag_key
            } else {
                sp.string
            };
            out.push((i..end, TokenFormat::plain(color)));
            i = end;
            continue;
        }
        if b.is_ascii_digit()
            || (b == b'-' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit))
        {
            let start = i;
            if b == b'-' {
                i += 1;
            }
            let end = scan_number(bytes, i);
            out.push((start..end, TokenFormat::plain(sp.number)));
            i = end;
            continue;
        }
        if line[i..].starts_with("true") && !is_ident_char(bytes.get(i + 4).copied()) {
            out.push((i..i + 4, TokenFormat::plain(sp.keyword)));
            i += 4;
            continue;
        }
        if line[i..].starts_with("false") && !is_ident_char(bytes.get(i + 5).copied()) {
            out.push((i..i + 5, TokenFormat::plain(sp.keyword)));
            i += 5;
            continue;
        }
        if line[i..].starts_with("null") && !is_ident_char(bytes.get(i + 4).copied()) {
            out.push((i..i + 4, TokenFormat::plain(sp.keyword)));
            i += 4;
            continue;
        }
        if matches!(b, b'{' | b'}' | b'[' | b']' | b':' | b',') {
            out.push((i..i + 1, TokenFormat::plain(sp.punctuation)));
            i += 1;
            continue;
        }
        i = next_char_end(line, i);
    }

    (out, state)
}

fn is_ident_char(b: Option<u8>) -> bool {
    b.is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
}

// ── TOML ─────────────────────────────────────────────────────────────────────

fn lex_toml(
    line: &str,
    mut state: LineState,
    sp: SyntaxPalette,
) -> (Vec<(Range<usize>, TokenFormat)>, LineState) {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;

    if let LineState::TripleString(q) = state {
        let delim = if q == b'"' { "\"\"\"" } else { "'''" };
        if let Some(pos) = line.find(delim) {
            out.push((0..pos + 3, TokenFormat::plain(sp.string)));
            i = pos + 3;
            state = LineState::Normal;
        } else {
            out.push((0..line.len(), TokenFormat::plain(sp.string)));
            return (out, LineState::TripleString(q));
        }
    }

    let trimmed_start = line.len() - line.trim_start().len();
    if i == 0 && bytes.get(trimmed_start) == Some(&b'[') {
        let comment_pos = line.find('#').unwrap_or(line.len());
        out.push((trimmed_start..comment_pos, TokenFormat::bold(sp.keyword)));
        if comment_pos < line.len() {
            out.push((comment_pos..line.len(), TokenFormat::italic(sp.comment)));
        }
        return (out, LineState::Normal);
    }

    // Highlight key before `=`
    let mut seen_eq = false;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'#' {
            out.push((i..line.len(), TokenFormat::italic(sp.comment)));
            break;
        }
        if !seen_eq && b == b'=' {
            out.push((i..i + 1, TokenFormat::plain(sp.punctuation)));
            seen_eq = true;
            i += 1;
            continue;
        }
        if !seen_eq && (b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.') {
            let start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric()
                    || matches!(bytes[i], b'_' | b'-' | b'.'))
            {
                i += 1;
            }
            out.push((start..i, TokenFormat::plain(sp.tag_key)));
            continue;
        }
        if (b == b'"' || b == b'\'')
            && i + 2 < bytes.len()
            && bytes[i + 1] == b
            && bytes[i + 2] == b
        {
            let delim = if b == b'"' { "\"\"\"" } else { "'''" };
            if let Some(rel) = line[i + 3..].find(delim) {
                let end = i + 6 + rel;
                out.push((i..end, TokenFormat::plain(sp.string)));
                i = end;
            } else {
                out.push((i..line.len(), TokenFormat::plain(sp.string)));
                return (out, LineState::TripleString(b));
            }
            continue;
        }
        if b == b'"' || b == b'\'' {
            let end = scan_quoted(line, i, b);
            let col = if seen_eq { sp.string } else { sp.tag_key };
            out.push((i..end, TokenFormat::plain(col)));
            i = end;
            continue;
        }
        if seen_eq && (b.is_ascii_digit() || (b == b'-' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit))) {
            let start = i;
            if b == b'-' {
                i += 1;
            }
            let end = scan_number(bytes, i);
            out.push((start..end, TokenFormat::plain(sp.number)));
            i = end;
            continue;
        }
        if seen_eq && (b.is_ascii_alphabetic() || b == b'_') {
            let start = i;
            let end = scan_ident(bytes, i);
            if matches!(&line[start..end], "true" | "false") {
                out.push((start..end, TokenFormat::plain(sp.keyword)));
            }
            i = end;
            continue;
        }
        i = next_char_end(line, i);
    }

    (out, state)
}

// ── YAML ─────────────────────────────────────────────────────────────────────

fn lex_yaml(line: &str, sp: SyntaxPalette) -> (Vec<(Range<usize>, TokenFormat)>, LineState) {
    let mut out = Vec::new();
    let trimmed_start = line.len() - line.trim_start().len();
    let trimmed = &line[trimmed_start..];
    if trimmed.starts_with('#') {
        return (
            vec![(trimmed_start..line.len(), TokenFormat::italic(sp.comment))],
            LineState::Normal,
        );
    }
    if trimmed == "---" || trimmed == "..." {
        return (
            vec![(trimmed_start..line.len(), TokenFormat::bold(sp.keyword))],
            LineState::Normal,
        );
    }

    let bytes = line.as_bytes();
    let mut i = trimmed_start;
    if line[i..].starts_with("- ") {
        out.push((i..i + 1, TokenFormat::plain(sp.keyword)));
        i += 2;
    }

    // Check for `key:` pattern
    if let Some(colon_rel) = line[i..].find(':') {
        let colon = i + colon_rel;
        let next_b = bytes.get(colon + 1).copied();
        if (next_b.is_none() || next_b == Some(b' ')) && !line[i..colon].contains('#') {
            out.push((i..colon, TokenFormat::plain(sp.tag_key)));
            out.push((colon..colon + 1, TokenFormat::plain(sp.punctuation)));
            i = colon + 1;
        }
    }

    while i < bytes.len() {
        let b = bytes[i];
        if b == b'#' && (i == 0 || bytes[i - 1].is_ascii_whitespace()) {
            out.push((i..line.len(), TokenFormat::italic(sp.comment)));
            break;
        }
        if b == b'"' || b == b'\'' {
            let end = scan_quoted(line, i, b);
            out.push((i..end, TokenFormat::plain(sp.string)));
            i = end;
            continue;
        }
        if b.is_ascii_digit()
            || (b == b'-' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit))
        {
            let start = i;
            if b == b'-' {
                i += 1;
            }
            let end = scan_number(bytes, i);
            out.push((start..end, TokenFormat::plain(sp.number)));
            i = end;
            continue;
        }
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            let end = scan_ident(bytes, i);
            if matches!(
                &line[start..end],
                "true" | "false" | "null" | "yes" | "no" | "on" | "off"
            ) {
                out.push((start..end, TokenFormat::plain(sp.keyword)));
            } else {
                out.push((start..end, TokenFormat::plain(sp.string)));
            }
            i = end;
            continue;
        }
        i = next_char_end(line, i);
    }

    (out, LineState::Normal)
}

// ── INI / .env / Config ──────────────────────────────────────────────────────

fn lex_ini(line: &str, sp: SyntaxPalette) -> (Vec<(Range<usize>, TokenFormat)>, LineState) {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return (Vec::new(), LineState::Normal);
    }
    if trimmed.starts_with('#') || trimmed.starts_with(';') {
        return (
            vec![(0..line.len(), TokenFormat::italic(sp.comment))],
            LineState::Normal,
        );
    }
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        return (
            vec![(0..line.len(), TokenFormat::bold(sp.keyword))],
            LineState::Normal,
        );
    }
    if let Some(eq) = line.find('=') {
        let mut out = Vec::new();
        if eq > 0 {
            out.push((0..eq, TokenFormat::plain(sp.tag_key)));
        }
        out.push((eq..eq + 1, TokenFormat::plain(sp.punctuation)));
        if eq + 1 < line.len() {
            out.push((eq + 1..line.len(), TokenFormat::plain(sp.string)));
        }
        return (out, LineState::Normal);
    }
    (Vec::new(), LineState::Normal)
}

// ── Markdown ─────────────────────────────────────────────────────────────────

fn lex_markdown(
    line: &str,
    state: LineState,
    sp: SyntaxPalette,
) -> (Vec<(Range<usize>, TokenFormat)>, LineState) {
    let trimmed = line.trim_start();
    if trimmed.starts_with("```") {
        let next = if state == LineState::MarkdownFence {
            LineState::Normal
        } else {
            LineState::MarkdownFence
        };
        return (
            vec![(0..line.len(), TokenFormat::plain(sp.macro_attr))],
            next,
        );
    }
    if state == LineState::MarkdownFence {
        return (
            vec![(0..line.len(), TokenFormat::plain(sp.string))],
            LineState::MarkdownFence,
        );
    }
    if trimmed.starts_with('#') {
        return (
            vec![(0..line.len(), TokenFormat::bold(sp.type_name))],
            LineState::Normal,
        );
    }
    if trimmed.starts_with('>') {
        return (
            vec![(0..line.len(), TokenFormat::italic(sp.comment))],
            LineState::Normal,
        );
    }

    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let start = i;
            if let Some(rel) = line[i + 1..].find('`') {
                let end = i + 2 + rel;
                out.push((start..end, TokenFormat::plain(sp.string)));
                i = end;
                continue;
            }
        }
        if line[i..].starts_with("**")
            && let Some(rel) = line[i + 2..].find("**")
        {
            let end = i + 4 + rel;
            out.push((i..end, TokenFormat::bold(sp.keyword)));
            i = end;
            continue;
        }
        if bytes[i] == b'['
            && let Some(close_br) = line[i + 1..].find("](")
            && let Some(close_par) = line[i + 1 + close_br + 2..].find(')')
        {
            let text_end = i + 1 + close_br + 1;
            let url_end = text_end + 1 + close_par + 1;
            out.push((i..text_end, TokenFormat::plain(sp.function)));
            out.push((text_end..url_end, TokenFormat::plain(sp.tag_key)));
            i = url_end;
            continue;
        }
        i = next_char_end(line, i);
    }
    (out, LineState::Normal)
}

// ── HTML / XML / SVG ─────────────────────────────────────────────────────────

fn lex_html(
    line: &str,
    mut state: LineState,
    sp: SyntaxPalette,
) -> (Vec<(Range<usize>, TokenFormat)>, LineState) {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;

    if state == LineState::HtmlComment {
        if let Some(pos) = line.find("-->") {
            out.push((0..pos + 3, TokenFormat::italic(sp.comment)));
            i = pos + 3;
            state = LineState::Normal;
        } else {
            out.push((0..line.len(), TokenFormat::italic(sp.comment)));
            return (out, LineState::HtmlComment);
        }
    }

    while i < bytes.len() {
        if line[i..].starts_with("<!--") {
            if let Some(rel) = line[i + 4..].find("-->") {
                let end = i + 7 + rel;
                out.push((i..end, TokenFormat::italic(sp.comment)));
                i = end;
            } else {
                out.push((i..line.len(), TokenFormat::italic(sp.comment)));
                return (out, LineState::HtmlComment);
            }
            continue;
        }
        if bytes[i] == b'<' {
            out.push((i..i + 1, TokenFormat::plain(sp.punctuation)));
            i += 1;
            if bytes.get(i) == Some(&b'/') || bytes.get(i) == Some(&b'!') || bytes.get(i) == Some(&b'?') {
                out.push((i..i + 1, TokenFormat::plain(sp.punctuation)));
                i += 1;
            }
            let tag_start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'-' | b'_' | b':'))
            {
                i += 1;
            }
            if i > tag_start {
                out.push((tag_start..i, TokenFormat::plain(sp.tag_key)));
            }
            // Scan attributes inside tag
            while i < bytes.len() && bytes[i] != b'>' {
                if bytes[i] == b'"' || bytes[i] == b'\'' {
                    let end = scan_quoted(line, i, bytes[i]);
                    out.push((i..end, TokenFormat::plain(sp.string)));
                    i = end;
                } else if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' || bytes[i] == b':' {
                    let a_start = i;
                    while i < bytes.len()
                        && (bytes[i].is_ascii_alphanumeric()
                            || matches!(bytes[i], b'-' | b'_' | b':' | b'@' | b'.'))
                    {
                        i += 1;
                    }
                    out.push((a_start..i, TokenFormat::plain(sp.type_name)));
                } else if bytes[i] == b'=' || bytes[i] == b'/' {
                    out.push((i..i + 1, TokenFormat::plain(sp.punctuation)));
                    i += 1;
                } else {
                    i = next_char_end(line, i);
                }
            }
            if bytes.get(i) == Some(&b'>') {
                out.push((i..i + 1, TokenFormat::plain(sp.punctuation)));
                i += 1;
            }
            continue;
        }
        i = next_char_end(line, i);
    }

    (out, state)
}

// ── CSS / SCSS ───────────────────────────────────────────────────────────────

fn lex_css(
    line: &str,
    mut state: LineState,
    sp: SyntaxPalette,
) -> (Vec<(Range<usize>, TokenFormat)>, LineState) {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;

    if state == LineState::BlockComment {
        if let Some(pos) = line.find("*/") {
            out.push((0..pos + 2, TokenFormat::italic(sp.comment)));
            i = pos + 2;
            state = LineState::Normal;
        } else {
            out.push((0..line.len(), TokenFormat::italic(sp.comment)));
            return (out, LineState::BlockComment);
        }
    }

    while i < bytes.len() {
        let b = bytes[i];
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            if let Some(rel) = line[i + 2..].find("*/") {
                let end = i + 4 + rel;
                out.push((i..end, TokenFormat::italic(sp.comment)));
                i = end;
            } else {
                out.push((i..line.len(), TokenFormat::italic(sp.comment)));
                return (out, LineState::BlockComment);
            }
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            out.push((i..line.len(), TokenFormat::italic(sp.comment)));
            break;
        }
        if b == b'@' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
                i += 1;
            }
            out.push((start..i, TokenFormat::plain(sp.keyword)));
            continue;
        }
        if b == b'"' || b == b'\'' {
            let end = scan_quoted(line, i, b);
            out.push((i..end, TokenFormat::plain(sp.string)));
            i = end;
            continue;
        }
        if b == b'#' && bytes.get(i + 1).is_some_and(u8::is_ascii_hexdigit) {
            let start = i;
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                i += 1;
            }
            out.push((start..i, TokenFormat::plain(sp.number)));
            continue;
        }
        if b.is_ascii_digit() {
            let end = scan_number(bytes, i);
            out.push((i..end, TokenFormat::plain(sp.number)));
            i = end;
            continue;
        }
        if b.is_ascii_alphabetic() || b == b'-' || b == b'.' {
            let start = i;
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'-' | b'_'))
            {
                i += 1;
            }
            let mut look = i;
            while look < bytes.len() && bytes[look].is_ascii_whitespace() {
                look += 1;
            }
            if bytes.get(look) == Some(&b':') && !line[start..i].starts_with('.') {
                out.push((start..i, TokenFormat::plain(sp.tag_key)));
            } else if line[start..i].starts_with('.') {
                out.push((start..i, TokenFormat::plain(sp.type_name)));
            } else if bytes.get(look) == Some(&b'(') {
                out.push((start..i, TokenFormat::plain(sp.function)));
            }
            continue;
        }
        i = next_char_end(line, i);
    }

    (out, state)
}

// ── SQL ──────────────────────────────────────────────────────────────────────

fn lex_sql(
    line: &str,
    mut state: LineState,
    sp: SyntaxPalette,
) -> (Vec<(Range<usize>, TokenFormat)>, LineState) {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;

    if state == LineState::BlockComment {
        if let Some(pos) = line.find("*/") {
            out.push((0..pos + 2, TokenFormat::italic(sp.comment)));
            i = pos + 2;
            state = LineState::Normal;
        } else {
            out.push((0..line.len(), TokenFormat::italic(sp.comment)));
            return (out, LineState::BlockComment);
        }
    }

    while i < bytes.len() {
        let b = bytes[i];
        if b == b'-' && bytes.get(i + 1) == Some(&b'-') {
            out.push((i..line.len(), TokenFormat::italic(sp.comment)));
            break;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            if let Some(rel) = line[i + 2..].find("*/") {
                let end = i + 4 + rel;
                out.push((i..end, TokenFormat::italic(sp.comment)));
                i = end;
            } else {
                out.push((i..line.len(), TokenFormat::italic(sp.comment)));
                return (out, LineState::BlockComment);
            }
            continue;
        }
        if b == b'\'' || b == b'"' {
            let end = scan_quoted(line, i, b);
            out.push((i..end, TokenFormat::plain(sp.string)));
            i = end;
            continue;
        }
        if b.is_ascii_digit() {
            let end = scan_number(bytes, i);
            out.push((i..end, TokenFormat::plain(sp.number)));
            i = end;
            continue;
        }
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            let end = scan_ident(bytes, i);
            let upper = line[start..end].to_ascii_uppercase();
            if matches!(
                upper.as_str(),
                "SELECT"
                    | "FROM"
                    | "WHERE"
                    | "JOIN"
                    | "LEFT"
                    | "RIGHT"
                    | "INNER"
                    | "OUTER"
                    | "ON"
                    | "AS"
                    | "INSERT"
                    | "INTO"
                    | "VALUES"
                    | "UPDATE"
                    | "SET"
                    | "DELETE"
                    | "CREATE"
                    | "ALTER"
                    | "DROP"
                    | "TABLE"
                    | "INDEX"
                    | "VIEW"
                    | "AND"
                    | "OR"
                    | "NOT"
                    | "NULL"
                    | "IS"
                    | "IN"
                    | "LIKE"
                    | "ORDER"
                    | "GROUP"
                    | "BY"
                    | "HAVING"
                    | "LIMIT"
                    | "OFFSET"
                    | "DISTINCT"
                    | "UNION"
                    | "ALL"
                    | "CASE"
                    | "WHEN"
                    | "THEN"
                    | "ELSE"
                    | "END"
                    | "PRIMARY"
                    | "KEY"
                    | "FOREIGN"
                    | "REFERENCES"
                    | "BEGIN"
                    | "COMMIT"
                    | "ROLLBACK"
                    | "TRUE"
                    | "FALSE"
            ) {
                out.push((start..end, TokenFormat::plain(sp.keyword)));
            } else if is_followed_by_open_paren(bytes, end) {
                out.push((start..end, TokenFormat::plain(sp.function)));
            }
            i = end;
            continue;
        }
        i = next_char_end(line, i);
    }

    (out, state)
}

// ── Shell / PowerShell / Dockerfile / Makefile ───────────────────────────────

fn lex_shell(line: &str, sp: SyntaxPalette) -> (Vec<(Range<usize>, TokenFormat)>, LineState) {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        let b = bytes[i];
        if b == b'#' && (i == 0 || bytes[i - 1].is_ascii_whitespace()) {
            out.push((i..line.len(), TokenFormat::italic(sp.comment)));
            break;
        }
        if b == b'"' || b == b'\'' || b == b'`' {
            let end = scan_quoted(line, i, b);
            out.push((i..end, TokenFormat::plain(sp.string)));
            i = end;
            continue;
        }
        if b == b'$' {
            let start = i;
            i += 1;
            if bytes.get(i) == Some(&b'{') {
                while i < bytes.len() && bytes[i] != b'}' {
                    i = next_char_end(line, i);
                }
                if i < bytes.len() {
                    i += 1;
                }
            } else {
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b':'))
                {
                    i += 1;
                }
            }
            out.push((start..i, TokenFormat::plain(sp.tag_key)));
            continue;
        }
        if b.is_ascii_digit() {
            let end = scan_number(bytes, i);
            out.push((i..end, TokenFormat::plain(sp.number)));
            i = end;
            continue;
        }
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            let end = scan_ident(bytes, i);
            let word = &line[start..end];
            if matches!(
                word,
                "if" | "then"
                    | "else"
                    | "elif"
                    | "fi"
                    | "for"
                    | "in"
                    | "do"
                    | "done"
                    | "while"
                    | "case"
                    | "esac"
                    | "function"
                    | "return"
                    | "export"
                    | "local"
                    | "set"
                    | "unset"
                    | "echo"
                    | "exit"
                    | "FROM"
                    | "RUN"
                    | "COPY"
                    | "ADD"
                    | "WORKDIR"
                    | "ENV"
                    | "EXPOSE"
                    | "CMD"
                    | "ENTRYPOINT"
                    | "ARG"
                    | "param"
                    | "foreach"
            ) {
                out.push((start..end, TokenFormat::plain(sp.keyword)));
            }
            i = end;
            continue;
        }
        i = next_char_end(line, i);
    }

    (out, LineState::Normal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use adv_text::Highlighter;

    #[test]
    fn detects_languages_from_extensions_and_filenames() {
        assert_eq!(CodeLanguage::from_path(Path::new("src/main.rs")), CodeLanguage::Rust);
        assert_eq!(CodeLanguage::from_path(Path::new("app.tsx")), CodeLanguage::TypeScript);
        assert_eq!(CodeLanguage::from_path(Path::new("Cargo.lock")), CodeLanguage::Toml);
        assert_eq!(CodeLanguage::from_path(Path::new("Dockerfile")), CodeLanguage::Shell);
        assert_eq!(CodeLanguage::from_path(Path::new(".env.local")), CodeLanguage::Ini);
        assert_eq!(CodeLanguage::from_path(Path::new("schema.sql")), CodeLanguage::Sql);
    }

    #[test]
    fn highlights_rust_multiline_block_comments_macros_and_utf8() {
        let settings = CodeHighlightSettings {
            language: CodeLanguage::Rust,
            theme_mode: ThemeMode::Dark,
            search_query: String::new(),
            search_options: SearchOptions::default(),
            active_match_line: None,
            active_match_cols: None,
        };
        let sp = SyntaxPalette::for_theme(ThemeMode::Dark);
        let mut hl = CodeSyntaxHighlighter::new(&settings);

        let l0: Vec<_> = hl.highlight_line("/* đầu comment tiếng Việt").collect();
        assert_eq!(l0.len(), 1);
        assert_eq!(l0[0].1.color, sp.comment);

        let l1: Vec<_> = hl.highlight_line("   still inside comment fn fake()").collect();
        assert_eq!(l1.len(), 1);
        assert_eq!(l1[0].1.color, sp.comment);

        let l2: Vec<_> = hl
            .highlight_line("end */ pub fn run<'a>(x: Option<usize>) { println!(\"ok\"); }")
            .collect();
        assert!(l2.iter().any(|(r, f)| r == &(0..6) && f.color == sp.comment));
        assert!(l2.iter().any(|(_, f)| f.color == sp.keyword));
        assert!(l2.iter().any(|(_, f)| f.color == sp.type_name));
        assert!(l2.iter().any(|(_, f)| f.color == sp.macro_attr));
        assert!(l2.iter().any(|(_, f)| f.color == sp.string));
    }

    #[test]
    fn overlays_search_matches_cleanly_without_losing_syntax_colors() {
        let settings = CodeHighlightSettings {
            language: CodeLanguage::Json,
            theme_mode: ThemeMode::Dark,
            search_query: "name".into(),
            search_options: SearchOptions::default(),
            active_match_line: Some(0),
            active_match_cols: Some((3, 7)),
        };
        let sp = SyntaxPalette::for_theme(ThemeMode::Dark);
        let mut hl = CodeSyntaxHighlighter::new(&settings);

        let line = r#"  "name": "gitfleet""#;
        let spans: Vec<_> = hl.highlight_line(line).collect();
        // Verify sorted and strictly non-overlapping
        for win in spans.windows(2) {
            assert!(
                win[0].0.end <= win[1].0.start,
                "spans must be disjoint: {:?} vs {:?}",
                win[0].0,
                win[1].0
            );
        }
        // Active search match `"name"` interior `name` is bold active color
        assert!(spans.iter().any(|(r, f)| &line[r.clone()] == "name" && f.bold && f.color == sp.search_active));
        // Surrounding value string `"gitfleet"` still keeps green syntax highlight
        assert!(spans.iter().any(|(r, f)| &line[r.clone()] == "\"gitfleet\"" && f.color == sp.string));
    }

    #[test]
    fn diff_syntax_highlighter_tracks_multiline_and_resets_across_gaps() {
        let sp = SyntaxPalette::for_theme(ThemeMode::Dark);
        let mut hl = DiffSyntaxHighlighter::new(CodeLanguage::Rust, ThemeMode::Dark);

        let l0 = hl.highlight_line("/* start block comment");
        assert_eq!(l0.len(), 1);
        assert_eq!(l0[0].1.color, sp.comment);

        let l1 = hl.highlight_line("fn still_commented()");
        assert_eq!(l1.len(), 1);
        assert_eq!(l1[0].1.color, sp.comment);

        // Simulated hidden diff gap / hunk header resets lexer state cleanly
        hl.reset();
        let l2 = hl.highlight_line("pub fn real_code() -> usize { 42 }");
        assert!(l2.iter().any(|(_, f)| f.color == sp.keyword));
        assert!(l2.iter().any(|(_, f)| f.color == sp.number));
    }
}
