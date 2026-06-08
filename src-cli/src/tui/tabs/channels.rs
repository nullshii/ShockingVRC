use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::Paragraph;

use shocking_vrc_core::raw_to_hz;

use crate::tui::app::{
    agg_name, aggregation_modes, channel_controls, Action, App, Channel, ChannelControl,
    SliderKind,
};
use crate::tui::theme;
use crate::tui::ui;

const CHANNEL_ROWS: u16 = 14;

pub fn render(app: &mut App, frame: &mut Frame, area: Rect) {
    let rows = Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).split(area);
    let cols = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[0]);

    let viewport_h = rows[0].height.saturating_sub(2);
    app.channels_viewport_h = viewport_h;
    app.channels_scroll = ui::clamp_scroll(app.channels_scroll, CHANNEL_ROWS, viewport_h);
    if let Some(ctrl) = channel_controls().get(app.channel_focus).copied() {
        if let Some(row) = crate::tui::app::channel_control_row(ctrl) {
            app.channels_scroll =
                ui::scroll_to_row(app.channels_scroll, viewport_h, row);
        }
    }

    let focused_ctrl = channel_controls().get(app.channel_focus).copied();
    render_channel(app, frame, cols[0], Channel::A, focused_ctrl);
    render_channel(app, frame, cols[1], Channel::B, focused_ctrl);
    render_actions(app, frame, rows[1], focused_ctrl);
}

fn render_channel(
    app: &mut App,
    frame: &mut Frame,
    area: Rect,
    ch: Channel,
    focus: Option<ChannelControl>,
) {
    let block = theme::panel_block(format!("Channel {} — waveform & limits", ch.label()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    ui::clear_panel_inner(frame, inner);

    let cfg = app.channel_config(ch).clone();
    let scroll = app.channels_scroll;

    for row in 0..CHANNEL_ROWS {
        let Some(rect) = ui::scrolled_row_rect(inner, scroll, row, 1) else {
            continue;
        };
        match row {
            0 => ui::header(frame, rect, "Frequency — drag for smooth control"),
            1..=4 => {
                let seg = (row - 1) as usize;
                let v = cfg.frequency[seg] as i32;
                let name = format!("Seg {seg}  {:>4.0} Hz", raw_to_hz(cfg.frequency[seg]));
                ui::slider_row(
                    frame,
                    app,
                    rect,
                    &name,
                    v,
                    10,
                    255,
                    SliderKind::Freq(ch, seg),
                    focus == Some(ChannelControl::Freq(ch, seg)),
                );
            }
            5 => ui::header(frame, rect, "Intensity"),
            6..=9 => {
                let seg = (row - 6) as usize;
                let v = cfg.intensity[seg] as i32;
                ui::slider_row(
                    frame,
                    app,
                    rect,
                    &format!("Seg {seg}"),
                    v,
                    0,
                    100,
                    SliderKind::Intensity(ch, seg),
                    focus == Some(ChannelControl::Intensity(ch, seg)),
                );
            }
            10 => ui::header(frame, rect, "Power limits & mix mode"),
            11 => ui::slider_row(
                frame,
                app,
                rect,
                "Minimum",
                cfg.limits.min as i32,
                0,
                200,
                SliderKind::LimitMin(ch),
                focus == Some(ChannelControl::LimitMin(ch)),
            ),
            12 => ui::slider_row(
                frame,
                app,
                rect,
                "Maximum",
                cfg.limits.max as i32,
                0,
                200,
                SliderKind::LimitMax(ch),
                focus == Some(ChannelControl::LimitMax(ch)),
            ),
            13 => {
                let agg_cols = Layout::horizontal([
                    Constraint::Length(10),
                    Constraint::Length(5),
                    Constraint::Length(5),
                    Constraint::Length(5),
                ])
                .split(rect);
                frame.render_widget(
                    Paragraph::new("Zone mix").style(Style::default().fg(theme::TEXT_DIM)),
                    agg_cols[0],
                );
                let row_focused = focus == Some(ChannelControl::Aggregation(ch));
                for (i, mode) in aggregation_modes().iter().enumerate() {
                    let selected = cfg.aggregation == *mode;
                    ui::choice_button(
                        frame,
                        app,
                        agg_cols[i + 1],
                        agg_name(mode),
                        selected,
                        row_focused && selected,
                        Action::SetAggregation(ch, *mode),
                    );
                }
            }
            _ => {}
        }
    }
}

fn render_actions(app: &mut App, frame: &mut Frame, area: Rect, focus: Option<ChannelControl>) {
    let block = theme::panel_block("Settings file — cli_config.json");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let cols = Layout::horizontal([
        Constraint::Length(18),
        Constraint::Length(2),
        Constraint::Length(18),
    ])
    .split(inner);
    ui::button(
        frame,
        app,
        cols[0],
        "Save settings",
        focus == Some(ChannelControl::Save),
        Action::SaveConfig,
    );
    ui::button(
        frame,
        app,
        cols[2],
        "Load settings",
        focus == Some(ChannelControl::Load),
        Action::LoadConfig,
    );
}
