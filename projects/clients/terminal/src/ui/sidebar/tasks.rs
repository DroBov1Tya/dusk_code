//! The tasks still open, each folded to its name and progress or unfolded to its steps.

use dusk_protocol::thread::Task;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

use super::{
    Sidebar,
    section::{GAUGE, PAD, fill, ruled, scroll_limit, section},
};
use crate::ui::{
    text::format::format_duration,
    theme::{
        ACCENT, DEEP, DONE, DOT, FOLDED, HAIRLINE, ON, POSITIVE, TEXT, TEXT_SOFT, UNFOLDED, bold,
        fg,
    },
};
use dusk_protocol::system::now_ms;

impl Sidebar<'_> {
    /// The tasks still open, each in a small block of its own: what it is and how many steps
    /// are done, and once unfolded its steps with how long each ran. Returns the rows used.
    pub fn tasks(&mut self, frame: &mut Frame, area: Rect) -> u16 {
        let width = area.width.saturating_sub(2 * PAD) as usize;
        let now = now_ms();
        // Every open task's rows, a blank row between tasks, each row with its task.
        let mut rows: Vec<(Line<'static>, Option<String>)> = Vec::new();
        for task in self.app.tasks.iter().filter(|task| !task.done) {
            if !rows.is_empty() {
                rows.push((Line::default(), None));
            }
            let unfolded = self.app.unfolded_tasks.contains(&task.task_id);
            for line in task_lines(task, unfolded, width, now) {
                rows.push((line, Some(task.task_id.clone())));
            }
        }
        self.app.task_max_scroll = scroll_limit(rows.len(), area);
        self.app.task_scroll = self.app.task_scroll.min(self.app.task_max_scroll);
        self.app.task_area = Some(area);
        let mut block = section(ruled("Tasks", None, width));
        let scrolls = self.app.task_max_scroll > 0 && area.height > 2;
        if scrolls {
            block =
                block.title_bottom(Line::from(Span::styled(" wheel to scroll ", fg(TEXT_SOFT))));
        }
        let inner = block.inner(area);
        let shown = rows
            .iter()
            .skip(self.app.task_scroll)
            .take(inner.height as usize);
        self.app.task_hits = shown
            .clone()
            .enumerate()
            .filter_map(|(row, (_, task_id))| {
                let task_id = task_id.clone()?;
                Some((
                    Rect {
                        y: inner.y + row as u16,
                        height: 1,
                        ..area
                    },
                    task_id,
                ))
            })
            .collect();
        let lines: Vec<Line> = shown.map(|(line, _)| line.clone()).collect();
        let used = 1 + lines.len() as u16 + u16::from(scrolls);
        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::new().fg(TEXT).bg(DEEP))
                .block(block),
            Rect {
                height: used.min(area.height),
                ..area
            },
        );
        // A blank row before the next section.
        (used + 1).min(area.height)
    }
}

fn task_lines(task: &Task, unfolded: bool, width: usize, now: i64) -> Vec<Line<'static>> {
    let finished = task.items.iter().filter(|item| item.done).count();
    let count = format!(" {finished}/{}", task.items.len());
    let fold = if unfolded { UNFOLDED } else { FOLDED };
    let mut lines = vec![Line::from(vec![
        Span::styled(format!("{fold} "), fg(ACCENT)),
        Span::styled(
            fill(&task.description, width.saturating_sub(count.len() + 2)),
            bold(TEXT),
        ),
        Span::styled(count, fg(TEXT_SOFT)),
    ])];
    // How far the task is, as a thin bar under its name.
    let bar = width.saturating_sub(2);
    if !task.items.is_empty() && bar > 0 {
        let done = bar * finished / task.items.len();
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(GAUGE.repeat(done), fg(POSITIVE)),
            Span::styled(GAUGE.repeat(bar - done), fg(HAIRLINE)),
        ]));
    }
    if !unfolded {
        return lines;
    }
    for item in &task.items {
        let (mark, color) = match (item.done, item.started_at) {
            (true, _) => (DONE, POSITIVE),
            (false, Some(_)) => (ON, ACCENT),
            (false, None) => (DOT, TEXT_SOFT),
        };
        // A running step counts up; a done one shows what it took.
        let time = item
            .started_at
            .map(|start| {
                let end = item.finished_at.unwrap_or(now);
                format!(
                    " {}",
                    format_duration(end.saturating_sub(start).max(0) as u64)
                )
            })
            .unwrap_or_default();
        lines.push(Line::from(vec![
            Span::styled(format!("{mark} "), fg(color)),
            Span::styled(
                fill(&item.description, width.saturating_sub(time.len() + 2)),
                fg(if item.done { TEXT_SOFT } else { TEXT }),
            ),
            Span::styled(time, fg(if item.done { TEXT_SOFT } else { ACCENT })),
        ]));
    }
    lines
}
