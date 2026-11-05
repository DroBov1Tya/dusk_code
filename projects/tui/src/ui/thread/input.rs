//! The bottom of the screen: a rule that tells what the agent is doing and how full the
//! context is, then the message being typed, on a background of its own. Typing `/`
//! lists the commands.

use ratatui::{
    Frame,
    layout::{Position, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{
    theme::{
        ACCENT, DEEP, DUSK, HAIRLINE, NEGATIVE, POSITIVE, RAISED, SELECTED, SURFACE, TEXT,
        TEXT_MUTED, TEXT_SOFT, bold, dim, fg, thought,
    },
    transcript::spinner,
    wrap,
};
use dusk_core::{config::ApprovalPolicy, state::GoalStatus, tools::Decision};

use crate::app::{
    Activity, App, ApprovalHit, ChipHit, Focus, GoalButton, GoalHit, Input, InputRow, PollHit,
    PollTarget, QueueButton, QueueHit, commands, paste,
};
use crate::host::git::GitSummary;

const MIN_ROWS: usize = 3;
const MAX_ROWS: usize = 10;
/// Queued messages shown above the box; older ones wait unseen.
const MAX_QUEUED: usize = 4;
const PROMPT: &str = " \u{203A} ";
/// Rows around the text: a blank row to breathe, then the card's two edges.
const FRAME_ROWS: u16 = 3;
/// Columns between the card's edge and its text on each side: the edge and one blank.
const SIDE: u16 = 2;

/// The message box and what sits above it: approvals, polls, the goal, the queue and the popups.
pub struct InputView<'a> {
    pub app: &'a mut App,
}

impl InputView<'_> {
    pub fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let approvals = self.app.status.approvals;
        self.draw_footer(
            frame,
            area,
            self.app.git.as_ref(),
            &self.app.folder,
            approvals,
        );
        let area = Rect {
            height: area.height.saturating_sub(1),
            ..area
        };
        let approval = approval_rows(self.app).min(area.height);
        self.draw_approval(
            frame,
            Rect {
                height: approval,
                ..area
            },
        );
        let area = Rect {
            y: area.y + approval,
            height: area.height.saturating_sub(approval),
            ..area
        };
        let polled = poll_rows(self.app).min(area.height);
        self.draw_poll(
            frame,
            Rect {
                height: polled,
                ..area
            },
        );
        let area = Rect {
            y: area.y + polled,
            height: area.height.saturating_sub(polled),
            ..area
        };
        let goal = goal_rows(self.app).min(area.height);
        self.draw_goal(
            frame,
            Rect {
                height: goal,
                ..area
            },
        );
        let area = Rect {
            y: area.y + goal,
            height: area.height.saturating_sub(goal),
            ..area
        };
        let queued = queue_rows(self.app).min(area.height);
        self.draw_queue(
            frame,
            Rect {
                height: queued,
                ..area
            },
        );
        let area = Rect {
            y: area.y + queued,
            height: area.height.saturating_sub(queued),
            ..area
        };
        // The message is a card: what the agent does on its top edge, what is below on the
        // right of it, and the text inside with a column of air on each side.
        let box_area = Rect {
            y: area.y + 1,
            height: area.height.saturating_sub(1),
            ..area
        };
        let (doing, below) = status(self.app);
        let edge = if self.app.is_busy() {
            SELECTED
        } else {
            HAIRLINE
        };
        let mut card = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(fg(edge))
            .style(Style::new().bg(SURFACE))
            .title(Line::from(doing));
        if !below.is_empty() {
            card = card
                .title(Line::from(Span::styled(format!(" {below} "), fg(ACCENT))).right_aligned());
        }
        if let Some(tag) = &self.app.update_available {
            let note = format!(" {tag} is out \u{00B7} /update ");
            card = card.title_bottom(Line::from(Span::styled(note, fg(TEXT_SOFT))).right_aligned());
        }
        frame.render_widget(card, box_area);
        let text_area = Rect {
            x: box_area.x + SIDE,
            y: box_area.y + 1,
            width: box_area.width.saturating_sub(2 * SIDE),
            height: box_area.height.saturating_sub(2),
        };
        self.app.input_chips.clear();
        self.app.input_rows.clear();
        self.app.input_area = Some(box_area);
        let width = text_width(area.width);
        let prompt = Span::styled(PROMPT, bold(ACCENT));
        if self.app.input.text.is_empty() {
            let hint = if !self.app.provider_available {
                "provider unavailable; /connect or /model to choose a provider"
            } else if self.app.is_busy() {
                "write to queue it; Alt+Enter sends now, Esc stops"
            } else {
                "write, or / for commands"
            };
            let line = Line::from(vec![prompt, Span::styled(hint, thought())]);
            frame.render_widget(Paragraph::new(line), text_area);
            show_cursor(frame, self.app, text_area, (0, 0));
            return;
        }
        let (rows, cursor) = layout(&self.app.input, width);
        let skip = (cursor.1 + 1).saturating_sub(text_area.height as usize);
        let left = text_area.x + PROMPT.width() as u16;
        let mut lines = Vec::new();
        for (row, shown) in rows.into_iter().enumerate().skip(skip) {
            let y = text_area.y + (row - skip) as u16;
            self.app.input_rows.push(InputRow {
                row: y,
                right: left + shown.width as u16,
                cells: shown
                    .cells
                    .iter()
                    .map(|&(column, offset)| (left + column as u16, offset))
                    .collect(),
                end: shown.end,
            });
            for &(from, to, index) in &shown.chips {
                self.app.input_chips.push(ChipHit {
                    row: y,
                    left: left + from as u16,
                    right: left + to as u16,
                    index,
                });
            }
            let lead = if row == 0 {
                prompt.clone()
            } else {
                Span::raw("   ")
            };
            lines.push(Line::from([vec![lead], shown.spans].concat()));
        }
        frame.render_widget(Paragraph::new(lines), text_area);
        show_cursor(frame, self.app, text_area, (cursor.0, cursor.1 - skip));
    }

    /// The repository summary and package version share a row below the message card.
    fn draw_footer(
        &self,
        frame: &mut Frame,
        area: Rect,
        git: Option<&GitSummary>,
        folder: &str,
        approvals: ApprovalPolicy,
    ) {
        if area.is_empty() {
            return;
        }
        let bottom = Rect {
            y: area.bottom() - 1,
            height: 1,
            ..area
        };
        // A status bar of its own colour, so the row reads as one even with little on it.
        frame.render_widget(Block::new().style(Style::new().bg(DEEP)), bottom);
        const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));
        // Keep the version whole; an extremely narrow terminal has no room for it.
        let version_width = VERSION.width() as u16;
        let reserved = if bottom.width >= version_width {
            frame.render_widget(
                Paragraph::new(Span::styled(VERSION, dim())).right_aligned(),
                bottom,
            );
            version_width.saturating_add(1)
        } else {
            0
        };
        let mut line = match git {
            Some(summary) => git_line(summary),
            None => Line::from(vec![
                Span::styled(format!("  {folder}"), dim()),
                Span::styled(" \u{00B7} not a git repository", dim()),
            ]),
        };
        // First, so a narrow terminal cuts the folder rather than the warning.
        let color = match approvals {
            ApprovalPolicy::Ask => None,
            ApprovalPolicy::FullAccess => Some(NEGATIVE),
            _ => Some(ACCENT),
        };
        if let Some(color) = color {
            let label = format!(" {} ", approvals.name());
            line.spans.insert(0, Span::styled(label, bold(color)));
        }
        frame.render_widget(
            Paragraph::new(line),
            Rect {
                width: bottom.width.saturating_sub(reserved),
                ..bottom
            },
        );
    }

    /// The first request of a tool waiting for the user's yes, with its choices.
    fn draw_approval(&mut self, frame: &mut Frame, area: Rect) {
        self.app.approval_hits.clear();
        let Some(asked) = self.app.approvals.first() else {
            return;
        };
        if area.height < 4 || area.width < 20 {
            return;
        }
        let waiting = self.app.approvals.len();
        let title = match waiting {
            1 => " approval ".to_owned(),
            _ => format!(" approval 1 of {waiting} "),
        };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(fg(ACCENT))
            .title(Span::styled(title, bold(DUSK)));
        let inner = block.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);
        let width = inner.width as usize;
        let mut lines = vec![Line::from(Span::styled(
            wrap::clip(&format!(" {}", asked.title()), width),
            bold(TEXT),
        ))];
        for (row, (decision, label)) in asked.choices().into_iter().enumerate() {
            let here = asked.cursor == row;
            let pointer = if here { " \u{203A} " } else { "   " };
            let style = match (here, decision) {
                (true, Decision::Deny) => bold(NEGATIVE),
                (true, _) => bold(ACCENT),
                (false, _) => Style::new(),
            };
            let y = inner.y + 1 + row as u16;
            if y < inner.bottom() {
                self.app.approval_hits.push(ApprovalHit {
                    row: y,
                    left: inner.x,
                    right: inner.right(),
                    choice: row,
                });
            }
            lines.push(Line::from(Span::styled(
                wrap::clip(&format!("{pointer}{label}"), width),
                style,
            )));
        }
        lines.push(Line::from(Span::styled(
            "  up/down choose  enter answers  esc denies",
            dim(),
        )));
        frame.render_widget(Paragraph::new(lines), inner);
    }

    /// The question the agent asked: options to choose, the user's own answer, and buttons.
    fn draw_poll(&mut self, frame: &mut Frame, area: Rect) {
        let cursor_allowed = self.app.screensaver.is_none();
        let Some(poll) = &mut self.app.poll else {
            return;
        };
        poll.hits.clear();
        if area.height < 4 || area.width < 20 {
            return;
        }
        let border = if poll.active { ACCENT } else { HAIRLINE };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(fg(border))
            .title(Span::styled(" question ", bold(DUSK)));
        let inner = block.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);
        let width = inner.width as usize;
        let mut lines = vec![Line::from(Span::styled(
            wrap::clip(&format!(" {}", poll.question.question), width),
            bold(TEXT),
        ))];
        let own_row = poll.own_row();
        let visible_rows = inner.height.saturating_sub(1) as usize;
        for row in (0..=own_row).take(visible_rows) {
            let here = poll.active && poll.cursor == row;
            let pointer = if here { " \u{203A} " } else { "   " };
            let (mark, text) = if row < own_row {
                let mark = match (poll.question.multiple, poll.checked[row]) {
                    (true, true) => "[x] ",
                    (true, false) => "[ ] ",
                    (false, _) if here => "(\u{2022}) ",
                    (false, _) => "( ) ",
                };
                (mark, poll.question.options[row].clone())
            } else {
                let own = if poll.own.text.is_empty() && !here {
                    "your own answer".to_owned()
                } else {
                    poll.own.text.clone()
                };
                ("\u{270E}  ", own)
            };
            let style = if here { bold(ACCENT) } else { Style::new() };
            let label = wrap::clip(&format!("{pointer}{mark}{text}"), width);
            poll.hits.push(PollHit {
                row: inner.y + 1 + row as u16,
                left: inner.x,
                right: inner.right(),
                target: PollTarget::Row(row),
            });
            lines.push(Line::from(Span::styled(label, style)));
        }
        let hint = if poll.question.multiple {
            "space marks  enter sends  tab to the box  esc declines"
        } else {
            "enter sends  tab to the box  esc declines"
        };
        let buttons_row = inner.y + lines.len() as u16;
        let send_left = inner.x + 1;
        let decline_left = send_left + SEND.width() as u16 + 1;
        if buttons_row < inner.bottom() {
            poll.hits.push(PollHit {
                row: buttons_row,
                left: send_left,
                right: send_left + SEND.width() as u16,
                target: PollTarget::Send,
            });
            poll.hits.push(PollHit {
                row: buttons_row,
                left: decline_left,
                right: decline_left + DECLINE.width() as u16,
                target: PollTarget::Decline,
            });
        }
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(SEND, fg(ACCENT).bg(RAISED)),
            Span::raw(" "),
            Span::styled(DECLINE, fg(NEGATIVE).bg(RAISED)),
            Span::styled(format!("  {hint}"), dim()),
        ]));
        frame.render_widget(Paragraph::new(lines), inner);
        if poll.active && poll.cursor == own_row && own_row < visible_rows && cursor_allowed {
            let column = 6 + poll.own.text[..poll.own.cursor].width();
            let x = (inner.x as usize + column).min(inner.right().saturating_sub(1) as usize);
            frame.set_cursor_position(Position::new(x as u16, inner.y + 1 + own_row as u16));
        }
    }

    /// The messages waiting for the turn to end, each with its buttons; the newest are shown
    /// when there are more than fit.
    /// The standing goal in a line: its state, the round and the text, then its buttons.
    fn draw_goal(&mut self, frame: &mut Frame, area: Rect) {
        self.app.goal_hits.clear();
        if area.is_empty() {
            return;
        }
        let Some(goal) = &self.app.goal else {
            return;
        };
        let color = match goal.status {
            _ if !goal.approved => ACCENT,
            GoalStatus::Paused => ACCENT,
            GoalStatus::Active | GoalStatus::Done => TEXT_SOFT,
        };
        let state = match goal.status {
            _ if !goal.approved && goal.criteria.is_empty() => "drafting criteria",
            _ if !goal.approved => "waiting for approval",
            _ => goal.verification.as_str(),
        };
        let name = if goal.approved {
            goal.status.name()
        } else {
            "draft"
        };
        let head = format!(
            "   \u{25CE} goal {name} [{state}]  {}/{}  ",
            goal.rounds, self.app.goal_rounds
        );
        let offered = crate::app::goal_buttons(goal);
        let buttons: usize = offered
            .iter()
            .map(|button| button.label().width() + 1)
            .sum();
        let room = (area.width as usize).saturating_sub(head.width() + buttons + 1);
        let text = wrap::clip(&goal.text.replace('\n', " "), room);
        let gap = room.saturating_sub(text.width()) + 1;
        let mut left = area.x + (head.width() + text.width() + gap) as u16;
        let mut spans = vec![
            Span::styled(head, bold(color)),
            Span::raw(text),
            Span::raw(" ".repeat(gap)),
        ];
        for &button in offered {
            let label = button.label();
            let tint = match button {
                GoalButton::Clear => NEGATIVE,
                GoalButton::Approve => POSITIVE,
                _ => color,
            };
            self.app.goal_hits.push(GoalHit {
                row: area.y,
                left,
                right: left + label.width() as u16,
                button,
            });
            spans.push(Span::styled(label, fg(tint).bg(RAISED)));
            spans.push(Span::raw(" "));
            left += label.width() as u16 + 1;
        }
        let line = Line::from(spans);
        frame.render_widget(Paragraph::new(line), area);
    }

    fn draw_queue(&mut self, frame: &mut Frame, area: Rect) {
        self.app.queue_hits.clear();
        let skip = self.app.queued.len().saturating_sub(area.height as usize);
        const SEND: &str = " send now ";
        const REMOVE: &str = " \u{00D7} ";
        let buttons = SEND.width() + REMOVE.width() + 1;
        let mut lines = Vec::new();
        for (index, text) in self.app.queued.iter().enumerate().skip(skip) {
            let row = area.y + lines.len() as u16;
            let label = format!("   \u{29D7} {}", paste::labelled(text).replace('\n', " "));
            let room = (area.width as usize).saturating_sub(buttons);
            let label = wrap::clip(&label, room);
            let gap = room.saturating_sub(label.width());
            let send_left = area.x + (label.width() + gap) as u16;
            let remove_left = send_left + SEND.width() as u16 + 1;
            let hits = [
                (area.x, send_left, QueueButton::Edit),
                (
                    send_left,
                    send_left + SEND.width() as u16,
                    QueueButton::SendNow,
                ),
                (
                    remove_left,
                    remove_left + REMOVE.width() as u16,
                    QueueButton::Remove,
                ),
            ];
            for (left, right, button) in hits {
                self.app.queue_hits.push(QueueHit {
                    row,
                    left,
                    right,
                    index,
                    button,
                });
            }
            lines.push(Line::from(vec![
                Span::styled(label, thought()),
                Span::raw(" ".repeat(gap)),
                Span::styled(SEND, fg(ACCENT).bg(HAIRLINE)),
                Span::raw(" "),
                Span::styled(REMOVE, fg(NEGATIVE).bg(HAIRLINE)),
            ]));
        }
        frame.render_widget(Paragraph::new(lines).style(Style::new().bg(RAISED)), area);
    }

    /// What the folder of the path being typed holds, folders first.
    pub fn draw_paths(&mut self, frame: &mut Frame, above: Rect) {
        let Some(popup) = &self.app.paths else {
            return;
        };
        let entries = &popup.found.entries;
        let selected = popup.selected.unwrap_or(0);
        let skip = (selected + 1).saturating_sub(PATH_ROWS);
        let more = entries.len().saturating_sub(skip + PATH_ROWS);
        let mut lines: Vec<Line> = entries
            .iter()
            .enumerate()
            .skip(skip)
            .take(PATH_ROWS)
            .map(|(index, entry)| {
                let chosen = popup.selected == Some(index);
                let slash = if entry.directory { "/" } else { "" };
                let style = match (chosen, entry.directory) {
                    (true, _) => bold(ACCENT).bg(RAISED),
                    (false, true) => bold(TEXT),
                    (false, false) => Style::new(),
                };
                Line::from(Span::styled(format!("  {}{slash}  ", entry.name), style))
            })
            .collect();
        if more > 0 || popup.found.partial {
            let note = match (popup.found.partial, more) {
                (true, 0) => "  large folder; type to narrow  ".to_owned(),
                (true, _) => format!("  {more}+ more; type to narrow  "),
                (false, _) => format!("  {more} more  "),
            };
            lines.push(Line::from(Span::styled(note, dim())));
        }
        let width = lines
            .iter()
            .map(Line::width)
            .max()
            .unwrap_or(0)
            .max(24)
            .min(above.width.saturating_sub(2) as usize) as u16
            + 2;
        let height = (lines.len() as u16 + 2).min(above.height);
        let area = Rect {
            x: above.x + 1,
            y: above.bottom().saturating_sub(height),
            width,
            height,
        };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(fg(HAIRLINE))
            .title(Span::styled(" tab completes ", dim()));
        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new(lines).block(block), area);
    }

    pub fn draw_commands(&mut self, frame: &mut Frame, above: Rect) {
        self.app.command_popup = None;
        let text = self
            .app
            .input
            .command_cycle
            .as_ref()
            .map_or(self.app.input.text.as_str(), |(prefix, _)| prefix.as_str());
        if !text.starts_with('/') || text.contains(char::is_whitespace) {
            return;
        }
        let matching: Vec<_> = commands()
            .filter(|(name, _)| name.starts_with(text))
            .collect();
        if matching.is_empty() {
            return;
        }
        let width = matching
            .iter()
            .map(|(name, about)| name.width() + about.width() + 6)
            .max()
            .unwrap_or(0)
            .min(above.width.saturating_sub(1) as usize) as u16;
        let height = matching.len() as u16 + 2;
        let area = Rect {
            x: above.x + 1,
            y: above.bottom().saturating_sub(height),
            width,
            height: height.min(above.height),
        };
        self.app.command_popup = Some(area);
        let selected = self
            .app
            .input
            .command_cycle
            .as_ref()
            .map_or(0, |(_, selected)| *selected);
        let visible = area.height.saturating_sub(2) as usize;
        let skip = (selected + 1).saturating_sub(visible);
        let lines: Vec<Line> = matching
            .iter()
            .enumerate()
            .skip(skip)
            .take(visible)
            .map(|(index, (name, about))| {
                let selected = self
                    .app
                    .input
                    .command_cycle
                    .as_ref()
                    .is_some_and(|(_, selected)| *selected == index);
                Line::from(vec![
                    Span::styled(
                        format!(" {}{name}", if selected { "> " } else { "  " }),
                        if selected {
                            bold(ACCENT).bg(RAISED)
                        } else {
                            bold(TEXT)
                        },
                    ),
                    Span::styled(format!("  {about}"), dim()),
                ])
            })
            .collect();
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(fg(HAIRLINE));
        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new(lines).block(block), area);
    }
}

