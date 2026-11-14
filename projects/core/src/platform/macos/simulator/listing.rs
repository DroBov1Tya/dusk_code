//! What is installed: Xcode, the iOS runtimes and their devices, read from
//! `simctl list`.

use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
};

use anyhow::{Context, Result};
use dusk_protocol::system::{SimDevice, Simulator};
use serde::Deserialize;

use super::{
    download,
    run::{LIMIT, run, simctl},
};

#[derive(Deserialize)]
struct Listing {
    runtimes: Vec<ListedRuntime>,
    devices: HashMap<String, Vec<ListedDevice>>,
    #[serde(default)]
    devicetypes: Vec<ListedType>,
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
    #[serde(default)]
    device_type_identifier: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListedType {
    identifier: String,
    bundle_path: String,
}

pub async fn look() -> Result<Simulator> {
    let (installing, install_failed) = download::state();
    let mut simulator = Simulator {
        installing,
        install_failed,
        ..Simulator::default()
    };
    if run("xcrun", &["--find", "simctl"], LIMIT).await.is_err() {
        return Ok(simulator);
    }
    simulator.xcode = true;
    let (runtimes, devices) = ios(listing().await?);
    simulator.runtimes = runtimes;
    simulator.devices = devices;
    simulator.input = run("axe", &["--version"], LIMIT).await.is_ok();
    AXE.store(simulator.input, Ordering::Relaxed);
    Ok(simulator)
}

/// Whether AXe was found when the devices were last looked at.
static AXE: AtomicBool = AtomicBool::new(false);

pub fn axe_found() -> bool {
    AXE.load(Ordering::Relaxed)
}

/// The first booted iOS device, which the agent's tool acts on.
pub async fn booted() -> Result<SimDevice> {
    let (_, devices) = ios(listing().await?);
    devices
        .into_iter()
        .find(|device| device.booted)
        .context("no iOS device is booted: boot one in the iOS Simulator panel")
}

/// Where Xcode keeps the profile of a device's type, which knows its screen.
pub async fn type_bundle(udid: &str) -> Result<String> {
    let listing = listing().await?;
    let kind = listing
        .devices
        .values()
        .flatten()
        .find(|device| device.udid == udid)
        .map(|device| device.device_type_identifier.clone())
        .context("the device is gone")?;
    listing
        .devicetypes
        .into_iter()
        .find(|listed| listed.identifier == kind)
        .map(|listed| listed.bundle_path)
        .context("the device's type is unknown")
}

async fn listing() -> Result<Listing> {
    let listing = simctl(&["list", "-j"], LIMIT).await?;
    serde_json::from_slice(&listing).context("simctl list")
}

/// The available iOS runtimes and their available devices: booted ones first, then
/// newer runtimes, each runtime's devices in the order Xcode lists them.
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
    devices.sort_by(|a, b| (b.booted, &b.runtime).cmp(&(a.booted, &a.runtime)));
    let mut names: Vec<String> = runtimes.into_values().collect();
    names.sort();
    (names, devices)
}

#[cfg(test)]
mod tests {
    use super::*;

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
                  {"udid": "C", "name": "Broken", "state": "Shutdown", "isAvailable": false},
                  {"udid": "F", "name": "iPhone Air", "state": "Shutdown", "isAvailable": true}
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
        assert_eq!(udids, ["B", "A", "F"]);
    }
}
