//! The one place a request is checked against the scope of the client that sent it,
//! before any handler runs, and where an answer that lists threads is cut to that scope.

use super::{ClientId, Server};
use crate::client::scope::{Subject, classify};
use dusk_protocol::{
    client::{Answer, Request, Response, ThreadRequest},
    plugin::StoreRequest,
};
use std::{collections::HashSet, sync::Arc};
use tracing::warn;

/// Says nothing of why: the kinds of requests a client may send are not its to learn.
const REFUSED: &str = "this client may not send that request";

/// The same for a thread that is not there and one out of the scope, so a client cannot
/// tell which ids exist.
const NO_THREAD: &str = "no such thread";

/// A request only an owner's client may send: one that may send it may answer what a
/// tool asks the owner.
fn owner_request() -> Request {
    Request::Store(StoreRequest::AddSource {
        url: String::new(),
        key: String::new(),
    })
}

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
        // A change of the settings a tool proposed is answered only by a client that
        // could make it itself: a chat shared through a narrower scope cannot.
        if let Request::Thread(ThreadRequest::Approve { id, .. }) = request
            && self.runtime.approval_owner_only(*id)
            && !attached.scope.permits(&owner_request())
        {
            warn!(
                client = source,
                "refused an answer: only the owner may answer it"
            );
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
            server.clients.insert(client, Attached::new(source, scope));
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

    /// A change of the settings a tool proposed is answered only by a client that could
    /// make it: a shared chat's scope may answer its other requests, never this one.
    #[tokio::test]
    async fn only_the_owner_answers_a_change_of_the_settings() {
        use crate::tools::Progress;
        use dusk_protocol::tools::Request as Ask;
        const APPROVE: &[Area] = &[Area::ThreadRead, Area::ThreadApprove];
        let mut server = server("owner").await;
        let source = "test".to_owned();
        server.clients.insert(
            3,
            Attached::new(source, Scope::threads(["a".to_owned()], APPROVE)),
        );
        let approvals = server.runtime.approvals();
        let (events, mut asked) = tokio::sync::mpsc::unbounded_channel();
        let progress = Progress {
            call_id: "c".into(),
            events,
        };
        let waiting = Arc::clone(&approvals);
        let change = Ask::Settings {
            change: "add the Workshop source far".into(),
        };
        let task =
            tokio::spawn(async move { waiting.ask_owner("a", change, Some(progress)).await });
        let id = loop {
            if let Some(crate::runtime::ThreadEvent::Approval(request)) = asked.recv().await {
                break request.id;
            }
        };
        let approve = |id| {
            Request::Thread(ThreadRequest::Approve {
                thread_id: "a".into(),
                id,
                decision: dusk_protocol::tools::Decision::Once,
            })
        };
        assert_eq!(server.admit(3, &approve(id)).await, Err(REFUSED));
        assert_eq!(server.admit(1, &approve(id)).await, Err(REFUSED));
        assert_eq!(server.admit(2, &approve(id)).await, Ok(None));
        // Any other request of that chat it may still answer.
        assert_eq!(server.admit(3, &approve(id + 1)).await, Ok(None));
        task.abort();
    }

    /// Only a scope holding the area of `/sources add` has a sent message read for the
    /// files of this machine it names; any other has its words kept as words.
    #[tokio::test]
    async fn a_message_names_host_files_only_for_a_scope_that_may() {
        use crate::message::sources::HostFiles;
        let mut server = server("files").await;
        let with_files = Scope::threads(["a".to_owned()], &[Area::ThreadWrite, Area::Command]);
        let source = "test".to_owned();
        let scope = with_files;
        server.clients.insert(3, Attached::new(source, scope));
        for (caller, files) in [
            (1, HostFiles::Left),
            (2, HostFiles::Taken),
            (3, HostFiles::Taken),
            (9, HostFiles::Left),
        ] {
            server.caller = caller;
            assert_eq!(server.host_files(), files, "client {caller}");
        }
        assert_eq!(Scope::web().host_files(), HostFiles::Taken);

        let runtime = &server.runtime;
        let dir = crate::testing::temp_dir("admit-files");
        std::fs::create_dir_all(&dir).unwrap();
        let shot = dir.join("shot.png");
        let mut png = Vec::new();
        image::RgbImage::new(8, 8)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        std::fs::write(&shot, png).unwrap();
        runtime.mentions().save_snippet("note", "SNIPPET").unwrap();
        let paste = dusk_protocol::thread::wrap("PASTED BODY");
        let message = format!("see {} and @note {paste}", shot.display());

        server.caller = 1;
        let kept = runtime
            .keep_pastes("a", &message, server.host_files())
            .await
            .unwrap();
        // The path and the mention are still words; the paste, which is the client's
        // own text, is kept as a source like anyone's.
        let words = format!("see {} and @note ", shot.display());
        assert!(kept.starts_with(&words), "{kept}");
        assert!(!kept.contains("PASTED BODY"));
        let names = |sources: Vec<crate::state::Source>| {
            sources
                .into_iter()
                .map(|source| source.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(runtime.sources("a").await.unwrap()), ["pasted-1.txt"]);

        server.caller = 2;
        let kept = runtime
            .keep_pastes("b", &message, server.host_files())
            .await
            .unwrap();
        assert!(!kept.contains(&shot.display().to_string()) && !kept.contains("@note"));
        assert_eq!(runtime.sources("b").await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn a_bound_client_cannot_point_a_resumed_chat_at_another_folder() {
        let mut server = server("resume").await;
        let state = server.runtime.state();
        state
            .set_thread_project("a", "/stored/project")
            .await
            .unwrap();
        let asked = || std::path::PathBuf::from("/");

        server.caller = 1;
        let folder = server.resume_folder("a", asked()).await.unwrap();
        assert_eq!(folder, std::path::Path::new("/stored/project"));
        // A chat with no folder stored does not take the one the client names.
        server.clients.get_mut(&1).unwrap().scope =
            Scope::threads(["a".to_owned(), "b".to_owned()], AREAS);
        for thread_id in ["b", "nowhere"] {
            let refused = server.resume_folder(thread_id, asked()).await.unwrap_err();
            assert_eq!(
                refused.to_string(),
                "this chat cannot be opened from this client"
            );
        }

        // Who sees every chat opens it where it says, as before.
        server.caller = 2;
        for thread_id in ["a", "b"] {
            assert_eq!(
                server.resume_folder(thread_id, asked()).await.unwrap(),
                asked()
            );
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
