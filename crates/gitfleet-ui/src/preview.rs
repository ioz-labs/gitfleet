use crate::syntax::{CodeLanguage, DiffSyntaxHighlighter, TokenFormat};
use crate::theme::{self, Palette, ThemeMode, type_scale};
use iced::advanced::text::Highlighter as TextHighlighter;
use iced::widget::{Space, column, container, horizontal_rule, image, markdown, row, svg, text};
use iced::{Alignment, Background, Border, Color, ContentFit, Element, Length, alignment};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct HighlightedPreviewLine {
    pub text: String,
    pub spans: Vec<(Range<usize>, TokenFormat)>,
}

#[derive(Debug)]
pub struct HighlightedPreviewDoc {
    pub lines: Vec<HighlightedPreviewLine>,
    pub max_chars: usize,
    pub language: CodeLanguage,
    pub theme_mode: ThemeMode,
}

impl HighlightedPreviewDoc {
    pub fn build(raw: &str, language: CodeLanguage, theme_mode: ThemeMode) -> Arc<Self> {
        let line_est = raw.bytes().filter(|&b| b == b'\n').count() + 1;
        let mut lines = Vec::with_capacity(line_est.min(50_000));
        let mut max_chars = 40usize;
        let effective_lang = if line_est > 12_000 {
            CodeLanguage::PlainText
        } else {
            language
        };
        let mut hl = DiffSyntaxHighlighter::new(effective_lang, theme_mode);
        for raw_line in raw.lines().take(50_000) {
            let clean = theme::sanitize_code_line(raw_line);
            max_chars = max_chars.max(clean.chars().count());
            let spans = if effective_lang == CodeLanguage::PlainText {
                Vec::new()
            } else {
                hl.highlight_line(&clean)
            };
            lines.push(HighlightedPreviewLine { text: clean, spans });
        }
        if lines.is_empty() {
            lines.push(HighlightedPreviewLine {
                text: String::new(),
                spans: Vec::new(),
            });
        }
        Arc::new(Self {
            lines,
            max_chars,
            language,
            theme_mode,
        })
    }
}

pub struct PreviewCodeCache {
    pub language: CodeLanguage,
    inner: Mutex<Option<Arc<HighlightedPreviewDoc>>>,
}

impl PreviewCodeCache {
    pub fn new(language: CodeLanguage) -> Self {
        Self {
            language,
            inner: Mutex::new(None),
        }
    }

    pub fn clone_with_language(&self) -> Self {
        let cached = self.inner.lock().ok().and_then(|g| g.clone());
        Self {
            language: self.language,
            inner: Mutex::new(cached),
        }
    }

    pub fn get_or_build(&self, raw: &str, theme_mode: ThemeMode) -> Arc<HighlightedPreviewDoc> {
        if let Ok(guard) = self.inner.lock()
            && let Some(existing) = guard.as_ref()
            && existing.theme_mode == theme_mode
            && existing.language == self.language
        {
            return Arc::clone(existing);
        }
        let built = HighlightedPreviewDoc::build(raw, self.language, theme_mode);
        if let Ok(mut guard) = self.inner.lock() {
            *guard = Some(Arc::clone(&built));
        }
        built
    }

    pub fn set_language(&mut self, language: CodeLanguage) {
        if self.language != language {
            self.language = language;
            if let Ok(mut guard) = self.inner.lock() {
                *guard = None;
            }
        }
    }
}

/// Zero-copy `Clone + Debug + Send + Sync` slot for moving an off-thread prebuilt `PreviewState`
/// across Iced `Message` boundaries onto the UI state without re-parsing on the main thread.
#[derive(Clone)]
pub struct PrebuiltPreviewSlot(Arc<Mutex<Option<PreviewState>>>);

impl std::fmt::Debug for PrebuiltPreviewSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PrebuiltPreviewSlot(..)")
    }
}

impl PrebuiltPreviewSlot {
    pub fn new(state: PreviewState) -> Self {
        Self(Arc::new(Mutex::new(Some(state))))
    }

    pub fn empty() -> Self {
        Self::new(PreviewState::Empty)
    }

