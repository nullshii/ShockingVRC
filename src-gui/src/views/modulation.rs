use egui::{Align2, Rect, Sense, Stroke, Ui, vec2};

use shocking_vrc_core::modulation::config::ModulationSource;
use shocking_vrc_core::modulation::evaluator::{KinematicsInput, advance_accumulator, evaluate_with_accumulator};
use shocking_vrc_core::raw_to_hz;

use crate::app::Ctx;
use crate::bridge::Command;
use crate::model::{Channel, MOD_SOURCES, ModKind, ModParam, default_mod_config, source_label};
use crate::theme::widgets::{self as w, font, space};

const PREVIEW_SAMPLES: usize = 240;
const PREVIEW_DT: f32 = 0.035;

pub fn show(ui: &mut Ui, cx: &mut Ctx) {
    let gap = space::SHEET_GAP;
    let total = ui.available_width() - gap;
    let slots_w = (total * 0.34).clamp(240.0, 340.0);
    let editor_w = total - slots_w;

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        w::column(ui, slots_w, |ui| slots(ui, cx));
        w::column(ui, editor_w, |ui| editor(ui, cx));
    });
}

fn slots(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let active = (cx.ui_state.mod_channel, cx.ui_state.mod_kind, cx.ui_state.mod_seg);
    let used = count_used(cx);

    w::sheet(ui, pal, |ui| {
        w::heading_with_suffix(ui, pal, "Slots", Some(&format!("{used}/16 active")));

        for ch in Channel::ALL {
            for kind in ModKind::ALL {
                w::caption(ui, pal, &format!("Channel {} · {}", ch.label(), kind.title()));
                ui.add_space(2.0);
                for seg in 0..4 {
                    let slot = cx.snap.mod_slot(ch, kind, seg).clone();
                    let selected = active == (ch, kind, seg);
                    let (rect, resp) = w::row(ui, pal, selected, 24.0);
                    let inner = rect.shrink2(vec2(8.0, 0.0));

                    w::text_in(
                        ui,
                        Rect::from_min_size(inner.min, vec2(40.0, inner.height())),
                        &format!("s{seg}"),
                        font::small(),
                        if selected { pal.accent } else { pal.dim },
                        Align2::LEFT_CENTER,
                    );
                    let (label, color) = match &slot {
                        Some(c) => (
                            format!("{} → {}", c.function, c.source),
                            if selected { pal.accent } else { pal.contrast },
                        ),
                        None => ("—".to_string(), pal.dim),
                    };
                    w::text_in(
                        ui,
                        Rect::from_min_max(egui::pos2(inner.left() + 34.0, inner.top()), inner.max),
                        &label,
                        font::small(),
                        color,
                        Align2::LEFT_CENTER,
                    );

                    if resp.clicked() {
                        cx.ui_state.select_mod_slot(ch, kind, seg);
                        cx.ui_state.reload_mod_editor(cx.snap);
                    }
                }
                ui.add_space(space::ROW_GAP);
            }

            ui.horizontal(|ui| {
                if w::button(ui, pal, &format!("Clear channel {}", ch.label()))
                    .on_hover_text("Remove every modulation slot on this channel")
                    .clicked()
                {
                    cx.send(Command::ClearChannelModulation(ch));
                    cx.ui_state.reload_mod_editor(cx.snap);
                }
            });
            ui.add_space(space::ITEM_GAP);
        }
    });
}

fn count_used(cx: &Ctx) -> usize {
    let mut n = 0;
    for ch in Channel::ALL {
        for kind in ModKind::ALL {
            for seg in 0..4 {
                if cx.snap.mod_slot(ch, kind, seg).is_some() {
                    n += 1;
                }
            }
        }
    }
    n
}

