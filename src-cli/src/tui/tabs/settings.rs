use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;

use crate::tui::app::{setup_controls, Action, App, SetupControl};
use crate::tui::theme;
use crate::tui::ui;

pub fn render(app: &mut App, frame: &mut Frame, area: Rect) {
    let rows = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(5),
        Constraint::Length(3),
    ])
    .split(area);
    let cols = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(rows[0]);
    let focus = setup_controls().get(app.setup_focus).copied();
    render_osc(app, frame, cols[0]);
    render_connection(app, frame, cols[1]);
    render_prefs(app, frame, rows[1], focus);
    render_tutorial_button(app, frame, rows[2], focus);
}

fn render_osc(app: &mut App, frame: &mut Frame, area: Rect) {
    let block = theme::panel_block("OSC — VRChat / OSCQuery");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner);

    ui::header(frame, rows[0], "Auto-discovered by VRChat via OSCQuery");

    let port_line = if app.osc_port == 0 {
        "Port  selecting…".to_string()
    } else if app.oscquery_port == Some(app.osc_port) {
        format!("Port  {}", app.osc_port)
    } else {
        format!("OSC (UDP)  {}  (fixed, legacy mode)", app.osc_port)
    };
    frame.render_widget(
        Paragraph::new(port_line).style(theme::label_style(false)),
        rows[1],
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

    let (vrc_badge, vrc_text) = if app.vrchat_found {
        (
            Span::styled(
                " OK ",
                Style::default()
                    .fg(ratatui::style::Color::Black)
                    .bg(theme::SUCCESS)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            " VRChat detected",
        )
    } else {
        (
            Span::styled(
                " !! ",
                Style::default()
                    .fg(ratatui::style::Color::Black)
                    .bg(theme::ERROR)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            " VRChat not found — enable OSC in game",
        )
    };
    frame.render_widget(
        Paragraph::new(ratatui::text::Line::from(vec![
            vrc_badge,
            Span::styled(vrc_text, Style::default().fg(theme::TEXT)),
        ])),
        rows[0],
    );

    let (dev_badge, dev_text) = if app.status.device_connected {
        let count = app.device_infos.len();
        let label = if count > 1 {
            format!(" {count} Coyotes connected")
        } else {
            " Coyote connected".to_string()
        };
        (
            Span::styled(
                " OK ",
                Style::default()
                    .fg(ratatui::style::Color::Black)
                    .bg(theme::SUCCESS)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            label,
        )
    } else {
        (
            Span::styled(
                " .. ",
                Style::default()
                    .fg(ratatui::style::Color::Black)
                    .bg(theme::WARNING)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            " Coyote searching…".to_string(),
        )
    };
    frame.render_widget(
        Paragraph::new(ratatui::text::Line::from(vec![
            dev_badge,
            Span::styled(dev_text, Style::default().fg(theme::TEXT)),
        ])),
        rows[1],
    );

    frame.render_widget(
        Paragraph::new(format!("     Avatar zones: {}", app.avatar_zones.len()))
            .style(Style::default().fg(theme::TEXT_DIM)),
        rows[2],
    );
}

fn render_prefs(app: &mut App, frame: &mut Frame, area: Rect, focus: Option<SetupControl>) {
    let block = theme::panel_block("Preferences");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);

    ui::header(frame, rows[0], "↑↓ select  ·  ←→ / Enter toggle");

    toggle_row(
        app,
        frame,
        rows[1],
        "Auto-save",
        app.auto_save,
        focus == Some(SetupControl::AutoSave),
        Action::SetAutoSave(true),
        Action::SetAutoSave(false),
        "Write device config to devices/ on every change",
    );

    let alarm_hint = if app.alarm_tab_visible {
        "Alarm tab is shown — press 9 to open it"
    } else {
        "Wake-up alarm: adds the Alarm tab (9)"
    };
    toggle_row(
        app,
        frame,
        rows[2],
        "Alarm tab",
        app.alarm_tab_visible,
        focus == Some(SetupControl::AlarmTab),
        Action::SetAlarmTabVisible(true),
        Action::SetAlarmTabVisible(false),
        alarm_hint,
    );
}

#[allow(clippy::too_many_arguments)]
fn toggle_row(
    app: &mut App,
    frame: &mut Frame,
    area: Rect,
    label: &str,
    on: bool,
    focused: bool,
    on_action: Action,
    off_action: Action,
    hint: &str,
) {
    let cols = Layout::horizontal([
        Constraint::Length(14),
        Constraint::Length(6),
        Constraint::Length(6),
        Constraint::Min(0),
    ])
    .split(area);

    frame.render_widget(
        Paragraph::new(label.to_string()).style(theme::label_style(focused)),
        cols[0],
    );
    ui::choice_button(frame, app, cols[1], "ON", on, focused, on_action);
    ui::choice_button(frame, app, cols[2], "OFF", !on, focused, off_action);
    frame.render_widget(
        Paragraph::new(format!("  {hint}")).style(Style::default().fg(theme::TEXT_DIM)),
        cols[3],
    );
}

fn render_tutorial_button(
    app: &mut App,
    frame: &mut Frame,
    area: Rect,
    focus: Option<SetupControl>,
) {
    let block = theme::panel_block("Tutorial");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let cols = Layout::horizontal([Constraint::Length(22), Constraint::Min(0)]).split(inner);
    ui::button(
        frame,
        app,
        cols[0],
        "Show Tutorial",
        focus == Some(SetupControl::Tutorial),
        Action::TutorialStart,
    );
    frame.render_widget(
        Paragraph::new("Re-open the interactive guide")
            .style(Style::default().fg(theme::TEXT_DIM)),
        cols[1],
    );
}
