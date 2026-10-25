use egui::{Align2, Color32, Rect, Sense, Stroke, StrokeKind, Ui, vec2};

use shocking_vrc_core::OldZoneType;
use shocking_vrc_core::cli::{MotionNorms, UkfConfig};
use shocking_vrc_core::graph::curve::{Curve, CurveInterp, CurvePreset};
use shocking_vrc_core::graph::device::FreqMode;
use shocking_vrc_core::graph::kind::*;
use shocking_vrc_core::graph::{
    DeviceBinding, GraphPresetEntry, GraphPresetKind, Node, NodeKind, NodeTint, PinType, Severity,
};
use shocking_vrc_core::raw_to_hz;

use super::{Env, GraphEditor, SideTab, node_ui};
use crate::app::short_mac;
use crate::model::{NormField, UkfField, mod_function_list};
use crate::overlays::graph_dialogs::GraphDialog;
use crate::theme::Palette;
use crate::theme::widgets::{self as w, font, space};

pub fn show(ui: &mut Ui, ed: &mut GraphEditor, env: &Env, dialog: &mut Option<GraphDialog>, nickname: &str) {
    let pal = env.pal;
    let tabs = ["Inspector", "Presets", "Devices"];
    let cur = match ed.side_tab {
        SideTab::Inspector => 0,
        SideTab::Presets => 1,
        SideTab::Devices => 2,
    };
    if let Some(i) = w::segmented(ui, pal, &tabs, cur) {
        ed.side_tab = [SideTab::Inspector, SideTab::Presets, SideTab::Devices][i];
    }
    ui.add_space(space::ROW_GAP);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt("graph_side")
        .show(ui, |ui| match ed.side_tab {
            SideTab::Inspector => inspector(ui, ed, env, dialog, nickname),
            SideTab::Presets => presets(ui, ed, env, dialog, nickname),
            SideTab::Devices => devices(ui, ed, env),
        });
}

const LABEL_W: f32 = 108.0;
const VALUE_W: f32 = 64.0;

fn row_label(ui: &mut Ui, pal: &Palette, text: &str) {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font::body(), pal.dim);
    job.wrap = egui::text::TextWrapping::truncate_at_width(LABEL_W);
    let galley = ui.painter().layout_job(job);
    let (rect, resp) = ui.allocate_exact_size(vec2(LABEL_W, 22.0), Sense::hover());
    let elided = galley.elided;
    ui.painter()
        .galley(egui::pos2(rect.left(), rect.center().y - galley.size().y * 0.5), galley, pal.dim);
    if elided {
        resp.on_hover_text(text);
    }
}

fn row_hint(ui: &mut Ui, pal: &Palette, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new(text).font(font::small()).color(pal.dim)).wrap());
}

fn empty_note(ui: &mut Ui, pal: &Palette, text: &str) {
    ui.add_space(6.0);
    ui.add(egui::Label::new(egui::RichText::new(text).font(font::small()).color(pal.dim).italics()).wrap());
    ui.add_space(6.0);
}

fn seg_row(ui: &mut Ui, pal: &Palette, label: &str, options: &[&str], selected: usize) -> Option<usize> {
    let need: f32 = options
        .iter()
        .map(|o| ui.painter().layout_no_wrap(o.to_string(), font::small(), pal.contrast).size().x + 18.0)
        .sum();
    if LABEL_W + ui.spacing().item_spacing.x + need <= ui.available_width() {
        ui.horizontal(|ui| {
            row_label(ui, pal, label);
            w::segmented(ui, pal, options, selected)
        })
        .inner
    } else {
        row_label(ui, pal, label);
        w::segmented(ui, pal, options, selected)
    }
}

fn num_row(ui: &mut Ui, pal: &Palette, label: &str, v: &mut f32, min: f32, max: f32) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        row_label(ui, pal, label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let speed = ((max - min).abs() / 300.0).max(1e-4) as f64;
            if ui
                .add_sized(vec2(VALUE_W, 20.0), egui::DragValue::new(v).speed(speed).max_decimals(4))
                .changed()
            {
                changed = true;
            }
            let sw = ui.available_width();
            if sw >= 40.0
                && let Some(nv) = w::Slider::new(*v, min, max).width(sw).show(ui, pal)
            {
                *v = (nv * 1000.0).round() / 1000.0;
                changed = true;
            }
        });
    });
    if !v.is_finite() {
        *v = min;
    }
    changed
}

fn int_row(ui: &mut Ui, pal: &Palette, label: &str, v: &mut u32, min: u32, max: u32) -> bool {
    let mut f = *v as f32;
    let changed = num_row(ui, pal, label, &mut f, min as f32, max as f32);
    if changed {
        *v = f.round().clamp(min as f32, max as f32) as u32;
    }
    changed
}

fn toggle_row(ui: &mut Ui, pal: &Palette, label: &str, v: &mut bool, hint: &str) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        row_label(ui, pal, label);
        if let Some(n) = w::toggle(ui, pal, *v)
            && n != *v
        {
            *v = n;
            changed = true;
        }
        if !hint.is_empty() {
            row_hint(ui, pal, hint);
        }
    });
    changed
}

fn text_row(ui: &mut Ui, pal: &Palette, label: &str, v: &mut String, hint: &str) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        row_label(ui, pal, label);
        let width = ui.available_width().max(60.0);
        if w::text_field(ui, pal, v, hint, width).changed() {
            changed = true;
        }
    });
    changed
}

