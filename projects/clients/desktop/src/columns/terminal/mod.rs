//! The terminal panel at the foot of the work area: the user's own shells in tabs, for
//! working in the project without leaving the window. Each shell runs in the core, in a
//! pseudo-terminal; this draws what it writes and sends it the keys. The agent has no
//! part in it: it neither reads these screens nor writes to them.
//!
//! - `screen`: the bytes a shell wrote, as a grid of cells;
//! - `keys`: a key as the bytes a terminal sends;
//! - `paint`: the grid drawn in the code font.

mod keys;
mod paint;
mod screen;

use std::{cell::Cell, path::PathBuf, rc::Rc};

use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};
use tokio::sync::mpsc;

use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};
use screen::{Reach, Screen, Size as Cells, Spot};

/// The panel's height at first, and the bounds a drag keeps it in.
const HEIGHT: f32 = 280.;
const HEIGHTS: (f32, f32) = (120., 720.);
const TABS: f32 = 36.;
/// Around the grid; the tabs start at the same left edge.
const PADDING: f32 = crate::ui::size::GAP_M;
/// The size a shell starts with, before the panel was measured.
const FIRST: Cells = Cells { cols: 80, rows: 24 };
/// Past this, a paste is refused rather than cut in the middle.
const MAX_PASTE: usize = 60 * 1024;

/// What is sent to a shell, kept in the order it was typed.
enum Sent {
    Input(String, Vec<u8>),
    Resize(String, Cells),
    Close(String),
}

struct Tab {
    /// Which tab this is, for the answer to its opening.
    key: u64,
    /// The core's name for the shell, once it has started.
    id: Option<String>,
    title: String,
    screen: Screen,
    /// The shell ended, with this code when it gave one.
    ended: Option<Option<i32>>,
}

pub struct Panel {
    pub open: bool,
    height: f32,
    tabs: Vec<Tab>,
    active: usize,
    pub focus: FocusHandle,
    next_key: u64,
    /// Where the grid was drawn last, to tell how many cells fit and which is under the
    /// pointer.
    measured: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// The pointer's height and the panel's when a drag of its edge began.
    drag: Option<(Pixels, f32)>,
    /// The button went down on the grid and is still held: moves extend the selection.
    selecting: bool,
    /// The queue to the shells; one task empties it, so keys arrive as they were typed.
    sent: Option<mpsc::UnboundedSender<Sent>>,
}

impl Panel {
    pub fn new(cx: &mut App) -> Self {
        Self {
            open: false,
            height: HEIGHT,
            tabs: Vec::new(),
            active: 0,
            focus: cx.focus_handle(),
            next_key: 0,
            measured: Rc::new(Cell::new(None)),
            drag: None,
            selecting: false,
            sent: None,
        }
    }

    /// The edge or a selection is being dragged, so the whole window follows the pointer.
    pub fn dragging(&self) -> bool {
        self.drag.is_some() || self.selecting
    }

    /// Selects cells `from..=to` of `row` in the shown shell, as a drag over them would.
    #[cfg(feature = "screenshot")]
    pub fn select_cells(&mut self, row: usize, from: usize, to: usize) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            let spot = |col, right| Spot { row, col, right };
            tab.screen.select(spot(from, false), Reach::Cells);
            tab.screen.extend_selection(spot(to, true));
        }
    }
}

