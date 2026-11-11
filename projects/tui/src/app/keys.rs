//! Keys. Typing goes to the message; Tab walks the foldable entries; panels take every
//! key while open.

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use std::time::{Duration, Instant};

use dusk_protocol::{client::Command, release::InstallStage};

use super::{
    Action, App, Focus, Panel, commands,
    mouse::inside,
    panels::{Connection, Preset, SecretEntry, step},
};

/// What /effort offers, as the core names them.
pub const EFFORTS: [&str; 9] = [
    "default", "off", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
];

/// How long a first Ctrl+C waits for the second.
const QUIT_WINDOW: Duration = Duration::from_secs(5);

impl App {
    pub fn handle(&mut self, event: Event) -> Action {
        match event {
            Event::FocusGained => {
                self.focused = Some(true);
                Action::None
            }
            Event::FocusLost => {
                self.focused = Some(false);
                Action::None
            }
            Event::Key(key) if key.kind == KeyEventKind::Press => self.key(key),
            Event::Paste(text) if matches!(self.panel, Some(Panel::Connect(_))) => {
                if let Some(Panel::Connect(form)) = &mut self.panel {
                    let single_line: String = text.chars().filter(|c| !c.is_control()).collect();
                    if let Some(field) = form.field() {
                        field.insert(&single_line);
                    }
                    form.error = None;
                }
                Action::None
            }
            Event::Paste(text)
                if matches!(self.panel, Some(Panel::Plugins { entry: Some(_), .. })) =>
            {
                if let Some(Panel::Plugins {
                    entry: Some(entry), ..
                }) = &mut self.panel
                {
                    let single_line: String = text.chars().filter(|c| !c.is_control()).collect();
                    entry.input.insert(&single_line);
                    entry.error = None;
                }
                Action::None
            }
            Event::Paste(text) if matches!(self.panel, Some(Panel::Paste(_))) => {
                self.editor_paste(&text.replace("\r\n", "\n").replace('\r', "\n"));
                Action::None
            }
            Event::Mouse(mouse) if matches!(self.panel, Some(Panel::Agent(_))) => {
                self.agent_window_mouse(mouse)
            }
            Event::Mouse(mouse) if matches!(self.panel, Some(Panel::Settings(_))) => {
                self.settings_mouse(mouse)
            }
            Event::Paste(text) if matches!(self.panel, Some(Panel::Settings(_))) => {
                self.settings_paste(&text.replace("\r\n", "\n").replace('\r', "\n"));
                Action::None
            }
            Event::Mouse(mouse) if matches!(self.panel, Some(Panel::Paste(_))) => {
                self.editor_mouse(mouse)
            }
            Event::Mouse(mouse) if matches!(self.panel, Some(Panel::Update { .. })) => {
                let pressed = mouse.kind == MouseEventKind::Down(MouseButton::Left);
                match &self.panel {
                    Some(Panel::Update {
                        info: Some(info),
                        progress: None,
                        button: Some((row, left, right)),
                        ..
                    }) if pressed
                        && mouse.row == *row
                        && (*left..*right).contains(&mouse.column) =>
                    {
                        Action::InstallUpdate(info.latest.clone())
                    }
                    _ => Action::None,
                }
            }
            Event::Mouse(mouse) if matches!(self.panel, Some(Panel::Connect(_))) => {
                if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                    && let Some(Panel::Connect(form)) = &mut self.panel
                    && let Some(index) = form.field_areas.iter().position(|area| {
                        area.is_some_and(|area| inside(area, mouse.column, mouse.row))
                    })
                {
                    match form.preset {
                        None if index < Preset::ALL.len() => form.pick(Preset::ALL[index]),
                        None => {}
                        Some(_) => form.selected = index.min(Connection::FIELDS - 1),
                    }
                }
                Action::None
            }
            Event::Paste(text) if self.panel.is_none() => {
                self.focus = Focus::Input;
                self.input
                    .paste(&text.replace("\r\n", "\n").replace('\r', "\n"));
                Action::None
            }
            Event::Mouse(mouse) if self.panel.is_none() => self.mouse(mouse),
            _ => Action::None,
        }
    }

