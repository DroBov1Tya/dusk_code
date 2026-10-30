//! Pictures in full resolution where the terminal draws them: the kitty, iTerm2 or sixel
//! protocol, asked of the terminal once at the start. Elsewhere the half-block previews
//! stay. DUSKCODE_GRAPHICS picks one by hand: kitty, iterm2 or sixel, or off for the
//! half blocks everywhere.

use std::{
    cell::RefCell,
    collections::VecDeque,
    env,
    process::Command,
    sync::{Arc, OnceLock},
};

use dusk_core::state::artifacts::ImageArtifact;
use image::{DynamicImage, imageops::FilterType};
use ratatui::{
    Frame,
    layout::{Rect, Size},
    widgets::Clear,
};
use ratatui_image::{
    Image, Resize,
    picker::{Picker, ProtocolType},
    protocol::Protocol,
};

/// Pictures encoded for the terminal, by artifact, size and visible slice; older ones
/// are dropped.
const CACHE_SIZE: usize = 16;
/// Images scaled to their cards, which slices are cut from while a card scrolls.
const FITTED_SIZE: usize = 4;

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

/// The artifact, the card's size in cells, and its rows cut above and shown.
type Key = (String, Size, u16, u16);
type Fitted = (String, Size, Option<Arc<DynamicImage>>);

#[derive(Default)]
pub struct Pictures {
    encoded: RefCell<VecDeque<(Key, Option<Protocol>)>>,
    fitted: RefCell<VecDeque<Fitted>>,
}

impl Pictures {
    /// Draws the rows of `artifact` that show in `area`, over its half-block preview.
    /// The card is `card` cells in all, `skip` rows of it above the screen. A picture
    /// that cannot be decoded or encoded leaves the preview in place.
    pub fn draw(
        &self,
        frame: &mut Frame,
        area: Rect,
        card: Size,
        skip: u16,
        artifact: &ImageArtifact,
        decode: impl FnOnce() -> Option<DynamicImage>,
    ) {
        let Some(Some(picker)) = PICKER.get() else {
            return;
        };
        let key = (artifact.id.clone(), card, skip, area.height);
        let mut encoded = self.encoded.borrow_mut();
        let known = encoded
            .iter()
            .position(|(known, _)| *known == key)
            .and_then(|index| encoded.remove(index));
        let entry = known.unwrap_or_else(|| {
            let protocol = self
                .fitted(picker, artifact, card, decode)
                .and_then(|image| {
                    let font = picker.font_size();
                    let cut = image.crop_imm(
                        0,
                        u32::from(skip) * u32::from(font.height),
                        image.width(),
                        u32::from(area.height) * u32::from(font.height),
                    );
                    let size = Size::new(card.width, area.height);
                    picker.new_protocol(cut, size, Resize::Fit(None)).ok()
                });
            (key, protocol)
        });
        if let Some(protocol) = &entry.1 {
            frame.render_widget(Clear, area);
            frame.render_widget(Image::new(protocol), area);
        }
        if encoded.len() == CACHE_SIZE {
            encoded.pop_front();
        }
        encoded.push_back(entry);
    }

    /// The image scaled smoothly to the card's pixels, the terminal's cells measured.
    fn fitted(
        &self,
        picker: &Picker,
        artifact: &ImageArtifact,
        card: Size,
        decode: impl FnOnce() -> Option<DynamicImage>,
    ) -> Option<Arc<DynamicImage>> {
        let mut fitted = self.fitted.borrow_mut();
        if let Some((_, _, image)) = fitted
            .iter()
            .find(|(id, size, _)| *id == artifact.id && *size == card)
        {
            return image.clone();
        }
        let image = decode().map(|image| {
            let resize = Resize::Fit(Some(FilterType::Lanczos3));
            Arc::new(resize.resize(&image, picker.font_size(), card, None))
        });
        if fitted.len() == FITTED_SIZE {
            fitted.pop_front();
        }
        fitted.push_back((artifact.id.clone(), card, image.clone()));
        image
    }
}
