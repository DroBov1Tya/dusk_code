//! What an attached client may see and ask. The code that attaches a client decides its
//! scope; nothing the client sends can widen it. The core checks every request against it
//! before any handler runs (`serve::admit`) and every thread event before it is queued.
//!
//! A scope bound to threads covers those chats and the agent threads below them: a shared
//! chat shows what its subagents do. Such a scope takes only requests about its threads,
//! whatever its areas, and gets a blank status.

mod classify;
#[cfg(test)]
mod names;
#[cfg(test)]
mod samples;
#[cfg(test)]
mod tests;

use std::{collections::HashSet, sync::Arc};

use dusk_protocol::client::Request;

use crate::message::sources::HostFiles;

pub use classify::{Access, Subject, classify};

/// A group of requests that is allowed or refused together. An area that would mix safe
/// and dangerous calls is split, so a scope never has to take both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    /// Listing and searching chats, reading a thread's messages, jobs, plan and the files
    /// it changed.
    ThreadRead,
    /// Opening a thread, sending to it, stopping it, renaming it, changing its sources.
    /// A sent message may be a command of its chat: `/compact`, `/fork`, `/goal`,
    /// `/undo <n>`.
    ThreadWrite,
    /// Starting a new chat in a folder the client names.
    ThreadCreate,
    ThreadDelete,
    /// Running a command the client wrote, in the chat's folder.
    ThreadCommand,
    /// Answering a tool's request for a yes, in the chat it was asked in.
    ThreadApprove,
    /// Changing how much a thread's agent may do without asking.
    ThreadApprovals,
    /// Limits, switches, modes, providers and secrets.
    Settings,
    /// Switching a plugin on: its programs start on this machine.
    PluginEnable,
    /// Skills, agents, snippets, the profile, and adding or restarting MCP servers.
    Catalog,
    /// The text of a saved MCP server's config, which holds its keys and tokens.
    McpConfig,
    /// Projects, their git state, files to read, variables and schedules; and the diff
    /// of a chat's folder, which is its whole project's.
    Project,
    FileWrite,
    /// Slash commands outside a thread: model, provider, settings, effort, migrate and
    /// the context report; and
    /// naming a file of this machine for a thread's sources, by `/sources add` or by a
    /// path or an `@` mention in a sent message.
    Command,
    System,
    Voice,
    /// The user's own shell.
    Terminal,
    /// The parts plugins add to the window.
    PluginParts,
    /// Reading the plugin store.
    Store,
    /// Installing, updating and removing what the store offers.
    StoreInstall,
    /// Installing a release of duskcode.
    Update,
    Shutdown,
    /// Listing the attached clients and detaching one.
    Clients,
}

/// What a browser may ask: everything but stopping the core, a shell, plugin parts,
/// installing or switching on a plugin, writing a file and the config text of a saved
/// MCP server.
const WEB: &[Area] = &[
    Area::ThreadRead,
    Area::ThreadWrite,
    Area::ThreadCreate,
    Area::ThreadDelete,
    Area::ThreadCommand,
    Area::ThreadApprove,
    Area::ThreadApprovals,
    Area::Settings,
    Area::Catalog,
    Area::Project,
    Area::Command,
    Area::System,
    Area::Voice,
    Area::Store,
    Area::Update,
];

/// What one attached client may see and ask; fixed when it attaches.
#[derive(Debug, Clone)]
pub struct Scope(Kind);

#[derive(Debug, Clone)]
enum Kind {
    Full,
    /// A signed-in browser: the areas of `WEB`.
    Web,
    /// These chats and the agent threads below them, and only requests about them.
    Threads {
        ids: Arc<HashSet<String>>,
        areas: &'static [Area],
    },
}

impl Scope {
    /// Everything: for a client in the core's own process, such as the terminal and the
    /// desktop app.
    pub fn full() -> Self {
        Self(Kind::Full)
    }

    /// A signed-in browser.
    pub fn web() -> Self {
        Self(Kind::Web)
    }

    /// Only these chats, and of the requests about them only those in `areas`. An area
    /// that is not about a thread gives nothing here.
    ///
    /// Open before a remote client may be attached this way:
    /// - the HTTP routes `/api/card` and `/api/output` are for the web scope and check no
    ///   scope;
    /// - event text, such as `TurnFailed.error_message` and tool output, reaches this
    ///   scope as the core wrote it, not mapped like a handler's error;
    /// - `Thread.owner` carries raw token ids.
    pub fn threads(ids: impl IntoIterator<Item = String>, areas: &'static [Area]) -> Self {
        Self(Kind::Threads {
            ids: Arc::new(ids.into_iter().collect()),
            areas,
        })
    }

    /// Whether the scope takes this kind of request. For a scope bound to threads the
    /// thread the request names is checked apart, by `covers`.
    pub fn permits(&self, request: &Request) -> bool {
        self.takes(&classify(request))
    }

    pub(super) fn takes(&self, access: &Access<'_>) -> bool {
        match &self.0 {
            Kind::Full => true,
            Kind::Web => WEB.contains(&access.area),
            Kind::Threads { areas, .. } => {
                access.subject != Subject::None && areas.contains(&access.area)
            }
        }
    }

    /// The chats the scope is bound to, or none when it sees every thread.
    pub(super) fn bound(&self) -> Option<&Arc<HashSet<String>>> {
        match &self.0 {
            Kind::Full | Kind::Web => None,
            Kind::Threads { ids, .. } => Some(ids),
        }
    }

    /// Whether the events of the open chat `thread_id` go to this client. An open thread
    /// is always a chat, and what its agents do comes inside its own events.
    pub(super) fn covers(&self, thread_id: &str) -> bool {
        self.bound().is_none_or(|ids| ids.contains(thread_id))
    }

    /// Whether a message the client sends may name files of this machine. It is the
    /// area of `/sources add`: both copy a file by a path the client wrote.
    pub(super) fn host_files(&self) -> HostFiles {
        let held = match &self.0 {
            Kind::Full => true,
            Kind::Web => WEB.contains(&Area::Command),
            Kind::Threads { areas, .. } => areas.contains(&Area::Command),
        };
        match held {
            true => HostFiles::Taken,
            false => HostFiles::Left,
        }
    }

    /// Whether it is full access: the only scope that learns which client stopped the core.
    pub(super) fn is_full(&self) -> bool {
        matches!(&self.0, Kind::Full)
    }

    /// What the scope is called in the list of clients.
    pub(super) fn name(&self) -> &'static str {
        match &self.0 {
            Kind::Full => "full",
            Kind::Web => "web",
            Kind::Threads { .. } => "threads",
        }
    }

    /// Whether the client reads the text of a handler's error as it is: the user's own
    /// clients and the browser, which is the same person as the desktop. Any other gets
    /// `Shown` sentences and references.
    pub fn raw_errors(&self) -> bool {
        matches!(&self.0, Kind::Full | Kind::Web)
    }

    /// Whether the client gets the status: it is settings-level state, so only a scope
    /// that may read the settings anyway has it.
    pub fn reads_status(&self) -> bool {
        match &self.0 {
            Kind::Full => true,
            Kind::Web => WEB.contains(&Area::Settings),
            Kind::Threads { .. } => false,
        }
    }
}