pub fn render(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Option<Div> {
    if !ws.terminal.open {
        return None;
    }
    ws.fit_terminal(window, cx);
    let theme = cx.theme();
    let (border, muted, accent, surface, foreground) = (
        theme.border,
        theme.muted_foreground,
        theme.primary,
        theme.background,
        theme.foreground,
    );
    let focused = ws.terminal.focus.is_focused(window);
    let mut tabs = div()
        .id("terminal-tabs")
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .h(px(TABS))
        .px(px(PADDING))
        .overflow_x_scroll();
    for (index, tab) in ws.terminal.tabs.iter().enumerate() {
        let chosen = index == ws.terminal.active;
        let title = match (&tab.ended, tab.title.is_empty()) {
            (Some(_), _) => format!("{} (ended)", index + 1),
            (None, true) => format!("Terminal {}", index + 1),
            (None, false) => tab.title.clone(),
        };
        tabs = tabs.child(
            div()
                .id(("terminal-tab", tab.key as usize))
                .flex_none()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .h(px(28.))
                .pl_3()
                .pr_1()
                .rounded_md()
                .small()
                .cursor_pointer()
                .text_color(if chosen { foreground } else { muted })
                .when(chosen, |tab| tab.bg(accent.opacity(0.14)))
                .child(div().max_w(px(180.)).truncate().child(title))
                .child(
                    Button::new(("terminal-close", tab.key as usize))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Close)
                        .tooltip("Close this terminal")
                        .on_click(cx.listener(move |ws, _, window, cx| {
                            cx.stop_propagation();
                            ws.close_terminal(index, window, cx);
                        })),
                )
                .on_click(cx.listener(move |ws, _, window, cx| {
                    ws.terminal.active = index;
                    ws.terminal.focus.focus(window, cx);
                    cx.notify();
                })),
        );
    }
    let bar = div()
        .flex()
        .flex_row()
        .items_center()
        .child(div().flex_1().min_w_0().child(tabs))
        .child(
            Button::new("terminal-new")
                .ghost()
                .small()
                .icon(IconName::Plus)
                .tooltip("New terminal")
                .on_click(cx.listener(|ws, _, window, cx| ws.new_terminal(window, cx))),
        )
        .child(
            Button::new("terminal-hide")
                .ghost()
                .small()
                .icon(IconName::ChevronDown)
                .tooltip("Hide the terminal; the shells go on")
                .on_click(cx.listener(|ws, _, window, cx| ws.toggle_terminal(window, cx))),
        )
        .child(div().w_2());
    let body = match ws.terminal.tabs.get(ws.terminal.active) {
        Some(tab) => paint::grid(
            tab.screen.picture(),
            paint::metrics(window, cx),
            paint::Palette::of(cx),
            focused,
            Rc::clone(&ws.terminal.measured),
        )
        .into_any_element(),
        None => widgets::muted("No terminal is open.", cx).into_any_element(),
    };
    Some(
        div()
            .flex_none()
            .h(px(ws.terminal.height))
            .flex()
            .flex_col()
            .bg(surface)
            // The edge is the handle: drag it to give the terminal more room or less.
            .child(
                div()
                    .id("terminal-edge")
                    .h(px(5.))
                    .flex_none()
                    .border_t_1()
                    .border_color(if focused { accent.opacity(0.5) } else { border })
                    .cursor_row_resize()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|ws, event: &MouseDownEvent, _, cx| {
                            ws.terminal.drag = Some((event.position.y, ws.terminal.height));
                            cx.notify();
                        }),
                    ),
            )
            .child(bar)
            .child(
                div()
                    .id("terminal-screen")
                    .flex_1()
                    .min_h_0()
                    .p(px(PADDING))
                    .overflow_hidden()
                    .track_focus(&ws.terminal.focus)
                    .key_context("Terminal")
                    .cursor_text()
                    .on_mouse_down(MouseButton::Left, cx.listener(Workspace::terminal_press))
                    .on_key_down(cx.listener(Workspace::terminal_key))
                    .on_scroll(cx.listener(Workspace::terminal_wheel))
                    .child(body),
            ),
    )
}

impl Workspace {
    /// Whether the keys go to a shell now, so the window's own bare keys stand aside.
    pub fn terminal_focused(&self, window: &Window) -> bool {
        self.terminal.open && self.terminal.focus.is_focused(window)
    }

