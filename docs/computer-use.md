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

## The real pointer: `physical_input`

Clicks and scrolls that move the user's own mouse are one tool of their own,
`physical_input`, with `action` one of `click`, `double_click`, `right_click` or
`scroll`. It is off until the user switches it on under Tools (one switch for every
mode), and each use is asked for under the chat's approval mode; allowing the screen
does not allow it.

```json
{"action": "right_click", "x": 320, "y": 180}
{"action": "scroll", "x": 320, "y": 180, "down": 400}
```

The old `mode`, `right` and `double` of `click` are gone; a saved call that still asks
for the pointer through them is refused with a message that names `physical_input`.

`type_text` and `press_keys` go to whatever has the keyboard focus and ask every time.
Keys are a key name with modifiers joined by `+`; only plain spaces may pad them, and
the request shows them by their normal names, such as `cmd+shift+t`.

## Several actions in one call

`computer_actions` runs up to 20 steps of `click`, `type_text`, `press_keys` and
`screenshot` in order, each with the arguments it takes alone, at most four
screenshots among them:

```json
{"steps": [
  {"tool": "click", "arguments": {"x": 320, "y": 180}},
  {"tool": "type_text", "arguments": {"text": "hello"}},
  {"tool": "press_keys", "arguments": {"keys": "enter"}},
  {"tool": "screenshot", "arguments": {}}
]}
```

Every step is checked before anything is asked or done; a wrong one names its number
and nothing runs. A step whose tool is switched off in the mode, or that a plugin's
`before_tool` hook denies, refuses the whole batch the same way.

The batch is approved before any step runs, in one request that lists every step with
its number, those a session grant covers marked "already allowed". So typing and keys
in a batch are approved before the user sees where the focus lands: approve one only
when the steps before it say where that is, or ask for the steps one by one. Steps of
one kind can be allowed for the session as that kind; mixed kinds are asked for this
time only, under a neutral heading. In arbiter mode each step is judged on its own,
and one no refuses the batch. So a batch is never asked for less than its steps one
by one. The terminal grows the request to show up to twelve rows of steps and pages
the rest; a yes there pages on until every step has been shown, and counts only a
moment after the last ones appear. The desktop scrolls a long list inside the request;
a chat platform such as Telegram gets every step in its message, and a request too
long for one message (over 3500 characters) is not offered for a yes there: it can
only be denied there, and is allowed in the app.

Every x, y is in the picture taken before the batch, even after a screenshot inside
it; after the batch the newest picture counts. The batch stops at the first failing
step and says which ran.

## Linux

Linux has no press without the pointer: a `click` always moves it, so it asks like
`physical_input` does on macOS, and no cursor of duskcode's own is drawn. The tools
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
