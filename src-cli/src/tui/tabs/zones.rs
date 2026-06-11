use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::Paragraph;

use shocking_vrc_core::cli::{ContactMode, ZoneId};

use crate::tui::app::{Action, App, Channel, ZonesPane};
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

    let layout = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
    let list_area = layout[0];
    let footer = layout[1];

    let entries: Vec<(String, ZoneId)> = app
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
            (format!("{} · {}", e.id, mode_str), e.id.clone())
        })
        .collect();

    let selected = match ch {
        Channel::A => app.sel_conf_a,
        Channel::B => app.sel_conf_b,
    };

    render_rows(
        frame,
        app,
        list_area,
        &entries.iter().map(|(s, _)| s.clone()).collect::<Vec<_>>(),
        selected,
        focused,
        &|i| Action::SelectConfigured(ch, i),
    );

    let btns = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(footer);
    ui::button(frame, app, btns[0], "Cycle mode", false, Action::CycleMode(ch, selected));
    ui::button(frame, app, btns[1], "Remove", false, Action::RemoveZone(ch, selected));
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