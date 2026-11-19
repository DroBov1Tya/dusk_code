//! Client scopes against a real core: what a client bound to some chats can reach, hear
//! and learn of the others, and what a browser is refused.

use std::path::{Path, PathBuf};

use dusk_core::{
    app::{Paths, start_hub},
    client::{Area, Client, Hub, Scope},
};
use dusk_protocol::{
    client::{
        Answer, CatalogRequest, Command, Event, Hello, PROTOCOL_VERSION, PluginRequest,
        ProjectRequest, Request, Response, Secret, SettingsRequest, TerminalRequest, ThreadRequest,
        VoiceRequest, WorkRequest,
    },
    plugin::StoreRequest,
    tools::Decision,
};
use std::sync::Arc;

const REFUSED: &str = "this client may not send that request";
const NO_THREAD: &str = "no such thread";

/// Every area a scope bound to threads could be given.
const EVERYTHING: &[Area] = &[
    Area::ThreadRead,
    Area::ThreadWrite,
    Area::ThreadCreate,
    Area::ThreadDelete,
    Area::ThreadCommand,
    Area::ThreadApprovals,
    Area::Settings,
    Area::PluginEnable,
    Area::Catalog,
    Area::McpConfig,
    Area::Project,
    Area::FileWrite,
    Area::Command,
    Area::System,
    Area::Voice,
    Area::Terminal,
    Area::PluginParts,
    Area::Store,
    Area::StoreInstall,
    Area::Update,
    Area::Shutdown,
];

struct Core {
    hub: Arc<Hub>,
    folder: PathBuf,
}

async fn core(name: &str) -> Core {
    let root = std::env::temp_dir().join(format!("dusk-scope-{name}-{}", std::process::id()));
    let folder = root.join("project");
    std::fs::create_dir_all(&folder).unwrap();
    let paths = Paths::new(root.join("config"), root.join("work"));
    let hub = start_hub(paths, Some(root.join("state.db"))).await.unwrap();
    Core { hub, folder }
}

fn attach(core: &Core, name: &str, scope: Scope) -> Client {
    let hello = Hello {
        protocol: PROTOCOL_VERSION,
        client: name.to_owned(),
    };
    core.hub.attach(hello, scope).unwrap()
}

fn bound(core: &Core, name: &str, thread_id: &str) -> Client {
    attach(
        core,
        name,
        Scope::threads([thread_id.to_owned()], EVERYTHING),
    )
}

/// What a client bound to `mine` is answered for one request, from a client of its own:
/// twenty refusals in a minute detach a client, and after that every request is refused
/// for that reason alone. It is still attached afterwards, or the answer proved nothing.
async fn refused_alone(core: &Core, mine: &str, request: Request) -> Response {
    let client = bound(core, "phone", mine);
    let answer = client.request(request).await;
    assert!(
        client.threads().messages(mine).await.is_ok(),
        "the client was detached by its refusals"
    );
    answer
}