fn editor(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let (ch, kind, seg) = (cx.ui_state.mod_channel, cx.ui_state.mod_kind, cx.ui_state.mod_seg);
    let stored = cx.snap.mod_slot(ch, kind, seg).clone();
    let dirty = cx.ui_state.mod_editor_dirty;

    w::sheet(ui, pal, |ui| {
        w::heading_with_suffix(
            ui,
            pal,
            &format!("{} {} · segment {seg}", ch.label(), kind.title()),
            Some(if dirty {
                "unsaved"
            } else if stored.is_some() {
                "active"
            } else {
                "empty"
            }),
        );

        let base = base_value(cx, ch, kind, seg);
        let samples = sample_curve(&cx.ui_state.mod_editor, base);
        preview(ui, cx.pal, &samples, base, kind);
        ui.add_space(space::ITEM_GAP);

        ui.horizontal(|ui| {
            w::field_label(ui, pal, "Function", 96.0, true);
            let label = cx.ui_state.mod_editor.function.to_string();
            if w::button(ui, pal, &label)
                .on_hover_text("Browse the full function list")
                .clicked()
            {
                cx.ui_state.show_function_picker = true;
            }
            ui.add_space(4.0);
            w::dim(ui, pal, "the shape the value traces");
        });

        ui.horizontal(|ui| {
            w::field_label(ui, pal, "Source", 96.0, true);
            let cur = MOD_SOURCES
                .iter()
                .position(|s| *s == cx.ui_state.mod_editor.source)
                .unwrap_or(0);
            let labels: Vec<&str> = MOD_SOURCES.iter().map(|s| source_label(*s)).collect();
            if let Some(p) = w::segmented(ui, pal, &labels, cur) {
                cx.ui_state.mod_editor.source = MOD_SOURCES[p];
                cx.ui_state.mod_editor_dirty = true;
            }
        });
        w::dim(ui, pal, source_hint(cx.ui_state.mod_editor.source));
        ui.add_space(space::ITEM_GAP);

        w::caption(ui, pal, "Parameters");
        ui.add_space(2.0);
        for p in ModParam::ALL {
            let (min, max) = p.range(kind);
            let value = p.get(&cx.ui_state.mod_editor);
            let display = p.format(&cx.ui_state.mod_editor);
            let changed =
                w::slider_row(ui, pal, p.label(), 96.0, w::Slider::new(value, min, max), &display, 84.0, None);
            if let Some(v) = changed {
                let v = p.clamp_value((v * 1000.0).round() / 1000.0, kind);
                p.set(&mut cx.ui_state.mod_editor, v);
                cx.ui_state.mod_editor.sanitise(kind.is_intensity());
                cx.ui_state.mod_editor_dirty = true;
            }
        }

        ui.add_space(space::ITEM_GAP);
        ui.horizontal(|ui| {
            if w::cta(ui, pal, "Apply").clicked() {
                cx.send(Command::ApplyModulation(
                    ch,
                    kind,
                    seg,
                    Box::new(cx.ui_state.mod_editor.clone()),
                ));
                cx.ui_state.mod_editor_dirty = false;
            }
            if w::button_enabled(ui, pal, "Revert", dirty)
                .on_hover_text("Discard edits and reload the stored slot")
                .clicked()
            {
                cx.ui_state.reload_mod_editor(cx.snap);
            }
            if w::button(ui, pal, "Reset")
                .on_hover_text("Back to a fresh slot")
                .clicked()
            {
                cx.ui_state.mod_editor = default_mod_config(kind);
                cx.ui_state.mod_editor_dirty = true;
            }
            if w::danger_button(ui, pal, "Clear slot", stored.is_some()).clicked() {
                cx.send(Command::ClearModulation(ch, kind, seg));
                cx.ui_state.mod_editor = default_mod_config(kind);
                cx.ui_state.mod_editor_dirty = false;
            }
        });

        if dirty {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Unsaved — Apply to send this to the device.")
                    .font(font::tiny())
                    .color(pal.warning),
            );
        }
    });
}

