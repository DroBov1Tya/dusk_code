mod card;
mod menu;
mod player;

pub use card::card;
pub use menu::menu;
pub use player::Audio;

#[cfg(test)]
mod tests {
    // Named imports: a glob would bring gpui's own `test` macro over the standard one.
    use super::card::clock;
    use super::player::sound_path;
    use std::time::Duration;

    #[test]
    fn a_sound_path_takes_only_an_id() {
        let folder = std::path::Path::new("/artifacts");
        assert!(sound_path(folder, "0192-ab").is_some());
        assert!(sound_path(folder, "../etc").is_none());
        assert!(sound_path(folder, "").is_none());
    }

    #[test]
    fn time_reads_as_minutes_and_seconds() {
        assert_eq!(clock(Duration::from_secs(6)), "0:06");
        assert_eq!(clock(Duration::from_secs(75)), "1:15");
        assert_eq!(clock(Duration::from_secs(3723)), "1:02:03");
    }
}
