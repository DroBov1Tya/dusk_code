//! Background commands: started by `run_command`, owned by the runtime, so ending the
//! call or the turn does not drop them. A lease of the kernel tells which runtime owns
//! a job, for recovery after a crash.

mod lease;
mod manager;

pub use manager::{JobInfo, JobManager, JobOwner, Launch};
