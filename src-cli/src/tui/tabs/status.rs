use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Gauge, List, ListItem, Paragraph};

use shocking_vrc_core::cli::ChannelStatus;
use shocking_vrc_core::raw_to_hz;

use crate::app_state::DeviceSlotInfo;
use crate::tui::app::{Action, App};
use crate::tui::theme;
use crate::tui::ui;

pub fn render(app: &mut App, frame: &mut Frame, area: Rect) {
    let multi_device = app.device_infos.len() > 1;
    let rows = if multi_device {
        let dev_panel_h = (app.device_infos.len() as u16 * 3 + 2).min(12);
        Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(dev_panel_h),
            Constraint::Min(0),
        ])
        .split(area)
    } else {
        Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area)
    };

    render_indicators(app, frame, rows[0]);

    let channel_area = if multi_device {
        render_device_list(app, frame, rows[1]);
        rows[2]
    } else {
        rows[1]
    };

    let cols = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(channel_area);
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

    let active = app.device_infos.get(app.active_device);
    let connected = active.map(|d| d.connected).unwrap_or(app.status.device_connected);

    let cols = Layout::horizontal([
        Constraint::Length(18),
        Constraint::Min(14),
        Constraint::Length(18),
        Constraint::Length(10),
    ])
    .split(inner);

    frame.render_widget(
        Paragraph::new(coyote_status(connected, active.map(|d| d.name.as_str()))),
        cols[0],
    );

    if connected {
        ui::battery_gauge(frame, cols[1], app.active_battery());
    } else {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Battery ", Style::default().fg(theme::TEXT_DIM)),
                Span::styled("—", Style::default().fg(theme::TEXT_DIM)),
            ])),
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
}

fn coyote_status(connected: bool, name: Option<&str>) -> Line<'static> {
    if connected {
        let text = name
            .map(|n| format!(" {n} "))
            .unwrap_or_else(|| " online ".to_string());
        Line::from(vec![
            Span::styled("Coyote ", Style::default().fg(theme::TEXT_DIM)),
            Span::styled(
                text,
                Style::default()
                    .fg(Color::Black)
                    .bg(theme::SUCCESS)
                    .add_modifier(Modifier::BOLD),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled("Coyote ", Style::default().fg(theme::TEXT_DIM)),
            Span::styled(
                " offline ",
                Style::default()
                    .fg(Color::Black)
                    .bg(theme::ERROR)
                    .add_modifier(Modifier::BOLD),
            ),
        ])
    }
}

fn render_device_list(app: &mut App, frame: &mut Frame, area: Rect) {
    let block = theme::panel_block("Devices");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.device_infos.is_empty() {
        return;
    }

    let devices: Vec<DeviceSlotInfo> = app.device_infos.clone();
    let row_constraints: Vec<Constraint> = devices
        .iter()
        .map(|_| Constraint::Length(3))
        .collect();
    let rows = Layout::vertical(row_constraints).split(inner);

    for (i, (row_area, dev)) in rows.iter().zip(devices.iter()).enumerate() {
        render_device_row(app, frame, *row_area, i, dev);
    }
}

