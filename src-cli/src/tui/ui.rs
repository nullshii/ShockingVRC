use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Gauge, Paragraph};

use super::app::{Action, App, SliderKind, Tab};
use super::tabs;
use super::theme;
use super::tutorial;
use super::update_overlay;

pub fn draw(frame: &mut Frame, app: &mut App) {
    app.clickables.clear();

    let area = frame.area();
    let show_device_bar = !app.device_infos.is_empty();
    let rows = Layout::vertical([
        Constraint::Length(if show_device_bar { 1 } else { 0 }),
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(area);

    if show_device_bar {
        render_device_selector(frame, app, rows[0]);
    }

    render_tabs(frame, app, rows[1]);

    let content = rows[2];
    match app.active_tab {
        Tab::Status => tabs::status::render(app, frame, content),
        Tab::Zones => tabs::zones::render(app, frame, content),
        Tab::Channels => tabs::channels::render(app, frame, content),
        Tab::Tuning => tabs::tuning::render(app, frame, content),
        Tab::Modulation => tabs::modulation::render(app, frame, content),
        Tab::Log => tabs::logs::render(app, frame, content),
        Tab::Presets => tabs::presets::render(app, frame, content),
        Tab::Setup => tabs::settings::render(app, frame, content),
    }

    render_status_bar(frame, app, rows[3]);

    if app.update_popup {
        update_overlay::render(app, frame, area);
    }

    if app.tutorial_active {
        tutorial::render_overlay(app, frame);
    }
}

fn render_device_selector(frame: &mut Frame, app: &mut App, area: Rect) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let devices: Vec<_> = app
        .device_infos
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let dot = if d.connected {
                "●"
            } else if d.reconnecting {
                "◌"
            } else {
                "○"
            };
            let short = d.name.chars().take(12).collect::<String>();
            let label = if d.reconnecting {
                format!(" D{} {} {dot} … ", i + 1, short)
            } else {
                format!(" D{} {} {dot} ", i + 1, short)
            };
            (i, label, i == app.active_device, d.reconnecting)
        })
        .collect();

    let mut x = area.x;
    for (i, label, active, reconnecting) in &devices {
        if x >= area.x + area.width {
            break;
        }
        if x > area.x {
            x += 1;
        }

        let w = label.chars().count() as u16;
        let avail = area.x + area.width - x;
        let chip_w = w.min(avail);
        let rect = Rect {
            x,
            y: area.y,
            width: chip_w,
            height: 1,
        };

        let style = if *active {
            Style::default()
                .fg(Color::Black)
                .bg(if *reconnecting {
                    theme::WARNING
                } else {
                    theme::ACCENT
                })
                .add_modifier(Modifier::BOLD)
        } else if *reconnecting {
            Style::default().fg(theme::WARNING)
        } else {
            Style::default().fg(theme::TEXT_DIM)
        };

        frame.render_widget(Paragraph::new(label.clone()).style(style), rect);
        app.push_click(rect, Action::SwitchDevice(*i));
        x += chip_w;
    }

    if devices.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                " Scanning for Coyote… ",
                Style::default().fg(theme::TEXT_DIM),
            )),
            area,
        );
    } else if devices.len() > 1 {
        let hint = "  [ / ]  Ctrl+1-9";
        let hint_x = x + 1;
        if hint_x < area.x + area.width {
            let hint_rect = Rect {
                x: hint_x,
                y: area.y,
                width: area.x + area.width - hint_x,
                height: 1,
            };
            frame.render_widget(
                Paragraph::new(Span::styled(hint, Style::default().fg(theme::TEXT_DIM))),
                hint_rect,
            );
        }
    }
}

fn render_tabs(frame: &mut Frame, app: &mut App, area: Rect) {
    let block = theme::tab_block();
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let tab_labels: [(&str, Tab); 8] = [
        ("1 Status", Tab::Status),
        ("2 Zones", Tab::Zones),
        ("3 Channels", Tab::Channels),
        ("4 Tuning", Tab::Tuning),
        ("5 Modulation", Tab::Modulation),
        ("6 Log", Tab::Log),
        ("7 Presets", Tab::Presets),
        ("8 Setup", Tab::Setup),
    ];

    let mut x = inner.x;
    for (i, (label, tab)) in tab_labels.iter().enumerate() {
        let text = format!(" {label} ");
        let w = text.chars().count() as u16;
        if x >= inner.x + inner.width {
            break;
        }
        let avail = inner.x + inner.width - x;
        let rect = Rect {
            x,
            y: inner.y,
            width: w.min(avail),
            height: 1,
        };
        let style = if *tab == app.active_tab {
            theme::active_tab_style()
        } else {
            theme::inactive_tab_style()
        };
        frame.render_widget(Paragraph::new(text).style(style), rect);
        app.push_click(rect, Action::SwitchTab(*tab));
        x += w;
        if i + 1 < tab_labels.len() && x < inner.x + inner.width {
            let sep = Rect { x, y: inner.y, width: 1, height: 1 };
            frame.render_widget(
                Paragraph::new("│").style(Style::default().fg(theme::ACCENT_DIM)),
                sep,
            );
            x += 1;
        }
    }
}