/// The requests that name a thread: reading it, feeding it, stopping it, going back in
/// it, opening it and erasing it.
fn about(thread_id: &str, folder: &Path) -> Vec<Request> {
    let id = || thread_id.to_owned();
    let mut requests: Vec<Request> = [
        ThreadRequest::Messages { thread_id: id() },
        ThreadRequest::Tasks { thread_id: id() },
        ThreadRequest::LocalUnsent { thread_id: id() },
        ThreadRequest::GetApprovals { thread_id: id() },
        ThreadRequest::Resume {
            thread_id: id(),
            cwd: folder.to_path_buf(),
        },
        ThreadRequest::Send {
            thread_id: id(),
            text: "hello".to_owned(),
            now: true,
        },
        // Forking and going back are messages to the thread.
        ThreadRequest::Send {
            thread_id: id(),
            text: "/fork".to_owned(),
            now: false,
        },
        ThreadRequest::Send {
            thread_id: id(),
            text: "/undo 1".to_owned(),
            now: false,
        },
        ThreadRequest::ResendLocalUnsent {
            thread_id: id(),
            id: "1".to_owned(),
        },
        ThreadRequest::Cancel { thread_id: id() },
        ThreadRequest::StopAgent { thread_id: id() },
        ThreadRequest::Background {
            thread_id: id(),
            call_id: "1".to_owned(),
        },
        ThreadRequest::RunCommand {
            thread_id: id(),
            command: "true".to_owned(),
            run: 1,
        },
        ThreadRequest::Rename {
            thread_id: id(),
            title: "taken".to_owned(),
        },
        ThreadRequest::SetApprovals {
            thread_id: id(),
            policy: None,
        },
        ThreadRequest::Close { thread_id: id() },
        ThreadRequest::Delete { thread_id: id() },
    ]
    .into_iter()
    .map(Request::Thread)
    .collect();
    requests.extend(
        [
            WorkRequest::Jobs { thread_id: id() },
            WorkRequest::Plan { thread_id: id() },
            WorkRequest::SavePlan {
                thread_id: id(),
                text: "plan".to_owned(),
            },
            WorkRequest::ChangedFiles { thread_id: id() },
            WorkRequest::UploadPicture {
                thread_id: id(),
                name: "x.png".to_owned(),
                bytes: Vec::new(),
            },
        ]
        .into_iter()
        .map(Request::Work),
    );
    requests.extend(
        [
            Command::Undo {
                thread_id: Some(id()),
                sequence: None,
            },
            Command::Sources {
                thread_id: Some(id()),
                add: None,
            },
            Command::Jobs {
                thread_id: Some(id()),
            },
        ]
        .into_iter()
        .map(Request::Command),
    );
    requests
}

fn thread_of(event: &Event) -> Option<&str> {
    match event {
        Event::Thread { thread_id, .. } | Event::Ended { thread_id } => Some(thread_id),
        _ => None,
    }
}

fn drain(client: &mut Client) -> Vec<Event> {
    std::iter::from_fn(|| client.events.try_recv().ok()).collect()
}

#[tokio::test]
async fn a_client_bound_to_a_chat_cannot_reach_hear_or_learn_of_another() {
    let core = core("threads").await;
    let mut owner = attach(&core, "owner", Scope::full());
    let mine = owner.threads().start(core.folder.clone()).await.unwrap();
    let other = owner.threads().start(core.folder.clone()).await.unwrap();
    owner.threads().rename(&mine, "shared").await.unwrap();
    owner.threads().rename(&other, "private").await.unwrap();
    let mut phone = bound(&core, "phone", &mine);

    // Another chat is refused in the words a chat that is not there gets, whatever is
    // asked of it, and it is still whole afterwards.
    for (theirs, absent) in about(&other, &core.folder)
        .into_iter()
        .zip(about("0190-not-a-thread", &core.folder))
    {
        // One client for each pair: twenty refusals in a minute detach a client.
        let probe = bound(&core, "probe", &mine);
        let refused = probe.request(theirs).await;
        assert_eq!(refused, Response::Error(NO_THREAD.to_owned()));
        assert_eq!(refused, probe.request(absent).await);
    }
    let listed = owner.threads().list().await.unwrap();
    let title = |id: &str| {
        listed
            .iter()
            .find(|t| t.id == id)
            .and_then(|t| t.title.clone())
    };
    assert_eq!(title(&other).as_deref(), Some("private"));

    // Its own chat answers.
    phone.threads().messages(&mine).await.unwrap();
    phone.threads().rename(&mine, "ours").await.unwrap();

    // Lists hold its chat only.
    let ids = |threads: Vec<dusk_protocol::thread::Thread>| -> Vec<String> {
        threads.into_iter().map(|thread| thread.id).collect()
    };
    assert_eq!(ids(phone.threads().list().await.unwrap()), [mine.as_str()]);
    assert_eq!(owner.threads().list().await.unwrap().len(), 2);
    let resume = Command::Resume {
        cwd: Some(core.folder.clone()),
    };
    match phone.command(resume.clone()).await.unwrap() {
        Answer::Threads(threads) => assert_eq!(ids(threads), [mine.as_str()]),
        other => panic!("not a list of threads: {other:?}"),
    }
    match owner.command(resume).await.unwrap() {
        Answer::Threads(threads) => assert_eq!(threads.len(), 2),
        other => panic!("not a list of threads: {other:?}"),
    }
    // Without a model nothing said is stored to be found; the cut of found messages is
    // tested beside `admit`.
    for found in phone.threads().search("private").await.unwrap() {
        assert_eq!(found.thread_id, mine);
    }

    // The status tells it nothing of this machine, while the owner's is the real one.
    let status = phone.status.borrow().clone();
    assert_eq!(status.model, None);
    assert!(status.plugins.is_empty() && status.mcp.is_empty());
    assert_eq!(status.workspace_dir, PathBuf::new());
    assert_eq!(status.artifacts_dir, PathBuf::new());
    assert_ne!(owner.status.borrow().workspace_dir, PathBuf::new());

    // Closing both chats ends both: the phone hears of its own only.
    owner.threads().close(&other).await.unwrap();
    owner.threads().close(&mine).await.unwrap();
    let ended = |thread_id: &str| Event::Ended {
        thread_id: thread_id.to_owned(),
    };
    let heard = drain(&mut phone);
    assert!(heard.contains(&ended(&mine)));
    assert!(heard.iter().all(|event| thread_of(event) == Some(&mine)));
    let owner_heard = drain(&mut owner);
    assert!(owner_heard.contains(&ended(&mine)) && owner_heard.contains(&ended(&other)));
}

