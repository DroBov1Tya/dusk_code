//! What the screen shows of a thread: the protocol's chat, folded from the stored history
//! and the live events, drawn as a list of entries. Thinking, tool calls and summaries fold
//! into one line. The entries are a view of the chat: only the items that changed are
//! drawn as entries again, and each entry keeps a stamp that changes with it, so the
//! layout of the ones that did not change is kept.

use ratatui::{
    layout::{Rect, Size},
    text::Line,
};
use std::{
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicU64, Ordering},
};

use super::paste;
use dusk_protocol::{
    catalog::SkillChange,
    system::now_ms,
    thread::{
        ChatLog, GoalEvaluation, JobResult, Message, ThreadEvent, Tool, ToolState,
        transcript::{INTERRUPTED, Item},
    },
    tools::{Answered, Card},
};

pub enum Entry {
    /// A sent message; `open` shows its pasted blocks in full.
    User {
        text: String,
        open: bool,
        /// The agent's questions this message answers, drawn in place of its text.
        answered: Vec<Answered>,
    },
    /// What a background job left when it ended; `open` shows its output.
    Job {
        result: JobResult,
        open: bool,
    },
    Answer(String),
    /// Stands before a run of thinking and tool calls: one line that says what goes on,
    /// with the run under it once opened.
    Activity {
        open: bool,
    },
    Reasoning {
        text: String,
        open: bool,
    },
    /// A step, with the pictures `display_image` showed, which stay in sight when it folds.
    Tool {
        tool: Tool,
        pictures: Vec<Card>,
        open: bool,
    },
    /// What the evaluator said of the goal after a turn; `open` shows its reason.
    Goal {
        evaluation: GoalEvaluation,
        open: bool,
    },
    /// What a compaction left in place of the older messages.
    Summary {
        text: String,
        open: bool,
    },
    Notice {
        text: String,
        tone: Tone,
    },
    /// One change an agent made to a skill, under the step that made it.
    Skill {
        id: i64,
        text: String,
        tone: Tone,
    },
    /// A message another agent sent this thread, from its thread `thread_id`.
    FromAgent {
        thread_id: String,
        text: String,
        open: bool,
    },
    /// What the agent showed with the `show` tool, in place of that tool step.
    Block(dusk_protocol::thread::Block),
    /// The Workshop listings the agent suggested, in place of that tool step.
    Suggestion(dusk_protocol::plugin::Suggestion),
    /// A session the agent proposed, and where it stands; the reply row starts or
    /// dismisses it while it is the last thing in the chat.
    Proposal {
        proposal: dusk_protocol::thread::Proposal,
        state: dusk_protocol::thread::ProposalState,
    },
    /// A team or subagents the chat started, as its card said at `seen` (Unix
    /// milliseconds): a running card's clock goes on from there.
    Run {
        card: dusk_protocol::thread::RunCard,
        seen: i64,
    },
}

/// A Workshop card's plain text, for the copy button: its heading and a line a listing.
fn card_text(card: &dusk_protocol::plugin::Suggestion) -> String {
    let mut text = card.label.clone();
    for listing in &card.listings {
        let kind = format!("{:?}", listing.kind).to_lowercase();
        text.push_str(&format!(
            "\n- {} ({kind}): {}",
            listing.name, listing.description
        ));
    }
    text
}

/// Thinking and tool calls: the steps an activity line stands for.
fn is_step(entry: &Entry) -> bool {
    matches!(entry, Entry::Reasoning { .. } | Entry::Tool { .. })
}

/// Longer agent messages start folded.
const OPEN_AGENT_LINES: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Info,
    Error,
    Success,
}

/// A line of this client's own in the chat: what a command said, a message kept unsent.
#[derive(Debug, Clone, PartialEq)]
pub struct Local {
    /// The unsent message it stands for.
    pub local_id: Option<String>,
    pub text: String,
    pub tone: Tone,
}

