use std::path::PathBuf;

use clap::{Arg, Command, value_parser};

use crate::constants::APP_NAME;

#[derive(Debug)]
pub struct AppArgs {
    pub state_db: Option<PathBuf>,
}

pub fn parse_args() -> AppArgs {
    let matches = Command::new(APP_NAME)
        .version(env!("CARGO_PKG_VERSION"))
        .about("Rust harness core")
        .arg(
            Arg::new("state-db")
                .long("state-db")
                .value_name("PATH")
                .value_parser(value_parser!(PathBuf))
                .help("SQLite state database (default: ~/.config/duskcode/session/state.db)"),
        )
        .get_matches();
    AppArgs {
        state_db: matches.get_one::<PathBuf>("state-db").cloned(),
    }
}
