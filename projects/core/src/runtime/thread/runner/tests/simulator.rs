//! The iOS Simulator plugin's tool through the agent's own path: a scripted model calls
//! it, the core runs it as an MCP tool, and the next request carries the screen as a
//! picture. Needs a booted simulator and the plugin built, so it runs only when asked:
//! `DUSK_IOS_PLUGIN=/path/to/ios-simulator cargo test -p dusk_core simulator -- --ignored`.

use std::collections::BTreeMap;

use super::*;
use crate::{
    config::{McpServerConfig, StdioServerConfig},
    mcp::{McpClient, McpServer},
    providers::ChatMessage,
    state::artifacts::ArtifactStore,
    tools::mcp::McpTool,
};

const TOOL: &str = "ios-simulator__ios_simulator";

fn call(id: &str, arguments: Value) -> Vec<Step> {
    vec![Step::Named(TOOL, id.into(), arguments), Step::Done]
}

#[tokio::test]
#[ignore = "needs a booted iOS Simulator and DUSK_IOS_PLUGIN"]
async fn the_agent_taps_types_and_sees_the_simulator() {
    let program = std::env::var("DUSK_IOS_PLUGIN").expect("DUSK_IOS_PLUGIN");
    let config = McpServerConfig::Stdio(Box::new(StdioServerConfig {
        command: program,
        enabled: true,
        args: vec!["mcp".into()],
        env: BTreeMap::new(),
        icon: None,
        isolation: None,
    }));
    let client = McpClient::start(&config, None).await.unwrap();
    let tools = client.list_tools().await.unwrap();
    let server = Arc::new(McpServer {
        name: "ios-simulator".into(),
        client,
        tools,
    });
    let root = temp_dir("simulator_scope");
    std::fs::create_dir_all(&root).unwrap();
    let store = ArtifactStore::new(root.join("artifacts"));
    let tool = McpTool::all(&server, &store).into_iter().next().unwrap();

    let state = thread_state("simulator_tool").await;
    let live = live_config("simulator_tool_config");
    let provider = provider(vec![
        call("call_home", json!({"action": "button", "button": "home"})),
        call(
            "call_settings",
            json!({"action": "launch", "bundle_id": "com.apple.Preferences"}),
        ),
        // The search field at the bottom of Settings on an iPhone 17 Pro.
        call("call_tap", json!({"action": "tap", "x": 200, "y": 822})),
        call("call_wait", json!({"action": "ui"})),
        call("call_type", json!({"action": "type", "text": "Wallpaper"})),
        call("call_screen", json!({"action": "screen"})),
        vec![Step::Text("done"), Step::Done],
        vec![Step::Text("Simulator"), Step::Done],
    ]);
    let (runner, input, mut output) = runner_seeing(
        &state,
        &provider,
        &live,
        model(None),
        vec![Box::new(tool)],
        root,
        store,
        true,
    );
    let client = async move {
        input.send("search Settings for Wallpaper").await.unwrap();
        assert_eq!(
            next_turn_finished(&mut output).await.status,
            TurnStatus::Completed
        );
    };
    tokio::join!(runner.run(), client);

    let requests = provider.requests.lock().unwrap();
    let results: Vec<(String, String, usize)> = requests
        .last()
        .into_iter()
        .chain(requests.iter().rev().skip(1))
        .flat_map(|messages| messages.iter())
        .filter_map(|message| match message {
            ChatMessage::Tool {
                call_id,
                content,
                pictures,
            } => Some((call_id.clone(), content.clone(), pictures.len())),
            _ => None,
        })
        .collect();
    for (id, content, pictures) in &results {
        eprintln!("{id}: {content} [{pictures} pictures]");
    }
    let find = |id: &str| results.iter().find(|(call, ..)| call == id).unwrap();
    assert!(find("call_tap").1.contains("tapped 200,822"));
    assert!(find("call_type").1.contains("typed"));
    assert!(find("call_settings").1.contains("points"));
    assert_eq!(
        find("call_screen").2,
        1,
        "the screen reaches the model as a picture"
    );
    let picture = requests.iter().flatten().find_map(|message| match message {
        ChatMessage::Tool {
            call_id, pictures, ..
        } if call_id == "call_screen" && !pictures.is_empty() => Some(pictures[0].clone()),
        _ => None,
    });
    if let (Some(picture), Ok(path)) = (picture, std::env::var("DUSK_IOS_SHOT")) {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&picture.base64)
            .unwrap();
        std::fs::write(path, bytes).unwrap();
    }
}
