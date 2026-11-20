use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use shocking_vrc_core::graph::draw::{
    DrawRect, DrawZoneParams, FaderParams, PlacedStamp, SegmentPainterParams, ShapeFillParams, StampParams,
    XyPadParams, sample_levels,
};
use shocking_vrc_core::graph::{NodeId, NodeProbe};

use crate::theme::Palette;
use crate::theme::widgets::{self as w, font};

fn well(ui: &Ui, pal: &Palette, rect: Rect) {
    ui.painter().rect_filled(rect, w::SQUARE, pal.well);
    ui.painter()
        .rect_stroke(rect, w::SQUARE, Stroke::new(1.0, pal.hair), StrokeKind::Inside);
}

fn to_unit(rect: Rect, p: Pos2) -> [f32; 2] {
    [
        ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0),
        ((rect.bottom() - p.y) / rect.height()).clamp(0.0, 1.0),
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

fn hint(ui: &Ui, pal: &Palette, rect: Rect, text: &str, zoom: f32) {
    if zoom < 0.6 {
        return;
    }
    ui.painter().text(
        rect.left_top() + vec2(4.0, 2.0),
        Align2::LEFT_TOP,
        text,
        font::tiny(),
        pal.faded(pal.dim, 0.2),
    );
}

fn mode_switch(ui: &mut Ui, pal: &Palette, rect: Rect, id: egui::Id, labels: [&str; 2]) -> bool {
    let key = id.with("mode");
    let mut zone_mode = ui.ctx().data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
    let bw = 40.0;
    for (i, label) in labels.iter().enumerate() {
        let r = Rect::from_min_size(
            pos2(rect.right() - bw * (2 - i) as f32 - 2.0, rect.top() + 2.0),
            vec2(bw - 2.0, 14.0),
        );
        let on = (i == 0) == zone_mode;
        let resp = ui.interact(r, id.with(("mode", i)), Sense::click());
        ui.painter()
            .rect_filled(r, w::SQUARE, if on { pal.accent } else { pal.sheet });
        ui.painter().text(
            r.center(),
            Align2::CENTER_CENTER,
            *label,
            font::tiny(),
            if on { pal.on_solid() } else { pal.dim },
        );
        if resp.clicked() {
            zone_mode = i == 0;
        }
    }
    ui.ctx().data_mut(|d| d.insert_temp(key, zone_mode));
    zone_mode
}

fn zone_drag(ui: &mut Ui, rect: Rect, id: egui::Id, zone: &mut DrawRect) -> bool {
    let resp = ui.interact(rect, id.with("zone"), Sense::drag());
    let start_key = id.with("zone_start");
    if resp.drag_started()
        && let Some(p) = resp.interact_pointer_pos()
    {
        ui.ctx().data_mut(|d| d.insert_temp(start_key, to_unit(rect, p)));
    }
    if resp.dragged()
        && let (Some(p), Some(a)) = (
            resp.interact_pointer_pos(),
            ui.ctx().data(|d| d.get_temp::<[f32; 2]>(start_key)),
        )
    {
        *zone = DrawRect::from_corners(a, to_unit(rect, p));
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
    let zone_mode = mode_switch(ui, pal, rect, base, ["zone", "pen"]);
    if zone_mode {
        hint(ui, pal, rect, "drag a zone (time × level)", zoom);
        zone_drag(ui, rect, base, &mut p.zone);
        return;
    }
    hint(ui, pal, rect, "draw inside the zone", zoom);
    let resp = ui.interact(zr, base.with("pen"), Sense::drag());
    if resp.drag_started() {
        p.points.clear();
    }
    if resp.dragged()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let x = ((pos.x - zr.left()) / zr.width()).clamp(0.0, 1.0);
        let y = ((zr.bottom() - pos.y) / zr.height()).clamp(0.0, 1.0);
        p.points.retain(|q| (q[0] - x).abs() > 0.004);
        p.points.push([x, y]);
        p.points.sort_by(|a, b| a[0].total_cmp(&b[0]));
        p.points.truncate(shocking_vrc_core::graph::draw::MAX_DRAW_POINTS);
    }
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
    hint(ui, pal, rect, "drag to set the zone", zoom);
    zone_drag(ui, rect, base, &mut p.zone);
}

fn stamp_shape(ui: &Ui, rect: Rect, levels: &[f32], color: Color32) {
    let pts: Vec<Pos2> = (0..=24)
        .map(|i| {
            let t = i as f32 / 24.0;
            pos2(
                rect.left() + t * rect.width(),
                rect.bottom() - sample_levels(levels, t) * rect.height(),
            )
        })
        .collect();
    ui.painter().add(egui::Shape::line(pts, Stroke::new(1.2, color)));
}

pub fn stamp_pattern(
    ui: &mut Ui,
    pal: &Palette,
    rect: Rect,
    p: &mut StampParams,
    probe: Option<&NodeProbe>,
    id: NodeId,
    zoom: f32,
) {
    let base = ui.id().with(("stamps", id.0));
    let sel_key = base.with("selected");
    let clip_key = base.with("clipboard");
    let mut selected = ui.ctx().data(|d| d.get_temp::<usize>(sel_key)).unwrap_or(0);
    selected = selected.min(p.stamps.len().saturating_sub(1));

    let palette_h = 26.0f32.min(rect.height() * 0.3);
    let strip = Rect::from_min_size(rect.min, vec2(rect.width(), palette_h));
    let lane = Rect::from_min_max(pos2(rect.left(), strip.bottom() + 4.0), rect.max);

    let n = p.stamps.len().max(1);
    let cw = strip.width() / n as f32;
    for (i, s) in p.stamps.iter().enumerate() {
        let r = Rect::from_min_size(pos2(strip.left() + i as f32 * cw, strip.top()), vec2(cw - 3.0, strip.height()));
        let on = i == selected;
        ui.painter()
            .rect_filled(r, w::SQUARE, if on { pal.faded(pal.accent, 0.7) } else { pal.well });
        ui.painter().rect_stroke(
            r,
            w::SQUARE,
            Stroke::new(1.0, if on { pal.accent } else { pal.hair }),
            StrokeKind::Inside,
        );
        stamp_shape(ui, r.shrink(3.0), &s.levels, if on { pal.contrast } else { pal.dim });
        if ui
            .interact(r, base.with(("pal", i)), Sense::click())
            .on_hover_text(&s.name)
            .clicked()
        {
            selected = i;
        }
    }

    well(ui, pal, lane);
    if p.snap > 0.0 {
        let mut x = 0.0;
        while x <= 1.0 {
            let px = lane.left() + x * lane.width();
            ui.painter()
                .vline(px, lane.y_range(), Stroke::new(1.0, pal.faded(pal.hair, 0.5)));
            x += p.snap.max(0.02);
        }
    }
    let bg = ui.interact(lane, base.with("lane"), Sense::click());
    let mut remove = None;
    let mut hovered_stamp = None;
    for k in 0..p.lane.len() {
        let pl = p.lane[k];
        let r = Rect::from_min_max(
            pos2(lane.left() + pl.start * lane.width(), lane.top() + 2.0),
            pos2(lane.left() + (pl.start + pl.len).min(1.0) * lane.width(), lane.bottom() - 2.0),
        );
        ui.painter().rect_filled(r, w::SQUARE, pal.faded(pal.accent, 0.85));
        let shape_r = Rect::from_min_max(pos2(r.left(), r.bottom() - r.height() * pl.gain), r.max);
        if let Some(def) = p.stamps.get(pl.stamp as usize) {
            stamp_shape(ui, shape_r, &def.levels, pal.accent);
        }
        let body = ui.interact(r, base.with(("placed", k)), Sense::click_and_drag());
        let edge = Rect::from_min_max(pos2(r.right() - 5.0, r.top()), r.max);
        let grip = ui.interact(edge, base.with(("edge", k)), Sense::drag());
        if body.hovered() || grip.hovered() {
            hovered_stamp = Some(k);
            ui.ctx().set_cursor_icon(if grip.hovered() {
                egui::CursorIcon::ResizeHorizontal
            } else {
                egui::CursorIcon::Grab
            });
        }
        if grip.dragged()
            && let Some(pos) = grip.interact_pointer_pos()
        {
            let end = p.snapped(((pos.x - lane.left()) / lane.width()).clamp(0.0, 1.0));
            p.lane[k].len = (end - p.lane[k].start).max(p.snap.max(0.02));
        } else if body.dragged() {
            let dx = body.drag_delta().x / lane.width();
            let raw_key = base.with(("raw", k));
            let raw = ui.ctx().data(|d| d.get_temp::<f32>(raw_key)).unwrap_or(pl.start) + dx;
            ui.ctx().data_mut(|d| d.insert_temp(raw_key, raw));
            p.lane[k].start = p.snapped(raw.clamp(0.0, 1.0 - pl.len.min(1.0)));
        } else {
            ui.ctx().data_mut(|d| d.remove::<f32>(base.with(("raw", k))));
        }
        if body.secondary_clicked() {
            remove = Some(k);
        }
    }
    if let Some(k) = remove {
        p.lane.remove(k);
    }
    if bg.clicked()
        && hovered_stamp.is_none()
        && let Some(pos) = bg.interact_pointer_pos()
        && p.lane.len() < shocking_vrc_core::graph::draw::MAX_PLACED
    {
        let start = p.snapped(((pos.x - lane.left()) / lane.width()).clamp(0.0, 1.0));
        let len = p.snap.max(0.0625) * 2.0;
        p.lane.push(PlacedStamp {
            stamp: selected as u16,
            start: start.min(1.0 - len.min(1.0)),
            len,
            gain: 1.0,
        });
    }
    let (copy, paste) = ui.input(|i| {
        (
            i.modifiers.command && i.key_pressed(egui::Key::C),
            i.modifiers.command && i.key_pressed(egui::Key::V),
        )
    });
    if copy && let Some(k) = hovered_stamp {
        ui.ctx().data_mut(|d| d.insert_temp(clip_key, p.lane[k]));
    }
    if paste
        && bg.hovered()
        && let (Some(clip), Some(pos)) = (
            ui.ctx().data(|d| d.get_temp::<PlacedStamp>(clip_key)),
            ui.input(|i| i.pointer.hover_pos()),
        )
        && p.lane.len() < shocking_vrc_core::graph::draw::MAX_PLACED
    {
        let start = p.snapped(((pos.x - lane.left()) / lane.width()).clamp(0.0, 1.0));
        p.lane.push(PlacedStamp {
            start: start.min(1.0 - clip.len.min(1.0)),
            ..clip
        });
    }
    playhead(ui, pal, lane, probe.and_then(|pr| pr.outputs.get(1)).copied());
    hint(
        ui,
        pal,
        lane,
        "click: stamp · drag: move · edge: length · right-click: remove",
        zoom,
    );
    ui.ctx().data_mut(|d| d.insert_temp(sel_key, selected));
}

pub fn xy_pad(ui: &mut Ui, pal: &Palette, rect: Rect, p: &mut XyPadParams, id: NodeId, zoom: f32) {
    let side = rect.width().min(rect.height());
    let pad = Rect::from_center_size(rect.center(), vec2(rect.width(), side));
    well(ui, pal, pad);
    w::hatch(ui, pad.shrink(1.0), pal.faded(pal.contrast, 0.94), 14.0);
    let resp = ui.interact(pad, ui.id().with(("xy", id.0)), Sense::drag());
    if resp.dragged()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let [x, y] = to_unit(pad, pos);
        p.x = x;
        p.y = y;
        p.touching = true;
    }
    if resp.drag_stopped() {
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
    ui.painter()
        .rect_filled(Rect::from_center_size(c, vec2(9.0, 9.0)), w::SQUARE, col);
    if zoom >= 0.6 {
        ui.painter().text(
            pad.right_bottom() - vec2(4.0, 2.0),
            Align2::RIGHT_BOTTOM,
            "freq →",
            font::tiny(),
            pal.dim,
        );
        ui.painter().text(
            pad.left_top() + vec2(4.0, 2.0),
            Align2::LEFT_TOP,
            "↑ int",
            font::tiny(),
            pal.dim,
        );
    }
}

pub fn segment_painter(ui: &mut Ui, pal: &Palette, rect: Rect, p: &mut SegmentPainterParams, id: NodeId, zoom: f32) {
    let base = ui.id().with(("segments", id.0));
    let half = rect.width() / 2.0;
    for ch in 0..2 {
        let col = Rect::from_min_size(
            pos2(rect.left() + ch as f32 * half, rect.top()),
            vec2(half - 4.0, rect.height()),
        );
        let rows = [
            Rect::from_min_size(col.min, vec2(col.width(), col.height() * 0.5 - 2.0)),
            Rect::from_min_size(
                pos2(col.left(), col.top() + col.height() * 0.5 + 2.0),
                vec2(col.width(), col.height() * 0.5 - 2.0),
            ),
        ];
        for (k, area) in rows.iter().enumerate() {
            well(ui, pal, *area);
            let values = if k == 0 { &mut p.freq[ch] } else { &mut p.int[ch] };
            let color = if k == 0 { pal.accent } else { pal.success };
            let bw = area.width() / 4.0;
            for (s, v) in values.iter().enumerate() {
                let bar = Rect::from_min_max(
                    pos2(area.left() + s as f32 * bw + 2.0, area.bottom() - v * area.height()),
                    pos2(area.left() + (s + 1) as f32 * bw - 2.0, area.bottom()),
                );
                ui.painter().rect_filled(bar, w::SQUARE, color);
            }
            let resp = ui.interact(*area, base.with((ch, k)), Sense::click_and_drag());
            if (resp.dragged() || resp.clicked())
                && let Some(pos) = resp.interact_pointer_pos()
            {
                let [x, y] = to_unit(*area, pos);
                let s = ((x * 4.0) as usize).min(3);
                values[s] = (y * 100.0).round() / 100.0;
            }
            if zoom >= 0.6 {
                ui.painter().text(
                    area.left_top() + vec2(3.0, 1.0),
                    Align2::LEFT_TOP,
                    format!("{} {}", ["A", "B"][ch], if k == 0 { "freq" } else { "int" }),
                    font::tiny(),
                    pal.dim,
                );
            }
        }
    }
}

pub fn fader(ui: &mut Ui, pal: &Palette, rect: Rect, p: &mut FaderParams, probe: Option<&NodeProbe>, id: NodeId) {
    let base = ui.id().with(("fader", id.0));
    let track = Rect::from_min_size(rect.min, vec2(28.0, rect.height()));
    well(ui, pal, track);
    let fill = Rect::from_min_max(pos2(track.left(), track.bottom() - p.level * track.height()), track.max);
    ui.painter().rect_filled(fill.shrink(2.0), w::SQUARE, pal.accent);
    let resp = ui.interact(track, base.with("level"), Sense::click_and_drag());
    if (resp.dragged() || resp.clicked())
        && let Some(pos) = resp.interact_pointer_pos()
    {
        p.level = (to_unit(track, pos)[1] * 100.0).round() / 100.0;
    }

    let env_rect = Rect::from_min_max(pos2(track.right() + 6.0, rect.top()), pos2(rect.right(), rect.bottom() - 26.0));
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

    let button = Rect::from_min_max(pos2(env_rect.left(), rect.bottom() - 22.0), rect.right_bottom());
    let b = ui.interact(button, base.with("press"), Sense::drag());
    let down = b.is_pointer_button_down_on();
    if down != p.pressed {
        p.pressed = down;
    }
    ui.painter()
        .rect_filled(button, w::SQUARE, if p.pressed { pal.accent } else { pal.sheet });
    ui.painter().rect_stroke(
        button,
        w::SQUARE,
        Stroke::new(1.0, if p.pressed { pal.accent } else { pal.hair }),
        StrokeKind::Inside,
    );
    ui.painter().text(
        button.center(),
        Align2::CENTER_CENTER,
        if p.pressed { "HELD" } else { "HOLD TO TRIGGER" },
        font::tiny(),
        if p.pressed { pal.on_solid() } else { pal.dim },
    );
}
