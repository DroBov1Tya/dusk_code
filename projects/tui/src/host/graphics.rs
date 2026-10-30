//! Pictures in full resolution where the terminal draws them: the kitty, iTerm2 or sixel
//! protocol, asked of the terminal once at the start. Elsewhere the half-block previews
//! stay. DUSKCODE_GRAPHICS picks one by hand: kitty, iterm2 or sixel, or off for the
//! half blocks everywhere.

use std::{cell::RefCell, collections::VecDeque, env, process::Command, sync::OnceLock};

use dusk_core::state::artifacts::ImageArtifact;
use image::DynamicImage;
use ratatui::{Frame, layout::Rect, widgets::Clear};
use ratatui_image::{
    Image, Resize,
    picker::{Picker, ProtocolType},
    protocol::Protocol,
};

/// Pictures encoded for the terminal, by artifact and size; older ones are dropped.
const CACHE_SIZE: usize = 16;

static PICKER: OnceLock<Option<Picker>> = OnceLock::new();

/// Asks the terminal what it draws. Called once, after the screen is taken and before
/// any event is read, since the answers come on stdin.
pub fn detect() {
    PICKER.get_or_init(|| {
        let chosen = match env::var("DUSKCODE_GRAPHICS").as_deref() {
            Ok("off") => return None,
            Ok("kitty") => Some(ProtocolType::Kitty),
            Ok("iterm2") => Some(ProtocolType::Iterm2),
            Ok("sixel") => Some(ProtocolType::Sixel),
            _ => None,
        };
        // The query also measures the cells, which a chosen protocol needs as well.
        let mut picker = Picker::from_query_stdio().ok()?;
        if let Some(protocol) = chosen {
            picker.set_protocol_type(protocol);
        }
        let drawn = picker.protocol_type() != ProtocolType::Halfblocks;
        // Inside tmux the pictures go wrapped for it; one it was not recognised in would
        // swallow them.
        let tmux = env::var_os("TMUX").is_some_and(|value| !value.is_empty());
        let reaches = match (tmux, picker.tmux_detected()) {
            (false, false) => true,
            (_, true) => tmux_passes_through(),
            (true, false) => false,
        };
        (drawn && reaches).then_some(picker)
    });
}

/// tmux hands the pictures on to the terminal only with allow-passthrough set; without
/// it they would leave a blank where the preview was. The query sets it for this pane
/// where tmux allows that.
fn tmux_passes_through() -> bool {
    Command::new("tmux")
        .args(["show", "-Apv", "allow-passthrough"])
        .output()
        .is_ok_and(|output| matches!(output.stdout.trim_ascii(), b"on" | b"all"))
}

type Entry = (String, Rect, Option<Protocol>);

#[derive(Default)]
pub struct Pictures {
    cache: RefCell<VecDeque<Entry>>,
}

impl Pictures {
    /// Draws `artifact` over `area`, where its half-block preview already is. A picture
    /// that cannot be decoded or encoded leaves the preview in place.
    pub fn draw(
        &self,
        frame: &mut Frame,
        area: Rect,
        artifact: &ImageArtifact,
        decode: impl FnOnce() -> Option<DynamicImage>,
    ) {
        let Some(Some(picker)) = PICKER.get() else {
            return;
        };
        let size = Rect { x: 0, y: 0, ..area };
        let mut cache = self.cache.borrow_mut();
        let known = cache
            .iter()
            .position(|(id, known, _)| *id == artifact.id && *known == size)
            .and_then(|index| cache.remove(index));
        let entry = known.unwrap_or_else(|| {
            let protocol = decode().and_then(|image| {
                picker
                    .new_protocol(image, size.as_size(), Resize::Fit(None))
                    .ok()
            });
            (artifact.id.clone(), size, protocol)
        });
        if let Some(protocol) = &entry.2 {
            frame.render_widget(Clear, area);
            frame.render_widget(Image::new(protocol), area);
        }
        if cache.len() == CACHE_SIZE {
            cache.pop_front();
        }
        cache.push_back(entry);
    }
}
