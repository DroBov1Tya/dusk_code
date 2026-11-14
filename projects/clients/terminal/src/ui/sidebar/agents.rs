//! The subagents, grouped by the zone of their work, each with its loader, name, time,
//! state and task.

use unicode_width::UnicodeWidthStr;

use dusk_protocol::agent::Zone;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

use super::{
    Sidebar,
    section::{PAD, ruled, scroll_limit, section},
};
use crate::ui::{
    loader::{Loader, agent_name, identity},
    text::{
        format::{format_duration, one_line},
        wrap,
    },
    theme::{ACCENT, DEEP, TEXT, TEXT_SOFT, bold, fg},
};

impl Sidebar<'_> {
    pub fn agents(&mut self, frame: &mut Frame, area: Rect) {
        let mut lines = Vec::new();
        let width = area.width.saturating_sub(8 + 2 * PAD) as usize;
        // When every agent does not fit in full, each takes two rows, so the last ones
        // still show what they work on. A blank row parts any two, or they run together.
        let zones = Zone::ALL
            .iter()
            .filter(|zone| {
                self.app
                    .subagents
                    .iter()
                    .any(|a| Zone::of(&a.agent) == **zone)
            })
            .count();
        let full_rows = self.app.subagents.len() * 4 - 1 + zones;
        let compact = full_rows > area.height.saturating_sub(1) as usize;
        // Agents stand in the zone of their work, those at work first.
        let mut order: Vec<&_> = self.app.subagents.iter().collect();
        order.sort_by_key(|a| {
            (
                Zone::ALL.iter().position(|z| *z == Zone::of(&a.agent)),
                !a.is_working(),
                a.started_at,
            )
        });
        let mut last_zone = None;
        for (index, agent) in order.iter().copied().enumerate() {
            let zone = Zone::of(&agent.agent);
            if last_zone != Some(zone) {
                last_zone = Some(zone);
                let at_work = order
                    .iter()
                    .filter(|a| Zone::of(&a.agent) == zone && a.is_working())
                    .count();
                let note = if at_work > 0 {
                    format!(" \u{00B7} {at_work} at work")
                } else {
                    String::new()
                };
                lines.push(Line::from(Span::styled(
                    format!(" {}{note}", zone.name()),
                    bold(TEXT_SOFT),
                )));
            }
            let seed = identity(&agent.thread_id);
            let loader = Loader::new(seed);
            let color = loader.color();
            let running = agent.is_working();
            let [top, middle, bottom] = loader.rows(running.then_some(self.app.frame));
            let name = agent_name(&agent.thread_id);
            let age = format_duration(agent.elapsed_ms());
            // The phase the agent is in, as it said; without one, whether it is still at work.
            let state = match (&agent.phase, agent.unavailable_at, running) {
                (_, Some(_), _) => "unavailable",
                (Some((phase, _)), _, _) => phase.name(),
                (None, _, true) => agent.doing.label(),
                (None, _, false) => "done",
            };
            // Room left by the loader, the time and the status, as wide as they are now:
            // the time grows past an hour.
            let loader_width: usize = middle.iter().map(|span| span.content.width()).sum();
            let used = loader_width + 1 + age.width() + 2 + state.width() + 2 * PAD as usize;
            let room = (area.width as usize).saturating_sub(used);
            let named = vec![
                Span::raw(" "),
                Span::styled(wrap::clip(&name, room), bold(color)),
                Span::styled(format!(" {age} "), fg(TEXT_SOFT)),
                Span::styled(state, fg(ACCENT)),
            ];
            if compact {
                lines.push(Line::from([middle, named].concat()));
                let task = format!("{} \u{00B7} {}", agent.agent, one_line(&agent.task));
                lines.push(Line::from(
                    [
                        bottom,
                        vec![
                            Span::raw(" "),
                            Span::styled(wrap::clip(&task, width), fg(TEXT)),
                        ],
                    ]
                    .concat(),
                ));
                if index + 1 < order.len() {
                    lines.push(Line::default());
                }
                continue;
            }
            // The random name stands out; the agent it works as and the task stay quiet.
            lines.push(Line::from(
                [
                    top,
                    vec![Span::raw(" ")],
                    vec![Span::styled(wrap::clip(&agent.agent, width), fg(TEXT_SOFT))],
                ]
                .concat(),
            ));
            lines.push(Line::from([middle, named].concat()));
            lines.push(Line::from(
                [
                    bottom,
                    vec![
                        Span::raw(" "),
                        Span::styled(wrap::clip(&one_line(&agent.task), width), fg(TEXT)),
                    ],
                ]
                .concat(),
            ));
            if index + 1 < order.len() {
                lines.push(Line::default());
            }
        }
        self.app.agent_max_scroll = scroll_limit(lines.len(), area);
        self.app.agent_scroll = self.app.agent_scroll.min(self.app.agent_max_scroll);
        let rule = area.width.saturating_sub(2 * PAD) as usize;
        let mut block = section(ruled("Subagents", None, rule));
        if self.app.agent_max_scroll > 0 && area.height > 2 {
            block =
                block.title_bottom(Line::from(Span::styled(" wheel to scroll ", fg(TEXT_SOFT))));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::new().fg(TEXT).bg(DEEP))
                .block(block)
                .scroll((self.app.agent_scroll as u16, 0)),
            area,
        );
    }
}
