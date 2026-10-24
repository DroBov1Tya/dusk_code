use crate::state::ThreadMetadata;

#[derive(Debug, Clone)]
pub struct ThreadCreated {
    pub thread: ThreadMetadata,
}