/// Rows the box needs: the queued messages, the rule, the padding and the wrapped text.
pub fn height(app: &App, width: u16) -> u16 {
    let (rows, _) = layout(&app.input, text_width(width));
    approval_rows(app)
        + poll_rows(app)
        + goal_rows(app)
        + queue_rows(app)
        + rows.len().clamp(MIN_ROWS, MAX_ROWS) as u16
        + FRAME_ROWS
        + 1
}

/// The border, the request, its choices and the hint.
fn approval_rows(app: &App) -> u16 {
    app.approvals
        .first()
        .map_or(0, |asked| asked.choices().len() as u16 + 4)
}

/// The question's border, its text, the options, the own answer and the buttons.
fn poll_rows(app: &App) -> u16 {
    app.poll
        .as_ref()
        .map_or(0, |poll| poll.question.options.len() as u16 + 5)
}

fn goal_rows(app: &App) -> u16 {
    u16::from(app.goal.is_some())
}

fn queue_rows(app: &App) -> u16 {
    app.queued.len().min(MAX_QUEUED) as u16
}

/// The chat's mode as a cell of its own colour, amber for MAIN and violet for the
/// others; Tab switches it.
fn mode_chip(mode: &str) -> Span<'static> {
    let color = if mode == "MAIN" { ACCENT } else { DUSK };
    Span::styled(format!(" {} ", mode.to_lowercase()), bold(DEEP).bg(color))
}

