# Computer use

The model takes a screenshot when it needs to look, then clicks, types, presses keys
and scrolls by the coordinates of that picture. The tools are offered only in a
graphical session, to a model that sees pictures, and the first use in a session asks
the user.

## Clicks that leave the pointer alone (macOS)

A `click` presses the control under the point through macOS Accessibility (`AXPress`)
and posts no mouse events, so the user keeps their pointer.

```json
{"x": 320, "y": 180}
```

It is a single left click on something that can be pressed: a button, a link, a
checkbox. It does not click a canvas, hover, drag, or always focus a text field, so
the agent checks the focus before it types. A control that cannot be pressed this way
returns an error. Take a new screenshot if the captured window moved or changed size.
Outside a window session, a covering window also causes the click to be refused.

For a window session, call `window_session` with `{"action":"list"}`. Then use
`{"action":"select","window_id":123,"pid":456}` with the IDs from that list.
`screenshot {}` and regions of that picture show only the selected window, even
when another app covers it. Click using the latest picture's coordinates. A press
can bring the app to the foreground. The session disables shared keyboard and
pointer input. Use `{"action":"release"}` with `window_session` to end it.

After a confirmed press, duskcode draws an amber ring at the target's screen
coordinates and leaves its own cursor there for ten seconds. The overlay passes
mouse input through and works in a selected window session too. If another window
covers the target, the ring marks the point over the visible window.

## The real pointer

`mode: "system"` clicks with the user's pointer, right and double clicks included;
`scroll` always does. Taking the pointer has a yes of its own: allowing the screen for
the session does not allow it.

```json
{"x": 320, "y": 180, "mode": "system", "right": true}
```

`type_text` and `press_keys` go to whatever has the keyboard focus and ask every time.

## Linux

Linux has no press without the pointer: a click always moves it, so it asks like
`mode: "system"` does on macOS, and no cursor of duskcode's own is drawn. The tools
drive the desktop through programs it has:

| Session | Clicks, keys, typing | Screenshots |
| --- | --- | --- |
| X11 | `xdotool` | `maim`, `scrot` or ImageMagick's `import` |
| Wayland | `ydotool`, with `ydotoold` running | `grim`, `gnome-screenshot` or `spectacle` |

Wayland is chosen when `WAYLAND_DISPLAY` is set. It does not show other apps' windows,
so there a screenshot is of the display or a region of it, not of one window. Shortcuts
take `cmd` as the Super key.

## Permissions (macOS)

Clicks and keys need Accessibility, screenshots need Screen Recording, both in System
Settings > Privacy & Security, for the terminal duskcode runs in. A pressed control
may answer late: a timeout can mean the press happened, so the agent looks before it
tries again.
