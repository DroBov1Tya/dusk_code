//! Keys of the open panels and windows.

use super::EFFORTS;
use crate::app::{
    Action, App, Panel,
    panels::{Connection, ModelRow, Preset, SecretEntry, step},
    schedule::ScheduleAction,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dusk_protocol::{client::Command, release::InstallStage};

impl App {
    pub(super) fn panel_key(&mut self, key: KeyEvent) -> Action {
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
                                input: crate::app::Input::default(),
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
                    let count = list.rows().len();
                    step(&mut list.selected, count, down);
                    list.removing = None;
                }
                KeyCode::Enter => match list.rows().get(list.selected).cloned() {
                    Some(ModelRow::Model { name, .. }) => {
                        return Action::Command(Command::Model { name: Some(name) });
                    }
                    Some(ModelRow::Provider(_)) => {
                        return Action::Command(Command::Model { name: None });
                    }
                    None => {}
                },
                // Removing asks twice: the first press names the provider.
                KeyCode::Char('d') => {
                    let Some(provider) = list
                        .rows()
                        .get(list.selected)
                        .map(|row| row.provider().to_owned())
                    else {
                        return Action::None;
                    };
                    if list.removing.as_deref() == Some(provider.as_str()) {
                        list.removing = None;
                        return Action::Disconnect(provider);
                    }
                    list.removing = Some(provider);
                }
                KeyCode::Char('t') => {
                    let rows = list.rows();
                    let Some(provider) = rows.get(list.selected).map(ModelRow::provider) else {
                        return Action::None;
                    };
                    let on = list
                        .providers
                        .iter()
                        .flatten()
                        .any(|status| status.name == provider && status.enabled);
                    return Action::ProviderEnabled(provider.to_owned(), !on);
                }
                KeyCode::Char('c') => self.panel = Some(Panel::Connect(Connection::default())),
                KeyCode::Char('r') => return Action::Command(Command::Model { name: None }),
                KeyCode::Esc => self.panel = None,
                _ => {}
            },
            Panel::Schedules {
                tasks,
                selected,
                removing,
            } => {
                let id = tasks.get(*selected).map(|task| task.id.clone());
                match key.code {
                    _ if up || down => {
                        step(selected, tasks.len(), down);
                        *removing = false;
                    }
                    KeyCode::Enter => {
                        return id
                            .map_or(Action::None, |id| Action::Schedule(ScheduleAction::Run(id)));
                    }
                    KeyCode::Char(' ') => {
                        let on = tasks.get(*selected).is_some_and(|task| !task.enabled);
                        return id.map_or(Action::None, |id| {
                            Action::Schedule(ScheduleAction::Toggle { id, on })
                        });
                    }
                    // Removing asks twice.
                    KeyCode::Char('d') if *removing => {
                        return id.map_or(Action::None, |id| {
                            Action::Schedule(ScheduleAction::Delete(id))
                        });
                    }
                    KeyCode::Char('d') => *removing = true,
                    KeyCode::Esc => self.panel = None,
                    _ => {}
                }
            }
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
}