    pub fn take(&self) -> PreviewState {
        self.0
            .lock()
            .ok()
            .and_then(|mut g| g.take())
            .unwrap_or(PreviewState::Empty)
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewHighlightKind {
    Plain,
    Json,
    Config,
    Code,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewHighlightSettings {
    pub kind: PreviewHighlightKind,
    pub theme_mode: ThemeMode,
}

#[allow(dead_code)]
pub struct PreviewHighlighter {
    settings: PreviewHighlightSettings,
    current_line: usize,
}

impl TextHighlighter for PreviewHighlighter {
    type Settings = PreviewHighlightSettings;
    type Highlight = Color;
    type Iterator<'a> = std::vec::IntoIter<(Range<usize>, Color)>;

    fn new(settings: &Self::Settings) -> Self {
        Self {
            settings: *settings,
            current_line: 0,
        }
    }

    fn update(&mut self, new_settings: &Self::Settings) {
        if self.settings != *new_settings {
            self.settings = *new_settings;
            self.current_line = 0;
        }
    }

    fn change_line(&mut self, line: usize) {
        if line < self.current_line {
            self.current_line = line;
        }
    }

    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        self.current_line = self.current_line.saturating_add(1);
        let p = self.settings.theme_mode.palette();
        let spans = match self.settings.kind {
            PreviewHighlightKind::Plain => Vec::new(),
            PreviewHighlightKind::Json => highlight_json_line(line, p),
            PreviewHighlightKind::Config => highlight_config_line(line, p),
            PreviewHighlightKind::Code => highlight_code_line(line, p),
        };
        spans.into_iter()
    }

    fn current_line(&self) -> usize {
        self.current_line
    }
}

#[allow(dead_code)]
fn highlight_json_line(line: &str, p: Palette) -> Vec<(Range<usize>, Color)> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                if bytes[i] == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            let mut look = i;
            while look < bytes.len() && bytes[look].is_ascii_whitespace() {
                look += 1;
            }
            let color = if look < bytes.len() && bytes[look] == b':' {
                p.accent_fg
            } else {
                p.success_fg
            };
            out.push((start..i, color));
        } else if b == b'-' || b.is_ascii_digit() {
            let start = i;
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_digit()
                    || matches!(bytes[i], b'.' | b'e' | b'E' | b'+' | b'-'))
            {
                i += 1;
            }
            out.push((start..i, p.warning_fg));
        } else if line[i..].starts_with("true") {
            out.push((i..i + 4, p.violet_fg));
            i += 4;
        } else if line[i..].starts_with("false") {
            out.push((i..i + 5, p.violet_fg));
            i += 5;
        } else if line[i..].starts_with("null") {
            out.push((i..i + 4, p.violet_fg));
            i += 4;
        } else {
            let ch = line[i..].chars().next().unwrap();
            i += ch.len_utf8();
        }
    }
    out
}

#[allow(dead_code)]
fn highlight_config_line(line: &str, p: Palette) -> Vec<(Range<usize>, Color)> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if trimmed.starts_with('#') || trimmed.starts_with(';') {
        return vec![(0..line.len(), p.text_muted)];
    }
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        return vec![(0..line.len(), p.violet_fg)];
    }
    let sep_pos = match (line.find('='), line.find(':')) {
        (Some(e), Some(c)) => Some(e.min(c)),
        (Some(e), None) => Some(e),
        (None, Some(c)) => Some(c),
        (None, None) => None,
    };
    if let Some(pos) = sep_pos {
        let mut out = Vec::new();
        if pos > 0 {
            out.push((0..pos, p.accent_fg));
        }
        out.push((pos..pos + 1, p.text_muted));
        if pos + 1 < line.len() {
            out.push((pos + 1..line.len(), p.success_fg));
        }
        out
    } else {
        Vec::new()
    }
}