/// The repository, branch, commits, changed files and the distance to the upstream.
fn git_line(summary: &GitSummary) -> Line<'static> {
    let dot = || Span::styled(" \u{00B7} ", dim());
    let mut spans = vec![
        Span::styled(format!("  {}", summary.repository), dim()),
        dot(),
        Span::styled(
            summary.branch.clone().unwrap_or_else(|| "detached".into()),
            fg(TEXT_SOFT),
        ),
        dot(),
        Span::styled(format!("{} commits", summary.commits), dim()),
        dot(),
    ];
    spans.push(match summary.changed {
        0 => Span::styled("clean", dim()),
        changed => Span::styled(format!("{changed} changed"), fg(TEXT_SOFT)),
    });
    if let Some((ahead, behind)) = summary.ahead_behind {
        spans.push(dot());
        spans.push(Span::styled(
            format!("\u{2191}{ahead} \u{2193}{behind}"),
            if ahead + behind == 0 {
                dim()
            } else {
                fg(TEXT_SOFT)
            },
        ));
    }
    Line::from(spans)
}

const SEND: &str = " send ";
const DECLINE: &str = " decline ";

/// The command list above the box while a command is being typed.
/// Rows of a folder listing above the box; more wait behind the last one.
const PATH_ROWS: usize = 8;

