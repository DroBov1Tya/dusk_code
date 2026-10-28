//! Client input as the thread sees it: while the agent works, messages are held
//! for later, `send_now` and `cancel` stop the work.

use std::future::Future;

use tokio::sync::mpsc;

/// What a client sends to a thread.
pub(super) enum Input {
    Send(String),
    SendNow(String),
    Cancel,
}

/// Why running work was stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stop {
    Cancelled,
    Preempted,
}

pub(super) struct Inbox {
    commands: mpsc::UnboundedReceiver<Input>,
    open: bool,
    held: Vec<String>,
}

impl Inbox {
    pub(super) fn new(commands: mpsc::UnboundedReceiver<Input>) -> Self {
        Self {
            commands,
            open: true,
            held: Vec::new(),
        }
    }

    /// Waits for the next command while idle; `None` once every client has gone.
    pub(super) async fn next(&mut self) -> Option<Input> {
        let command = self.commands.recv().await;
        self.open &= command.is_some();
        command
    }

    /// Commands already sent, without waiting.
    pub(super) fn ready(&mut self) -> Vec<Input> {
        std::iter::from_fn(|| self.commands.try_recv().ok()).collect()
    }

    /// Runs `work` while taking commands: messages are held until the work is done,
    /// `send_now` and `cancel` stop it. Gone clients let the work finish.
    pub(super) async fn race<T>(&mut self, work: impl Future<Output = T>) -> Result<T, Stop> {
        tokio::pin!(work);
        loop {
            tokio::select! {
                value = &mut work => return Ok(value),
                command = self.commands.recv(), if self.open => match command {
                    Some(Input::Send(text)) => self.held.push(text),
                    Some(Input::SendNow(text)) => {
                        self.held.push(text);
                        return Err(Stop::Preempted);
                    }
                    Some(Input::Cancel) => return Err(Stop::Cancelled),
                    None => self.open = false,
                },
            }
        }
    }

    /// Messages held during the work, in the order they were sent.
    pub(super) fn take_held(&mut self) -> Vec<String> {
        std::mem::take(&mut self.held)
    }
}
