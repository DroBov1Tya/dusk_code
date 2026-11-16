//! Install the pinned, notarized Launchpad without changing host settings.

use std::{
    ffi::CString,
    fs::{self, DirBuilder},
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use uuid::Uuid;

use super::{APP_NAME, run, user_app};

const RELEASE: &str = "https://github.com/Lakr233/vphone-cli/releases/download/2.17.1/vphone-launchpad-2.17.1-notarized.zip";
const SHA256: &str = "c501d33e1e4d94a9a45595567045c255f928388c360916fc08820a06e2348dfc";
const LIMIT: Duration = Duration::from_secs(90);
const DOWNLOAD: Duration = Duration::from_secs(180);

async fn supported() -> bool {
    let silicon = run("/usr/sbin/sysctl", &["-n", "hw.optional.arm64"], LIMIT, 64).await;
    if !silicon.is_ok_and(|value| value == b"1\n") {
        return false;
    }
    run("/usr/bin/sw_vers", &["-productVersion"], LIMIT, 64)
        .await
        .ok()
        .and_then(|value| {
            std::str::from_utf8(&value)
                .ok()?
                .split('.')
                .next()?
                .parse::<u32>()
                .ok()
        })
        .is_some_and(|major| major >= 15)
}

pub(super) async fn install() -> Result<()> {
    ensure!(
        supported().await,
        "vphone Launchpad requires macOS 15 or newer on an Apple silicon Mac"
    );
    let target = user_app().context("Could not find your Applications folder")?;
    let applications = target.parent().context("No Applications folder")?;
    match fs::create_dir(applications) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error).context("Could not create your Applications folder"),
    }
    ensure!(
        applications.symlink_metadata()?.is_dir(),
        "Your Applications folder must be a directory"
    );
    ensure!(
        matches!(target.symlink_metadata(), Err(error) if error.kind() == std::io::ErrorKind::NotFound),
        "Launchpad already exists in your Applications folder. Open it there or move it aside and retry."
    );
    let temp = Staging::new(applications)?;
    let archive = temp.0.join("launchpad.zip");
    let app = temp.0.join(APP_NAME);
    let zip = utf8(&archive)?;
    let unpack = utf8(&temp.0)?;
    let app_path = utf8(&app)?;
    run(
        "/usr/bin/curl",
        &[
            "--disable",
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-filesize",
            "20000000",
            "--output",
            zip,
            RELEASE,
        ],
        DOWNLOAD,
        4096,
    )
    .await
    .context("Could not download vphone Launchpad")?;
    let hash = run("/usr/bin/shasum", &["-a", "256", zip], LIMIT, 4096).await?;
    ensure!(
        hash.split(|byte| byte.is_ascii_whitespace()).next() == Some(SHA256.as_bytes()),
        "The vphone Launchpad download failed its checksum"
    );
    run("/usr/bin/ditto", &["-x", "-k", zip, unpack], LIMIT, 4096)
        .await
        .context("Could not unpack vphone Launchpad")?;
    ensure!(
        app.symlink_metadata()?.is_dir() && app.join(super::CLI_SUBPATH).is_file(),
        "The download did not contain the vphone Launchpad app and CLI"
    );
    run(
        "/usr/sbin/spctl",
        &["--assess", "--type", "execute", app_path],
        LIMIT,
        4096,
    )
    .await
    .context("Gatekeeper rejected vphone Launchpad")?;
    place(&app, &target).context("Could not place vphone Launchpad in your Applications folder")?;
    run("/usr/bin/open", &[utf8(&target)?], LIMIT, 4096)
        .await
        .context(
            "Launchpad is installed. Open it from your Applications folder to finish Host Setup.",
        )?;
    // The new plugin sandbox resolves the socket directory on its next scan.
    let cli = target.join(super::CLI_SUBPATH);
    run(utf8(&cli)?, &["status"], LIMIT, 65536)
        .await
        .context("Launchpad is installed. Open it from Applications and refresh this panel once Host Setup is available.")?;
    Ok(())
}

fn utf8(path: &Path) -> Result<&str> {
    path.to_str()
        .context("The Applications folder path is not UTF-8")
}

/// Publish atomically, including when another installer wins the race.
fn place(source: &Path, target: &Path) -> std::io::Result<()> {
    let source = CString::new(source.as_os_str().as_encoded_bytes())?;
    let target = CString::new(target.as_os_str().as_encoded_bytes())?;
    // Both paths are NUL-terminated and remain live until the call returns.
    let result =
        unsafe { nix::libc::renamex_np(source.as_ptr(), target.as_ptr(), nix::libc::RENAME_EXCL) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

struct Staging(PathBuf);

impl Staging {
    fn new(applications: &Path) -> Result<Self> {
        let path = applications.join(format!(".dusk-vphone-{}", Uuid::new_v4()));
        DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .context("Could not create a staging directory")?;
        Ok(Self(path))
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("Launchpad staging cleanup failed: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staging_keeps_an_existing_destination_intact() -> Result<()> {
        let staging = Staging::new(&std::env::temp_dir())?;
        let source = staging.0.join("new");
        let target = staging.0.join("installed");
        fs::create_dir(&source)?;
        fs::create_dir(&target)?;
        fs::write(target.join("marker"), "existing application")?;
        assert!(place(&source, &target).is_err());
        assert!(source.is_dir());
        assert_eq!(fs::read(target.join("marker"))?, b"existing application");
        let vacant = staging.0.join("vacant");
        place(&source, &vacant)?;
        assert!(vacant.is_dir());
        assert!(!source.exists());
        Ok(())
    }
}
