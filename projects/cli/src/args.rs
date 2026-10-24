use clap::Command;

use crate::constants::APP_NAME;

#[derive(Debug)]
pub struct AppArgs;

pub fn parse_args() -> AppArgs {
    Command::new(APP_NAME)
        .version(env!("CARGO_PKG_VERSION"))
        .about("Rust harness core scaffold")
        .get_matches();
    AppArgs
}
