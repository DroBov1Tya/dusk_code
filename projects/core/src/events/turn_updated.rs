use crate::state::Turn;

#[derive(Debug, Clone)]
pub struct TurnUpdated {
    pub turn: Turn,
}
