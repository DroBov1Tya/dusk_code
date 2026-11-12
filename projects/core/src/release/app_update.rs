//! Updating the desktop app in place. The app is `Duskcode.app`; a release carries it as a
//! disk image. The image is downloaded by this process, so it never gets the quarantine
//! mark a browser puts on files, and macOS lets the new copy start without asking. The
//! image is checked against the digest GitHub keeps, its app is copied beside the running
//! one and checked again, and then the two are swapped by renames: the running copy goes on
//! until the user restarts it, and any failure leaves it as it was.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, ensure};
use tokio::sync::watch;

use super::update::{self, InstallProgress, InstallStage, Release};

const APP: &str = "Duskcode.app";

/// The app bundle this process runs from; `None` for a bare binary.
pub fn running_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().and_then(fs::canonicalize).ok()?;
    bundle_of(&exe)
}

/// `/Applications/Duskcode.app` for `.../Duskcode.app/Contents/MacOS/duskcode-gui`.
fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let shaped = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.file_name()?.to_str()?.ends_with(".app");
    shaped.then(|| bundle.to_path_buf())
}

/// The name of the image a release of `tag` carries.
fn image_name(tag: &str) -> String {
    format!("Duskcode-{}-macos-arm64.dmg", tag.trim_start_matches('v'))
}

pub async fn install(
    tag: &str,
    bundle: PathBuf,
    progress: watch::Sender<InstallProgress>,
) -> Result<PathBuf> {
    ensure!(
        cfg!(target_os = "macos"),
        "the app is updated in place on macOS only"
    );
    let name = image_name(tag);
    let client = update::client()?;
    let release: Release = update::release(&client, tag).await?;
    let expected = release
        .digest_of(&name)
        .with_context(|| format!("{tag} has no {name} yet; try again in a few minutes"))?;
    let image = update::fetch(&client, &update::asset_url(tag, &name), &progress).await?;
    progress.send_modify(|value| value.stage = InstallStage::Verifying);
    tokio::task::spawn_blocking(move || {
        ensure!(
            update::sha256_hex(&image) == expected,
            "the download does not match its checksum; nothing was changed"
        );
        progress.send_modify(|value| value.stage = InstallStage::Installing);
        let folder = bundle.parent().context("the app has no folder")?;
        let work = folder.join(format!(".duskcode-update-{}", uuid::Uuid::now_v7()));
        let swapped = replace(&image, &work, &bundle);
        // The work folder holds only what this update wrote.
        let _ = fs::remove_dir_all(&work);
        swapped?;
        progress.send_modify(|value| value.stage = InstallStage::Complete);
        Ok(bundle)
    })
    .await
    .context("the update installer stopped")?
}

/// Copies the app out of the image into `work`, checks it, and swaps it with `bundle`.
fn replace(image: &[u8], work: &Path, bundle: &Path) -> Result<()> {
    fs::create_dir(work).context("cannot write next to the app")?;
    let file = work.join("app.dmg");
    fs::write(&file, image).context("cannot write the download")?;
    let mount = work.join("image");
    fs::create_dir(&mount)?;
    run(
        Command::new("hdiutil")
            .args([
                "attach",
                "-nobrowse",
                "-readonly",
                "-noverify",
                "-mountpoint",
            ])
            .arg(&mount)
            .arg(&file),
        "open the disk image",
    )?;
    let new = work.join(APP);
    let copied = run(
        Command::new("ditto").arg(mount.join(APP)).arg(&new),
        "copy the app out of the image",
    );
    // The image goes whether or not the copy worked.
    let detached = run(
        Command::new("hdiutil")
            .arg("detach")
            .arg("-force")
            .arg(&mount),
        "close the disk image",
    );
    copied?;
    detached?;
    // A copy made here is not quarantined; this only clears a mark that came with the image.
    let _ = Command::new("xattr")
        .args(["-dr", "com.apple.quarantine"])
        .arg(&new)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    run(
        Command::new("codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&new),
        "verify the new app's signature",
    )?;
    // Signed by a certificate, the running app takes only a copy signed by the same one:
    // macOS keeps the user's permissions for it, and a copy signed by anyone else is not
    // installed. An ad hoc app names only its own hash, which no new build can meet.
    if let Some(requirement) = certificate_requirement(bundle) {
        run(
            Command::new("codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(format!("-R={requirement}"))
                .arg(&new),
            "check that the new app is signed by the same certificate",
        )?;
    }
    let old = work.join("old.app");
    fs::rename(bundle, &old).context("cannot move the running app aside")?;
    if let Err(error) = fs::rename(&new, bundle) {
        // Put the running app back before saying no.
        let _ = fs::rename(&old, bundle);
        return Err(error).context("cannot put the new app in place");
    }
    Ok(())
}

/// The app's designated requirement when it names a certificate, `None` for ad hoc.
fn certificate_requirement(bundle: &Path) -> Option<String> {
    let output = Command::new("codesign")
        .args(["--display", "--requirements", "-"])
        .arg(bundle)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    designated(&text)
}

fn designated(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix("designated => "))
        .filter(|requirement| requirement.contains("certificate"))
        .map(str::to_owned)
}

fn run(command: &mut Command, what: &str) -> Result<()> {
    let output = command
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("cannot {what}"))?;
    ensure!(
        output.status.success(),
        "cannot {what}: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bundle_is_found_only_from_its_own_binary() {
        let exe = Path::new("/Applications/Duskcode.app/Contents/MacOS/duskcode-gui");
        assert_eq!(
            bundle_of(exe),
            Some(PathBuf::from("/Applications/Duskcode.app"))
        );
        assert_eq!(bundle_of(Path::new("/usr/local/bin/duskcode")), None);
        assert_eq!(
            bundle_of(Path::new("/x/Other/Contents/MacOS/duskcode")),
            None
        );
    }

    #[test]
    fn only_a_certificate_requirement_binds_the_next_version() {
        let signed = "Executable=/A.app/Contents/MacOS/a\ndesignated => identifier \"dev.duskcode.gui\" and certificate leaf = H\"ab12\"\n";
        assert_eq!(
            designated(signed).as_deref(),
            Some("identifier \"dev.duskcode.gui\" and certificate leaf = H\"ab12\"")
        );
        let ad_hoc = "designated => cdhash H\"ff00\"\n";
        assert_eq!(designated(ad_hoc), None);
    }

    #[test]
    fn the_image_is_named_by_the_version() {
        assert_eq!(image_name("v0.72.1"), "Duskcode-0.72.1-macos-arm64.dmg");
    }
}
