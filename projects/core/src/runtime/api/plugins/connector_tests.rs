//! Adding and removing a connector on a running core: through the settings' own path,
//! never over a server of the user's, keeping the keys the user typed only while the
//! server still goes where they were typed for, and removing only what is unchanged.

use dusk_protocol::plugin::Kind;

use crate::{
    config::{HttpServerConfig, McpServerConfig},
    init::paths::Paths,
    plugins::store::{Installed, connector},
    runtime::Runtime,
    testing::temp_dir,
};

/// Nothing listens there, so a start fails at once and reaches nobody.
const HERE: &str = "http://127.0.0.1:9/mcp";
const MOVED: &str = "http://127.0.0.1:9/mcp/v2";

async fn runtime(name: &str) -> Runtime {
    let root = temp_dir(name);
    let paths = Paths::new(root.join("config"), root.join("work"));
    Runtime::start(paths, Some(root.join("state.db")))
        .await
        .unwrap()
}

/// A remote connector that asks for a key in `Authorization`, as a listing gives it.
fn server(url: &str) -> McpServerConfig {
    McpServerConfig::Http(HttpServerConfig {
        url: url.into(),
        enabled: false,
        headers: [("Authorization".to_owned(), String::new())].into(),
        icon: None,
    })
}

fn added(url: &str) -> Installed {
    Installed {
        version: "1.0.0".into(),
        sha256: String::new(),
        source: "official".into(),
        url: String::new(),
        key: String::new(),
        kind: Kind::Connector,
        folder: String::new(),
        author: String::new(),
        license: String::new(),
        origin: String::new(),
        target: url.into(),
    }
}

fn header(runtime: &Runtime, name: &str) -> Option<(String, bool)> {
    match runtime.config().current().mcp().get(name)? {
        McpServerConfig::Http(http) => {
            Some((http.headers.get("Authorization")?.clone(), http.enabled))
        }
        McpServerConfig::Stdio(_) => None,
    }
}

/// The user typing their key into the server's settings and switching it on.
fn fill_in(runtime: &Runtime, name: &str, url: &str) {
    let text = format!("{name}:\n  url: {url}\n  headers:\n    Authorization: Bearer typed\n");
    runtime.save_mcp_server(Some(name), &text).unwrap();
}

#[tokio::test]
async fn a_server_of_the_users_under_the_name_is_never_replaced() {
    let runtime = runtime("connector_yours").await;
    let mine = "tracker:\n  url: http://127.0.0.1:9/mine\n";
    runtime.save_mcp_server(None, mine).unwrap();
    let error = runtime
        .add_connector("tracker", server(HERE), added(HERE))
        .unwrap_err();
    assert!(format!("{error:#}").contains("of yours"), "{error:#}");
    let config = runtime.config().current();
    assert_eq!(config.mcp()["tracker"].target(), "http://127.0.0.1:9/mine");
    assert!(
        connector::added(runtime.paths(), "tracker")
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn adding_again_keeps_the_typed_key_only_for_the_same_address() {
    let runtime = runtime("connector_readd").await;
    runtime
        .add_connector("tracker", server(HERE), added(HERE))
        .unwrap();
    // Written off with the key's field empty, and remembered as the store's.
    assert_eq!(header(&runtime, "tracker"), Some((String::new(), false)));
    let record = connector::added(runtime.paths(), "tracker")
        .unwrap()
        .unwrap();
    assert_eq!(record.target, HERE);

    fill_in(&runtime, "tracker", HERE);
    runtime
        .add_connector("tracker", server(HERE), added(HERE))
        .unwrap();
    assert_eq!(
        header(&runtime, "tracker"),
        Some(("Bearer typed".into(), true))
    );

    // The listing moved the server: the key typed for the old address does not follow.
    runtime
        .add_connector("tracker", server(MOVED), added(MOVED))
        .unwrap();
    assert_eq!(header(&runtime, "tracker"), Some((String::new(), false)));
    let config = runtime.config().current();
    assert_eq!(config.mcp()["tracker"].target(), MOVED);
}

#[tokio::test]
async fn removing_takes_out_only_the_server_the_store_added_unchanged() {
    let runtime = runtime("connector_remove").await;
    assert!(!runtime.remove_connector("tracker").unwrap());

    runtime
        .add_connector("tracker", server(HERE), added(HERE))
        .unwrap();
    assert!(runtime.remove_connector("tracker").unwrap());
    assert!(runtime.config().current().mcp().get("tracker").is_none());
    assert!(
        connector::added(runtime.paths(), "tracker")
            .unwrap()
            .is_none()
    );

    // Pointed elsewhere by the user since: the server is theirs now and stays.
    runtime
        .add_connector("tracker", server(HERE), added(HERE))
        .unwrap();
    let theirs = "tracker:\n  url: http://127.0.0.1:9/theirs\n";
    runtime.save_mcp_server(Some("tracker"), theirs).unwrap();
    assert!(runtime.remove_connector("tracker").unwrap());
    let config = runtime.config().current();
    assert_eq!(
        config.mcp()["tracker"].target(),
        "http://127.0.0.1:9/theirs"
    );
    assert!(
        connector::added(runtime.paths(), "tracker")
            .unwrap()
            .is_none()
    );
}
