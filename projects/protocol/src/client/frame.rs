//! The frames a remote client, such as a browser, exchanges with the core over one
//! message stream. Each carries the protocol's own types, so the stream adds no API.

use serde::{Deserialize, Serialize};

use super::{Event, Request, Response, Status};

/// What the remote client sends: a request and the number its response comes back under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestFrame {
    pub id: u64,
    pub request: Request,
}

/// What the core sends: answers, and what it pushes on its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServerFrame {
    Response {
        id: u64,
        response: Response,
    },
    Event(Event),
    /// The status as it is now, first on a new stream and after each change.
    Status(Box<Status>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ThreadRequest;

    #[test]
    fn a_request_frame_survives_json() {
        let frame = RequestFrame {
            id: 7,
            request: Request::Thread(ThreadRequest::Send {
                thread_id: "t".into(),
                text: "hi".into(),
                now: false,
            }),
        };
        let json = serde_json::to_string(&frame).unwrap();
        assert_eq!(serde_json::from_str::<RequestFrame>(&json).unwrap(), frame);
    }

    #[test]
    fn a_frame_that_is_not_a_request_is_refused() {
        assert!(serde_json::from_str::<RequestFrame>(r#"{"id":1,"request":"Nope"}"#).is_err());
        assert!(serde_json::from_str::<RequestFrame>(r#"{"request":"Shutdown"}"#).is_err());
    }
}
