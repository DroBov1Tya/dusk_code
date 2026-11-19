# Pictures of the GUI without a screen

The shell cannot record the screen (no permission, no display session), so the window can
draw itself into a picture instead. It is a development feature, off in release builds:

```sh
cargo build -p dusk_desktop --features screenshot --target-dir target/shot
DUSK_SHOT_DIR=/tmp/shots \
DUSK_SHOTS='wait=4000;project=0;wait=2000;thread=0;wait=3500;shot=chat;review;wait=2500;shot=review;quit' \
  target/shot/debug/duskcode-gui
```

A separate target folder keeps the feature's extra crates (gpui's test support) out of the
normal build. `DUSK_SHOTS` is steps separated by `;`:

| Step | Does |
|---|---|
| `wait=MS` | lets the core answer and the window draw |
| `project=N`, `thread=N` | chooses the Nth project or chat of the lists |
| `center=chat\|review\|settings\|connectors\|artifacts` | switches the middle column (it does not load that page's data; use the click paths for that) |
| `review` | opens the review page |
| `file=PATH[:LINE]` | opens a file in the viewer |
| `fake=welcome`, `welcome=form`, `welcome=address:URL` | the first screen of a clean install, its form, an address typed in it (`key=enter` connects) |
| `fake=providers` | two saved providers, to look at a row's switch and buttons |
| `fake=pulls` | four pull requests in Changes, one of each state and checks (a click on a count opens its window of checks), a chat of the chosen project with their icon, and a chat whose links to two of them are drawn as chips (`hover=` on one shows its card); needs `right=all` before it, since reading the column again replaces the fake |
| `fake=forge-signin`, `fake=forge-unserved` | the quiet line in Changes when the forge asks to sign in (its hint as a command to copy), or when no plugin serves the host |
| `fake=plan-limits` | the ChatGPT plan's 5-hour and weekly windows in the limits ring |
| `fake=store` | fills the Workshop with made-up listings of every tier and kind, packs and a failed source instead of the store's, drawn with the Workshop's real covers and screenshots (`dev/media/`, built in only with the feature) |
| `workshop`, `workshop=install:NAME\|remove:NAME\|busy:NAME\|failed:NAME\|coming:NAME\|pack:NAME\|open:NAME`, `workshop=offline:`, `workshop=gallery:N\|hero:N\|next:N\|scroll:PX`, `workshop=tier:TIER\|all:TIER`, `workshop=filters:`, `workshop=report` | shows the Workshop; opens its confirmation for a listing, shows a capsule at work or failed, its cover still coming, the store not read again (covers not here drawn offline), a pack's page, a listing's window and its Nth picture, the Nth featured pick, the Official shelf from its Nth capsule, the page scrolled down, a tier filtered or its shelf whole, the filter menu, or what "Update all" did |
| `fake=chat`, `fake=chat-open` | a long chat with folded turns, steps of each state, markdown with code and a table, a picture, a notice, an error, tasks and pins; `-open` unfolds the first turn and a step |
| `fake=mcp` | five connectors, one of each state, in the place of the config's |
| `open=pins\|plan\|panel\|done`, `scroll=N`, `right=lower` | the pins drawer, the plan's rows, the composer's panel, the Todo's finished tasks; the transcript from its Nth row; the right column with its tall upper modules folded |
| `hover=X,Y` | moves the pointer to a point of the window, for what shows only under it (the composer's tab, a module header's grip) |
| `down=X,Y`, `move=X,Y`, `up=X,Y` | presses the left button, moves with it held, lets it go: a click is `down` then `up`, a drag (a module's grip to another place) is `down`, a few `move`, `up`; a `shot` between shows the drop line |
| `press=X,Y` | `down` with no `up`: the pressed look of what is under the pointer |
| `select=ROW:FROM-TO` | selects cells of a row of the shown terminal |
| `sidebar=project\|recent` | lists the chats under their projects, or all by last activity |
| `size=WxH` | resizes the window, as a drag of its edge would |
| `shot=NAME` | saves `$DUSK_SHOT_DIR/NAME.png`, drawn at the window's scale (2x on a Retina screen) |
| `quit` | ends |

Use a throwaway `HOME` with a copy of `config.yml` (mode 0600, folder 0700) and a project row
inserted into `state.db`, so no real chat is shown; `duskcode run "..." ` in the project folder
makes a chat to look at.

The window draws no frames of its own, so an animation or a picture decoded after the first
draw shows only on a second `shot`: take one, `wait`, and take the one to keep.
