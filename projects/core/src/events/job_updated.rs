use crate::state::Job;

#[derive(Debug, Clone)]
pub struct JobUpdated {
    pub job: Job,
}
