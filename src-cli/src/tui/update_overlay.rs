use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Block, Clear, Gauge, Paragraph, Wrap};

use super::app::{Action, App};
use super::theme;
use super::ui;

pub fn render(app: &mut App, frame: &mut Frame, area: Rect) {
    let Some(release) = app.update_available.as_ref() else {
        return;
    };

    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default().style(Style::default().bg(ratatui::style::Color::Rgb(10, 12, 18))),
        area,
    );

    let has_error = app.update_error.is_some();
    let popup_w = 50u16.min(area.width.saturating_sub(4));
    let popup_h = if app.update_downloading {
        11
    } else if has_error {
        10
    } else {
        9
    };
    let popup_h = popup_h.min(area.height.saturating_sub(2));
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(popup_w)) / 2,
        y: area.y + (area.height.saturating_sub(popup_h)) / 2,
        width: popup_w,
        height: popup_h,
    };

    let title = if app.update_downloading {
        "Downloading update"
    } else if has_error {
        "Download failed"
    } else {
        "Update available"
    };
    let block = theme::panel_block(title);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(if app.update_downloading { 2 } else { 1 }),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);

    frame.render_widget(
        Paragraph::new(format!(
            "Version {} → {}",
            shocking_vrc_core::VERSION,
            release.tag
        ))
        .style(Style::default().fg(theme::TEXT)),
        rows[0],
    );

    if app.update_downloading {
        let progress = app
            .update_progress
            .unwrap_or(shocking_vrc_core::DownloadProgress {
                downloaded: 0,
                total: (release.asset_size > 0).then_some(release.asset_size),
            });
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(theme::ACCENT).bg(theme::SURFACE_ELEVATED))
            .ratio(progress.ratio())
            .label("");
        frame.render_widget(gauge, rows[1]);
        frame.render_widget(
            Paragraph::new(progress.label()).style(Style::default().fg(theme::TEXT_DIM)),
            rows[2],
        );
        frame.render_widget(
            Paragraph::new("The app will restart when the download finishes.")
                .style(Style::default().fg(theme::TEXT_DIM)),
            rows[3],
        );
    } else if let Some(err) = &app.update_error {
        frame.render_widget(
            Paragraph::new(err.clone())
                .style(Style::default().fg(theme::ERROR))
                .wrap(Wrap { trim: true }),
            rows[1],
        );
        frame.render_widget(
            Paragraph::new("Details are in the Log tab.")
                .style(Style::default().fg(theme::TEXT_DIM)),
            rows[2],
        );
        let btn_row = rows[3];
        let cols = Layout::horizontal([
            Constraint::Length(12),
            Constraint::Length(10),
            Constraint::Min(0),
        ])
        .split(btn_row);
        ui::button(frame, app, cols[0], "Retry", false, Action::ConfirmUpdate);
        ui::button(frame, app, cols[1], "Later", false, Action::DismissUpdate);
        frame.render_widget(
            Paragraph::new("Y/Enter retry · N/Esc later")
                .style(Style::default().fg(theme::TEXT_DIM)),
            cols[2],
        );
    } else {
        frame.render_widget(
            Paragraph::new("Install the latest release from GitHub?")
                .style(Style::default().fg(theme::TEXT_DIM)),
            rows[1],
        );
        frame.render_widget(
            Paragraph::new("Y / Enter — yes   ·   N / Esc — no   ·   S — skip this version")
                .style(Style::default().fg(theme::TEXT_DIM)),
            rows[2],
        );
        let btn_row = rows[3];
        let cols = Layout::horizontal([
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(16),
            Constraint::Min(0),
        ])
        .split(btn_row);
        ui::button(frame, app, cols[0], "Yes", false, Action::ConfirmUpdate);
        ui::button(frame, app, cols[1], "No", false, Action::DismissUpdate);
        ui::button(
            frame,
            app,
            cols[2],
            "Skip version",
            false,
            Action::SkipUpdateVersion,
        );
    }
}
