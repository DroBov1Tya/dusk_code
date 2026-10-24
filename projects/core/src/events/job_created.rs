use crate::state::Job;

#[derive(Debug, Clone)]
pub struct JobCreated {
    pub job: Job,
}
