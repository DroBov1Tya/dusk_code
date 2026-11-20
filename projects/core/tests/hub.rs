//! The hub against a real core: who is attached, when it is busy and how it stops.

use std::{path::PathBuf, sync::Arc, time::Duration};

use dusk_core::{
    app::{LogRole, Paths, init_logs, start_hub},
    client::{Client, Hub, Scope},
};
use dusk_protocol::client::{Busy, Event, Hello, Request, Response};

const REFUSED: &str = "this client may not send that request";

async fn core(name: &str) -> (Arc<Hub>, PathBuf) {
    let root = std::env::temp_dir().join(format!("dusk-hub-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let paths = Paths::new(root.join("config"), root.join("work"));
    let hub = start_hub(paths, Some(root.join("state.db"))).await.unwrap();
    (hub, root)
}

fn owner(hub: &Arc<Hub>, name: &str) -> Client {
    hub.attach_owner(Hello::new(name), Scope::full()).unwrap()
}

fn attach(hub: &Arc<Hub>, name: &str, scope: Scope) -> Client {
    hub.attach(Hello::new(name), scope).unwrap()
}

async fn next(client: &mut Client) -> Option<Event> {
    tokio::time::timeout(Duration::from_secs(5), client.events.recv())
        .await
        .expect("an event within 5 s")
}

async fn busy_is(hub: &Hub, want: Busy) {
    let mut busy = hub.busy();
    tokio::time::timeout(Duration::from_secs(5), busy.wait_for(|now| *now == want))
        .await
        .expect("the busy count within 5 s")
        .unwrap();
}

#[tokio::test]
async fn busy_counts_other_clients_and_running_turns() {
    let (hub, _) = core("busy").await;
    let _owner = owner(&hub, "owner");
    busy_is(
        &hub,
        Busy {
            clients: 0,
            running: 0,
        },
    )
    .await;
    let other = attach(&hub, "phone", Scope::web());
    busy_is(
        &hub,
        Busy {
            clients: 1,
            running: 0,
        },
    )
    .await;
    drop(other);
    busy_is(
        &hub,
        Busy {
            clients: 0,
            running: 0,
        },
    )
    .await;
}

/// The owner is the one attached as such, not whoever comes first, and the core has none
/// once it goes, whether it left or was disconnected.
#[tokio::test]
async fn the_owner_is_explicit_and_cleared_when_it_goes() {
    let (hub, _) = core("owner").await;
    let first = attach(&hub, "web", Scope::web());
    busy_is(
        &hub,
        Busy {
            clients: 1,
            running: 0,
        },
    )
    .await;
    let app = owner(&hub, "app");
    busy_is(
        &hub,
        Busy {
            clients: 1,
            running: 0,
        },
    )
    .await;
    let other = attach(&hub, "phone", Scope::web());
    busy_is(
        &hub,
        Busy {
            clients: 2,
            running: 0,
        },
    )
    .await;
    drop(app);
    busy_is(
        &hub,
        Busy {
            clients: 2,
            running: 0,
        },
    )
    .await;
    // A second app attached as the owner takes the place and is disconnected by a
    // full client: nothing is left uncounted afterwards.
    let again = owner(&hub, "app2");
    busy_is(
        &hub,
        Busy {
            clients: 2,
            running: 0,
        },
    )
    .await;
    let admin = attach(&hub, "admin", Scope::full());
    let Response::Clients(listed) = admin.request(Request::Clients).await else {
        panic!("a list of clients");
    };
    let id = listed.iter().find(|c| c.label == "app2").unwrap().id;
    assert_eq!(
        admin.request(Request::Disconnect { id }).await,
        Response::Done
    );
    busy_is(
        &hub,
        Busy {
            clients: 3,
            running: 0,
        },
    )
    .await;
    let _ = (first, other, again);
}

#[tokio::test]
async fn stop_sends_core_stopping_to_every_client_first() {
    let (hub, _) = core("stop").await;
    let mut owner = attach(&hub, "owner", Scope::full());
    let mut web = attach(&hub, "phone", Scope::web());
    let mut bound = attach(&hub, "bot", Scope::threads(["a".to_owned()], &[]));
    hub.stop("update");
    assert_eq!(
        next(&mut owner).await,
        Some(Event::CoreStopping {
            reason: "update".to_owned()
        })
    );
    // Clients that are not the user's own never read why.
    for client in [&mut web, &mut bound] {
        assert_eq!(
            next(client).await,
            Some(Event::CoreStopping {
                reason: "the core is stopping".to_owned()
            })
        );
    }
}

#[tokio::test]
async fn shutdown_from_a_full_client_names_it() {
    let (hub, _) = core("shutdown").await;
    let mut owner = attach(&hub, "owner", Scope::full());
    let mut web = attach(&hub, "phone", Scope::web());
    assert_eq!(
        web.request(Request::Shutdown).await,
        Response::Error(REFUSED.to_owned())
    );
    assert_eq!(owner.request(Request::Shutdown).await, Response::Done);
    assert_eq!(
        next(&mut owner).await,
        Some(Event::CoreStopping {
            reason: "stopped by owner".to_owned()
        })
    );
    assert_eq!(
        next(&mut web).await,
        Some(Event::CoreStopping {
            reason: "the core is stopping".to_owned()
        })
    );
}

#[tokio::test]
async fn clients_lists_labels_and_is_full_only() {
    let (hub, _) = core("list").await;
    let owner = attach(&hub, "owner", Scope::full());
    let web = attach(&hub, "phone", Scope::web());
    let Response::Clients(listed) = owner.request(Request::Clients).await else {
        panic!("a list of clients");
    };
    let labels: Vec<_> = listed
        .iter()
        .map(|client| {
            (
                client.label.as_str(),
                client.scope.as_str(),
                client.transport.as_str(),
            )
        })
        .collect();
    assert_eq!(
        labels,
        [
            ("owner", "full", "in-process"),
            ("phone", "web", "in-process")
        ]
    );
    assert!(listed[0].id < listed[1].id && listed[0].since > 0);
    assert_eq!(
        web.request(Request::Clients).await,
        Response::Error(REFUSED.to_owned())
    );
    assert_eq!(
        web.request(Request::Disconnect { id: listed[0].id }).await,
        Response::Error(REFUSED.to_owned())
    );
}

#[tokio::test]
async fn disconnect_closes_that_clients_events() {
    let (hub, _) = core("disconnect").await;
    let owner = attach(&hub, "owner", Scope::full());
    let mut web = attach(&hub, "phone", Scope::web());
    let Response::Clients(listed) = owner.request(Request::Clients).await else {
        panic!("a list of clients");
    };
    let id = listed.iter().find(|c| c.label == "phone").unwrap().id;
    assert_eq!(
        owner.request(Request::Disconnect { id }).await,
        Response::Done
    );
    assert_eq!(next(&mut web).await, None);
    assert!(matches!(
        owner.request(Request::Disconnect { id }).await,
        Response::Error(_)
    ));
}

#[test]
fn an_attached_role_writes_no_core_log() {
    let root = std::env::temp_dir().join(format!("dusk-hub-logs-{}", std::process::id()));
    let paths = Paths::new(root.join("config"), root.join("work"));
    let _telemetry = init_logs(&paths, "hub_test", "attached", LogRole::Attached).unwrap();
    let logs = paths.logs_dir();
    assert!(logs.join("attached.log").exists());
    assert!(!logs.join("core.log").exists());
}
