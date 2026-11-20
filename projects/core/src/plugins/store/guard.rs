//! What a source without a key may bring, and what it may never take. Nobody signed its
//! list, so the core believes nothing of it but the files it pins: it keeps only skills,
//! packs and agents fetched from a pinned upstream commit with the SHA-256 of every file,
//! text only, and connectors to a public `https` address, which run nothing here; no
//! plugin or local connector (a program) and no package. Its names may not pass for a name
//! that is trusted or already here. What its text then asks of the agent, scripts
//! included, is the user's to accept: the install says so before it happens.

use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Result, ensure};
use dusk_protocol::plugin::Kind;

use super::index::Index;

/// What an Unverified source may bring, as the owner is told.
pub const BRINGS: &str = "It may bring only skills, packs and agents whose text files are \
    pinned to a GitHub commit, and connectors to a public https address; never a plugin or \
    a program. Nobody vouches for them.";

/// Keeps of an unsigned list only what an Unverified source may bring, without its
/// pictures and stars, and says what it left out.
pub fn admit(index: &mut Index) {
    let (mut plugins, mut packaged, mut local) = (0, 0, 0);
    index.plugins.retain_mut(|entry| {
        match entry.kind {
            Kind::Plugin => plugins += 1,
            Kind::Connector
                if entry
                    .server
                    .as_ref()
                    .is_some_and(|server| server.url.is_some()) =>
            {
                entry.cover = None;
                entry.screenshots.clear();
                entry.stars = 0;
                return true;
            }
            Kind::Connector => local += 1,
            _ if entry.upstream.is_none() => packaged += 1,
            Kind::Skill | Kind::Pack | Kind::Agent => {
                // Parsed images from a list nobody signed buy nothing; the name's
                // gradient stands in.
                entry.cover = None;
                entry.screenshots.clear();
                entry.stars = 0;
                return true;
            }
        }
        false
    });
    let mut parts = Vec::new();
    if plugins > 0 {
        parts.push(format!(
            "{plugins} {} ignored: an unverified source cannot bring programs",
            noun(plugins, "plugin")
        ));
    }
    if local > 0 {
        parts.push(format!(
            "{local} {} ignored: an unverified source cannot bring programs",
            noun(local, "local connector")
        ));
    }
    if packaged > 0 {
        parts.push(format!(
            "{packaged} {} ignored: an unverified source brings only files pinned upstream",
            noun(packaged, "package")
        ));
    }
    index.left_out = (!parts.is_empty()).then(|| parts.join("; "));
}

fn noun(count: usize, word: &str) -> String {
    match count {
        1 => word.to_owned(),
        _ => format!("{word}s"),
    }
}

/// Why `name` may not be taken by an Unverified listing: it is, or nearly is, one of
/// `taken`, the names trusted or already here.
pub fn shadow<'a>(name: &str, taken: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let found = taken.into_iter().find(|other| close(name, other))?;
    Some(match found == name {
        true => format!("{name} is taken here; an unverified source cannot take it"),
        false => format!(
            "{name} is too close to {found}, which is taken here; an unverified source \
             cannot take it"
        ),
    })
}

/// The same name, or one edit apart once case and dashes are left out: `debuging` for
/// `debugging`. Names are `a-z0-9-` already, so no look-alike letter gets in; a short
/// name must match exactly, or every three letters would clash.
pub fn close(a: &str, b: &str) -> bool {
    let plain = |text: &str| -> Vec<u8> {
        text.bytes()
            .filter(|byte| *byte != b'-')
            .map(|byte| byte.to_ascii_lowercase())
            .collect()
    };
    let (a, b) = (plain(a), plain(b));
    if a == b {
        return true;
    }
    a.len().min(b.len()) >= 5 && one_edit(&a, &b)
}

/// Whether one insertion, deletion or substitution turns `a` into `b`.
fn one_edit(a: &[u8], b: &[u8]) -> bool {
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    if long.len() - short.len() > 1 {
        return false;
    }
    let start = short.iter().zip(long).take_while(|(x, y)| x == y).count();
    match short.len() == long.len() {
        true => short[start + 1..] == long[start + 1..],
        false => short[start..] == long[start + 1..],
    }
}

/// The files of an Unverified install: text only, so nothing it writes is a program to
/// run; they are written readable and writable by the owner only, never executable.
pub fn check_files(files: &BTreeMap<PathBuf, Vec<u8>>) -> Result<()> {
    for (path, bytes) in files {
        ensure!(
            std::str::from_utf8(bytes).is_ok() && !bytes.contains(&0),
            "{} is not text; an unverified source brings text only",
            path.display()
        );
    }
    Ok(())
}
