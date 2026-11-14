//! The core's requests as typed calls, so a view says what it wants and not which
//! response variant carries it. Every error is the core's own text.

use crate::link::request;
use dusk_protocol::client::{Response, Secret, SettingsRequest, Switch};

use super::*;

pub async fn set(call: SettingsRequest) -> Res<()> {
    done(settings(call)).await
}

pub async fn switch(switch: Switch, on: bool) -> Res<()> {
    set(SettingsRequest::Switch { switch, on }).await
}

pub async fn next_mode() -> Res<String> {
    ask!(settings(SettingsRequest::NextMode), Response::Text(t) => t)
}

pub async fn number(call: SettingsRequest) -> Res<u64> {
    ask!(settings(call), Response::Number(n) => n)
}

pub async fn connect_provider(call: SettingsRequest) -> Res<String> {
    ask!(settings(call), Response::Text(t) => t)
}

pub async fn set_secret(name: String, value: String) -> Res<()> {
    let value = Secret(value);
    set(SettingsRequest::SetSecret { name, value }).await
}
