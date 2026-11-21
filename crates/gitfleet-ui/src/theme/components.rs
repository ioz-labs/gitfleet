//! Reusable GitFleet Design System UI primitives (modals, section cards, dock toolbars, badges, empty states).

use crate::icons;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use iced::advanced::layout as adv_layout;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, overlay, renderer};
use iced::event;
use iced::mouse;
use iced::widget::{
    Space, button, column, container, horizontal_rule, mouse_area, opaque, row, scrollable, text,
};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Padding, Point, Rectangle, Renderer,
    Shadow, Size, Theme, Vector,
};
use std::cell::Cell;
use std::rc::Rc;

/// Lays out its child with a generous finite horizontal budget (`4096px`) so Iced 0.13's
/// `Paragraph` never wraps single-line labels onto multiple lines in narrow resizable panes,
/// then clips the rendered output strictly to the widget's allocated bounds.
pub fn clipped_line<'a, M>(
    content: impl Into<Element<'a, M>>,
    width: Length,
) -> Element<'a, M>
where
    M: 'a,
{
    struct SingleLineClip<'a, M> {
        content: Element<'a, M>,
        width: Length,
    }

    impl<'a, M> Widget<M, Theme, Renderer> for SingleLineClip<'a, M> {
        fn tag(&self) -> tree::Tag {
            tree::Tag::stateless()
        }

        fn state(&self) -> tree::State {
            tree::State::None
        }

        fn children(&self) -> Vec<Tree> {
            vec![Tree::new(&self.content)]
        }

        fn diff(&self, tree: &mut Tree) {
            tree.diff_children(std::slice::from_ref(&self.content));
        }

        fn size(&self) -> Size<Length> {
            Size::new(self.width, Length::Shrink)
        }

        fn layout(
            &self,
            tree: &mut Tree,
            renderer: &Renderer,
            limits: &adv_layout::Limits,
        ) -> adv_layout::Node {
            let child_limits = adv_layout::Limits::new(
                Size::ZERO,
                Size::new(4096.0, limits.max().height),
            );
            let child_node =
                self.content
                    .as_widget()
                    .layout(&mut tree.children[0], renderer, &child_limits);
            let child_size = child_node.size();
            let resolved = limits.resolve(self.width, Length::Shrink, child_size);
            adv_layout::Node::with_children(resolved, vec![child_node])
        }

        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            if let Some(clipped) = layout.bounds().intersection(viewport)
                && let Some(child_layout) = layout.children().next()
            {
                let clipped_cursor = if cursor.is_over(layout.bounds()) {
                    cursor
                } else {
                    mouse::Cursor::Unavailable
                };
                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    child_layout,
                    clipped_cursor,
                    &clipped,
                );
            }
        }

        fn on_event(
            &mut self,
            tree: &mut Tree,
            event: iced::Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &Renderer,
            clipboard: &mut dyn Clipboard,
            shell: &mut Shell<'_, M>,
            viewport: &Rectangle,
        ) -> event::Status {
            let Some(child_layout) = layout.children().next() else {
                return event::Status::Ignored;
            };
            let clipped_cursor = if cursor.is_over(layout.bounds()) {
                cursor
            } else {
                mouse::Cursor::Unavailable
            };
            let clipped_vp = layout.bounds().intersection(viewport).unwrap_or(*viewport);
            self.content.as_widget_mut().on_event(
                &mut tree.children[0],
                event,
                child_layout,
                clipped_cursor,
                renderer,
                clipboard,
                shell,
                &clipped_vp,
            )
        }

        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            renderer: &Renderer,
        ) -> mouse::Interaction {
            if !cursor.is_over(layout.bounds()) {
                return mouse::Interaction::None;
            }
            let Some(child_layout) = layout.children().next() else {
                return mouse::Interaction::None;
            };
            let clipped_vp = layout.bounds().intersection(viewport).unwrap_or(*viewport);
            self.content.as_widget().mouse_interaction(
                &tree.children[0],
                child_layout,
                cursor,
                &clipped_vp,
                renderer,
            )
        }

        fn operate(
            &self,
            tree: &mut Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            operation: &mut dyn iced::advanced::widget::Operation,
        ) {
            if let Some(child_layout) = layout.children().next() {
                self.content.as_widget().operate(
                    &mut tree.children[0],
                    child_layout,
                    renderer,
                    operation,
                );
            }
        }

        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            translation: Vector,
        ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
            let child_layout = layout.children().next()?;
            self.content.as_widget_mut().overlay(
                &mut tree.children[0],
                child_layout,
                renderer,
                translation,
            )
        }
    }

    Element::new(SingleLineClip {
        content: content.into(),
        width,
    })
}

