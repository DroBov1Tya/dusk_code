//! The command line.

use std::path::PathBuf;

use clap::{Arg, ArgAction, Command, value_parser};

pub struct Options {
    pub tasks: PathBuf,
    pub only: Vec<String>,
    pub config: PathBuf,
    /// In place of the shipped MAIN prompt, to compare two.
    pub main_prompt: Option<PathBuf>,
    pub mode: Option<String>,
    pub runs: usize,
    pub timeout_seconds: u64,
    pub out: PathBuf,
}

pub enum Args {
    /// Each check must fail on the untouched task and pass on its known solution.
    Selfcheck {
        tasks: PathBuf,
        only: Vec<String>,
    },
    Compare {
        before: PathBuf,
        after: PathBuf,
    },
    Run(Options),
}

pub fn parse_args() -> Args {
    let home = std::env::var_os("HOME").map_or_else(PathBuf::new, PathBuf::from);
    let matches = Command::new("dusk-evals")
        .about("Benchmark tasks for duskcode agents, with automatic checks")
        .arg(
            Arg::new("tasks")
                .long("tasks")
                .default_value("evals/tasks")
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("only")
                .long("only")
                .action(ArgAction::Append)
                .help("Run this task only; may be repeated"),
        )
        .arg(
            Arg::new("config")
                .long("config")
                .value_parser(value_parser!(PathBuf))
                .help("config.yml with the provider and model; default the user's"),
        )
        .arg(
            Arg::new("main-prompt")
                .long("main-prompt")
                .value_parser(value_parser!(PathBuf))
                .help("Use this file as the MAIN prompt"),
        )
        .arg(
            Arg::new("mode")
                .long("mode")
                .help("MAIN, ORCHESTRATOR or another mode"),
        )
        .arg(
            Arg::new("runs")
                .long("runs")
                .default_value("1")
                .value_parser(value_parser!(usize))
                .help("Runs of each task; models are not deterministic"),
        )
        .arg(
            Arg::new("timeout")
                .long("timeout")
                .default_value("600")
                .value_parser(value_parser!(u64))
                .help("Seconds a task may take"),
        )
        .arg(
            Arg::new("out")
                .long("out")
                .default_value("evals/results/latest.json")
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("selfcheck")
                .long("selfcheck")
                .action(ArgAction::SetTrue)
                .help("Check the checks; needs no model"),
        )
        .arg(
            Arg::new("compare")
                .long("compare")
                .num_args(2)
                .value_names(["BEFORE", "AFTER"])
                .value_parser(value_parser!(PathBuf))
                .help("Compare two result files"),
        )
        .get_matches();
    let tasks = matches
        .get_one::<PathBuf>("tasks")
        .cloned()
        .unwrap_or_default();
    let only: Vec<String> = matches
        .get_many::<String>("only")
        .map(|names| names.cloned().collect())
        .unwrap_or_default();
    if matches.get_flag("selfcheck") {
        return Args::Selfcheck { tasks, only };
    }
    if let Some(files) = matches.get_many::<PathBuf>("compare") {
        let files: Vec<PathBuf> = files.cloned().collect();
        return Args::Compare {
            before: files[0].clone(),
            after: files[1].clone(),
        };
    }
    Args::Run(Options {
        tasks,
        only,
        config: matches
            .get_one::<PathBuf>("config")
            .cloned()
            .unwrap_or_else(|| home.join(".config/duskcode/config.yml")),
        main_prompt: matches.get_one::<PathBuf>("main-prompt").cloned(),
        mode: matches.get_one::<String>("mode").cloned(),
        runs: matches.get_one::<usize>("runs").copied().unwrap_or(1),
        timeout_seconds: matches.get_one::<u64>("timeout").copied().unwrap_or(600),
        out: matches
            .get_one::<PathBuf>("out")
            .cloned()
            .unwrap_or_default(),
    })
}