fn base_value(cx: &Ctx, ch: Channel, kind: ModKind, seg: usize) -> f32 {
    let c = cx.snap.channel(ch);
    match kind {
        ModKind::Freq => c.frequency[seg] as f32,
        ModKind::Intensity => c.intensity[seg] as f32,
    }
}

fn sample_curve(config: &shocking_vrc_core::modulation::ModulationConfig, base: f32) -> Vec<f32> {
    let kin = steady_kinematics(config.source);
    let mut accum = 0.0;
    let mut out = Vec::with_capacity(PREVIEW_SAMPLES);
    for _ in 0..PREVIEW_SAMPLES {
        advance_accumulator(config, &mut accum, &kin, PREVIEW_DT);
        out.push(evaluate_with_accumulator(config, base, accum));
    }
    out
}

fn steady_kinematics(source: ModulationSource) -> KinematicsInput {
    let v = 0.5;
    let mut k = KinematicsInput::default();
    match source {
        ModulationSource::Depth => k.depth = v,
        ModulationSource::Speed => k.speed = v,
        ModulationSource::Acc => k.acc = v,
        ModulationSource::Recoil => k.recoil = v,
    }
    k
}

fn preview(ui: &mut Ui, pal: &crate::theme::Palette, samples: &[f32], base: f32, kind: ModKind) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 132.0), Sense::hover());
    w::dashed_rect(ui, rect, pal.hair);
    if samples.len() < 2 {
        return;
    }

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

    let mut lo = min_v.min(base_m);
    let mut hi = max_v.max(base_m);
    let span = (hi - lo).max(0.001);
    lo -= span * 0.15;
    hi += span * 0.15;
    if (hi - lo) < span * 0.5 {
        lo -= span * 0.25;
        hi += span * 0.25;
    }

    let unit = if kind == ModKind::Freq { "Hz" } else { "%" };
    let header = Rect::from_min_size(rect.min + vec2(8.0, 4.0), vec2(rect.width() - 16.0, 13.0));
    w::text_in(
        ui,
        header,
        &format!("base {base_m:.0} {unit}"),
        font::tiny(),
        pal.dim,
        Align2::LEFT_CENTER,
    );
    w::text_in(
        ui,
        header,
        &format!("{min_v:.0} – {max_v:.0} {unit}"),
        font::tiny(),
        pal.dim,
        Align2::RIGHT_CENTER,
    );

    let plot = Rect::from_min_max(
        egui::pos2(rect.left() + 8.0, rect.top() + 20.0),
        egui::pos2(rect.right() - 8.0, rect.bottom() - 8.0),
    );
    if plot.height() < 10.0 {
        return;
    }
    let to_y = |v: f32| plot.bottom() - ((v - lo) / (hi - lo).max(1e-6)) * plot.height();
    let painter = ui.painter().with_clip_rect(plot);

    let by = to_y(base_m);
    w::dashed_line(
        &painter,
        egui::pos2(plot.left(), by),
        egui::pos2(plot.right(), by),
        Stroke::new(1.0, pal.faded(pal.contrast, 0.55)),
        3.0,
        3.0,
    );

    let n = mapped.len();
    let step = plot.width() / (n - 1) as f32;
    let mut prev = egui::pos2(plot.left(), to_y(mapped[0]));
    for (i, v) in mapped.iter().enumerate().skip(1) {
        let p = egui::pos2(plot.left() + step * i as f32, to_y(*v));
        painter.line_segment([prev, p], Stroke::new(1.5, pal.accent));
        prev = p;
    }
}

fn source_hint(s: ModulationSource) -> &'static str {
    match s {
        ModulationSource::Depth => "Driven by how deep the contact is.",
        ModulationSource::Speed => "Driven by how fast the contact moves.",
        ModulationSource::Acc => "Driven by how sharply the motion changes.",
        ModulationSource::Recoil => "Driven by pull-out motion.",
    }
}
