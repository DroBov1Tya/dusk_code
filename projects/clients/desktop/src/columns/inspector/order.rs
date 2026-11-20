//! The order of the right column's modules: a grip on each header drags a module to
//! another place, Alt+Up and Alt+Down on a focused header move it by one, and the order
//! is kept in `gui.json` as the folding is.

use gpui_kit::{
    component::{ActiveTheme, Icon},
    *,
};

use crate::{
    ui::{Type, icons, widgets},
    workspace::{Right, Workspace},
};

/// The module under the pointer while its header is dragged.
#[derive(Clone)]
pub struct Dragged {
    module: Right,
    title: &'static str,
}

/// What follows the pointer: the module's name on a small card.
impl Render for Dragged {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .px_3()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .shadow_md()
            .small()
            .font_weight(FontWeight::MEDIUM)
            .child(self.title)
    }
}

/// The six dots a header is dragged by; they show while the pointer is over the header
/// (the group `header`), and a press on them does not fold the module.
pub fn grip(module: Right, title: &'static str, header: SharedString, cx: &App) -> Stateful<Div> {
    let muted = cx.theme().muted_foreground;
    div()
        .id(SharedString::from(format!("module-grip-{}", module.key())))
        .flex_none()
        .opacity(0.)
        .group_hover(header, |grip| grip.opacity(1.))
        .cursor_grab()
        .child(widgets::icon(Icon::default().path(icons::GRIP), muted))
        .on_click(|_, _, cx| cx.stop_propagation())
        .on_drag(Dragged { module, title }, |dragged, _, _, cx| {
            cx.new(|_| dragged.clone())
        })
}

/// Where a module dragged over `section` would land, kept for the line that shows it.
pub fn target(section: Div, module: Right, cx: &mut Context<Workspace>) -> Div {
    section.on_drag_move(
        cx.listener(move |ws, event: &DragMoveEvent<Dragged>, _, cx| {
            if !event.bounds.contains(&event.event.position) {
                return;
            }
            let below = event.event.position.y > event.bounds.center().y;
            let next = Some((module, below));
            if ws.module_drop != next {
                ws.module_drop = next;
                cx.notify();
            }
        }),
    )
}

/// The line where a dragged module would land, at the top or the foot of `module`.
pub fn indicator(ws: &Workspace, module: Right, cx: &App) -> Option<Div> {
    let (over, below) = ws.module_drop?;
    if over != module || !cx.has_active_drag() {
        return None;
    }
    let line = div()
        .absolute()
        .left_4()
        .right_4()
        .h(px(2.))
        .rounded_full()
        .bg(cx.theme().primary);
    Some(if below { line.bottom_0() } else { line.top_0() })
}

impl Workspace {
    /// The drop: the dragged module goes where the line showed.
    pub fn drop_module(&mut self, dragged: &Dragged, cx: &mut Context<Self>) {
        if let Some((over, below)) = self.module_drop.take() {
            let order = moved(&self.right_order, dragged.module, over, below);
            self.keep_order(order, dragged.module, cx);
        }
        cx.notify();
    }

    /// Alt+Up and Alt+Down: the module trades places with the shown one above or below.
    pub fn step_module(&mut self, module: Right, up: bool, cx: &mut Context<Self>) {
        let shown: Vec<Right> = self
            .right_order
            .iter()
            .copied()
            .filter(|shown| self.has_content(*shown))
            .collect();
        let Some(at) = shown.iter().position(|shown| *shown == module) else {
            return;
        };
        let neighbour = if up {
            at.checked_sub(1).map(|above| shown[above])
        } else {
            shown.get(at + 1).copied()
        };
        if let Some(neighbour) = neighbour {
            let order = moved(&self.right_order, module, neighbour, !up);
            self.keep_order(order, module, cx);
        }
    }

    fn keep_order(&mut self, order: Vec<Right>, module: Right, cx: &mut Context<Self>) {
        if order == self.right_order {
            return;
        }
        let at = |order: &[Right]| order.iter().position(|m| *m == module);
        let from = if at(&order) > at(&self.right_order) {
            -SETTLE_FROM
        } else {
            SETTLE_FROM
        };
        self.module_moved = Some((module, from, std::time::Instant::now()));
        let keys = order.iter().map(|module| module.key().to_owned()).collect();
        self.right_order = order;
        crate::ui::theme::change(cx, |prefs| prefs.right_order = keys);
        cx.notify();
    }
}

/// How far a moved module slides into its place.
const SETTLE_FROM: f32 = 12.;

/// The module just moved slides the last pixels into its place, from the side it came.
/// A style on the section itself, so its header keeps the focus it had.
pub fn settle(ws: &Workspace, module: Right, section: Div, window: &Window, cx: &App) -> Div {
    let Some((moved, from, since)) = ws.module_moved else {
        return section;
    };
    match crate::ui::motion::progress(since, crate::ui::motion::SETTLE, window, cx) {
        Some(t) if moved == module => section.opacity(0.6 + 0.4 * t).top(px((1. - t) * from)),
        _ => section,
    }
}

/// The order kept in `gui.json`: its known names once each, then any module it does not
/// name in the default order, so a file from before a module existed still shows it.
pub fn saved(keys: &[String]) -> Vec<Right> {
    let mut order = Vec::with_capacity(Right::ALL.len());
    let named = keys
        .iter()
        .filter_map(|key| Right::ALL.into_iter().find(|module| module.key() == key));
    for module in named.chain(Right::ALL) {
        if !order.contains(&module) {
            order.push(module);
        }
    }
    order
}

/// `order` with `module` taken out and put back above or below `over`.
fn moved(order: &[Right], module: Right, over: Right, below: bool) -> Vec<Right> {
    if module == over {
        return order.to_vec();
    }
    let mut out: Vec<Right> = order.iter().copied().filter(|m| *m != module).collect();
    match out.iter().position(|m| *m == over) {
        Some(at) => {
            out.insert(at + usize::from(below), module);
            out
        }
        None => order.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: the window's prelude brings a `test` of its own.
    use super::{Right, moved, saved};

    fn keys(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn a_file_without_an_order_keeps_the_default() {
        assert_eq!(saved(&[]), Right::ALL.to_vec());
    }

    #[test]
    fn unknown_and_repeated_names_are_ignored_and_missing_ones_follow() {
        let order = saved(&keys(&["todo", "nonsense", "git", "todo", ""]));
        assert_eq!(&order[..2], &[Right::Todo, Right::Git]);
        assert_eq!(order.len(), Right::ALL.len());
        let rest: Vec<Right> = Right::ALL
            .into_iter()
            .filter(|module| !matches!(module, Right::Todo | Right::Git))
            .collect();
        assert_eq!(&order[2..], &rest[..]);
    }

    #[test]
    fn a_module_lands_above_or_below_the_one_it_is_dropped_on() {
        let order = [Right::Arbiter, Right::Overview, Right::Git, Right::Todo];
        assert_eq!(
            moved(&order, Right::Todo, Right::Overview, false),
            [Right::Arbiter, Right::Todo, Right::Overview, Right::Git]
        );
        assert_eq!(
            moved(&order, Right::Arbiter, Right::Git, true),
            [Right::Overview, Right::Git, Right::Arbiter, Right::Todo]
        );
        assert_eq!(moved(&order, Right::Git, Right::Git, true), order);
    }
}