/// Which entry of which item: what keeps an entry's open state and whether it is hidden
/// while the entries are drawn again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    Item(u64, Part),
    Skill(i64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Part {
    Activity,
    Thinking,
    Main,
}

#[derive(Default)]
pub struct Transcript {
    /// Boxed: a chat is large, and a transcript is held in place in the app and panels.
    log: Box<ChatLog<Local>>,
    /// The chat as entries, in order.
    pub entries: Vec<Entry>,
    keys: Vec<Key>,
    /// The item each entry draws, at the same index.
    items: Vec<usize>,
    /// Changes whenever the entry at the same index does; unique across transcripts.
    stamps: Vec<u64>,
    /// The entry each item's entries start at.
    starts: Vec<usize>,
    /// The skill changes the entries show, to tell when they changed.
    skills: Vec<SkillChange>,
    /// How many entries at the end are skill changes whose step is not in the chat.
    orphans: usize,
    /// Entries the user hid; each shows as one line until shown again.
    hidden: HashSet<Key>,
    /// What the agent does now, for the activity line of the run in progress.
    live: Option<String>,
    /// Only the root transcript is connected to the local image store.
    pub images: super::images::Images,
    /// The thread that asked an agent window's agent: its messages read as requests.
    asker: Option<String>,
}

/// The drawn lines of every entry but the last, kept while the width holds, so a frame
/// lays out only the entries that changed and the one still being written.
pub struct Layout {
    pub width: usize,
    /// What each laid-out entry looked like, from `Transcript::signature`.
    pub signatures: Vec<u64>,
    pub lines: Vec<Line<'static>>,
    /// Each line as plain text, for copying a selection.
    pub text: Vec<String>,
    /// The line each entry starts on.
    pub starts: Vec<usize>,
    /// How many lines there were before each entry, with the blank line above it.
    pub before: Vec<usize>,
    pub images: Vec<ImageSpot>,
}

/// The rows of an image preview on screen: where they are, the card's whole size in
/// cells, and how many of its rows are scrolled off above.
pub struct ShownPreview {
    pub area: Rect,
    pub card: Size,
    pub skip: u16,
    pub artifact: Card,
}

/// An image preview in the lines: its first line, its column and size in cells.
#[derive(Clone)]
pub struct ImageSpot {
    pub line: usize,
    pub column: u16,
    pub width: u16,
    pub height: u16,
    pub artifact: Card,
}

/// Stamps come from one counter, so a new transcript never matches an old one.
fn next_stamp() -> u64 {
    static STAMPS: AtomicU64 = AtomicU64::new(1);
    STAMPS.fetch_add(1, Ordering::Relaxed)
}

/// What an opened job result shows on screen: this many lines, and no more than this many characters.
const JOB_LINES: usize = 12;
const JOB_CHARS: usize = 1200;

impl Transcript {
    /// The stored messages as entries; system messages are not shown.
    pub fn from_history(messages: &[Message]) -> Self {
        let mut transcript = Self {
            log: Box::new(ChatLog::from_history(messages)),
            ..Self::default()
        };
        transcript.sync();
        transcript
    }

    /// For an agent window: messages from `asker`, the thread that asked the agent, read
    /// as what the user asked.
    pub fn with_asker(mut self, asker: Option<String>) -> Self {
        self.asker = asker;
        self.redraw_all();
        self
    }

    /// Folds a live event of the thread into it.
    /// What the thread does at `now`, as the protocol tells the kinds of work apart; none
    /// when the chat knows of nothing that runs.
    pub fn work(&self, now: i64) -> Option<dusk_protocol::thread::Work> {
        dusk_protocol::thread::activity(&self.log.live, now)
    }

    /// The user's message went to the agent: no question waits for it.
    pub fn answered(&mut self) {
        self.log.live.answered();
    }

    /// The thread stopped: nothing of it runs.
    pub fn stopped(&mut self) {
        self.log.live = Default::default();
    }

    pub fn apply(&mut self, event: ThreadEvent) {
        self.log.apply(event, now_ms());
        self.sync();
    }

    pub fn set_artifact_store(&mut self, store: dusk_core::app::ArtifactStore) {
        self.images = super::images::Images::new(store);
        self.redraw_all();
    }

    pub fn set_live(&mut self, live: Option<String>) {
        self.live = live;
    }

    /// The user's own message, shown as it is sent; one that answers the agent's question
    /// keeps the question beside it.
    pub fn add_user(&mut self, text: &str) {
        self.log.add_user(text.to_owned(), None, now_ms());
        self.sync();
    }

    /// The summary a new thread starts from, as a fork wrote it.
    pub fn add_summary(&mut self, text: &str) {
        self.log.push(Item::Summary(text.to_owned()), now_ms());
        self.sync();
    }

    pub fn notice(&mut self, text: impl Into<String>, tone: Tone) {
        self.local(None, text.into(), tone);
    }

    /// A line of this client's own; `local_id` names the unsent message it stands for.
    pub fn local(&mut self, local_id: Option<String>, text: String, tone: Tone) {
        let local = Local {
            local_id,
            text,
            tone,
        };
        self.log.push(Item::Local(local), now_ms());
        self.sync();
    }

    /// Changes the lines of this client's own that `change` picks.
    pub fn change_locals(&mut self, mut change: impl FnMut(&mut Local) -> bool) {
        let picked: Vec<usize> = (0..self.log.items.len())
            .filter(|&index| matches!(&self.log.items[index], Item::Local(_)))
            .collect();
        for index in picked {
            let mut local = match &self.log.items[index] {
                Item::Local(local) => local.clone(),
                _ => continue,
            };
            if change(&mut local)
                && let Some(Item::Local(kept)) = self.log.item_mut(index)
            {
                *kept = local;
            }
        }
        self.sync();
    }

    /// The changes of a thread read from the core, each under the step that made it.
    pub fn add_skill_changes(&mut self, changes: &[SkillChange]) {
        self.log.skills_loaded(changes.to_vec());
        self.sync();
    }

    /// The user stopped the thread: the calls still running stopped with it.
    pub fn stop_running_tools(&mut self) {
        self.log.stop_running_tools(INTERRUPTED);
        self.sync();
    }

    /// Drops the item the entry at `index` draws, with all its entries.
    pub fn remove(&mut self, index: usize) {
        if let Some(&item) = self.items.get(index) {
            self.log.remove(item);
            self.sync();
        }
    }

    /// When the entry at `index` came, in ms since the epoch.
    pub fn time(&self, index: usize) -> Option<i64> {
        self.log.at(*self.items.get(index)?)
    }

    /// Changes when the entry at `index` does.
    pub fn stamp(&self, index: usize) -> u64 {
        self.stamps.get(index).copied().unwrap_or(0)
    }

    /// Draws every entry again, as when the look of all of them changed.
    fn redraw_all(&mut self) {
        self.reproject(0);
    }

    /// Draws again the entries of the items that changed or came since the last time.
    fn sync(&mut self) {
        let items = self.log.items.len();
        let mut from = self.log.take_changed().unwrap_or(items);
        // Items that came since are drawn too.
        from = from.min(self.starts.len());
        let skills_changed = self.skills != self.log.skill_changes;
        if skills_changed {
            // A change shows under its step: draw again from the first step whose
            // changes differ, or the end.
            let changed = self
                .log
                .skill_changes
                .iter()
                .filter(|change| !self.skills.contains(change))
                .chain(
                    self.skills
                        .iter()
                        .filter(|change| !self.log.skill_changes.contains(change)),
                )
                .filter_map(|change| self.step_of(&change.call_id))
                .min()
                .unwrap_or(items);
            from = from.min(changed);
            self.skills = self.log.skill_changes.clone();
        }
        if from < items || self.starts.len() != items || skills_changed {
            self.reproject(from);
        }
    }

    fn step_of(&self, call_id: &str) -> Option<usize> {
        self.log
            .items
            .iter()
            .position(|item| matches!(item, Item::Tool(tool) if tool.call_id == call_id))
    }

    /// Draws the entries of item `from` and every item after it again; open entries stay
    /// open.
    fn reproject(&mut self, from: usize) {
        let end = self.entries.len() - self.orphans;
        let cut = self.starts.get(from).copied().unwrap_or(end).min(end);
        let mut open: HashMap<Key, bool> = HashMap::new();
        for (key, entry) in self.keys[cut..].iter().zip(&self.entries[cut..]) {
            if let Some(was) = open_of(entry) {
                open.insert(*key, was);
            }
        }
        self.entries.truncate(cut);
        self.keys.truncate(cut);
        self.items.truncate(cut);
        self.stamps.truncate(cut);
        self.starts.truncate(from);
        for index in from..self.log.items.len() {
            self.starts.push(self.entries.len());
            self.project(index, &open);
        }
        // A change whose step is not in the chat shows at its end.
        let orphans: Vec<Entry> = self
            .log
            .skill_changes
            .iter()
            .filter(|change| self.step_of(&change.call_id).is_none())
            .map(skill_entry)
            .collect();
        self.orphans = orphans.len();
        let last = self.log.items.len().saturating_sub(1);
        for entry in orphans {
            let Entry::Skill { id, .. } = entry else {
                continue;
            };
            self.put(Key::Skill(id), last, entry, &open);
        }
    }

    fn put(&mut self, key: Key, item: usize, mut entry: Entry, open: &HashMap<Key, bool>) {
        if let (Some(was), Some(now)) = (open.get(&key), open_mut(&mut entry)) {
            *now = *was;
        }
        self.entries.push(entry);
        self.keys.push(key);
        self.items.push(item);
        self.stamps.push(next_stamp());
    }

    /// A step: after an activity line when it starts a run.
    fn put_step(&mut self, id: u64, item: usize, entry: Entry, open: &HashMap<Key, bool>) {
        let in_run = matches!(
            self.entries.last(),
            Some(Entry::Activity { .. } | Entry::Reasoning { .. } | Entry::Tool { .. })
        );
        if !in_run {
            let activity = Entry::Activity { open: false };
            self.put(Key::Item(id, Part::Activity), item, activity, open);
        }
        let part = match entry {
            Entry::Reasoning { .. } => Part::Thinking,
            _ => Part::Main,
        };
        self.put(Key::Item(id, part), item, entry, open);
    }

    /// The entries of the item at `index`.
    fn project(&mut self, index: usize, open: &HashMap<Key, bool>) {
        let id = self.log.id(index);
        let main = Key::Item(id, Part::Main);
        let entry = match &self.log.items[index] {
            Item::User { text, answered, .. } => Entry::User {
                text: text.clone(),
                open: false,
                answered: answered.clone(),
            },
            Item::Agent {
                thread_id, text, ..
            } => {
                let from = thread_id.clone().unwrap_or_default();
                match self.asker.as_ref() == Some(&from) {
                    true => Entry::User {
                        text: text.clone(),
                        open: false,
                        answered: Vec::new(),
                    },
                    false => Entry::FromAgent {
                        open: text.lines().count() <= OPEN_AGENT_LINES,
                        thread_id: from,
                        text: text.clone(),
                    },
                }
            }
            Item::Job(result) => Entry::Job {
                result: job_shown(result),
                open: false,
            },
            Item::Assistant { text, reasoning } => {
                let (text, reasoning) = (text.clone(), reasoning.clone());
                if !reasoning.is_empty() {
                    let thinking = Entry::Reasoning {
                        text: reasoning,
                        open: false,
                    };
                    self.put_step(id, index, thinking, open);
                }
                // Models often open with blank lines; a new block does not need them.
                let text = text.trim_start();
                if !text.is_empty() {
                    self.put(main, index, Entry::Answer(text.to_owned()), open);
                }
                return;
            }
            Item::Summary(text) => Entry::Summary {
                text: text.clone(),
                open: false,
            },
            Item::Goal(evaluation) => Entry::Goal {
                evaluation: evaluation.clone(),
                open: false,
            },
            Item::Tool(tool) => {
                let (tool, call_id) = (tool.clone(), tool.call_id.clone());
                let step = Entry::Tool {
                    tool,
                    pictures: Vec::new(),
                    open: false,
                };
                self.put_step(id, index, step, open);
                let changes: Vec<Entry> = self
                    .log
                    .skill_changes_of(&call_id)
                    .map(skill_entry)
                    .collect();
                for change in changes {
                    let Entry::Skill { id, .. } = change else {
                        continue;
                    };
                    self.put(Key::Skill(id), index, change, open);
                }
                return;
            }
            Item::Block(block) => Entry::Block(block.clone()),
            Item::Suggestion(card) => Entry::Suggestion(card.clone()),
            Item::Proposal { proposal, state } => Entry::Proposal {
                proposal: proposal.clone(),
                state: state.clone(),
            },
            Item::Run(_) => {
                let seen = now_ms();
                let Some(card) = self.log.run_card(index, seen) else {
                    return;
                };
                Entry::Run { card, seen }
            }
            // A picture `display_image` showed stays with its step.
            Item::Image {
                card,
                call_id: Some(call_id),
                aside: false,
            } => {
                let card = card.clone();
                let step = self.entries.iter().rposition(
                    |entry| matches!(entry, Entry::Tool { tool, .. } if tool.call_id == *call_id),
                );
                if let Some(step) = step
                    && let Entry::Tool { pictures, .. } = &mut self.entries[step]
                    && !pictures.iter().any(|shown| shown.id == card.id)
                {
                    pictures.push(card);
                    self.stamps[step] = next_stamp();
                }
                return;
            }
            // Sounds, and pictures the model took to see, are not drawn here.
            Item::Image { .. } | Item::Audio(_) => return,
            Item::Notice(text) => Entry::Notice {
                text: text.clone(),
                tone: Tone::Info,
            },
            Item::Error(text) => Entry::Notice {
                text: text.clone(),
                tone: Tone::Error,
            },
            Item::Local(local) => Entry::Notice {
                text: local.text.clone(),
                tone: local.tone,
            },
        };
        self.put(main, index, entry, open);
    }

    /// The run under the activity line at `index`: its steps.
    pub fn run(&self, index: usize) -> impl Iterator<Item = &Entry> {
        self.entries[index + 1..]
            .iter()
            .take_while(|entry| is_step(entry))
    }

    /// What the activity line at `index` says goes on, while its run is the newest thing
    /// in the thread: the step reported, else the tool still running.
    pub fn current_step(&self, index: usize) -> Option<String> {
        if index + 1 + self.run(index).count() < self.entries.len() {
            return None;
        }
        self.live.clone().or_else(|| {
            self.run(index).find_map(|entry| match entry {
                Entry::Tool { tool, .. } if tool.state == ToolState::Running => {
                    Some(tool.name.clone())
                }
                _ => None,
            })
        })
    }

    /// A step under a folded activity line, which shows nothing of it.
    pub fn is_folded_away(&self, index: usize) -> bool {
        if !self.entries.get(index).is_some_and(is_step) {
            return false;
        }
        let line = self.entries[..index]
            .iter()
            .rev()
            .find(|entry| !is_step(entry));
        matches!(line, Some(Entry::Activity { open: false }))
    }

    /// The newest command still running, which the user may send to the background.
    pub fn running_command(&self) -> Option<String> {
        self.entries.iter().rev().find_map(running_command)
    }

    /// The command still running at entry `index`, or in the run under the activity line
    /// there.
    pub fn running_command_at(&self, index: usize) -> Option<String> {
        match self.entries.get(index)? {
            Entry::Activity { .. } => self.run(index).find_map(running_command),
            entry => running_command(entry),
        }
    }

    /// What the copy button of an entry copies: its whole text, folded or not.
    pub fn text(&self, index: usize) -> Option<String> {
        Some(match self.entries.get(index)? {
            Entry::User { text, .. } => paste::plain(text),
            // What is drawn as markdown copies its labels' words without the braces.
            Entry::Answer(text) | Entry::Summary { text, .. } | Entry::FromAgent { text, .. } => {
                dusk_protocol::thread::unmarked(text)
            }
            Entry::Activity { .. } => String::new(),
            Entry::Reasoning { text, .. }
            | Entry::Notice { text, .. }
            | Entry::Skill { text, .. } => text.clone(),
            Entry::Job { result, .. } => format!("{}\n{}", result.title(), result.output),
            Entry::Goal { evaluation, .. } => {
                format!("{}\n{}", evaluation.title(), evaluation.reason)
            }
            Entry::Block(block) => block.text(),
            Entry::Suggestion(card) => card_text(card),
            Entry::Proposal { proposal, .. } => format!("{}\n{}", proposal.title, proposal.brief),
            Entry::Run { card, .. } => format!("{} \u{00B7} {}", card.title, card.line()),
            Entry::Tool { tool, .. } => match tool.state {
                ToolState::Running => tool.arguments.clone(),
                _ => tool.output.clone(),
            },
        })
    }

    pub fn hide(&mut self, index: usize) {
        if let Some(key) = self.keys.get(index) {
            self.hidden.insert(*key);
            self.stamps[index] = next_stamp();
        }
    }

    pub fn is_hidden(&self, index: usize) -> bool {
        self.keys
            .get(index)
            .is_some_and(|key| self.hidden.contains(key))
    }

    /// A hidden entry opens like a folded one.
    pub fn is_foldable(&self, index: usize) -> bool {
        if self.is_hidden(index) {
            return true;
        }
        if self.is_folded_away(index) {
            return false;
        }
        match self.entries.get(index) {
            Some(Entry::User { text, .. }) => paste::has_pasted(text),
            entry => matches!(
                entry,
                Some(
                    Entry::Activity { .. }
                        | Entry::Reasoning { .. }
                        | Entry::Tool { .. }
                        | Entry::Summary { .. }
                        | Entry::Goal { .. }
                        | Entry::FromAgent { .. }
                        | Entry::Job { .. }
                )
            ),
        }
    }

    pub fn toggle(&mut self, index: usize) {
        let Some(key) = self.keys.get(index) else {
            return;
        };
        self.stamps[index] = next_stamp();
        if self.hidden.remove(key) {
            return;
        }
        if let Some(open) = self.entries.get_mut(index).and_then(open_mut) {
            *open = !*open;
        }
    }

    /// Opens every thinking block, or folds them all when any is open.
    pub fn toggle_reasoning(&mut self) {
        let any_open = self
            .entries
            .iter()
            .any(|entry| matches!(entry, Entry::Reasoning { open: true, .. }));
        for (index, entry) in self.entries.iter_mut().enumerate() {
            let changed = match entry {
                Entry::Reasoning { open, .. } => {
                    *open = !any_open;
                    true
                }
                // What is opened has to show.
                Entry::Activity { open } if !any_open => {
                    *open = true;
                    true
                }
                _ => false,
            };
            if changed {
                self.stamps[index] = next_stamp();
            }
        }
    }

    /// The nearest foldable entry before or after `from`.
    pub fn next_foldable(&self, from: usize, forward: bool) -> Option<usize> {
        if forward {
            (from + 1..self.entries.len()).find(|&index| self.is_foldable(index))
        } else {
            (0..from).rev().find(|&index| self.is_foldable(index))
        }
    }

    pub fn last_foldable(&self) -> Option<usize> {
        (0..self.entries.len())
            .rev()
            .find(|&index| self.is_foldable(index))
    }
}

fn open_of(entry: &Entry) -> Option<bool> {
    match entry {
        Entry::User { open, .. }
        | Entry::Activity { open }
        | Entry::Reasoning { open, .. }
        | Entry::Tool { open, .. }
        | Entry::Summary { open, .. }
        | Entry::Goal { open, .. }
        | Entry::FromAgent { open, .. }
        | Entry::Job { open, .. } => Some(*open),
        _ => None,
    }
}

fn open_mut(entry: &mut Entry) -> Option<&mut bool> {
    match entry {
        Entry::User { open, .. }
        | Entry::Activity { open }
        | Entry::Reasoning { open, .. }
        | Entry::Tool { open, .. }
        | Entry::Summary { open, .. }
        | Entry::Goal { open, .. }
        | Entry::FromAgent { open, .. }
        | Entry::Job { open, .. } => Some(open),
        _ => None,
    }
}

fn running_command(entry: &Entry) -> Option<String> {
    match entry {
        Entry::Tool { tool, .. }
            if tool.state == ToolState::Running && tool.name == "run_command" =>
        {
            Some(tool.call_id.clone())
        }
        _ => None,
    }
}

/// A background job's result, folded to one line. Opened, it shows the first lines of
/// its output: the model gets all of it; a chatty job must not push the conversation off
/// the screen.
fn job_shown(result: &JobResult) -> JobResult {
    let mut lines = result.output.lines();
    let mut text = lines
        .by_ref()
        .take(JOB_LINES)
        .collect::<Vec<_>>()
        .join("\n");
    if let Some((end, _)) = text.char_indices().nth(JOB_CHARS) {
        text.truncate(end);
        text.push_str("...");
    }
    match lines.count() {
        0 => {}
        more => text.push_str(&format!("\n[{more} more lines]")),
    }
    JobResult {
        output: text,
        ..result.clone()
    }
}

/// The row a skill change draws as. Its name and reason are the agent's text, so they
/// are shown as plain text: control and bidi characters never reach the screen.
pub fn skill_line(change: &SkillChange) -> String {
    // A tab reads as a space; any other control or bidi character is marked.
    let clean = |text: &str| {
        text.chars()
            .map(|c| match c {
                '\t' => ' ',
                c if plain(c) => c,
                _ => '?',
            })
            .collect()
    };
    // The terminal has no Undo; it says where to find one.
    let state = if change.undone {
        "undone"
    } else if change.can_undo {
        "undo in the app"
    } else {
        "previous text not kept"
    };
    format!("{} ({state})", change.label(clean))
}

fn skill_entry(change: &SkillChange) -> Entry {
    Entry::Skill {
        id: change.id,
        text: skill_line(change),
        tone: if change.undone {
            Tone::Info
        } else {
            Tone::Success
        },
    }
}

/// Whether a character can stand in a row as itself: no control, no bidi override.
fn plain(c: char) -> bool {
    !c.is_control()
        && !matches!(
            c,
            '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
        )
}

#[cfg(test)]
mod tests;
