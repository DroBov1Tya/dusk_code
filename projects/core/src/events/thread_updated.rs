use crate::state::ThreadMetadata;

#[derive(Debug, Clone)]
pub struct ThreadUpdated {
    pub thread: ThreadMetadata,
}
