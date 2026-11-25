use egui::{Align2, Color32, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use shocking_vrc_core::graph::draw::{
    DrawRect, DrawZoneParams, FaderParams, FeedbackParams, PanParams, SegmentPainterParams, ShapeFillParams,
    StepSeqParams, XyPadParams,
};
use shocking_vrc_core::graph::{NodeId, NodeProbe};

use super::node_ui::Z;
use crate::theme::Palette;
use crate::theme::widgets::{self as w, font};

const HINT_MIN_ZOOM: f32 = 0.6;
const BUTTONS_MIN_ZOOM: f32 = 0.5;
const ZONE_COMMIT_PX: f32 = 4.0;

fn well(ui: &Ui, pal: &Palette, rect: Rect) {
    ui.painter().rect_filled(rect, w::SQUARE, pal.well);
    ui.painter()
        .rect_stroke(rect, w::SQUARE, Stroke::new(1.0, pal.hair), StrokeKind::Inside);
}

fn to_unit(rect: Rect, p: Pos2) -> [f32; 2] {
    let w = rect.width().max(1e-3);
    let h = rect.height().max(1e-3);
    [
        ((p.x - rect.left()) / w).clamp(0.0, 1.0),
        ((rect.bottom() - p.y) / h).clamp(0.0, 1.0),
    ]
}

fn to_screen(rect: Rect, u: [f32; 2]) -> Pos2 {
    pos2(rect.left() + u[0] * rect.width(), rect.bottom() - u[1] * rect.height())
}

fn zone_rect(rect: Rect, z: &DrawRect) -> Rect {
    Rect::from_min_max(to_screen(rect, [z.t0, z.hi]), to_screen(rect, [z.t1, z.lo]))
}

fn playhead(ui: &Ui, pal: &Palette, rect: Rect, phase: Option<f32>) {
    if let Some(t) = phase {
        let x = rect.left() + t.clamp(0.0, 1.0) * rect.width();
        ui.painter()
            .vline(x, rect.y_range(), Stroke::new(1.0, pal.faded(pal.contrast, 0.35)));
    }
}

fn label(ui: &Ui, z: Z, pos: Pos2, anchor: Align2, text: &str, color: Color32) {
    if z.text() {
        ui.painter().text(pos, anchor, text, z.font(font::tiny()), color);
    }
}

fn hint(ui: &Ui, pal: &Palette, rect: Rect, text: &str, z: Z) {
    if z.0 < HINT_MIN_ZOOM {
        return;
    }
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.text(
        rect.left_top() + z.v(4.0, 2.0),
        Align2::LEFT_TOP,
        text,
        z.font(font::tiny()),
        pal.faded(pal.dim, 0.2),
    );
}

fn live_input_ok(ui: &Ui) -> bool {
    !ui.input(|i| i.key_down(egui::Key::Space) || i.pointer.middle_down() || i.pointer.secondary_down())
}

fn primary_dragged(ui: &Ui, r: &Response) -> bool {
    r.dragged_by(egui::PointerButton::Primary) && live_input_ok(ui)
}

fn primary_drag_started(ui: &Ui, r: &Response) -> bool {
    r.drag_started_by(egui::PointerButton::Primary) && live_input_ok(ui)
}

fn primary_held(ui: &Ui, r: &Response) -> bool {
    r.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_down()) && live_input_ok(ui)
}

struct ModeStrip {
    buttons: [Rect; 2],
    visible: bool,
}

fn mode_strip(rect: Rect, z: Z) -> ModeStrip {
    let bw = z.px(40.0);
    let bh = z.px(14.0);
    let buttons = [0, 1].map(|i| {
        Rect::from_min_size(
            pos2(rect.right() - bw * (2 - i) as f32 - z.px(2.0), rect.top() + z.px(2.0)),
            vec2(bw - z.px(2.0), bh),
        )
    });
    ModeStrip {
        buttons,
        visible: z.0 >= BUTTONS_MIN_ZOOM && rect.width() > bw * 2.5,
    }
}

impl ModeStrip {
    fn work_area(&self, rect: Rect, z: Z) -> Rect {
        if self.visible {
            Rect::from_min_max(pos2(rect.left(), self.buttons[0].bottom() + z.px(2.0)), rect.max)
        } else {
            rect
        }
    }
}

