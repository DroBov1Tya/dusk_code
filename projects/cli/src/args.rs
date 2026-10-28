use std::path::PathBuf;

use clap::{Arg, ArgAction, Command, value_parser};

use crate::constants::APP_NAME;

#[derive(Debug)]
pub struct AppArgs {
    pub state_db: Option<PathBuf>,
    /// Continue this thread instead of starting a new one.
    pub thread: Option<String>,
    /// Print the threads and exit.
    pub list_threads: bool,
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
        .arg(
            Arg::new("thread")
                .long("thread")
                .value_name("ID")
                .help("Continue a thread, showing its history first"),
        )
        .arg(
            Arg::new("threads")
                .long("threads")
                .action(ArgAction::SetTrue)
                .conflicts_with("thread")
                .help("List the threads, the most recently active first, and exit"),
        )
        .get_matches();
    AppArgs {
        state_db: matches.get_one::<PathBuf>("state-db").cloned(),
        thread: matches.get_one::<String>("thread").cloned(),
        list_threads: matches.get_flag("threads"),
    }
}
