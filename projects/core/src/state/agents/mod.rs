//! What belongs to agents: the identities found under `agents/`, and for each agent's
//! thread the effort it works at, the model its asker picked and the phase it is in.

mod efforts;
mod identities;
mod models;
mod phases;

pub use identities::Identity;
pub use phases::AgentPhase;

use super::{State, timestamp_ms};