fn render_status_bar(frame: &mut Frame, app: &mut App, area: Rect) {
    let dev_count = app.device_infos.len();
    let active = app.device_infos.get(app.active_device);
    let dev_label = if let Some(d) = active {
        if dev_count > 1 {
            format!(" D{} {} ", app.active_device + 1, d.name)
        } else if d.connected {
            " Coyote connected ".to_string()
        } else if d.reconnecting {
            " Reconnecting… ".to_string()
        } else {
            " Coyote offline ".to_string()
        }
    } else {
        " No device ".to_string()
    };
    let dev_style = if app.status.device_connected {
        Style::default().fg(Color::Black).bg(theme::SUCCESS)
    } else {
        Style::default().fg(Color::Black).bg(theme::ERROR)
    };

    let hint = match app.active_tab {
        Tab::Status => "  Q/Esc — quit",
        Tab::Setup => "  Enter — type port  ·  [ / ] ±1  ·  −/+  ·  Esc — cancel edit",
        Tab::Presets => "  ↑↓ select  ·  × delete mine  ·  → A/B apply  ·  Save A/B  ·  R refresh",
        Tab::Zones => "  ↑↓ select  ·  ←→ pane  ·  − / + scale ±5  ·  R reset  ·  drag threshold sliders  ·  M mode  ·  X remove",
        Tab::Channels => "  ↑↓ field  ·  ←→ ±1  ·  Shift+←→ ±10  ·  wheel — scroll",
        Tab::Tuning => "  ↑↓ field  ·  ←→ adjust  ·  wheel — scroll  ·  Enter — reset",
        Tab::Modulation => "  Enter — function list  ·  source buttons  ·  ↑↓ field  ·  wheel — scroll",
        Tab::Log => "  ↑↓ / wheel — scroll  ·  Q/Esc — quit",
    };
    let line = Line::from(vec![
        Span::styled(dev_label.clone(), dev_style.add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled(hint, theme::hint_style()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

pub fn button(
    frame: &mut Frame,
    app: &mut App,
    area: Rect,
    label: &str,
    keyboard_focus: bool,
    action: Action,
) {
    let lit = app.button_lit(&action, keyboard_focus);
    frame.render_widget(
        Paragraph::new(format!(" {label} "))
            .style(theme::button_style(lit))
            .alignment(Alignment::Center),
        area,
    );
    app.push_click(area, action);
}

pub fn icon_button(
    frame: &mut Frame,
    app: &mut App,
    area: Rect,
    label: &str,
    keyboard_focus: bool,
    action: Action,
) {
    let lit = app.button_lit(&action, keyboard_focus);
    frame.render_widget(
        Paragraph::new(label)
            .style(theme::button_style(lit))
            .alignment(Alignment::Center),
        area,
    );
    app.push_click(area, action);
}

pub fn choice_button(
    frame: &mut Frame,
    app: &mut App,
    area: Rect,
    label: &str,
    selected: bool,
    row_focused: bool,
    action: Action,
) {
    let emphasis = app.choice_emphasis(&action, selected, row_focused);
    frame.render_widget(
        Paragraph::new(format!(" {label} "))
            .style(theme::choice_button_style(selected, emphasis))
            .alignment(Alignment::Center),
        area,
    );
    app.push_click(area, action);
}

#[allow(clippy::too_many_arguments)]
pub fn slider_row(
    frame: &mut Frame,
    app: &mut App,
    area: Rect,
    name: &str,
    value: i32,
    min: i32,
    max: i32,
    kind: SliderKind,
    focused: bool,
) {
    let cols = Layout::horizontal([Constraint::Length(24), Constraint::Min(8)]).split(area);
    frame.render_widget(
        Paragraph::new(name.to_string()).style(theme::label_style(focused)),
        cols[0],
    );
    let span = (max - min).max(1) as f64;
    let mut ratio = (((value - min) as f64) / span).clamp(0.0, 1.0);
    if kind.is_inverted() {
        ratio = 1.0 - ratio;
    }
    let bar_color = if focused {
        theme::HIGHLIGHT
    } else {
        theme::ACCENT
    };
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(bar_color).bg(theme::SURFACE_ELEVATED))
        .ratio(ratio)
        .label("");
    frame.render_widget(gauge, cols[1]);
    app.push_slider(cols[1], kind);
}

#[allow(clippy::too_many_arguments)]
pub fn stepper_row(
    frame: &mut Frame,
    app: &mut App,
    area: Rect,
    label: &str,
    focused: bool,
    minus: Action,
    plus: Action,
) {
    let cols = Layout::horizontal([
        Constraint::Min(10),
        Constraint::Length(5),
        Constraint::Length(1),
        Constraint::Length(5),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new(label.to_string()).style(theme::label_style(focused)),
        cols[0],
    );
    button(frame, app, cols[1], "−", false, minus);
    button(frame, app, cols[3], "+", false, plus);
}

pub fn header(frame: &mut Frame, area: Rect, text: &str) {
    frame.render_widget(
        Paragraph::new(text).style(theme::section_header_style()),
        area,
    );
}

pub fn max_scroll(content_h: u16, viewport_h: u16) -> u16 {
    content_h.saturating_sub(viewport_h)
}

pub fn clamp_scroll(scroll: u16, content_h: u16, viewport_h: u16) -> u16 {
    scroll.min(max_scroll(content_h, viewport_h))
}

pub fn scrolled_row_rect(inner: Rect, scroll: u16, logical_y: u16, height: u16) -> Option<Rect> {
    let rel = logical_y as i32 - scroll as i32;
    if rel + height as i32 <= 0 || rel >= inner.height as i32 {
        return None;
    }
    Some(Rect {
        x: inner.x,
        y: inner.y + rel as u16,
        width: inner.width,
        height,
    })
}

pub fn scroll_to_row(scroll: u16, viewport_h: u16, row: u16) -> u16 {
    if row < scroll {
        row
    } else if row + 1 > scroll + viewport_h {
        row + 1 - viewport_h
    } else {
        scroll
    }
}

pub fn apply_scroll_delta(scroll: u16, delta: i32, step: u16) -> u16 {
    if delta > 0 {
        scroll.saturating_add(step)
    } else if delta < 0 {
        scroll.saturating_sub(step)
    } else {
        scroll
    }
}

pub fn clear_panel_inner(frame: &mut Frame, inner: Rect) {
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    frame.render_widget(
        Block::default().style(Style::default().bg(theme::SURFACE)),
        inner,
    );
}

pub fn battery_gauge(frame: &mut Frame, area: Rect, pct: Option<u8>) {
    battery_gauge_inner(frame, area, pct, true);
}

pub fn battery_bar(frame: &mut Frame, area: Rect, pct: Option<u8>) {
    battery_gauge_inner(frame, area, pct, false);
}

fn battery_gauge_inner(frame: &mut Frame, area: Rect, pct: Option<u8>, show_label: bool) {
    use ratatui::text::Line;

    if area.width == 0 || area.height == 0 {
        return;
    }

    let Some(pct) = pct else {
        if show_label {
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("Battery ", Style::default().fg(theme::TEXT_DIM)),
                    Span::styled("…", Style::default().fg(theme::TEXT_DIM)),
                ])),
                area,
            );
        } else {
            frame.render_widget(
                Paragraph::new(Span::styled("…", Style::default().fg(theme::TEXT_DIM))),
                area,
            );
        }
        return;
    };

    let pct = pct.min(100);

    if area.width < 10 {
        frame.render_widget(
            Paragraph::new(format!("{pct}%")).style(
                Style::default()
                    .fg(theme::battery_gradient_color(pct as f32 / 100.0))
                    .add_modifier(Modifier::BOLD),
            ),
            area,
        );
        return;
    }

    let label_w = 5u16.min(area.width.saturating_sub(1));
    let prefix_w = if show_label {
        8u16.min(area.width.saturating_sub(label_w + 2))
    } else {
        0
    };
    let bar_w_avail = area
        .width
        .saturating_sub(prefix_w + label_w + 1);
    if bar_w_avail < 4 {
        frame.render_widget(
            Paragraph::new(format!("{pct}%")).style(
                Style::default()
                    .fg(theme::battery_gradient_color(pct as f32 / 100.0))
                    .add_modifier(Modifier::BOLD),
            ),
            area,
        );
        return;
    }

    let prefix_area = Rect {
        x: area.x,
        y: area.y,
        width: prefix_w,
        height: 1,
    };
    let bar_area = Rect {
        x: prefix_area.x + prefix_area.width,
        y: area.y,
        width: bar_w_avail,
        height: 1,
    };
    let label_area = Rect {
        x: bar_area.x + bar_area.width,
        y: area.y,
        width: label_w,
        height: 1,
    };

    if show_label && prefix_w > 0 {
        frame.render_widget(
            Paragraph::new(Span::styled("Battery", Style::default().fg(theme::TEXT_DIM))),
            prefix_area,
        );
    }

    let bar_w = bar_area.width.max(1) as usize;
    let filled = ((bar_w as f32 * pct as f32 / 100.0).round() as usize).min(bar_w);
    let mut spans: Vec<Span> = Vec::with_capacity(bar_w + 2);
    spans.push(Span::styled("▐", Style::default().fg(theme::TEXT_DIM)));

    for i in 0..bar_w {
        let ch = if i < filled { "█" } else { "░" };
        let seg_t = (i as f32 + 0.5) / bar_w as f32;
        let style = if i < filled {
            Style::default().fg(theme::battery_gradient_color(seg_t))
        } else {
            Style::default().fg(theme::SURFACE_ELEVATED)
        };
        spans.push(Span::styled(ch, style));
    }
    spans.push(Span::styled("▌", Style::default().fg(theme::TEXT_DIM)));

    frame.render_widget(Paragraph::new(Line::from(spans)), bar_area);
    frame.render_widget(
        Paragraph::new(format!("{pct:>3}%")).style(
            Style::default()
                .fg(theme::battery_gradient_color(pct as f32 / 100.0))
                .add_modifier(Modifier::BOLD),
        ),
        label_area,
    );
}
