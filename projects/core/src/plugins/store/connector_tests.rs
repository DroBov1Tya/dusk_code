//! Connector listings: read from a list beside kinds this duskcode does not know, held to
//! a public address or a pinned package, kept from an Unverified source only when they
//! run nothing here, and added as the settings would add the same server.

use dusk_protocol::plugin::{ConnectorState, Kind, Runs, Tier};

use super::{
    Agreed, connector, guard,
    index::{self, Index},
    listing::{self, Here},
    pick,
    record::{Installed, Record},
    source::{Fetched, settle},
};
use crate::config::{McpServerConfig, Mirror, mcp_server_yaml, parse_mcp_server};

fn now() -> chrono::DateTime<chrono::Utc> {
    "2026-11-19T00:00:00Z".parse().unwrap()
}

const CREDIT: &str = r#""author": "Linear", "license": "LicenseRef-vendor-terms", "source": "https://linear.app/docs/mcp", "categories": ["productivity"], "badge": "verified""#;

fn list(format: u32, entries: &[String]) -> String {
    format!(
        r#"{{"format": {format}, "serial": 1, "expires": "2030-01-01T00:00:00Z", "plugins": [{}]}}"#,
        entries.join(",")
    )
}

fn remote(name: &str, url: &str, auth: &str) -> String {
    format!(
        r#"{{"name": "{name}", "version": "1.0.0", "kind": "connector", "description": "Issues.", "server": {{"url": "{url}", "auth": "{auth}", "docs": "https://linear.app/docs/mcp", "run_by": "vendor", "reaches": "the issues and projects of your workspace"}}, {CREDIT}}}"#
    )
}

fn local(name: &str, args: &str) -> String {
    format!(
        r#"{{"name": "{name}", "version": "1.0.0", "kind": "connector", "server": {{"command": "npx", "args": {args}, "run_by": "vendor", "reaches": "a browser this computer starts"}}, {CREDIT}}}"#
    )
}

fn parse(json: &str) -> anyhow::Result<Index> {
    index::parse(json.as_bytes(), now())
}

fn official(index: Index) -> Vec<Fetched> {
    settle(
        Some(Ok(("https://o.example.com/index.json".into(), index))),
        Vec::new(),
    )
    .0
}

#[test]
fn a_connector_is_read_beside_kinds_this_duskcode_does_not_know() {
    let gadget =
        r#"{"name": "gadget", "version": "1.0.0", "kind": "gadget", "whatever": {"x": 1}}"#;
    let index = parse(&list(
        3,
        &[
            remote("linear", "https://mcp.linear.app/mcp", "oauth"),
            gadget.to_owned(),
        ],
    ))
    .unwrap();
    assert_eq!(index.plugins.len(), 1);
    assert_eq!(index.plugins[0].kind, Kind::Connector);
    // Format 2, as the list is published today, reads it the same without credit.
    let plain =
        remote("linear", "https://mcp.linear.app/mcp", "oauth").replace(&format!(", {CREDIT}"), "");
    assert_eq!(parse(&list(2, &[plain])).unwrap().plugins.len(), 1);
    // An older duskcode never saw `connector`: it leaves such an entry out the way this
    // one leaves out `gadget`, and reads the rest of the list.
}

