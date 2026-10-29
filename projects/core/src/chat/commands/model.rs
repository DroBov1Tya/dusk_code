//! `/model [provider/model]`: without an argument, every configured provider checked
//! live, with its models; with one, every thread switches to that model.

use anyhow::Result;

use crate::runtime::{
    Runtime,
    model_choice::{self, Choice, ProviderStatus},
};

pub async fn check(runtime: &Runtime) -> Result<Vec<ProviderStatus>> {
    model_choice::check_providers(&runtime.config().current()).await
}

pub async fn switch(runtime: &Runtime, name: &str) -> Result<Choice> {
    let choice = model_choice::pick(&runtime.config().current(), Some(name)).await?;
    runtime.set_model(choice.clone())?;
    Ok(choice)
}