fn choices(ui: &mut Ui, pal: &Palette, options: &[String], selected: Option<usize>) -> Option<usize> {
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for (i, label) in options.iter().enumerate() {
            let galley = ui
                .painter()
                .layout_no_wrap(label.clone(), font::small(), pal.contrast);
            let size = vec2(galley.size().x + 14.0, galley.size().y + 6.0);
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
            let on = selected == Some(i);
            let (fill, fg) = match (on, resp.hovered()) {
                (true, _) => (pal.accent, pal.on_solid()),
                (false, true) => (pal.well, pal.contrast),
                (false, false) => (Color32::TRANSPARENT, pal.dim),
            };
            if fill != Color32::TRANSPARENT {
                ui.painter().rect_filled(rect, w::SQUARE, fill);
            }
            ui.painter().rect_stroke(
                rect,
                w::SQUARE,
                Stroke::new(1.0, if on { pal.accent } else { pal.hair }),
                StrokeKind::Inside,
            );
            ui.painter()
                .text(rect.center(), Align2::CENTER_CENTER, label, font::small(), fg);
            if resp.clicked() {
                picked = Some(i);
            }
        }
    });
    picked
}

fn pick_enum<T: Copy + PartialEq>(ui: &mut Ui, pal: &Palette, all: &[T], cur: &mut T, label: impl Fn(T) -> String) -> bool {
    let names: Vec<String> = all.iter().map(|v| label(*v)).collect();
    let sel = all.iter().position(|v| v == cur);
    match choices(ui, pal, &names, sel) {
        Some(i) if Some(i) != sel => {
            *cur = all[i];
            true
        }
        _ => false,
    }
}

