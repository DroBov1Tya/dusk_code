use std::{env, path::PathBuf};

use anyhow::{Context, Result, ensure};
use dusk_core::{api::CORE_API_VERSION, state::State};
use tracing::{error, info};

use crate::args::AppArgs;

pub struct Application {
    state: State,
}

impl Application {
    pub async fn build(args: AppArgs) -> Result<Self> {
        let path = match args.state_db {
            Some(path) => path,
            None => default_state_db()?,
        };
        let state = match State::open(path).await {
            Ok(state) => {
                info!("✅ Инициализация: SQLite state | Статус: Успешно");
                state
            }
            Err(error) => {
                error!("❌ Инициализация: SQLite state | Статус: {}", error);
                return Err(error);
            }
        };
        Ok(Self { state })
    }

    pub async fn run(self) -> Result<()> {
        info!(
            "ℹ️ Действие: Проверка каркаса | Статус: Готово, Core API={}.{}",
            CORE_API_VERSION.major, CORE_API_VERSION.minor
        );
        self.state.close().await;
        Ok(())
    }
}

fn default_state_db() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    let directory =
        PathBuf::from(env::var_os("HOME").context("HOME is unavailable; use --state-db")?)
            .join("Library/Application Support");

    #[cfg(target_os = "windows")]
    let directory = PathBuf::from(
        env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is unavailable; use --state-db")?,
    );

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let directory = match env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(env::var_os("HOME").context("HOME is unavailable; use --state-db")?)
            .join(".local/share"),
    };

    ensure!(
        directory.is_absolute(),
        "application data directory must be absolute; use --state-db"
    );
    Ok(directory.join(crate::constants::APP_NAME).join("state.db"))
}
