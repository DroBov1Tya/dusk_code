//! The web client as trunk built it, embedded in the binary. A checkout that has not built
//! it serves a page that says how.

use base64::{Engine, engine::general_purpose::STANDARD};
use rust_embed::RustEmbed;
use sha2::{Digest, Sha256};

#[derive(RustEmbed)]
#[folder = "../web-ui/dist"]
struct Bundle;

const MISSING: &str = "<!doctype html><meta charset=utf-8><title>duskcode</title>\
<p>The web client is not built in this binary. Run <code>scripts/build-web.sh</code> and \
build again.";

/// A file of the bundle and its content type.
pub struct Asset {
    pub body: Vec<u8>,
    pub content_type: &'static str,
}

/// The file at `path` of the bundle; `/` is the page.
pub fn get(path: &str) -> Option<Asset> {
    let path = path.trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match Bundle::get(path) {
        Some(file) => Some(Asset {
            body: file.data.into_owned(),
            content_type: content_type(path),
        }),
        None if path == "index.html" => Some(Asset {
            body: MISSING.as_bytes().to_vec(),
            content_type: "text/html; charset=utf-8",
        }),
        None => None,
    }
}

/// The CSP sources allowing exactly the scripts the page inlines, which is the loader
/// trunk writes; any other inline script stays blocked.
pub fn inline_script_sources() -> String {
    let Some(page) = get("index.html") else {
        return String::new();
    };
    let page = String::from_utf8_lossy(&page.body);
    let mut sources = Vec::new();
    let mut rest = page.as_ref();
    while let Some(open) = rest.find("<script") {
        rest = &rest[open..];
        let Some(tag_end) = rest.find('>') else { break };
        let Some(close) = rest.find("</script>") else {
            break;
        };
        let (tag, body) = (&rest[..tag_end], &rest[tag_end + 1..close]);
        if !tag.contains("src=") && !body.trim().is_empty() {
            let digest = Sha256::digest(body.as_bytes());
            sources.push(format!("'sha256-{}'", STANDARD.encode(digest)));
        }
        rest = &rest[close + "</script>".len()..];
    }
    sources.join(" ")
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "wasm" => "application/wasm",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_types_follow_the_extension() {
        assert_eq!(content_type("a_bg.wasm"), "application/wasm");
        assert_eq!(content_type("x.unknown"), "application/octet-stream");
    }

    #[test]
    fn a_path_outside_the_bundle_is_not_served() {
        assert!(get("../../Cargo.toml").is_none());
        assert!(get("/nope.js").is_none());
    }
}
