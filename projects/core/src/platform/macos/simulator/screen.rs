//! A device's screen: the picture simctl takes, and its size in points, the unit
//! touches are given in. The points come from the scale in the profile of the device's
//! type, read once per device.

use std::{collections::HashMap, sync::LazyLock};

use anyhow::{Context, Result};
use dusk_protocol::system::SimScreen;
use parking_lot::Mutex;

use super::{
    listing,
    run::{LIMIT, run, simctl},
};

static SCALES: LazyLock<Mutex<HashMap<String, f32>>> = LazyLock::new(Mutex::default);

/// `udid` has been checked by `run::device`.
pub async fn take(udid: &str) -> Result<SimScreen> {
    let args = ["io", udid, "screenshot", "--type=jpeg", "-"];
    let jpeg = simctl(&args, LIMIT).await?;
    let (width, height) = pixels(&jpeg)?;
    let scale = scale(udid).await?;
    Ok(SimScreen {
        jpeg,
        width: width as f32 / scale,
        height: height as f32 / scale,
    })
}

fn pixels(jpeg: &[u8]) -> Result<(u32, u32)> {
    let reader =
        image::ImageReader::with_format(std::io::Cursor::new(jpeg), image::ImageFormat::Jpeg);
    reader.into_dimensions().context("simctl gave no picture")
}

async fn scale(udid: &str) -> Result<f32> {
    if let Some(scale) = SCALES.lock().get(udid) {
        return Ok(*scale);
    }
    let bundle = listing::type_bundle(udid).await?;
    let profile = format!("{bundle}/Contents/Resources/profile.plist");
    let args = ["-extract", "mainScreenScale", "raw", "-o", "-", &profile];
    let raw = run("plutil", &args, LIMIT).await?;
    let scale: f32 = String::from_utf8_lossy(&raw)
        .trim()
        .parse()
        .context("the device's profile has no screen scale")?;
    anyhow::ensure!((1.0..=4.0).contains(&scale), "an odd screen scale: {scale}");
    SCALES.lock().insert(udid.to_owned(), scale);
    Ok(scale)
}
