use std::path::PathBuf;

use clap::{Arg, Command, value_parser};

pub enum Args {
    Run {
        state_db: Option<PathBuf>,
        /// Open this thread instead of a new one.
        thread: Option<String>,
    },
    Remove,
}

pub fn parse_args() -> Args {
    let matches = Command::new("duskcode")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Terminal agent for coding, run in the current directory")
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
        .subcommand(
            Command::new("remove")
                .about("Remove duskcode, its links and ~/.config/duskcode after one question"),
        )
        .get_matches();
    if matches.subcommand_matches("remove").is_some() {
        return Args::Remove;
    }
    Args::Run {
        state_db: matches.get_one::<PathBuf>("state-db").cloned(),
        thread: matches.get_one::<String>("thread").cloned(),
    }
}
