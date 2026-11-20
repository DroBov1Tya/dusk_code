//! What a connector's server may name. An address is `https` on a named public host,
//! written in ASCII as it will be reached, so what the user approves is what the core
//! connects to. A program is `npx` or `uvx` with a registry package pinned to one exact
//! version. A secret is a plain header or variable name that cannot change what runs or
//! where a request goes.

use anyhow::{Context, Result, bail, ensure};

use super::RUNNERS;

/// In characters, as for every line the user is shown.
pub const MAX_LINE: usize = 200;
pub const MAX_SECRET_NAME: usize = 64;
const MAX_ARGS: usize = 16;
/// npm's limit on a package name.
const MAX_PACKAGE: usize = 214;

/// Names that only reach this machine or a private network, whatever they resolve to.
const PRIVATE_SUFFIXES: [&str; 11] = [
    "localhost",
    "local",
    "lan",
    "corp",
    "internal",
    "intranet",
    "home",
    "home.arpa",
    "localdomain",
    "test",
    "invalid",
];

/// Variables that change what a runner or the program it starts loads, runs or trusts;
/// a listing may not ask the user to fill them in.
const RUNNER_PREFIXES: [&str; 10] = [
    "NODE_",
    "NPM_",
    "NPX_",
    "COREPACK_",
    "PYTHON",
    "PIP_",
    "UV_",
    "LD_",
    "DYLD_",
    "XDG_",
];
const RUNNER_NAMES: [&str; 7] = [
    "PATH",
    "HOME",
    "SHELL",
    "TMPDIR",
    "USER",
    "LANG",
    "SSL_CERT_FILE",
];
/// Headers the transport sets itself or that carry another site's session.
const TRANSPORT_HEADERS: [&str; 10] = [
    "host",
    "cookie",
    "connection",
    "content-length",
    "content-type",
    "transfer-encoding",
    "accept",
    "origin",
    "mcp-session-id",
    "mcp-protocol-version",
];

/// The host of `url` as written, once `url` passes `check_url`.
pub fn host(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    Some(&rest[..end])
}

/// An `https` address on a named public host on the default port: ASCII as written, no
/// punycode label, no credentials or fragment, and no name that only a private network
/// answers. Where the name resolves is checked again when it is added.
pub fn check_url(url: &str) -> Result<()> {
    ensure!(
        url.starts_with("https://") && url.is_ascii() && !url.contains(' '),
        "a connector's address is not an https address in plain ASCII"
    );
    let parsed = reqwest::Url::parse(url).context("a connector's address is not a URL")?;
    ensure!(
        parsed.username().is_empty() && parsed.password().is_none() && parsed.fragment().is_none(),
        "a connector's address carries credentials or a fragment"
    );
    ensure!(
        parsed.port().is_none(),
        "a connector's address names a port; it is reached on 443"
    );
    let written = host(url).unwrap_or_default();
    let name = parsed.host_str().unwrap_or_default();
    // What is shown is what is reached: no case, dots or escapes the parser rewrites.
    ensure!(
        name == written,
        "a connector's host is not written as it is reached ({name})"
    );
    ensure!(
        name.parse::<std::net::IpAddr>().is_err() && !name.starts_with('['),
        "a connector's address is a name, not an IP address"
    );
    let labels: Vec<&str> = name.trim_end_matches('.').split('.').collect();
    ensure!(
        labels.len() >= 2 && labels.iter().all(|label| !label.is_empty()),
        "a connector's host is not a public name"
    );
    ensure!(
        !labels.iter().any(|label| label.starts_with("xn--")),
        "a connector's host is an internationalized name; only plain ASCII names are taken"
    );
    let lower = name.trim_end_matches('.');
    ensure!(
        !PRIVATE_SUFFIXES
            .iter()
            .any(|end| lower == *end || lower.ends_with(&format!(".{end}"))),
        "a connector's host is on a private network"
    );
    Ok(())
}

/// `npx` or `uvx`, its own flags, then a package pinned to one exact version, then the
/// package's arguments.
pub fn check_command(command: &str, args: &[String]) -> Result<()> {
    ensure!(
        RUNNERS.iter().any(|(name, ..)| *name == command),
        "a connector's command is npx or uvx"
    );
    ensure!(args.len() <= MAX_ARGS, "up to {MAX_ARGS} arguments");
    ensure!(
        args.iter()
            .all(|arg| crate::plugins::store::index::is_exact_line(arg, MAX_LINE)),
        "an argument is not one plain line"
    );
    let flags = match command {
        "npx" => ["-y", "--yes"].as_slice(),
        _ => [].as_slice(),
    };
    let start = args
        .iter()
        .position(|arg| !arg.starts_with('-'))
        .context("a connector names no package")?;
    ensure!(
        args[..start]
            .iter()
            .all(|arg| flags.contains(&arg.as_str())),
        "only -y may come before the package"
    );
    ensure!(
        command != "npx" || start > 0,
        "npx is told -y, so it never waits for a yes nobody sees"
    );
    match command {
        "npx" => npm_package(&args[start]),
        _ => pypi_package(&args[start]),
    }
}

