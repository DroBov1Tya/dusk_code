//! `/model [provider/model]`: without an argument, every configured provider checked
//! live, with its models; with one, every thread switches to that model.

use anyhow::Result;

use crate::runtime::{
    Runtime,
    model_choice::{Choice, ProviderStatus, check_providers},
};

pub async fn check(runtime: &Runtime) -> Result<Vec<ProviderStatus>> {
    let statuses = check_providers(&runtime.config().current()).await?;
    runtime.learn_model(&statuses);
    Ok(statuses)
}

pub async fn switch(runtime: &Runtime, name: &str) -> Result<Choice> {
    let choice = Choice::pick(&runtime.config().current(), Some(name)).await?;
    runtime.set_model(choice.clone())?;
    Ok(choice)
}
