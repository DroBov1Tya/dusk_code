//! The buttons under the message box: the commands used most.

use teloxide::types::{KeyboardButton, KeyboardMarkup};

pub fn keyboard() -> KeyboardMarkup {
    let row = |labels: [&str; 2]| labels.map(KeyboardButton::new).to_vec();
    KeyboardMarkup::new([
        row(["/stop", "/resume"]),
        row(["/status", "/approve"]),
        row(["/mode", "/model"]),
    ])
    .resize_keyboard()
    .persistent()
}
