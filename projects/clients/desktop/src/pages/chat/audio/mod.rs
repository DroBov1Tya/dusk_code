//! Sound in the chat: a clip the agent made, or an answer read aloud. `card` draws a clip,
//! `player` plays one at a time, and `menu` is what the `...` on a card offers.

mod card;
mod menu;
mod player;

pub use card::card;
pub use menu::menu;
pub use player::Audio;

#[cfg(test)]
mod tests {
    // Named imports: a glob would bring gpui's own `test` macro over the standard one.
    use super::player::sound_path;

    #[test]
    fn a_sound_path_takes_only_an_id() {
        let folder = std::path::Path::new("/artifacts");
        assert!(sound_path(folder, "0192-ab").is_some());
        assert!(sound_path(folder, "../etc").is_none());
        assert!(sound_path(folder, "").is_none());
    }
}
