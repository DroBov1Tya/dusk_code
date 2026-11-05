//! /update: the release on offer and its install.

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use dusk_protocol::update::InstallStage;

use crate::app::{App, Panel};
use crate::ui::windows::{
    theme::{ACCENT, DONE, DUSK, HAIRLINE, RAISED, TEXT, bold, dim, fg},
    wrap,
};

pub const UPDATE_BUTTON: &str = " update ";

/// The running version, the newest on the channel, and a button to install it.
pub fn draw_update(frame: &mut Frame, screen: Rect, app: &mut App) {
    let Some(Panel::Update {
        info,
        note,
        progress,
        button,
    }) = &mut app.panel
    else {
        return;
    };
    *button = None;
    let row = |label: &str, value: Span<'static>| {
        Line::from(vec![Span::styled(format!(" {label:<10}"), dim()), value])
    };
    let width = screen.width.saturating_sub(4).min(84);
    // Text wraps inside the border and one column of air on each side.
    let room = width.saturating_sub(4) as usize;
    let mut lines = Vec::new();
    let mut button_line = None;
    if let Some(info) = info {
        let latest = match info.prerelease {
            true => format!("{} (pre-release)", info.latest),
            false => info.latest.clone(),
        };
        let behind = match (info.newer, info.commits_behind) {
            (false, _) => "nothing; this is as new or newer".to_owned(),
            (true, Some(commits)) => format!("{commits} commits"),
            (true, None) => "unknown".to_owned(),
        };
        let channel = |name: &str| {
            let style = match name == info.channel.name() {
                true => bold(ACCENT).bg(RAISED),
                false => dim(),
            };
            Span::styled(format!(" {name} "), style)
        };
        lines.push(row("running", Span::raw(info.current.clone())));
        lines.push(row("newest", Span::styled(latest, bold(TEXT))));
        lines.push(row("behind", Span::raw(behind)));
        lines.push(Line::from(vec![
            Span::styled(format!(" {:<10}", "channel"), dim()),
            channel("stable"),
            Span::raw(" "),
            channel("dev"),
        ]));
        lines.push(Line::default());
        if info.newer && !info.ready {
            let waiting = format!(
                "{} is tagged, but its build for this machine is not on GitHub yet: \
                 release builds take a few minutes after a tag. Try again shortly.",
                info.latest
            );
            lines.extend(
                wrap::words(vec![Span::styled(waiting, fg(ACCENT))], room, 0)
                    .into_iter()
                    .map(|spans| Line::from([vec![Span::raw(" ")], spans].concat())),
            );
        } else if info.newer && progress.is_none() {
            button_line = Some(lines.len());
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(UPDATE_BUTTON, bold(HAIRLINE).bg(ACCENT)),
                Span::styled(format!("  to {}", info.latest), dim()),
            ]));
        } else if !info.newer {
            lines.push(Line::from(Span::styled(
                " up to date on this channel",
                dim(),
            )));
        }
    }
    if let Some(progress) = progress {
        let stage = match progress.stage {
            InstallStage::Preparing => "preparing update",
            InstallStage::Downloading => "downloading",
            InstallStage::Verifying => "verifying checksum",
            InstallStage::Installing => "installing update",
            InstallStage::Complete => "installed",
        };
        let marker = if progress.stage == InstallStage::Complete {
            DONE
        } else {
            crate::ui::frame::TITLE_SPINNER[app.frame % crate::ui::frame::TITLE_SPINNER.len()]
        };
        lines.push(Line::from(Span::styled(
            format!(" {marker} {stage}"),
            bold(TEXT),
        )));
        if progress.stage == InstallStage::Downloading {
            let bytes = match progress.total.filter(|total| *total > 0) {
                Some(total) => {
                    let fraction = (progress.downloaded as f64 / total as f64).clamp(0.0, 1.0);
                    let bar_width = room.saturating_sub(2).min(32);
                    let filled = (fraction * bar_width as f64) as usize;
                    lines.push(Line::from(Span::styled(
                        format!(
                            " [{}{}]",
                            "\u{2588}".repeat(filled),
                            "\u{2591}".repeat(bar_width - filled)
                        ),
                        fg(ACCENT),
                    )));
                    format!(
                        "{:.0}%  {} / {total} bytes",
                        fraction * 100.0,
                        progress.downloaded
                    )
                }
                None => format!("{} bytes received; total unknown", progress.downloaded),
            };
            lines.extend(
                wrap::words(vec![Span::styled(bytes, dim())], room, 0)
                    .into_iter()
                    .map(|spans| Line::from([vec![Span::raw(" ")], spans].concat())),
            );
        }
    }
    if let Some(note) = note {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        // The whole reason, wrapped: the window grows to hold it.
        for line in note.lines() {
            lines.extend(
                wrap::words(vec![Span::styled(line.to_owned(), fg(ACCENT))], room, 0)
                    .into_iter()
                    .map(|spans| Line::from([vec![Span::raw(" ")], spans].concat())),
            );
        }
    }
    let height = (lines.len() as u16 + 2).min(screen.height);
    let area = Rect {
        x: screen.x + (screen.width - width) / 2,
        y: screen.y + (screen.height - height) / 3,
        width,
        height,
    };
    if let Some(at) = button_line {
        let left = area.x + 2;
        *button = Some((
            area.y + 1 + at as u16,
            left,
            left + UPDATE_BUTTON.width() as u16,
        ));
    }
    let help = match progress.as_ref().map(|progress| progress.stage) {
        Some(InstallStage::Complete) => " y copy  c channel  esc close ",
        Some(_) => " update in progress; please wait ",
        None => " enter update  y copy  c channel  esc close ",
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(HAIRLINE))
        .title(Line::from(Span::styled(" update ", bold(DUSK))))
        .title_bottom(Line::from(Span::styled(help, dim())).right_aligned());
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}