fn render_device_row(
    app: &mut App,
    frame: &mut Frame,
    area: Rect,
    index: usize,
    dev: &DeviceSlotInfo,
) {
    let active = index == app.active_device;
    let border_color = if active {
        theme::ACCENT
    } else {
        theme::ACCENT_DIM
    };

    let row_block = Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(if active {
            theme::SURFACE_ELEVATED
        } else {
            theme::SURFACE
        }));
    let inner = row_block.inner(area);
    frame.render_widget(row_block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let cols = Layout::horizontal([
        Constraint::Length(4),
        Constraint::Length(16),
        Constraint::Length(11),
        Constraint::Min(16),
        Constraint::Length(13),
    ])
    .split(inner);

    let badge_style = if active {
        theme::active_tab_style()
    } else {
        Style::default().fg(theme::TEXT_DIM)
    };
    frame.render_widget(
        Paragraph::new(format!("D{}", index + 1)).style(badge_style),
        cols[0],
    );

    let name_style = if active {
        Style::default()
            .fg(theme::HIGHLIGHT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT).add_modifier(Modifier::BOLD)
    };
    frame.render_widget(
        Paragraph::new(dev.name.clone()).style(name_style),
        cols[1],
    );

    let short_mac = &dev.mac[dev.mac.len().saturating_sub(8)..];
    frame.render_widget(
        Paragraph::new(short_mac).style(Style::default().fg(theme::TEXT_DIM)),
        cols[2],
    );

    ui::battery_bar(frame, cols[3], dev.battery);

    let (status_text, status_bg) = if dev.reconnecting {
        (" reconnect ", theme::WARNING)
    } else if dev.connected {
        (" online ", theme::SUCCESS)
    } else {
        (" offline ", theme::ERROR)
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            status_text,
            Style::default()
                .fg(Color::Black)
                .bg(status_bg)
                .add_modifier(Modifier::BOLD),
        ))),
        cols[4],
    );

    app.push_click(area, Action::SwitchDevice(index));
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
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner);

    let pct = st.raw_level.clamp(0.0, 1.0) * 100.0;
    let input_label = Line::from(vec![
        Span::styled("Input ", Style::default().fg(theme::TEXT_DIM)),
        Span::styled(
            format!("{pct:.0}%"),
            Style::default()
                .fg(if pct > 0.0 { theme::SUCCESS } else { theme::TEXT_DIM })
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    frame.render_widget(Paragraph::new(input_label), rows[0]);

    let level_gauge = Gauge::default()
        .gauge_style(Style::default().fg(theme::SUCCESS).bg(theme::SURFACE_ELEVATED))
        .ratio(st.raw_level.clamp(0.0, 1.0) as f64)
        .label("");
    frame.render_widget(level_gauge, rows[1]);

    let str_pct = if limit_max > 0 {
        format!("{}%", (st.strength as f32 / limit_max as f32 * 100.0) as u32)
    } else {
        "0%".into()
    };
    let output_label = Line::from(vec![
        Span::styled("Power ", Style::default().fg(theme::TEXT_DIM)),
        Span::styled(
            format!("{}", st.strength),
            Style::default()
                .fg(if st.strength > 0 { theme::HIGHLIGHT } else { theme::TEXT_DIM })
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" / {limit_max}"),
            Style::default().fg(theme::TEXT_DIM),
        ),
        Span::styled(
            format!("  ({str_pct})"),
            Style::default().fg(theme::TEXT_DIM),
        ),
    ]);
    frame.render_widget(Paragraph::new(output_label), rows[2]);

    let denom = limit_max.max(1) as f64;
    let strength_ratio = (st.strength as f64 / denom).clamp(0.0, 1.0);
    let bar_color = theme::battery_gradient_color(1.0 - strength_ratio as f32);
    let strength_gauge = Gauge::default()
        .gauge_style(Style::default().fg(bar_color).bg(theme::SURFACE_ELEVATED))
        .ratio(strength_ratio)
        .label("");
    frame.render_widget(strength_gauge, rows[3]);

    frame.render_widget(Paragraph::new(""), rows[4]);

    frame.render_widget(
        Paragraph::new("Frequency").style(theme::section_header_style()),
        rows[5],
    );
    let freq_style = if st.strength > 0 {
        Style::default()
            .fg(theme::ACCENT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_DIM)
    };
    frame.render_widget(
        Paragraph::new(format_frequencies(st.frequency)).style(freq_style),
        rows[6],
    );

    frame.render_widget(
        Paragraph::new("Active zones").style(theme::section_header_style()),
        rows[7],
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
                    Span::styled("  ", Style::default()),
                    Span::styled(
                        format!("{id}"),
                        Style::default().fg(theme::TEXT),
                    ),
                    Span::styled(" ", Style::default()),
                    Span::styled(
                        format!("{:.0}%", lvl * 100.0),
                        Style::default()
                            .fg(theme::HIGHLIGHT)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]))
            })
            .collect()
    };
    frame.render_widget(List::new(items), rows[8]);
}
