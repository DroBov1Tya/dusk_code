//! `duskcode remove`: takes duskcode off this machine after one question. The core works
//! out and deletes what goes; this asks and prints. The desktop client runs it as
//! `remove --yes` after it has closed, having asked the user itself.

use std::io::{self, Write};

use anyhow::Result;
use dusk_core::app::{Paths, Uninstall};

use super::args::RemoveOptions;

pub fn run(paths: &Paths, options: &RemoveOptions) -> Result<()> {
    let removal = Uninstall::plan(paths, options.files, options.assume_yes)?;
    println!("This removes:");
    println!("  {}", removal.exe.display());
    for link in &removal.links {
        println!("  {} (link)", link.display());
    }
    if let Some(data) = &removal.data {
        println!("  {} (application data)", data.display());
        println!(
            "    {} (all versioned resources)",
            data.join("resources").display()
        );
    }
    println!(
        "  {} (config, history, skills, agents) and the keys in the system keychain",
        removal.config.display()
    );
    for extra in &removal.extras {
        println!("  {} (desktop client)", extra.display());
    }
    if removal.files {
        println!("  {} (the chats' files)", removal.workspace.display());
    } else {
        println!("Thread files in {} are kept.", removal.workspace.display());
    }
    println!(
        "The coordination lock {} is kept.",
        removal.lock_path().display()
    );
    if !options.assume_yes {
        print!("Remove duskcode? [y/N] ");
        io::stdout().flush()?;
        let mut answer = String::new();
        io::stdin().read_line(&mut answer)?;
        if !matches!(answer.trim(), "y" | "Y" | "yes") {
            println!("Nothing removed.");
            return Ok(());
        }
    }
    removal.run(|error| eprintln!("duskcode: {error:#}; delete them in the keychain app"))?;
    println!("duskcode is removed.");
    Ok(())
}