/// Wraps a shrink-sized tab label with a crisp bottom underline matching its exact measured width
/// (safe inside horizontal `scrollable` containers where `Length::Fill` would expand infinitely).
pub fn bottom_underline<'a, M>(
    content: impl Into<Element<'a, M>>,
    color: Color,
    thickness: f32,
) -> Element<'a, M>
where
    M: 'a,
{
    struct BottomUnderline<'a, M> {
        content: Element<'a, M>,
        color: Color,
        thickness: f32,
    }

    impl<'a, M> Widget<M, Theme, Renderer> for BottomUnderline<'a, M> {
        fn tag(&self) -> tree::Tag {
            tree::Tag::stateless()
        }

        fn state(&self) -> tree::State {
            tree::State::None
        }

        fn children(&self) -> Vec<Tree> {
            vec![Tree::new(&self.content)]
        }

        fn diff(&self, tree: &mut Tree) {
            tree.diff_children(std::slice::from_ref(&self.content));
        }

        fn size(&self) -> Size<Length> {
            Size::new(Length::Shrink, Length::Shrink)
        }

        fn layout(
            &self,
            tree: &mut Tree,
            renderer: &Renderer,
            limits: &adv_layout::Limits,
        ) -> adv_layout::Node {
            let child_node =
                self.content
                    .as_widget()
                    .layout(&mut tree.children[0], renderer, limits);
            let sz = child_node.size();
            adv_layout::Node::with_children(
                Size::new(sz.width, sz.height + self.thickness),
                vec![child_node],
            )
        }

        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            use iced::advanced::Renderer as _;

            if let Some(child_layout) = layout.children().next() {
                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    child_layout,
                    cursor,
                    viewport,
                );
            }
            if self.color != Color::TRANSPARENT {
                let b = layout.bounds();
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: b.x,
                            y: b.y + b.height - self.thickness,
                            width: b.width,
                            height: self.thickness,
                        },
                        border: Border::default(),
                        shadow: Shadow::default(),
                    },
                    Background::Color(self.color),
                );
            }
        }

        fn on_event(
            &mut self,
            tree: &mut Tree,
            event: iced::Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &Renderer,
            clipboard: &mut dyn Clipboard,
            shell: &mut Shell<'_, M>,
            viewport: &Rectangle,
        ) -> event::Status {
            if let Some(child_layout) = layout.children().next() {
                self.content.as_widget_mut().on_event(
                    &mut tree.children[0],
                    event,
                    child_layout,
                    cursor,
                    renderer,
                    clipboard,
                    shell,
                    viewport,
                )
            } else {
                event::Status::Ignored
            }
        }

        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            renderer: &Renderer,
        ) -> mouse::Interaction {
            if let Some(child_layout) = layout.children().next() {
                self.content.as_widget().mouse_interaction(
                    &tree.children[0],
                    child_layout,
                    cursor,
                    viewport,
                    renderer,
                )
            } else {
                mouse::Interaction::None
            }
        }

        fn operate(
            &self,
            tree: &mut Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            operation: &mut dyn iced::advanced::widget::Operation,
        ) {
            if let Some(child_layout) = layout.children().next() {
                self.content.as_widget().operate(
                    &mut tree.children[0],
                    child_layout,
                    renderer,
                    operation,
                );
            }
        }

        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            translation: Vector,
        ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
            let child_layout = layout.children().next()?;
            self.content.as_widget_mut().overlay(
                &mut tree.children[0],
                child_layout,
                renderer,
                translation,
            )
        }
    }

    Element::new(BottomUnderline {
        content: content.into(),
        color,
        thickness,
    })
}

/// Wraps a layer (such as the main workspace beneath popups/modals) so that while `inert` is
/// `true`, hover states, tooltips/overlays, pointer interactions, and clicks NEVER bleed through
/// overlays above it — while keeping a stable widget type at Stack index 0 so scroll positions
/// never reset when opening or closing popups.
pub fn inert_when<'a, M>(content: impl Into<Element<'a, M>>, inert: bool) -> Element<'a, M>
where
    M: 'a,
{
    struct InertWhen<'a, M> {
        content: Element<'a, M>,
        inert: bool,
    }

    impl<'a, M> Widget<M, Theme, Renderer> for InertWhen<'a, M> {
        fn tag(&self) -> tree::Tag {
            tree::Tag::stateless()
        }

        fn state(&self) -> tree::State {
            tree::State::None
        }

        fn children(&self) -> Vec<Tree> {
            vec![Tree::new(&self.content)]
        }

        fn diff(&self, tree: &mut Tree) {
            tree.diff_children(std::slice::from_ref(&self.content));
        }

        fn size(&self) -> Size<Length> {
            self.content.as_widget().size()
        }

        fn size_hint(&self) -> Size<Length> {
            self.content.as_widget().size_hint()
        }

        fn layout(
            &self,
            tree: &mut Tree,
            renderer: &Renderer,
            limits: &adv_layout::Limits,
        ) -> adv_layout::Node {
            let child = self
                .content
                .as_widget()
                .layout(&mut tree.children[0], renderer, limits);
            adv_layout::Node::with_children(child.size(), vec![child])
        }

        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            let eff_cursor = if self.inert {
                mouse::Cursor::Unavailable
            } else {
                cursor
            };
            if let Some(child_layout) = layout.children().next() {
                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    child_layout,
                    eff_cursor,
                    viewport,
                );
            }
        }

        fn on_event(
            &mut self,
            tree: &mut Tree,
            event: iced::Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &Renderer,
            clipboard: &mut dyn Clipboard,
            shell: &mut Shell<'_, M>,
            viewport: &Rectangle,
        ) -> event::Status {
            let Some(child_layout) = layout.children().next() else {
                return event::Status::Ignored;
            };
            if self.inert {
                if matches!(
                    event,
                    iced::Event::Mouse(
                        mouse::Event::CursorMoved { .. }
                            | mouse::Event::CursorLeft
                            | mouse::Event::CursorEntered
                            | mouse::Event::ButtonReleased(_)
                    ) | iced::Event::Window(_)
                ) {
                    let _ = self.content.as_widget_mut().on_event(
                        &mut tree.children[0],
                        event,
                        child_layout,
                        mouse::Cursor::Unavailable,
                        renderer,
                        clipboard,
                        shell,
                        viewport,
                    );
                }
                return event::Status::Ignored;
            }
            self.content.as_widget_mut().on_event(
                &mut tree.children[0],
                event,
                child_layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            )
        }

        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            renderer: &Renderer,
        ) -> mouse::Interaction {
            if self.inert {
                return mouse::Interaction::None;
            }
            let Some(child_layout) = layout.children().next() else {
                return mouse::Interaction::None;
            };
            self.content.as_widget().mouse_interaction(
                &tree.children[0],
                child_layout,
                cursor,
                viewport,
                renderer,
            )
        }

        fn operate(
            &self,
            tree: &mut Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            operation: &mut dyn iced::advanced::widget::Operation,
        ) {
            if let Some(child_layout) = layout.children().next() {
                self.content.as_widget().operate(
                    &mut tree.children[0],
                    child_layout,
                    renderer,
                    operation,
                );
            }
        }

        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            translation: Vector,
        ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
            if self.inert {
                return None;
            }
            let child_layout = layout.children().next()?;
            self.content.as_widget_mut().overlay(
                &mut tree.children[0],
                child_layout,
                renderer,
                translation,
            )
        }
    }

    Element::new(InertWhen {
        content: content.into(),
        inert,
    })
}