    pub fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.terminal.open = !self.terminal.open;
        if !self.terminal.open {
            window.focus(&self.focus, cx);
        } else if self.terminal.tabs.is_empty() {
            self.new_terminal(window, cx);
        } else {
            self.terminal.focus.focus(window, cx);
        }
        cx.notify();
    }

    /// Opens a shell in the chosen project's folder, or in the workspace when none is.
    pub fn new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cwd: PathBuf = self.project_dir().unwrap_or_else(|| self.home.clone());
        let size = self
            .terminal
            .tabs
            .last()
            .map_or(FIRST, |tab| tab.screen.size);
        let key = self.terminal.next_key;
        self.terminal.next_key += 1;
        self.terminal.tabs.push(Tab {
            key,
            id: None,
            title: String::new(),
            screen: Screen::new(size),
            ended: None,
        });
        self.terminal.active = self.terminal.tabs.len() - 1;
        self.terminal.open = true;
        self.terminal.focus.focus(window, cx);
        cx.notify();
        self.core.call(
            cx,
            move |client| async move { client.terminal().open(cwd, size.cols, size.rows).await },
            move |ws, result, cx| {
                match result {
                    Ok(id) => match ws.terminal.tabs.iter_mut().find(|tab| tab.key == key) {
                        Some(tab) => tab.id = Some(id),
                        // The tab was closed before its shell started: the shell goes too.
                        None => ws.send_terminal(Sent::Close(id), cx),
                    },
                    Err(error) => {
                        ws.terminal.tabs.retain(|tab| tab.key != key);
                        ws.terminal.active = ws
                            .terminal
                            .active
                            .min(ws.terminal.tabs.len().saturating_sub(1));
                        ws.fail(error, cx);
                    }
                }
                cx.notify();
            },
        );
    }

    fn close_terminal(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.terminal.tabs.len() {
            return;
        }
        let tab = self.terminal.tabs.remove(index);
        if let (Some(id), None) = (tab.id, tab.ended) {
            self.send_terminal(Sent::Close(id), cx);
        }
        if self.terminal.active >= index {
            self.terminal.active = self.terminal.active.saturating_sub(1);
        }
        // The last tab takes the panel with it.
        if self.terminal.tabs.is_empty() {
            self.terminal.open = false;
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    /// What a shell wrote. The answers its programs ask the terminal for go straight back.
    pub fn terminal_output(&mut self, id: &str, data: &[u8], cx: &mut Context<Self>) {
        let Some(tab) = self
            .terminal
            .tabs
            .iter_mut()
            .find(|tab| tab.id.as_deref() == Some(id))
        else {
            return;
        };
        let replies = tab.screen.feed(data);
        if let Some(title) = tab.screen.take_title() {
            tab.title = title;
        }
        if !replies.is_empty() {
            self.send_terminal(Sent::Input(id.to_owned(), replies), cx);
        }
        cx.notify();
    }

    pub fn terminal_closed(&mut self, id: &str, code: Option<i32>, cx: &mut Context<Self>) {
        if let Some(tab) = self
            .terminal
            .tabs
            .iter_mut()
            .find(|tab| tab.id.as_deref() == Some(id))
        {
            tab.ended = Some(code);
            cx.notify();
        }
    }

    /// Puts something on the queue to the shells, starting the task that empties it. The
    /// calls go one after another, so what is typed fast still arrives in order.
    fn send_terminal(&mut self, sent: Sent, cx: &mut Context<Self>) {
        let queue = self.terminal.sent.get_or_insert_with(|| {
            let (queue, mut waiting) = mpsc::unbounded_channel();
            self.core.run(move |client| async move {
                while let Some(sent) = waiting.recv().await {
                    let terminal = client.terminal();
                    let done = match sent {
                        Sent::Input(id, data) => terminal.input(&id, data).await,
                        Sent::Resize(id, size) => terminal.resize(&id, size.cols, size.rows).await,
                        Sent::Close(id) => terminal.close(&id).await,
                    };
                    // A shell that ended meanwhile refuses what was on its way: not news.
                    if let Err(error) = done {
                        tracing::debug!("terminal: {error:#}");
                    }
                }
            });
            queue
        });
        if queue.send(sent).is_err() {
            self.terminal.sent = None;
            self.fail(anyhow::anyhow!("the terminal lost its way to the core"), cx);
        }
    }

    /// Bytes for the shown shell, with its view back at the newest line.
    pub fn type_in_terminal(&mut self, data: Vec<u8>, cx: &mut Context<Self>) {
        let Some(tab) = self.terminal.tabs.get_mut(self.terminal.active) else {
            return;
        };
        let (Some(id), None) = (tab.id.clone(), tab.ended) else {
            return;
        };
        tab.screen.follow();
        tab.screen.clear_selection();
        self.send_terminal(Sent::Input(id, data), cx);
        cx.notify();
    }

    fn terminal_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        // Copy is the platform's own too; Control-C alone stays the shell's interrupt.
        let copy = keystroke.key == "c"
            && (modifiers.platform
                || (modifiers.control && modifiers.shift && !cfg!(target_os = "macos")));
        if copy {
            cx.stop_propagation();
            let selected = self
                .terminal
                .tabs
                .get(self.terminal.active)
                .and_then(|tab| tab.screen.selected_text());
            if let Some(text) = selected {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
            return;
        }
        // Paste is the platform's own: Command-V on a Mac, Control-Shift-V elsewhere.
        let paste = keystroke.key == "v"
            && (modifiers.platform
                || (modifiers.control && modifiers.shift && !cfg!(target_os = "macos")));
        if paste {
            cx.stop_propagation();
            self.paste_in_terminal(cx);
            return;
        }
        let app_cursor = self
            .terminal
            .tabs
            .get(self.terminal.active)
            .is_some_and(|tab| tab.screen.app_cursor());
        if let Some(data) = keys::bytes(keystroke, app_cursor) {
            cx.stop_propagation();
            self.type_in_terminal(data, cx);
        }
    }

    fn paste_in_terminal(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        if text.len() > MAX_PASTE {
            self.fail(anyhow::anyhow!("the paste is too long for a terminal"), cx);
            return;
        }
        let fenced = self
            .terminal
            .tabs
            .get(self.terminal.active)
            .is_some_and(|tab| tab.screen.bracketed_paste());
        // A paste must not close its own fence and go on as typed commands, and a line
        // end is the Return key, as a terminal sends it.
        let text = text
            .replace("\x1b[201~", "")
            .replace("\r\n", "\r")
            .replace('\n', "\r");
        let data = match fenced {
            true => format!("\x1b[200~{text}\x1b[201~").into_bytes(),
            false => text.into_bytes(),
        };
        self.type_in_terminal(data, cx);
    }

    fn terminal_wheel(&mut self, event: &ScrollEvent, window: &mut Window, cx: &mut Context<Self>) {
        let line = paint::metrics(window, cx).cell.height;
        let lines = (event.delta.pixel_delta(line).y / line).round() as i32;
        if lines == 0 {
            return;
        }
        cx.stop_propagation();
        let Some(tab) = self.terminal.tabs.get_mut(self.terminal.active) else {
            return;
        };
        if tab.screen.alt_screen() {
            // A full-screen program scrolls itself: the wheel is its arrow keys.
            let arrow: &[u8] = if lines > 0 { b"\x1b[A" } else { b"\x1b[B" };
            let data = arrow.repeat(lines.unsigned_abs().min(20) as usize);
            self.type_in_terminal(data, cx);
        } else {
            tab.screen.scroll(lines);
            cx.notify();
        }
    }

    /// A press on the grid takes the keyboard and starts a selection: one click from the
    /// cell, two the word, three the line; with Shift it moves the end of the one there is.
    fn terminal_press(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.terminal.focus.focus(window, cx);
        let spot = self.terminal_spot(event.position, window, cx);
        if let (Some(spot), Some(tab)) = (spot, self.terminal.tabs.get_mut(self.terminal.active)) {
            if event.modifiers.shift {
                tab.screen.extend_selection(spot);
            } else {
                let reach = match event.click_count {
                    0 | 1 => Reach::Cells,
                    2 => Reach::Word,
                    _ => Reach::Line,
                };
                tab.screen.select(spot, reach);
            }
            self.terminal.selecting = true;
        }
        cx.notify();
    }

    /// The cell under `position`, held to the grid so a drag past its edge keeps going.
    fn terminal_spot(&self, position: Point<Pixels>, window: &Window, cx: &App) -> Option<Spot> {
        let area = self.terminal.measured.get()?;
        let cell = paint::metrics(window, cx).cell;
        let at = |from: Pixels, cell: Pixels| ((from / cell).max(0.)).floor();
        let x = position.x - area.origin.x;
        let col = at(x, cell.width);
        Some(Spot {
            row: at(position.y - area.origin.y, cell.height) as usize,
            col: col as usize,
            right: x / cell.width - col >= 0.5,
        })
    }

    /// Gives every shell the size that fits the panel as it was last drawn.
    fn fit_terminal(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(area) = self.terminal.measured.get().map(|bounds| bounds.size) else {
            return;
        };
        let cell = paint::metrics(window, cx).cell;
        let fit = |space: Pixels, cell: Pixels| ((space / cell).floor() as u16).clamp(2, 500);
        let size = Cells {
            cols: fit(area.width, cell.width),
            rows: fit(area.height, cell.height),
        };
        let mut resized = Vec::new();
        for tab in &mut self.terminal.tabs {
            if tab.screen.size != size {
                tab.screen.resize(size);
                resized.extend(tab.id.clone().filter(|_| tab.ended.is_none()));
            }
        }
        for id in resized {
            self.send_terminal(Sent::Resize(id, size), cx);
        }
    }

    /// The pointer moved while the panel's edge is held, and the panel follows it; or
    /// while a selection is, and its end follows.
    pub fn drag_terminal(
        &mut self,
        event: &MouseMoveEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        // Released outside the window, the button is up without a word: stop here.
        if event.pressed_button != Some(MouseButton::Left) {
            self.drop_terminal_edge(cx);
            return;
        }
        if self.terminal.selecting {
            let spot = self.terminal_spot(event.position, window, cx);
            if let (Some(spot), Some(tab)) =
                (spot, self.terminal.tabs.get_mut(self.terminal.active))
            {
                tab.screen.extend_selection(spot);
                cx.notify();
            }
            return;
        }
        let Some((from, height)) = self.terminal.drag else {
            return;
        };
        let moved = (from - event.position.y).as_f32();
        self.terminal.height = (height + moved).clamp(HEIGHTS.0, HEIGHTS.1);
        cx.notify();
    }

    pub fn drop_terminal_edge(&mut self, cx: &mut Context<Self>) {
        self.terminal.drag = None;
        self.terminal.selecting = false;
        cx.notify();
    }
}
