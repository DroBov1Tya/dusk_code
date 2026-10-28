use std::{env, sync::Arc};

use anyhow::{Context, Result, bail};
use dusk_core::{
    api::CORE_VERSION,
    init,
    live_config::LiveConfig,
    paths::Paths,
    providers::{Model, ModelProvider, Provider, ProviderRegistry},
    runtime::{ThreadRunner, ThreadSetup},
    state::State,
    thread_start::start_thread,
    tools::{ToolRegistry, ToolScope},
};
use tracing::{error, info};

use crate::{
    args::AppArgs,
    chat,
    commands::Settings,
    constants::{CONFIG_RELOAD_PERIOD, THREAD_SOURCE},
};

pub struct Application {
    paths: Paths,
    state: Arc<State>,
    config: Arc<LiveConfig>,
    provider: Arc<Provider>,
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
                Arc::new(state)
            }
            Err(error) => {
                error!("cannot open state database: {error:#}");
                return Err(error);
            }
        };
        let config = Arc::new(LiveConfig::load(paths.config_file())?);
        let providers = ProviderRegistry::from_config(&config.current())?;
        let provider_name = providers
            .names()
            .next()
            .context("no provider configured")?
            .to_owned();
        let provider = providers
            .provider(&provider_name)
            .context("configured provider is missing")?;
        let model = provider.default_model().await?;
        info!("provider {provider_name}, model {}", model.id);
        Ok(Self {
            paths,
            state,
            config,
            provider,
            model,
        })
    }

    pub async fn run(self) -> Result<()> {
        info!("chat started, core {CORE_VERSION}");
        let result = self.chat().await;
        self.state.close().await;
        result
    }

    async fn chat(&self) -> Result<()> {
        let thread = start_thread(&self.state, &self.paths, THREAD_SOURCE).await?;
        info!(
            "thread {} started in {}",
            thread.thread_id,
            thread.workspace.display()
        );
        tokio::spawn(Arc::clone(&self.config).watch(CONFIG_RELOAD_PERIOD));
        let tools = ToolRegistry::builtin(&self.paths.skills_dir());
        let settings = Settings {
            config: Arc::clone(&self.config),
            tools: tools.names().collect(),
            skills_dir: self.paths.skills_dir(),
        };
        let (runner, input, output) = ThreadRunner::new(ThreadSetup {
            state: Arc::clone(&self.state),
            provider: Arc::clone(&self.provider) as Arc<dyn ModelProvider>,
            model: self.model.clone(),
            config: Arc::clone(&self.config),
            scope: ToolScope::new(&env::current_dir()?, &thread.workspace)?,
            thread_id: thread.thread_id,
            tools,
        });
        chat::run(runner, input, output, settings).await
    }
}