/// Modern pill toggle switch (38×20px) for Settings & Preferences rows.
pub fn toggle_switch<'a, M>(p: Palette, checked: bool, on_toggle: M) -> Element<'a, M>
where
    M: Clone + 'a,
{
    let thumb = container(Space::new(Length::Fixed(14.0), Length::Fixed(14.0))).style(
        move |_| container::Style {
            background: Some(Background::Color(Color::WHITE)),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 999.0.into(),
            },
            shadow: Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.25),
                offset: Vector::new(0.0, 1.0),
                blur_radius: 3.0,
            },
            ..Default::default()
        },
    );

    let track_inner = if checked {
        row![Space::with_width(Length::Fill), thumb].align_y(Alignment::Center)
    } else {
        row![thumb, Space::with_width(Length::Fill)].align_y(Alignment::Center)
    };

    button(
        container(track_inner)
            .width(Length::Fixed(36.0))
            .height(Length::Fixed(20.0))
            .padding(Padding::from([3, 3]))
            .align_y(iced::alignment::Vertical::Center),
    )
    .padding(0)
    .style(move |_, status| {
        let hovered = matches!(status, button::Status::Hovered);
        let bg = if checked {
            if hovered {
                p.accent_hover
            } else {
                p.accent_primary
            }
        } else if hovered {
            p.border_strong
        } else {
            p.border_default
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: Color::WHITE,
            border: Border {
                color: if checked {
                    p.accent_primary
                } else {
                    p.border_strong
                },
                width: 1.0,
                radius: 999.0.into(),
            },
            ..Default::default()
        }
    })
    .on_press(on_toggle)
    .into()
}

/// Grouped segmented-control track container for mutually exclusive options.
pub fn segmented_group<'a, M>(p: Palette, content: impl Into<Element<'a, M>>) -> Element<'a, M>
where
    M: 'a,
{
    container(content)
        .padding(2)
        .style(move |_| container::Style {
            background: Some(Background::Color(p.surface_input)),
            border: Border {
                color: p.border_subtle,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// Wraps an overlay's full-screen positioned container in an `opaque` click-catching backdrop so
/// clicks, right-clicks, hover states, tooltips, and wheel scrolling NEVER leak through to widgets
/// behind the popup/modal.
pub fn overlay_backdrop<'a, M>(
    positioned_overlay: impl Into<Element<'a, M>>,
    on_dismiss: M,
) -> Element<'a, M>
where
    M: Clone + 'a,
{
    opaque(
        mouse_area(positioned_overlay)
            .on_press(on_dismiss.clone())
            .on_right_press(on_dismiss),
    )
}

/// Unified centered modal dialog shell with opaque clickable backdrop, consistent header, close
/// icon, hairline divider, and comfortable outer padding.
#[allow(clippy::too_many_arguments)]
pub fn modal_shell<'a, M>(
    p: Palette,
    width: f32,
    icon: impl Into<Element<'a, M>>,
    title: impl Into<String>,
    header_badge: Option<Element<'a, M>>,
    body: impl Into<Element<'a, M>>,
    on_close: M,
) -> Element<'a, M>
where
    M: Clone + 'a,
{
    let mut header = row![
        icon.into(),
        text(title.into())
            .size(type_scale::TITLE)
            .color(p.text_primary),
    ]
    .spacing(space::SM)
    .align_y(Alignment::Center);

    if let Some(b) = header_badge {
        header = header.push(b);
    }

    header = header.push(Space::with_width(Length::Fill)).push(
        button(icons::close_x(icon_size::SM, p.text_secondary))
            .padding(pad::BTN_ICON)
            .style(theme::vscode_icon_btn(p, false))
            .on_press(on_close.clone()),
    );

    let card = opaque(
        container(
            column![
                header,
                horizontal_rule(1).style(theme::subtle_rule(p)),
                body.into()
            ]
            .spacing(space::MD),
        )
            .padding(pad::MODAL)
            .width(Length::Fixed(width))
            .max_height(layout::MODAL_MAX_HEIGHT)
            .style(theme::modal_box(p)),
    );

    let positioned = container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .padding(Padding::from([24, 24]))
        .style(theme::backdrop_box);

    overlay_backdrop(positioned, on_close)
}

/// Elevated section card with structured header (title + optional subtitle + optional trailing action)
/// used across modals and dock tabs.
pub fn section_card<'a, M>(
    p: Palette,
    title: impl Into<String>,
    subtitle: Option<&'a str>,
    trailing: Option<Element<'a, M>>,
    content: impl Into<Element<'a, M>>,
) -> Element<'a, M>
where
    M: 'a,
{
    let mut title_block = column![
        text(title.into())
            .size(type_scale::SUBHEADING)
            .color(p.text_primary),
    ]
    .spacing(2.0);

    if let Some(sub) = subtitle {
        title_block = title_block.push(
            text(sub)
                .size(type_scale::CAPTION)
                .color(p.text_muted),
        );
    }

    let mut hdr = row![title_block, Space::with_width(Length::Fill)]
        .spacing(space::SM)
        .align_y(Alignment::Center);

    if let Some(right) = trailing {
        hdr = hdr.push(right);
    }

    container(column![hdr, content.into()].spacing(space::SM))
        .padding(pad::CARD)
        .width(Length::Fill)
        .style(theme::section_panel_box(p))
        .into()
}

/// Full-bleed secondary toolbar strip rendered at the top of bottom dock tabs (Secrets, Search,
/// Ignore, Output) so every dock tab shares the exact same VS Code-inspired visual cadence.
pub fn dock_toolbar<'a, M>(p: Palette, content: impl Into<Element<'a, M>>) -> Element<'a, M>
where
    M: 'a,
{
    column![
        container(content)
            .padding(pad::DOCK_TOOLBAR)
            .width(Length::Fill)
            .style(theme::dock_subtoolbar_box(p)),
        horizontal_rule(1).style(theme::subtle_rule(p)),
    ]
    .width(Length::Fill)
    .into()
}

