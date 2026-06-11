use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;

use crate::tui::app::{Action, App};
use crate::tui::theme;
use crate::tui::ui;

pub fn render(app: &mut App, frame: &mut Frame, area: Rect) {
    let rows = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(4),
        Constraint::Length(3),
    ])
    .split(area);
    let cols = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(rows[0]);
    render_osc(app, frame, cols[0]);
    render_connection(app, frame, cols[1]);
    render_prefs(app, frame, rows[1]);
    render_tutorial_button(app, frame, rows[2]);
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

    render_port_row(app, frame, rows[1]);

    let hint = if app.osc_port_editing {
        "Editing: type digits, Enter apply, Esc cancel  ·  range 1024–65535"
    } else {
        "VRChat default: 9000  ·  this app: 9001  ·  click port or press Enter to type"
    };
    frame.render_widget(
        Paragraph::new(hint).style(Style::default().fg(theme::TEXT_DIM)),
        rows[2],
    );
}

fn render_port_row(app: &mut App, frame: &mut Frame, area: Rect) {
    let cols = Layout::horizontal([
        Constraint::Min(16),
        Constraint::Length(5),
        Constraint::Length(1),
        Constraint::Length(5),
    ])
    .split(area);

    let label = if app.osc_port_editing {
        format!("Port  {}{}", app.osc_port_input, "▏")
    } else {
        format!("Port  {}", app.osc_port)
    };
    frame.render_widget(
        Paragraph::new(label).style(
            if app.osc_port_editing {
                theme::focused_style()
            } else {
                theme::label_style(false)
            },
        ),
        cols[0],
    );
    app.push_click(cols[0], Action::StartOscPortEdit);

    ui::button(frame, app, cols[1], "−", false, Action::StepOscPort(-1));
    ui::button(frame, app, cols[3], "+", false, Action::StepOscPort(1));
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
        (
            Span::styled(
                " OK ",
                Style::default()
                    .fg(ratatui::style::Color::Black)
                    .bg(theme::SUCCESS)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            " Coyote connected",
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
            " Coyote searching…",
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

fn render_prefs(app: &mut App, frame: &mut Frame, area: Rect) {
    let block = theme::panel_block("Config — cli_config.json");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(inner);

    ui::header(frame, rows[0], "Auto-save ON / OFF");

    let cols = Layout::horizontal([
        Constraint::Length(14),
        Constraint::Length(6),
        Constraint::Length(6),
        Constraint::Min(0),
    ])
    .split(rows[1]);

    frame.render_widget(
        Paragraph::new("Auto-save").style(Style::default().fg(theme::TEXT_DIM)),
        cols[0],
    );
    ui::choice_button(
        frame,
        app,
        cols[1],
        "ON",
        app.auto_save,
        false,
        Action::SetAutoSave(true),
    );
    ui::choice_button(
        frame,
        app,
        cols[2],
        "OFF",
        !app.auto_save,
        false,
        Action::SetAutoSave(false),
    );
}

fn render_tutorial_button(app: &mut App, frame: &mut Frame, area: Rect) {
    let block = theme::panel_block("Tutorial");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let cols = Layout::horizontal([Constraint::Length(22), Constraint::Min(0)]).split(inner);
    ui::button(
        frame,
        app,
        cols[0],
        "Show Tutorial",
        false,
        Action::TutorialStart,
    );
    frame.render_widget(
        Paragraph::new("Re-open the interactive guide")
            .style(Style::default().fg(theme::TEXT_DIM)),
        cols[1],
    );
}