#[test]
fn a_connector_is_held_to_a_public_address_or_a_pinned_package() {
    let refused = [
        remote("a", "http://mcp.example.com/mcp", "none"),
        remote("a", "https://localhost/mcp", "none"),
        remote("a", "https://127.0.0.1/mcp", "none"),
        remote("a", "https://10.0.0.2/mcp", "none"),
        remote("a", "https://intranet/mcp", "none"),
        remote("a", "https://box.local/mcp", "none"),
        remote("a", "https://user:pw@mcp.example.com/mcp", "none"),
        // A key with no secret named for the user to fill in.
        remote("a", "https://mcp.example.com/mcp", "key"),
        local("a", r#"["-y", "@playwright/mcp@latest"]"#),
        local("a", r#"["-y", "@playwright/mcp"]"#),
        local("a", r#"["-y", "@playwright/mcp@^1.2.0"]"#),
        local("a", r#"["@playwright/mcp@0.0.86"]"#),
        local("a", r#"["--registry", "https://evil.example.com", "x@1.0.0"]"#),
        local("a", r#"["-y", "x@1.0.0"]"#).replace(r#""npx""#, r#""bash""#),
        // Signing in is for a remote server.
        local("a", r#"["-y", "x@1.0.0"]"#).replace(r#""run_by""#, r#""auth": "oauth", "run_by""#),
        // No package to download, and nothing a package brings.
        remote("a", "https://mcp.example.com/mcp", "none").replace(
            r#""kind": "connector","#,
            r#""kind": "connector", "downloads": [{"os": "any", "arch": "any", "url": "https://e.com/p.tar.gz", "sha256": "0000000000000000000000000000000000000000000000000000000000000000", "size": 1}],"#,
        ),
        remote("a_b", "https://mcp.example.com/mcp", "none"),
    ];
    for entry in refused {
        assert!(
            parse(&list(3, std::slice::from_ref(&entry))).is_err(),
            "{entry}"
        );
    }
    let key = remote("github", "https://api.githubcopilot.com/mcp/", "key")
        .replace(r#""run_by""#, r#""secrets": ["Authorization"], "run_by""#);
    let pinned = local(
        "playwright",
        r#"["-y", "@playwright/mcp@0.0.86", "--headless"]"#,
    );
    let uvx = local("fetch", r#"["mcp-server-fetch==2026.11.18"]"#).replace("npx", "uvx");
    for entry in [key, pinned, uvx] {
        parse(&list(3, std::slice::from_ref(&entry)))
            .unwrap_or_else(|error| panic!("{entry}: {error:#}"));
    }
    // A server field on anything else is refused.
    let skill = remote("a", "https://mcp.example.com/mcp", "none").replace("connector", "skill");
    assert!(parse(&list(3, &[skill])).is_err());
}

#[test]
fn an_unverified_source_keeps_only_remote_connectors() {
    let mut index = parse(&list(
        3,
        &[
            remote("issues", "https://mcp.example.com/mcp", "oauth"),
            local("browser", r#"["-y", "@playwright/mcp@0.0.86"]"#),
        ],
    ))
    .unwrap();
    guard::admit(&mut index);
    let names: Vec<&str> = index.plugins.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["issues"]);
    assert!(
        index
            .left_out
            .as_deref()
            .is_some_and(|note| note.contains("1 local connector ignored")),
        "{:?}",
        index.left_out
    );
    // Added without a review: there are no files, and the address is in what the user
    // agreed to.
    let mirror = Mirror {
        name: "loose".into(),
        url: "https://loose.example.com/index.json".into(),
        key: String::new(),
    };
    let (fetched, _) = settle(None, vec![(mirror, Ok(index))]);
    let consent = fetched[0].index.plugins[0].consent();
    let agreed = Agreed {
        source: "loose",
        tier: Tier::Unverified,
        name: "issues",
        version: "1.0.0",
        permissions: &consent,
        sha256: None,
    };
    assert!(pick(&fetched, &agreed).is_ok());
}

#[test]
fn a_program_comes_only_from_an_official_or_verified_source() {
    let index = parse(&list(
        3,
        &[local("browser", r#"["-y", "@playwright/mcp@0.0.86"]"#)],
    ))
    .unwrap();
    let server = index.plugins[0].server.clone().unwrap();
    for tier in [Tier::Community, Tier::Unverified, Tier::Local] {
        let why = server.unfit(tier).unwrap();
        assert!(why.contains("official or verified"), "{tier:?}: {why}");
    }
    // From the official list it is refused only when npx is missing here.
    if let Some(why) = server.unfit(Tier::Verified) {
        assert!(why.contains("needs npx"), "{why}");
    }
    assert_eq!(
        server.package().as_deref(),
        Some("@playwright/mcp@0.0.86 from npm")
    );
}

#[test]
fn its_listing_says_what_it_reaches_and_whether_it_is_here() {
    let fetched = official(
        parse(&list(
            3,
            &[remote("linear", "https://mcp.linear.app/mcp", "oauth")],
        ))
        .unwrap(),
    );
    let entry = &fetched[0].index.plugins[0];
    let paths = crate::testing::temp_paths("store-connector-listing");
    let shown = |record: &Record, yours: &[(&str, &str)]| {
        let here = Here {
            yours: yours
                .iter()
                .map(|(name, target)| ((*name).to_owned(), (*target).to_owned()))
                .collect(),
            on: Default::default(),
        };
        listing::listing(&paths, record, &here, &fetched[0], entry)
    };
    let fresh = shown(&Record::default(), &[]);
    assert_eq!(fresh.kind, Kind::Connector);
    assert_eq!(fresh.tier, Tier::Verified);
    assert_eq!(
        fresh.connectors[0].runs,
        Runs::Url("https://mcp.linear.app/mcp".into())
    );
    assert_eq!(fresh.connectors[0].state, ConnectorState::NotAdded);
    assert_eq!(fresh.unfit, None);
    assert!(fresh.reach().iter().any(|line| line.contains("sign in")));
    assert!(fresh.server.is_some());

    // A server of the user's under the name is theirs, and keeps it from being added.
    let yours = shown(
        &Record::default(),
        &[("linear", "https://elsewhere.example.com")],
    );
    assert_eq!(yours.connectors[0].state, ConnectorState::Yours);
    assert!(yours.unfit.is_some());

    // Added by the store from this source, with the address it added.
    let mut added = Installed::new(&fetched[0], "1.0.0", "");
    added.kind = Kind::Connector;
    added.target = "https://mcp.linear.app/mcp".into();
    let mut record = Record::default();
    record.connectors.insert("linear".into(), added);
    let here = shown(&record, &[("linear", "https://mcp.linear.app/mcp")]);
    assert_eq!(here.connectors[0].state, ConnectorState::Added);
    assert_eq!(here.installed.as_deref(), Some("1.0.0"));
    // Changed by the user since: theirs now.
    let changed = shown(
        &record,
        &[("linear", "https://mcp.linear.app/mcp/readonly")],
    );
    assert_eq!(changed.connectors[0].state, ConnectorState::Yours);
}

/// The server a connector adds is the one the settings write for the same YAML, so both
/// go into the config by `Runtime::put_mcp_server` alike.
#[test]
fn it_adds_the_server_the_settings_would() {
    let key = remote("github", "https://api.githubcopilot.com/mcp/", "key")
        .replace(r#""run_by""#, r#""secrets": ["Authorization"], "run_by""#);
    let pinned = local("playwright", r#"["-y", "@playwright/mcp@0.0.86"]"#);
    let index = parse(&list(3, &[key, pinned])).unwrap();
    let typed = [
        "github:\n  url: https://api.githubcopilot.com/mcp/\n  enabled: false\n  headers:\n    Authorization: ''\n",
        "playwright:\n  command: npx\n  args:\n  - -y\n  - '@playwright/mcp@0.0.86'\n",
    ];
    for (entry, typed) in index.plugins.iter().zip(typed) {
        let config = entry.server.as_ref().unwrap().config();
        let (name, by_hand) = parse_mcp_server(typed).unwrap();
        assert_eq!(name, entry.name);
        assert_eq!(
            mcp_server_yaml(&entry.name, &config).unwrap(),
            mcp_server_yaml(&name, &by_hand).unwrap()
        );
    }
}

#[test]
fn adding_it_again_keeps_what_the_user_filled_in() {
    let index = parse(&list(
        3,
        &[
            remote("github", "https://api.githubcopilot.com/mcp/", "key")
                .replace(r#""run_by""#, r#""secrets": ["Authorization"], "run_by""#),
        ],
    ))
    .unwrap();
    let spec = index.plugins[0].server.clone().unwrap();
    let mut old = spec.config();
    assert!(!old.enabled());
    if let McpServerConfig::Http(http) = &mut old {
        http.headers
            .insert("Authorization".into(), "Bearer t".into());
    }
    old.set_enabled(true);
    let mut new = spec.config();
    connector::keep_secrets(&mut new, &old);
    assert!(new.enabled());
    let McpServerConfig::Http(http) = &new else {
        panic!("a remote server");
    };
    assert_eq!(http.headers["Authorization"], "Bearer t");
}