/// Unified pill badge for status, format, counts, and ownership indicators.
pub fn badge<'a, M>(
    label: impl Into<String>,
    bg: Color,
    fg: Color,
    border: Color,
    monospace: bool,
) -> Element<'a, M>
where
    M: 'a,
{
    let mut t = text(label.into())
        .size(type_scale::BADGE)
        .color(fg)
        .wrapping(text::Wrapping::None);
    if monospace {
        t = t.font(theme::CODE_FONT);
    }
    container(t)
        .padding(pad::PILL)
        .style(theme::pill_box(bg, fg, border))
        .into()
}

/// Consistent form field caption label.
pub fn field_label<'a, M>(p: Palette, label: &'a str) -> Element<'a, M>
where
    M: 'a,
{
    text(label)
        .size(type_scale::CAPTION)
        .color(p.text_muted)
        .into()
}

/// Centered, spacious empty state card for dock tabs and lists.
pub fn empty_state<'a, M>(
    p: Palette,
    icon: impl Into<Element<'a, M>>,
    title: impl Into<String>,
    subtitle: impl Into<String>,
    action: Option<Element<'a, M>>,
) -> Element<'a, M>
where
    M: 'a,
{
    let mut inner = column![
        icon.into(),
        text(title.into())
            .size(type_scale::SUBHEADING)
            .color(p.text_primary),
        text(subtitle.into())
            .size(type_scale::BODY)
            .color(p.text_muted),
    ]
    .spacing(space::XS)
    .align_x(Alignment::Center);

    if let Some(cta) = action {
        inner = inner.push(Space::with_height(Length::Fixed(space::XXS))).push(cta);
    }

    container(inner)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .padding(pad::MODAL)
        .into()
}

/// Computes the exact top-left origin for a popup/context-menu card of `card_size` anchored to
/// `anchor` within `viewport`, opening down-right by default and flipping upward/leftward cleanly
/// so the popup corner stays touching the cursor near window edges.
pub fn compute_popup_origin(
    anchor: Point,
    card_size: Size,
    viewport: Size,
    margin: f32,
) -> Point {
    let max_x = (viewport.width - card_size.width - margin).max(margin);
    let x = if anchor.x + card_size.width + margin <= viewport.width {
        anchor.x.max(margin)
    } else if anchor.x - card_size.width >= margin {
        anchor.x - card_size.width
    } else {
        anchor.x.clamp(margin, max_x)
    };

    let max_y = (viewport.height - card_size.height - margin).max(margin);
    let y = if anchor.y + card_size.height + margin <= viewport.height {
        anchor.y.max(margin)
    } else if anchor.y - card_size.height >= margin {
        anchor.y - card_size.height
    } else {
        anchor.y.clamp(margin, max_y)
    };

    Point::new(x, y)
}

