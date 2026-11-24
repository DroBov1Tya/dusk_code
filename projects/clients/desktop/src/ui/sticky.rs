//! A corner that stays in sight: a child kept at the top-right of the visible part of
//! the block it is put in, the way CSS `position: sticky` keeps a header. gpui has no
//! sticky position, so this element reads the clip of the scroll it is drawn in.

use gpui_kit::*;

/// Keeps `child` at the top-right of `block`'s visible part while it scrolls, and never
/// outside the block. Put it as the last child of a `relative` block, so it is drawn over
/// the text.
pub fn top_right(child: impl IntoElement) -> Sticky {
    Sticky {
        child: child.into_any_element(),
    }
}

pub struct Sticky {
    child: AnyElement,
}

impl IntoElement for Sticky {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Sticky {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        // As tall as the block and no wider than its right edge, so it takes no room.
        let mut style = Style {
            position: Position::Absolute,
            ..Style::default()
        };
        style.inset.top = px(0.).into();
        style.inset.bottom = px(0.).into();
        style.inset.right = px(0.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let size = self
            .child
            .layout_as_root(AvailableSpace::min_size(), window, cx);
        let visible = window.content_mask().bounds;
        let lowest = (bounds.bottom() - size.height).max(bounds.top());
        let y = visible.top().max(bounds.top()).min(lowest);
        let x = bounds.right() - size.width;
        self.child.prepaint_at(point(x, y), window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}