fn mode_switch(ui: &mut Ui, pal: &Palette, strip: &ModeStrip, id: egui::Id, labels: [&str; 2], z: Z) -> bool {
    let key = id.with("mode");
    let mut zone_mode = ui.ctx().data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
    if !strip.visible {
        return zone_mode;
    }
    for (i, text) in labels.iter().enumerate() {
        let r = strip.buttons[i];
        let on = (i == 0) == zone_mode;
        let resp = ui.interact(r, id.with(("mode", i)), Sense::click());
        ui.painter()
            .rect_filled(r, w::SQUARE, if on { pal.accent } else { pal.sheet });
        ui.painter().rect_stroke(
            r,
            w::SQUARE,
            Stroke::new(1.0, if on { pal.accent } else { pal.hair }),
            StrokeKind::Inside,
        );
        label(
            ui,
            z,
            r.center(),
            Align2::CENTER_CENTER,
            text,
            if on { pal.on_solid() } else { pal.dim },
        );
        if resp.clicked() {
            zone_mode = i == 0;
        }
    }
    ui.ctx().data_mut(|d| d.insert_temp(key, zone_mode));
    zone_mode
}

fn zone_drag(ui: &mut Ui, rect: Rect, area: Rect, id: egui::Id, zone: &mut DrawRect) -> bool {
    let resp = ui.interact(area, id.with("zone"), Sense::drag());
    let start_key = id.with("zone_start");
    if primary_drag_started(ui, &resp)
        && let Some(p) = resp.interact_pointer_pos()
    {
        ui.ctx().data_mut(|d| d.insert_temp(start_key, p));
    }
    if primary_dragged(ui, &resp)
        && let (Some(p), Some(a)) = (resp.interact_pointer_pos(), ui.ctx().data(|d| d.get_temp::<Pos2>(start_key)))
        && (p - a).length() >= ZONE_COMMIT_PX
    {
        *zone = DrawRect::from_corners(to_unit(rect, a), to_unit(rect, p));
        return true;
    }
    false
}

fn draw_zone_box(ui: &Ui, pal: &Palette, rect: Rect, z: &DrawRect) {
    let zr = zone_rect(rect, z);
    ui.painter().rect_filled(zr, w::SQUARE, pal.faded(pal.accent, 0.85));
    w::dashed_rect(ui, zr, pal.accent);
}

pub fn draw_zone(
    ui: &mut Ui,
    pal: &Palette,
    rect: Rect,
    p: &mut DrawZoneParams,
    probe: Option<&NodeProbe>,
    id: NodeId,
    zoom: f32,
) {
    let z = Z(zoom);
    well(ui, pal, rect);
    let base = ui.id().with(("draw_zone", id.0));
    draw_zone_box(ui, pal, rect, &p.zone);
    let zr = zone_rect(rect, &p.zone);
    let pts: Vec<Pos2> = p
        .points
        .iter()
        .map(|q| pos2(zr.left() + q[0] * zr.width(), zr.bottom() - q[1] * zr.height()))
        .collect();
    if pts.len() > 1 {
        ui.painter().add(egui::Shape::line(pts, Stroke::new(1.5, pal.contrast)));
    }
    playhead(ui, pal, rect, probe.and_then(|pr| pr.outputs.get(1)).copied());

    let strip = mode_strip(rect, z);
    let area = strip.work_area(rect, z);
    let zone_mode = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(base.with("mode")))
        .unwrap_or(false);
    if zone_mode {
        hint(ui, pal, rect, "drag a zone (time × level)", z);
        zone_drag(ui, rect, area, base, &mut p.zone);
    } else {
        hint(ui, pal, rect, "draw · shift adds to the line", z);
        let resp = ui.interact(area, base.with("pen"), Sense::drag());
        if primary_drag_started(ui, &resp) && !ui.input(|i| i.modifiers.shift) {
            p.points.clear();
        }
        if primary_dragged(ui, &resp)
            && let Some(pos) = resp.interact_pointer_pos()
            && zr.width() > 1.0
            && zr.height() > 1.0
        {
            let x = ((pos.x - zr.left()) / zr.width()).clamp(0.0, 1.0);
            let y = ((zr.bottom() - pos.y) / zr.height()).clamp(0.0, 1.0);
            p.points.retain(|q| (q[0] - x).abs() > 0.004);
            p.points.push([x, y]);
            p.points.sort_by(|a, b| a[0].total_cmp(&b[0]));
            p.points.truncate(shocking_vrc_core::graph::draw::MAX_DRAW_POINTS);
        }
    }
    mode_switch(ui, pal, &strip, base, ["zone", "pen"], z);
}