fn inspector(ui: &mut Ui, ed: &mut GraphEditor, env: &Env, dialog: &mut Option<GraphDialog>, nickname: &str) {
    let pal = env.pal;
    if ed.selection.len() > 1 {
        w::sheet(ui, pal, |ui| {
            w::heading_with_suffix(ui, pal, "Selection", Some(&format!("{} nodes", ed.selection.len())));
            ui.horizontal_wrapped(|ui| {
                if w::button(ui, pal, "Group").clicked() {
                    ed.group_selection(env);
                }
                if w::button(ui, pal, "Duplicate").clicked() {
                    ed.duplicate();
                }
                if w::button(ui, pal, "Save as fragment…").clicked() {
                    *dialog = Some(GraphDialog::save_preset(nickname, true));
                }
                if w::danger_button(ui, pal, "Delete", true).clicked() {
                    ed.delete_selection(env);
                }
            });
        });
        return;
    }
    let Some(id) = ed.selection.iter().next().copied() else {
        summary(ui, ed, env);
        return;
    };
    let Some(original) = ed.scope().node(id).cloned() else { return };
    let mut node = original.clone();
    let connected: Vec<String> = ed
        .scope()
        .edges
        .iter()
        .filter(|e| e.to.node == id)
        .map(|e| e.to.pin.clone())
        .collect();
    let probe = super::node_probe(&env.snap.graph.probe, &ed.path, &original);
    let mut enter = false;
    let mut ungroup = false;

    w::sheet(ui, pal, |ui| {
        w::heading_with_suffix(ui, pal, &node.kind.label(), Some(node.kind.category().label()));
        text_row(ui, pal, "Title", &mut node.title, &node.kind.label());
        ui.horizontal(|ui| {
            row_label(ui, pal, "Colour");
            let mut opts = vec![None];
            opts.extend(NodeTint::ALL.iter().copied().map(Some));
            for t in opts {
                let (rect, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                let c = t.map(|t| node_ui::tint_color(pal, t)).unwrap_or(pal.contrast);
                ui.painter().rect_filled(rect.shrink(2.0), w::SQUARE, c);
                ui.painter().rect_stroke(
                    rect,
                    w::SQUARE,
                    Stroke::new(1.0, if node.tint == t { pal.accent } else { pal.hair }),
                    StrokeKind::Inside,
                );
                if resp.clicked() {
                    node.tint = t;
                }
            }
        });
        text_row(ui, pal, "Comment", &mut node.comment, "(none)");
        toggle_row(ui, pal, "Collapsed", &mut node.collapsed, "H");
    });

    ui.add_space(space::SHEET_GAP);
    w::sheet(ui, pal, |ui| {
        w::heading(ui, pal, "Parameters");
        params(ui, ed, env, &mut node, &mut enter, &mut ungroup);

        let ins = node.inputs();
        let unconnected: Vec<&PinSpec> = ins.iter().filter(|p| !connected.iter().any(|c| *c == p.key)).collect();
        if !unconnected.is_empty() {
            ui.add_space(space::ITEM_GAP);
            w::caption(ui, pal, "Unconnected inputs");
            ui.add_space(2.0);
            for p in unconnected {
                let mut v = node.input_default(p);
                let changed = match p.ty {
                    PinType::Bool => {
                        let mut b = v > 0.5;
                        let c = toggle_row(ui, pal, &p.label, &mut b, "");
                        v = if b { 1.0 } else { 0.0 };
                        c
                    }
                    PinType::Float => num_row(ui, pal, &p.label, &mut v, p.range.0, p.range.1),
                };
                if changed {
                    node.set_input_default(&p.key, v);
                }
            }
        }
        if let Some(pr) = &probe {
            ui.add_space(space::ITEM_GAP);
            w::caption(ui, pal, "Live outputs");
            for (spec, v) in node.outputs().iter().zip(&pr.outputs) {
                ui.horizontal(|ui| {
                    row_label(ui, pal, &spec.label);
                    w::body(ui, pal, node_ui::fmt_value(*v));
                });
            }
        }
        for issue in env.snap.graph.issues.iter().filter(|i| i.node == Some(id)) {
            ui.add_space(4.0);
            let c = match issue.severity {
                Severity::Error => pal.error,
                Severity::Warning => pal.warning,
                Severity::Info => pal.dim,
            };
            ui.label(egui::RichText::new(&issue.message).font(font::small()).color(c));
        }
    });

    if node != original {
        ed.edit(|g| {
            if let Some(slot) = g.node_mut(id) {
                *slot = node;
            }
            g.prune_dangling_edges();
        });
    }
    if enter {
        ed.enter_group(id);
    }
    if ungroup {
        ed.ungroup_selection();
    }
}

fn summary(ui: &mut Ui, ed: &mut GraphEditor, env: &Env) {
    let pal = env.pal;
    let scope = ed.scope();
    w::sheet(ui, pal, |ui| {
        w::heading_with_suffix(
            ui,
            pal,
            "Graph",
            Some(&format!("{} node(s) · {} wire(s)", scope.nodes.len(), scope.edges.len())),
        );
        let g = &env.snap.graph;
        if g.panic {
            ui.label(
                egui::RichText::new("PANIC — every device is held at 0")
                    .font(font::body())
                    .color(pal.error),
            );
        }
        if g.issues.is_empty() {
            w::dim(ui, pal, "No problems found.");
        }
        for issue in &g.issues {
            let c = match issue.severity {
                Severity::Error => pal.error,
                Severity::Warning => pal.warning,
                Severity::Info => pal.dim,
            };
            ui.label(egui::RichText::new(&issue.message).font(font::small()).color(c));
        }
        for note in &g.notes {
            ui.label(egui::RichText::new(note).font(font::small()).color(pal.warning));
        }
        ui.add_space(space::ITEM_GAP);
        w::caption(ui, pal, "Getting started");
        for line in [
            "Right-click or Shift+A adds a node.",
            "Drag from an output square to an input to wire it.",
            "Minimal graph: Contact → Device · Channel A · Power.",
            "Power 0–1 maps through Limit Min/Max, never above the cap.",
            "Ctrl+drag cuts wires · Ctrl+G groups · F frames all.",
        ] {
            ui.label(egui::RichText::new(line).font(font::small()).color(pal.dim));
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn params(ui: &mut Ui, ed: &GraphEditor, env: &Env, node: &mut Node, enter: &mut bool, ungroup: &mut bool) {
    let pal = env.pal;
    match &mut node.kind {
        NodeKind::OscFloat(p) => {
            text_row(ui, pal, "Address", &mut p.address, "/avatar/parameters/…");
            int_row(ui, pal, "Stale after ms", &mut p.stale_ms, 50, 60_000);
            ui.add_space(4.0);
            w::caption(ui, pal, "Seen parameters");
            let filter_id = egui::Id::new("graph_param_filter");
            let mut filter = ui
                .ctx()
                .data(|d| d.get_temp::<String>(filter_id))
                .unwrap_or_default();
            ui.horizontal(|ui| {
                w::text_field(ui, pal, &mut filter, "filter…", ui.available_width());
            });
            let needle = filter.to_lowercase();
            egui::ScrollArea::vertical()
                .max_height(160.0)
                .id_salt("osc_params")
                .show(ui, |ui| {
                    let params = env.snap.graph.seen_params.clone();
                    if params.is_empty() {
                        empty_note(ui, pal, "nothing yet — VRChat sends parameters once connected");
                    }
                    for (addr, value) in params.iter().filter(|(a, _)| needle.is_empty() || a.to_lowercase().contains(&needle)).take(200) {
                        let (rect, resp) = w::row(ui, pal, *addr == p.address, 18.0);
                        let short = addr.strip_prefix("/avatar/parameters/").unwrap_or(addr);
                        w::text_in(ui, rect.shrink2(vec2(6.0, 0.0)), short, font::tiny(), pal.contrast, Align2::LEFT_CENTER);
                        w::text_in(ui, rect.shrink2(vec2(6.0, 0.0)), &node_ui::fmt_value(*value), font::tiny(), pal.dim, Align2::RIGHT_CENTER);
                        if resp.clicked() {
                            p.address = addr.clone();
                        }
                    }
                });
            ui.ctx().data_mut(|d| d.insert_temp(filter_id, filter));
        }
        NodeKind::Contact(p) => {
            let types = [OldZoneType::Any, OldZoneType::Pen, OldZoneType::Orf, OldZoneType::Touch, OldZoneType::DGB];
            let names = ["Any", "Pen", "Orf", "Tps", "DGB"];
            let sel = types.iter().position(|t| *t == p.zone.zone_type).unwrap_or(0);
            if let Some(i) = seg_row(ui, pal, "Zone type", &names, sel) {
                p.zone.zone_type = types[i];
            }
            text_row(ui, pal, "Name", &mut p.zone.name, "* matches any");
            int_row(ui, pal, "Stale after ms", &mut p.stale_ms, 50, 10_000);
            toggle_row(ui, pal, "Spoof guard", &mut p.spoof_guard, "when on in Setup");
            ui.add_space(4.0);
            w::caption(ui, pal, "Avatar zones");
            egui::ScrollArea::vertical()
                .max_height(140.0)
                .id_salt("contact_zones")
                .show(ui, |ui| {
                    if env.snap.avatar_zones.is_empty() {
                        empty_note(ui, pal, "no avatar zones reported yet");
                    }
                    for z in &env.snap.avatar_zones {
                        let on = p.zone.zone_type == z.zone_type && p.zone.name == z.id;
                        let (rect, resp) = w::row(ui, pal, on, 18.0);
                        w::text_in(ui, rect.shrink2(vec2(6.0, 0.0)), &format!("{}/{}", z.zone_type, z.id), font::tiny(), pal.contrast, Align2::LEFT_CENTER);
                        if resp.clicked() {
                            p.zone.zone_type = z.zone_type;
                            p.zone.name = z.id.clone();
                        }
                    }
                });
            w::dim(ui, pal, "Raw level, before any filtering. Add a UKF Filter node for speed/acc/recoil.");
        }
        NodeKind::Constant(p) => {
            num_row(ui, pal, "Value", &mut p.value, -1.0, 2.0);
        }
        NodeKind::Slider(p) => {
            num_row(ui, pal, "Value", &mut p.value, p.min, p.max);
            num_row(ui, pal, "Min", &mut p.min, -10.0, 10.0);
            num_row(ui, pal, "Max", &mut p.max, -10.0, 10.0);
            num_row(ui, pal, "Step", &mut p.step, 0.001, 1.0);
            text_row(ui, pal, "Key up", &mut p.key_up, "e.g. F2, Up");
            text_row(ui, pal, "Key down", &mut p.key_down, "e.g. F1, Down");
        }
        NodeKind::Toggle(p) => {
            toggle_row(ui, pal, "On", &mut p.on, "");
            toggle_row(ui, pal, "Momentary", &mut p.momentary, "a button: on while held");
            text_row(ui, pal, "Key", &mut p.key, "e.g. F3, B");
        }
        NodeKind::Ukf(p) => {
            w::caption(ui, pal, "Unscented Kalman filter");
            let ranges = |f: UkfField| match f {
                UkfField::Q => (0.0001, 200.0),
                UkfField::R => (0.0001, 1.0),
                _ => f.range(),
            };
            for f in UkfField::ALL {
                let (lo, hi) = ranges(f);
                let mut v = f.get(&p.ukf);
                if num_row(ui, pal, f.label(), &mut v, lo, hi) {
                    f.set(&mut p.ukf, v);
                }
            }
            if w::button(ui, pal, "Reset filter defaults").clicked() {
                p.ukf = UkfConfig::default();
            }
            ui.add_space(space::ITEM_GAP);
            w::caption(ui, pal, "Motion normalisation");
            for f in NormField::ALL {
                let (lo, hi) = f.range();
                let mut v = f.get(&p.norms);
                if num_row(ui, pal, f.label(), &mut v, lo, hi) {
                    f.set(&mut p.norms, v);
                }
            }
            if w::button(ui, pal, "Reset normalisation defaults").clicked() {
                p.norms = MotionNorms::default();
            }
            w::dim(ui, pal, "This node's own filter — the Tuning tab is Classic-only. Edits apply live.");
        }
        NodeKind::Integrate(p) => {
            num_row(ui, pal, "Leak /s", &mut p.leak, 0.0, 5.0);
            num_row(ui, pal, "Min", &mut p.min, -10.0, 10.0);
            num_row(ui, pal, "Max", &mut p.max, -10.0, 10.0);
        }
        NodeKind::Smooth(p) => {
            num_row(ui, pal, "Attack ms", &mut p.attack_ms, 0.0, 3000.0);
            num_row(ui, pal, "Release ms", &mut p.release_ms, 0.0, 3000.0);
        }
        NodeKind::MovingAverage(p) => {
            num_row(ui, pal, "Window ms", &mut p.window_ms, TICK_MS, TICK_MS * MAX_BUFFER as f32);
        }
        NodeKind::Normalize(p) => {
            num_row(ui, pal, "Divisor", &mut p.divisor, 0.01, 200.0);
            toggle_row(ui, pal, "Absolute", &mut p.abs, "|x| first");
            toggle_row(ui, pal, "Clamp", &mut p.clamp, "to 0..1");
        }
        NodeKind::SpoofGuard(p) => {
            num_row(ui, pal, "Jump limit", &mut p.jump_limit, 0.0, 1.0);
            num_row(ui, pal, "Release level", &mut p.release_level, 0.0, 0.5);
            w::dim(ui, pal, "Holds the signal at 0 when it jumps from idle above the limit in one tick.");
        }
        NodeKind::Math(p) => {
            pick_enum(ui, pal, &MathOp::ALL, &mut p.op, |o| o.label().to_string());
        }
        NodeKind::Remap(p) => {
            toggle_row(ui, pal, "Clamp", &mut p.clamp, "keep inside the out range");
        }
        NodeKind::Curve(p) => {
            w::caption(ui, pal, "Presets");
            let names: Vec<String> = CurvePreset::ALL.iter().map(|c| c.label().to_string()).collect();
            if let Some(i) = choices(ui, pal, &names, None) {
                p.curve = Curve::preset(CurvePreset::ALL[i]);
            }
            ui.add_space(4.0);
            w::caption(ui, pal, "Interpolation");
            pick_enum(ui, pal, &CurveInterp::ALL, &mut p.curve.interp, |c| c.label().to_string());
            w::dim(ui, pal, "On the node: drag points, double-click to add, right-click to remove.");
        }
        NodeKind::Function(p) => {
            w::caption(ui, pal, "Function");
            egui::ScrollArea::vertical()
                .max_height(140.0)
                .id_salt("fn_list")
                .show(ui, |ui| {
                    let names: Vec<String> = mod_function_list().iter().map(|f| f.to_string()).collect();
                    let list = mod_function_list();
                    let sel = list.iter().position(|f| *f == p.function);
                    if let Some(i) = choices(ui, pal, &names, sel) {
                        p.function = list[i].clone();
                    }
                });
            num_row(ui, pal, "Phase (cyc)", &mut p.phase, 0.0, 1.0);
            num_row(ui, pal, "Freq mul", &mut p.frequency_multiplier, 0.001, 8.0);
            num_row(ui, pal, "Power", &mut p.power, 0.001, 6.0);
            num_row(ui, pal, "Deviation", &mut p.max_deviation, 0.0, 2.0);
            num_row(ui, pal, "Offset", &mut p.offset, -2.0, 2.0);
            num_row(ui, pal, "Clamp min", &mut p.clamp_min, -2.0, 2.0);
            num_row(ui, pal, "Clamp max", &mut p.clamp_max, -2.0, 2.0);
            w::dim(ui, pal, "Same functions as Classic modulation; Cycles drives the phase.");
        }
        NodeKind::Combine(p) => {
            pick_enum(ui, pal, &CombineMode::ALL, &mut p.mode, |m| m.label().to_string());
            let mut n = p.count as u32;
            if int_row(ui, pal, "Inputs", &mut n, 2, MAX_FAN as u32) {
                p.count = n as u8;
            }
            if p.mode == CombineMode::Weighted {
                p.weights.resize(p.count as usize, 1.0);
                for (i, wgt) in p.weights.iter_mut().enumerate() {
                    num_row(ui, pal, &format!("Weight {}", i + 1), wgt, 0.0, 2.0);
                }
            }
            w::dim(ui, pal, "Sum and average are capped at 1, like Classic zone mixing.");
        }
        NodeKind::Lfo(p) => {
            pick_enum(ui, pal, &LfoShape::ALL, &mut p.shape, |s| s.label().to_string());
            num_row(ui, pal, "Phase (cyc)", &mut p.phase, 0.0, 1.0);
            toggle_row(ui, pal, "One-shot", &mut p.one_shot, "one cycle per reset");
            int_row(ui, pal, "Seed", &mut p.seed, 1, 9999);
            w::dim(ui, pal, "Hz, amp, bias and width are inputs — wire them to modulate.");
        }
        NodeKind::Noise(p) => {
            pick_enum(ui, pal, &NoiseKind::ALL, &mut p.kind, |k| k.label().to_string());
            int_row(ui, pal, "Seed", &mut p.seed, 1, 9999);
        }
        NodeKind::Envelope(p) => {
            pick_enum(ui, pal, &[EnvMode::Adsr, EnvMode::Ar], &mut p.mode, |m| match m {
                EnvMode::Adsr => "ADSR".to_string(),
                EnvMode::Ar => "AR".to_string(),
            });
        }
        NodeKind::Delay(p) => {
            pick_enum(ui, pal, &[DelayMode::Ticks, DelayMode::Ms], &mut p.mode, |m| match m {
                DelayMode::Ticks => "ticks".to_string(),
                DelayMode::Ms => "ms".to_string(),
            });
            int_row(ui, pal, "Amount", &mut p.amount, 1, 10_000);
            w::dim(ui, pal, format!("= {} tick(s) of 25 ms. Feedback loops need a Delay.", p.ticks()));
        }
        NodeKind::Sequencer(p) => {
            let mut n = p.steps.len() as u32;
            if int_row(ui, pal, "Steps", &mut n, 1, MAX_STEPS as u32) {
                p.steps.resize(n as usize, 0.5);
            }
            for (i, s) in p.steps.iter_mut().enumerate() {
                num_row(ui, pal, &format!("Step {}", i + 1), s, 0.0, 1.0);
            }
        }
        NodeKind::Compare(p) => {
            pick_enum(ui, pal, &CompareOp::ALL, &mut p.op, |o| o.label().to_string());
            num_row(ui, pal, "Hysteresis", &mut p.hysteresis, 0.0, 0.5);
        }
        NodeKind::Logic(p) => {
            pick_enum(ui, pal, &LogicOp::ALL, &mut p.op, |o| o.label().to_string());
        }
        NodeKind::Switch(p) | NodeKind::Router(p) => {
            let mut n = p.count as u32;
            if int_row(ui, pal, "Ways", &mut n, 2, MAX_FAN as u32) {
                p.count = n as u8;
            }
        }
        NodeKind::Crossfade(p) => {
            toggle_row(ui, pal, "Equal power", &mut p.equal_power, "constant loudness");
        }
        NodeKind::Frame(p) => {
            ui.label(egui::RichText::new("Text").font(font::small()).color(pal.dim));
            ui.add(
                egui::TextEdit::multiline(&mut p.text)
                    .font(font::small())
                    .desired_rows(3)
                    .desired_width(ui.available_width()),
            );
            num_row(ui, pal, "Width", &mut p.size[0], 80.0, 2000.0);
            num_row(ui, pal, "Height", &mut p.size[1], 60.0, 2000.0);
        }
        NodeKind::Group(p) => {
            text_row(ui, pal, "Name", &mut p.name, "Group");
            w::dim(ui, pal, format!("{} node(s) inside", p.graph.nodes.len()));
            for n in &mut p.graph.nodes {
                if let NodeKind::GroupInput(ports) | NodeKind::GroupOutput(ports) = &mut n.kind {
                    for port in &mut ports.ports {
                        text_row(ui, pal, &port.key, &mut port.label, "label");
                    }
                }
            }
            ui.horizontal(|ui| {
                if w::cta(ui, pal, "Enter group").clicked() {
                    *enter = true;
                }
                if w::button(ui, pal, "Ungroup").clicked() {
                    *ungroup = true;
                }
            });
        }
        NodeKind::GroupInput(p) | NodeKind::GroupOutput(p) => {
            let mut remove = None;
            for (i, port) in p.ports.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    w::field_label(ui, pal, &port.key, 60.0, false);
                    w::text_field(ui, pal, &mut port.label, "label", 150.0);
                    if w::button(ui, pal, "×").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                p.ports.remove(i);
            }
            w::dim(ui, pal, "Ports become the group's pins outside.");
        }
        NodeKind::Panic(p) => {
            toggle_row(ui, pal, "Latched", &mut p.latched, "holds every device at 0");
        }
        NodeKind::Scope(p) => {
            let mut n = p.count as u32;
            if int_row(ui, pal, "Inputs", &mut n, 1, 4) {
                p.count = n as u8;
            }
            num_row(ui, pal, "Window s", &mut p.window_s, 0.25, 10.0);
        }
        NodeKind::Device(d) => device_params(ui, ed, env, d),
        NodeKind::Missing(m) => {
            ui.label(
                egui::RichText::new(format!("Unknown type '{}' — saved back exactly as loaded.", m.type_name))
                    .font(font::small())
                    .color(pal.error),
            );
            let raw = serde_json::to_string_pretty(&m.raw).unwrap_or_default();
            ui.label(egui::RichText::new(raw.chars().take(600).collect::<String>()).font(font::tiny()).color(pal.dim));
        }
        _ => {
            w::dim(ui, pal, "No settings — tune it through its inputs.");
        }
    }
}

fn device_params(ui: &mut Ui, ed: &GraphEditor, env: &Env, d: &mut DeviceParams) {
    let pal = env.pal;
    let profile = d.profile();
    w::caption(ui, pal, "Device");
    let current = match &d.binding {
        DeviceBinding::Mac(m) => m.clone(),
        DeviceBinding::Slot(s) => format!("preset slot {s}"),
        DeviceBinding::Unbound => "unbound".to_string(),
    };
    ui.horizontal(|ui| {
        row_label(ui, pal, "Bound to");
        w::body(ui, pal, current);
    });
    let online = d
        .binding
        .mac()
        .is_some_and(|m| env.snap.devices.iter().any(|x| x.connected && x.mac.eq_ignore_ascii_case(m)));
    if !online {
        let mut candidates: Vec<(String, String)> = env
            .snap
            .devices
            .iter()
            .map(|x| (x.mac.clone(), format!("{} …{}", x.name, short_mac(&x.mac))))
            .collect();
        if let Ok(k) = env.bridge.shared.known_devices.read() {
            for m in k.iter() {
                if !candidates.iter().any(|(c, _)| c.eq_ignore_ascii_case(m)) {
                    candidates.push((m.clone(), format!("known …{}", short_mac(m))));
                }
            }
        }
        candidates.retain(|(m, _)| ed.doc.device_node_for(m).is_none());
        ui.add_space(2.0);
        w::caption(ui, pal, "Pick a device");
        if candidates.is_empty() {
            empty_note(ui, pal, "no free devices — connect one or type a MAC");
        }
        for (mac, label) in candidates {
            let (rect, resp) = w::row(ui, pal, false, 20.0);
            w::text_in(ui, rect.shrink2(vec2(6.0, 0.0)), &label, font::small(), pal.contrast, Align2::LEFT_CENTER);
            if resp.clicked() {
                d.binding = DeviceBinding::Mac(mac.to_uppercase());
            }
        }
        let mut typed = ui
            .ctx()
            .data(|m| m.get_temp::<String>(egui::Id::new("graph_mac_entry")))
            .unwrap_or_default();
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if w::button_enabled(ui, pal, "Bind", typed.trim().len() >= 11).clicked() {
                    d.binding = DeviceBinding::Mac(typed.trim().to_uppercase());
                    typed.clear();
                }
                let width = ui.available_width();
                w::text_field(ui, pal, &mut typed, "AA:BB:CC:DD:EE:FF", width);
            });
        });
        ui.ctx().data_mut(|m| m.insert_temp(egui::Id::new("graph_mac_entry"), typed));
    }
    for (ci, ch) in profile.channels.iter().enumerate() {
        let Some(cp) = d.channels.get_mut(ci) else { continue };
        ui.add_space(space::ITEM_GAP);
        w::caption(ui, pal, ch.label);
        let mut ef = cp.is_expanded("freq");
        if toggle_row(ui, pal, "Freq ×4", &mut ef, "one pin per 25 ms segment") {
            cp.set_expanded("freq", ef);
        }
        let mut ei = cp.is_expanded("int");
        if toggle_row(ui, pal, "Intensity ×4", &mut ei, "") {
            cp.set_expanded("int", ei);
        }
        let names: Vec<&str> = FreqMode::ALL.iter().map(|m| m.label()).collect();
        let sel = FreqMode::ALL.iter().position(|m| *m == cp.freq_mode).unwrap_or(0);
        if let Some(i) = seg_row(ui, pal, "Freq pin", &names, sel) {
            cp.freq_mode = FreqMode::ALL[i];
        }
        if cp.freq_mode == FreqMode::Normalized {
            num_row(ui, pal, "0 → Hz", &mut cp.freq_hz[0], 1.0, 100.0);
            num_row(ui, pal, "1 → Hz", &mut cp.freq_hz[1], 1.0, 100.0);
        }
        let mut lo = cp.freq_clamp[0] as u32;
        let mut hi = cp.freq_clamp[1] as u32;
        if int_row(ui, pal, "Raw min", &mut lo, 10, 255) {
            cp.freq_clamp[0] = lo as u8;
        }
        if int_row(ui, pal, "Raw max", &mut hi, 10, 255) {
            cp.freq_clamp[1] = hi as u8;
        }
        w::dim(
            ui,
            pal,
            format!(
                "raw {}–{} = {:.0}–{:.0} Hz",
                cp.freq_clamp[0],
                cp.freq_clamp[1],
                raw_to_hz(cp.freq_clamp[1]),
                raw_to_hz(cp.freq_clamp[0])
            ),
        );
        let mut ilo = cp.intensity_clamp[0] as u32;
        let mut ihi = cp.intensity_clamp[1] as u32;
        if int_row(ui, pal, "Int. min %", &mut ilo, 0, 100) {
            cp.intensity_clamp[0] = ilo as u8;
        }
        if int_row(ui, pal, "Int. max %", &mut ihi, 0, 100) {
            cp.intensity_clamp[1] = ihi as u8;
        }
    }
    ui.add_space(space::ITEM_GAP);
    let caps = env.snap.graph.caps;
    ui.label(
        egui::RichText::new(format!(
            "Limit pins are requests: the effective max is never above the Graph cap ({} / {}) set in Setup.",
            caps[0], caps[1]
        ))
        .font(font::small())
        .color(pal.warning),
    );
}