#[allow(dead_code)]
fn highlight_code_line(line: &str, p: Palette) -> Vec<(Range<usize>, Color)> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//")
        || trimmed.starts_with('#')
        || trimmed.starts_with("/*")
        || trimmed.starts_with('*')
    {
        return vec![(0..line.len(), p.text_muted)];
    }
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            out.push((i..line.len(), p.text_muted));
            break;
        }
        let b = bytes[i];
        if b == b'"' || b == b'\'' || b == b'`' {
            let quote = b;
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                if bytes[i] == quote {
                    i += 1;
                    break;
                }
                let ch = line[i..].chars().next().unwrap();
                i += ch.len_utf8();
            }
            out.push((start..i, p.success_fg));
        } else if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &line[start..i];
            if matches!(
                word,
                "fn" | "pub"
                    | "let"
                    | "mut"
                    | "const"
                    | "struct"
                    | "enum"
                    | "impl"
                    | "trait"
                    | "use"
                    | "mod"
                    | "match"
                    | "if"
                    | "else"
                    | "for"
                    | "while"
                    | "loop"
                    | "return"
                    | "async"
                    | "await"
                    | "func"
                    | "package"
                    | "import"
                    | "type"
                    | "interface"
                    | "var"
                    | "defer"
                    | "go"
                    | "select"
                    | "case"
                    | "default"
                    | "def"
                    | "class"
                    | "from"
                    | "export"
                    | "function"
                    | "true"
                    | "false"
                    | "nil"
                    | "null"
                    | "None"
                    | "Some"
                    | "Ok"
                    | "Err"
            ) {
                out.push((start..i, p.violet_fg));
            }
        } else if b.is_ascii_digit() {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.') {
                i += 1;
            }
            out.push((start..i, p.warning_fg));
        } else {
            let ch = line[i..].chars().next().unwrap();
            i += ch.len_utf8();
        }
    }
    out
}

pub enum PreviewState {
    Empty,
    Markdown {
        items: Vec<markdown::Item>,
        raw: String,
        cache: PreviewCodeCache,
    },
    Image {
        path: PathBuf,
        size_bytes: u64,
    },
    Svg {
        path: PathBuf,
        size_bytes: u64,
    },
    Json {
        pretty: String,
        valid: bool,
        error: Option<String>,
        cache: PreviewCodeCache,
    },
    Table {
        delimiter: char,
        headers: Vec<String>,
        col_widths: Vec<f32>,
        rows: Vec<Vec<String>>,
        total_rows: usize,
        raw: String,
        cache: PreviewCodeCache,
    },
    Config {
        kind: &'static str,
        content: String,
        cache: PreviewCodeCache,
    },
    Code {
        lang: String,
        content: String,
        cache: PreviewCodeCache,
    },
    Binary {
        ext: String,
        size_bytes: u64,
    },
}

impl Default for PreviewState {
    fn default() -> Self {
        Self::Empty
    }
}

impl PreviewState {
    /// Creates an independent deep copy of this preview state (re-parsing markdown items when
    /// needed) so detached popup windows stay completely decoupled from the host panel.
    pub fn clone_snapshot(&self) -> Self {
        match self {
            Self::Empty => Self::Empty,
            Self::Markdown { raw, cache, .. } => Self::Markdown {
                items: markdown::parse(raw).collect(),
                raw: raw.clone(),
                cache: cache.clone_with_language(),
            },
            Self::Image { path, size_bytes } => Self::Image {
                path: path.clone(),
                size_bytes: *size_bytes,
            },
            Self::Svg { path, size_bytes } => Self::Svg {
                path: path.clone(),
                size_bytes: *size_bytes,
            },
            Self::Json {
                pretty,
                valid,
                error,
                cache,
            } => Self::Json {
                pretty: pretty.clone(),
                valid: *valid,
                error: error.clone(),
                cache: cache.clone_with_language(),
            },
            Self::Table {
                delimiter,
                headers,
                col_widths,
                rows,
                total_rows,
                raw,
                cache,
            } => Self::Table {
                delimiter: *delimiter,
                headers: headers.clone(),
                col_widths: col_widths.clone(),
                rows: rows.clone(),
                total_rows: *total_rows,
                raw: raw.clone(),
                cache: cache.clone_with_language(),
            },
            Self::Config {
                kind,
                content,
                cache,
            } => Self::Config {
                kind,
                content: content.clone(),
                cache: cache.clone_with_language(),
            },
            Self::Code {
                lang,
                content,
                cache,
            } => Self::Code {
                lang: lang.clone(),
                content: content.clone(),
                cache: cache.clone_with_language(),
            },
            Self::Binary { ext, size_bytes } => Self::Binary {
                ext: ext.clone(),
                size_bytes: *size_bytes,
            },
        }
    }

    /// Builds `PreviewState` and pre-tokenizes syntax highlighting on the caller thread (ideal inside `spawn_blocking`).
    pub fn from_file_prewarmed(
        full_path: &Path,
        text_content: Option<&str>,
        theme_mode: ThemeMode,
    ) -> Self {
        let state = Self::from_file(full_path, text_content);
        let _ = state.highlighted_doc(theme_mode);
        state
    }

