//! The left rail. Statistics always have a place; agent and MCP sections appear only when
//! their event sources have supplied something to show.
//!
//! `draw` here shares the rail's rows between the sections; each section draws itself in
//! a file of its own (`statistics`, `tasks`, `jobs`, `agents`, `connectors`), from the
//! pieces in `section`.

mod agents;
mod connectors;
mod jobs;
mod section;
mod statistics;
mod tasks;

use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Clear},
};

use super::theme::DEEP;
use crate::app::App;
use jobs::MAX_JOB_ROWS;

/// The statistics: eleven lines and a blank one below.
const STATS_HEIGHT: u16 = 12;
/// The connectors folded: the heading and one line that sums them up.
const CONNECTORS_FOLDED: u16 = 2;
/// A compact bottom panel, kept the same height when connectors fold.
const CONNECTORS_HEIGHT: u16 = 5;

/// The rail beside the thread: statistics, tasks, background jobs, agents and connectors.
pub struct Sidebar<'a> {
    pub app: &'a mut App,
}

impl Sidebar<'_> {
    pub fn draw(&mut self, frame: &mut Frame, area: Rect, overlay: bool) {
        if overlay {
            frame.render_widget(Clear, area);
        }
        frame.render_widget(Block::default().style(Style::new().bg(DEEP)), area);
        if area.width < 4 || area.height < 3 {
            return;
        }
        let stats_height = STATS_HEIGHT.min(area.height);
        self.statistics(
            frame,
            Rect {
                height: stats_height,
                ..area
            },
        );
        let mut y = area.y + stats_height;
        let remaining = area.bottom() - y;
        // Dock connectors before allocating the content above them. Their geometry
        // depends only on the rail, never on agents, tasks, or their folded state.
        let mcp_section = if !self.app.mcp.is_empty() && remaining >= CONNECTORS_FOLDED {
            let height = CONNECTORS_HEIGHT.min(remaining.div_ceil(2).max(CONNECTORS_FOLDED));
            Some(Rect {
                y: area.bottom() - height,
                height,
                ..area
            })
        } else {
            None
        };
        // Leave one blank row above the dock when the rail has room for it.
        let content_bottom = mcp_section
            .map(|section| section.y.saturating_sub(1).max(y))
            .unwrap_or(area.bottom());
        if self.app.tasks.iter().any(|task| !task.done) && y < content_bottom {
            let remaining = content_bottom - y;
            // The tasks take what they need, up to half of what is left.
            let room = remaining.div_ceil(2).max(3).min(remaining);
            y += self.tasks(
                frame,
                Rect {
                    y,
                    height: room,
                    ..area
                },
            );
        }
        let running_jobs = self.app.jobs.iter().filter(|job| job.running).count();
        if running_jobs > 0 && y + 1 < content_bottom {
            let wanted = 1 + running_jobs.min(MAX_JOB_ROWS + 1) as u16;
            let height = wanted.min(content_bottom - y);
            y += self.background(frame, Rect { y, height, ..area }) + 1;
        }
        if !self.app.subagents.is_empty() && y < content_bottom {
            let section = Rect {
                y,
                height: content_bottom - y,
                ..area
            };
            self.app.agent_area = Some(section);
            self.agents(frame, section);
        }
        if let Some(section) = mcp_section {
            self.app.mcp_area = Some(section);
            self.mcp(frame, section);
        }
    }
}
