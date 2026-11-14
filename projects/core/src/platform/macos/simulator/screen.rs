//! A device's screen: one picture from simctl, and the screen's size in points, the unit
//! touches are given in, read once per device from the profile of its type.

use std::{collections::HashMap, sync::LazyLock};

use anyhow::{Context, Result, ensure};
use dusk_protocol::system::SimScreen;
use parking_lot::Mutex;

use super::{
    listing,
    run::{LIMIT, run, simctl},
};

static SIZES: LazyLock<Mutex<HashMap<String, (f32, f32)>>> = LazyLock::new(Mutex::default);

/// The screen at full size; `udid` has been checked by `run::device`.
pub async fn take(udid: &str) -> Result<SimScreen> {
    let args = ["io", udid, "screenshot", "--type=jpeg", "-"];
    let jpeg = simctl(&args, LIMIT).await?;
    let (width, height) = points(udid).await?;
    Ok(SimScreen {
        jpeg,
        width,
        height,
        seq: 0,
    })
}

/// The screen's width and height in points, upright.
pub async fn points(udid: &str) -> Result<(f32, f32)> {
    if let Some(size) = SIZES.lock().get(udid) {
        return Ok(*size);
    }
    let bundle = listing::type_bundle(udid).await?;
    let profile = format!("{bundle}/Contents/Resources/profile.plist");
    let read = async |key: &str| -> Result<f32> {
        let raw = run(
            "plutil",
            &["-extract", key, "raw", "-o", "-", &profile],
            LIMIT,
        )
        .await?;
        String::from_utf8_lossy(&raw)
            .trim()
            .parse()
            .with_context(|| format!("the device's profile has no {key}"))
    };
    let scale = read("mainScreenScale").await?;
    ensure!((1.0..=4.0).contains(&scale), "an odd screen scale: {scale}");
    let size = (
        read("mainScreenWidth").await? / scale,
        read("mainScreenHeight").await? / scale,
    );
    SIZES.lock().insert(udid.to_owned(), size);
    Ok(size)
}