#[tokio::test]
async fn two_bound_clients_each_get_their_own_chat() {
    let core = core("two").await;
    let owner = attach(&core, "owner", Scope::full());
    let first = owner.threads().start(core.folder.clone()).await.unwrap();
    let second = owner.threads().start(core.folder.clone()).await.unwrap();
    let mut one = bound(&core, "one", &first);
    let mut two = bound(&core, "two", &second);

    for (client, own, theirs) in [(&one, &first, &second), (&two, &second, &first)] {
        let listed = client.threads().list().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(&listed[0].id, own);
        assert!(client.threads().messages(own).await.is_ok());
        let refused = client.threads().messages(theirs).await.unwrap_err();
        assert_eq!(refused.to_string(), NO_THREAD);
        // Opening it to be sent its events is refused too.
        let resume = client.threads().resume(theirs, core.folder.clone()).await;
        assert_eq!(resume.unwrap_err().to_string(), NO_THREAD);
    }
    // Each joins its own chat, which the owner keeps open.
    one.threads()
        .resume(&first, core.folder.clone())
        .await
        .unwrap();
    two.threads()
        .resume(&second, core.folder.clone())
        .await
        .unwrap();

    owner.threads().close(&first).await.unwrap();
    owner.threads().close(&second).await.unwrap();
    one.threads().close(&first).await.unwrap();
    two.threads().close(&second).await.unwrap();
    for (client, own) in [(&mut one, &first), (&mut two, &second)] {
        let heard = drain(client);
        assert!(heard.contains(&Event::Ended {
            thread_id: own.clone()
        }));
        assert!(heard.iter().all(|event| thread_of(event) == Some(own)));
    }
}

/// What is about no thread: files, the shell, settings, secrets, plugins, MCP, projects,
/// updates and stopping the core.
fn apart(folder: &Path) -> Vec<Request> {
    let word = || "x".to_owned();
    vec![
        Request::Shutdown,
        Request::InstallUpdate { tag: word() },
        Request::Store(StoreRequest::List {
            order: Default::default(),
            category: None,
            kinds: Vec::new(),
        }),
        Request::Store(StoreRequest::Contents {
            source: word(),
            name: word(),
        }),
        Request::Store(StoreRequest::UpdateAll),
        Request::SystemReport,
        Request::Terminal(TerminalRequest::Open {
            cwd: folder.to_path_buf(),
            cols: 80,
            rows: 24,
        }),
        Request::Plugin(PluginRequest::Open {
            plugin: word(),
            part: word(),
        }),
        Request::Voice(VoiceRequest::Models),
        Request::Settings(SettingsRequest::SetSecret {
            name: word(),
            value: Secret(word()),
        }),
        Request::Settings(SettingsRequest::Mode(word())),
        Request::Settings(SettingsRequest::PluginEnabled {
            name: word(),
            enabled: true,
        }),
        Request::Catalog(CatalogRequest::Secrets),
        Request::Catalog(CatalogRequest::McpServers),
        Request::Catalog(CatalogRequest::McpServerText { name: word() }),
        Request::Project(ProjectRequest::List),
        Request::Project(ProjectRequest::Outputs),
        Request::Project(ProjectRequest::ReadFile {
            path: folder.join("x"),
        }),
        Request::Project(ProjectRequest::WriteFile {
            path: folder.join("x"),
            text: word(),
            seen: None,
        }),
        Request::Command(Command::Settings),
        Request::Command(Command::Migrate { path: None }),
        Request::Command(Command::Model { name: None }),
        Request::Thread(ThreadRequest::New {
            cwd: folder.to_path_buf(),
        }),
        Request::Thread(ThreadRequest::NewTemporary {
            cwd: folder.to_path_buf(),
        }),
        Request::Thread(ThreadRequest::Fork {
            summary: word(),
            cwd: folder.to_path_buf(),
        }),
        Request::Thread(ThreadRequest::Approve {
            id: 1,
            decision: Decision::Once,
        }),
    ]
}

