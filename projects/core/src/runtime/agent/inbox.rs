//! Client input as the thread sees it. What clients send is already stored as pending
//! messages; the channel only wakes the thread, and `send_now` and `cancel` stop the
//! running work.

use std::future::Future;

use tokio::sync::mpsc;

/// What a client did.
pub enum Input {
    Sent,
    SentNow,
    Cancel,
}

/// Why running work was stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Cancelled,
    Preempted,
}

pub struct Inbox {
    inputs: mpsc::UnboundedReceiver<Input>,
    open: bool,
}

impl Inbox {
    pub fn new(inputs: mpsc::UnboundedReceiver<Input>) -> Self {
        Self { inputs, open: true }
    }

    /// Waits while idle for what woke the thread; `None` once every client has gone.
    pub async fn wait(&mut self) -> Option<Input> {
        let input = self.inputs.recv().await;
        self.open &= input.is_some();
        // Whatever else is already there has been sent too; one wake-up is enough, but a
        // stop among them still counts.
        let mut input = input?;
        while let Ok(next) = self.inputs.try_recv() {
            if matches!(next, Input::Cancel) {
                input = next;
            }
        }
        Some(input)
    }

    /// Runs `work` while taking input: sent messages wait until the work is done,
    /// `send_now` and `cancel` stop it. Gone clients let the work finish.
    pub async fn race<T>(&mut self, work: impl Future<Output = T>) -> Result<T, Stop> {
        tokio::pin!(work);
        loop {
            tokio::select! {
                value = &mut work => return Ok(value),
                input = self.inputs.recv(), if self.open => match input {
                    Some(Input::Sent) => {}
                    Some(Input::SentNow) => return Err(Stop::Preempted),
                    Some(Input::Cancel) => return Err(Stop::Cancelled),
                    None => self.open = false,
                },
            }
        }
    }
}