/// The rule: what the agent is doing and how far the transcript is scrolled.
/// What goes on the card's top edge, and the note for its right end.
fn status(app: &App) -> (Vec<Span<'static>>, String) {
    let doing = match (&app.activity, app.focus) {
        (Activity::Idle, Focus::Browse(_)) => {
            vec![Span::styled("up/down move  enter fold  esc back", dim())]
        }
        (Activity::Idle, Focus::Input) => {
            let state = if app.provider_available {
                "ready"
            } else {
                "provider unavailable"
            };
            let saved = if app.local_unsent.is_empty() {
                String::new()
            } else {
                format!("  {} unsent \u{00B7} /resend", app.local_unsent.len())
            };
            vec![Span::styled(
                format!("{state}{saved}  /  F2 sidebar"),
                dim(),
            )]
        }
        (activity, _) => {
            let (color, what) = match activity {
                Activity::Thinking => (ACCENT, "thinking".to_owned()),
                Activity::Writing => (ACCENT, "writing".to_owned()),
                Activity::Tool(name) => (TEXT_SOFT, format!("running {name}")),
                Activity::Compacting => (DUSK, "compacting the context".to_owned()),
                Activity::Forking => (DUSK, "describing the thread for a fork".to_owned()),
                Activity::Retrying(attempt) => {
                    (NEGATIVE, format!("retrying after attempt {attempt}"))
                }
                Activity::Idle => (TEXT_MUTED, String::new()),
            };
            vec![
                Span::styled(format!("{} ", spinner(app)), fg(color)),
                Span::styled(what, fg(color)),
            ]
        }
    };
    let below = match (&app.flash, app.scroll) {
        (Some(flash), _) => flash.clone(),
        (None, 0) => String::new(),
        (None, scroll) => format!("{scroll} lines below"),
    };
    let mut spans = vec![Span::raw(" ")];
    if !app.mode.is_empty() {
        spans.push(mode_chip(&app.mode));
        spans.push(Span::raw("  "));
    }
    spans.extend(doing);
    spans.push(Span::raw(" "));
    (spans, below)
}