#[tokio::test]
async fn a_client_bound_to_a_chat_reaches_nothing_else_in_the_core() {
    let core = core("areas").await;
    let owner = attach(&core, "owner", Scope::full());
    let mine = owner.threads().start(core.folder.clone()).await.unwrap();

    // What answers with more than the chat it names is about no thread either: the
    // context report, and the diff of the chat's whole project.
    let beyond = [
        Request::Command(Command::Context {
            thread_id: Some(mine.clone()),
        }),
        Request::Work(WorkRequest::Diff {
            thread_id: mine.clone(),
            path: None,
        }),
    ];
    for request in beyond.into_iter().chain(apart(&core.folder)) {
        let shown = format!("{request:?}");
        let refused = refused_alone(&core, &mine, request).await;
        assert_eq!(refused, Response::Error(REFUSED.to_owned()), "{shown}");
    }
    assert!(!core.folder.join("x").exists());
    // Asked to stop, the core still runs.
    assert_eq!(owner.threads().list().await.unwrap().len(), 1);

    // A scope without the area is refused its own chat in the same words.
    let reader = attach(
        &core,
        "reader",
        Scope::threads([mine.clone()], &[Area::ThreadRead]),
    );
    assert!(reader.threads().messages(&mine).await.is_ok());
    let send = reader
        .threads()
        .send(&mine, "hello".to_owned(), false)
        .await;
    assert_eq!(send.unwrap_err().to_string(), REFUSED);
    let add = Request::Command(Command::Sources {
        thread_id: Some(mine.clone()),
        add: Some("/etc/hosts".to_owned()),
    });
    assert_eq!(
        reader.request(add).await,
        Response::Error(REFUSED.to_owned())
    );
    let delete = reader.threads().delete(&mine).await;
    assert_eq!(delete.unwrap_err().to_string(), REFUSED);
    assert_eq!(owner.threads().list().await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_browser_is_refused_in_the_core_and_the_core_runs_on() {
    let core = core("web").await;
    let owner = attach(&core, "owner", Scope::full());
    let web = attach(&core, "web", Scope::web());

    for request in [
        Request::Shutdown,
        Request::Store(StoreRequest::UpdateAll),
        Request::Terminal(TerminalRequest::Open {
            cwd: core.folder.clone(),
            cols: 80,
            rows: 24,
        }),
        Request::Project(ProjectRequest::WriteFile {
            path: core.folder.join("x"),
            text: String::new(),
            seen: None,
        }),
    ] {
        assert_eq!(
            web.request(request).await,
            Response::Error(REFUSED.to_owned())
        );
    }
    assert!(!core.folder.join("x").exists());

    // It works as before in what it may do, and gets the real status.
    let thread = web.threads().start(core.folder.clone()).await.unwrap();
    assert_eq!(owner.threads().list().await.unwrap()[0].id, thread);
    assert_ne!(web.status.borrow().workspace_dir, PathBuf::new());

    // The full scope may stop the core.
    owner.shutdown().await;
    assert!(web.threads().list().await.is_err());
}
