use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::tui::app::{Action, App, Channel};
use crate::tui::theme;
use crate::tui::ui;

const DELETE_BTN_W: u16 = 5;

pub fn render(app: &mut App, frame: &mut Frame, area: Rect) {
    let rows = Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).split(area);
    render_list(app, frame, rows[0]);
    render_footer(app, frame, rows[1]);
    if app.preset_delete_confirm.is_some() {
        render_delete_confirm(app, frame, area);
    }
    if app.preset_save_editing {
        render_save_popup(app, frame, area);
    }
}

fn render_list(app: &mut App, frame: &mut Frame, area: Rect) {
    let source = app
        .presets_source
        .as_deref()
        .map(|s| format!(" — {s}"))
        .unwrap_or_default();
    let block = theme::panel_block(&format!("Presets{source} — click to select"));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    app.presets_viewport_h = inner.height;

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    if app.presets_loading {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "  Loading catalog…",
                Style::default().fg(theme::TEXT_DIM),
            )),
            inner,
        );
        return;
    }

    if let Some(err) = &app.presets_error {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    format!("  {err}"),
                    Style::default().fg(theme::ERROR),
                )),
                Line::from(Span::styled(
                    "  Press R or Refresh to retry",
                    Style::default().fg(theme::TEXT_DIM),
                )),
            ]),
            inner,
        );
        return;
    }

    if app.preset_entries.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "  No presets loaded — press R or Refresh",
                Style::default().fg(theme::TEXT_DIM),
            )),
            inner,
        );
        return;
    }

    let rows: Vec<(String, String, bool)> = app
        .preset_entries
        .iter()
        .map(|e| (e.name.clone(), e.description.clone(), e.user))
        .collect();

    let content_h = rows.len() as u16;
    app.preset_scroll = ui::clamp_scroll(app.preset_scroll, content_h, inner.height);

    for (i, (name, description, user)) in rows.iter().enumerate() {
        let row_y = i as u16;
        let Some(rect) = ui::scrolled_row_rect(inner, app.preset_scroll, row_y, 1) else {
            continue;
        };
        render_preset_row(app, frame, rect, i, name, description, *user);
    }
}

fn render_preset_row(
    app: &mut App,
    frame: &mut Frame,
    rect: Rect,
    index: usize,
    name: &str,
    description: &str,
    user: bool,
) {
    let selected = index == app.sel_preset;
    let marker = if selected { "▸" } else { " " };
    let name_style = if selected {
        theme::focused_style().add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT)
    };

    let delete_w = if user {
        DELETE_BTN_W.min(rect.width.saturating_sub(6))
    } else {
        0
    };

    let cols = if delete_w > 0 {
        Layout::horizontal([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(delete_w),
        ])
        .split(rect)
    } else {
        Layout::horizontal([Constraint::Length(2), Constraint::Min(0)]).split(rect)
    };

    frame.render_widget(
        Paragraph::new(format!("{marker}")).style(Style::default().fg(theme::ACCENT)),
        cols[0],
    );

    let content = cols[1];
    let mut spans = vec![Span::styled(name.to_string(), name_style)];
    if user {
        let author = &app.preset_entries.get(index)
            .map(|e| e.preset.author.clone())
            .unwrap_or_default();
        let badge = if author.is_empty() {
            "  ✦ mine".to_string()
        } else {
            format!("  ✦ {author}")
        };
        spans.push(Span::styled(
            badge,
            Style::default()
                .fg(theme::SUCCESS)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if !description.is_empty() {
        spans.push(Span::styled(
            format!("  {description}"),
            Style::default().fg(theme::TEXT_DIM),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), content);
    app.push_click(content, Action::SelectPreset(index));

    if delete_w > 0 {
        ui::icon_button(
            frame,
            app,
            cols[2],
            "×",
            false,
            Action::RequestDeletePreset(index),
        );
    }
}

fn render_delete_confirm(app: &mut App, frame: &mut Frame, area: Rect) {
    let Some(index) = app.preset_delete_confirm else {
        return;
    };
    let Some(entry) = app.preset_entries.get(index) else {
        return;
    };

    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default().style(Style::default().bg(ratatui::style::Color::Rgb(10, 12, 18))),
        area,
    );

    let popup_w = 48u16.min(area.width.saturating_sub(4));
    let popup_h = 7u16.min(area.height.saturating_sub(2));
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(popup_w)) / 2,
        y: area.y + (area.height.saturating_sub(popup_h)) / 2,
        width: popup_w,
        height: popup_h,
    };
    let block = theme::panel_block("Confirm delete");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Delete ", Style::default().fg(theme::TEXT)),
            Span::styled(
                format!("\"{}\"", entry.name),
                Style::default()
                    .fg(theme::HIGHLIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ?", Style::default().fg(theme::TEXT)),
        ])),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new("This will remove the file from presets/user/")
            .style(Style::default().fg(theme::TEXT_DIM)),
        rows[1],
    );

    let btns = Layout::horizontal([
        Constraint::Length(12),
        Constraint::Length(2),
        Constraint::Length(12),
        Constraint::Min(0),
    ])
    .split(rows[2]);
    ui::button(
        frame,
        app,
        btns[0],
        "Delete",
        false,
        Action::ConfirmDeletePreset,
    );
    ui::button(
        frame,
        app,
        btns[2],
        "Cancel",
        false,
        Action::CancelDeletePreset,
    );

    frame.render_widget(
        Paragraph::new(Span::styled(
            "Y / Enter — delete  ·  N / Esc — cancel",
            Style::default().fg(theme::TEXT_DIM),
        )),
        rows[3],
    );
}

