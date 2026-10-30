//! Copying text out of the screen. Mouse capture takes selection away from the terminal,
//! so the client copies itself: with the system tool where there is one, and with the
//! OSC 52 sequence, which terminals support over SSH too.

use std::{
    io::{Write, stdout},
    process::{Command, Stdio},
};

/// Tools that take text on stdin and put it on the clipboard, tried in order.
const TOOLS: [(&str, &[&str]); 3] = [
    ("pbcopy", &[]),
    ("wl-copy", &[]),
    ("xclip", &["-selection", "clipboard"]),
];

/// True when some way of copying worked.
pub fn copy(text: &str) -> bool {
    let tool = TOOLS.iter().any(|(name, args)| pipe(name, args, text));
    let sequence = format!("\x1b]52;c;{}\x07", base64(text.as_bytes()));
    let terminal = stdout()
        .write_all(sequence.as_bytes())
        .and_then(|()| stdout().flush())
        .is_ok();
    tool || terminal
}

fn pipe(name: &str, args: &[&str], text: &str) -> bool {
    let Ok(mut child) = Command::new(name)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let written = child
        .stdin
        .take()
        .is_some_and(|mut stdin| stdin.write_all(text.as_bytes()).is_ok());
    child.wait().is_ok_and(|status| status.success()) && written
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let group = chunk.iter().enumerate().fold(0u32, |group, (at, &byte)| {
            group | u32::from(byte) << (16 - 8 * at)
        });
        for at in 0..4 {
            if at <= chunk.len() {
                encoded.push(ALPHABET[(group >> (18 - 6 * at) & 63) as usize] as char);
            } else {
                encoded.push('=');
            }
        }
    }
    encoded
}