/// Positions a floating popup/context-menu card at the exact cursor `anchor`, measuring the card's
/// true rendered dimensions inside the live window layout bounds (no estimated heights or stale
/// window sizes) and flipping upward/leftward when close to screen edges.
pub fn popup_at<'a, M>(anchor: Point, content: impl Into<Element<'a, M>>) -> Element<'a, M>
where
    M: 'a,
{
    struct PopupPositioner<'a, M> {
        anchor: Point,
        content: Element<'a, M>,
    }

    impl<'a, M> Widget<M, Theme, Renderer> for PopupPositioner<'a, M> {
        fn tag(&self) -> tree::Tag {
            tree::Tag::stateless()
        }

        fn state(&self) -> tree::State {
            tree::State::None
        }

        fn children(&self) -> Vec<Tree> {
            vec![Tree::new(&self.content)]
        }

        fn diff(&self, tree: &mut Tree) {
            tree.diff_children(std::slice::from_ref(&self.content));
        }

        fn size(&self) -> Size<Length> {
            Size::new(Length::Fill, Length::Fill)
        }

        fn layout(
            &self,
            tree: &mut Tree,
            renderer: &Renderer,
            limits: &adv_layout::Limits,
        ) -> adv_layout::Node {
            let viewport = limits.max();
            let margin = 8.0;
            let child_limits = adv_layout::Limits::new(
                Size::ZERO,
                Size::new(
                    (viewport.width - margin * 2.0).max(120.0),
                    (viewport.height - margin * 2.0).max(120.0),
                ),
            );
            let child_node =
                self.content
                    .as_widget()
                    .layout(&mut tree.children[0], renderer, &child_limits);
            let origin = compute_popup_origin(self.anchor, child_node.size(), viewport, margin);
            adv_layout::Node::with_children(viewport, vec![child_node.move_to(origin)])
        }

        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            if let Some(child_layout) = layout.children().next() {
                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    child_layout,
                    cursor,
                    viewport,
                );
            }
        }

        fn on_event(
            &mut self,
            tree: &mut Tree,
            event: event::Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &Renderer,
            clipboard: &mut dyn Clipboard,
            shell: &mut Shell<'_, M>,
            viewport: &Rectangle,
        ) -> event::Status {
            if let Some(child_layout) = layout.children().next() {
                self.content.as_widget_mut().on_event(
                    &mut tree.children[0],
                    event,
                    child_layout,
                    cursor,
                    renderer,
                    clipboard,
                    shell,
                    viewport,
                )
            } else {
                event::Status::Ignored
            }
        }

        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            renderer: &Renderer,
        ) -> mouse::Interaction {
            if let Some(child_layout) = layout.children().next() {
                self.content.as_widget().mouse_interaction(
                    &tree.children[0],
                    child_layout,
                    cursor,
                    viewport,
                    renderer,
                )
            } else {
                mouse::Interaction::default()
            }
        }

        fn operate(
            &self,
            tree: &mut Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            operation: &mut dyn iced::advanced::widget::Operation,
        ) {
            if let Some(child_layout) = layout.children().next() {
                self.content.as_widget().operate(
                    &mut tree.children[0],
                    child_layout,
                    renderer,
                    operation,
                );
            }
        }

        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            translation: Vector,
        ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
            let child_layout = layout.children().next()?;
            self.content.as_widget_mut().overlay(
                &mut tree.children[0],
                child_layout,
                renderer,
                translation,
            )
        }
    }

    Element::new(PopupPositioner {
        anchor,
        content: content.into(),
    })
}

/// Renders a non-interactive floating ghost pill near `cursor_pos` during drag-and-drop without
/// intercepting mouse events so underlying drop targets still receive hover enter/exit events.
pub fn floating_ghost<'a, M>(pos: Point, content: impl Into<Element<'a, M>>) -> Element<'a, M>
where
    M: 'a,
{
    struct FloatingGhost<'a, M> {
        pos: Point,
        content: Element<'a, M>,
    }

    impl<'a, M> Widget<M, Theme, Renderer> for FloatingGhost<'a, M> {
        fn tag(&self) -> tree::Tag {
            tree::Tag::stateless()
        }

        fn state(&self) -> tree::State {
            tree::State::None
        }

        fn children(&self) -> Vec<Tree> {
            vec![Tree::new(&self.content)]
        }

        fn diff(&self, tree: &mut Tree) {
            tree.diff_children(std::slice::from_ref(&self.content));
        }

        fn size(&self) -> Size<Length> {
            Size::new(Length::Fill, Length::Fill)
        }

        fn layout(
            &self,
            tree: &mut Tree,
            renderer: &Renderer,
            limits: &adv_layout::Limits,
        ) -> adv_layout::Node {
            let viewport = limits.max();
            let child_limits = adv_layout::Limits::new(Size::ZERO, viewport);
            let child_node =
                self.content
                    .as_widget()
                    .layout(&mut tree.children[0], renderer, &child_limits);
            let size = child_node.size();
            let x = (self.pos.x + 14.0).min((viewport.width - size.width - 8.0).max(4.0));
            let y = (self.pos.y + 12.0).min((viewport.height - size.height - 8.0).max(4.0));
            adv_layout::Node::with_children(viewport, vec![child_node.move_to(Point::new(x, y))])
        }

        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout<'_>,
            _cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            if let Some(child_layout) = layout.children().next() {
                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    child_layout,
                    mouse::Cursor::Unavailable,
                    viewport,
                );
            }
        }

        fn mouse_interaction(
            &self,
            _tree: &Tree,
            _layout: Layout<'_>,
            _cursor: mouse::Cursor,
            _viewport: &Rectangle,
            _renderer: &Renderer,
        ) -> mouse::Interaction {
            mouse::Interaction::Grabbing
        }
    }

    Element::new(FloatingGhost {
        pos,
        content: content.into(),
    })
}

