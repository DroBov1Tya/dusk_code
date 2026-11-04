# Computer use

The model takes a screenshot when it needs to look, then clicks, types, presses keys
and scrolls by the coordinates of that picture. The tools are offered only in a
graphical session, to a model that sees pictures, and the first use in a session asks
the user. A cursor of duskcode's own marks where the agent last acted; it glides to
each point, rocks a little and hides after ten seconds at rest.

## Clicks that leave the pointer alone

A `click` presses the control under the point through macOS Accessibility (`AXPress`)
and posts no mouse events, so the user keeps their pointer:

```json
{"x": 320, "y": 180}
```

It is a single left click on something that can be pressed: a button, a link, a
checkbox. It does not click a canvas, hover, drag, or always focus a text field, so
the agent checks the focus before it types. A control that cannot be pressed this way
is an error, never a silent fallback to the real pointer. When the screenshot was of
one window, the click is refused if that window moved, changed size or got covered at
the point since: take a new screenshot then.

## The real pointer

`mode: "system"` clicks with the user's pointer, right and double clicks included;
`scroll` always does. Taking the pointer has a yes of its own: allowing the screen for
the session does not allow it.

```json
{"x": 320, "y": 180, "mode": "system", "right": true}
```

`type_text` and `press_keys` go to whatever has the keyboard focus and ask every time.

## Permissions

Clicks and keys need Accessibility, screenshots need Screen Recording, both in System
Settings > Privacy & Security, for the terminal duskcode runs in. A pressed control
may answer late: a timeout can mean the press happened, so the agent looks before it
tries again.