pub fn shape_fill(
    ui: &mut Ui,
    pal: &Palette,
    rect: Rect,
    p: &mut ShapeFillParams,
    probe: Option<&NodeProbe>,
    id: NodeId,
    zoom: f32,
) {
    let z = Z(zoom);
    well(ui, pal, rect);
    let base = ui.id().with(("shape_fill", id.0));
    draw_zone_box(ui, pal, rect, &p.zone);
    let zr = zone_rect(rect, &p.zone);
    let duty = probe.and_then(|pr| pr.inputs.get(1)).copied().unwrap_or(0.5);
    let cycles = 3.0;
    let pts: Vec<Pos2> = (0..=96)
        .map(|i| {
            let t = i as f32 / 96.0;
            let walk = 0.5 + 0.35 * (t * 17.0).sin() * (t * 5.0).cos();
            let v = p.shape.value(t * cycles, duty, walk);
            pos2(zr.left() + t * zr.width(), zr.bottom() - v * zr.height())
        })
        .collect();
    ui.painter().add(egui::Shape::line(pts, Stroke::new(1.5, pal.contrast)));
    playhead(ui, pal, rect, probe.and_then(|pr| pr.outputs.get(1)).copied());
    hint(ui, pal, rect, "drag to set the zone", z);
    zone_drag(ui, rect, rect, base, &mut p.zone);
}

pub fn xy_pad(ui: &mut Ui, pal: &Palette, rect: Rect, p: &mut XyPadParams, id: NodeId, zoom: f32) {
    let z = Z(zoom);
    let pad = rect;
    well(ui, pal, pad);
    w::hatch(ui, pad.shrink(1.0), pal.faded(pal.contrast, 0.94), z.px(14.0).max(4.0));
    if p.mode == shocking_vrc_core::graph::draw::XyMode::PanPower {
        ui.painter()
            .vline(pad.center().x, pad.y_range(), Stroke::new(1.0, pal.faded(pal.hair, 0.3)));
    }
    let resp = ui.interact(pad, ui.id().with(("xy", id.0)), Sense::drag());
    if primary_dragged(ui, &resp)
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let [x, y] = to_unit(pad, pos);
        p.x = x;
        p.y = y;
        p.touching = true;
    } else if p.touching && !primary_held(ui, &resp) {
        p.touching = false;
        if p.spring {
            p.y = 0.0;
        }
    }
    let c = to_screen(pad, [p.x, p.y]);
    let col = if p.touching { pal.accent } else { pal.contrast };
    ui.painter()
        .hline(pad.x_range(), c.y, Stroke::new(1.0, pal.faded(col, 0.6)));
    ui.painter()
        .vline(c.x, pad.y_range(), Stroke::new(1.0, pal.faded(col, 0.6)));
    let dot = z.px(10.0).max(5.0);
    ui.painter()
        .rect_filled(Rect::from_center_size(c, vec2(dot, dot)), w::SQUARE, col);
    if zoom >= HINT_MIN_ZOOM {
        let (xl, yl) = p.mode.axis_labels();
        label(ui, z, pad.right_bottom() - z.v(4.0, 2.0), Align2::RIGHT_BOTTOM, xl, pal.dim);
        label(ui, z, pad.left_top() + z.v(4.0, 2.0), Align2::LEFT_TOP, yl, pal.dim);
        label(
            ui,
            z,
            pad.right_top() + z.v(-4.0, 2.0),
            Align2::RIGHT_TOP,
            p.mode.label(),
            pal.faded(pal.dim, 0.3),
        );
    }
}