fn show_cursor(frame: &mut Frame, app: &App, area: Rect, (column, row): (usize, usize)) {
    let polling = app.poll.as_ref().is_some_and(|poll| poll.active);
    if app.panel.is_none() && app.focus == Focus::Input && !polling && app.screensaver.is_none() {
        let x = area.x + PROMPT.width() as u16 + column as u16;
        frame.set_cursor_position(Position::new(x, area.y + row as u16));
    }
}

fn text_width(width: u16) -> usize {
    (width as usize)
        .saturating_sub(PROMPT.width() + 1 + 2 * SIDE as usize)
        .max(1)
}

/// A row of the box: its spans, where its chips are as (from, to, paste index), and where
/// each character starts as (column, byte offset), up to the offset that ends the row.
#[derive(Default)]
pub struct Row {
    pub spans: Vec<Span<'static>>,
    chips: Vec<(usize, usize, usize)>,
    cells: Vec<(usize, usize)>,
    end: usize,
    plain: String,
    plain_selected: bool,
    width: usize,
}

impl Row {
    fn flush(&mut self) {
        if !self.plain.is_empty() {
            let text = std::mem::take(&mut self.plain);
            self.spans.push(match self.plain_selected {
                true => Span::styled(text, Style::new().bg(SELECTED)),
                false => Span::raw(text),
            });
        }
    }

