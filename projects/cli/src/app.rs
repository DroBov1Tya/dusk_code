use anyhow::Result;
use dusk_core::api::CORE_API_VERSION;
use tracing::info;

use crate::args::AppArgs;

pub struct Application;

impl Application {
    pub async fn build(_args: AppArgs) -> Result<Self> {
        info!("✅ Инициализация: Каркас harness | Статус: Успешно");
        Ok(Self)
    }

    pub async fn run(self) -> Result<()> {
        info!(
            "ℹ️ Действие: Проверка каркаса | Статус: Готово, Core API={}.{}",
            CORE_API_VERSION.major, CORE_API_VERSION.minor
        );
        Ok(())
    }
}