pub fn segment_painter(ui: &mut Ui, pal: &Palette, rect: Rect, p: &mut SegmentPainterParams, id: NodeId, zoom: f32) {
    let z = Z(zoom);
    let base = ui.id().with(("segments", id.0));
    let half = rect.width() / 2.0;
    for ch in 0..2 {
        let col = Rect::from_min_size(
            pos2(rect.left() + ch as f32 * half, rect.top()),
            vec2(half - z.px(4.0), rect.height()),
        );
        let gap = z.px(2.0);
        let rows = [
            Rect::from_min_size(col.min, vec2(col.width(), col.height() * 0.5 - gap)),
            Rect::from_min_size(
                pos2(col.left(), col.top() + col.height() * 0.5 + gap),
                vec2(col.width(), col.height() * 0.5 - gap),
            ),
        ];
        for (k, area) in rows.iter().enumerate() {
            well(ui, pal, *area);
            let values = if k == 0 { &mut p.freq[ch] } else { &mut p.int[ch] };
            let color = if k == 0 { pal.accent } else { pal.success };
            let bw = area.width() / 4.0;
            for (s, v) in values.iter().enumerate() {
                let bar = Rect::from_min_max(
                    pos2(area.left() + s as f32 * bw + gap, area.bottom() - v * area.height()),
                    pos2(area.left() + (s + 1) as f32 * bw - gap, area.bottom()),
                );
                ui.painter().rect_filled(bar, w::SQUARE, color);
            }
            let resp = ui.interact(*area, base.with((ch, k)), Sense::click_and_drag());
            if (primary_dragged(ui, &resp) || resp.clicked())
                && let Some(pos) = resp.interact_pointer_pos()
            {
                let [x, y] = to_unit(*area, pos);
                let s = ((x * 4.0) as usize).min(3);
                values[s] = (y * 100.0).round() / 100.0;
            }
            if zoom >= HINT_MIN_ZOOM {
                label(
                    ui,
                    z,
                    area.left_top() + z.v(3.0, 1.0),
                    Align2::LEFT_TOP,
                    &format!("{} {}", ["A", "B"][ch], if k == 0 { "freq" } else { "int" }),
                    pal.dim,
                );
            }
        }
    }
}

pub fn fader(
    ui: &mut Ui,
    pal: &Palette,
    rect: Rect,
    p: &mut FaderParams,
    probe: Option<&NodeProbe>,
    id: NodeId,
    zoom: f32,
) {
    let z = Z(zoom);
    let base = ui.id().with(("fader", id.0));
    let track = Rect::from_min_size(rect.min, vec2(z.px(28.0), rect.height()));
    well(ui, pal, track);
    let fill = Rect::from_min_max(pos2(track.left(), track.bottom() - p.level * track.height()), track.max);
    ui.painter().rect_filled(fill.shrink(z.px(2.0)), w::SQUARE, pal.accent);
    let resp = ui.interact(track, base.with("level"), Sense::click_and_drag());
    if (primary_dragged(ui, &resp) || resp.clicked())
        && let Some(pos) = resp.interact_pointer_pos()
    {
        p.level = (to_unit(track, pos)[1] * 100.0).round() / 100.0;
    }

    let button_h = z.px(22.0);
    let gap = z.px(6.0);
    let env_rect = Rect::from_min_max(
        pos2(track.right() + gap, rect.top()),
        pos2(rect.right(), rect.bottom() - button_h - z.px(4.0)),
    );
    well(ui, pal, env_rect);
    let total = (p.attack + p.decay + p.release + 0.5).max(0.01);
    let x = |t: f32| env_rect.left() + t / total * env_rect.width();
    let y = |v: f32| env_rect.bottom() - v * env_rect.height();
    let pts = vec![
        pos2(x(0.0), y(0.0)),
        pos2(x(p.attack), y(1.0)),
        pos2(x(p.attack + p.decay), y(p.sustain)),
        pos2(x(p.attack + p.decay + 0.5), y(p.sustain)),
        pos2(x(total), y(0.0)),
    ];
    ui.painter().add(egui::Shape::line(pts, Stroke::new(1.5, pal.contrast)));
    if let Some(env) = probe.and_then(|pr| pr.outputs.get(1)) {
        ui.painter()
            .hline(env_rect.x_range(), y(*env), Stroke::new(1.0, pal.accent));
    }

    let button = Rect::from_min_max(pos2(env_rect.left(), rect.bottom() - button_h), rect.right_bottom());
    let b = ui.interact(button, base.with("press"), Sense::click_and_drag());
    if p.latch {
        if b.clicked() {
            p.pressed = !p.pressed;
        }
    } else {
        p.pressed = primary_held(ui, &b);
    }
    ui.painter()
        .rect_filled(button, w::SQUARE, if p.pressed { pal.accent } else { pal.sheet });
    ui.painter().rect_stroke(
        button,
        w::SQUARE,
        Stroke::new(1.0, if p.pressed { pal.accent } else { pal.hair }),
        StrokeKind::Inside,
    );
    let text = match (p.latch, p.pressed) {
        (true, true) => "ON · click to stop",
        (true, false) => "click to start",
        (false, true) => "HELD",
        (false, false) => "hold to trigger",
    };
    let short = match (p.latch, p.pressed) {
        (true, true) => "ON",
        (true, false) => "START",
        (false, true) => "HELD",
        (false, false) => "HOLD",
    };
    let font = z.font(font::tiny());
    let fits = ui
        .painter()
        .layout_no_wrap(text.to_string(), font.clone(), pal.dim)
        .size()
        .x
        <= button.width() - z.px(6.0);
    label(
        ui,
        z,
        button.center(),
        Align2::CENTER_CENTER,
        if fits { text } else { short },
        if p.pressed { pal.on_solid() } else { pal.dim },
    );
}