fn byline(e: &GraphPresetEntry) -> String {
    let who = if e.preset.author.trim().is_empty() {
        if e.user { "mine".to_string() } else { "built-in".to_string() }
    } else {
        format!("by {}", e.preset.author.trim())
    };
    match e.preset.kind {
        GraphPresetKind::Fragment => format!("{who} · fragment"),
        GraphPresetKind::Graph => who,
    }
}

fn presets(ui: &mut Ui, ed: &mut GraphEditor, env: &Env, dialog: &mut Option<GraphDialog>, nickname: &str) {
    let pal = env.pal;
    let entries = env
        .bridge
        .shared
        .graph_presets
        .read()
        .map(|p| p.entries.clone())
        .unwrap_or_default();
    let needle = ed.preset_filter.to_lowercase();
    let visible: Vec<&GraphPresetEntry> = entries
        .iter()
        .filter(|e| !ed.preset_mine_only || e.user)
        .filter(|e| {
            needle.is_empty()
                || e.name().to_lowercase().contains(&needle)
                || e.preset.description.to_lowercase().contains(&needle)
                || e.preset.author.to_lowercase().contains(&needle)
        })
        .collect();

    w::sheet(ui, pal, |ui| {
        w::heading_with_suffix(ui, pal, "Graph presets", Some(&format!("{}", entries.len())));
        ui.horizontal(|ui| {
            w::text_field(ui, pal, &mut ed.preset_filter, "search…", 150.0);
            let mine = ed.preset_mine_only;
            if let Some(i) = w::segmented(ui, pal, &["All", "Mine"], if mine { 1 } else { 0 }) {
                ed.preset_mine_only = i == 1;
            }
        });
        ui.add_space(space::ROW_GAP);
        if visible.is_empty() {
            empty_note(ui, pal, if entries.is_empty() { "no presets" } else { "nothing matches those filters" });
        }
        let mut delete = None;
        egui::ScrollArea::vertical()
            .max_height(230.0)
            .auto_shrink([false, true])
            .id_salt("graph_preset_list")
            .show(ui, |ui| {
                for e in &visible {
                    let selected = ed.sel_preset.as_deref() == Some(e.id.as_str());
                    let (rect, resp) = w::row(ui, pal, selected, 36.0);
                    let inner = rect.shrink2(vec2(8.0, 3.0));
                    w::text_in(
                        ui,
                        Rect::from_min_size(inner.min, vec2(inner.width() - 44.0, 15.0)),
                        e.name(),
                        font::body(),
                        if selected { pal.accent } else { pal.contrast },
                        Align2::LEFT_CENTER,
                    );
                    w::text_in(
                        ui,
                        Rect::from_min_size(inner.min + vec2(0.0, 16.0), vec2(inner.width(), 13.0)),
                        &byline(e),
                        font::tiny(),
                        pal.dim,
                        Align2::LEFT_CENTER,
                    );
                    if e.user {
                        let t = Rect::from_min_size(egui::pos2(inner.right() - 40.0, inner.top() + 1.0), vec2(40.0, 15.0));
                        ui.painter().rect_stroke(t, w::SQUARE, Stroke::new(1.0, pal.accent), StrokeKind::Inside);
                        w::text_in(ui, t, "mine", font::tiny(), pal.accent, Align2::CENTER_CENTER);
                    }
                    if resp.clicked() {
                        ed.sel_preset = Some(e.id.clone());
                    }
                    if e.user {
                        resp.context_menu(|ui| {
                            if ui.button("Delete preset").clicked() {
                                delete = Some((*e).clone());
                                ui.close();
                            }
                        });
                    }
                }
            });
        if let Some(e) = delete {
            *dialog = Some(GraphDialog::ConfirmDelete {
                id: e.id.clone(),
                name: e.name().to_string(),
            });
        }
    });

    ui.add_space(space::SHEET_GAP);
    let selected = ed
        .sel_preset
        .as_ref()
        .and_then(|id| entries.iter().find(|e| &e.id == id))
        .cloned();
    w::sheet(ui, pal, |ui| {
        w::heading(ui, pal, "Preset");
        let Some(e) = selected else {
            empty_note(ui, pal, "select a preset to preview it");
            return;
        };
        ui.label(egui::RichText::new(e.name()).font(font::mono_big()).color(pal.contrast));
        w::dim(ui, pal, byline(&e));
        if !e.preset.description.is_empty() {
            ui.add_space(4.0);
            w::body(ui, pal, e.preset.description.clone());
        }
        ui.add_space(4.0);
        w::dim(
            ui,
            pal,
            format!("{} node(s) · {} device slot(s)", e.preset.node_count(), e.preset.device_slots()),
        );
        ui.add_space(4.0);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 96.0), Sense::hover());
        super::canvas::thumbnail(ui, pal, rect, &e.preset.graph);
        ui.add_space(space::ITEM_GAP);
        ui.horizontal_wrapped(|ui| {
            if e.preset.kind == GraphPresetKind::Graph
                && w::cta(ui, pal, "Apply")
                    .on_hover_text("Replace the whole graph (one undo step)")
                    .clicked()
            {
                *dialog = Some(GraphDialog::ConfirmApply { id: e.id.clone() });
            }
            if w::button(ui, pal, "Insert")
                .on_hover_text("Add next to what is there; device slots reuse your Device nodes")
                .clicked()
            {
                let at = ed.pointer_canvas.unwrap_or_else(|| ed.view_centre());
                ed.insert_preset(&e.preset, &env.snap.devices, at);
            }
            if w::button(ui, pal, "Export").clicked() {
                env.bridge.send(crate::bridge::Command::ExportGraphPreset(e.id.clone()));
            }
            if e.user {
                if w::button(ui, pal, "Rename").clicked() {
                    *dialog = Some(GraphDialog::Rename {
                        id: e.id.clone(),
                        name: e.name().to_string(),
                    });
                }
                if w::danger_button(ui, pal, "Delete", true).clicked() {
                    *dialog = Some(GraphDialog::ConfirmDelete {
                        id: e.id.clone(),
                        name: e.name().to_string(),
                    });
                }
            }
        });
        ui.add_space(4.0);
        w::dim(ui, pal, "Presets never store a device's MAC — slots bind to connected devices in order.");
    });

    ui.add_space(space::SHEET_GAP);
    w::sheet(ui, pal, |ui| {
        w::heading(ui, pal, "Save & import");
        ui.horizontal_wrapped(|ui| {
            if w::button(ui, pal, "Save graph as preset…").clicked() {
                *dialog = Some(GraphDialog::save_preset(nickname, false));
            }
            if w::button_enabled(ui, pal, "Save selection…", !ed.selection.is_empty())
                .on_hover_text("A fragment: the selected nodes and the wires between them")
                .clicked()
            {
                *dialog = Some(GraphDialog::save_preset(nickname, true));
            }
            if w::button(ui, pal, "Import…").clicked() {
                *dialog = Some(GraphDialog::ImportPreset { path: String::new() });
            }
        });
    });
}

