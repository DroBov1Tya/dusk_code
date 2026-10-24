use crate::state::Message;

#[derive(Debug, Clone)]
pub struct MessageCreated {
    pub message: Message,
}
