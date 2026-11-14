//! A notice in the system's notification area and a short sound, for a client that is
//! not in front when a chat finishes. Nothing here waits for the system: the helper
//! program is started and left to finish, and a missing one means no notice.

use std::process::{Command, Stdio};

const MAX_TITLE_CHARS: usize = 80;
const MAX_BODY_CHARS: usize = 200;
#[cfg(target_os = "macos")]
const SOUND: &str = "/System/Library/Sounds/Glass.aiff";

/// Posts a notice. The text may come from the model or a chat's title, so control
/// characters are dropped and the length is capped; it reaches the helper as arguments,
/// never as part of a script or a shell line.
pub fn show(title: &str, body: &str) {
    let (title, body) = (clean(title, MAX_TITLE_CHARS), clean(body, MAX_BODY_CHARS));
    #[cfg(target_os = "macos")]
    let command = {
        let mut command = Command::new("osascript");
        command
            .args([
                "-e",
                "on run argv",
                "-e",
                "display notification (item 2 of argv) with title (item 1 of argv)",
                "-e",
                "end run",
                "--",
            ])
            .args([title, body]);
        command
    };
    #[cfg(target_os = "linux")]
    let command = {
        let mut command = Command::new("notify-send");
        command.args(["--app-name=duskcode", "--", &title, &body]);
        command
    };
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    start(command);
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let _ = (title, body);
}

/// A short sound where the system has one to play from a program.
pub fn sound() {
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("afplay");
        command.arg(SOUND);
        start(command);
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn start(mut command: Command) {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // The program may be missing; a notice is a courtesy, so that is not an error.
    if let Ok(mut child) = command.spawn() {
        // Reaped on a thread of its own, so a finished helper is not left a zombie.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

fn clean(text: &str, max: usize) -> String {
    let single: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    single
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::clean;

    #[test]
    fn text_for_a_notice_is_one_short_clean_line() {
        assert_eq!(clean("a\x1b[2J\nb\tc", 20), "a [2J b c");
        assert_eq!(clean(&"x".repeat(500), 80).len(), 80);
        assert_eq!(clean("  \n ", 10), "");
    }
}
