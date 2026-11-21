#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermColor {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl From<vt100::Color> for TermColor {
    fn from(value: vt100::Color) -> Self {
        match value {
            vt100::Color::Default => Self::Default,
            vt100::Color::Idx(i) => Self::Indexed(i),
            vt100::Color::Rgb(r, g, b) => Self::Rgb(r, g, b),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermRun {
    pub col: u16,
    pub cell_width: u16,
    pub text: String,
    pub fg: TermColor,
    pub bg: TermColor,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    /// Single non-ASCII glyph (Nerd Font icon, Powerline cap, box drawing, wide char) pinned to exact grid col.
    pub is_glyph: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TermRow {
    pub runs: Vec<TermRun>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TermScreenSnapshot {
    pub rows: u16,
    pub cols: u16,
    pub lines: Vec<TermRow>,
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub cursor_visible: bool,
    pub scrollback_offset: usize,
    pub alternate_screen: bool,
    pub exited: bool,
    pub dirty_seq: u64,
}

pub fn extract_screen_snapshot(
    screen: &vt100::Screen,
    dirty_seq: u64,
    exited: bool,
) -> TermScreenSnapshot {
    let (rows, cols) = screen.size();
    let mut lines = Vec::with_capacity(rows as usize);

    for r in 0..rows {
        let mut runs: Vec<TermRun> = Vec::new();
        let mut c: u16 = 0;
        while c < cols {
            let Some(cell) = screen.cell(r, c) else {
                c += 1;
                continue;
            };
            if cell.is_wide_continuation() {
                c += 1;
                continue;
            }
            let width: u16 = if cell.is_wide() { 2 } else { 1 };
            let raw = cell.contents();
            let is_empty = raw.is_empty();
            let fg: TermColor = cell.fgcolor().into();
            let bg: TermColor = cell.bgcolor().into();
            let bold = cell.bold();
            let italic = cell.italic();
            let underline = cell.underline();
            let inverse = cell.inverse();

            let has_visible_style = bg != TermColor::Default || inverse || underline;
            if is_empty && !has_visible_style {
                c += width;
                continue;
            }

            let cell_str = if is_empty { " " } else { raw.as_str() };
            let is_glyph = !cell_str.is_ascii();

            if !is_glyph
                && let Some(last) = runs.last_mut()
                && !last.is_glyph
                && last.col + last.cell_width == c
                && last.fg == fg
                && last.bg == bg
                && last.bold == bold
                && last.italic == italic
                && last.underline == underline
                && last.inverse == inverse
            {
                last.text.push_str(cell_str);
                last.cell_width += width;
            } else {
                runs.push(TermRun {
                    col: c,
                    cell_width: width,
                    text: cell_str.to_string(),
                    fg,
                    bg,
                    bold,
                    italic,
                    underline,
                    inverse,
                    is_glyph,
                });
            }
            c += width;
        }
        lines.push(TermRow { runs });
    }

    let (cursor_row, cursor_col) = screen.cursor_position();
    TermScreenSnapshot {
        rows,
        cols,
        lines,
        cursor_row,
        cursor_col,
        cursor_visible: !screen.hide_cursor() && screen.scrollback() == 0 && !exited,
        scrollback_offset: screen.scrollback(),
        alternate_screen: screen.alternate_screen(),
        exited,
        dirty_seq,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_truecolor_runs_and_nerd_glyphs() {
        let mut parser = vt100::Parser::new(4, 40, 100);
        parser.process(b"\x1b[38;2;255;255;255m\x1b[48;2;160;100;220m quach \x1b[0m\xee\x82\xb4");
        let snap = extract_screen_snapshot(parser.screen(), 7, false);
        assert_eq!(snap.dirty_seq, 7);
        let row = &snap.lines[0];
        assert!(row.runs.len() >= 2);
        assert_eq!(row.runs[0].text, " quach ");
        assert_eq!(row.runs[0].fg, TermColor::Rgb(255, 255, 255));
        assert_eq!(row.runs[0].bg, TermColor::Rgb(160, 100, 220));
        assert!(!row.runs[0].is_glyph);
        assert_eq!(row.runs[1].text, "\u{e0b4}");
        assert!(row.runs[1].is_glyph);
        assert_eq!(row.runs[1].col, 7);
    }
}