pub fn step_sequencer(
    ui: &mut Ui,
    pal: &Palette,
    rect: Rect,
    p: &mut StepSeqParams,
    probe: Option<&NodeProbe>,
    id: NodeId,
    zoom: f32,
) {
    let z = Z(zoom);
    let base = ui.id().with(("stepseq", id.0));
    let n = p.steps.max(1) as usize;
    let current = probe.and_then(|pr| pr.outputs.get(3)).map(|v| v.round() as usize);
    let gap = z.px(4.0);
    let lanes = [
        Rect::from_min_size(rect.min, vec2(rect.width(), rect.height() * 0.5 - gap * 0.5)),
        Rect::from_min_max(pos2(rect.left(), rect.center().y + gap * 0.5), rect.max),
    ];
    for (li, lane) in lanes.iter().enumerate() {
        well(ui, pal, *lane);
        let color = if li == 0 { pal.accent } else { pal.success };
        let cw = lane.width() / n as f32;
        let values = if li == 0 { &mut p.a } else { &mut p.b };
        for s in 0..n {
            let cell = Rect::from_min_size(pos2(lane.left() + s as f32 * cw, lane.top()), vec2(cw, lane.height()));
            if current == Some(s) {
                ui.painter()
                    .rect_filled(cell.shrink(z.px(1.0)), w::SQUARE, pal.faded(pal.contrast, 0.85));
            }
            if s % 4 == 0 && s > 0 {
                ui.painter()
                    .vline(cell.left(), lane.y_range(), Stroke::new(1.0, pal.faded(pal.hair, 0.2)));
            }
            let v = values.get(s).copied().unwrap_or(0.0);
            if v > 0.0 {
                let bar = Rect::from_min_max(
                    pos2(cell.left() + z.px(2.0), cell.bottom() - v * (cell.height() - z.px(2.0))),
                    pos2(cell.right() - z.px(2.0), cell.bottom() - z.px(1.0)),
                );
                ui.painter().rect_filled(bar, w::SQUARE, color);
            }
        }
        let resp = ui.interact(*lane, base.with(("lane", li)), Sense::click_and_drag());
        if let Some(pos) = resp.interact_pointer_pos() {
            let [x, y] = to_unit(*lane, pos);
            let s = ((x * n as f32) as usize).min(n - 1);
            if (primary_dragged(ui, &resp) || resp.clicked()) && s < values.len() {
                values[s] = ((y * 20.0).round() / 20.0).clamp(0.0, 1.0);
            }
            if resp.secondary_clicked() && s < values.len() {
                values[s] = 0.0;
            }
        }
        if zoom >= HINT_MIN_ZOOM {
            label(
                ui,
                z,
                lane.left_top() + z.v(3.0, 1.0),
                Align2::LEFT_TOP,
                ["A", "B"][li],
                pal.dim,
            );
        }
    }
}

