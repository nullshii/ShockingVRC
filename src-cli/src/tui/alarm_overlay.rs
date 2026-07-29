use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Clear, Gauge, Paragraph};

use super::app::{format_secs, Action, App};
use super::theme;
use super::ui;

pub fn render(app: &mut App, frame: &mut Frame, area: Rect) {
    let st = app.alarm_status;

    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default().style(Style::default().bg(Color::Rgb(30, 8, 10))),
        area,
    );

    let popup_w = 54u16.min(area.width.saturating_sub(4));
    let popup_h = 11u16.min(area.height.saturating_sub(2));
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(popup_w)) / 2,
        y: area.y + (area.height.saturating_sub(popup_h)) / 2,
        width: popup_w,
        height: popup_h,
    };

    let title = if st.test { " Alarm — test ring " } else { " Alarm " };
    let block = theme::panel_block(title);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    ui::clear_panel_inner(frame, inner);

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner);

    let cfg = app.alarm_config();
    frame.render_widget(
        Paragraph::new(format!("WAKE UP — {}", cfg.time_label()))
            .style(
                Style::default()
                    .fg(theme::ERROR)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center),
        rows[0],
    );

    let peak = cfg.peak_strength.max(1) as f64;
    frame.render_widget(
        Paragraph::new(format!(
            "Power {} / {}   ·   ringing for {}",
            st.strength,
            cfg.peak_strength,
            format_secs(st.elapsed_secs as i32)
        ))
        .style(Style::default().fg(theme::TEXT))
        .alignment(Alignment::Center),
        rows[1],
    );

    frame.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(theme::ERROR).bg(theme::SURFACE_ELEVATED))
            .ratio((st.strength as f64 / peak).clamp(0.0, 1.0))
            .label(""),
        rows[2],
    );

    let tail = if st.attempt < cfg.repeats {
        format!(
            "Attempt {}/{} — retries in {} min if you do not answer",
            st.attempt.max(1),
            cfg.repeats,
            cfg.snooze_mins
        )
    } else {
        format!(
            "Attempt {}/{} — gives up in {}",
            st.attempt.max(1),
            cfg.repeats,
            format_secs(st.auto_stop_in_secs as i32)
        )
    };
    frame.render_widget(
        Paragraph::new(tail)
            .style(Style::default().fg(theme::TEXT_DIM))
            .alignment(Alignment::Center),
        rows[3],
    );

    if app.device_infos.is_empty() {
        frame.render_widget(
            Paragraph::new("No device connected — nothing is being sent")
                .style(Style::default().fg(theme::WARNING))
                .alignment(Alignment::Center),
            rows[4],
        );
    }

    let btn_cols = Layout::horizontal([
        Constraint::Min(0),
        Constraint::Length(14),
        Constraint::Length(2),
        Constraint::Length(14),
        Constraint::Min(0),
    ])
    .split(rows[5]);
    ui::button(frame, app, btn_cols[1], "Stop", false, Action::AlarmStop);
    ui::button(
        frame,
        app,
        btn_cols[3],
        "Snooze",
        false,
        Action::AlarmSnooze,
    );

    frame.render_widget(
        Paragraph::new("Enter / Esc / Space — stop   ·   S — snooze")
            .style(theme::hint_style())
            .alignment(Alignment::Center),
        rows[6],
    );
}
