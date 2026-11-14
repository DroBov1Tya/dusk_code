//! Apps on a device: open a link in it, start an installed app, and Simulator.app on
//! the Mac.

use anyhow::{Result, ensure};

use super::run::{LIMIT, run, simctl};

/// Opens `url` on the device: a web page or an app's own scheme, never a file.
pub async fn open_url(udid: &str, url: &str) -> Result<()> {
    let parsed = reqwest::Url::parse(url)?;
    ensure!(
        parsed.scheme() != "file",
        "a file is not opened on the device"
    );
    simctl(&["openurl", udid, parsed.as_str()], LIMIT).await?;
    Ok(())
}

/// Starts an installed app by its bundle ID, such as `com.apple.mobilesafari`.
pub async fn launch(udid: &str, bundle_id: &str) -> Result<()> {
    let plain = bundle_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    ensure!(
        plain && !bundle_id.is_empty() && !bundle_id.starts_with('-'),
        "not a bundle ID"
    );
    simctl(&["launch", udid, bundle_id], LIMIT).await?;
    Ok(())
}

pub async fn open_simulator_app() -> Result<()> {
    run("open", &["-a", "Simulator"], LIMIT).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn files_and_flags_never_reach_simctl() {
        let udid = "543C1E74-F9CC-4EB5-81A0-E1470375459B";
        assert!(open_url(udid, "file:///etc/passwd").await.is_err());
        assert!(open_url(udid, "not a url").await.is_err());
        for bad in ["--help", "", "com.app; rm", "a b"] {
            assert!(launch(udid, bad).await.is_err(), "{bad}");
        }
    }
}
