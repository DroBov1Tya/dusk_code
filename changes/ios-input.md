Fixed
The iOS Simulator panel takes a press, a long press, a drag, the wheel, keys and typed text: the desktop sends a screen's touches by phase as they happen (`touch`, `scroll`, `key`, `text` events) instead of one tap or swipe after the click, and MCP tool calls tell the server the chat's project folder in `_meta`, so the simulator installs only apps built there.
