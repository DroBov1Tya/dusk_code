use std::env;

use anyhow::{Context, Result, bail};
use dusk_core::{
    api::CORE_API_VERSION,
    config::{Config, RecordMode},
    init,
    paths::Paths,
    providers::{Model, ProviderRegistry},
    runtime::SessionRunner,
    session::start_session,
    state::State,
};
use tracing::{error, info};

use crate::{
    args::AppArgs,
    chat,
    constants::{AGENT_ID, THREAD_SOURCE},
};

pub struct Application {
    paths: Paths,
    state: State,
    record: RecordMode,
    providers: ProviderRegistry,
    provider_name: String,
    model: Model,
}

impl Application {
    pub async fn build(args: AppArgs) -> Result<Self> {
        let paths = Paths::from_home()?;
        if init::initialize(&paths)?.config_created {
            bail!(
                "created {}; add a provider and restart",
                paths.config_file().display()
            );
        }
        let path = args.state_db.unwrap_or_else(|| paths.state_db());
        let state = match State::open(path).await {
            Ok(state) => {
                info!("state database opened");
                state
            }
            Err(error) => {
                error!("cannot open state database: {error:#}");
                return Err(error);
            }
        };
        let config = Config::load(&paths)?;
        let record = config.session().record;
        let providers = ProviderRegistry::from_config(&config)?;
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
        info!("provider {provider_name}, model {}", model.id);
        Ok(Self {
            paths,
            state,
            record,
            providers,
            provider_name,
            model,
        })
    }

    pub async fn run(self) -> Result<()> {
        info!(
            "chat started, core API {}.{}",
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
        let session = start_session(&self.state, &self.paths, THREAD_SOURCE).await?;
        info!(
            "session {} started in {}",
            session.thread_id,
            session.workspace.display()
        );
        let cwd = env::current_dir()?.to_string_lossy().into_owned();
        let (runner, input, output) = SessionRunner::new(
            &self.state,
            provider,
            self.model.clone(),
            self.record,
            AGENT_ID,
            &session.thread_id,
            &cwd,
        )?;
        chat::run(runner, input, output).await
    }
}
