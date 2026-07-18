use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use shocking_vrc_core::cli::ContactMode;

use crate::tui::app::{Action, App, Channel, SliderKind, ZonesPane};
use crate::tui::theme;
use crate::tui::ui;

pub fn render(app: &mut App, frame: &mut Frame, area: Rect) {
    let cols = Layout::horizontal([
        Constraint::Percentage(33),
        Constraint::Percentage(33),
        Constraint::Percentage(34),
    ])
    .split(area);
    render_configured(app, frame, cols[0], Channel::A);
    render_configured(app, frame, cols[1], Channel::B);
    render_avatar(app, frame, cols[2]);
}

fn render_configured(app: &mut App, frame: &mut Frame, area: Rect, ch: Channel) {
    let pane = match ch {
        Channel::A => ZonesPane::ConfiguredA,
        Channel::B => ZonesPane::ConfiguredB,
    };
    let focused = app.zones_pane == pane;
    let block = theme::panel_block(format!("Channel {} — mapped zones", ch.label()))
        .border_style(if focused {
            Style::default().fg(theme::ACCENT)
        } else {
            Style::default().fg(theme::ACCENT_DIM)
        });
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let layout = Layout::vertical([
        Constraint::Min(1),    // mapped-zone list
        Constraint::Length(1), // Cycle mode / Remove buttons
        Constraint::Length(1), // Scale slider
        Constraint::Length(1), // Min threshold slider
        Constraint::Length(1), // Max threshold slider
    ])
    .split(inner);
    let list_area = layout[0];
    let btn_row = layout[1];
    let slider_row_area = layout[2];

    let selected = match ch {
        Channel::A => app.sel_conf_a,
        Channel::B => app.sel_conf_b,
    };

    let entries: Vec<(String, u8)> = app
        .channel_config(ch)
        .zones
        .iter()
        .map(|e| {
            let mode_str = match e.mode {
                ContactMode::Depth => "depth",
                ContactMode::Speed => "speed",
                ContactMode::Acc => "acc",
                ContactMode::Recoil => "recoil",
            };
            let scale_badge = if e.scale != 100 {
                format!(" ·{}%", e.scale)
            } else {
                String::new()
            };
            let thr_badge = if e.min_threshold != 1 || e.max_threshold != 100 {
                format!(
                    " ·thr {:.2}-{:.2}",
                    e.min_threshold as f32 / 100.0,
                    e.max_threshold as f32 / 100.0
                )
            } else {
                String::new()
            };
            (
                format!("{} · {}{}{}", e.id, mode_str, scale_badge, thr_badge),
                e.scale,
            )
        })
        .collect();

    render_rows(
        frame,
        app,
        list_area,
        &entries.iter().map(|(s, _)| s.clone()).collect::<Vec<_>>(),
        selected,
        focused,
        &|i| Action::SelectConfigured(ch, i),
    );

    let btns = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(btn_row);
    ui::button(frame, app, btns[0], "Cycle mode", false, Action::CycleMode(ch, selected));
    ui::button(frame, app, btns[1], "Remove", false, Action::RemoveZone(ch, selected));


    let current_scale = entries.get(selected).map(|(_, s)| *s).unwrap_or(100);

    let label_cols = Layout::horizontal([
        Constraint::Length(7),
        Constraint::Min(8),
        Constraint::Length(5),
    ])
    .split(slider_row_area);

    let label_style = if focused {
        Style::default().fg(theme::HIGHLIGHT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_DIM)
    };

    frame.render_widget(
        Paragraph::new("Scale ").style(label_style),
        label_cols[0],
    );

    let ratio = current_scale as f64 / 100.0;
    let bar_color = scale_bar_color(current_scale);
    let gauge = ratatui::widgets::Gauge::default()
        .gauge_style(Style::default().fg(bar_color).bg(theme::SURFACE_ELEVATED))
        .ratio(ratio)
        .label("");
    frame.render_widget(gauge, label_cols[1]);
    app.push_slider(label_cols[1], SliderKind::ZoneScale(ch, selected));

    let val_style = if current_scale != 100 {
        Style::default()
            .fg(bar_color)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_DIM)
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("{:>3}%", current_scale),
            val_style,
        ))),
        label_cols[2],
    );

    // Threshold-range sliders (stored in hundredths, shown 0.01–1.00).
    let (thr_min, thr_max) = app
        .channel_config(ch)
        .zones
        .get(selected)
        .map(|e| (e.min_threshold, e.max_threshold))
        .unwrap_or((1, 100));

    threshold_row(
        frame,
        app,
        layout[3],
        "Min thr",
        thr_min,
        SliderKind::ZoneThresholdMin(ch, selected),
        focused,
    );
    threshold_row(
        frame,
        app,
        layout[4],
        "Max thr",
        thr_max,
        SliderKind::ZoneThresholdMax(ch, selected),
        focused,
    );
}

