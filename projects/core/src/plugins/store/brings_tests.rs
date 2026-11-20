//! What a plugin brings, MCP servers and programs: listed, agreed to, checked against its
//! manifest, and never put over a connector the user configured.

use dusk_protocol::plugin::{ConnectorState, Runs, Tier};

use super::{
    Agreed, Here, index, listing, pick, place,
    record::Record,
    source::settle,
    tests::{SHA, package, signed},
};
use crate::config::{Config, McpServerConfig, Mirror, Plugged, StdioServerConfig};

const CREDIT: &str = r#", "author": "someone", "license": "MIT", "source": "https://github.com/someone/bot", "categories": ["coding"], "badge": "verified""#;
const BRINGS: &str = r#", "mcp": [{"name": "search", "command": "./search-server", "args": ["--fast"]}], "programs": ["./bin/bot"]"#;

fn list(format: u32, extra: &str) -> String {
    format!(
        r#"{{"format": {format}, "serial": 1, "expires": "2030-01-01T00:00:00Z", "plugins": [{{"name": "bot", "version": "1.0.0", "downloads": [{{"url": "https://example.com/b.tar.gz", "sha256": "{SHA}", "size": 10}}]{extra}}}]}}"#
    )
}

fn read(json: &str) -> anyhow::Result<(index::Index, String)> {
    let (key, signature) = signed(json);
    let now = "2026-11-14T00:00:00Z".parse().unwrap();
    index::verify(json.as_bytes(), &signature, &key, now).map(|index| (index, key))
}

fn plugin(manifest: &str) -> Vec<u8> {
    package(&[("plugin.yml", tar::EntryType::Regular, manifest.as_bytes())])
}

const MANIFEST: &str = "name: bot\nversion: 1.0.0\nmcp:\n  search:\n    command: ./search-server\n    args: [--fast]\nplatform:\n  command: [./bin/bot]\n";

#[test]
fn nothing_a_plugin_brings_installs_until_it_is_agreed_to() {
    let (index, key) = read(&list(3, &format!("{CREDIT}{BRINGS}"))).unwrap();
    let consent = index.plugins[0].consent();
    assert!(consent[0].contains("./bin/bot"), "{consent:?}");
    assert!(consent[1].contains("search") && consent[1].contains("--fast"));

    let mirror = Mirror {
        name: "friends".into(),
        url: "https://friends.example.com/index.json".into(),
        key,
    };
    let (fetched, _) = settle(None, vec![(mirror, Ok(index))]);
    let mut agreed = Agreed {
        source: "friends",
        tier: Tier::Community,
        name: "bot",
        version: "1.0.0",
        permissions: &consent[2..],
        sha256: None,
    };
    let error = pick(&fetched, &agreed).unwrap_err();
    assert!(format!("{error:#}").contains("changed"), "{error:#}");
    agreed.permissions = &consent;
    let (_, entry) = pick(&fetched, &agreed).unwrap();

    let root = crate::testing::temp_dir("store-brings");
    std::fs::create_dir_all(&root).unwrap();
    for (why, manifest) in [
        (
            "a server it does not list",
            format!("{MANIFEST}  more:\n    command: ./x\n"),
        ),
        ("another command", MANIFEST.replace("[--fast]", "[--slow]")),
        (
            "no program",
            MANIFEST.replace("platform:\n  command: [./bin/bot]\n", ""),
        ),
    ] {
        let error = place(&root, entry, &plugin(&manifest), false).unwrap_err();
        assert!(!root.join("bot").exists(), "{why}: {error:#}");
    }
    place(&root, entry, &plugin(MANIFEST), false).unwrap();

    // A list before format 3 says nothing of servers; such a plugin installs as before.
    let (old, _) = read(&list(2, "")).unwrap();
    assert!(old.plugins[0].consent().is_empty());
    assert!(
        read(&list(2, BRINGS)).is_err(),
        "format 3 fields in format 2"
    );
}

#[test]
fn a_users_connector_is_never_replaced_and_a_plugins_goes_with_it() {
    let yours = "mcp:\n  search:\n    command: my-search\n";
    let mut config = Config::parse(yours.as_bytes()).unwrap();
    let theirs = |command: &str| {
        McpServerConfig::Stdio(Box::new(StdioServerConfig {
            command: command.into(),
            enabled: true,
            args: Vec::new(),
            env: Default::default(),
            icon: None,
            isolation: None,
        }))
    };
    let mut plugged = Plugged::default();
    plugged
        .mcp
        .insert("search".into(), theirs("./search-server"));
    plugged.mcp.insert("notes".into(), theirs("./notes-server"));
    config.set_plugged(plugged);
    let all = config.all_mcp();
    let command = |name: &str| match &all[name] {
        McpServerConfig::Stdio(stdio) => stdio.command.clone(),
        McpServerConfig::Http(http) => http.url.clone(),
    };
    assert_eq!(command("search"), "my-search");
    assert_eq!(command("notes"), "./notes-server");
    // Switched off or removed, the plugin brings nothing; the user's stays.
    config.set_plugged(Plugged::default());
    assert!(!config.all_mcp().contains_key("notes"));
    assert!(config.all_mcp().contains_key("search"));

    // The listing says so.
    let (index, _) = read(&list(3, &format!("{CREDIT}{BRINGS}"))).unwrap();
    let (fetched, _) = settle(
        Some(Ok(("https://o.example.com/index.json".into(), index))),
        Vec::new(),
    );
    let paths = crate::testing::temp_paths("store-brings-here");
    let here = Here {
        yours: [("search".to_owned(), "search-server".to_owned())].into(),
        on: ["bot".to_owned()].into(),
    };
    let shown = listing::listing(
        &paths,
        &Record::default(),
        &here,
        &fetched[0],
        &fetched[0].index.plugins[0],
    );
    assert_eq!(shown.connectors[0].state, ConnectorState::Yours);
    assert_eq!(
        shown.connectors[0].runs,
        Runs::Command("./search-server --fast".into())
    );
    assert_eq!(shown.reach(), &shown.permissions[2..]);
}
