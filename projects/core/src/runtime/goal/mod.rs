//! Goals: what `/goal` sets and how a chat works toward it. `action` reads the
//! command and writes the messages that start and remind each round; `rounds` queues
//! the rounds after each turn, `stall` pauses them once they change nothing and
//! `monologue` notes rounds that only talk;
//! `verification` checks saved evidence against the criteria, and only it marks a goal
//! done. The records are in `state::thread::goals`.

pub mod action;
mod monologue;
mod rounds;
mod stall;
mod verification;

pub(crate) use monologue::Monologue;
