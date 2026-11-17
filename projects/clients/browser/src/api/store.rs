//! The core's requests as typed calls, so a view says what it wants and not which
//! response variant carries it. Every error is the core's own text. A browser reads the
//! store and switches packs; installing is refused by the server.

use crate::link::request;
use dusk_protocol::{
    catalog::SkillPack,
    client::{CatalogRequest, Request, Response},
    plugin::{Order, StoreList, StoreRequest},
};

use super::*;

pub async fn store(order: Order, category: Option<String>) -> Res<StoreList> {
    let call = Request::Store(StoreRequest::List { order, category });
    ask!(call, Response::Store(list) => list)
}

/// A cover as a `data:` address, only for the two types a list may name.
pub async fn cover(source: String, name: String) -> Res<String> {
    let call = Request::Store(StoreRequest::Cover { source, name });
    let (base64, mime) = ask!(call, Response::Cover { base64, mime } => (base64, mime))?;
    match mime.as_str() {
        "image/png" | "image/webp" => Ok(format!("data:{mime};base64,{base64}")),
        other => Err(format!("a cover of type {other} is not shown")),
    }
}

pub async fn skill_packs() -> Res<Vec<SkillPack>> {
    ask!(catalog(CatalogRequest::SkillPacks), Response::SkillPacks(packs) => packs)
}