/// Interactive row matching `repo_row_btn`'s exact visual states (selected/hover/pressed) while
/// publishing `on_left_down` immediately on `mouse::Event::ButtonPressed(Left)` rather than
/// waiting for `ButtonReleased` (unlike `iced::widget::button`), enabling click + drag-and-drop.
pub fn pressable_row<'a, M>(
    p: Palette,
    selected: bool,
    padding: Padding,
    content: impl Into<Element<'a, M>>,
    on_left_down: M,
) -> Element<'a, M>
where
    M: Clone + 'a,
{
    #[derive(Default)]
    struct PressRowState {
        is_hovered: bool,
        is_pressed: bool,
    }

    struct PressableRow<'a, M> {
        p: Palette,
        selected: bool,
        padding: Padding,
        content: Element<'a, M>,
        on_left_down: M,
    }

    impl<'a, M: Clone> Widget<M, Theme, Renderer> for PressableRow<'a, M> {
        fn tag(&self) -> tree::Tag {
            tree::Tag::of::<PressRowState>()
        }

        fn state(&self) -> tree::State {
            tree::State::new(PressRowState::default())
        }

        fn children(&self) -> Vec<Tree> {
            vec![Tree::new(&self.content)]
        }

        fn diff(&self, tree: &mut Tree) {
            tree.diff_children(std::slice::from_ref(&self.content));
        }

        fn size(&self) -> Size<Length> {
            Size::new(Length::Fill, Length::Shrink)
        }

        fn layout(
            &self,
            tree: &mut Tree,
            renderer: &Renderer,
            limits: &adv_layout::Limits,
        ) -> adv_layout::Node {
            let inner_limits = limits.shrink(self.padding);
            let child_node =
                self.content
                    .as_widget()
                    .layout(&mut tree.children[0], renderer, &inner_limits);
            let content_size = child_node.size().expand(self.padding);
            let size = limits.resolve(Length::Fill, Length::Shrink, content_size);
            adv_layout::Node::with_children(
                size,
                vec![child_node.move_to(Point::new(self.padding.left, self.padding.top))],
            )
        }

        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            use iced::advanced::Renderer as _;

            let state = tree.state.downcast_ref::<PressRowState>();
            let bounds = layout.bounds();
            let hovered = state.is_hovered || cursor.is_over(bounds);
            let bg = if self.selected {
                self.p.fill_selected
            } else if state.is_pressed {
                self.p.fill_active
            } else if hovered {
                self.p.fill_hover
            } else {
                Color::TRANSPARENT
            };

            if bg != Color::TRANSPARENT || self.selected {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds,
                        border: Border {
                            color: if self.selected {
                                self.p.accent_primary
                            } else {
                                Color::TRANSPARENT
                            },
                            width: if self.selected { 1.0 } else { 0.0 },
                            radius: 5.0.into(),
                        },
                        shadow: Shadow::default(),
                    },
                    Background::Color(bg),
                );
            }

            if let Some(child_layout) = layout.children().next() {
                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    &renderer::Style {
                        text_color: self.p.text_primary,
                    },
                    child_layout,
                    cursor,
                    viewport,
                );
            }
            let _ = style;
        }

        fn on_event(
            &mut self,
            tree: &mut Tree,
            event: iced::Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &Renderer,
            clipboard: &mut dyn Clipboard,
            shell: &mut Shell<'_, M>,
            viewport: &Rectangle,
        ) -> event::Status {
            if let Some(child_layout) = layout.children().next()
                && self.content.as_widget_mut().on_event(
                    &mut tree.children[0],
                    event.clone(),
                    child_layout,
                    cursor,
                    renderer,
                    clipboard,
                    shell,
                    viewport,
                ) == event::Status::Captured
            {
                return event::Status::Captured;
            }

            let state = tree.state.downcast_mut::<PressRowState>();
            match event {
                iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                    state.is_hovered = cursor.is_over(layout.bounds());
                }
                iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                    if cursor.is_over(layout.bounds()) {
                        state.is_pressed = true;
                        shell.publish(self.on_left_down.clone());
                        return event::Status::Captured;
                    }
                }
                iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    state.is_pressed = false;
                }
                _ => {}
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
            if cursor.is_over(layout.bounds()) {
                mouse::Interaction::Pointer
            } else {
                mouse::Interaction::None
            }
        }

        fn operate(
            &self,
            tree: &mut Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            operation: &mut dyn iced::advanced::widget::Operation,
        ) {
            if let Some(child_layout) = layout.children().next() {
                self.content.as_widget().operate(
                    &mut tree.children[0],
                    child_layout,
                    renderer,
                    operation,
                );
            }
        }

        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            translation: Vector,
        ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
            let child_layout = layout.children().next()?;
            self.content.as_widget_mut().overlay(
                &mut tree.children[0],
                child_layout,
                renderer,
                translation,
            )
        }
    }

    Element::new(PressableRow {
        p,
        selected,
        padding,
        content: content.into(),
        on_left_down,
    })
}

