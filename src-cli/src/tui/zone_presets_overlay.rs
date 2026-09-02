use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::tui::app::{Action, App};
use crate::tui::theme;
use crate::tui::ui;

const DELETE_BTN_W: u16 = 3;

pub fn render(app: &mut App, frame: &mut Frame, area: Rect) {
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default().style(Style::default().bg(ratatui::style::Color::Rgb(10, 12, 18))),
        area,
    );

    let popup_w = 64u16.min(area.width.saturating_sub(4));
    let popup_h = 16u16.min(area.height.saturating_sub(2));
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(popup_w)) / 2,
        y: area.y + (area.height.saturating_sub(popup_h)) / 2,
        width: popup_w,
        height: popup_h,
    };

    let block = theme::panel_block("Zone sets — saved zone mappings");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.width == 0 || inner.height < 4 {
        return;
    }

    let rows = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);

    render_list(app, frame, rows[0]);
    render_action_row(app, frame, rows[2]);
    render_hints(app, frame, rows[3]);
}

fn render_list(app: &mut App, frame: &mut Frame, area: Rect) {
    app.zone_presets_viewport_h = area.height;

    if app.zone_presets.is_empty() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    "  No zone sets saved yet.",
                    Style::default().fg(theme::TEXT_DIM),
                )),
                Line::from(Span::styled(
                    "  Map the zones you want, then Save current.",
                    Style::default().fg(theme::TEXT_DIM),
                )),
            ]),
            area,
        );
        return;
    }

    let content_h = app.zone_presets.len() as u16;
    app.zone_preset_scroll = ui::clamp_scroll(app.zone_preset_scroll, content_h, area.height);

    let rows: Vec<(String, usize, usize)> = app
        .zone_presets
        .iter()
        .map(|e| {
            let (a, b) = e.preset.counts();
            (e.name().to_string(), a, b)
        })
        .collect();

    for (i, (name, a, b)) in rows.iter().enumerate() {
        let Some(rect) = ui::scrolled_row_rect(area, app.zone_preset_scroll, i as u16, 1) else {
            continue;
        };
        render_row(app, frame, rect, i, name, *a, *b);
    }
}

fn render_row(
    app: &mut App,
    frame: &mut Frame,
    rect: Rect,
    index: usize,
    name: &str,
    zones_a: usize,
    zones_b: usize,
) {
    let selected = index == app.sel_zone_preset;
    let marker = if selected { "▸" } else { " " };
    let name_style = if selected {
        theme::focused_style().add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT)
    };

    let delete_w = DELETE_BTN_W.min(rect.width.saturating_sub(6));
    let cols = Layout::horizontal([
        Constraint::Length(2),
        Constraint::Min(0),
        Constraint::Length(delete_w),
    ])
    .split(rect);

    frame.render_widget(
        Paragraph::new(marker).style(Style::default().fg(theme::ACCENT)),
        cols[0],
    );

    let count_style = if zones_a + zones_b == 0 {
        Style::default().fg(theme::WARNING)
    } else {
        Style::default().fg(theme::TEXT_DIM)
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(name.to_string(), name_style),
            Span::styled(format!("   A: {zones_a}  ·  B: {zones_b}"), count_style),
        ])),
        cols[1],
    );
    app.push_click(cols[1], Action::SelectZonePreset(index));

    if delete_w > 0 {
        ui::icon_button(
            frame,
            app,
            cols[2],
            "×",
            false,
            Action::RequestDeleteZonePreset(index),
        );
    }
}

fn render_action_row(app: &mut App, frame: &mut Frame, area: Rect) {
    if app.zone_preset_save_editing {
        render_save_row(app, frame, area);
        return;
    }
    if app.zone_preset_delete_confirm.is_some() {
        render_delete_row(app, frame, area);
        return;
    }

    let cols = Layout::horizontal([
        Constraint::Length(10),
        Constraint::Length(1),
        Constraint::Length(16),
        Constraint::Min(0),
        Constraint::Length(9),
    ])
    .split(area);

    if !app.zone_presets.is_empty() {
        ui::button(frame, app, cols[0], "Load", false, Action::ApplyZonePreset);
    }
    ui::button(
        frame,
        app,
        cols[2],
        "Save current",
        false,
        Action::StartSaveZonePreset,
    );
    ui::button(frame, app, cols[4], "Close", false, Action::CloseZonePresets);
}

fn render_save_row(app: &mut App, frame: &mut Frame, area: Rect) {
    let cols = Layout::horizontal([
        Constraint::Min(10),
        Constraint::Length(9),
        Constraint::Length(1),
        Constraint::Length(9),
    ])
    .split(area);

    let value = format!("{}▏", app.zone_preset_save_input);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" Name  ", Style::default().fg(theme::TEXT_DIM)),
            Span::styled(value, theme::focused_style()),
        ])),
        cols[0],
    );

    ui::button(frame, app, cols[1], "Save", false, Action::CommitSaveZonePreset);
    ui::button(frame, app, cols[3], "Cancel", false, Action::CancelSaveZonePreset);
}

fn render_delete_row(app: &mut App, frame: &mut Frame, area: Rect) {
    let name = app
        .zone_preset_delete_confirm
        .and_then(|i| app.zone_presets.get(i))
        .map(|e| e.name().to_string())
        .unwrap_or_default();

    let cols = Layout::horizontal([
        Constraint::Min(10),
        Constraint::Length(10),
        Constraint::Length(1),
        Constraint::Length(9),
    ])
    .split(area);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" Delete ", Style::default().fg(theme::TEXT)),
            Span::styled(
                format!("\"{name}\""),
                Style::default()
                    .fg(theme::HIGHLIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ?", Style::default().fg(theme::TEXT)),
        ])),
        cols[0],
    );

    ui::button(
        frame,
        app,
        cols[1],
        "Delete",
        false,
        Action::ConfirmDeleteZonePreset,
    );
    ui::button(
        frame,
        app,
        cols[3],
        "Cancel",
        false,
        Action::CancelDeleteZonePreset,
    );
}

fn render_hints(app: &mut App, frame: &mut Frame, area: Rect) {
    let hint = if app.zone_preset_save_editing {
        " Enter — save  ·  Esc — cancel  ·  blank name is auto-filled"
    } else if app.zone_preset_delete_confirm.is_some() {
        " Y / Enter — delete  ·  N / Esc — cancel"
    } else {
        " Enter — load  ·  s — save  ·  x — delete  ·  Esc — close"
    };
    frame.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(theme::TEXT_DIM))),
        area,
    );
}