/// Render a single threshold slider row: label, gauge (0.01–1.00), and the
/// current value shown as a decimal in real time. Mirrors the Scale gauge so it
/// blends into the existing Zone menu styling.
#[allow(clippy::too_many_arguments)]
fn threshold_row(
    frame: &mut Frame,
    app: &mut App,
    area: Rect,
    label: &str,
    value: u8,
    kind: SliderKind,
    focused: bool,
) {
    let cols = Layout::horizontal([
        Constraint::Length(10),
        Constraint::Min(6),
        Constraint::Length(5),
    ])
    .split(area);

    let label_style = if focused {
        Style::default().fg(theme::HIGHLIGHT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_DIM)
    };
    frame.render_widget(Paragraph::new(label.to_string()).style(label_style), cols[0]);

    let bar_color = if focused { theme::HIGHLIGHT } else { theme::ACCENT };
    let gauge = ratatui::widgets::Gauge::default()
        .gauge_style(Style::default().fg(bar_color).bg(theme::SURFACE_ELEVATED))
        .ratio((value as f64 / 100.0).clamp(0.0, 1.0))
        .label("");
    frame.render_widget(gauge, cols[1]);
    app.push_slider(cols[1], kind);

    let val_style = Style::default().fg(bar_color).add_modifier(Modifier::BOLD);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("{:>4.2}", value as f32 / 100.0),
            val_style,
        ))),
        cols[2],
    );
}

fn scale_bar_color(scale: u8) -> ratatui::style::Color {
    use ratatui::style::Color;
    match scale {
        0 => Color::DarkGray,
        1..=29 => theme::ERROR,
        30..=59 => theme::WARNING,
        60..=89 => Color::Rgb(160, 210, 100),
        90..=99 => Color::Rgb(100, 200, 160),
        100 => theme::ACCENT,
        _ => theme::ACCENT,
    }
}

fn render_avatar(app: &mut App, frame: &mut Frame, area: Rect) {
    let focused = app.zones_pane == ZonesPane::Avatar;
    let block = theme::panel_block("Avatar — discovered touch zones")
        .border_style(if focused {
            Style::default().fg(theme::ACCENT)
        } else {
            Style::default().fg(theme::ACCENT_DIM)
        });
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let layout = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);

    let rows: Vec<String> = app
        .avatar_zones
        .iter()
        .map(|z| {
            let pct = (z.level * 100.0) as u32;
            let bar = if pct > 0 {
                format!("  {pct:>3}%")
            } else {
                "     ".into()
            };
            format!("{:<6} {}{bar}", z.zone_type, z.id)
        })
        .collect();

    render_rows(
        frame,
        app,
        layout[0],
        &rows,
        app.sel_avatar,
        focused,
        &Action::SelectAvatar,
    );

    let sel = app.sel_avatar;
    let add = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(layout[1]);
    ui::button(frame, app, add[0], "Add → A", false, Action::AddAvatarZone(Channel::A, sel));
    ui::button(frame, app, add[1], "Add → B", false, Action::AddAvatarZone(Channel::B, sel));

    let all = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(layout[2]);
    ui::button(frame, app, all[0], "Add all → A", false, Action::AddAllZones(Channel::A));
    ui::button(frame, app, all[1], "Add all → B", false, Action::AddAllZones(Channel::B));
}

fn render_rows(
    frame: &mut Frame,
    app: &mut App,
    area: Rect,
    rows: &[String],
    selected: usize,
    focused: bool,
    make_action: &dyn Fn(usize) -> Action,
) {
    if area.height == 0 {
        return;
    }
    let height = area.height as usize;
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new("  No zones yet — pick from avatar list →")
                .style(Style::default().fg(theme::TEXT_DIM)),
            area,
        );
        return;
    }
    let offset = selected.saturating_sub(height.saturating_sub(1));
    for (vis, idx) in (offset..rows.len()).enumerate() {
        if vis >= height {
            break;
        }
        let rect = Rect {
            x: area.x,
            y: area.y + vis as u16,
            width: area.width,
            height: 1,
        };
        let is_sel = idx == selected;
        let style = if is_sel && focused {
            theme::focused_style()
        } else if is_sel {
            theme::selected_style()
        } else {
            Style::default().fg(theme::TEXT)
        };
        frame.render_widget(Paragraph::new(format!(" {}", rows[idx])).style(style), rect);
        app.push_click(rect, make_action(idx));
    }
}