    fn key(&mut self, key: KeyEvent) -> Action {
        self.flash = None;
        if self.panel.is_none() && key.code == KeyCode::Esc && self.selection.take().is_some() {
            return Action::None;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let typing = self.panel.is_none() && self.focus == Focus::Input;
        if typing && key.code == KeyCode::Esc && self.input.anchor.take().is_some() {
            return Action::None;
        }
        // With text selected in the box, Ctrl+C copies it instead of clearing the box.
        if typing
            && ctrl
            && key.code == KeyCode::Char('c')
            && let Some(text) = self.input.selected()
        {
            return Action::Copy(text);
        }
        if ctrl && key.code == KeyCode::Char('c') {
            let selected = match &self.panel {
                Some(Panel::Agent(window)) => window.selected_text(),
                None => self.selection.and_then(|selection| {
                    (selection.anchor != selection.cursor)
                        .then(|| self.selected_text(selection))
                        .filter(|text| !text.trim().is_empty())
                }),
                _ => None,
            };
            if let Some(text) = selected {
                self.quit_armed = None;
                return Action::Copy(text);
            }
            return self.interrupt_key();
        }
        self.quit_armed = None;
        if ctrl && key.code == KeyCode::Char('q') {
            return Action::Quit;
        }
        if self.panel.is_some() {
            return self.panel_key(key);
        }
        match key.code {
            KeyCode::Char('t') if ctrl => return Action::Command(Command::Resume { cwd: None }),
            KeyCode::Char('n') if ctrl => return Action::Command(Command::New),
            KeyCode::Char('o') if ctrl => return Action::Command(Command::Settings),
            KeyCode::Char('r') if ctrl => {
                self.transcript.toggle_reasoning();
                return Action::None;
            }
            KeyCode::Char('b') if ctrl => {
                match self.transcript.running_command() {
                    Some(call_id) => return Action::Background(call_id),
                    None => self.flash = Some("no command runs in the foreground".into()),
                }
                return Action::None;
            }
            KeyCode::F(3) => {
                if let Some(card) = self.subagents.last() {
                    return Action::OpenAgent(card.thread_id.clone());
                }
                return Action::None;
            }
            KeyCode::F(1) => {
                self.panel = Some(Panel::Help);
                return Action::None;
            }
            KeyCode::F(2) => {
                if self.compact_layout {
                    self.sidebar_overlay_open = !self.sidebar_overlay_open;
                } else {
                    self.sidebar_visible = !self.sidebar_visible;
                }
                return Action::None;
            }
            KeyCode::PageUp if key.modifiers.contains(KeyModifiers::SHIFT) => {
                let page = self
                    .agent_area
                    .map_or(1, |area| area.height.saturating_sub(2) as usize)
                    .max(1);
                self.agent_scroll = self.agent_scroll.saturating_sub(page);
                return Action::None;
            }
            KeyCode::PageDown if key.modifiers.contains(KeyModifiers::SHIFT) => {
                let page = self
                    .agent_area
                    .map_or(1, |area| area.height.saturating_sub(2) as usize)
                    .max(1);
                self.agent_scroll = self
                    .agent_scroll
                    .saturating_add(page)
                    .min(self.agent_max_scroll);
                return Action::None;
            }
            KeyCode::PageUp if key.modifiers.contains(KeyModifiers::ALT) => {
                let page = self
                    .mcp_area
                    .map_or(1, |area| area.height.saturating_sub(2) as usize)
                    .max(1);
                self.mcp_scroll = self.mcp_scroll.saturating_sub(page);
                return Action::None;
            }
            KeyCode::PageDown if key.modifiers.contains(KeyModifiers::ALT) => {
                let page = self
                    .mcp_area
                    .map_or(1, |area| area.height.saturating_sub(2) as usize)
                    .max(1);
                self.mcp_scroll = self
                    .mcp_scroll
                    .saturating_add(page)
                    .min(self.mcp_max_scroll);
                return Action::None;
            }
            KeyCode::PageUp => {
                if self.scroll_by(self.viewport.saturating_sub(2).max(1), true) {
                    self.focus = Focus::Input;
                }
                return Action::None;
            }
            KeyCode::PageDown => {
                if self.scroll_by(self.viewport.saturating_sub(2).max(1), false) {
                    self.focus = Focus::Input;
                }
                return Action::None;
            }
            _ => {}
        }
        if !self.approvals.is_empty() {
            return self.approval_key(key);
        }
        if self.poll.as_ref().is_some_and(|poll| poll.active) {
            return self.poll_key(key);
        }
        match self.focus {
            Focus::Input => self.input_key(key),
            Focus::Browse(selected) => self.browse_key(key, selected),
        }
    }

    /// Ctrl+C clears the box; a second one within the window quits, so one press
    /// never loses the session.
    fn interrupt_key(&mut self) -> Action {
        if self
            .quit_armed
            .is_some_and(|armed| armed.elapsed() <= QUIT_WINDOW)
        {
            return Action::Quit;
        }
        if matches!(self.panel, Some(Panel::Connect(_))) {
            self.panel = None;
        }
        self.input.take();
        self.quit_armed = Some(Instant::now());
        self.flash = Some("Ctrl+C again to quit".into());
        Action::None
    }

    fn input_key(&mut self, key: KeyEvent) -> Action {
        if key.code != KeyCode::Tab {
            self.input.command_cycle = None;
            self.end_path_cycle();
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Enter if shift => self.input.insert("\n"),
            // Where Shift+Enter reads as Enter, a backslash before it asks for a new line.
            KeyCode::Enter if self.input.text[..self.input.cursor].ends_with('\\') => {
                self.input.backspace();
                self.input.insert("\n");
            }
            // A command typed in part runs as the one it can only be.
            KeyCode::Enter => {
                self.complete_command();
                return self.submit(alt);
            }
            KeyCode::Char('j') if ctrl => self.input.insert("\n"),
            KeyCode::Char('u') if ctrl => {
                self.input.take();
            }
            KeyCode::Char('e') if ctrl => {
                if let Some(index) = self.input.chip_at_cursor() {
                    self.open_paste(index);
                }
            }
            KeyCode::Char('a') if ctrl => self.input.select_all(),
            KeyCode::Char('x') if ctrl => {
                if let Some(text) = self.input.selected() {
                    self.input.delete_selection();
                    return Action::Copy(text);
                }
            }
            KeyCode::Char(c) if !ctrl && !alt => self.input.insert(c.encode_utf8(&mut [0; 4])),
            KeyCode::Backspace => self.input.backspace(),
            KeyCode::Delete => self.input.delete(),
            KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End => {
                self.input.mark(shift);
                match key.code {
                    KeyCode::Left => self.input.left(),
                    KeyCode::Right => self.input.right(),
                    KeyCode::Home => self.input.home(),
                    _ => self.input.end(),
                }
            }
            KeyCode::Up | KeyCode::Down if shift => {
                self.input.mark(true);
                self.input.vertical(key.code == KeyCode::Down);
            }
            KeyCode::Up | KeyCode::Down => {
                self.input.mark(false);
                let down = key.code == KeyCode::Down;
                if !self.input.vertical(down) {
                    self.recall(!down);
                }
            }
            KeyCode::Tab if self.cycle_path() => {}
            KeyCode::Tab if self.cycle_command() => {}
            KeyCode::Tab if self.poll.is_some() => {
                if let Some(poll) = &mut self.poll {
                    poll.active = true;
                }
            }
            KeyCode::Tab => return Action::NextMode,
            KeyCode::BackTab => {
                if let Some(last) = self.transcript.last_foldable() {
                    self.focus = Focus::Browse(last);
                }
            }
            // Agents can work on after the turn that asked them; Esc stops them too.
            KeyCode::Esc if self.is_busy() || self.agents_at_work() => {
                self.unqueue_into_box();
                return Action::Cancel;
            }
            _ => {}
        }
        Action::None
    }

    /// Tab selects a candidate without submitting or changing the operating mode.
    fn cycle_command(&mut self) -> bool {
        let typed = self.input.text.as_str();
        if !typed.starts_with('/') || typed.contains(char::is_whitespace) {
            self.input.command_cycle = None;
            return false;
        }
        let (prefix, previous) = match &self.input.command_cycle {
            Some((prefix, selected)) => (prefix.clone(), Some(*selected)),
            None => (typed.to_owned(), None),
        };
        let fitting: Vec<_> = commands()
            .filter(|(name, _)| name.starts_with(&prefix))
            .collect();
        if !fitting.is_empty() {
            let selected = previous.map_or(0, |selected| (selected + 1) % fitting.len());
            self.input.set(fitting[selected].0.to_owned());
            self.input.command_cycle = Some((prefix, selected));
        }
        // Even an unknown slash token owns Tab until the command token ends.
        true
    }

    /// Finishes a command name when only one fits what is typed.
    fn complete_command(&mut self) -> bool {
        let typed = self.input.text.as_str();
        if !typed.starts_with('/') || typed.contains(char::is_whitespace) {
            return false;
        }
        let mut fitting = commands().filter(|(name, _)| name.starts_with(typed));
        match (fitting.next(), fitting.next()) {
            (Some((name, _)), None) => {
                self.input.set(format!("{name} "));
                true
            }
            _ => false,
        }
    }

    fn browse_key(&mut self, key: KeyEvent, selected: usize) -> Action {
        let transcript = &self.transcript;
        let moved = match key.code {
            KeyCode::Up | KeyCode::Char('k') => transcript.next_foldable(selected, false),
            KeyCode::Down | KeyCode::Char('j') => transcript.next_foldable(selected, true),
            KeyCode::Home | KeyCode::Char('g') => transcript.next_foldable(0, true),
            KeyCode::End | KeyCode::Char('G') => transcript.last_foldable(),
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.transcript.toggle(selected);
                None
            }
            KeyCode::Esc | KeyCode::BackTab => {
                self.focus = Focus::Input;
                None
            }
            KeyCode::Char(c) => {
                self.focus = Focus::Input;
                self.input.insert(c.encode_utf8(&mut [0; 4]));
                None
            }
            _ => None,
        };
        if let Some(index) = moved {
            self.focus = Focus::Browse(index);
        }
        Action::None
    }

