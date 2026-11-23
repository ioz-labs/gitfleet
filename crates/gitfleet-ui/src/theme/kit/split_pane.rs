//! `HairlineSplitter`: `7px` ergonomic hit zone painting a crisp `1px` hairline border that
//! illuminates to `2px` accent and switches to native resize cursor (`↔` / `↕`) on hover or drag.
#![allow(dead_code)]

use crate::theme::palette::Palette;
use crate::theme::tokens::layout as tok_layout;
use iced::advanced::layout as adv_layout;
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Renderer as _, Shell, Widget};
use iced::event::{self, Event};
use iced::mouse;
use iced::{Background, Border, Element, Length, Rectangle, Renderer, Size, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitterAxis {
    /// Vertical divider line between left and right panes (`↔` horizontal resize cursor).
    Vertical,
    /// Horizontal divider line between top and bottom panes (`↕` vertical resize cursor).
    Horizontal,
}

struct HairlineSplitterWidget<M> {
    axis: SplitterAxis,
    p: Palette,
    dragging: bool,
    on_press: M,
}

impl<M: Clone> Widget<M, Theme, Renderer> for HairlineSplitterWidget<M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
    }

    fn state(&self) -> tree::State {
        tree::State::None
    }

    fn size(&self) -> Size<Length> {
        match self.axis {
            SplitterAxis::Vertical => {
                Size::new(Length::Fixed(tok_layout::SPLITTER_HIT_SIZE), Length::Fill)
            }
            SplitterAxis::Horizontal => {
                Size::new(Length::Fill, Length::Fixed(tok_layout::SPLITTER_HIT_SIZE))
            }
        }
    }

    fn layout(
        &self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &adv_layout::Limits,
    ) -> adv_layout::Node {
        let size = match self.axis {
            SplitterAxis::Vertical => limits.resolve(
                Length::Fixed(tok_layout::SPLITTER_HIT_SIZE),
                Length::Fill,
                Size::ZERO,
            ),
            SplitterAxis::Horizontal => limits.resolve(
                Length::Fill,
                Length::Fixed(tok_layout::SPLITTER_HIT_SIZE),
                Size::ZERO,
            ),
        };
        adv_layout::Node::new(size)
    }

    fn on_event(
        &mut self,
        _tree: &mut Tree,
        event: Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        _viewport: &Rectangle,
    ) -> event::Status {
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && cursor.is_over(layout.bounds())
        {
            shell.publish(self.on_press.clone());
            return event::Status::Captured;
        }
        event::Status::Ignored
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if self.dragging || cursor.is_over(layout.bounds()) {
            match self.axis {
                SplitterAxis::Vertical => mouse::Interaction::ResizingHorizontally,
                SplitterAxis::Horizontal => mouse::Interaction::ResizingVertically,
            }
        } else {
            mouse::Interaction::None
        }
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        if bounds.intersection(viewport).is_none() {
            return;
        }
        let hovered = cursor.is_over(bounds);
        let active = self.dragging || hovered;

        // Paint surrounding surface under the 7px hit area so adjacent panes touch seamlessly
        renderer.fill_quad(
            Quad {
                bounds,
                border: Border::default(),
                shadow: Default::default(),
            },
            Background::Color(self.p.surface_panel),
        );

        let thickness = if active {
            2.0
        } else {
            tok_layout::SPLITTER_LINE_SIZE
        };
        let color = if active {
            self.p.splitter_active
        } else {
            self.p.splitter_idle
        };

        let line_bounds = match self.axis {
            SplitterAxis::Vertical => Rectangle {
                x: (bounds.x + (bounds.width - thickness) * 0.5).round(),
                y: bounds.y,
                width: thickness,
                height: bounds.height,
            },
            SplitterAxis::Horizontal => Rectangle {
                x: bounds.x,
                y: (bounds.y + (bounds.height - thickness) * 0.5).round(),
                width: bounds.width,
                height: thickness,
            },
        };

        renderer.fill_quad(
            Quad {
                bounds: line_bounds,
                border: Border::default(),
                shadow: Default::default(),
            },
            Background::Color(color),
        );
    }
}

/// Vertical `1px` hairline splitter (`7px` hit zone, `↔` resize cursor) between left & right panes.
pub fn hairline_v_splitter<'a, M: Clone + 'a>(
    p: Palette,
    dragging: bool,
    on_press: M,
) -> Element<'a, M> {
    Element::new(HairlineSplitterWidget {
        axis: SplitterAxis::Vertical,
        p,
        dragging,
        on_press,
    })
}

/// Horizontal `1px` hairline splitter (`7px` hit zone, `↕` resize cursor) between top & bottom panes.
pub fn hairline_h_splitter<'a, M: Clone + 'a>(
    p: Palette,
    dragging: bool,
    on_press: M,
) -> Element<'a, M> {
    Element::new(HairlineSplitterWidget {
        axis: SplitterAxis::Horizontal,
        p,
        dragging,
        on_press,
    })
}
