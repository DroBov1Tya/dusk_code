//! What a tool call is doing, said the way a person would say it: "Taking a screenshot"
//! rather than `screenshot`. Each tool has a few ways to say it, chosen by the call's ID,
//! so a run of the same tool reads less like a log; the tool's own name stays for whoever
//! unfolds the step.

/// A phrase for a call of `tool`; `seed` picks among its ways, stable for one call.
pub fn phrase(tool: &str, seed: &str) -> &'static str {
    let ways: &[&str] = match tool {
        "run_command" => &["Running a command", "Working in the terminal", "Running it"],
        "read_file" => &[
            "Reading a file",
            "Looking through the code",
            "Opening a file",
        ],
        "search_files" => &["Searching the project", "Looking for it", "Searching files"],
        "plan_edit" => &["Editing files", "Writing the changes", "Making the edit"],
        "screenshot" => &[
            "Taking a screenshot",
            "Looking at the screen",
            "Checking the screen",
        ],
        "screen" => &["Looking at the screen", "Reading the screen"],
        "click" => &["Clicking", "Pressing a button"],
        "type_text" => &["Typing", "Writing into the field"],
        "press_keys" => &["Pressing keys", "Using the keyboard"],
        "scroll" => &["Scrolling", "Scrolling through"],
        "window_session" => &["Choosing a window", "Focusing a window"],
        "ask_agent" => &[
            "Calling in a helper",
            "Handing work to an agent",
            "Asking a teammate",
        ],
        "agent_state" => &["Checking on the agents", "Seeing how the agents do"],
        "message_parent" => &["Reporting back", "Sending word up"],
        "ask_user" => &["Asking you", "Waiting for your answer"],
        "display_image" => &["Showing a picture"],
        "display_audio" => &["Playing a sound"],
        "load_skill" => &["Picking up a skill", "Reading a skill"],
        "remember" => &["Taking a note", "Remembering this"],
        "read_history" => &["Looking back at past work", "Reading the history"],
        "tasks" => &["Updating the todo list", "Ticking off the plan"],
        "goal" => &["Checking the goal", "Weighing the goal"],
        "jobs" => &["Checking background jobs"],
        "name_session" => &["Naming the chat"],
        _ => &["Using a tool"],
    };
    let pick = seed.bytes().fold(0usize, |sum, byte| {
        sum.wrapping_mul(31).wrapping_add(byte.into())
    });
    ways[pick % ways.len()]
}

/// Whether a tool starts or speaks to another agent, so a client can light it up.
pub fn is_agent_tool(tool: &str) -> bool {
    matches!(tool, "ask_agent" | "message_parent" | "agent_state")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_keeps_its_phrase_and_an_unknown_tool_still_reads() {
        assert_eq!(
            phrase("screenshot", "call-1"),
            phrase("screenshot", "call-1")
        );
        assert!(phrase("screenshot", "x").contains("screen"));
        assert_eq!(phrase("mcp__thing", "x"), "Using a tool");
    }
}
