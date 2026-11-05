//! Opens the system's own terminal in a folder. The folder is one argument, never
//! part of a shell line.

use std::{path::Path, process::Command};

use anyhow::{Context, Result};
#[cfg(not(target_os = "macos"))]
use anyhow::bail;

pub fn open(dir: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .args(["-a", "Terminal"])
            .arg(dir)
            .spawn()
            .context("cannot open Terminal")?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        // The user's choice first, then what desktops commonly ship.
        let preferred = std::env::var("TERMINAL").ok();
        let candidates = preferred
            .iter()
            .map(String::as_str)
            .chain(["x-terminal-emulator", "gnome-terminal", "konsole", "xfce4-terminal", "kitty", "alacritty", "wezterm", "foot", "xterm"]);
        for program in candidates {
            let mut command = Command::new(program);
            command.current_dir(dir);
            match program {
                "gnome-terminal" | "xfce4-terminal" => {
                    command.arg("--working-directory").arg(dir);
                }
                "konsole" => {
                    command.arg("--workdir").arg(dir);
                }
                _ => {}
            }
            if command.spawn().is_ok() {
                return Ok(());
            }
        }
        bail!("no terminal found; set TERMINAL to one")
    }
}