    pub fn from_file(full_path: &Path, text_content: Option<&str>) -> Self {
        let code_lang = CodeLanguage::from_path(full_path);
        let size_bytes = std::fs::metadata(full_path)
            .map(|m| m.len())
            .unwrap_or_else(|_| text_content.map(|s| s.len() as u64).unwrap_or(0));
        let file_name = full_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        let ext = full_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        // 1. Raster Images
        if matches!(
            ext.as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico"
        ) {
            return Self::Image {
                path: full_path.to_path_buf(),
                size_bytes,
            };
        }

        // 2. Vector SVG
        if ext == "svg" {
            return Self::Svg {
                path: full_path.to_path_buf(),
                size_bytes,
            };
        }

        let Some(raw) = text_content else {
            return Self::Binary { ext, size_bytes };
        };

        // 3. Markdown
        if matches!(ext.as_str(), "md" | "markdown" | "mdx") {
            let items: Vec<markdown::Item> = markdown::parse(raw).collect();
            return Self::Markdown {
                items,
                raw: raw.to_string(),
                cache: PreviewCodeCache::new(CodeLanguage::Markdown),
            };
        }

        // 4. JSON (fast zero-DOM streaming validation above 512 KB)
        if ext == "json" {
            if raw.len() > 512 * 1024 {
                return match serde_json::from_str::<serde::de::IgnoredAny>(raw) {
                    Ok(_) => Self::Json {
                        pretty: raw.to_string(),
                        valid: true,
                        error: None,
                        cache: PreviewCodeCache::new(CodeLanguage::Json),
                    },
                    Err(e) => Self::Json {
                        pretty: raw.to_string(),
                        valid: false,
                        error: Some(e.to_string()),
                        cache: PreviewCodeCache::new(CodeLanguage::Json),
                    },
                };
            }
            return match serde_json::from_str::<serde_json::Value>(raw) {
                Ok(val) => {
                    let pretty =
                        serde_json::to_string_pretty(&val).unwrap_or_else(|_| raw.to_string());
                    Self::Json {
                        pretty,
                        valid: true,
                        error: None,
                        cache: PreviewCodeCache::new(CodeLanguage::Json),
                    }
                }
                Err(e) => Self::Json {
                    pretty: raw.to_string(),
                    valid: false,
                    error: Some(e.to_string()),
                    cache: PreviewCodeCache::new(CodeLanguage::Json),
                },
            };
        }

        // 5. CSV / TSV Table (RFC-4180 compliant, balanced column widths)
        if ext == "csv" || ext == "tsv" {
            let path_str = full_path.to_string_lossy();
            let delimiter = crate::views::csv_diff::detect_delimiter(&path_str, raw);
            let records = crate::views::csv_diff::parse_csv_records(raw, delimiter);
            let (raw_headers, body_slice) = if records.is_empty() {
                (Vec::new(), &records[..])
            } else {
                (records[0].clone(), &records[1..])
            };
            let col_count = raw_headers
                .len()
                .max(body_slice.iter().map(|r| r.len()).max().unwrap_or(0))
                .max(1);
            let sanitize = |s: &str| -> String {
                let mut out = String::with_capacity(s.len());
                for ch in s.chars() {
                    match ch {
                        '\r' => {}
                        '\n' => out.push_str(" ↵ "),
                        '\t' => out.push_str("  "),
                        c if c.is_control() => {}
                        c => out.push(c),
                    }
                }
                out
            };
            let pad = |cells: &[String]| -> Vec<String> {
                (0..col_count)
                    .map(|i| cells.get(i).map(|s| sanitize(s)).unwrap_or_default())
                    .collect()
            };
            let headers: Vec<String> = if raw_headers.is_empty() {
                (1..=col_count).map(|i| format!("col_{i}")).collect()
            } else {
                pad(&raw_headers)
            };
            let total_rows = body_slice.len();
            let rows: Vec<Vec<String>> = body_slice.iter().take(1000).map(|r| pad(r)).collect();

            let mut max_chars: Vec<usize> =
                headers.iter().map(|h| h.chars().count().max(4)).collect();
            for r in rows.iter().take(400) {
                for (i, cell) in r.iter().enumerate() {
                    if let Some(slot) = max_chars.get_mut(i) {
                        *slot = (*slot).max(cell.chars().count());
                    }
                }
            }
            let col_widths: Vec<f32> = max_chars
                .into_iter()
                .map(|chars| ((chars as f32) * 8.1 + 22.0).clamp(80.0, 280.0))
                .collect();

            return Self::Table {
                delimiter,
                headers,
                col_widths,
                rows,
                total_rows,
                raw: raw.to_string(),
                cache: PreviewCodeCache::new(CodeLanguage::PlainText),
            };
        }

        // 6. Structured Configs (YAML, TOML, ENV, INI)
        if matches!(ext.as_str(), "yml" | "yaml" | "toml" | "ini" | "cfg")
            || file_name.starts_with(".env")
        {
            let kind = match ext.as_str() {
                "yml" | "yaml" => "YAML",
                "toml" => "TOML",
                "ini" | "cfg" => "INI",
                _ => "ENV",
            };
            return Self::Config {
                kind,
                content: raw.to_string(),
                cache: PreviewCodeCache::new(code_lang),
            };
        }

        // 7. Code / Text fallback
        let lang = if ext.is_empty() {
            "TEXT".to_string()
        } else {
            ext.to_uppercase()
        };
        Self::Code {
            lang,
            content: raw.to_string(),
            cache: PreviewCodeCache::new(code_lang),
        }
    }

