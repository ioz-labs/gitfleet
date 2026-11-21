use crate::theme::{self, Palette, ThemeMode, type_scale};
use iced::advanced::text::Highlighter as TextHighlighter;
use iced::widget::{Space, column, container, horizontal_rule, image, markdown, row, svg, text};
use iced::{Alignment, Color, ContentFit, Element, Length};
use std::ops::Range;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewHighlightKind {
    Plain,
    Json,
    Config,
    Code,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewHighlightSettings {
    pub kind: PreviewHighlightKind,
    pub theme_mode: ThemeMode,
}

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
    },
    Table {
        delimiter: char,
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
        total_rows: usize,
        raw: String,
    },
    Config {
        kind: &'static str,
        content: String,
    },
    Code {
        lang: String,
        content: String,
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
    pub fn from_file(full_path: &Path, text_content: Option<&str>) -> Self {
        let size_bytes = std::fs::metadata(full_path).map(|m| m.len()).unwrap_or(0);
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
            };
        }

        // 4. JSON
        if ext == "json" {
            return match serde_json::from_str::<serde_json::Value>(raw) {
                Ok(val) => {
                    let pretty =
                        serde_json::to_string_pretty(&val).unwrap_or_else(|_| raw.to_string());
                    Self::Json {
                        pretty,
                        valid: true,
                        error: None,
                    }
                }
                Err(e) => Self::Json {
                    pretty: raw.to_string(),
                    valid: false,
                    error: Some(e.to_string()),
                },
            };
        }

        // 5. CSV / TSV Table
        if ext == "csv" || ext == "tsv" {
            let delimiter = if ext == "tsv" { '\t' } else { ',' };
            let mut lines = raw.lines().filter(|l| !l.trim().is_empty());
            let headers = lines
                .next()
                .map(|h| parse_delimited_row(h, delimiter))
                .unwrap_or_default();
            let all_rows: Vec<Vec<String>> = lines
                .map(|l| parse_delimited_row(l, delimiter))
                .collect();
            let total_rows = all_rows.len();
            let rows = all_rows.into_iter().take(200).collect();
            return Self::Table {
                delimiter,
                headers,
                rows,
                total_rows,
                raw: raw.to_string(),
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
                rows,
                total_rows,
                ..
            } => {
                let mut tbl = column![
                    text(format!(
                        "{} Table  •  {} columns × {} rows",
                        if *delimiter == '\t' { "TSV" } else { "CSV" },
                        headers.len(),
                        total_rows
                    ))
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
                ]
                .spacing(4);

                let mut header_row = row![].spacing(4);
                for h in headers {
                    header_row = header_row.push(
                        container(
                            text(h.clone())
                                .size(type_scale::CAPTION)
                                .color(p.accent_fg),
                        )
                        .width(Length::Fixed(152.0))
                        .padding([4, 8])
                        .style(theme::diff_line_box(p.fill_selected, p.accent_fg)),
                    );
                }
                tbl = tbl.push(header_row);

                for (i, r) in rows.iter().enumerate() {
                    let bg = if i % 2 == 0 {
                        p.surface_panel
                    } else {
                        p.surface_card
                    };
                    let mut r_ui = row![].spacing(4);
                    for cell in r {
                        r_ui = r_ui.push(
                            container(
                                text(cell.clone())
                                    .size(type_scale::CAPTION)
                                    .color(p.text_primary),
                            )
                            .width(Length::Fixed(152.0))
                            .padding([4, 8])
                            .style(theme::diff_line_box(bg, p.text_primary)),
                        );
                    }
                    tbl = tbl.push(r_ui);
                }
                container(tbl).padding(8).width(Length::Fill).into()
            }

            Self::Config { kind, content } => {
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

            Self::Code { lang, content } => {
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

fn parse_delimited_row(line: &str, delim: char) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for ch in line.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if ch == delim && !in_quotes {
            cells.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(ch);
        }
    }
    cells.push(cur.trim().to_string());
    cells
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
