use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Gauge, List, ListItem, Paragraph};

use shocking_vrc_core::cli::ChannelStatus;
use shocking_vrc_core::raw_to_hz;

use crate::tui::app::{Action, App};
use crate::tui::theme;
use crate::tui::ui;

pub fn render(app: &mut App, frame: &mut Frame, area: Rect) {
    let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);
    render_indicators(app, frame, rows[0]);

    let cols = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);

    let a = app.status.channel_a.clone();
    let b = app.status.channel_b.clone();
    let lim_a = app.config.channel_a.limits.max;
    let lim_b = app.config.channel_b.limits.max;
    render_channel(frame, cols[0], "Channel A", &a, lim_a);
    render_channel(frame, cols[1], "Channel B", &b, lim_b);
}

fn render_indicators(app: &mut App, frame: &mut Frame, area: Rect) {
    let block = theme::panel_block("Overview");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let cols = Layout::horizontal([
        Constraint::Percentage(18),
        Constraint::Percentage(22),
        Constraint::Percentage(18),
        Constraint::Percentage(12),
        Constraint::Percentage(30),
    ])
    .split(inner);

    frame.render_widget(
        Paragraph::new(coyote_status(app.status.device_connected)),
        cols[0],
    );

    if app.status.device_connected {
        ui::battery_gauge(frame, cols[1], app.device_battery);
    } else {
        frame.render_widget(
            Paragraph::new(Span::styled("—", Style::default().fg(theme::TEXT_DIM))),
            cols[1],
        );
    }

    frame.render_widget(Paragraph::new(indicator_line("VRChat", app.vrchat_found)), cols[2]);

    let zones = Line::from(vec![
        Span::styled("Zones ", Style::default().fg(theme::TEXT_DIM)),
        Span::styled(
            app.avatar_zones.len().to_string(),
            Style::default()
                .fg(theme::HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    frame.render_widget(Paragraph::new(zones), cols[3]);

    let label = if app.monitor {
        "Monitor ON"
    } else {
        "Monitor OFF"
    };
    ui::choice_button(
        frame,
        app,
        cols[4],
        label,
        app.monitor,
        false,
        Action::ToggleMonitor,
    );
}

fn coyote_status(connected: bool) -> Line<'static> {
    let (text, bg) = if connected {
        (" online ", theme::SUCCESS)
    } else {
        (" offline ", theme::ERROR)
    };
    Line::from(vec![
        Span::styled("Coyote ", Style::default().fg(theme::TEXT_DIM)),
        Span::styled(
            text,
            Style::default()
                .fg(Color::Black)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

fn indicator_line(name: &str, on: bool) -> Line<'static> {
    let (text, bg) = if on {
        (" online ", theme::SUCCESS)
    } else {
        (" offline ", theme::ERROR)
    };
    Line::from(vec![
        Span::styled(format!("{name} "), Style::default().fg(theme::TEXT_DIM)),
        Span::styled(
            text,
            Style::default()
                .fg(Color::Black)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

fn format_frequencies(freq: [u8; 4]) -> String {
    format!(
        "s0 {:>3.0} Hz  ·  s1 {:>3.0} Hz  ·  s2 {:>3.0} Hz  ·  s3 {:>3.0} Hz",
        raw_to_hz(freq[0]),
        raw_to_hz(freq[1]),
        raw_to_hz(freq[2]),
        raw_to_hz(freq[3]),
    )
}

fn render_channel(frame: &mut Frame, area: Rect, title: &str, st: &ChannelStatus, limit_max: u8) {
    let block = theme::panel_block(format!("{title} — live output"));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner);

    frame.render_widget(
        Paragraph::new(format!("Input level  {:.1}%", st.raw_level.clamp(0.0, 1.0) * 100.0))
            .style(Style::default().fg(theme::TEXT_DIM)),
        rows[0],
    );
    let level_gauge = Gauge::default()
        .gauge_style(Style::default().fg(theme::SUCCESS).bg(theme::SURFACE))
        .ratio(st.raw_level.clamp(0.0, 1.0) as f64)
        .label("");
    frame.render_widget(level_gauge, rows[1]);

    frame.render_widget(
        Paragraph::new(format!("Output strength  {} / {limit_max}", st.strength))
            .style(Style::default().fg(theme::TEXT_DIM)),
        rows[2],
    );
    let denom = limit_max.max(1) as f64;
    let strength_gauge = Gauge::default()
        .gauge_style(Style::default().fg(theme::HIGHLIGHT).bg(theme::SURFACE))
        .ratio((st.strength as f64 / denom).clamp(0.0, 1.0))
        .label("");
    frame.render_widget(strength_gauge, rows[3]);

    frame.render_widget(
        Paragraph::new("Output frequency").style(theme::section_header_style()),
        rows[4],
    );
    let freq_style = if st.strength > 0 {
        Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_DIM)
    };
    frame.render_widget(
        Paragraph::new(format_frequencies(st.frequency)).style(freq_style),
        rows[5],
    );

    frame.render_widget(
        Paragraph::new("Active zones").style(theme::section_header_style()),
        rows[6],
    );

    let items: Vec<ListItem> = if st.active_zones.is_empty() {
        vec![ListItem::new(Span::styled(
            "  none — waiting for touch",
            Style::default().fg(theme::TEXT_DIM),
        ))]
    } else {
        st.active_zones
            .iter()
            .map(|(id, lvl)| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("  {id} "), Style::default().fg(theme::TEXT)),
                    Span::styled(
                        format!("{:.0}%", lvl * 100.0),
                        Style::default().fg(theme::HIGHLIGHT),
                    ),
                ]))
            })
            .collect()
    };
    frame.render_widget(List::new(items), rows[7]);
}