/// Computes `(overflows_x, child_content_width, canvas_total_width, bottom_pad)` for
/// `responsive_hscroll` given the live `viewport_w` and desired `min_content_width`.
fn compute_hscroll_widths(viewport_w: f32, min_content_width: f32) -> (bool, f32, f32, f32) {
    let vw = viewport_w.max(1.0);
    let min_w = min_content_width.max(1.0);
    if min_w > vw + 0.5 {
        // Overflow: render content at full finite `min_w`, leaving 8px right clearance for the
        // vertical scrollbar and 10px bottom clearance for the horizontal scrollbar.
        (true, min_w, min_w + 8.0, 10.0)
    } else {
        // Fits viewport: span exact `vw` so horizontal scrollbar stays hidden while reserving
        // 8px right clearance for the floating vertical scrollbar.
        (false, (vw - 8.0).max(1.0), vw, 0.0)
    }
}

/// Scrollable container with BOTH vertical and horizontal scrollbars whose content width is
/// resolved dynamically to `viewport_width.max(min_content_width)`:
/// - Wide panel: fills 100% of the viewport cleanly with NO horizontal scrollbar.
/// - Narrow panel (or wide items): maintains finite `min_content_width` and shows a slim
///   horizontal scrollbar at the bottom so resized panels never lose content.
pub fn responsive_hscroll<'a, M>(
    content: impl Into<Element<'a, M>>,
    min_content_width: f32,
    p: Palette,
) -> Element<'a, M>
where
    M: 'a,
{
    responsive_hscroll_impl(content, min_content_width, p, None, None)
}

pub fn responsive_hscroll_with_id<'a, M>(
    content: impl Into<Element<'a, M>>,
    min_content_width: f32,
    id: scrollable::Id,
    p: Palette,
) -> Element<'a, M>
where
    M: 'a,
{
    responsive_hscroll_impl(content, min_content_width, p, Some(id), None)
}

/// Same as `responsive_hscroll` but attaches a persistent `scrollable::Id` and `on_scroll`
/// listener so synchronized panes (like Side-by-Side Diff) can scroll horizontally independently
/// while staying vertically synchronized and feeding live scroll offsets to connector canvases.
pub fn responsive_hscroll_synced<'a, M>(
    content: impl Into<Element<'a, M>>,
    min_content_width: f32,
    id: scrollable::Id,
    on_scroll: impl Fn(scrollable::Viewport) -> M + 'a,
    p: Palette,
) -> Element<'a, M>
where
    M: 'a,
{
    responsive_hscroll_impl(content, min_content_width, p, Some(id), Some(Box::new(on_scroll)))
}