pub fn pan(ui: &mut Ui, pal: &Palette, rect: Rect, _p: &mut PanParams, probe: Option<&NodeProbe>, zoom: f32) {
    let z = Z(zoom);
    well(ui, pal, rect);
    let a = probe.and_then(|pr| pr.outputs.first()).copied().unwrap_or(0.0);
    let b = probe.and_then(|pr| pr.outputs.get(1)).copied().unwrap_or(0.0);
    let half = rect.width() * 0.5;
    let inner = rect.shrink(z.px(3.0));
    let ra = Rect::from_min_max(
        pos2(inner.center().x - (half - z.px(3.0)) * a.clamp(0.0, 1.0), inner.top()),
        pos2(inner.center().x, inner.bottom()),
    );
    let rb = Rect::from_min_max(
        pos2(inner.center().x, inner.top()),
        pos2(inner.center().x + (half - z.px(3.0)) * b.clamp(0.0, 1.0), inner.bottom()),
    );
    ui.painter().rect_filled(ra, w::SQUARE, pal.accent);
    ui.painter().rect_filled(rb, w::SQUARE, pal.success);
    ui.painter()
        .vline(rect.center().x, rect.y_range(), Stroke::new(1.0, pal.hair));
    label(
        ui,
        z,
        rect.left_center() + z.v(4.0, 0.0),
        Align2::LEFT_CENTER,
        &format!("A {:.0}%", a * 100.0),
        pal.contrast,
    );
    label(
        ui,
        z,
        rect.right_center() - z.v(4.0, 0.0),
        Align2::RIGHT_CENTER,
        &format!("B {:.0}%", b * 100.0),
        pal.contrast,
    );
}

pub fn partner_feedback(
    ui: &mut Ui,
    pal: &Palette,
    rect: Rect,
    _p: &mut FeedbackParams,
    probe: Option<&NodeProbe>,
    zoom: f32,
) {
    let z = Z(zoom);
    well(ui, pal, rect);
    let o = |k: usize| probe.and_then(|pr| pr.outputs.get(k)).copied().unwrap_or(0.0);
    let rows = [
        ("their A", o(0), pal.accent),
        ("their B", o(1), pal.success),
        ("wish", o(5), pal.warning),
    ];
    let rh = rect.height() / rows.len() as f32;
    let label_w = if z.text() { z.px(52.0) } else { 0.0 };
    for (k, (name, v, color)) in rows.iter().enumerate() {
        let row = Rect::from_min_size(pos2(rect.left(), rect.top() + k as f32 * rh), vec2(rect.width(), rh));
        label(ui, z, row.left_center() + z.v(4.0, 0.0), Align2::LEFT_CENTER, name, pal.dim);
        let track = Rect::from_min_max(
            pos2(row.left() + label_w + z.px(4.0), row.top() + z.px(3.0)),
            pos2(row.right() - z.px(4.0), row.bottom() - z.px(3.0)),
        );
        let bipolar = k == 2;
        let bar = if bipolar {
            let mid = track.center().x;
            let end = mid + track.width() * 0.5 * v.clamp(-1.0, 1.0);
            Rect::from_min_max(pos2(mid.min(end), track.top()), pos2(mid.max(end), track.bottom()))
        } else {
            Rect::from_min_max(
                track.min,
                pos2(track.left() + track.width() * v.clamp(0.0, 1.0), track.bottom()),
            )
        };
        let fill = match (bipolar, *v < 0.0) {
            (true, true) => pal.warning,
            (true, false) => pal.success,
            _ => *color,
        };
        ui.painter().rect_filled(bar, w::SQUARE, fill);
        ui.painter()
            .rect_stroke(track, w::SQUARE, Stroke::new(1.0, pal.hair), StrokeKind::Inside);
        if bipolar {
            ui.painter()
                .vline(track.center().x, track.y_range(), Stroke::new(1.0, pal.contrast));
            label(
                ui,
                z,
                track.right_center() - z.v(3.0, 0.0),
                Align2::RIGHT_CENTER,
                &format!("{:+.1}", v),
                pal.contrast,
            );
        }
    }
    let live = o(2) > 0.5;
    let dot = Rect::from_center_size(rect.right_top() + z.v(-6.0, 6.0), z.v(6.0, 6.0));
    ui.painter()
        .rect_filled(dot, w::SQUARE, if live { pal.success } else { pal.dim });
    if o(3) > 0.5 || o(4) > 0.5 {
        ui.painter().rect_stroke(
            rect,
            w::SQUARE,
            Stroke::new(2.0, if o(3) > 0.5 { pal.success } else { pal.warning }),
            StrokeKind::Inside,
        );
    }
}
