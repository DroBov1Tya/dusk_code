use std::{env, path::PathBuf};

use anyhow::{Context, Result, ensure};
use dusk_core::{
    api::CORE_API_VERSION,
    config::Config,
    providers::{Model, ProviderRegistry},
    runtime::AgentRuntime,
    state::{ApprovalMode, State, ThreadInput},
};
use tracing::{error, info};
use uuid::Uuid;

use crate::{
    args::AppArgs,
    chat,
    constants::{AGENT_ID, THREAD_SOURCE},
};

pub struct Application {
    state: State,
    providers: ProviderRegistry,
    provider_name: String,
    model: Model,
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
        let providers = ProviderRegistry::from_config(&Config::load()?)?;
        let provider_name = providers
            .names()
            .next()
            .context("no provider configured")?
            .to_owned();
        let model = providers
            .provider(&provider_name)
            .context("configured provider is missing")?
            .default_model()
            .await?;
        info!(
            "✅ Инициализация: Провайдер {} | Статус: Модель {}",
            provider_name, model.id
        );
        Ok(Self {
            state,
            providers,
            provider_name,
            model,
        })
    }

    pub async fn run(self) -> Result<()> {
        info!(
            "ℹ️ Действие: Запуск чата | Статус: Готово, Core API={}.{}",
            CORE_API_VERSION.major, CORE_API_VERSION.minor
        );
        let result = self.chat().await;
        self.state.close().await;
        result
    }

    async fn chat(&self) -> Result<()> {
        let provider = self
            .providers
            .provider(&self.provider_name)
            .context("configured provider is missing")?;
        let runtime = AgentRuntime::new(&self.state, AGENT_ID, provider, self.model.clone())?;
        let thread_id = self.start_thread().await?;
        info!("✅ Сессия: {} | Статус: Создана", thread_id);
        chat::run(&self.state, &runtime, &thread_id).await
    }

    async fn start_thread(&self) -> Result<String> {
        let thread_id = Uuid::now_v7().to_string();
        self.state
            .create_thread(
                &thread_id,
                ThreadInput {
                    source: THREAD_SOURCE.to_owned(),
                    tokens_used: 0,
                    compaction_count: 0,
                    sandbox_policy: "none".to_owned(),
                    approval_mode: ApprovalMode::Never,
                    git_branch: None,
                    git_commit_sha: None,
                    git_origin_url: None,
                    title: String::new(),
                },
            )
            .await?;
        Ok(thread_id)
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
