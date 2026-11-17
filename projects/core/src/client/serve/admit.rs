//! The one place a request is checked against the scope of the client that sent it,
//! before any handler runs, and where an answer that lists threads is cut to that scope.

use super::{ClientId, Server};
use crate::client::scope::{Subject, classify};
use dusk_protocol::client::{Answer, Request, Response};
use std::{collections::HashSet, sync::Arc};
use tracing::warn;

/// Says nothing of why: the kinds of requests a client may send are not its to learn.
const REFUSED: &str = "this client may not send that request";

/// The same for a thread that is not there and one out of the scope, so a client cannot
/// tell which ids exist.
const NO_THREAD: &str = "no such thread";

impl Server {
    /// Whether `client` may send `request`. A yes may carry the chats its answer is cut
    /// to; a no is the text for the client, and is logged here with who asked and the
    /// kind of request, never what the request held.
    pub(super) async fn admit(
        &self,
        client: ClientId,
        request: &Request,
    ) -> Result<Option<Arc<HashSet<String>>>, &'static str> {
        let access = classify(request);
        let Some(attached) = self.clients.get(&client) else {
            warn!(area = ?access.area, "refused a request: its client is not attached");
            return Err(REFUSED);
        };
        let source = attached.source.as_str();
        if !attached.scope.takes(&access) {
            warn!(client = source, area = ?access.area, "refused a request: outside the client's scope");
            return Err(REFUSED);
        }
        let Some(ids) = attached.scope.bound() else {
            return Ok(None);
        };
        match access.subject {
            Subject::Thread(thread_id) if self.within(ids, thread_id).await => Ok(None),
            Subject::Thread(_) => {
                warn!(client = source, area = ?access.area, "refused a request: its thread is outside the client's scope");
                Err(NO_THREAD)
            }
            Subject::Threads => Ok(Some(Arc::clone(ids))),
            // `takes` refused it already; a scope bound to threads asks about them only.
            Subject::None => Err(REFUSED),
        }
    }

    /// Whether the thread is one of the chats or an agent's thread below one.
    async fn within(&self, ids: &HashSet<String>, thread_id: &str) -> bool {
        if ids.contains(thread_id) {
            return true;
        }
        // A thread that is not stored has no chat above it: refused like any other.
        let state = self.runtime.state_of(thread_id).await;
        state
            .root_thread(thread_id)
            .await
            .is_ok_and(|root| ids.contains(&root))
    }

    /// Keeps of an answer that lists threads only those within `ids`. Any other answer is
    /// refused: what it holds was not checked.
    pub(super) async fn cut(&self, ids: &HashSet<String>, response: Response) -> Response {
        match response {
            Response::Threads(threads) => {
                let mut kept = Vec::new();
                for thread in threads {
                    if self.within(ids, &thread.id).await {
                        kept.push(thread);
                    }
                }
                Response::Threads(kept)
            }
            Response::Found(found) => {
                let mut kept = Vec::new();
                for message in found {
                    if self.within(ids, &message.thread_id).await {
                        kept.push(message);
                    }
                }
                Response::Found(kept)
            }
            Response::Answer(answer) => match *answer {
                Answer::Threads(threads) => {
                    let mut kept = Vec::new();
                    for thread in threads {
                        if self.within(ids, &thread.id).await {
                            kept.push(thread);
                        }
                    }
                    Response::Answer(Box::new(Answer::Threads(kept)))
                }
                _ => Response::Error(REFUSED.to_owned()),
            },
            Response::Error(error) => Response::Error(error),
            _ => Response::Error(REFUSED.to_owned()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        client::{
            Area, Scope,
            serve::{Attached, Subscribers},
        },
        init::paths::Paths,
        runtime::Runtime,
    };
    use dusk_protocol::{
        client::{ThreadRequest, WorkRequest},
        thread::Found,
    };

    const AREAS: &[Area] = &[Area::ThreadRead, Area::ThreadWrite];

    /// A core with the chats `a` and `b`, each with an agent's thread below it, `a` with
    /// one more below that; client 1 is bound to `a`, client 2 sees all.
    async fn server(name: &str) -> Server {
        let root = std::env::temp_dir().join(format!("dusk-admit-{name}-{}", std::process::id()));
        let paths = Paths::new(root.join("config"), root.join("work"));
        let runtime = Runtime::start(paths, Some(root.join("state.db")))
            .await
            .unwrap();
        let state = runtime.state();
        for chat in ["a", "b"] {
            state.create_thread(chat, "test").await.unwrap();
        }
        for (id, parent) in [("a1", "a"), ("a2", "a1"), ("b1", "b")] {
            state
                .create_agent_thread(id, parent, "agent", id)
                .await
                .unwrap();
        }
        let (mut server, _) = Server::new(runtime, Subscribers::default());
        for (client, scope) in [
            (1, Scope::threads(["a".to_owned()], AREAS)),
            (2, Scope::full()),
        ] {
            let source = "test".to_owned();
            server.clients.insert(client, Attached { source, scope });
        }
        server
    }

    fn messages(thread_id: &str) -> Request {
        Request::Thread(ThreadRequest::Messages {
            thread_id: thread_id.to_owned(),
        })
    }

    #[tokio::test]
    async fn a_shared_chat_brings_its_agents_threads_and_no_other() {
        let server = server("agents").await;
        for thread_id in ["a", "a1", "a2"] {
            assert_eq!(server.admit(1, &messages(thread_id)).await, Ok(None));
        }
        // Another chat, an agent of another chat and a thread that is not there are
        // refused in the same words.
        for thread_id in ["b", "b1", "nowhere", "", "a%", "A"] {
            assert_eq!(
                server.admit(1, &messages(thread_id)).await,
                Err(NO_THREAD),
                "{thread_id}"
            );
            let plan = Request::Work(WorkRequest::Plan {
                thread_id: thread_id.to_owned(),
            });
            assert_eq!(server.admit(1, &plan).await, Err(NO_THREAD));
            assert_eq!(server.admit(2, &messages(thread_id)).await, Ok(None));
        }
    }

    #[tokio::test]
    async fn a_client_that_is_not_attached_is_refused() {
        let server = server("unknown").await;
        for request in [messages("a"), Request::Thread(ThreadRequest::List)] {
            assert_eq!(server.admit(3, &request).await, Err(REFUSED));
        }
    }

    #[tokio::test]
    async fn an_answer_that_lists_threads_is_cut_to_the_scope() {
        let server = server("cut").await;
        let list = Request::Thread(ThreadRequest::List);
        assert_eq!(server.admit(2, &list).await, Ok(None));
        let ids = server.admit(1, &list).await.unwrap().unwrap();

        let all = server.runtime.list_threads().await.unwrap();
        assert_eq!(all.len(), 2);
        let Response::Threads(kept) = server.cut(&ids, Response::Threads(all.clone())).await else {
            panic!("not a list of threads");
        };
        assert_eq!(
            kept.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["a"]
        );

        let found = |thread_id: &str| Found {
            thread_id: thread_id.to_owned(),
            title: Some("title".to_owned()),
            sequence: 1,
            snippet: "words".to_owned(),
        };
        let everything = ["a", "b", "a2", "b1", "nowhere"].map(found).to_vec();
        let Response::Found(kept) = server.cut(&ids, Response::Found(everything)).await else {
            panic!("not found messages");
        };
        assert_eq!(kept, [found("a"), found("a2")]);

        let answer = Response::Answer(Box::new(Answer::Threads(all)));
        let Response::Answer(answer) = server.cut(&ids, answer).await else {
            panic!("not an answer");
        };
        assert!(matches!(*answer, Answer::Threads(kept) if kept.len() == 1 && kept[0].id == "a"));

        // What was not checked does not pass.
        for unchecked in [
            Response::Text("b".to_owned()),
            Response::Answer(Box::new(Answer::NewThread)),
        ] {
            assert_eq!(
                server.cut(&ids, unchecked).await,
                Response::Error(REFUSED.to_owned())
            );
        }
    }
}
