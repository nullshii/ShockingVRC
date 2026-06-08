use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::Paragraph;

use crate::tui::app::{
    mod_controls_len, mod_function_list_for, mod_kind_name, mod_slot_index, mod_source_list,
    Action, App, Channel, ModKind, ModParam,
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
    if app.mod_function_picker {
        render_function_picker(app, frame, cols[1]);
    } else {
        render_editor_panel(app, frame, cols[1]);
    }
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

fn render_function_picker(app: &mut App, frame: &mut Frame, area: Rect) {
    let block = theme::panel_block("Function — pick from list");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    ui::clear_panel_inner(frame, inner);

    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(inner);
    frame.render_widget(
        Paragraph::new(" ↑↓ move · Enter apply · Esc cancel · click row")
            .style(Style::default().fg(theme::TEXT_DIM)),
        rows[0],
    );

    let list = mod_function_list_for(&app.mod_editor.function);
    let count = list.len() as u16;
    app.mod_func_pick_viewport = rows[1].height;
    app.mod_func_pick_scroll = ui::clamp_scroll(
        app.mod_func_pick_scroll,
        count,
        app.mod_func_pick_viewport,
    );
    app.mod_func_pick_scroll = ui::scroll_to_row(
        app.mod_func_pick_scroll,
        app.mod_func_pick_viewport,
        app.mod_func_pick_ix as u16,
    );

    let scroll = app.mod_func_pick_scroll;
    for (i, func) in list.iter().enumerate() {
        let Some(rect) = ui::scrolled_row_rect(rows[1], scroll, i as u16, 1) else {
            continue;
        };
        let selected = i == app.mod_func_pick_ix;
        let current = func == &app.mod_editor.function;
        let prefix = if selected { "▸ " } else { "  " };
        let label = format!("{prefix}{func}");
        let style = if selected {
            theme::focused_style()
        } else if current {
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT)
        };
        frame.render_widget(Paragraph::new(label).style(style), rect);
        app.push_click(rect, Action::PickModFunction(i));
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
        render_function_field(app, frame, rect, &editor, focus == 0);
    }
    if let Some(rect) = ui::scrolled_row_rect(inner, scroll, 1, 1) {
        render_source_field(app, frame, rect, &editor, focus == 1);
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

fn render_function_field(
    app: &mut App,
    frame: &mut Frame,
    area: Rect,
    editor: &shocking_vrc_core::modulation::config::ModulationConfig,
    focused: bool,
) {
    let cols = Layout::horizontal([
        Constraint::Length(11),
        Constraint::Min(8),
        Constraint::Length(11),
        Constraint::Length(1),
    ])
    .split(area);

    frame.render_widget(
        Paragraph::new("Function").style(theme::label_style(focused)),
        cols[0],
    );

    let value = editor.function.to_string();
    frame.render_widget(
        Paragraph::new(value).style(
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        cols[1],
    );
    app.push_click(cols[1], Action::OpenModFunctionPicker);

    ui::button(
        frame,
        app,
        cols[2],
        "Pick list",
        focused,
        Action::OpenModFunctionPicker,
    );
}

fn render_source_field(
    app: &mut App,
    frame: &mut Frame,
    area: Rect,
    editor: &shocking_vrc_core::modulation::config::ModulationConfig,
    focused: bool,
) {
    let cols = Layout::horizontal([
        Constraint::Length(13),
        Constraint::Length(9),
        Constraint::Length(9),
        Constraint::Length(9),
        Constraint::Length(9),
    ])
    .split(area);

    frame.render_widget(
        Paragraph::new("Source").style(theme::label_style(focused)),
        cols[0],
    );

    for (i, src) in mod_source_list().iter().enumerate() {
        let label = src.to_string();
        let selected = editor.source == *src;
        ui::choice_button(
            frame,
            app,
            cols[i + 1],
            &label,
            selected,
            focused && selected,
            Action::SetModSource(*src),
        );
    }
}
