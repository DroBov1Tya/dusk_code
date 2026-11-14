//! The MCP servers docked at the bottom of the rail.

use dusk_protocol::config::McpState;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

use super::{
    Sidebar,
    section::{PAD, fill, ruled, scroll_limit, section},
};
use crate::ui::{
    text::{format::one_line, wrap},
    theme::{
        ACCENT, DEEP, FAILED, FOLDED, NEGATIVE, OFF, ON, POSITIVE, TEXT, TEXT_SOFT, UNFOLDED, bold,
        fg,
    },
};

impl Sidebar<'_> {
    /// The MCP servers. Folded, one line sums them up; unfolded, each server shows how it
    /// stands: its tools once ready, why it failed otherwise. A click folds and unfolds.
    pub fn mcp(&mut self, frame: &mut Frame, area: Rect) {
        let width = area.width.saturating_sub(2 * PAD) as usize;
        let mut lines = Vec::new();
        let mark = if self.app.connectors_open {
            UNFOLDED
        } else {
            FOLDED
        };
        let title = ruled("Connectors", Some(Span::styled(mark, fg(TEXT_SOFT))), width);
        if !self.app.connectors_open {
            let ready = self
                .app
                .mcp
                .iter()
                .filter(|server| matches!(server.state, McpState::Ready(_)))
                .count();
            let failed = self
                .app
                .mcp
                .iter()
                .filter(|server| matches!(server.state, McpState::Failed(_)))
                .count();
            let mut summary = vec![Span::styled(format!("{ON} {ready} ready"), fg(POSITIVE))];
            if failed > 0 {
                summary.push(Span::styled(
                    format!("  {FAILED} {failed} failed"),
                    fg(NEGATIVE),
                ));
            }
            let block = section(title);
            frame.render_widget(
                Paragraph::new(Line::from(summary))
                    .style(Style::new().fg(TEXT).bg(DEEP))
                    .block(block),
                area,
            );
            return;
        }
        for server in &self.app.mcp {
            let (mark, color, state) = match &server.state {
                McpState::Off => (OFF, TEXT_SOFT, "off".to_owned()),
                McpState::Starting => (OFF, ACCENT, "starting".to_owned()),
                McpState::Ready(tools) => {
                    let plural = if tools.len() == 1 { "" } else { "s" };
                    (ON, POSITIVE, format!("{} tool{plural}", tools.len()))
                }
                McpState::Failed(_) => (FAILED, NEGATIVE, "failed".to_owned()),
            };
            let room = width.saturating_sub(state.len() + 3);
            lines.push(Line::from(vec![
                Span::styled(format!("{mark} "), fg(color)),
                Span::styled(fill(&server.name, room), bold(TEXT)),
                Span::styled(format!(" {state}"), fg(TEXT_SOFT)),
            ]));
            if let McpState::Failed(error) = &server.state {
                lines.push(Line::from(Span::styled(
                    format!(
                        "   {}",
                        wrap::clip(&one_line(error), width.saturating_sub(3))
                    ),
                    fg(TEXT_SOFT),
                )));
            }
        }
        self.app.mcp_max_scroll = scroll_limit(lines.len(), area);
        self.app.mcp_scroll = self.app.mcp_scroll.min(self.app.mcp_max_scroll);
        let mut block = section(title);
        if self.app.mcp_max_scroll > 0 && area.height > 2 {
            block =
                block.title_bottom(Line::from(Span::styled(" wheel to scroll ", fg(TEXT_SOFT))));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::new().fg(TEXT).bg(DEEP))
                .block(block)
                .scroll((self.app.mcp_scroll as u16, 0)),
            area,
        );
    }
}