    fn panel_key(&mut self, key: KeyEvent) -> Action {
        if matches!(self.panel, Some(Panel::Settings(_))) {
            return self.settings_key(key);
        }
        let busy = self.is_busy();
        let Some(panel) = &mut self.panel else {
            return Action::None;
        };
        let down = matches!(key.code, KeyCode::Down | KeyCode::Char('j'));
        let up = matches!(key.code, KeyCode::Up | KeyCode::Char('k'));
        match panel {
            Panel::Connect(form) if form.preset.is_none() => match key.code {
                KeyCode::Esc => self.panel = None,
                KeyCode::Enter => form.pick(Preset::ALL[form.choice]),
                _ if up || down => step(&mut form.choice, Preset::ALL.len(), down),
                _ => {}
            },
            Panel::Connect(form) => {
                let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                let preset = form.preset.unwrap_or(Preset::Custom);
                match key.code {
                    KeyCode::Esc => form.back(),
                    KeyCode::Char('o') if ctrl => {
                        if let Some(page) = preset.key_page() {
                            return Action::OpenLink(page.into());
                        }
                    }
                    KeyCode::Up | KeyCode::Down => {
                        step(
                            &mut form.selected,
                            Connection::FIELDS,
                            key.code == KeyCode::Down,
                        );
                    }
                    KeyCode::Tab | KeyCode::BackTab => {
                        let back = key.code == KeyCode::BackTab
                            || key.modifiers.contains(KeyModifiers::SHIFT);
                        let count = Connection::FIELDS;
                        form.selected = if back {
                            (form.selected + count - 1) % count
                        } else {
                            (form.selected + 1) % count
                        };
                    }
                    KeyCode::Enter if form.selected + 1 < Connection::FIELDS => form.selected += 1,
                    KeyCode::Enter => {
                        let base_url = form.fields[1].text.trim().to_owned();
                        if base_url.is_empty() {
                            form.error = Some("Provider URL is required".into());
                            form.selected = 1;
                        } else if preset.needs_key() && form.fields[2].is_empty() {
                            form.error = Some("This provider needs an API key".into());
                            form.selected = 2;
                        } else {
                            let given = |index: usize| {
                                let text = form.fields[index].text.trim();
                                (!text.is_empty()).then(|| text.to_owned())
                            };
                            return Action::Connect {
                                kind: preset.kind(),
                                name: given(0),
                                base_url,
                                api_key: given(2),
                            };
                        }
                    }
                    code => {
                        let alt = key.modifiers.contains(KeyModifiers::ALT);
                        if let Some(field) = form.field() {
                            match code {
                                KeyCode::Char(c) if !ctrl && !alt => {
                                    field.insert(c.encode_utf8(&mut [0; 4]));
                                    form.error = None;
                                }
                                KeyCode::Backspace => field.backspace(),
                                KeyCode::Delete => field.delete(),
                                KeyCode::Left => field.left(),
                                KeyCode::Right => field.right(),
                                KeyCode::Home => field.home(),
                                KeyCode::End => field.end(),
                                _ => {}
                            }
                        }
                    }
                }
                return Action::None;
            }
            Panel::Update {
                info,
                note,
                progress,
                ..
            } => {
                // Once replacement starts there is no safe cancellation contract.
                if progress.is_some_and(|progress| progress.stage != InstallStage::Complete) {
                    return Action::None;
                }
                match key.code {
                    KeyCode::Enter if progress.is_none() => {
                        if let Some(info) = info.as_ref().filter(|info| info.newer && info.ready) {
                            return Action::InstallUpdate(info.latest.clone());
                        }
                    }
                    // What the window says, an error above all, to paste elsewhere.
                    KeyCode::Char('y') => {
                        let mut text = info
                            .as_ref()
                            .map(|info| {
                                format!("running {}\nnewest {}\n", info.current, info.latest)
                            })
                            .unwrap_or_default();
                        text.extend(note.iter().map(|note| format!("{note}\n")));
                        if !text.is_empty() {
                            return Action::Copy(text.trim_end().to_owned());
                        }
                    }
                    KeyCode::Tab | KeyCode::Char('c') => {
                        if let Some(info) = info {
                            let channel = info.channel.other().name().to_owned();
                            return Action::Command(Command::Update {
                                channel: Some(channel),
                            });
                        }
                    }
                    KeyCode::Esc => self.panel = None,
                    _ => {}
                }
                return Action::None;
            }
            Panel::Undo { points, selected } => {
                match key.code {
                    _ if up || down => step(selected, points.len(), down),
                    KeyCode::Enter if busy => {
                        self.flash = Some("stop the agent first: Esc".into());
                    }
                    KeyCode::Enter => {
                        let sequence = points.get(*selected).map(|point| point.sequence);
                        self.panel = None;
                        if let Some(sequence) = sequence {
                            return Action::Send(format!("/undo {sequence}"));
                        }
                    }
                    KeyCode::Esc => self.panel = None,
                    _ => {}
                }
                return Action::None;
            }
            Panel::Sources {
                sources,
                selected,
                removing,
            } => {
                let source = sources.get(*selected);
                match key.code {
                    KeyCode::Char('d') if !*removing => {
                        *removing = true;
                        return Action::None;
                    }
                    KeyCode::Char('d') => {
                        if let Some(source) = source {
                            return Action::RemoveSource(source.source_id.clone());
                        }
                    }
                    _ if up || down => step(selected, sources.len(), down),
                    KeyCode::Char(' ') | KeyCode::Enter => {
                        if let Some(source) = source {
                            return Action::PinSource {
                                id: source.source_id.clone(),
                                pinned: !source.pinned,
                            };
                        }
                    }
                    KeyCode::Esc => {
                        self.panel = None;
                        return Action::None;
                    }
                    _ => {}
                }
                *removing = false;
                return Action::None;
            }
            Panel::Jobs(view) => {
                match key.code {
                    _ if up || down => {
                        step(&mut view.selected, view.jobs.len(), down);
                        view.stale = true;
                    }
                    KeyCode::Char('x') => {
                        let stoppable = view
                            .jobs
                            .get(view.selected)
                            .filter(|job| job.running && job.stoppable);
                        match stoppable {
                            Some(job) => return Action::StopJob(job.job_id.clone()),
                            None => self.flash = Some("this job cannot be stopped here".into()),
                        }
                    }
                    KeyCode::Char('r') => view.stale = true,
                    KeyCode::Esc => self.panel = None,
                    _ => {}
                }
                return Action::None;
            }
            Panel::Diff(view) => {
                match key.code {
                    _ if view.open && (up || down) => {
                        view.scroll = if up {
                            view.scroll.saturating_sub(1)
                        } else {
                            view.scroll + 1
                        }
                    }
                    KeyCode::PageUp if view.open => view.scroll = view.scroll.saturating_sub(15),
                    KeyCode::PageDown if view.open => view.scroll += 15,
                    _ if up || down => step(&mut view.selected, view.files.len(), down),
                    KeyCode::Enter if !view.open => {
                        view.open = true;
                        view.scroll = 0;
                    }
                    KeyCode::Esc | KeyCode::Left if view.open => view.open = false,
                    KeyCode::Esc => self.panel = None,
                    _ => {}
                }
                return Action::None;
            }
            Panel::Effort { selected } => {
                match key.code {
                    _ if up || down => step(selected, EFFORTS.len(), down),
                    KeyCode::Enter => {
                        let level = EFFORTS[*selected].to_owned();
                        self.panel = None;
                        return Action::Command(Command::Effort { level: Some(level) });
                    }
                    KeyCode::Esc => self.panel = None,
                    _ => {}
                }
                return Action::None;
            }
            Panel::Plugins {
                entry: Some(entry), ..
            } => {
                let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                let alt = key.modifiers.contains(KeyModifiers::ALT);
                match key.code {
                    KeyCode::Esc => {
                        if let Panel::Plugins { entry, .. } = panel {
                            *entry = None;
                        }
                    }
                    KeyCode::Enter => {
                        return Action::SetPluginSecret {
                            plugin: entry.plugin.clone(),
                            key: entry.key.clone(),
                            value: entry.input.text.clone(),
                        };
                    }
                    KeyCode::Char(c) if !ctrl && !alt => {
                        entry.input.insert(c.encode_utf8(&mut [0; 4]));
                        entry.error = None;
                    }
                    KeyCode::Backspace => entry.input.backspace(),
                    KeyCode::Delete => entry.input.delete(),
                    KeyCode::Left => entry.input.left(),
                    KeyCode::Right => entry.input.right(),
                    KeyCode::Home => entry.input.home(),
                    KeyCode::End => entry.input.end(),
                    _ => {}
                }
                return Action::None;
            }
            Panel::Plugins { selected, entry } => {
                let plugin = self
                    .plugins
                    .get(*selected)
                    .map(|plugin| plugin.name.clone());
                match key.code {
                    _ if up || down => step(selected, self.plugins.len(), down),
                    KeyCode::Char('s') => {
                        let secret = self.plugins.get(*selected).and_then(|plugin| {
                            let (key, _) = plugin.secrets.first()?;
                            Some(SecretEntry {
                                plugin: plugin.name.clone(),
                                key: key.clone(),
                                input: super::Input::default(),
                                error: None,
                            })
                        });
                        match secret {
                            Some(secret) => *entry = Some(secret),
                            None => self.flash = Some("this plugin has no secret setting".into()),
                        }
                    }
                    KeyCode::Char(' ') | KeyCode::Enter => {
                        return plugin.map_or(Action::None, Action::TogglePlugin);
                    }
                    KeyCode::Esc => self.panel = None,
                    _ => {}
                }
                return Action::None;
            }
            Panel::Unsent { selected } => {
                match key.code {
                    _ if up || down => step(selected, self.local_unsent.len(), down),
                    KeyCode::Enter => {
                        return self
                            .local_unsent
                            .get(*selected)
                            .map_or(Action::None, |item| Action::Resend(item.id.clone()));
                    }
                    KeyCode::Esc => self.panel = None,
                    _ => {}
                }
                return Action::None;
            }
            Panel::Paste(_) => return self.editor_key(key),
            Panel::Agent(_) => return self.agent_window_key(key),
            Panel::Models(list) => match key.code {
                _ if up || down => {
                    let count = list.choices().len();
                    step(&mut list.selected, count, down);
                }
                KeyCode::Enter => {
                    let chosen = list.choices().get(list.selected).cloned();
                    return chosen.map_or(Action::None, |name| {
                        Action::Command(Command::Model { name: Some(name) })
                    });
                }
                KeyCode::Char('r') => return Action::Command(Command::Model { name: None }),
                KeyCode::Esc => self.panel = None,
                _ => {}
            },
            Panel::Help => self.panel = None,
            // The drawing keeps the row within the lines.
            Panel::Context { row, .. } => {
                *row = match key.code {
                    _ if up => row.saturating_sub(1),
                    _ if down => *row + 1,
                    KeyCode::PageUp => row.saturating_sub(10),
                    KeyCode::PageDown => *row + 10,
                    _ => {
                        self.panel = None;
                        return Action::None;
                    }
                };
            }
            Panel::Threads(list) => match key.code {
                KeyCode::Char('y') => {
                    return list
                        .threads
                        .get(list.selected)
                        .map_or(Action::None, |thread| Action::Copy(thread.id.clone()));
                }
                _ if up || down => {
                    step(&mut list.selected, list.threads.len(), down);
                    let shown = list.preview.as_ref().map(|preview| &preview.thread_id);
                    return match list.threads.get(list.selected) {
                        Some(thread) if shown != Some(&thread.id) => {
                            Action::Preview(thread.id.clone())
                        }
                        _ => Action::None,
                    };
                }
                KeyCode::Enter => {
                    let chosen = list
                        .threads
                        .get(list.selected)
                        .map(|thread| thread.id.clone());
                    self.panel = None;
                    return chosen.map_or(Action::None, Action::OpenThread);
                }
                KeyCode::Char('n') => {
                    self.panel = None;
                    return Action::Command(Command::New);
                }
                KeyCode::Esc => self.panel = None,
                _ => {}
            },
            Panel::Settings(_) => {}
        }
        Action::None
    }

    pub fn scroll_by(&mut self, lines: usize, up: bool) -> bool {
        let before = self.scroll;
        self.scroll = if up {
            self.scroll.saturating_add(lines).min(self.max_scroll)
        } else {
            self.scroll.saturating_sub(lines)
        };
        self.scroll != before
    }
}
