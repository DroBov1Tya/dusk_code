use anyhow::{Context, Result, bail};
use dusk_core::{
    api::CORE_API_VERSION,
    config::{Config, RecordMode},
    init,
    paths::Paths,
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
        let runtime = AgentRuntime::new(
            &self.state,
            AGENT_ID,
            provider,
            self.model.clone(),
            self.record,
        )?;
        let thread_id = self.start_thread().await?;
        info!("thread {thread_id} created");
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
