use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use shocking_vrc_core::modulation::config::{ModulationConfig, ModulationSource};
use shocking_vrc_core::modulation::evaluator::{
    advance_accumulator, evaluate_with_accumulator, KinematicsInput,
};
use shocking_vrc_core::raw_to_hz;

use super::app::ModKind;
use super::theme;

const PREVIEW_SAMPLES: usize = 160;
const PREVIEW_DT: f32 = 0.035;

pub fn sample_modulation_curve(config: &ModulationConfig, base: f32) -> Vec<f32> {
    let kin = steady_kinematics(config.source);
    let mut accum = config.phase / config.frequency_multiplier.max(0.001);
    let mut out = Vec::with_capacity(PREVIEW_SAMPLES);
    for _ in 0..PREVIEW_SAMPLES {
        advance_accumulator(config, &mut accum, &kin, PREVIEW_DT);
        out.push(evaluate_with_accumulator(config, base, accum));
    }
    out
}

fn steady_kinematics(source: ModulationSource) -> KinematicsInput {
    let v = 0.5;
    let mut kin = KinematicsInput::default();
    match source {
        ModulationSource::Depth => kin.depth = v,
        ModulationSource::Speed => kin.speed = v,
        ModulationSource::Acc => kin.acc = v,
        ModulationSource::Recoil => kin.recoil = v,
    }
    kin
}

struct DisplaySeries {
    samples: Vec<f32>,
    base: f32,
    min_v: f32,
    max_v: f32,
}

fn to_display(samples: &[f32], base: f32, kind: ModKind) -> DisplaySeries {
    let map = |v: f32| -> f32 {
        if kind == ModKind::Freq {
            raw_to_hz(v.clamp(10.0, 255.0) as u8)
        } else {
            v
        }
    };
    let mapped: Vec<f32> = samples.iter().copied().map(map).collect();
    let base_m = map(base);
    let min_v = mapped.iter().copied().fold(f32::INFINITY, f32::min);
    let max_v = mapped.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    DisplaySeries {
        samples: mapped,
        base: base_m,
        min_v,
        max_v,
    }
}

fn y_bounds(min_v: f32, max_v: f32, base: f32) -> (f64, f64) {
    let mut lo = min_v.min(base);
    let mut hi = max_v.max(base);
    let span = (hi - lo).max(0.001);
    let pad = span * 0.15;
    lo -= pad;
    hi += pad;
    if (hi - lo) < span * 0.5 {
        lo -= span * 0.25;
        hi += span * 0.25;
    }
    (lo as f64, hi as f64)
}

pub fn render_modulation_preview(
    frame: &mut Frame,
    area: Rect,
    samples: &[f32],
    base: f32,
    kind: ModKind,
) {
    if samples.is_empty() || area.height < 6 || area.width < 16 {
        return;
    }

    let series = to_display(samples, base, kind);
    let (y_min, y_max) = y_bounds(series.min_v, series.max_v, series.base);

    let stats = match kind {
        ModKind::Freq => format!(
            "base {:.0} Hz   range {:.0}–{:.0} Hz",
            series.base, series.min_v, series.max_v
        ),
        ModKind::Intensity => format!(
            "base {:.0}%   range {:.0}–{:.0}%",
            series.base, series.min_v, series.max_v
        ),
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::ACCENT_DIM))
        .title(" Live preview ")
        .title_style(
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(theme::SURFACE));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 4 {
        return;
    }

    let stats_rect = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(stats).style(Style::default().fg(theme::TEXT_DIM)),
        stats_rect,
    );

    let graph_area = Rect {
        x: inner.x,
        y: inner.y + 1,
        width: inner.width,
        height: inner.height.saturating_sub(1),
    };
    if graph_area.height < 3 {
        return;
    }

    let n = series.samples.len().max(2);
    let x_max = (n - 1) as f64;

    let base_y = series.base as f64;
    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([-0.5, x_max + 0.5])
        .y_bounds([y_min, y_max])
        .paint(|ctx| {
            ctx.draw(&CanvasLine {
                x1: -0.5,
                y1: base_y,
                x2: x_max + 0.5,
                y2: base_y,
                color: Color::Rgb(60, 65, 80),
            });

            for i in 1..series.samples.len() {
                ctx.draw(&CanvasLine {
                    x1: (i - 1) as f64,
                    y1: series.samples[i - 1] as f64,
                    x2: i as f64,
                    y2: series.samples[i] as f64,
                    color: theme::HIGHLIGHT,
                });
            }
        });

    frame.render_widget(canvas, graph_area);
}
