use egui::{Align2, Rect, Ui, vec2};

use shocking_vrc_core::modulation::config::{ModulationConfig, ModulationFunction};
use shocking_vrc_core::modulation::evaluator::{KinematicsInput, advance_accumulator, evaluate_with_accumulator};

use crate::app::Ctx;
use crate::model::{function_group, mod_function_list_for};
use crate::theme::Palette;
use crate::theme::widgets::{self as w, font, space};

pub fn show(ctx: &egui::Context, cx: &mut Ctx) {
    let pal = cx.pal;
    let current = cx.ui_state.mod_editor.function.clone();
    let all = mod_function_list_for(&current);
    let needle = cx.ui_state.function_filter.to_lowercase();

    let mut chosen: Option<ModulationFunction> = None;
    let mut close = false;

    let dismissed = super::modal(ctx, pal, "function_picker", "Function", 560.0, |ui| {
        w::dim(
            ui,
            pal,
            "The shape the modulated value traces over time. Hover a row to preview it.",
        );
        ui.add_space(space::ROW_GAP);
        w::text_field(ui, pal, &mut cx.ui_state.function_filter, "search…", ui.available_width());
        ui.add_space(space::ROW_GAP);

        let matches: Vec<&ModulationFunction> = all
            .iter()
            .filter(|f| needle.is_empty() || f.to_string().to_lowercase().contains(&needle))
            .collect();

        if matches.is_empty() {
            w::empty_state(ui, pal, "no function matches that search");
        }

        egui::ScrollArea::vertical()
            .max_height(420.0)
            .auto_shrink([false, true])
            .id_salt("fn_picker")
            .show(ui, |ui| {
                let mut last_group = None;
                for f in matches {
                    let group = function_group(f);
                    if last_group != Some(group) {
                        if last_group.is_some() {
                            ui.add_space(space::ROW_GAP);
                        }
                        w::caption(ui, pal, group.label());
                        ui.add_space(2.0);
                        last_group = Some(group);
                    }

                    let selected = *f == current;
                    let (rect, resp) = w::row(ui, pal, selected, 30.0);
                    let inner = rect.shrink2(vec2(8.0, 3.0));

                    w::text_in(
                        ui,
                        Rect::from_min_size(inner.min, vec2(180.0, inner.height())),
                        &f.to_string(),
                        font::body(),
                        if selected { pal.accent } else { pal.contrast },
                        Align2::LEFT_CENTER,
                    );

                    let spark = Rect::from_min_max(egui::pos2(inner.left() + 190.0, inner.top()), inner.max);
                    if spark.width() > 40.0 {
                        sparkline(ui, pal, spark, f, selected || resp.hovered());
                    }

                    if resp.clicked() {
                        chosen = Some(f.clone());
                    }
                }
            });

        ui.add_space(space::ITEM_GAP);
        ui.horizontal(|ui| {
            if w::button(ui, pal, "Close").clicked() {
                close = true;
            }
            ui.add_space(6.0);
            w::dim(ui, pal, format!("Currently: {current}"));
        });
    });

    if dismissed {
        close = true;
    }

    if let Some(f) = chosen {
        cx.ui_state.mod_editor.function = f;
        cx.ui_state.mod_editor_dirty = true;
        cx.ui_state.show_function_picker = false;
    } else if close {
        cx.ui_state.show_function_picker = false;
    }
}

fn sparkline(ui: &Ui, pal: &Palette, rect: Rect, f: &ModulationFunction, lit: bool) {
    let cfg = ModulationConfig {
        function: f.clone(),
        base_speed: 1.0,
        sensitivity: 0.0,
        max_deviation: 1.0,
        phase: 0.0,
        frequency_multiplier: 1.0,
        offset: 0.0,
        power: 1.0,
        clamp_min: -10.0,
        clamp_max: 10.0,
        ..ModulationConfig::default()
    };

    const N: usize = 64;
    let dt = 1.0 / N as f32;
    let kin = KinematicsInput::default();
    let mut accum = 0.0;
    let mut pts = Vec::with_capacity(N);
    for _ in 0..N {
        advance_accumulator(&cfg, &mut accum, &kin, dt);
        pts.push(evaluate_with_accumulator(&cfg, 0.0, accum));
    }

    let lo = pts.iter().copied().fold(f32::INFINITY, f32::min);
    let hi = pts.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let span = (hi - lo).max(1e-3);

    let painter = ui.painter().with_clip_rect(rect);
    let color = if lit { pal.accent } else { pal.dim };
    let step = rect.width() / (N - 1) as f32;
    let to_y = |v: f32| rect.bottom() - ((v - lo) / span) * (rect.height() - 4.0) - 2.0;

    let mut prev = egui::pos2(rect.left(), to_y(pts[0]));
    for (i, v) in pts.iter().enumerate().skip(1) {
        let p = egui::pos2(rect.left() + step * i as f32, to_y(*v));
        painter.line_segment([prev, p], egui::Stroke::new(1.0, color));
        prev = p;
    }
}
