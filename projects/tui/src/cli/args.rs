use std::path::PathBuf;

use clap::{Arg, Command, value_parser};

pub enum Args {
    Run {
        state_db: Option<PathBuf>,
        /// Open this thread instead of a new one.
        thread: Option<String>,
    },
    Remove,
    /// One prompt in, the answer out, without the screen.
    Headless(Headless),
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

pub struct Headless {
    /// `-` reads the prompt from standard input.
    pub prompt: String,
    pub project: Option<PathBuf>,
    pub access: Option<String>,
    pub timeout_seconds: Option<u64>,
    pub json: bool,
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
            Command::new("run")
                .about("Run one prompt without the screen and print the answer")
                .long_about("Runs one prompt in a new chat with your config and prints the answer to standard output; progress goes to standard error. A request for approval is denied unless --access allows it, and a question is answered with a note that nobody is there. Exit code 0: the turn completed; 1: it failed or timed out; 130: interrupted.")
                .arg(Arg::new("prompt").required(true).value_name("PROMPT").help("The prompt, or - to read it from standard input"))
                .arg(
                    Arg::new("project")
                        .long("project")
                        .value_name("DIR")
                        .value_parser(value_parser!(PathBuf))
                        .help("The folder to work in (default: the current one)"),
                )
                .arg(
                    Arg::new("access")
                        .long("access")
                        .value_name("MODE")
                        .value_parser(["ask", "sandbox", "arbiter", "full"])
                        .help("Approval mode of this chat (default: the one in the settings)"),
                )
                .arg(
                    Arg::new("timeout")
                        .long("timeout")
                        .value_name("SECONDS")
                        .value_parser(value_parser!(u64))
                        .help("Stop the chat after this long and exit with 1"),
                )
                .arg(
                    Arg::new("json")
                        .long("json")
                        .action(clap::ArgAction::SetTrue)
                        .help("Print one JSON object with the answer and the usage"),
                ),
        )
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
    if let Some(run) = matches.subcommand_matches("run") {
        return Args::Headless(Headless {
            prompt: run.get_one::<String>("prompt").expect("required").clone(),
            project: run.get_one::<PathBuf>("project").cloned(),
            access: run.get_one::<String>("access").cloned(),
            timeout_seconds: run.get_one::<u64>("timeout").copied(),
            json: run.get_flag("json"),
        });
    }
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