    pub fn highlighted_doc(&self, theme_mode: ThemeMode) -> Option<Arc<HighlightedPreviewDoc>> {
        match self {
            Self::Json { pretty, cache, .. } => Some(cache.get_or_build(pretty, theme_mode)),
            Self::Config { content, cache, .. } => Some(cache.get_or_build(content, theme_mode)),
            Self::Code { content, cache, .. } => Some(cache.get_or_build(content, theme_mode)),
            Self::Markdown { raw, cache, .. } => Some(cache.get_or_build(raw, theme_mode)),
            Self::Table { raw, cache, .. } => Some(cache.get_or_build(raw, theme_mode)),
            Self::Empty | Self::Image { .. } | Self::Svg { .. } | Self::Binary { .. } => None,
        }
    }

    pub fn kind_badge(&self) -> &'static str {
        match self {
            Self::Empty => "",
            Self::Markdown { .. } => "Markdown Preview",
            Self::Image { .. } => "Image Preview",
            Self::Svg { .. } => "SVG Preview",
            Self::Json { .. } => "JSON Tree",
            Self::Table { .. } => "Table Preview",
            Self::Config { kind, .. } => kind,
            Self::Code { .. } => "Code Preview",
            Self::Binary { .. } => "Binary File",
        }
    }

    pub fn copyable_text(&self) -> Option<&str> {
        match self {
            Self::Json { pretty, .. } => Some(pretty.as_str()),
            Self::Config { content, .. } => Some(content.as_str()),
            Self::Code { content, .. } => Some(content.as_str()),
            Self::Markdown { raw, .. } => Some(raw.as_str()),
            Self::Table { raw, .. } => Some(raw.as_str()),
            Self::Empty | Self::Image { .. } | Self::Svg { .. } | Self::Binary { .. } => None,
        }
    }

    pub fn has_rich_view(&self) -> bool {
        matches!(self, Self::Markdown { .. } | Self::Table { .. })
    }

    pub fn uses_selectable_editor(&self, raw_mode: bool) -> bool {
        match self {
            Self::Json { .. } | Self::Config { .. } | Self::Code { .. } => true,
            Self::Markdown { .. } | Self::Table { .. } => raw_mode,
            _ => false,
        }
    }

    pub fn preferred_min_width(&self) -> f32 {
        match self {
            Self::Table { col_widths, .. } => {
                (44.0 + col_widths.iter().sum::<f32>() + 24.0).max(420.0)
            }
            _ => 380.0,
        }
    }

    pub fn set_code_language(&mut self, lang: CodeLanguage) {
        match self {
            Self::Config { cache, .. }
            | Self::Code { cache, .. }
            | Self::Json { cache, .. }
            | Self::Markdown { cache, .. }
            | Self::Table { cache, .. } => cache.set_language(lang),
            Self::Empty | Self::Image { .. } | Self::Svg { .. } | Self::Binary { .. } => {}
        }
    }

    #[allow(dead_code)]
    pub fn highlight_kind(&self) -> PreviewHighlightKind {
        match self {
            Self::Json { .. } => PreviewHighlightKind::Json,
            Self::Config { .. } => PreviewHighlightKind::Config,
            Self::Code { .. } => PreviewHighlightKind::Code,
            _ => PreviewHighlightKind::Plain,
        }
    }

    pub fn view<'a, M: 'a>(
        &'a self,
        p: Palette,
        mode: ThemeMode,
        on_link: impl Fn(String) -> M + 'a,
    ) -> Element<'a, M> {
        match self {
            Self::Empty => container(
                text("Select a file (.md, .png/.jpg/.svg, .json, .csv, .yaml/.toml/.env, or code) to preview.")
                    .size(type_scale::BODY)
                    .color(p.text_muted),
            )
            .padding(14)
            .into(),

            Self::Markdown { items, .. } => {
                let md_style = markdown::Style::from_palette(mode.iced_theme().palette());
                let md_view = markdown::view(items, markdown::Settings::default(), md_style)
                    .map(move |url| on_link(url.to_string()));
                container(md_view)
                    .padding([14, 20])
                    .width(Length::Fill)
                    .style(theme::card_box(p))
                    .into()
            }

            Self::Image { path, size_bytes } => container(
                column![
                    row![
                        text(format!("Image  •  {}", format_bytes(*size_bytes)))
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                        Space::with_width(Length::Fill),
                        text(path.display().to_string())
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                    ]
                    .align_y(Alignment::Center),
                    horizontal_rule(1).style(theme::subtle_rule(p)),
                    container(
                        image(image::Handle::from_path(path))
                            .content_fit(ContentFit::Contain)
                            .width(Length::Fill)
                            .height(Length::Fixed(340.0)),
                    )
                    .padding(12)
                    .center_x(Length::Fill)
                    .style(theme::card_box(p)),
                ]
                .spacing(8),
            )
            .padding(8)
            .width(Length::Fill)
            .into(),

            Self::Svg { path, size_bytes } => container(
                column![
                    row![
                        text(format!("SVG Vector  •  {}", format_bytes(*size_bytes)))
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                        Space::with_width(Length::Fill),
                        text(path.display().to_string())
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                    ]
                    .align_y(Alignment::Center),
                    horizontal_rule(1).style(theme::subtle_rule(p)),
                    container(
                        svg(svg::Handle::from_path(path))
                            .content_fit(ContentFit::Contain)
                            .width(Length::Fixed(320.0))
                            .height(Length::Fixed(320.0)),
                    )
                    .padding(16)
                    .center_x(Length::Fill)
                    .width(Length::Fill)
                    .style(theme::card_box(p)),
                ]
                .spacing(8),
            )
            .padding(8)
            .width(Length::Fill)
            .into(),

            Self::Json {
                pretty,
                valid,
                error,
                ..
            } => {
                let mut col = column![].spacing(3);
                if let Some(err) = error {
                    col = col.push(
                        container(
                            text(format!("Invalid JSON: {err}"))
                                .size(type_scale::CAPTION)
                                .color(p.danger_fg),
                        )
                        .padding([4, 8])
                        .width(Length::Fill)
                        .style(theme::pill_box(p.danger_bg, p.danger_fg, p.danger_fg)),
                    );
                } else if *valid {
                    col = col.push(
                        text("Valid JSON — formatted")
                            .size(type_scale::CAPTION)
                            .color(p.success_fg),
                    );
                }

                for (idx, line) in pretty.lines().take(1200).enumerate() {
                    let trimmed = line.trim();
                    let color = if trimmed.contains("\":") {
                        p.accent_fg
                    } else if trimmed.starts_with('"') {
                        p.success_fg
                    } else if trimmed.starts_with("true")
                        || trimmed.starts_with("false")
                        || trimmed.starts_with("null")
                    {
                        p.violet_fg
                    } else if trimmed
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_digit() || c == '-')
                    {
                        p.warning_fg
                    } else {
                        p.text_secondary
                    };
                    col = col.push(
                        row![
                            text(format!("{:>4}", idx + 1))
                                .size(type_scale::CAPTION)
                                .font(theme::CODE_FONT)
                                .color(p.text_muted),
                            text(theme::sanitize_code_line(line))
                                .size(type_scale::CODE)
                                .font(theme::CODE_FONT)
                                .color(color)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(12),
                    );
                }
                container(col).padding(8).width(Length::Fill).into()
            }

            Self::Table {
                delimiter,
                headers,
                col_widths,
                rows,
                total_rows,
                ..
            } => {
                const GUTTER_W: f32 = 44.0;
                const ROW_H: f32 = 25.0;
                let format_tag = if *delimiter == '\t' { "TSV" } else { "CSV" };
                let delim_label = match *delimiter {
                    '\t' => "Tab",
                    ';' => "Semicolon",
                    _ => "Comma",
                };
                let grid_line = Color {
                    a: 0.55,
                    ..p.border_subtle
                };

                let meta_bar = container(
                    row![
                        container(
                            text(format_tag)
                                .size(type_scale::BADGE)
                                .font(theme::CODE_FONT)
                                .color(p.accent_fg)
                                .wrapping(text::Wrapping::None)
                        )
                        .padding([1, 6])
                        .style(theme::pill_box(p.fill_selected, p.accent_fg, p.border_default)),
                        text(format!(
                            "{} columns × {} rows  ·  Delimiter: {delim_label}",
                            headers.len(),
                            total_rows
                        ))
                        .size(type_scale::CAPTION)
                        .color(p.text_muted)
                        .wrapping(text::Wrapping::None),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding([4, 8]);

                let make_row = |row_no: Option<usize>,
                                cells: &[String],
                                row_bg: Color,
                                fg: Color,
                                border_col: Color,
                                is_header: bool|
                 -> Element<'a, M> {
                    let row_h = if is_header { ROW_H + 2.0 } else { ROW_H };
                    let gutter_label = row_no
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| if is_header { "#".into() } else { String::new() });
                    let mut r = row![
                        container(
                            text(gutter_label)
                                .size(type_scale::CAPTION)
                                .font(theme::CODE_FONT)
                                .color(p.text_muted)
                                .wrapping(text::Wrapping::None),
                        )
                        .width(Length::Fixed(GUTTER_W))
                        .height(Length::Fixed(row_h))
                        .padding([0, 6])
                        .align_x(alignment::Horizontal::Right)
                        .align_y(alignment::Vertical::Center),
                    ]
                    .spacing(0)
                    .align_y(Alignment::Center);

                    for (idx, &w) in col_widths.iter().enumerate() {
                        let val = cells.get(idx).cloned().unwrap_or_default();
                        let cell_text = theme::clipped_line(
                            text(val)
                                .size(if is_header {
                                    type_scale::CAPTION
                                } else {
                                    type_scale::CODE
                                })
                                .font(theme::CODE_FONT)
                                .color(fg)
                                .wrapping(text::Wrapping::None),
                            Length::Fill,
                        );
                        r = r.push(
                            container(cell_text)
                                .width(Length::Fixed(w))
                                .height(Length::Fixed(row_h))
                                .padding([0, 9])
                                .align_y(alignment::Vertical::Center),
                        );
                    }

                    container(r)
                        .width(Length::Fill)
                        .height(Length::Fixed(row_h))
                        .style(move |_| container::Style {
                            background: Some(Background::Color(row_bg)),
                            border: Border {
                                color: border_col,
                                width: 0.5,
                                radius: 0.0.into(),
                            },
                            ..Default::default()
                        })
                        .into()
                };

                let mut tbl = column![
                    meta_bar,
                    make_row(
                        None,
                        headers,
                        p.surface_titlebar,
                        p.text_primary,
                        p.border_strong,
                        true,
                    ),
                ]
                .spacing(0);

                for (i, r) in rows.iter().enumerate() {
                    let bg = if i % 2 == 0 {
                        p.surface_panel
                    } else {
                        Color {
                            a: 0.65,
                            ..p.surface_card
                        }
                    };
                    tbl = tbl.push(make_row(
                        Some(i + 2),
                        r,
                        bg,
                        p.text_secondary,
                        grid_line,
                        false,
                    ));
                }

                if *total_rows > rows.len() {
                    tbl = tbl.push(
                        container(
                            text(format!(
                                "Showing first {} of {} rows — click 'Select Text' to inspect raw CSV.",
                                rows.len(),
                                total_rows
                            ))
                            .size(type_scale::CAPTION)
                            .color(p.warning_fg),
                        )
                        .padding([8, 12])
                        .width(Length::Fill)
                        .style(theme::section_header_box(p)),
                    );
                }

                tbl = tbl.push(Space::with_height(Length::Fixed(14.0)));
                tbl.width(Length::Fill).into()
            }

            Self::Config { kind, content, .. } => {
                let mut col = column![
                    text(format!("{kind} Structured Preview"))
                        .size(type_scale::CAPTION)
                        .color(p.text_muted),
                ]
                .spacing(2);

                for (idx, line) in content.lines().take(1200).enumerate() {
                    let clean = theme::sanitize_code_line(line);
                    let trimmed = clean.trim();
                    let line_view: Element<'a, M> = if trimmed.starts_with('#') || trimmed.starts_with(';') {
                        text(clean)
                            .size(type_scale::CODE)
                            .font(theme::CODE_FONT)
                            .color(p.text_muted)
                            .wrapping(text::Wrapping::None)
                            .into()
                    } else if trimmed.starts_with('[') && trimmed.ends_with(']') {
                        text(clean)
                            .size(type_scale::CODE)
                            .font(theme::CODE_FONT)
                            .color(p.violet_fg)
                            .wrapping(text::Wrapping::None)
                            .into()
                    } else if let Some((k, v)) = clean.split_once('=').or_else(|| clean.split_once(':')) {
                        let sep = if clean.contains('=') && (!clean.contains(':') || clean.find('=') < clean.find(':')) {
                            "="
                        } else {
                            ":"
                        };
                        row![
                            text(k.to_string())
                                .size(type_scale::CODE)
                                .font(theme::CODE_FONT)
                                .color(p.accent_fg),
                            text(sep)
                                .size(type_scale::CODE)
                                .font(theme::CODE_FONT)
                                .color(p.text_muted),
                            text(v.to_string())
                                .size(type_scale::CODE)
                                .font(theme::CODE_FONT)
                                .color(p.success_fg),
                        ]
                        .into()
                    } else {
                        text(clean)
                            .size(type_scale::CODE)
                            .font(theme::CODE_FONT)
                            .color(p.text_primary)
                            .wrapping(text::Wrapping::None)
                            .into()
                    };

                    col = col.push(
                        row![
                            text(format!("{:>4}", idx + 1))
                                .size(type_scale::CAPTION)
                                .font(theme::CODE_FONT)
                                .color(p.text_muted),
                            line_view,
                        ]
                        .spacing(12),
                    );
                }
                container(col).padding(8).width(Length::Fill).into()
            }

            Self::Code { lang, content, .. } => {
                let mut col = column![
                    text(format!("{lang}  •  {} lines", content.lines().count()))
                        .size(type_scale::CAPTION)
                        .color(p.text_muted),
                ]
                .spacing(1);
                for (idx, line) in content.lines().take(1500).enumerate() {
                    let trimmed = line.trim_start();
                    let color = if trimmed.starts_with("//")
                        || trimmed.starts_with('#')
                        || trimmed.starts_with("/*")
                        || trimmed.starts_with('*')
                    {
                        p.text_muted
                    } else if trimmed.starts_with("pub ")
                        || trimmed.starts_with("fn ")
                        || trimmed.starts_with("struct ")
                        || trimmed.starts_with("enum ")
                        || trimmed.starts_with("impl ")
                        || trimmed.starts_with("use ")
                        || trimmed.starts_with("import ")
                        || trimmed.starts_with("export ")
                        || trimmed.starts_with("function ")
                        || trimmed.starts_with("class ")
                        || trimmed.starts_with("def ")
                    {
                        p.accent_fg
                    } else {
                        p.text_primary
                    };
                    col = col.push(
                        row![
                            text(format!("{:>4}", idx + 1))
                                .size(type_scale::CAPTION)
                                .font(theme::CODE_FONT)
                                .color(p.text_muted),
                            text(theme::sanitize_code_line(line))
                                .size(type_scale::CODE)
                                .font(theme::CODE_FONT)
                                .color(color)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(12),
                    );
                }
                container(col).padding(8).width(Length::Fill).into()
            }

            Self::Binary { ext, size_bytes } => container(
                column![
                    text("Binary File")
                        .size(type_scale::HEADING)
                        .color(p.text_primary),
                    text(format!(
                        "Extension: .{}  •  Size: {}",
                        if ext.is_empty() { "bin" } else { ext.as_str() },
                        format_bytes(*size_bytes)
                    ))
                    .size(type_scale::BODY)
                    .color(p.text_muted),
                ]
                .spacing(6),
            )
            .padding(20)
            .style(theme::card_box(p))
            .into(),
        }
    }
}

fn format_bytes(b: u64) -> String {
    if b < 1024 {
        format!("{b} B")
    } else if b < 1024 * 1024 {
        format!("{:.1} KB", b as f64 / 1024.0)
    } else {
        format!("{:.2} MB", b as f64 / (1024.0 * 1024.0))
    }
}
