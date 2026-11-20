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
        tier: Some(dusk_protocol::plugin::Tier::Verified),
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

    // Pointed elsewhere by the user since: the server is theirs now, the store forgot
    // it, and the Workshop's Remove leaves it.
    runtime
        .add_connector("tracker", server(HERE), added(HERE))
        .unwrap();
    let theirs = "tracker:\n  url: http://127.0.0.1:9/theirs\n";
    runtime.save_mcp_server(Some("tracker"), theirs).unwrap();
    assert!(!runtime.remove_connector("tracker").unwrap());
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

#[tokio::test]
async fn a_server_the_user_deleted_and_set_up_again_is_theirs() {
    let runtime = runtime("connector_stale").await;
    runtime
        .add_connector("tracker", server(HERE), added(HERE))
        .unwrap();
    let shown = runtime.mcp_servers();
    let from = shown[0].added_from.as_ref().expect("its origin");
    assert_eq!(from.label(), "From official \u{b7} Verified");

    // Deleted in the settings, then set up by hand at the same address.
    runtime.save_mcp_server(Some("tracker"), "").unwrap();
    assert!(
        connector::added(runtime.paths(), "tracker")
            .unwrap()
            .is_none()
    );
    let mine = format!("tracker:\n  url: {HERE}\n");
    runtime.save_mcp_server(None, &mine).unwrap();
    assert!(runtime.mcp_servers()[0].added_from.is_none());
    // The Workshop's Remove does not touch it, and no source takes it over.
    assert!(!runtime.remove_connector("tracker").unwrap());
    assert!(runtime.config().current().mcp().contains_key("tracker"));
    assert!(
        runtime
            .add_connector("tracker", server(HERE), added(HERE))
            .is_err()
    );
}

#[tokio::test]
async fn another_source_never_replaces_the_server_one_source_added() {
    let runtime = runtime("connector_other_source").await;
    runtime
        .add_connector("tracker", server(HERE), added(HERE))
        .unwrap();
    let mut other = added(HERE);
    other.source = "friends".into();
    other.url = "https://friends.example.com/index.json".into();
    let error = runtime
        .add_connector("tracker", server(HERE), other)
        .unwrap_err();
    assert!(format!("{error:#}").contains("of yours"), "{error:#}");
    let record = connector::added(runtime.paths(), "tracker")
        .unwrap()
        .unwrap();
    assert_eq!(record.source, "official");
}

#[tokio::test]
async fn renaming_or_moving_it_in_the_settings_makes_it_the_users() {
    let runtime = runtime("connector_renamed").await;
    runtime
        .add_connector("tracker", server(HERE), added(HERE))
        .unwrap();
    // Its key filled in, same address: still the store's.
    fill_in(&runtime, "tracker", HERE);
    assert!(
        connector::added(runtime.paths(), "tracker")
            .unwrap()
            .is_some()
    );
    // Pointed elsewhere: the user's.
    fill_in(&runtime, "tracker", MOVED);
    assert!(
        connector::added(runtime.paths(), "tracker")
            .unwrap()
            .is_none()
    );

    runtime.save_mcp_server(Some("tracker"), "").unwrap();
    runtime
        .add_connector("tracker", server(HERE), added(HERE))
        .unwrap();
    let renamed = format!("issues:\n  url: {HERE}\n");
    runtime.save_mcp_server(Some("tracker"), &renamed).unwrap();
    assert!(
        connector::added(runtime.paths(), "tracker")
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn a_connector_never_takes_the_name_of_a_plugins_server() {
    let runtime = runtime("connector_plugin_server").await;
    let folder = crate::plugins::catalog::folder(runtime.paths()).join("bot");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("plugin.yml"),
        "name: bot\nversion: 1.0.0\nmcp:\n  tracker:\n    url: https://mcp.example.com/mcp\n",
    )
    .unwrap();
    let error = runtime
        .add_connector("tracker", server(HERE), added(HERE))
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("a plugin here brings"),
        "{error:#}"
    );
    assert!(runtime.config().current().mcp().get("tracker").is_none());
}

/// What the listing marks unfit is refused again when the install comes: a client may
/// send it without looking.
#[tokio::test]
async fn a_near_name_from_another_source_is_refused_when_it_is_added() {
    let runtime = runtime("connector_near_name").await;
    runtime
        .save_mcp_server(None, "linear:\n  url: http://127.0.0.1:9/mine\n")
        .unwrap();
    let mut from_mirror = added(HERE);
    from_mirror.source = "friends".into();
    let error = runtime
        .add_connector("linnear", server(HERE), from_mirror)
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("too close to the MCP server linear"),
        "{error:#}"
    );
    assert!(runtime.config().current().mcp().get("linnear").is_none());
    // The official list may name its own servers as it likes.
    runtime
        .add_connector("linnear", server(HERE), added(HERE))
        .unwrap();
}