    fn push_plain(&mut self, c: char, selected: bool) {
        if selected != self.plain_selected {
            self.flush();
            self.plain_selected = selected;
        }
        self.plain.push(c);
    }
}

/// The text broken into rows of `width` columns, chips drawn as their labels, the
/// selection marked, and the cursor as (column, row).
pub fn layout(input: &Input, width: usize) -> (Vec<Row>, (usize, usize)) {
    let selection = input.selection();
    let mut rows = vec![Row::default()];
    let mut at = (0, 0);
    for (offset, c) in input.text.char_indices() {
        if offset == input.cursor {
            at = (rows.last().map_or(0, |row| row.width), rows.len() - 1);
        }
        let selected = selection.is_some_and(|(from, to)| offset >= from && offset < to);
        if c == '\n' {
            if let Some(row) = rows.last_mut() {
                row.end = offset;
            }
            rows.push(Row::default());
            continue;
        }
        let chip = paste::chip_index(c).and_then(|index| {
            let pasted = input.pastes.get(index)?;
            Some((
                index,
                wrap::clip(&format!("[{}]", paste::label(pasted)), width),
            ))
        });
        let char_width = match &chip {
            Some((_, label)) => label.width(),
            None => c.width().unwrap_or(0),
        };
        if rows
            .last()
            .is_some_and(|row| row.width + char_width > width)
        {
            if let Some(row) = rows.last_mut() {
                row.end = offset;
            }
            rows.push(Row::default());
        }
        let Some(row) = rows.last_mut() else {
            continue;
        };
        row.cells.push((row.width, offset));
        match chip {
            Some((index, label)) => {
                row.flush();
                row.chips.push((row.width, row.width + char_width, index));
                let style = fg(TEXT_SOFT).bg(if selected { SELECTED } else { RAISED });
                row.spans.push(Span::styled(label, style));
            }
            None => row.push_plain(c, selected),
        }
        row.width += char_width;
    }
    if input.cursor >= input.text.len() {
        at = (rows.last().map_or(0, |row| row.width), rows.len() - 1);
    }
    if let Some(row) = rows.last_mut() {
        row.end = input.text.len();
    }
    for row in &mut rows {
        row.flush();
    }
    (rows, at)
}
