//! A key as the bytes a terminal sends for it. The platform's main key is not the
//! shell's: those presses stay the window's own shortcuts.

use gpui_kit::Keystroke;

/// The bytes for a key, or none when the key is the window's. `app_cursor` is the mode a
/// full-screen program sets to get its arrows another way.
pub fn bytes(keystroke: &Keystroke, app_cursor: bool) -> Option<Vec<u8>> {
    let modifiers = keystroke.modifiers;
    if modifiers.platform {
        return None;
    }
    let key = keystroke.key.as_str();
    // As xterm counts them: 1, plus shift 1, alt 2, control 4.
    let held = 1
        + u8::from(modifiers.shift)
        + 2 * u8::from(modifiers.alt)
        + 4 * u8::from(modifiers.control);
    let escape = |tail: &str| Some(format!("\x1b{tail}").into_bytes());
    let arrow = |letter: char| match (held, app_cursor) {
        (1, true) => escape(&format!("O{letter}")),
        (1, false) => escape(&format!("[{letter}")),
        _ => escape(&format!("[1;{held}{letter}")),
    };
    let tilde = |number: u8| match held {
        1 => escape(&format!("[{number}~")),
        _ => escape(&format!("[{number};{held}~")),
    };
    match key {
        "enter" => return Some(vec![b'\r']),
        "tab" if modifiers.shift => return escape("[Z"),
        "tab" => return Some(vec![b'\t']),
        "backspace" if modifiers.alt => return Some(vec![0x1b, 0x7f]),
        "backspace" => return Some(vec![0x7f]),
        "escape" => return Some(vec![0x1b]),
        "space" if modifiers.control => return Some(vec![0]),
        "up" => return arrow('A'),
        "down" => return arrow('B'),
        "right" => return arrow('C'),
        "left" => return arrow('D'),
        "home" => return arrow('H'),
        "end" => return arrow('F'),
        "insert" => return tilde(2),
        "delete" => return tilde(3),
        "pageup" => return tilde(5),
        "pagedown" => return tilde(6),
        "f1" => return escape("OP"),
        "f2" => return escape("OQ"),
        "f3" => return escape("OR"),
        "f4" => return escape("OS"),
        "f5" => return tilde(15),
        "f6" => return tilde(17),
        "f7" => return tilde(18),
        "f8" => return tilde(19),
        "f9" => return tilde(20),
        "f10" => return tilde(21),
        "f11" => return tilde(23),
        "f12" => return tilde(24),
        _ => {}
    }
    let mut letters = key.chars();
    let single = match (letters.next(), letters.next()) {
        (Some(letter), None) => Some(letter),
        _ => None,
    };
    if modifiers.control
        && let Some(letter) = single
    {
        // Control takes the letter's low five bits: C-a is 1, C-[ is Escape.
        let code = match letter.to_ascii_lowercase() {
            letter @ 'a'..='z' => letter as u8 - b'a' + 1,
            '@' | '2' => 0,
            '[' => 0x1b,
            '\\' => 0x1c,
            ']' => 0x1d,
            '^' | '6' => 0x1e,
            '_' | '-' => 0x1f,
            _ => return None,
        };
        return Some(match modifiers.alt {
            true => vec![0x1b, code],
            false => vec![code],
        });
    }
    if modifiers.alt
        && let Some(letter) = single
    {
        // Alt is Meta: Escape, then the key itself, not the sign Option types on a Mac.
        let letter = match modifiers.shift {
            true => letter.to_ascii_uppercase(),
            false => letter,
        };
        let mut sent = vec![0x1b];
        sent.extend_from_slice(letter.encode_utf8(&mut [0; 4]).as_bytes());
        return Some(sent);
    }
    match (&keystroke.key_char, key) {
        (Some(typed), _) if !typed.is_empty() => Some(typed.clone().into_bytes()),
        (_, "space") => Some(vec![b' ']),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(source: &'static str) -> Keystroke {
        let mut keystroke = Keystroke::parse(source).unwrap();
        let plain = !keystroke.modifiers.control && !keystroke.modifiers.alt;
        if keystroke.key.chars().count() == 1 && plain {
            keystroke.key_char = Some(keystroke.key.clone());
        }
        keystroke
    }

    #[test]
    fn keys_become_what_a_terminal_sends() {
        assert_eq!(bytes(&press("a"), false), Some(b"a".to_vec()));
        assert_eq!(bytes(&press("enter"), false), Some(b"\r".to_vec()));
        assert_eq!(bytes(&press("ctrl-c"), false), Some(vec![3]));
        assert_eq!(bytes(&press("ctrl-["), false), Some(vec![0x1b]));
        assert_eq!(bytes(&press("alt-b"), false), Some(b"\x1bb".to_vec()));
        assert_eq!(bytes(&press("up"), false), Some(b"\x1b[A".to_vec()));
        assert_eq!(bytes(&press("up"), true), Some(b"\x1bOA".to_vec()));
        assert_eq!(
            bytes(&press("ctrl-right"), false),
            Some(b"\x1b[1;5C".to_vec())
        );
        assert_eq!(bytes(&press("shift-tab"), false), Some(b"\x1b[Z".to_vec()));
        assert_eq!(bytes(&press("delete"), false), Some(b"\x1b[3~".to_vec()));
        // The window's own shortcuts never reach the shell.
        assert_eq!(bytes(&press("cmd-k"), false), None);
    }
}
