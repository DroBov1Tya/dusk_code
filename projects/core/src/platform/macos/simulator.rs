//! Xcode's iOS Simulator for the desktop app's panel, run through `xcrun simctl`. Every
//! command is a program with its arguments, never a shell, and a device is named only by
//! a UDID that parses as one. The agents have no part in it.

use std::{collections::HashMap, process::Stdio, time::Duration};

use anyhow::{Context, Result, bail};
use dusk_protocol::{
    client::{Response, SimulatorRequest},
    system::{SimDevice, Simulator},
};
use parking_lot::Mutex;
use serde::Deserialize;
use tokio::process::Command;

/// A first boot builds the device's data and can take this long.
const BOOT_LIMIT: Duration = Duration::from_secs(180);
const LIMIT: Duration = Duration::from_secs(30);

/// The runtime download, one at a time for the whole process.
struct Download {
    running: bool,
    failed: Option<String>,
}

static DOWNLOAD: Mutex<Download> = Mutex::new(Download {
    running: false,
    failed: None,
});

pub async fn answer(request: SimulatorRequest) -> Result<Response> {
    Ok(match request {
        SimulatorRequest::Look => Response::Simulator(look().await?),
        SimulatorRequest::InstallRuntime => {
            install();
            Response::Done
        }
        SimulatorRequest::Boot { udid } => {
            settle(
                simctl(&["boot", &device(&udid)?], BOOT_LIMIT).await,
                "Booted",
            )?;
            Response::Done
        }
        SimulatorRequest::Shutdown { udid } => {
            settle(
                simctl(&["shutdown", &device(&udid)?], LIMIT).await,
                "Shutdown",
            )?;
            Response::Done
        }
        SimulatorRequest::Screen { udid } => {
            let udid = device(&udid)?;
            let args = ["io", &udid, "screenshot", "--type=jpeg", "-"];
            Response::Picture(simctl(&args, LIMIT).await?)
        }
        SimulatorRequest::OpenApp => {
            run("open", &["-a", "Simulator"], LIMIT).await?;
            Response::Done
        }
    })
}

/// A device already in the state asked for is not a failure.
fn settle(result: Result<Vec<u8>>, state: &str) -> Result<()> {
    match result {
        Err(error) if format!("{error:#}").contains(&format!("current state: {state}")) => Ok(()),
        other => other.map(drop),
    }
}

/// A UDID in the one form simctl prints, so nothing else reaches its arguments.
fn device(udid: &str) -> Result<String> {
    let parsed = uuid::Uuid::try_parse(udid).context("not a simulator device")?;
    Ok(parsed.hyphenated().to_string().to_uppercase())
}

