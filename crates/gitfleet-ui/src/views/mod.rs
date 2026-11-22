mod branches;
pub(crate) mod code_editor;
pub(crate) mod commit_files;
mod conflict_solver;
pub(crate) mod csv_diff;
pub(crate) mod detached_viewer;
pub(crate) mod diff_viewer;
mod dock;
mod graph_pane;
mod host;
pub(crate) mod image_diff;
pub(crate) mod overlays;
mod repositories;
mod status_bar;

use crate::app::{GitFleetApp, Message, WindowKind};
use crate::theme::{self, Palette};
use iced::widget::{container, scrollable, text, tooltip};
use iced::{Element, Length, window};

impl GitFleetApp {
    pub(crate) fn view(&self, window_id: window::Id) -> Element<'_, Message> {
        let p = self.theme_mode.palette();
        match self.windows.get(&window_id) {
            Some(WindowKind::DetachedViewer) => self.view_detached_viewer(window_id, p),
            _ => self.view_host(p),
        }
    }
}

pub(crate) fn tip<'a>(
    content: impl Into<Element<'a, Message>>,
    label: impl text::IntoFragment<'a>,
    position: tooltip::Position,
    p: Palette,
) -> Element<'a, Message> {
    tooltip(
        content,
        container(
            text(label)
                .size(theme::type_scale::CAPTION)
                .color(p.text_primary),
        )
        .padding([5, 9])
        .style(theme::tooltip_box(p)),
        position,
    )
    .gap(5)
    .into()
}

pub(crate) fn slim_scroll<'a>(
    content: impl Into<Element<'a, Message>>,
    p: Palette,
) -> scrollable::Scrollable<'a, Message> {
    scrollable(content)
        .width(Length::Fill)
        .direction(theme::slim_vertical_direction())
        .style(theme::slim_scrollable(p))
}

/// Vertical scrollable with an overlay scrollbar (0px reserved right gutter when content fits)
/// so context menus and popup cards keep symmetrical start/end horizontal padding.
pub(crate) fn slim_scroll_overlay<'a>(
    content: impl Into<Element<'a, Message>>,
    p: Palette,
) -> scrollable::Scrollable<'a, Message> {
    scrollable(content)
        .width(Length::Fill)
        .direction(theme::slim_overlay_vertical_direction())
        .style(theme::slim_scrollable(p))
}

pub(crate) fn slim_scroll_h<'a>(
    content: impl Into<Element<'a, Message>>,
    min_width: f32,
    p: Palette,
) -> Element<'a, Message> {
    theme::responsive_hscroll(content, min_width, p)
}

pub(crate) fn truncate_chars(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

pub(crate) fn parent_dir(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some((dir, _)) => dir,
        None => "",
    }
}