fn render_footer(app: &mut App, frame: &mut Frame, area: Rect) {
    let block = theme::panel_block("Actions");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let cols = Layout::horizontal([
        Constraint::Ratio(1, 5),
        Constraint::Ratio(1, 5),
        Constraint::Ratio(1, 5),
        Constraint::Ratio(1, 5),
        Constraint::Ratio(1, 5),
    ])
    .split(inner);

    ui::button(frame, app, cols[0], "→ A", false, Action::ApplyPreset(Channel::A));
    ui::button(frame, app, cols[1], "→ B", false, Action::ApplyPreset(Channel::B));
    ui::button(frame, app, cols[2], "Save A", false, Action::StartSavePreset(Channel::A));
    ui::button(frame, app, cols[3], "Save B", false, Action::StartSavePreset(Channel::B));
    ui::button(frame, app, cols[4], "Refresh", false, Action::RefreshPresets);
}

fn render_save_popup(app: &mut App, frame: &mut Frame, area: Rect) {
    use crate::tui::app::PresetSaveField;

    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default().style(Style::default().bg(ratatui::style::Color::Rgb(10, 12, 18))),
        area,
    );

    let popup_w = 56u16.min(area.width.saturating_sub(4));
    let popup_h = 9u16.min(area.height.saturating_sub(2));
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(popup_w)) / 2,
        y: area.y + (area.height.saturating_sub(popup_h)) / 2,
        width: popup_w,
        height: popup_h,
    };

    let title = format!("Save preset — channel {}", app.preset_save_channel.label());
    let block = theme::panel_block(title);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner);

    let name_focused = app.preset_save_field == PresetSaveField::Name;
    let nick_focused = app.preset_save_field == PresetSaveField::Nickname;
    let cursor = "▏";

    let name_val = if name_focused {
        format!("{}{cursor}", app.preset_save_input)
    } else {
        if app.preset_save_input.is_empty() {
            "(random)".to_string()
        } else {
            app.preset_save_input.clone()
        }
    };
    let name_style = if name_focused {
        theme::focused_style()
    } else {
        Style::default().fg(theme::TEXT)
    };
    frame.render_widget(
        Paragraph::new(format!(" Name     {name_val}")).style(name_style),
        rows[0],
    );

    let nick_val = if nick_focused {
        format!("{}{cursor}", app.preset_save_nickname)
    } else {
        if app.preset_save_nickname.is_empty() {
            "(no author)".to_string()
        } else {
            app.preset_save_nickname.clone()
        }
    };
    let nick_style = if nick_focused {
        theme::focused_style()
    } else {
        Style::default().fg(theme::TEXT)
    };
    frame.render_widget(
        Paragraph::new(format!(" Author   {nick_val}")).style(nick_style),
        rows[1],
    );

    frame.render_widget(Paragraph::new(""), rows[2]);

    let btns = Layout::horizontal([
        Constraint::Length(12),
        Constraint::Length(2),
        Constraint::Length(12),
        Constraint::Min(0),
    ])
    .split(rows[3]);

    ui::button(frame, app, btns[0], "Save", false, Action::CommitSavePreset);
    ui::button(frame, app, btns[2], "Cancel", false, Action::CancelSavePreset);

    frame.render_widget(
        Paragraph::new(Span::styled(
            " Tab — switch field  ·  Enter — save  ·  Esc — cancel",
            Style::default().fg(theme::TEXT_DIM),
        )),
        rows[4],
    );
}
