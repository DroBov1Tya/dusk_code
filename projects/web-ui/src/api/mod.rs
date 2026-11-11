//! The core's requests as typed calls, so a view says what it wants and not which
//! response variant carries it. Every error is the core's own text.

//! The core's requests as typed calls, so a view says what it wants and not which
//! response variant carries it. Every error is the core's own text.

use crate::link::request;
use dusk_protocol::client::{
    CatalogRequest, ProjectRequest, Request, Response, SettingsRequest, ThreadRequest, WorkRequest,
};

macro_rules! ask {
    ($call:expr, $variant:pat => $value:expr) => {
        match request($call).await? {
            $variant => Ok($value),
            other => unexpected(other),
        }
    };
}

mod catalog;
mod projects;
mod reports;
mod settings;
mod threads;
mod voice;
mod work;

pub use catalog::*;
pub use projects::*;
pub use reports::*;
pub use settings::*;
pub use threads::*;
pub use voice::*;
pub use work::*;

pub type Res<T> = Result<T, String>;

pub(super) fn unexpected<T>(response: Response) -> Res<T> {
    Err(format!("The core gave an unexpected answer: {response:?}"))
}

/// A request whose answer is `Done`.
pub async fn done(call: Request) -> Res<()> {
    match request(call).await? {
        Response::Done => Ok(()),
        other => unexpected(other),
    }
}

pub(super) fn thread(call: ThreadRequest) -> Request {
    Request::Thread(call)
}

pub(super) fn work(call: WorkRequest) -> Request {
    Request::Work(call)
}

pub(super) fn project(call: ProjectRequest) -> Request {
    Request::Project(call)
}

pub(super) fn catalog(call: CatalogRequest) -> Request {
    Request::Catalog(call)
}

pub(super) fn settings(call: SettingsRequest) -> Request {
    Request::Settings(call)
}
