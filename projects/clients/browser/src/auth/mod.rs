//! Who may use the page: the token traded for a session cookie, checked and dropped.

mod session;

pub use session::{is_signed_in, sign_in, sign_out, take_fragment_token};