#[derive(Deserialize)]
struct Listing {
    runtimes: Vec<ListedRuntime>,
    devices: HashMap<String, Vec<ListedDevice>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListedRuntime {
    identifier: String,
    name: String,
    #[serde(default)]
    is_available: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListedDevice {
    udid: String,
    name: String,
    state: String,
    #[serde(default)]
    is_available: bool,
}

async fn look() -> Result<Simulator> {
    let (installing, install_failed) = {
        let download = DOWNLOAD.lock();
        (download.running, download.failed.clone())
    };
    let mut simulator = Simulator {
        installing,
        install_failed,
        ..Simulator::default()
    };
    if run("xcrun", &["--find", "simctl"], LIMIT).await.is_err() {
        return Ok(simulator);
    }
    simulator.xcode = true;
    let listing = simctl(&["list", "-j"], LIMIT).await?;
    let listing: Listing = serde_json::from_slice(&listing).context("simctl list")?;
    let (runtimes, devices) = ios(listing);
    simulator.runtimes = runtimes;
    simulator.devices = devices;
    Ok(simulator)
}

/// The available iOS runtimes and their available devices, booted ones first.
fn ios(listing: Listing) -> (Vec<String>, Vec<SimDevice>) {
    let runtimes: HashMap<String, String> = listing
        .runtimes
        .into_iter()
        .filter(|runtime| runtime.is_available && runtime.name.starts_with("iOS "))
        .map(|runtime| (runtime.identifier, runtime.name))
        .collect();
    let mut devices: Vec<SimDevice> = listing
        .devices
        .into_iter()
        .filter_map(|(key, listed)| Some((runtimes.get(&key)?.clone(), listed)))
        .flat_map(|(runtime, listed)| {
            listed
                .into_iter()
                .filter(|device| device.is_available)
                .map(move |device| SimDevice {
                    udid: device.udid,
                    name: device.name,
                    runtime: runtime.clone(),
                    booted: device.state == "Booted",
                })
        })
        .collect();
    devices.sort_by(|a, b| (b.booted, &b.runtime, &a.name).cmp(&(a.booted, &a.runtime, &b.name)));
    let mut names: Vec<String> = runtimes.into_values().collect();
    names.sort();
    (names, devices)
}

/// Downloads the iOS runtime, several gigabytes; `look` reports how it went.
fn install() {
    {
        let mut download = DOWNLOAD.lock();
        if download.running {
            return;
        }
        download.running = true;
        download.failed = None;
    }
    tokio::spawn(async move {
        let mut command = Command::new("xcodebuild");
        command
            .args(["-downloadPlatform", "iOS"])
            .stdin(Stdio::null())
            .kill_on_drop(true);
        let failed = match command.output().await {
            Ok(output) if output.status.success() => None,
            Ok(output) => Some(last_line(&output.stderr)),
            Err(error) => Some(format!("xcodebuild did not start: {error}")),
        };
        let mut download = DOWNLOAD.lock();
        download.running = false;
        download.failed = failed;
    });
}

async fn simctl(args: &[&str], limit: Duration) -> Result<Vec<u8>> {
    let mut all = vec!["simctl"];
    all.extend_from_slice(args);
    run("xcrun", &all, limit).await
}

async fn run(program: &str, args: &[&str], limit: Duration) -> Result<Vec<u8>> {
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null()).kill_on_drop(true);
    let output = tokio::time::timeout(limit, command.output())
        .await
        .with_context(|| format!("{program} took too long"))?
        .with_context(|| format!("{program} did not start"))?;
    if !output.status.success() {
        bail!("{}", last_line(&output.stderr));
    }
    Ok(output.stdout)
}

/// The last line of what a tool wrote to stderr, which is where it says why.
fn last_line(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let line = text.lines().rev().find(|line| !line.trim().is_empty());
    line.unwrap_or("failed").trim().chars().take(300).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_udid_reaches_simctl() {
        assert_eq!(
            device("543c1e74-f9cc-4eb5-81a0-e1470375459b").unwrap(),
            "543C1E74-F9CC-4EB5-81A0-E1470375459B"
        );
        for bad in ["booted", "--help", "all", "", "543C1E74 F9CC"] {
            assert!(device(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn unavailable_runtimes_and_devices_are_left_out() {
        let listing: Listing = serde_json::from_str(
            r#"{
              "runtimes": [
                {"identifier": "r.ios", "name": "iOS 26.2", "isAvailable": true},
                {"identifier": "r.old", "name": "iOS 16.2", "isAvailable": false},
                {"identifier": "r.tv", "name": "tvOS 26.0", "isAvailable": true}
              ],
              "devices": {
                "r.ios": [
                  {"udid": "A", "name": "iPhone 17", "state": "Shutdown", "isAvailable": true},
                  {"udid": "B", "name": "iPad", "state": "Booted", "isAvailable": true},
                  {"udid": "C", "name": "Broken", "state": "Shutdown", "isAvailable": false}
                ],
                "r.old": [{"udid": "D", "name": "Old", "state": "Shutdown", "isAvailable": true}],
                "r.tv": [{"udid": "E", "name": "TV", "state": "Shutdown", "isAvailable": true}]
              }
            }"#,
        )
        .unwrap();
        let (runtimes, devices) = ios(listing);
        assert_eq!(runtimes, ["iOS 26.2"]);
        let udids: Vec<&str> = devices.iter().map(|device| device.udid.as_str()).collect();
        assert_eq!(udids, ["B", "A"]);
    }
}
