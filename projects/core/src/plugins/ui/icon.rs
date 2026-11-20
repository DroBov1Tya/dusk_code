//! A part's icon: an SVG file in the plugin's folder, small and with nothing it runs or
//! loads. A client draws it as a picture, so anything that could reach out, run a script
//! or pull another file is refused rather than cleaned.

use std::{
    path::{Component, Path},
    sync::LazyLock,
};

use anyhow::{Context, Result, bail, ensure};
use regex::Regex;

pub const MAX_ICON_BYTES: u64 = 16 * 1024;

/// Elements that run, embed or animate something.
static ELEMENTS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"<\s*(?:[a-z0-9_-]+:)?(script|foreignobject|image|iframe|embed|object|audio|video|animate[a-z]*|set|feimage)\b",
    )
    .expect("a valid pattern")
});
/// A link's first character, which must be `#`, a place in the same file.
static LINKS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"href\s*=\s*["']?\s*(.?)"#).expect("a valid pattern"));
static URLS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"url\s*\(\s*["']?\s*(.?)"#).expect("a valid pattern"));
static HANDLERS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\s/]on[a-z]+\s*=").expect("a valid pattern"));

/// The icon at `relative` in `folder`, checked, as text.
pub fn load(folder: &Path, relative: &str) -> Result<String> {
    let path = Path::new(relative);
    ensure!(
        path.components()
            .all(|part| matches!(part, Component::Normal(_))),
        "{relative} is not a path inside the plugin's folder"
    );
    let folder = folder
        .canonicalize()
        .context("cannot resolve the plugin's folder")?;
    let file = folder
        .join(path)
        .canonicalize()
        .with_context(|| format!("cannot find {relative}"))?;
    ensure!(
        file.starts_with(&folder),
        "{relative} leads out of the plugin's folder"
    );
    let bytes = crate::init::files::read_regular(&file, MAX_ICON_BYTES)
        .with_context(|| format!("cannot read {relative}"))?;
    let text = String::from_utf8(bytes).context("the icon is not UTF-8 text")?;
    check(&text)?;
    Ok(text)
}

/// Refuses an SVG that could run or load anything.
pub fn check(text: &str) -> Result<()> {
    let lower = text.to_ascii_lowercase();
    ensure!(lower.contains("<svg"), "the icon is not an SVG");
    if let Some(found) = ELEMENTS.captures(&lower) {
        bail!("the icon has a <{}> element", &found[1]);
    }
    for (pattern, what) in [(&*LINKS, "a link"), (&*URLS, "a url()")] {
        for found in pattern.captures_iter(&lower) {
            ensure!(&found[1] == "#", "the icon has {what} out of the file");
        }
    }
    ensure!(!HANDLERS.is_match(&lower), "the icon has an event handler");
    for word in [
        "@import",
        "javascript:",
        "<!doctype",
        "<!entity",
        "<?xml-stylesheet",
    ] {
        ensure!(!lower.contains(word), "the icon has {word}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">
        <defs><linearGradient id="g"/></defs>
        <path d="M0 0h16v16z" fill="url(#g)"/><a href="#g"/></svg>"##;

    #[test]
    fn a_plain_icon_passes() {
        check(PLAIN).unwrap();
    }

    #[test]
    fn an_icon_that_runs_or_loads_is_refused() {
        for bad in [
            "<svg><script>alert(1)</script></svg>",
            "<svg><SCRIPT>x</SCRIPT></svg>",
            "<svg><svg:script>x</svg:script></svg>",
            "<svg><foreignObject><div/></foreignObject></svg>",
            r#"<svg><image href="https://x.test/a.png"/></svg>"#,
            r#"<svg><a href="https://x.test">x</a></svg>"#,
            r#"<svg><a xlink:href="javascript:alert(1)">x</a></svg>"#,
            r#"<svg><a href = 'data:text/html,x'>x</a></svg>"#,
            r#"<svg><a href="&#106;avascript:x">x</a></svg>"#,
            r#"<svg><rect fill="url(https://x.test/a)"/></svg>"#,
            r#"<svg><style>@import "https://x.test/a.css";</style></svg>"#,
            r#"<svg onload="alert(1)"/>"#,
            r#"<svg><rect/onclick="x"/></svg>"#,
            r#"<!DOCTYPE svg [<!ENTITY x SYSTEM "file:///etc/passwd">]><svg>&x;</svg>"#,
            r#"<svg><animate attributeName="href" to="javascript:x"/></svg>"#,
            "<html>no svg</html>",
        ] {
            assert!(check(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_icon_outside_the_folder_or_too_large_is_refused() {
        let root = crate::testing::temp_dir("plugin-icon");
        let folder = root.join("plugin");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(root.join("outside.svg"), PLAIN).unwrap();
        std::fs::write(folder.join("icon.svg"), PLAIN).unwrap();
        std::fs::write(folder.join("big.svg"), "<svg>".repeat(5000)).unwrap();
        assert!(load(&folder, "icon.svg").is_ok());
        assert!(load(&folder, "../outside.svg").is_err());
        assert!(load(&folder, "/etc/hosts").is_err());
        assert!(load(&folder, "big.svg").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("outside.svg"), folder.join("link.svg")).unwrap();
            assert!(load(&folder, "link.svg").is_err());
        }
    }
}
