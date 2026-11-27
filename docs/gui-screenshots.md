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
| `center=chat\|review\|settings\|customize\|artifacts\|overview\|team` | switches the middle column (`connectors` still names Customize) (it does not load that page's data; use the click paths for that) |
| `review` | opens the review page |
| `file=PATH[:LINE]` | opens a file in the viewer |
| `fake=welcome`, `welcome=page:N`, `welcome=form`, `welcome=address:URL`, `welcome=connected:NAME` | the first run's welcome, its Nth page (1 Hello, 2 Model, 3 Tools, 4 Ready), the Model page, an address typed in it (`key=enter` connects), the Model page as after a connection to NAME |
| `fake=providers` | two saved providers, to look at a row's switch and buttons |
| `fake=tiers` | two providers with models and the subagent tiers in each state (chosen, the chat's model, a provider no longer connected), for `settings=llm` after its models loaded |
| `fake=pulls` | four pull requests in Changes, one of each state and checks (a click on a count opens its window of checks), a chat of the chosen project with eight pull requests of its own, seven read and counted under its icon and one not read yet left out (`hover=` on the icon shows their card, faded at the bottom while it scrolls on), and a chat whose links to two of them are drawn as chips (`hover=` on one shows its card); needs `right=all` before it, since reading the column again replaces the fake |
| `fake=branch` | the shown chat works on `agent/fix-login` in a worktree: the Changes module says so under the project's branch |
| `fake=unread` | three loose chats at the top of the sidebar: one unread, one read, one unread whose agent works again |
| `fake=forge-signin`, `fake=forge-unserved` | the quiet line in Changes when the forge asks to sign in (its hint as a command to copy), or when no plugin serves the host |
| `fake=plan-limits` | the ChatGPT plan's 5-hour and weekly windows in the limits ring |
| `fake=store` | fills the Workshop with made-up listings of every tier and kind, packs and a failed source instead of the store's, drawn with the Workshop's real covers and screenshots (`dev/media/`, built in only with the feature) |
| `fake=suggest` | after `fake=store`: a chat whose agent put a card of three of its listings in it, one installed |
| `workshop`, `workshop=connectors\|skills\|agents`, `workshop=installed:SHELF`, `workshop=install:NAME\|remove:NAME\|busy:NAME\|failed:NAME\|coming:NAME\|pack:NAME\|open:NAME`, `workshop=offline:`, `workshop=gallery:N\|hero:N\|next:N\|scroll:PX`, `workshop=tier:TIER\|all:TIER`, `workshop=filters:`, `workshop=report` | shows the Workshop of Plugins, or of another shelf of Customize, or a shelf's Installed list; opens its confirmation for a listing, shows a capsule at work or failed, its cover still coming, the store not read again (covers not here drawn offline), a pack's page, a listing's window and its Nth picture, the Nth featured pick, the Official shelf from its Nth capsule, the page scrolled down, a tier filtered or its shelf whole, the filter menu, or what "Update all" did |
| `fake=diagrams`, `fake=diagram-KIND` | a chat with a diagram of each kind the `draw_diagram` tool draws (flow, architecture, sequence, state, timeline, tree), or of one kind; works without a provider |
| `fake=chat`, `fake=chat-open` | a long chat with folded turns, steps of each state, markdown with code and a table, a picture, a notice, an error, tasks and pins; `-open` unfolds the first turn and a step |
| `fake=todo-short` | after `fake=chat`: its board cut to three open tasks, so the Todo unfolds in place instead of opening as a screen |
| `fake=activity-thinking\|stalled\|writing\|planning\|steps\|agents\|waiting\|compacting` | after `fake=chat`: a running turn in each kind of work the working row tells apart (`fake=retry` and `fake=unreachable` show the held one) |
| `fake=goal-active\|goal-paused\|goal-stalled\|goal-achieved\|goal-failed` | a chat working toward a standing goal, built from the core's goal events: its indicator over the composer (`open=goal` opens its card) and the evaluator's verdict rows (a click unfolds a reason); running, paused on a spent limit, stalled after three turns without a tool, met or found impossible |
| `fake=team`, `fake=team-ended` | after `fake=chat`: the chat leads a team on a plan of eight tasks, one in each status, with two fresh reviews (one approved, one asking for a fix), a skipped task and the critic to come (the core's `Team`, `TaskChanged` and member events), running or ended; the Overview and the plan strip get the answers their requests would give; then `modal=team-stop|team-at-once|task-card` opens the yes Stop team asks for, the members-at-once stepper or the card of task #4 |
| `fake=subagents`, `fake=subagents-ended` | after `fake=chat`: the chat asks three subagents in one step, one done, one at work and one failed, or all ended; the chat shows their run's card |
| `fake=agent-approvals` | after `fake=subagents`: each agent's context fill and compactions, and three requests for approval open at once, one of the chat and two of the subagent at work, the second shown (`modal=agents` lists the agents with their numbers) |
| `fake=team-2\|team-6\|team-24\|team-24-ended\|team-empty` | after `fake=chat`: a team of two members on four tasks, six on eight, or twenty-four on forty-four whose first waves are done (its done members fold by role, its done waves fold), the large one ended as stalled, or a TEAM chat with no plan yet; `center=team` shows the Team page and `layer=people\|plan\|both\|unfold` its layers or a long plan's done waves |
| `fake=model-own` | the shown chat tells its own model and effort (`deepseek/deepseek-chat`, high), which its Models button shows instead of the one new chats start with |
| `fake=mcp` | five connectors, one of each state, in the place of the config's |
| `screen=team\|overview\|plan\|todo` | opens that module as the right column's screen (the Todo only with more than three open tasks, else the list shows); `screen=` goes back to the list (take a `shot` before a click on it: the window draws no frames of its own, so a click lands on the last drawn frame) |
| `open=pins\|plan\|goal\|panel\|done`, `scroll=N`, `right=lower` | the pins drawer, the plan's rows, the goal's card, the composer's panel, the Todo's finished tasks; the transcript from its Nth row; the right column with its tall upper modules folded |
| `hover=X,Y` | moves the pointer to a point of the window, for what shows only under it (the composer's tab, a module header's grip) |
| `wheel=X,Y,DY` | scrolls DY pixels at a point of the window (negative goes down), for what is below a window's fold |
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

The working row's words swap faster than a shot is taken: `DUSK_SWAP=MS` holds a swap that many
milliseconds in (`fake=activity-thinking`, a shot, then `fake=activity-steps` in the same run).

The subagents' mascots move faster than a shot is taken: `DUSK_MASCOT=MS` in the environment holds
each that many milliseconds into its activity (`fake=agents;modal=agents` shows one of each).

## The web page

A debug build of the web client (`trunk build` in `projects/clients/browser`, served from its
`dist` by any static server) draws a made-up chat without a core when its address asks for one:
`?fake=team|team-ended|subagents|subagents-ended` is a chat that set a team of seven to work on a
plan of eight tasks, one in each status, or asked three subagents in one step, running or ended
(`src/dev`, not built into release). Nothing is sent; the page's calls fail quietly. Take the
pictures with any browser.