fn devices(ui: &mut Ui, ed: &mut GraphEditor, env: &Env) {
    let pal = env.pal;
    let mut rows: Vec<(String, String, bool, Option<u8>)> = env
        .snap
        .devices
        .iter()
        .map(|d| (d.mac.clone(), d.name.clone(), d.connected, d.battery))
        .collect();
    for n in &ed.doc.nodes {
        if let Some(DeviceBinding::Mac(m)) = n.device_binding()
            && !rows.iter().any(|(mac, ..)| mac.eq_ignore_ascii_case(m))
        {
            rows.push((m.clone(), n.display_title(), false, None));
        }
    }
    w::sheet(ui, pal, |ui| {
        w::heading_with_suffix(
            ui,
            pal,
            "Devices",
            Some(&format!("{} online", rows.iter().filter(|r| r.2).count())),
        );
        if rows.is_empty() {
            empty_note(ui, pal, "no devices yet — they appear when a Coyote connects");
        }
        for (mac, name, online, battery) in &rows {
            let (rect, resp) = w::row(ui, pal, false, 24.0);
            let inner = rect.shrink2(vec2(8.0, 0.0));
            w::text_in(ui, inner, name, font::body(), if *online { pal.contrast } else { pal.dim }, Align2::LEFT_CENTER);
            let state = match (online, battery) {
                (true, Some(b)) => format!("online · {b}%"),
                (true, None) => "online".to_string(),
                (false, _) => "offline".to_string(),
            };
            w::text_in(ui, inner, &state, font::small(), if *online { pal.success } else { pal.error }, Align2::RIGHT_CENTER);
            if resp.clicked()
                && let Some(n) = ed.doc.device_node_for(mac)
            {
                let id = n.id;
                ed.path.clear();
                ed.focus_request = Some(id);
            }
            let out = env.snap.graph.outputs.iter().find(|o| o.mac.eq_ignore_ascii_case(mac));
            for (ci, label) in ["A", "B"].iter().enumerate() {
                let s = out.map(|o| o.channels[ci].safe).unwrap_or_default();
                let hz = raw_to_hz(s.freq[3].max(10));
                let txt = format!(
                    "{label}  power {:>3} / {:<3}  {:>3.0} Hz  {:>3}%   ·  …{}",
                    s.strength,
                    s.limit_max,
                    hz,
                    s.intensity[3],
                    if ci == 0 { short_mac(mac) } else { String::new() }
                );
                ui.label(
                    egui::RichText::new(txt)
                        .font(font::tiny())
                        .color(if s.strength > 0 { pal.accent } else { pal.dim }),
                );
            }
            ui.add_space(2.0);
        }
        ui.add_space(space::ITEM_GAP);
        let caps = env.snap.graph.caps;
        w::dim(ui, pal, format!("Graph cap A {} · B {} — change it in Setup. Click a device to find its node.", caps[0], caps[1]));
    });
}