/// `name@1.2.3` or `@scope/name@1.2.3[-pre][+build]`: a registry name and one full
/// semantic version, never a range, a tag, a folder, a tarball or another protocol.
pub fn npm_package(package: &str) -> Result<()> {
    let at = package
        .char_indices()
        .skip(1)
        .find(|(_, c)| *c == '@')
        .map(|(at, _)| at)
        .context("the package is not pinned: name@1.2.3")?;
    let (name, version) = (&package[..at], &package[at + 1..]);
    ensure!(
        is_npm_name(name),
        "the package {name} is not a plain npm name"
    );
    ensure!(
        semver::Version::parse(version).is_ok(),
        "the package is not pinned to one exact version: name@MAJOR.MINOR.PATCH"
    );
    Ok(())
}

/// npm's name grammar: lower case, at most one `/` and only after a `@scope`, no leading
/// `.` or `_`, so nothing reads as a path or a URL.
fn is_npm_name(name: &str) -> bool {
    let part = |text: &str| {
        !text.is_empty()
            && !text.starts_with(['.', '_'])
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-._~".contains(&b))
    };
    let fits = !name.is_empty() && name.len() <= MAX_PACKAGE;
    fits && match name.strip_prefix('@') {
        Some(scoped) => scoped
            .split_once('/')
            .is_some_and(|(scope, rest)| part(scope) && part(rest)),
        None => part(name),
    }
}

/// `name==1.2.3`, the name as PEP 503 normalises it, and a PEP 440 release of at least
/// three parts with an optional pre, post or dev mark: one version, never a range.
pub fn pypi_package(package: &str) -> Result<()> {
    let (name, version) = package
        .split_once("==")
        .context("the package is not pinned: name==1.2.3")?;
    let normalised = !name.is_empty()
        && name.len() <= MAX_PACKAGE
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--");
    ensure!(
        normalised,
        "the package {name} is not a PyPI name as PEP 503 normalises it"
    );
    ensure!(
        is_pep440_release(version),
        "the package is not pinned to one exact version: name==MAJOR.MINOR.PATCH"
    );
    Ok(())
}

fn is_pep440_release(version: &str) -> bool {
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    let mut rest = version;
    // Marks in the order PEP 440 puts them: pre, post, dev.
    let mut marks = Vec::new();
    for mark in [".dev", ".post", "rc", "a", "b"] {
        if let Some(at) = rest.rfind(mark) {
            let number = &rest[at + mark.len()..];
            if digits(number) {
                marks.push(mark);
                rest = &rest[..at];
            }
        }
    }
    let pre = marks
        .iter()
        .filter(|mark| ["rc", "a", "b"].contains(mark))
        .count();
    let release: Vec<&str> = rest.split('.').collect();
    pre <= 1 && release.len() >= 3 && release.iter().all(|part| digits(part))
}

/// A header name for a remote server, a variable name for a local one; neither one the
/// transport, the runner or the program's loader reads.
pub fn secret_name(name: &str, header: bool) -> Result<()> {
    let fits = !name.is_empty() && name.len() <= MAX_SECRET_NAME;
    if header {
        ensure!(
            fits && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            "the secret {name} is not a plain header name"
        );
        let lower = name.to_ascii_lowercase();
        ensure!(
            !TRANSPORT_HEADERS.contains(&lower.as_str()) && !lower.starts_with("proxy-"),
            "the header {name} is the transport's, not a key"
        );
        return Ok(());
    }
    ensure!(
        fits && !name.starts_with(|c: char| c.is_ascii_digit())
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
        "the secret {name} is not a plain variable name"
    );
    let upper = name.to_ascii_uppercase();
    if RUNNER_NAMES.contains(&upper.as_str())
        || RUNNER_PREFIXES
            .iter()
            .any(|prefix| upper.starts_with(prefix))
    {
        bail!("the variable {name} changes what runs; a connector may not ask for it");
    }
    Ok(())
}

/// Where `url`'s host resolves, refused unless every address is public: checked when a
/// connector is added, since a public name may point at this machine.
pub async fn resolves_publicly(url: &str) -> Result<()> {
    let host = host(url).context("a connector's address has no host")?;
    crate::net::public::resolve(host, 443)
        .await
        .with_context(|| format!("the connector's host {host}"))?;
    Ok(())
}
