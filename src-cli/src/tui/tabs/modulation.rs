use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::Paragraph;

use crate::tui::app::{
    mod_controls_len, mod_kind_name, mod_slot_index, Action, App, Channel, ModKind, ModParam,
};
use crate::tui::preview::{render_modulation_preview, sample_modulation_curve};
use crate::tui::theme;
use crate::tui::ui;

const MOD_SLOTS: u16 = 16;

pub fn render(app: &mut App, frame: &mut Frame, area: Rect) {
    let cols = Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(area);
    app.mod_split_x = cols[1].x;
    render_slots(app, frame, cols[0]);
    render_editor_panel(app, frame, cols[1]);
}

fn render_slots(app: &mut App, frame: &mut Frame, area: Rect) {
    let block = theme::panel_block("Modulation slots — click to edit");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    ui::clear_panel_inner(frame, inner);

    app.mod_slots_viewport_h = inner.height;
    app.mod_slots_scroll = ui::clamp_scroll(app.mod_slots_scroll, MOD_SLOTS, inner.height);
    app.mod_slots_scroll = ui::scroll_to_row(
        app.mod_slots_scroll,
        inner.height,
        mod_slot_index(app.mod_channel, app.mod_kind, app.mod_seg),
    );

    let cfg = app.config.clone();
    let scroll = app.mod_slots_scroll;
    let mut logical_y = 0u16;

    for ch in [Channel::A, Channel::B] {
        let ch_cfg = match ch {
            Channel::A => &cfg.channel_a,
            Channel::B => &cfg.channel_b,
        };
        for kind in [ModKind::Freq, ModKind::Intensity] {
            let arr = match kind {
                ModKind::Freq => &ch_cfg.freq_modulation,
                ModKind::Intensity => &ch_cfg.intensity_modulation,
            };
            for seg in 0..4 {
                let Some(rect) = ui::scrolled_row_rect(inner, scroll, logical_y, 1) else {
                    logical_y += 1;
                    continue;
                };
                let status = match &arr[seg] {
                    Some(c) => format!("{} · {}", c.function, c.source),
                    None => "off".to_string(),
                };
                let selected =
                    app.mod_channel == ch && app.mod_kind == kind && app.mod_seg == seg;
                let label = format!(
                    " {}-{}[{}]: {}",
                    ch.label(),
                    mod_kind_name(kind),
                    seg,
                    status
                );
                let style = if selected {
                    theme::focused_style()
                } else if arr[seg].is_some() {
                    Style::default().fg(theme::SUCCESS)
                } else {
                    Style::default().fg(theme::TEXT_DIM)
                };
                frame.render_widget(Paragraph::new(label).style(style), rect);
                app.push_click(rect, Action::SelectModSlot(ch, kind, seg));
                logical_y += 1;
            }
        }
    }
}

fn render_editor_panel(app: &mut App, frame: &mut Frame, area: Rect) {
    if area.height > 12 {
        let preview_h = area.height.saturating_sub(12).min(18).max(6);
        let rows = Layout::vertical([
            Constraint::Min(4),
            Constraint::Length(preview_h),
        ])
        .split(area);
        render_editor(app, frame, rows[0]);
        let base = app.mod_preview_base();
        let samples = sample_modulation_curve(&app.mod_editor, base);
        render_modulation_preview(frame, rows[1], &samples, base, app.mod_kind);
    } else {
        render_editor(app, frame, area);
    }
}

fn render_editor(app: &mut App, frame: &mut Frame, area: Rect) {
    let title = format!(
        "Editor — Channel {} {} segment {}",
        app.mod_channel.label(),
        mod_kind_name(app.mod_kind),
        app.mod_seg
    );
    let block = theme::panel_block(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    ui::clear_panel_inner(frame, inner);

    let editor_rows = mod_controls_len() as u16;
    app.mod_editor_viewport_h = inner.height;
    app.mod_editor_scroll =
        ui::clamp_scroll(app.mod_editor_scroll, editor_rows, inner.height);
    app.mod_editor_scroll = ui::scroll_to_row(
        app.mod_editor_scroll,
        inner.height,
        app.mod_focus as u16,
    );

    let focus = app.mod_focus;
    let editor = app.mod_editor.clone();
    let scroll = app.mod_editor_scroll;

    if let Some(rect) = ui::scrolled_row_rect(inner, scroll, 0, 1) {
        cycler_row(
            frame,
            app,
            rect,
            "Function",
            &editor.function.to_string(),
            focus == 0,
            Action::CycleModFunction(-1),
            Action::CycleModFunction(1),
        );
    }
    if let Some(rect) = ui::scrolled_row_rect(inner, scroll, 1, 1) {
        cycler_row(
            frame,
            app,
            rect,
            "Source",
            &editor.source.to_string(),
            focus == 1,
            Action::CycleModSource(-1),
            Action::CycleModSource(1),
        );
    }

    for (i, param) in ModParam::ALL.iter().enumerate() {
        let row = 2 + i as u16;
        let Some(rect) = ui::scrolled_row_rect(inner, scroll, row, 1) else {
            continue;
        };
        let val = param.get(&editor);
        ui::stepper_row(
            frame,
            app,
            rect,
            &format!("{:<13} {:.3}", param.label(), val),
            focus == 2 + i,
            Action::StepModParam(*param, -1),
            Action::StepModParam(*param, 1),
        );
    }

    let base = 2 + ModParam::ALL.len();
    if let Some(rect) = ui::scrolled_row_rect(inner, scroll, base as u16, 1) {
        let btns = Layout::horizontal([
            Constraint::Length(12),
            Constraint::Length(1),
            Constraint::Length(12),
            Constraint::Length(1),
            Constraint::Length(16),
        ])
        .split(rect);
        ui::button(frame, app, btns[0], "Apply", focus == base, Action::ApplyMod);
        ui::button(frame, app, btns[2], "Clear slot", focus == base + 1, Action::ClearMod);
        ui::button(
            frame,
            app,
            btns[4],
            "Clear channel",
            focus == base + 2,
            Action::ClearAllMod(app.mod_channel),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn cycler_row(
    frame: &mut Frame,
    app: &mut App,
    area: Rect,
    name: &str,
    value: &str,
    focused: bool,
    prev: Action,
    next: Action,
) {
    let cols = Layout::horizontal([
        Constraint::Length(13),
        Constraint::Length(4),
        Constraint::Min(6),
        Constraint::Length(4),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new(name.to_string()).style(theme::label_style(focused)),
        cols[0],
    );
    ui::button(frame, app, cols[1], "◀", false, prev);
    frame.render_widget(
        Paragraph::new(value.to_string()).style(
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        cols[2],
    );
    ui::button(frame, app, cols[3], "▶", false, next);
}
