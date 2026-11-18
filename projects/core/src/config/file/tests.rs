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
fn a_refused_file_says_what_and_where_without_its_values() {
    let wrong = |yaml: &str| format!("{:#}", parse(yaml).unwrap_err());
    let unknown = wrong(&format!("{PROVIDERS}tool: {{}}\n"));
    assert!(unknown.contains("unknown field `tool`"), "{unknown}");
    assert!(unknown.contains("line 6"), "{unknown}");
    // A secret accidentally entered as a key is not a safe field name either.
    for yaml in [
        "sk-secret-key: value\n",
        "thread:\n  sk-secret-key: value\n",
        "'sk-secret-key\\nother': value\n",
    ] {
        let error = wrong(yaml);
        assert!(
            error.contains("unknown field") && error.contains("line"),
            "{error}"
        );
        assert!(
            !error.contains("sk-secret-key") && !error.contains("value"),
            "{error}"
        );
    }
    // A value of the wrong type may be a secret: it is never repeated.
    let secret = wrong(
        "providers:\n  main:\n    kind: openai_compatible\n    base_url: 'https://host/v1'\n    api_key: [sk-secret]\n",
    );
    assert!(secret.contains("line 5"), "{secret}");
    assert!(!secret.contains("sk-secret"), "{secret}");
    let broken = wrong("providers: [sk-secret\n");
    assert!(
        broken.contains("line") && !broken.contains("sk-secret"),
        "{broken}"
    );
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

#[test]
fn the_model_timeout_and_attempts_stay_inside_their_bounds() {
    let plain = parse(PROVIDERS).unwrap();
    assert_eq!(plain.limits().model_timeout_seconds, 120);
    assert_eq!(plain.limits().max_attempts, 6);
    assert_eq!(plain.limits().retry_unreachable_seconds, 300);
    assert_eq!(plain.limits().resume_within_seconds, 3600);
    assert!(plain.limits().wait_for_provider);
    assert!(!plain.limits().shrink_context);
    assert!(!plain.limits().read_many_files && plain.limits().parallel_reads);
    assert!(!plain.limits().stable_prefix);
    assert!(plain.limits().record_changes);
    let set = parse(
        "limits:\n  model_timeout_seconds: 300\n  max_attempts: 3\n  retry_unreachable_seconds: 60\n  wait_for_provider: false\n  shrink_context: true\n  stable_prefix: true\n  record_changes: false\n",
    )
    .unwrap();
    assert!(!set.limits().record_changes);
    assert!(!set.limits().wait_for_provider);
    assert!(set.limits().shrink_context);
    let reads = parse("limits:\n  read_many_files: true\n  parallel_reads: false\n").unwrap();
    assert!(reads.limits().read_many_files && !reads.limits().parallel_reads);
    assert!(set.limits().stable_prefix);
    assert_eq!(set.limits().model_timeout_seconds, 300);
    assert_eq!(set.limits().max_attempts, 3);
    assert_eq!(set.limits().retry_unreachable_seconds, 60);
    for bad in [
        "limits:\n  model_timeout_seconds: 4\n",
        "limits:\n  model_timeout_seconds: 3601\n",
        "limits:\n  max_attempts: 0\n",
        "limits:\n  max_attempts: 21\n",
        "limits:\n  retry_unreachable_seconds: 9\n",
        "limits:\n  retry_unreachable_seconds: 3601\n",
        "limits:\n  resume_within_seconds: 59\n",
        "limits:\n  resume_within_seconds: 604801\n",
        "limits:\n  wait_for_provider: 3\n",
        "limits:\n  shrink_context: often\n",
        "limits:\n  read_many_files: 1\n",
        "limits:\n  parallel_reads: sometimes\n",
        "limits:\n  stable_prefix: 1\n",
        "limits:\n  record_changes: sometimes\n",
    ] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn stubs_are_off_and_their_limits_stay_inside_their_bounds() {
    let plain = parse(PROVIDERS).unwrap().limits();
    assert!(!plain.stub_results);
    assert_eq!((plain.stub_keep_steps, plain.stub_min_tokens), (8, 200));
    let set =
        parse("limits:\n  stub_results: true\n  stub_keep_steps: 2\n  stub_min_tokens: 4000\n")
            .unwrap()
            .limits();
    assert!(set.stub_results);
    assert_eq!((set.stub_keep_steps, set.stub_min_tokens), (2, 4000));
    let set = parse("limits:\n  stub_keep_steps: 50\n  stub_min_tokens: 50\n")
        .unwrap()
        .limits();
    assert_eq!((set.stub_keep_steps, set.stub_min_tokens), (50, 50));
    for bad in [
        "limits:\n  stub_results: 1\n",
        "limits:\n  stub_keep_steps: 1\n",
        "limits:\n  stub_keep_steps: 51\n",
        "limits:\n  stub_keep_steps: -8\n",
        "limits:\n  stub_min_tokens: 49\n",
        "limits:\n  stub_min_tokens: 4001\n",
        "limits:\n  stub_min_tokens: many\n",
    ] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn shaped_output_is_off_until_set() {
    assert!(!parse(PROVIDERS).unwrap().limits().shape_output);
    let set = parse("limits:\n  shape_output: true\n").unwrap().limits();
    assert!(set.shape_output);
    for bad in [
        "limits:\n  shape_output: 1\n",
        "limits:\n  shape_output: yes please\n",
    ] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn the_graph_and_episodes_are_off_until_set() {
    let plain = parse(PROVIDERS).unwrap().limits();
    assert!(!plain.project_graph && !plain.episodes && !plain.episode_lesson);
    let set = parse("limits:\n  project_graph: true\n  episodes: true\n  episode_lesson: true\n")
        .unwrap()
        .limits();
    assert!(set.project_graph && set.episodes && set.episode_lesson);
    for bad in [
        "limits:\n  project_graph: 1\n",
        "limits:\n  episodes: sometimes\n",
        "limits:\n  episode_lesson: yes please\n",
    ] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn the_graph_and_recall_tools_are_off_until_switched_on() {
    use crate::{catalog::modes, tools::ToolKind};
    let tools = [
        ("project_graph", ToolKind::Files),
        ("recall", ToolKind::Harness),
    ];
    let mut config = parse(PROVIDERS).unwrap();
    for (name, kind) in tools {
        // A mode's switch alone cannot turn it on: only the user's `tools:` can.
        config.set_mode_tool_enabled(modes::PLAN, name, true);
        for mode in [modes::MAIN, modes::PLAN, modes::ORCHESTRATOR] {
            assert!(
                !config.mode_tool_enabled(mode, name, kind),
                "{name} in {mode}"
            );
        }
    }
    let on = parse("tools:\n  project_graph: true\n  recall: true\n").unwrap();
    for (name, kind) in tools {
        assert!(on.mode_tool_enabled(modes::MAIN, name, kind), "{name}");
    }
}

#[test]
fn the_kept_tail_is_off_and_stays_inside_its_bounds() {
    let plain = parse(PROVIDERS).unwrap().limits();
    assert!(!plain.compact_tail && !plain.compact_early);
    assert_eq!(plain.compact_tail_tokens, 16_000);
    let set = parse(
        "limits:\n  compact_tail: true\n  compact_early: true\n  compact_tail_tokens: 2000\n",
    )
    .unwrap()
    .limits();
    assert!(set.compact_tail && set.compact_early);
    assert_eq!(set.compact_tail_tokens, 2_000);
    let set = parse("limits:\n  compact_tail_tokens: 64000\n")
        .unwrap()
        .limits();
    assert_eq!(set.compact_tail_tokens, 64_000);
    for bad in [
        "limits:\n  compact_tail: yes please\n",
        "limits:\n  compact_early: 1\n",
        "limits:\n  compact_tail_tokens: 1999\n",
        "limits:\n  compact_tail_tokens: 64001\n",
        "limits:\n  compact_tail_tokens: -16000\n",
        "limits:\n  compact_tail_tokens: lots\n",
    ] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn the_failure_ladder_is_off_and_its_rungs_rise() {
    let plain = parse(PROVIDERS).unwrap().limits();
    assert!(!plain.failure_ladder);
    let rungs = (
        plain.failure_note_at,
        plain.failure_replan_at,
        plain.failure_stop_at,
    );
    assert_eq!(rungs, (3, 5, 8));
    let set = parse("limits:\n  failure_ladder: true\n  failure_stop_at: 12\n").unwrap();
    assert!(set.limits().failure_ladder);
    assert_eq!(set.limits().failure_stop_at, 12);
    for bad in [
        "limits:\n  failure_note_at: 1\n",
        "limits:\n  failure_replan_at: 3\n",
        "limits:\n  failure_stop_at: 5\n",
        "limits:\n  failure_stop_at: 51\n",
    ] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn the_team_switches_are_off_and_its_ceilings_have_defaults() {
    let plain = parse(PROVIDERS).unwrap().limits();
    assert!(!plain.plan_graph && !plain.task_packets && !plain.member_worktrees);
    assert!(!plain.peer_messages && !plain.procedures);
    let ceilings = (
        plain.team_at_once,
        plain.team_max_steps,
        plain.team_max_tokens,
        plain.team_max_minutes,
    );
    assert_eq!(ceilings, (3, 400, 4_000_000, 120));
    let set = parse(
        "limits:\n  plan_graph: true\n  task_packets: true\n  member_worktrees: true\n  \
         peer_messages: true\n  procedures: true\n",
    )
    .unwrap()
    .limits();
    assert!(set.plan_graph && set.task_packets && set.member_worktrees);
    assert!(set.peer_messages && set.procedures);
    for bad in [
        "limits:\n  plan_graph: 1\n",
        "limits:\n  procedures: maybe\n",
        "limits:\n  team_unknown: 1\n",
    ] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn the_team_ceilings_accept_their_bounds_and_refuse_beyond() {
    let limit = |key: &str, value: u64| parse(&format!("limits:\n  {key}: {value}\n"));
    for (key, low, high) in [
        ("team_at_once", 1, 8),
        ("team_max_steps", 10, 5_000),
        ("team_max_tokens", 100_000, 100_000_000),
        ("team_max_minutes", 5, 1_440),
    ] {
        assert!(limit(key, low).is_ok(), "{key} {low}");
        assert!(limit(key, high).is_ok(), "{key} {high}");
        assert!(limit(key, low - 1).is_err(), "{key} {}", low - 1);
        assert!(limit(key, high + 1).is_err(), "{key} {}", high + 1);
    }
}

#[test]
fn a_team_never_runs_more_at_once_than_agents_may_work() {
    let limits = |yaml: &str| parse(yaml).unwrap().limits();
    assert_eq!(
        limits("limits:\n  max_agents_at_work: 8\n").team_members_at_once(),
        3
    );
    // The default of 3 would be too many for a user who allows two agents; it is cut.
    assert_eq!(
        limits("limits:\n  max_agents_at_work: 2\n").team_members_at_once(),
        2
    );
    let both = "limits:\n  max_agents_at_work: 4\n  team_at_once: 6\n";
    assert_eq!(limits(both).team_members_at_once(), 4);
}

#[test]
fn the_team_section_names_models_and_roles() {
    let plain = parse(PROVIDERS).unwrap();
    assert!(plain.team().model.is_none() && plain.team().models.is_empty());
    let set = parse(
        "team:\n  model: main/small\n  models:\n    code-reviewer: main/strong\n    general: main/small\n",
    )
    .unwrap();
    assert_eq!(set.team().model.as_deref(), Some("main/small"));
    assert_eq!(set.team().models["code-reviewer"], "main/strong");
    let yaml = set.to_yaml().unwrap();
    assert_eq!(parse(&yaml).unwrap().team(), set.team());
    for bad in [
        "team:\n  model: small\n",
        "team:\n  model: '/small'\n",
        "team:\n  models:\n    reviewer: nowhere\n",
        "team:\n  models:\n    '../x': main/small\n",
        "team:\n  models:\n    '': main/small\n",
        "team:\n  speed: fast\n",
    ] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
}
