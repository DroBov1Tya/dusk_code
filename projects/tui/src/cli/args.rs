use std::path::PathBuf;

use clap::{Arg, Command, value_parser};

pub enum Args {
    Run {
        state_db: Option<PathBuf>,
        /// Open this thread instead of a new one.
        thread: Option<String>,
    },
    Remove,
    /// Restore data offline, before config, logs or SQLite are opened.
    Restore {
        archive: PathBuf,
    },
    /// Explicitly resume a journaled offline restore after interruption.
    RecoverRestore,
    /// Install the newest version on the configured channel, without the screen.
    Update,
    /// Internal installer entry point; does not initialize config or start the UI.
    InstallResources {
        directory: PathBuf,
    },
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
        .arg(
            Arg::new("update")
                .long("update")
                .action(clap::ArgAction::SetTrue)
                .help("Install the newest release, or pre-release on the dev channel, and exit"),
        )
        .arg(
            Arg::new("install-resources")
                .long("install-resources")
                .value_name("PATH")
                .value_parser(value_parser!(PathBuf))
                .hide(true),
        )
        .subcommand(Command::new("remove").about(
            "Remove duskcode, its resources, links and ~/.config/duskcode after one question",
        ))
        .subcommand(
            Command::new("restore")
                .about("Restore a private migration archive offline into absent or empty data roots")
                .long_about("Restore a /migrate archive before starting duskcode. Close every other duskcode process first. Both ~/.config/duskcode and ~/Documents/duskcode must be absent or empty, real directories. Existing data is never merged or replaced. An interrupted restore blocks normal startup; use duskcode restore --recover to validate and finish its journaled installation. Archives include API keys and chat history; keep them private. Absolute project paths inside saved data are not rewritten.")
                .arg(
                    Arg::new("archive")
                        .required_unless_present("recover")
                        .conflicts_with("recover")
                        .value_name("ARCHIVE")
                        .value_parser(value_parser!(PathBuf)),
                )
                .arg(Arg::new("recover")
                    .long("recover")
                    .action(clap::ArgAction::SetTrue)
                    .help("Validate and finish an interrupted restore from its private transaction journal")),
        )
        .get_matches();
    if let Some(restore) = matches.subcommand_matches("restore") {
        if restore.get_flag("recover") {
            return Args::RecoverRestore;
        }
        return Args::Restore {
            archive: restore
                .get_one::<PathBuf>("archive")
                .expect("required archive")
                .clone(),
        };
    }
    if let Some(directory) = matches.get_one::<PathBuf>("install-resources") {
        return Args::InstallResources {
            directory: directory.clone(),
        };
    }
    if matches.subcommand_matches("remove").is_some() {
        return Args::Remove;
    }
    if matches.get_flag("update") {
        return Args::Update;
    }
    Args::Run {
        state_db: matches.get_one::<PathBuf>("state-db").cloned(),
        thread: matches.get_one::<String>("thread").cloned(),
    }
}
