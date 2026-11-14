//! The parts plugins add to the window. The core lists them in the status, runs the
//! plugins and sends the views of the parts this window opened; this module places their
//! icons, opens them where their manifest says and draws their views with the app's own
//! widgets. A plugin never draws itself.
//!
//! Placement survives many plugins: each place (header, rail, beside the message box)
//! shows at most `place::SHOWN` pinned icons; every part is in the menu behind the one
//! Plugins button in the header, where it is pinned or unpinned. Pins are this window's
//! look, kept in `gui.json`.
//!
//! - `place`: which parts are offered and pinned, and their icons in each place;
//! - `menu`: the menu of all parts: search, a fold per plugin, a pin per part;
//! - `dock`: the right and bottom docks, a tab per open part;
//! - `view`: a part's widget tree drawn; `fields` keeps its text boxes;
//! - `frames`: the pictures of screen widgets, decoded off the window's thread;
//! - `link`: what opening, closing and using a part asks the core, and what it sends;
//! - `settings`: the Settings page of the parts placed there.

mod dock;
mod fields;
mod frames;
mod link;
mod menu;
pub mod place;
mod settings;
mod view;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use dusk_protocol::plugin::{Part, Widget};
use gpui_kit::{
    component::input::{InputEvent, InputState},
    *,
};

use crate::{ui::phone::Drawn, workspace::Workspace};

pub use dock::{bottom as bottom_dock, right as right_dock};
pub use menu::render as menu;
pub use settings::{any as any_settings, render as settings_page};
pub use view::{Room, render as view};

/// One part of one plugin.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key {
    pub plugin: String,
    pub part: String,
}

impl Key {
    pub fn of(part: &Part) -> Self {
        Self {
            plugin: part.plugin.clone(),
            part: part.id.clone(),
        }
    }

    /// The name its pin is kept under.
    pub fn pin(&self) -> String {
        format!("{}/{}", self.plugin, self.part)
    }
}

/// A widget of an open part.
type Slot = (Key, String);

/// A dock's tabs: the open parts, the shown one, and whether it is folded away.
#[derive(Default)]
pub struct Dock {
    pub tabs: Vec<Key>,
    pub active: usize,
    pub collapsed: bool,
    /// The strip of tabs, scrolled to the shown one when a part comes forward.
    strip: ScrollHandle,
}

pub struct Plugins {
    pub right: Dock,
    pub bottom: Dock,
    /// The bottom dock's height, and the pointer's height and the dock's when a drag of
    /// its edge began.
    pub bottom_height: f32,
    pub bottom_drag: Option<(Pixels, f32)>,
    /// The part shown on the Settings page of plugins.
    pub settings: Option<Key>,
    /// The latest view of each open part.
    views: HashMap<Key, Widget>,
    /// The newest picture of each screen widget; replaced ones wait in `retired` for the
    /// next draw to free their textures.
    frames: HashMap<Slot, Arc<RenderImage>>,
    retired: Vec<Arc<RenderImage>>,
    /// Screens being decoded, and the newest picture that came meanwhile.
    decoding: HashSet<Slot>,
    waiting: HashMap<Slot, Vec<u8>>,
    /// Where each screen widget was drawn, to turn a click into its units.
    drawn: HashMap<Slot, Drawn>,
    /// The text boxes of open parts, by widget.
    fields: HashMap<Slot, fields::Field>,
    /// The list shown as a menu that is open.
    open_list: Option<Slot>,
    /// The menu of all parts is open; when it closed, so the click that closed it does
    /// not open it again.
    pub menu: bool,
    menu_closed: Option<std::time::Instant>,
    pub search: Entity<InputState>,
    /// Plugins whose fold in the menu was turned from its default.
    turned: HashSet<String>,
}

impl Plugins {
    pub fn new(window: &mut Window, cx: &mut Context<Workspace>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Find a plugin"));
        cx.subscribe_in(&search, window, |_, _, _: &InputEvent, _, cx| cx.notify())
            .detach();
        Self {
            right: Dock::default(),
            bottom: Dock::default(),
            bottom_height: dock::HEIGHT,
            bottom_drag: None,
            settings: None,
            views: HashMap::new(),
            frames: HashMap::new(),
            retired: Vec::new(),
            decoding: HashSet::new(),
            waiting: HashMap::new(),
            drawn: HashMap::new(),
            fields: HashMap::new(),
            open_list: None,
            menu: false,
            menu_closed: None,
            search,
            turned: HashSet::new(),
        }
    }

    /// Whether the part is open anywhere in this window.
    fn is_open(&self, key: &Key, modal: Option<&crate::modal::Modal>) -> bool {
        self.right.tabs.contains(key)
            || self.bottom.tabs.contains(key)
            || self.settings.as_ref() == Some(key)
            || matches!(modal, Some(crate::modal::Modal::Plugin(shown)) if shown == key)
    }

    /// Forgets what the window kept for a part that closed.
    fn forget(&mut self, key: &Key) {
        self.views.remove(key);
        let retired: Vec<Slot> = self
            .frames
            .keys()
            .filter(|(k, _)| k == key)
            .cloned()
            .collect();
        for slot in retired {
            self.retired.extend(self.frames.remove(&slot));
        }
        self.waiting.retain(|(k, _), _| k != key);
        self.drawn.retain(|(k, _), _| k != key);
        self.fields.retain(|(k, _), _| k != key);
        if self.open_list.as_ref().is_some_and(|(k, _)| k == key) {
            self.open_list = None;
        }
    }
}

/// Before the window draws: frees the textures of replaced pictures and gives every text
/// box of the open views its state.
pub fn settle(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    for old in ws.plugins.retired.drain(..) {
        let _ = window.drop_image(old);
    }
    fields::settle(ws, window, cx);
    let plugins = &mut ws.plugins;
    let mut screens = HashSet::new();
    for (key, root) in &plugins.views {
        view::each(root, &mut |widget| {
            if let Widget::Screen { id, .. } = widget {
                screens.insert((key.clone(), id.clone()));
            }
        });
    }
    plugins.drawn.retain(|slot, _| screens.contains(slot));
    for slot in screens {
        plugins.drawn.entry(slot).or_default();
    }
}

/// The part the status lists under this key.
pub fn part<'a>(ws: &'a Workspace, key: &Key) -> Option<&'a Part> {
    ws.status
        .plugin_parts
        .iter()
        .find(|part| part.plugin == key.plugin && part.id == key.part)
}

/// The part's title, or its id while the status does not list it.
pub fn title(ws: &Workspace, key: &Key) -> String {
    part(ws, key).map_or_else(|| key.part.clone(), |part| part.title.clone())
}
