# Pictures of the GUI without a screen

The shell cannot record the screen (no permission, no display session), so the window can
draw itself into a picture instead. It is a development feature, off in release builds:

```sh
cargo build -p dusk_gui --features screenshot --target-dir target/shot
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
| `fake=providers` | two saved providers, to look at a row's switch and buttons |
| `fake=plan-limits` | the ChatGPT plan's 5-hour and weekly windows in the limits ring |
| `fake=store` | fills the Workshop with thirty made-up listings instead of the store's |
| `workshop`, `workshop=install:NAME\|remove:NAME\|busy:NAME\|failed:NAME` | shows the Workshop; opens its confirmation for a listing, or shows a row at work or failed |
| `shot=NAME` | saves `$DUSK_SHOT_DIR/NAME.png`, drawn at the window's scale (2x on a Retina screen) |
| `quit` | ends |

Use a throwaway `HOME` with a copy of `config.yml` (mode 0600, folder 0700) and a project row
inserted into `state.db`, so no real chat is shown; `duskcode run "..." ` in the project folder
makes a chat to look at.
