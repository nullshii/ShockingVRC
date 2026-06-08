use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::Paragraph;

use crate::tui::app::{Action, App};
use crate::tui::theme;
use crate::tui::ui;

pub fn render(app: &mut App, frame: &mut Frame, area: Rect) {
    let cols = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);
    render_osc(app, frame, cols[0]);
    render_connection(app, frame, cols[1]);
}

fn render_osc(app: &mut App, frame: &mut Frame, area: Rect) {
    let block = theme::panel_block("OSC — VRChat listener");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner);

    ui::header(frame, rows[0], "UDP port for incoming avatar parameters");

    let port_row = rows[1];
    ui::stepper_row(
        frame,
        app,
        port_row,
        &format!("Port  {}", app.osc_port),
        false,
        Action::StepOscPort(-1),
        Action::StepOscPort(1),
    );

    frame.render_widget(
        Paragraph::new("VRChat default: 9000  ·  this app: 9001  ·  range 1024–65535")
            .style(Style::default().fg(theme::TEXT_DIM)),
        rows[2],
    );
}

fn render_connection(app: &mut App, frame: &mut Frame, area: Rect) {
    let block = theme::panel_block("Connection status");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner);

    let vrc = if app.vrchat_found {
        "VRChat  found"
    } else {
        "VRChat  not found — enable OSC in game settings"
    };
    frame.render_widget(
        Paragraph::new(vrc).style(Style::default().fg(theme::TEXT)),
        rows[0],
    );

    let dev = if app.status.device_connected {
        "Coyote  connected"
    } else {
        "Coyote  searching…"
    };
    frame.render_widget(
        Paragraph::new(dev).style(Style::default().fg(theme::TEXT)),
        rows[1],
    );

    frame.render_widget(
        Paragraph::new(format!("Avatar zones discovered: {}", app.avatar_zones.len()))
            .style(Style::default().fg(theme::TEXT_DIM)),
        rows[2],
    );
}
