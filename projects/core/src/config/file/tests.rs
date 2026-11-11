use super::*;
use crate::config::{UNKNOWN_WINDOW, mcp_server_yaml, parse_mcp_server};

#[test]
fn auto_takes_the_model_window() {
    let auto = ContextPolicy::Auto;
    assert_eq!(auto.input_limit(Some(256_000)), 256_000);
    assert_eq!(auto.input_limit(None), UNKNOWN_WINDOW);
    let manual = ContextPolicy::Manual(300_000);
    assert_eq!(manual.input_limit(Some(256_000)), 256_000);
    assert_eq!(manual.input_limit(None), 300_000);
}

const PROVIDERS: &str = "providers:\n  main:\n    kind: openai_compatible\n    base_url: 'https://host/v1'\n    api_key: 'sk-test'\n";

fn parse(yaml: &str) -> Result<Config> {
    Config::parse(yaml.as_bytes())
}

#[test]
fn unknown_fields_and_values_are_rejected() {
    assert!(parse(&format!("{PROVIDERS}session:\n  record: everything\n")).is_err());
    assert!(parse(&format!("{PROVIDERS}tool: {{}}\n")).is_err());
    assert!(parse("providers: {}\n").is_ok(), "offline config");
}

#[test]
fn an_mcp_server_is_edited_as_its_own_yaml() {
    let (name, mut server) =
        parse_mcp_server("docs:\n  command: docs-server\n  args: [--stdio]\n").unwrap();
    assert_eq!((name.as_str(), server.enabled()), ("docs", true));
    server.set_enabled(false);
    let text = mcp_server_yaml(&name, &server).unwrap();
    let (_, again) = parse_mcp_server(&text).unwrap();
    assert!(!again.enabled() && again.target() == "docs-server --stdio");
    assert!(parse_mcp_server("a:\n  command: x\nb:\n  command: y\n").is_err());
    assert!(parse_mcp_server("docs:\n  command: x\n  extra: 1\n").is_err());
}

#[test]
fn traces_go_encrypted_or_stay_on_this_machine() {
    let traces = |endpoint: &str| {
        parse(&format!(
            "logging:\n  traces:\n    endpoint: '{endpoint}'\n"
        ))
    };
    assert!(traces("https://otel.example.com").is_ok());
    assert!(traces("http://127.0.0.1:4318").is_ok());
    for wrong in [
        "http://otel.example.com",
        "https://k:s@otel.example.com",
        "otel",
    ] {
        assert!(traces(wrong).is_err(), "{wrong}");
    }
    let off = parse("logging:\n  enabled: false\n").unwrap();
    assert!(!off.logging().enabled);
}

#[test]
fn removing_a_provider_takes_its_models_policies_with_it() {
    let mut config = parse(
        "providers:\n  a:\n    kind: openai_compatible\n    base_url: 'https://a/v1'\n    api_key: k\n  b:\n    kind: openai_compatible\n    base_url: 'https://b/v1'\n    api_key: k\n",
    )
    .unwrap();
    config.set_context_policy("a", "m", ContextPolicy::Manual(1000));
    config.set_context_policy("ab", "m", ContextPolicy::Manual(2000));
    assert!(config.remove_provider("a"));
    assert!(!config.remove_provider("a"));
    assert_eq!(config.providers().keys().collect::<Vec<_>>(), ["b"]);
    assert_eq!(config.context_policy("a", "m"), ContextPolicy::Auto);
    // A name that only starts the same is another provider.
    assert_eq!(
        config.context_policy("ab", "m"),
        ContextPolicy::Manual(2000)
    );
}

#[test]
fn mcp_servers_are_one_kind_and_reached_safely() {
    let server = |body: &str| parse(&format!("mcp:\n  docs:\n{body}"));
    assert!(server("    command: docs-server\n").is_ok());
    assert!(server("    url: 'https://mcp.example.com/mcp'\n").is_ok());
    assert!(server("    url: 'http://127.0.0.1:9000/mcp'\n").is_ok());
    for wrong in [
        "    command: docs-server\n    url: 'https://mcp.example.com/mcp'\n",
        "    url: 'http://mcp.example.com/mcp'\n",
        "    url: 'https://user:pass@mcp.example.com/mcp'\n",
        "    url: 'https://mcp.example.com/mcp'\n    headers:\n      'bad name': x\n",
    ] {
        assert!(server(wrong).is_err(), "{wrong}");
    }
}

#[test]
fn written_yaml_reads_back_the_same() {
    let mut config = parse(PROVIDERS).unwrap();
    config.set_tool_enabled("run_command", false);
    config.set_branch_prefix("dusk/");
    config.set_notifications(false);
    config.set_sound_notifications(false);
    let yaml = config.to_yaml().unwrap();
    assert!(yaml.starts_with(HEADER));
    let again = parse(&yaml).unwrap();
    assert!(!again.tool_enabled("run_command"));
    assert_eq!(again.providers()["main"].api_key.0, "sk-test");
    assert_eq!(again.version_control().branch_prefix, "dusk/");
    assert!(!again.notifications());
    assert!(!again.sound_notifications());
}

#[test]
fn branch_prefixes_are_bounded_and_safe() {
    assert!(
        parse(&format!(
            "{PROVIDERS}version_control:\n  branch_prefix: 'dusk/'\n"
        ))
        .is_ok()
    );
    assert!(parse("version_control:\n  branch_prefix: ''\n").is_ok());
    for bad in ["a//b", "..", "a b/", "/a", &"x".repeat(65)] {
        assert!(
            parse(&format!("version_control:\n  branch_prefix: '{bad}'\n")).is_err(),
            "{bad:?}"
        );
    }
}

#[test]
fn notifications_default_to_on_and_stay_off_when_set() {
    let plain = parse(PROVIDERS).unwrap();
    assert!(plain.notifications() && plain.sound_notifications());
    let off = parse("notifications: false\n").unwrap();
    assert!(!off.notifications() && off.sound_notifications());
    let silent = parse("sound_notifications: false\n").unwrap();
    assert!(silent.notifications() && !silent.sound_notifications());
}
