use std::path::PathBuf;

use clap::{Arg, Command, value_parser};

pub struct Args {
    pub state_db: Option<PathBuf>,
    /// Open this thread instead of a new one.
    pub thread: Option<String>,
}

pub fn parse_args() -> Args {
    let matches = Command::new("dusk")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Full-screen terminal client for dusk_code")
        .arg(
            Arg::new("state-db")
                .long("state-db")
                .value_name("PATH")
                .value_parser(value_parser!(PathBuf))
                .help("SQLite state database (default: ~/.config/duskcode/session/state.db)"),
        )
        .arg(
            Arg::new("thread")
                .long("thread")
                .value_name("ID")
                .help("Open a thread instead of starting a new one"),
        )
        .get_matches();
    Args {
        state_db: matches.get_one::<PathBuf>("state-db").cloned(),
        thread: matches.get_one::<String>("thread").cloned(),
    }
}
