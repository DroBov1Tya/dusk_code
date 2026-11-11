//! Money left on a provider account, where a plain API key may read it. The request goes
//! only to the provider's own host, with its own key; admin keys are never asked for.

use std::time::Duration;

use anyhow::{Context, Result, anyhow, ensure};
use reqwest::{StatusCode, Url};
use serde_json::Value;

use crate::config::{BalanceApi, ProviderConfig};

use super::transport::http;

pub use dusk_protocol::model::Balance;

const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_BODY_BYTES: usize = 64 * 1024;

/// The configured endpoint, or the one a known host has.
pub fn api(settings: &ProviderConfig) -> Option<BalanceApi> {
    settings.balance.or_else(|| {
        let host = Url::parse(&settings.base_url).ok()?.host_str()?.to_owned();
        match host.as_str() {
            "api.deepseek.com" => Some(BalanceApi::Deepseek),
            "api.moonshot.ai" | "api.moonshot.cn" => Some(BalanceApi::Kimi),
            "openrouter.ai" => Some(BalanceApi::Openrouter),
            _ => None,
        }
    })
}

/// None when the provider has no balance endpoint.
pub async fn fetch(settings: &ProviderConfig) -> Option<Result<Balance, String>> {
    let api = api(settings)?;
    let read = tokio::time::timeout(TIMEOUT, read(settings, api)).await;
    Some(match read {
        Ok(Ok(balance)) => Ok(balance),
        Ok(Err(error)) => Err(format!("{error:#}")),
        Err(_) => Err("no answer in time".into()),
    })
}

async fn read(settings: &ProviderConfig, api: BalanceApi) -> Result<Balance> {
    let base = http::base_url(&settings.base_url, settings.allow_http)?;
    let path = match api {
        BalanceApi::Deepseek => "/user/balance",
        BalanceApi::Kimi => "/v1/users/me/balance",
        BalanceApi::Openrouter => "/api/v1/key",
        BalanceApi::Litellm => "/key/info",
    };
    // Same scheme, host and port as the base URL, so the key goes nowhere else.
    let url = base.join(path).context("invalid balance URL")?;
    let mut request = http::client()?.get(url);
    if !settings.api_key.0.is_empty() {
        request = request.bearer_auth(&settings.api_key.0);
    }
    let response = request
        .send()
        .await
        .map_err(|_| anyhow!("cannot reach the provider"))?;
    let status = response.status();
    ensure!(
        status != StatusCode::UNAUTHORIZED && status != StatusCode::FORBIDDEN,
        "the key may not read the balance"
    );
    ensure!(status.is_success(), "the provider answered {status}");
    let body = response
        .bytes()
        .await
        .map_err(|_| anyhow!("cannot read the provider's answer"))?;
    ensure!(body.len() <= MAX_BODY_BYTES, "the answer is too large");
    let value: Value =
        serde_json::from_slice(&body).map_err(|_| anyhow!("the answer is not JSON"))?;
    parse(api, &value, base.host_str().unwrap_or_default())
}

fn parse(api: BalanceApi, value: &Value, host: &str) -> Result<Balance> {
    let unknown = || anyhow!("the answer has no balance in the expected place");
    match api {
        BalanceApi::Deepseek => {
            let first = value["balance_infos"].get(0).ok_or_else(unknown)?;
            Ok(Balance {
                left: Some(decimal(&first["total_balance"]).ok_or_else(unknown)?),
                currency: currency(&first["currency"]).ok_or_else(unknown)?,
            })
        }
        BalanceApi::Kimi => Ok(Balance {
            left: Some(decimal(&value["data"]["available_balance"]).ok_or_else(unknown)?),
            // The Chinese platform bills in yuan, the international one in dollars.
            currency: if host.ends_with(".cn") { "CNY" } else { "USD" }.into(),
        }),
        BalanceApi::Openrouter => {
            let data = &value["data"];
            ensure!(data.is_object(), unknown());
            Ok(Balance {
                left: optional_decimal(&data["limit_remaining"]).ok_or_else(unknown)?,
                currency: "USD".into(),
            })
        }
        BalanceApi::Litellm => {
            let info = &value["info"];
            let spend = info["spend"].as_f64().unwrap_or(0.0);
            let left = match &info["max_budget"] {
                Value::Null => None,
                budget => {
                    let budget = budget.as_f64().ok_or_else(unknown)?;
                    Some(format!("{:.2}", (budget - spend).max(0.0)))
                }
            };
            ensure!(info.is_object(), unknown());
            Ok(Balance {
                left,
                currency: "USD".into(),
            })
        }
    }
}

/// A number or a numeric string; anything else from the server is refused, since the
/// text is shown as it is.
fn decimal(value: &Value) -> Option<String> {
    let text = match value {
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.trim().to_owned(),
        _ => return None,
    };
    let numeric = !text.is_empty()
        && text.len() <= 32
        && text.parse::<f64>().is_ok_and(f64::is_finite)
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == '-');
    numeric.then_some(text)
}

/// Null means no limit.
fn optional_decimal(value: &Value) -> Option<Option<String>> {
    match value {
        Value::Null => Some(None),
        other => decimal(other).map(Some),
    }
}

fn currency(value: &Value) -> Option<String> {
    let text = value.as_str()?;
    (!text.is_empty() && text.len() <= 5 && text.chars().all(|c| c.is_ascii_uppercase()))
        .then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn reads_each_provider_answer_and_refuses_odd_text() {
        let deepseek = json!({"is_available": true, "balance_infos": [
            {"currency": "CNY", "total_balance": "110.00", "granted_balance": "10.00"}]});
        let balance = parse(BalanceApi::Deepseek, &deepseek, "api.deepseek.com").unwrap();
        assert_eq!(balance.text(), "110.00 CNY");

        let kimi = json!({"code": 0, "data": {"available_balance": 49.5}});
        let balance = parse(BalanceApi::Kimi, &kimi, "api.moonshot.ai").unwrap();
        assert_eq!(balance.text(), "49.5 USD");

        let open = json!({"data": {"limit": null, "limit_remaining": null, "usage": 2.5}});
        let balance = parse(BalanceApi::Openrouter, &open, "openrouter.ai").unwrap();
        assert_eq!(balance.text(), "no limit");

        let litellm = json!({"info": {"spend": 2.5, "max_budget": 10.0}});
        let balance = parse(BalanceApi::Litellm, &litellm, "proxy").unwrap();
        assert_eq!(balance.text(), "7.50 USD");

        let injected = json!({"balance_infos": [
            {"currency": "USD", "total_balance": "1\u{1b}[31m"}]});
        assert!(parse(BalanceApi::Deepseek, &injected, "api.deepseek.com").is_err());
        let currency = json!({"balance_infos": [{"currency": "<b>", "total_balance": "1"}]});
        assert!(parse(BalanceApi::Deepseek, &currency, "api.deepseek.com").is_err());
    }

    #[test]
    fn only_known_hosts_get_a_balance_without_config() {
        let mut settings: ProviderConfig =
            serde_json::from_value(json!({"base_url": "https://api.deepseek.com"})).unwrap();
        assert_eq!(api(&settings), Some(BalanceApi::Deepseek));
        settings.base_url = "https://api.deepseek.com.evil.example/v1".into();
        assert_eq!(api(&settings), None);
    }
}
