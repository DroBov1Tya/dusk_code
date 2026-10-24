use crate::state::Turn;

#[derive(Debug, Clone)]
pub struct TurnCreated {
    pub turn: Turn,
}