fn responsive_hscroll_impl<'a, M>(
    content: impl Into<Element<'a, M>>,
    min_content_width: f32,
    p: Palette,
    id: Option<scrollable::Id>,
    on_scroll: Option<Box<dyn Fn(scrollable::Viewport) -> M + 'a>>,
) -> Element<'a, M>
where
    M: 'a,
{
    struct MinWidthCanvas<'a, M> {
        content: Element<'a, M>,
        min_width: f32,
        viewport_w: Rc<Cell<f32>>,
    }

    impl<'a, M> Widget<M, Theme, Renderer> for MinWidthCanvas<'a, M> {
        fn tag(&self) -> tree::Tag {
            tree::Tag::stateless()
        }

        fn state(&self) -> tree::State {
            tree::State::None
        }

        fn children(&self) -> Vec<Tree> {
            vec![Tree::new(&self.content)]
        }

        fn diff(&self, tree: &mut Tree) {
            tree.diff_children(std::slice::from_ref(&self.content));
        }

        fn size(&self) -> Size<Length> {
            Size::new(Length::Shrink, Length::Shrink)
        }

        fn layout(
            &self,
            tree: &mut Tree,
            renderer: &Renderer,
            limits: &adv_layout::Limits,
        ) -> adv_layout::Node {
            let (_, content_w, total_w, bottom_pad) =
                compute_hscroll_widths(self.viewport_w.get(), self.min_width);
            let child_limits = adv_layout::Limits::new(
                Size::new(content_w, 0.0),
                Size::new(content_w, limits.max().height),
            );
            let child_node =
                self.content
                    .as_widget()
                    .layout(&mut tree.children[0], renderer, &child_limits);
            let h = child_node.size().height + bottom_pad;
            adv_layout::Node::with_children(Size::new(total_w, h), vec![child_node])
        }

        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            if let Some(child_layout) = layout.children().next() {
                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    child_layout,
                    cursor,
                    viewport,
                );
            }
        }

        fn on_event(
            &mut self,
            tree: &mut Tree,
            event: iced::Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &Renderer,
            clipboard: &mut dyn Clipboard,
            shell: &mut Shell<'_, M>,
            viewport: &Rectangle,
        ) -> event::Status {
            let Some(child_layout) = layout.children().next() else {
                return event::Status::Ignored;
            };
            self.content.as_widget_mut().on_event(
                &mut tree.children[0],
                event,
                child_layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            )
        }

        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            renderer: &Renderer,
        ) -> mouse::Interaction {
            let Some(child_layout) = layout.children().next() else {
                return mouse::Interaction::None;
            };
            self.content.as_widget().mouse_interaction(
                &tree.children[0],
                child_layout,
                cursor,
                viewport,
                renderer,
            )
        }

        fn operate(
            &self,
            tree: &mut Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            operation: &mut dyn iced::advanced::widget::Operation,
        ) {
            if let Some(child_layout) = layout.children().next() {
                self.content.as_widget().operate(
                    &mut tree.children[0],
                    child_layout,
                    renderer,
                    operation,
                );
            }
        }

        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            translation: Vector,
        ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
            let child_layout = layout.children().next()?;
            self.content.as_widget_mut().overlay(
                &mut tree.children[0],
                child_layout,
                renderer,
                translation,
            )
        }
    }

    struct ResponsiveHScroll<'a, M> {
        viewport_w: Rc<Cell<f32>>,
        min_width: f32,
        scroll: Element<'a, M>,
    }

    impl<'a, M> Widget<M, Theme, Renderer> for ResponsiveHScroll<'a, M> {
        fn tag(&self) -> tree::Tag {
            tree::Tag::stateless()
        }

        fn state(&self) -> tree::State {
            tree::State::None
        }

        fn children(&self) -> Vec<Tree> {
            vec![Tree::new(&self.scroll)]
        }

        fn diff(&self, tree: &mut Tree) {
            tree.diff_children(std::slice::from_ref(&self.scroll));
        }

        fn size(&self) -> Size<Length> {
            Size::new(Length::Fill, Length::Fill)
        }

        fn layout(
            &self,
            tree: &mut Tree,
            renderer: &Renderer,
            limits: &adv_layout::Limits,
        ) -> adv_layout::Node {
            let vw = limits.max().width;
            self.viewport_w
                .set(if vw.is_finite() { vw } else { self.min_width });
            let child_node =
                self.scroll
                    .as_widget()
                    .layout(&mut tree.children[0], renderer, limits);
            adv_layout::Node::with_children(child_node.size(), vec![child_node])
        }

        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            if let Some(child_layout) = layout.children().next() {
                self.scroll.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    child_layout,
                    cursor,
                    viewport,
                );
            }
        }

        fn on_event(
            &mut self,
            tree: &mut Tree,
            event: iced::Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &Renderer,
            clipboard: &mut dyn Clipboard,
            shell: &mut Shell<'_, M>,
            viewport: &Rectangle,
        ) -> event::Status {
            let Some(child_layout) = layout.children().next() else {
                return event::Status::Ignored;
            };
            self.scroll.as_widget_mut().on_event(
                &mut tree.children[0],
                event,
                child_layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            )
        }

        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            renderer: &Renderer,
        ) -> mouse::Interaction {
            let Some(child_layout) = layout.children().next() else {
                return mouse::Interaction::None;
            };
            self.scroll.as_widget().mouse_interaction(
                &tree.children[0],
                child_layout,
                cursor,
                viewport,
                renderer,
            )
        }

        fn operate(
            &self,
            tree: &mut Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            operation: &mut dyn iced::advanced::widget::Operation,
        ) {
            if let Some(child_layout) = layout.children().next() {
                self.scroll.as_widget().operate(
                    &mut tree.children[0],
                    child_layout,
                    renderer,
                    operation,
                );
            }
        }

        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            translation: Vector,
        ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
            let child_layout = layout.children().next()?;
            self.scroll.as_widget_mut().overlay(
                &mut tree.children[0],
                child_layout,
                renderer,
                translation,
            )
        }
    }

    let viewport_w = Rc::new(Cell::new(min_content_width.max(1.0)));
    let canvas: Element<'a, M> = Element::new(MinWidthCanvas {
        content: content.into(),
        min_width: min_content_width,
        viewport_w: Rc::clone(&viewport_w),
    });
    let mut s = scrollable(canvas)
        .width(Length::Fill)
        .height(Length::Fill)
        .direction(theme::slim_both_direction())
        .style(theme::slim_scrollable(p));
    if let Some(scroll_id) = id {
        s = s.id(scroll_id);
    }
    if let Some(cb) = on_scroll {
        s = s.on_scroll(cb);
    }
    let scroll: Element<'a, M> = s.into();

    Element::new(ResponsiveHScroll {
        viewport_w,
        min_width: min_content_width,
        scroll,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_origin_anchors_and_flips_cleanly_at_viewport_edges() {
        let vp = Size::new(1440.0, 860.0);
        let card = Size::new(248.0, 340.0);

        // Normal top-left click opens down-right from exact cursor
        assert_eq!(
            compute_popup_origin(Point::new(120.0, 200.0), card, vp, 8.0),
            Point::new(120.0, 200.0)
        );

        // Bottom-left branch pane click flips upward so bottom touches cursor
        assert_eq!(
            compute_popup_origin(Point::new(140.0, 760.0), card, vp, 8.0),
            Point::new(140.0, 420.0)
        );

        // Right-edge files pane click flips leftward so right edge touches cursor
        assert_eq!(
            compute_popup_origin(Point::new(1380.0, 300.0), card, vp, 8.0),
            Point::new(1132.0, 300.0)
        );
    }

    #[test]
    fn responsive_hscroll_only_overflows_when_narrower_than_content() {
        // Wide viewport: fits cleanly with no horizontal overflow
        let (overflows, content_w, total_w, bottom_pad) = compute_hscroll_widths(500.0, 320.0);
        assert!(!overflows);
        assert_eq!(content_w, 492.0);
        assert_eq!(total_w, 500.0);
        assert_eq!(bottom_pad, 0.0);

        // Narrow viewport: triggers horizontal scrolling with exact finite content width
        let (overflows, content_w, total_w, bottom_pad) = compute_hscroll_widths(220.0, 420.0);
        assert!(overflows);
        assert_eq!(content_w, 420.0);
        assert_eq!(total_w, 428.0);
        assert_eq!(bottom_pad, 10.0);
    }
}
