use std::collections::{HashMap, HashSet, VecDeque};

use egui::emath::TSTransform;
use egui::{
    Align2, Color32, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};

use shocking_vrc_core::OldZoneType;
use shocking_vrc_core::cli::GraphChannelOutput;
use shocking_vrc_core::graph::device::MAX_CHANNELS;
use shocking_vrc_core::graph::kind::*;
use shocking_vrc_core::graph::{
    DeviceBinding, GraphIssue, Node, NodeId, NodeKind, NodeProbe, NodeTint, PinType, ScopeTrace,
    Severity,
};

use super::{PinEnd, Typing};
use crate::theme::Palette;
use crate::theme::widgets::{self as w, font};

pub const NODE_W: f32 = 200.0;
pub const HEADER_H: f32 = 24.0;
pub const ROW_H: f32 = 20.0;
pub const PIN: f32 = 8.0;
const PAD: f32 = 8.0;
const REROUTE: f32 = 16.0;
pub const TEXT_MIN_ZOOM: f32 = 0.4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Z(pub f32);

impl Z {
    pub fn px(self, v: f32) -> f32 {
        v * self.0
    }

    pub fn v(self, x: f32, y: f32) -> Vec2 {
        vec2(x * self.0, y * self.0)
    }

    pub fn font(self, f: FontId) -> FontId {
        FontId::new(((f.size * self.0) * 2.0).round() * 0.5, f.family)
    }

    pub fn text(self) -> bool {
        self.0 >= TEXT_MIN_ZOOM
    }

    fn margin(self, x: f32, y: f32) -> egui::Margin {
        egui::Margin::symmetric(
            (x * self.0).round().clamp(0.0, 100.0) as i8,
            (y * self.0).round().clamp(0.0, 100.0) as i8,
        )
    }
}

#[derive(Debug, Clone)]
pub struct PinSlot {
    pub key: String,
    pub label: String,
    pub ty: PinType,
    pub pos: Pos2,
    pub row: Rect,
    pub default: f32,
    pub range: (f32, f32),
}

#[derive(Debug, Clone)]
pub struct NodeLayout {
    pub rect: Rect,
    pub header: Rect,
    pub inputs: Vec<PinSlot>,
    pub outputs: Vec<PinSlot>,
    pub captions: Vec<(usize, Rect)>,
    pub body: Rect,
    pub preview: Option<Rect>,
}

impl NodeLayout {
    pub fn to_screen(&self, t: &TSTransform) -> NodeLayout {
        let r = |x: Rect| if x.is_finite() { *t * x } else { x };
        let pin = |s: &PinSlot| PinSlot {
            pos: *t * s.pos,
            row: *t * s.row,
            ..s.clone()
        };
        NodeLayout {
            rect: r(self.rect),
            header: r(self.header),
            inputs: self.inputs.iter().map(pin).collect(),
            outputs: self.outputs.iter().map(pin).collect(),
            captions: self.captions.iter().map(|(c, x)| (*c, r(*x))).collect(),
            body: r(self.body),
            preview: self.preview.map(r),
        }
    }

    pub fn input(&self, key: &str) -> Option<&PinSlot> {
        self.inputs.iter().find(|p| p.key == key)
    }

    pub fn output(&self, key: &str) -> Option<&PinSlot> {
        self.outputs.iter().find(|p| p.key == key)
    }
}

pub fn width(kind: &NodeKind) -> f32 {
    match kind {
        NodeKind::Device(_) => 236.0,
        NodeKind::Curve(_) | NodeKind::Scope(_) | NodeKind::Ukf(_) => 220.0,
        NodeKind::Frame(p) => p.size[0],
        NodeKind::Reroute(_) => REROUTE,
        _ => NODE_W,
    }
}

fn body_rows(kind: &NodeKind) -> f32 {
    match kind {
        NodeKind::Contact(_) | NodeKind::Ukf(_) | NodeKind::Combine(_) | NodeKind::Delay(_) => 2.0,
        NodeKind::Compare(_) | NodeKind::Function(_) | NodeKind::Lfo(_) | NodeKind::Smooth(_) => 2.0,
        NodeKind::OscFloat(_)
        | NodeKind::Constant(_)
        | NodeKind::Slider(_)
        | NodeKind::Toggle(_)
        | NodeKind::Integrate(_)
        | NodeKind::MovingAverage(_)
        | NodeKind::Normalize(_)
        | NodeKind::SpoofGuard(_)
        | NodeKind::Math(_)
        | NodeKind::Remap(_)
        | NodeKind::Noise(_)
        | NodeKind::Envelope(_)
        | NodeKind::Logic(_)
        | NodeKind::Switch(_)
        | NodeKind::Router(_)
        | NodeKind::Crossfade(_)
        | NodeKind::Group(_)
        | NodeKind::GroupInput(_)
        | NodeKind::GroupOutput(_)
        | NodeKind::Scope(_)
        | NodeKind::Metronome(_)
        | NodeKind::Missing(_) => 1.0,
        NodeKind::Panic(_) => 1.6,
        NodeKind::Sequencer(_) => 2.5,
        NodeKind::Device(_) => 1.0,
        _ => 0.0,
    }
}

fn preview_h(kind: &NodeKind) -> f32 {
    match kind {
        NodeKind::Curve(_) => 110.0,
        NodeKind::Scope(_) => 96.0,
        NodeKind::Lfo(_)
        | NodeKind::Smooth(_)
        | NodeKind::Envelope(_)
        | NodeKind::Noise(_)
        | NodeKind::Metronome(_) => 44.0,
        _ => 0.0,
    }
}

pub fn traced_outputs(kind: &NodeKind) -> &'static [usize] {
    match kind {
        NodeKind::Lfo(_)
        | NodeKind::Smooth(_)
        | NodeKind::Envelope(_)
        | NodeKind::Noise(_)
        | NodeKind::Metronome(_) => &[0],
        _ => &[],
    }
}

fn slot(spec: &PinSpec, pos: Pos2, row: Rect) -> PinSlot {
    PinSlot {
        key: spec.key.to_string(),
        label: spec.label.to_string(),
        ty: spec.ty,
        pos,
        row,
        default: spec.default,
        range: spec.range,
    }
}

pub fn layout(node: &Node) -> NodeLayout {
    let origin = pos2(node.pos[0], node.pos[1]);
    let w = width(&node.kind);
    let ins = node.inputs();
    let outs = node.outputs();

    if let NodeKind::Reroute(_) = node.kind {
        let rect = Rect::from_min_size(origin, vec2(REROUTE, REROUTE));
        let c = rect.center();
        return NodeLayout {
            rect,
            header: rect,
            inputs: ins.iter().map(|s| slot(s, c, rect)).collect(),
            outputs: outs.iter().map(|s| slot(s, c, rect)).collect(),
            captions: Vec::new(),
            body: Rect::NOTHING,
            preview: None,
        };
    }
    if let NodeKind::Frame(p) = &node.kind {
        let rect = Rect::from_min_size(origin, vec2(p.size[0], p.size[1]));
        return NodeLayout {
            rect,
            header: Rect::from_min_size(origin, vec2(p.size[0], HEADER_H)),
            inputs: Vec::new(),
            outputs: Vec::new(),
            captions: Vec::new(),
            body: Rect::from_min_max(origin + vec2(0.0, HEADER_H), rect.max),
            preview: None,
        };
    }

    let header = Rect::from_min_size(origin, vec2(w, HEADER_H));
    if node.collapsed {
        let left = pos2(header.left(), header.center().y);
        let right = pos2(header.right(), header.center().y);
        return NodeLayout {
            rect: header,
            header,
            inputs: ins.iter().map(|s| slot(s, left, header)).collect(),
            outputs: outs.iter().map(|s| slot(s, right, header)).collect(),
            captions: Vec::new(),
            body: Rect::NOTHING,
            preview: None,
        };
    }

    let mut y = header.bottom() + 4.0;
    let row = |y: f32| Rect::from_min_size(pos2(origin.x, y), vec2(w, ROW_H));
    let mut outputs = Vec::new();
    for s in &outs {
        let r = row(y);
        outputs.push(slot(s, pos2(r.right(), r.center().y), r));
        y += ROW_H;
    }
    let mut inputs = Vec::new();
    let mut captions = Vec::new();
    if let NodeKind::Device(d) = &node.kind {
        let profile = d.profile();
        let mut k = 0usize;
        for (ci, ch) in profile.channels.iter().enumerate() {
            captions.push((ci, row(y)));
            y += ROW_H;
            let prefix = format!("{}.", ch.key);
            while k < ins.len() && ins[k].key.starts_with(&prefix) {
                let r = row(y);
                inputs.push(slot(&ins[k], pos2(r.left(), r.center().y), r));
                y += ROW_H;
                k += 1;
            }
        }
    } else {
        for s in &ins {
            let r = row(y);
            inputs.push(slot(s, pos2(r.left(), r.center().y), r));
            y += ROW_H;
        }
    }
    let body_h = body_rows(&node.kind) * ROW_H;
    let body = Rect::from_min_size(pos2(origin.x, y), vec2(w, body_h));
    y += body_h;
    let ph = preview_h(&node.kind);
    let preview = (ph > 0.0).then(|| {
        let r = Rect::from_min_size(pos2(origin.x + PAD, y + 2.0), vec2(w - PAD * 2.0, ph));
        y += ph + 4.0;
        r
    });
    let rect = Rect::from_min_max(origin, pos2(origin.x + w, y + 6.0));
    NodeLayout {
        rect,
        header,
        inputs,
        outputs,
        captions,
        body,
        preview,
    }
}

pub fn pin_color(pal: &Palette, ty: PinType) -> Color32 {
    match ty {
        PinType::Float => pal.accent,
        PinType::Bool => pal.warning,
    }
}

pub fn tint_color(pal: &Palette, t: NodeTint) -> Color32 {
    match t {
        NodeTint::Accent => pal.accent,
        NodeTint::Success => pal.success,
        NodeTint::Warning => pal.warning,
        NodeTint::Error => pal.error,
        NodeTint::Muted => pal.dim,
    }
}

pub fn fmt_value(v: f32) -> String {
    let a = v.abs();
    if a >= 1000.0 {
        format!("{v:.0}")
    } else if a >= 100.0 {
        format!("{v:.1}")
    } else if a >= 10.0 {
        format!("{v:.2}")
    } else {
        format!("{v:.3}")
    }
}

fn short(v: f32) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

pub struct DeviceView {
    pub online: bool,
    pub battery: Option<u8>,
    pub outputs: Option<[GraphChannelOutput; MAX_CHANNELS]>,
    pub caps: [u8; MAX_CHANNELS],
}

pub struct NodeView<'a> {
    pub pal: &'a Palette,
    pub probe: Option<&'a NodeProbe>,
    pub connected: &'a HashSet<String>,
    pub selected: bool,
    pub hovered_wire_target: bool,
    pub device: Option<DeviceView>,
    pub traces: &'a HashMap<(u64, usize), VecDeque<f32>>,
    pub flat: u64,
    pub scope: Option<&'a ScopeTrace>,
    pub issue: Option<&'a GraphIssue>,
    pub zoom: f32,
}

#[derive(Default)]
pub struct NodeResponse {
    pub header: Option<Response>,
    pub pin_drag_started: Option<PinEnd>,
    pub enter_group: bool,
    pub pick_device: bool,
    pub add_port: bool,
    pub resize: Option<egui::Vec2>,
    pub hovered: bool,
}

fn small(ui: &Ui, z: Z, rect: Rect, text: &str, color: Color32, align: Align2) {
    if z.text() {
        w::text_in(ui, rect, text, z.font(font::tiny()), color, align);
    }
}

fn node_text_edit(ui: &mut Ui, z: Z, rect: Rect, buf: &mut String, hint: &str) {
    if !z.text() {
        return;
    }
    ui.put(
        rect,
        egui::TextEdit::singleline(buf)
            .font(z.font(font::tiny()))
            .hint_text(hint)
            .margin(z.margin(4.0, 1.0))
            .desired_width(rect.width()),
    );
}

#[allow(clippy::too_many_arguments)]
fn node_slider(ui: &mut Ui, pal: &Palette, z: Z, rect: Rect, id: egui::Id, value: f32, min: f32, max: f32) -> Option<f32> {
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    let track = Rect::from_center_size(rect.center(), vec2(rect.width(), z.px(10.0).max(3.0)));
    let p = ui.painter();
    p.rect_filled(track, w::SQUARE, pal.well);
    w::hatch(ui, track.shrink(1.0), pal.faded(pal.contrast, 0.7), 6.0);
    let span = (max - min).max(1e-6);
    let frac = ((value - min) / span).clamp(0.0, 1.0);
    let filled = Rect::from_min_max(track.left_top(), pos2(track.left() + frac * track.width(), track.bottom()));
    if filled.width() > 0.5 {
        ui.painter().rect_filled(filled, w::SQUARE, pal.faded(pal.accent, 0.35));
    }
    ui.painter().rect_stroke(
        track,
        w::SQUARE,
        Stroke::new(1.0, if resp.hovered() { pal.accent } else { pal.hair }),
        StrokeKind::Inside,
    );
    let tx = track.left() + frac * track.width();
    let thumb = Rect::from_center_size(
        pos2(tx, track.center().y),
        vec2(z.px(11.0).max(4.0), z.px(16.0).max(6.0)),
    );
    ui.painter().rect_filled(thumb, w::SQUARE, pal.contrast);
    ui.painter()
        .rect_stroke(thumb, w::SQUARE, Stroke::new(1.0, pal.base), StrokeKind::Inside);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    if (resp.dragged() || resp.clicked())
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let f = ((pos.x - track.left()) / track.width().max(1.0)).clamp(0.0, 1.0);
        return Some(min + f * span);
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn value_cell(
    ui: &mut Ui,
    pal: &Palette,
    z: Z,
    rect: Rect,
    id: egui::Id,
    value: &mut f32,
    range: (f32, f32),
    typing: &mut Option<Typing>,
    node: NodeId,
    key: &str,
) -> bool {
    let typing_here = typing.as_ref().is_some_and(|t| t.node == node && t.key == key);
    if typing_here {
        let mut done = false;
        let mut changed = false;
        if let Some(t) = typing.as_mut() {
            let resp = ui.put(
                rect,
                egui::TextEdit::singleline(&mut t.buf)
                    .font(z.font(font::tiny()))
                    .margin(z.margin(3.0, 1.0))
                    .desired_width(rect.width()),
            );
            resp.request_focus();
            if resp.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                if let Ok(v) = t.buf.trim().parse::<f32>()
                    && v.is_finite()
                {
                    *value = v;
                    changed = true;
                }
                done = true;
            }
        }
        if done {
            *typing = None;
        }
        return changed;
    }
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    let hovered = resp.hovered() || resp.dragged();
    ui.painter().rect_filled(rect, w::SQUARE, pal.well);
    ui.painter().rect_stroke(
        rect,
        w::SQUARE,
        Stroke::new(1.0, if hovered { pal.accent } else { pal.hair }),
        StrokeKind::Inside,
    );
    small(ui, z, rect.shrink2(z.v(4.0, 0.0)), &fmt_value(*value), pal.contrast, Align2::RIGHT_CENTER);
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    if resp.double_clicked() {
        *typing = Some(Typing {
            node,
            key: key.to_string(),
            buf: fmt_value(*value),
        });
        return false;
    }
    if resp.dragged() {
        let span = (range.1 - range.0).abs().max(0.01);
        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        let dv = resp.drag_delta().x * span / 140.0 * fine;
        if dv != 0.0 {
            *value += dv;
            return true;
        }
    }
    false
}

fn cycle(ui: &mut Ui, pal: &Palette, z: Z, rect: Rect, id: egui::Id, label: &str) -> i32 {
    let resp = ui.interact(rect, id, Sense::click());
    let hovered = resp.hovered();
    ui.painter().rect_stroke(
        rect,
        w::SQUARE,
        Stroke::new(1.0, if hovered { pal.accent } else { pal.hair }),
        StrokeKind::Inside,
    );
    small(ui, z, rect, &format!("‹ {label} ›"), if hovered { pal.accent } else { pal.contrast }, Align2::CENTER_CENTER);
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() {
        1
    } else if resp.secondary_clicked() {
        -1
    } else {
        0
    }
}

fn tag_button(ui: &mut Ui, pal: &Palette, z: Z, rect: Rect, id: egui::Id, label: &str, on: bool) -> bool {
    let resp = ui.interact(rect, id, Sense::click());
    let (fill, fg) = if on {
        (pal.accent, pal.on_solid())
    } else if resp.hovered() {
        (pal.well, pal.contrast)
    } else {
        (Color32::TRANSPARENT, pal.dim)
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
    small(ui, z, rect, label, fg, Align2::CENTER_CENTER);
    resp.clicked()
}

fn step<T: Copy + PartialEq>(all: &[T], cur: T, by: i32) -> T {
    let i = all.iter().position(|v| *v == cur).unwrap_or(0) as i32;
    let n = all.len() as i32;
    all[((i + by).rem_euclid(n)) as usize]
}

fn cells(z: Z, row: Rect, n: usize) -> Vec<Rect> {
    let inner = row.shrink2(z.v(PAD, 2.0));
    let gap = z.px(4.0);
    let cw = (inner.width() - gap * (n as f32 - 1.0)) / n as f32;
    (0..n)
        .map(|i| {
            Rect::from_min_size(
                pos2(inner.left() + i as f32 * (cw + gap), inner.top()),
                vec2(cw, inner.height()),
            )
        })
        .collect()
}

fn plot(ui: &Ui, pal: &Palette, rect: Rect, values: &VecDeque<f32>, color: Color32) {
    ui.painter().rect_filled(rect, w::SQUARE, pal.well);
    ui.painter()
        .rect_stroke(rect, w::SQUARE, Stroke::new(1.0, pal.hair), StrokeKind::Inside);
    if values.len() < 2 {
        return;
    }
    let (lo, hi) = values
        .iter()
        .fold((0.0f32, 1.0f32), |(a, b), v| (a.min(*v), b.max(*v)));
    let span = (hi - lo).max(1e-3);
    let n = super::TRACE_LEN.max(values.len());
    let pts: Vec<Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let x = rect.right() - (values.len() - 1 - i) as f32 / (n - 1) as f32 * rect.width();
            let y = rect.bottom() - (v - lo) / span * (rect.height() - 4.0) - 2.0;
            pos2(x, y)
        })
        .collect();
    ui.painter()
        .with_clip_rect(rect)
        .add(egui::Shape::line(pts, Stroke::new(1.2, color)));
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    ui: &mut Ui,
    node: &mut Node,
    lay: &NodeLayout,
    v: &NodeView,
    typing: &mut Option<Typing>,
) -> NodeResponse {
    let pal = v.pal;
    let z = Z(v.zoom);
    let mut out = NodeResponse::default();
    let id = node.id;
    let base_id = ui.id().with(("graph_node", id.0));

    if let NodeKind::Reroute(_) = node.kind {
        let resp = ui.interact(lay.rect, base_id.with("hdr"), Sense::click_and_drag());
        let c = lay.rect.center();
        let col = if v.selected { pal.accent } else { pal.contrast };
        ui.painter()
            .rect_filled(Rect::from_center_size(c, z.v(9.0, 9.0)), w::SQUARE, col);
        pins(ui, node, lay, v, base_id, &mut out);
        out.hovered = resp.hovered();
        out.header = Some(resp);
        return out;
    }

    if let NodeKind::Frame(p) = &mut node.kind {
        ui.painter().rect_filled(lay.rect, w::SQUARE, pal.faded(pal.sheet, 0.4));
        let tint = node.tint.map(|t| tint_color(pal, t)).unwrap_or(pal.hair);
        w::dashed_rect(ui, lay.rect, if v.selected { pal.accent } else { tint });
        let resp = ui.interact(lay.header, base_id.with("hdr"), Sense::click_and_drag());
        let title = if node.title.is_empty() { "Frame".to_string() } else { node.title.clone() };
        if z.text() {
            w::text_in(ui, lay.header.shrink2(z.v(PAD, 0.0)), &title.to_uppercase(), z.font(font::small()), tint, Align2::LEFT_CENTER);
        }
        if !p.text.is_empty() && z.text() {
            let galley = ui.painter().layout(
                p.text.clone(),
                z.font(font::small()),
                pal.dim,
                (lay.body.width() - z.px(PAD * 2.0)).max(10.0),
            );
            ui.painter()
                .with_clip_rect(lay.body.intersect(ui.clip_rect()))
                .galley(lay.body.min + z.v(PAD, 2.0), galley, pal.dim);
        }
        let grip = Rect::from_min_size(lay.rect.max - vec2(12.0, 12.0), vec2(12.0, 12.0));
        let g = ui.interact(grip, base_id.with("grip"), Sense::drag());
        ui.painter().line_segment([grip.left_bottom(), grip.right_top()], Stroke::new(1.0, tint));
        if g.dragged() {
            out.resize = Some(g.drag_delta() / z.0);
        }
        if g.hovered() || g.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
        }
        out.hovered = resp.hovered();
        out.header = Some(resp);
        return out;
    }

    let offline = v.device.as_ref().is_some_and(|d| !d.online);
    let fade = |c: Color32| if offline { pal.faded(c, 0.55) } else { c };

    ui.painter().rect_filled(lay.rect, w::SQUARE, pal.sheet);
    let border = match (v.selected, v.issue.map(|i| i.severity)) {
        (true, _) => pal.accent,
        (_, Some(Severity::Error)) => pal.error,
        (_, Some(Severity::Warning)) => pal.warning,
        _ => pal.hair,
    };
    if v.selected {
        ui.painter()
            .rect_stroke(lay.rect, w::SQUARE, Stroke::new(1.0, border), StrokeKind::Inside);
        w::focus_corners(ui, lay.rect, pal.accent);
    } else {
        w::dashed_rect(ui, lay.rect, border);
    }
    if v.hovered_wire_target {
        ui.painter().rect_stroke(
            lay.rect.expand(z.px(3.0).max(2.0)),
            w::SQUARE,
            Stroke::new(1.0, pal.accent),
            StrokeKind::Outside,
        );
    }

    let header_fill = match (&node.kind, node.tint) {
        (_, Some(t)) => tint_color(pal, t),
        (NodeKind::Device(_), None) => {
            if offline {
                pal.dim
            } else {
                pal.success
            }
        }
        (NodeKind::Missing(_), None) => pal.error,
        (NodeKind::Panic(_), None) => pal.error,
        _ => pal.contrast,
    };
    let chip = lay.header.shrink(z.px(3.0));
    ui.painter().rect_filled(chip, w::SQUARE, header_fill);
    let resp = ui.interact(lay.header, base_id.with("hdr"), Sense::click_and_drag());
    let title_rect = chip.shrink2(z.v(6.0, 0.0));
    if z.text() {
        w::text_in(ui, title_rect, &node.display_title().to_uppercase(), z.font(font::small()), pal.on_solid(), Align2::LEFT_CENTER);
    }
    if let Some(d) = &v.device {
        let state = match (d.online, d.battery) {
            (true, Some(b)) => format!("{b}%"),
            (true, None) => "online".to_string(),
            (false, _) => "offline".to_string(),
        };
        small(ui, z, title_rect, &state, pal.on_solid(), Align2::RIGHT_CENTER);
    } else if node.collapsed {
        small(ui, z, title_rect, "▸", pal.on_solid(), Align2::RIGHT_CENTER);
    }
    if !node.comment.is_empty() && !node.collapsed {
        let resp2 = resp.clone();
        let _ = resp2.on_hover_text(&node.comment);
    }
    out.hovered = resp.hovered();

    if !node.collapsed {
        outputs_rows(ui, node, lay, v);
        inputs_rows(ui, node, lay, v, base_id, typing, &fade);
        body(ui, node, lay, v, base_id, typing, &mut out);
        preview(ui, node, lay, v);
    }
    pins(ui, node, lay, v, base_id, &mut out);
    out.header = Some(resp);
    out
}

fn outputs_rows(ui: &mut Ui, _node: &Node, lay: &NodeLayout, v: &NodeView) {
    let pal = v.pal;
    let z = Z(v.zoom);
    for (i, p) in lay.outputs.iter().enumerate() {
        let r = p.row.shrink2(z.v(PAD + 4.0, 0.0));
        small(ui, z, r, &p.label, pal.dim, Align2::RIGHT_CENTER);
        if let Some(val) = v.probe.and_then(|pr| pr.outputs.get(i)) {
            let txt = match p.ty {
                PinType::Bool => if *val > 0.5 { "on" } else { "off" }.to_string(),
                PinType::Float => fmt_value(*val),
            };
            small(ui, z, r, &txt, pin_color(pal, p.ty), Align2::LEFT_CENTER);
        }
    }
}

fn inputs_rows(
    ui: &mut Ui,
    node: &mut Node,
    lay: &NodeLayout,
    v: &NodeView,
    base_id: egui::Id,
    typing: &mut Option<Typing>,
    fade: &dyn Fn(Color32) -> Color32,
) {
    let pal = v.pal;
    let z = Z(v.zoom);
    if let NodeKind::Device(d) = &mut node.kind {
        let profile = d.profile();
        for (ci, r) in &lay.captions {
            let Some(ch) = profile.channels.get(*ci) else { continue };
            let r = r.shrink2(z.v(PAD, 2.0));
            small(ui, z, r, &ch.label.to_uppercase(), fade(pal.contrast), Align2::LEFT_CENTER);
            if let Some(o) = v.device.as_ref().and_then(|dv| dv.outputs.map(|o| o[*ci])) {
                let cap = v.device.as_ref().map(|dv| dv.caps[*ci]).unwrap_or(0);
                let mid = Rect::from_min_max(
                    pos2(r.left() + z.px(76.0), r.top()),
                    pos2(r.right() - z.px(58.0), r.bottom()),
                );
                small(
                    ui,
                    z,
                    mid,
                    &format!("{} / {}", o.safe.strength, o.safe.limit_max.min(cap)),
                    if o.safe.strength > 0 { pal.accent } else { pal.dim },
                    Align2::LEFT_CENTER,
                );
            }
            let params = d.channels.get_mut(*ci);
            if let Some(params) = params {
                for (k, (group, label)) in [("freq", "F×4"), ("int", "I×4")].iter().enumerate() {
                    let b = Rect::from_min_size(
                        pos2(r.right() - z.px(56.0 - k as f32 * 29.0), r.top()),
                        vec2(z.px(27.0), r.height()),
                    );
                    let on = params.is_expanded(group);
                    if tag_button(ui, pal, z, b, base_id.with(("exp", *ci, k)), label, on) {
                        params.set_expanded(group, !on);
                    }
                }
            }
        }
    }
    for p in &lay.inputs {
        let r = p.row.shrink2(z.v(PAD + 4.0, 0.0));
        let connected = v.connected.contains(&p.key);
        let split = r.left() + r.width() * 0.5;
        let label_r = Rect::from_min_max(r.min, pos2(split, r.bottom()));
        small(ui, z, label_r, &p.label, fade(if connected { pal.contrast } else { pal.dim }), Align2::LEFT_CENTER);
        let cell = Rect::from_min_max(
            pos2(split + z.px(4.0), r.top() + z.px(2.0)),
            pos2(r.right(), r.bottom() - z.px(2.0)),
        );
        let idx = lay.inputs.iter().position(|q| q.key == p.key).unwrap_or(0);
        if connected {
            if let Some(val) = v.probe.and_then(|pr| pr.inputs.get(idx)) {
                small(ui, z, cell, &fmt_value(*val), pin_color(pal, p.ty), Align2::RIGHT_CENTER);
            }
        } else if p.ty == PinType::Bool {
            let on = node.defaults.get(&p.key).copied().unwrap_or(p.default) > 0.5;
            let b = Rect::from_min_max(pos2(cell.right() - z.px(36.0), cell.top()), cell.max);
            if tag_button(ui, pal, z, b, base_id.with(("def", &p.key)), if on { "on" } else { "off" }, on) {
                node.defaults.insert(p.key.clone(), if on { 0.0 } else { 1.0 });
            }
        } else if p.key.ends_with(".bf") && node.defaults.get(&p.key).copied().unwrap_or(p.default) < 0.0 {
            let b = Rect::from_min_max(pos2(cell.right() - z.px(44.0), cell.top()), cell.max);
            if tag_button(ui, pal, z, b, base_id.with(("def", &p.key)), "auto", false) {
                node.defaults.insert(p.key.clone(), 30.0);
            }
        } else {
            let mut val = node.defaults.get(&p.key).copied().unwrap_or(p.default);
            if value_cell(ui, pal, z, cell, base_id.with(("def", &p.key)), &mut val, p.range, typing, node.id, &p.key) {
                node.defaults.insert(p.key.clone(), val);
            }
        }
    }
}

fn body(
    ui: &mut Ui,
    node: &mut Node,
    lay: &NodeLayout,
    v: &NodeView,
    base_id: egui::Id,
    typing: &mut Option<Typing>,
    out: &mut NodeResponse,
) {
    let pal = v.pal;
    let z = Z(v.zoom);
    let row_h = z.px(ROW_H);
    let rows: Vec<Rect> = (0..3)
        .map(|i| Rect::from_min_size(lay.body.min + vec2(0.0, i as f32 * row_h), vec2(lay.body.width(), row_h)))
        .collect();
    let id = node.id;
    let bid = |s: &str| base_id.with(("body", s));
    match &mut node.kind {
        NodeKind::Contact(p) => {
            let types = [OldZoneType::Any, OldZoneType::Pen, OldZoneType::Orf, OldZoneType::Touch, OldZoneType::DGB];
            let c = cells(z, rows[0], 1)[0];
            let by = cycle(ui, pal, z, c, bid("zt"), &p.zone.zone_type.to_string());
            if by != 0 {
                p.zone.zone_type = step(&types, p.zone.zone_type, by);
            }
            let c = cells(z, rows[1], 1)[0];
            node_text_edit(ui, z, c, &mut p.zone.name, "* = any");
        }
        NodeKind::OscFloat(p) => {
            let c = cells(z, rows[0], 1)[0];
            node_text_edit(ui, z, c, &mut p.address, "/avatar/parameters/…");
            let fresh = v.probe.and_then(|pr| pr.outputs.get(1)).is_some_and(|f| *f > 0.5);
            if !fresh {
                small(ui, z, lay.header.shrink2(z.v(10.0, 0.0)), "stale", pal.on_solid(), Align2::RIGHT_CENTER);
            }
        }
        NodeKind::Constant(p) => {
            let c = cells(z, rows[0], 1)[0];
            value_cell(ui, pal, z, c, bid("v"), &mut p.value, (0.0, 1.0), typing, id, "value");
        }
        NodeKind::Slider(p) => {
            let c = cells(z, rows[0], 1)[0];
            if let Some(nv) = node_slider(ui, pal, z, c, bid("slider"), p.value, p.min, p.max) {
                let st = p.step.max(1e-4);
                p.value = ((nv / st).round() * st).clamp(p.min.min(p.max), p.max.max(p.min));
            }
        }
        NodeKind::Toggle(p) => {
            let c = cells(z, rows[0], 1)[0];
            if p.momentary {
                let resp = ui.interact(c, bid("hold"), Sense::click_and_drag());
                let down = resp.is_pointer_button_down_on();
                if down != p.on {
                    p.on = down;
                }
                tag_button(ui, pal, z, c, bid("holdv"), if p.on { "HELD" } else { "HOLD" }, p.on);
            } else if tag_button(ui, pal, z, c, bid("tog"), if p.on { "ON" } else { "OFF" }, p.on) {
                p.on = !p.on;
            }
        }
        NodeKind::Ukf(p) => {
            small(ui, z, rows[0].shrink2(z.v(PAD, 0.0)), &format!("q {}  r {}", fmt_value(p.ukf.q), fmt_value(p.ukf.r)), pal.dim, Align2::LEFT_CENTER);
            small(
                ui,
                z,
                rows[1].shrink2(z.v(PAD, 0.0)),
                &format!("norm ÷ {} / {} / {}", short(p.norms.speed), short(p.norms.acc), short(p.norms.recoil)),
                pal.dim,
                Align2::LEFT_CENTER,
            );
        }
        NodeKind::Smooth(p) => {
            let c = cells(z, rows[0], 2);
            small(ui, z, c[0], "attack ms", pal.dim, Align2::LEFT_CENTER);
            value_cell(ui, pal, z, c[1], bid("a"), &mut p.attack_ms, (0.0, 2000.0), typing, id, "attack_ms");
            let c = cells(z, rows[1], 2);
            small(ui, z, c[0], "release ms", pal.dim, Align2::LEFT_CENTER);
            value_cell(ui, pal, z, c[1], bid("r"), &mut p.release_ms, (0.0, 2000.0), typing, id, "release_ms");
        }
        NodeKind::Integrate(p) => {
            let c = cells(z, rows[0], 2);
            small(ui, z, c[0], "leak /s", pal.dim, Align2::LEFT_CENTER);
            value_cell(ui, pal, z, c[1], bid("l"), &mut p.leak, (0.0, 5.0), typing, id, "leak");
        }
        NodeKind::MovingAverage(p) => {
            let c = cells(z, rows[0], 2);
            small(ui, z, c[0], "window ms", pal.dim, Align2::LEFT_CENTER);
            value_cell(ui, pal, z, c[1], bid("w"), &mut p.window_ms, (25.0, 5000.0), typing, id, "window_ms");
        }
        NodeKind::Normalize(p) => {
            let c = cells(z, rows[0], 2);
            small(ui, z, c[0], "÷", pal.dim, Align2::LEFT_CENTER);
            value_cell(ui, pal, z, c[1], bid("d"), &mut p.divisor, (0.01, 100.0), typing, id, "divisor");
        }
        NodeKind::SpoofGuard(p) => {
            small(
                ui,
                z,
                rows[0].shrink2(z.v(PAD, 0.0)),
                &format!("jump > {:.2}  release ≤ {:.2}", p.jump_limit, p.release_level),
                pal.dim,
                Align2::LEFT_CENTER,
            );
        }
        NodeKind::Math(p) => {
            let by = cycle(ui, pal, z, cells(z, rows[0], 1)[0], bid("op"), p.op.label());
            if by != 0 {
                p.op = step(&MathOp::ALL, p.op, by);
            }
        }
        NodeKind::Remap(p) => {
            let c = cells(z, rows[0], 1)[0];
            if tag_button(ui, pal, z, c, bid("cl"), if p.clamp { "clamped" } else { "unclamped" }, p.clamp) {
                p.clamp = !p.clamp;
            }
        }
        NodeKind::Function(p) => {
            let list = crate::model::mod_function_list();
            let label = p.function.to_string();
            let by = cycle(ui, pal, z, cells(z, rows[0], 1)[0], bid("fn"), &label);
            if by != 0 {
                let i = list.iter().position(|f| *f == p.function).unwrap_or(0) as i32;
                p.function = list[((i + by).rem_euclid(list.len() as i32)) as usize].clone();
            }
            small(
                ui,
                z,
                rows[1].shrink2(z.v(PAD, 0.0)),
                &format!("dev {}  off {}  ×{}", fmt_value(p.max_deviation), fmt_value(p.offset), fmt_value(p.frequency_multiplier)),
                pal.dim,
                Align2::LEFT_CENTER,
            );
        }
        NodeKind::Combine(p) => {
            let by = cycle(ui, pal, z, cells(z, rows[0], 1)[0], bid("m"), p.mode.label());
            if by != 0 {
                p.mode = step(&CombineMode::ALL, p.mode, by);
            }
            count_row(ui, pal, z, rows[1], bid("n"), &mut p.count, 2, MAX_FAN);
        }
        NodeKind::Switch(p) | NodeKind::Router(p) => count_row(ui, pal, z, rows[0], bid("n"), &mut p.count, 2, MAX_FAN),
        NodeKind::Scope(p) => count_row(ui, pal, z, rows[0], bid("n"), &mut p.count, 1, 4),
        NodeKind::Lfo(p) => {
            let by = cycle(ui, pal, z, cells(z, rows[0], 1)[0], bid("sh"), p.shape.label());
            if by != 0 {
                p.shape = step(&LfoShape::ALL, p.shape, by);
            }
            let c = cells(z, rows[1], 1)[0];
            if tag_button(ui, pal, z, c, bid("os"), if p.one_shot { "one-shot" } else { "looping" }, p.one_shot) {
                p.one_shot = !p.one_shot;
            }
        }
        NodeKind::Noise(p) => {
            let by = cycle(ui, pal, z, cells(z, rows[0], 1)[0], bid("k"), p.kind.label());
            if by != 0 {
                p.kind = step(&NoiseKind::ALL, p.kind, by);
            }
        }
        NodeKind::Envelope(p) => {
            let label = match p.mode {
                EnvMode::Adsr => "ADSR",
                EnvMode::Ar => "AR",
            };
            if cycle(ui, pal, z, cells(z, rows[0], 1)[0], bid("m"), label) != 0 {
                p.mode = match p.mode {
                    EnvMode::Adsr => EnvMode::Ar,
                    EnvMode::Ar => EnvMode::Adsr,
                };
            }
        }
        NodeKind::Metronome(_) => {
            let c = cells(z, rows[0], 2);
            let hz = v
                .probe
                .and_then(|pr| pr.inputs.first().copied())
                .or_else(|| node.defaults.get("hz").copied())
                .unwrap_or(2.0);
            small(ui, z, c[0], &format!("{:.0} bpm", hz.max(0.0) * 60.0), pal.dim, Align2::LEFT_CENTER);
            let out = |k: usize| v.probe.and_then(|pr| pr.outputs.get(k)).is_some_and(|x| *x > 0.5);
            let lamp = Rect::from_center_size(
                pos2(c[1].right() - c[1].height() * 0.5, c[1].center().y),
                Vec2::splat(c[1].height()),
            );
            ui.painter()
                .rect_filled(lamp, w::SQUARE, if out(0) { pal.accent } else { pal.well });
            ui.painter().rect_stroke(
                lamp,
                w::SQUARE,
                Stroke::new(1.0, if out(1) { pal.contrast } else { pal.hair }),
                StrokeKind::Inside,
            );
            small(
                ui,
                z,
                Rect::from_min_max(c[1].min, pos2(lamp.left() - z.px(4.0), c[1].bottom())),
                if out(1) { "beat" } else { "" },
                pal.contrast,
                Align2::RIGHT_CENTER,
            );
        }
        NodeKind::Delay(p) => {
            let label = match p.mode {
                DelayMode::Ticks => "ticks (25 ms)",
                DelayMode::Ms => "milliseconds",
            };
            if cycle(ui, pal, z, cells(z, rows[0], 1)[0], bid("m"), label) != 0 {
                p.mode = match p.mode {
                    DelayMode::Ticks => DelayMode::Ms,
                    DelayMode::Ms => DelayMode::Ticks,
                };
            }
            let mut amount = p.amount as f32;
            let c = cells(z, rows[1], 2);
            small(ui, z, c[0], "amount", pal.dim, Align2::LEFT_CENTER);
            if value_cell(ui, pal, z, c[1], bid("a"), &mut amount, (1.0, 400.0), typing, id, "amount") {
                p.amount = amount.round().clamp(1.0, 10_000.0) as u32;
            }
        }
        NodeKind::Sequencer(p) => {
            let area = Rect::from_min_size(
                lay.body.min + z.v(PAD, 2.0),
                vec2(lay.body.width() - z.px(PAD * 2.0), lay.body.height() - z.px(4.0)),
            );
            ui.painter().rect_filled(area, w::SQUARE, pal.well);
            let n = p.steps.len().max(1);
            let bw = area.width() / n as f32;
            let current = v.probe.and_then(|pr| pr.outputs.get(1)).map(|i| *i as usize);
            let resp = ui.interact(area, bid("steps"), Sense::click_and_drag());
            if (resp.dragged() || resp.clicked())
                && let Some(pos) = resp.interact_pointer_pos()
            {
                let k = (((pos.x - area.left()) / bw) as usize).min(n - 1);
                let val = (1.0 - (pos.y - area.top()) / area.height()).clamp(0.0, 1.0);
                if let Some(s) = p.steps.get_mut(k) {
                    *s = (val * 20.0).round() / 20.0;
                }
            }
            for (k, s) in p.steps.iter().enumerate() {
                let h = s.clamp(0.0, 1.0) * area.height();
                let bar = Rect::from_min_max(
                    pos2(area.left() + k as f32 * bw + 1.0, area.bottom() - h),
                    pos2(area.left() + (k + 1) as f32 * bw - 1.0, area.bottom()),
                );
                let col = if current == Some(k) { pal.accent } else { pal.faded(pal.accent, 0.5) };
                ui.painter().rect_filled(bar, w::SQUARE, col);
            }
            ui.painter()
                .rect_stroke(area, w::SQUARE, Stroke::new(1.0, pal.hair), StrokeKind::Inside);
        }
        NodeKind::Compare(p) => {
            let by = cycle(ui, pal, z, cells(z, rows[0], 1)[0], bid("op"), p.op.label());
            if by != 0 {
                p.op = step(&CompareOp::ALL, p.op, by);
            }
            let c = cells(z, rows[1], 2);
            small(ui, z, c[0], "hysteresis", pal.dim, Align2::LEFT_CENTER);
            value_cell(ui, pal, z, c[1], bid("h"), &mut p.hysteresis, (0.0, 0.5), typing, id, "hysteresis");
        }
        NodeKind::Logic(p) => {
            let by = cycle(ui, pal, z, cells(z, rows[0], 1)[0], bid("op"), p.op.label());
            if by != 0 {
                p.op = step(&LogicOp::ALL, p.op, by);
            }
        }
        NodeKind::Crossfade(p) => {
            let c = cells(z, rows[0], 1)[0];
            if tag_button(ui, pal, z, c, bid("ep"), if p.equal_power { "equal power" } else { "linear" }, p.equal_power) {
                p.equal_power = !p.equal_power;
            }
        }
        NodeKind::Group(p) => {
            let c = cells(z, rows[0], 2);
            small(ui, z, c[0], &format!("{} node(s)", p.graph.nodes.len()), pal.dim, Align2::LEFT_CENTER);
            if tag_button(ui, pal, z, c[1], bid("enter"), "enter ▸", false) {
                out.enter_group = true;
            }
        }
        NodeKind::GroupInput(_) | NodeKind::GroupOutput(_) => {
            let c = cells(z, rows[0], 1)[0];
            if tag_button(ui, pal, z, c, bid("port"), "+ port", false) {
                out.add_port = true;
            }
        }
        NodeKind::Panic(p) => {
            let c = Rect::from_min_size(
                lay.body.min + z.v(PAD, 3.0),
                vec2(lay.body.width() - z.px(PAD * 2.0), lay.body.height() - z.px(6.0)),
            );
            let resp = ui.interact(c, bid("panic"), Sense::click());
            let (fill, fg) = if p.latched || resp.hovered() {
                (pal.error, pal.on_solid())
            } else {
                (Color32::TRANSPARENT, pal.error)
            };
            if fill != Color32::TRANSPARENT {
                ui.painter().rect_filled(c, w::SQUARE, fill);
            }
            ui.painter()
                .rect_stroke(c, w::SQUARE, Stroke::new(1.0, pal.error), StrokeKind::Inside);
            if z.text() {
                w::text_in(ui, c, if p.latched { "RELEASE" } else { "PANIC" }, z.font(font::body()), fg, Align2::CENTER_CENTER);
            }
            if resp.clicked() {
                p.latched = !p.latched;
            }
        }
        NodeKind::Device(d) => {
            let c = cells(z, rows[0], 2);
            let bind = match &d.binding {
                DeviceBinding::Mac(m) => format!("…{}", crate::app::short_mac(m)),
                DeviceBinding::Slot(s) => format!("slot {s}"),
                DeviceBinding::Unbound => "unbound".to_string(),
            };
            small(ui, z, c[0], &bind, pal.dim, Align2::LEFT_CENTER);
            let needs_pick = v.device.as_ref().is_none_or(|dv| !dv.online);
            if needs_pick && tag_button(ui, pal, z, c[1], bid("pick"), "pick device", false) {
                out.pick_device = true;
            }
        }
        NodeKind::Missing(m) => {
            small(ui, z, rows[0].shrink2(z.v(PAD, 0.0)), &format!("'{}' kept as-is", m.type_name), pal.error, Align2::LEFT_CENTER);
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn count_row(ui: &mut Ui, pal: &Palette, z: Z, row: Rect, id: egui::Id, count: &mut u8, lo: u8, hi: u8) {
    let c = cells(z, row, 3);
    small(ui, z, c[0], "inputs", pal.dim, Align2::LEFT_CENTER);
    if tag_button(ui, pal, z, c[1], id.with("-"), "−", false) && *count > lo {
        *count -= 1;
    }
    if tag_button(ui, pal, z, c[2], id.with("+"), &format!("+  ({count})"), false) && *count < hi {
        *count += 1;
    }
}

fn preview(ui: &mut Ui, node: &mut Node, lay: &NodeLayout, v: &NodeView) {
    let Some(rect) = lay.preview else { return };
    let pal = v.pal;
    match &mut node.kind {
        NodeKind::Curve(p) => curve_editor(ui, pal, rect, &mut p.curve, v, node.id),
        NodeKind::Scope(p) => {
            ui.painter().rect_filled(rect, w::SQUARE, pal.well);
            ui.painter()
                .rect_stroke(rect, w::SQUARE, Stroke::new(1.0, pal.hair), StrokeKind::Inside);
            let Some(trace) = v.scope else { return };
            let colors = [pal.accent, pal.success, pal.warning, pal.error];
            let n = p.samples().max(2);
            let (lo, hi) = trace
                .samples
                .iter()
                .flat_map(|s| s.iter().take(trace.count))
                .fold((0.0f32, 1.0f32), |(a, b), v| (a.min(*v), b.max(*v)));
            let span = (hi - lo).max(1e-3);
            for ch in 0..trace.count.min(4) {
                let len = trace.samples.len();
                let pts: Vec<Pos2> = trace
                    .samples
                    .iter()
                    .enumerate()
                    .map(|(i, s)| {
                        let x = rect.right() - (len - 1 - i) as f32 / (n - 1) as f32 * rect.width();
                        let y = rect.bottom() - (s[ch] - lo) / span * (rect.height() - 4.0) - 2.0;
                        pos2(x, y)
                    })
                    .collect();
                if pts.len() > 1 {
                    ui.painter()
                        .with_clip_rect(rect)
                        .add(egui::Shape::line(pts, Stroke::new(1.2, colors[ch])));
                }
            }
        }
        _ => {
            if let Some(t) = v.traces.get(&(v.flat, 0)) {
                plot(ui, pal, rect, t, pal.accent);
            } else {
                plot(ui, pal, rect, &VecDeque::new(), pal.accent);
            }
        }
    }
}

fn curve_editor(
    ui: &mut Ui,
    pal: &Palette,
    rect: Rect,
    curve: &mut shocking_vrc_core::graph::Curve,
    v: &NodeView,
    id: NodeId,
) {
    let mark = (6.0 * v.zoom).max(4.0);
    ui.painter().rect_filled(rect, w::SQUARE, pal.well);
    w::hatch(ui, rect.shrink(1.0), pal.faded(pal.contrast, 0.92), 12.0);
    ui.painter()
        .rect_stroke(rect, w::SQUARE, Stroke::new(1.0, pal.hair), StrokeKind::Inside);
    let to_screen = |p: [f32; 2]| pos2(rect.left() + p[0] * rect.width(), rect.bottom() - p[1] * rect.height());
    let from_screen = |p: Pos2| {
        [
            ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0),
            ((rect.bottom() - p.y) / rect.height()).clamp(0.0, 1.0),
        ]
    };
    let pts: Vec<Pos2> = (0..=48)
        .map(|i| {
            let x = i as f32 / 48.0;
            to_screen([x, curve.eval(x)])
        })
        .collect();
    ui.painter().add(egui::Shape::line(pts, Stroke::new(1.5, pal.accent)));

    if let Some(x) = v.probe.and_then(|p| p.inputs.first()) {
        let x = x.clamp(0.0, 1.0);
        let px = rect.left() + x * rect.width();
        ui.painter()
            .vline(px, rect.y_range(), Stroke::new(1.0, pal.faded(pal.contrast, 0.4)));
        ui.painter()
            .rect_filled(Rect::from_center_size(to_screen([x, curve.eval(x)]), vec2(mark, mark)), w::SQUARE, pal.contrast);
    }

    let base = ui.id().with(("curve", id.0));
    let bg = ui.interact(rect, base.with("bg"), Sense::click());
    if bg.double_clicked()
        && let Some(p) = bg.interact_pointer_pos()
    {
        let [x, y] = from_screen(p);
        curve.add_point(x, y);
    }
    let mut remove = None;
    for i in 0..curve.points.len() {
        let c = to_screen(curve.points[i]);
        let hit = Rect::from_center_size(c, Vec2::splat((10.0 * v.zoom).max(8.0)));
        let resp = ui.interact(hit, base.with(i), Sense::click_and_drag());
        let col = if resp.hovered() || resp.dragged() { pal.accent } else { pal.contrast };
        ui.painter()
            .rect_filled(Rect::from_center_size(c, Vec2::splat((7.0 * v.zoom).max(4.0))), w::SQUARE, col);
        if resp.dragged()
            && let Some(p) = resp.interact_pointer_pos()
        {
            let [x, y] = from_screen(p);
            curve.move_point(i, x, y);
        }
        if resp.secondary_clicked() {
            remove = Some(i);
        }
    }
    if let Some(i) = remove {
        curve.remove_point(i);
    }
}

fn pins(ui: &mut Ui, node: &Node, lay: &NodeLayout, v: &NodeView, base_id: egui::Id, out: &mut NodeResponse) {
    let pal = v.pal;
    let z = Z(v.zoom);
    let pin = z.px(PIN).max(4.0);
    let collapsed = node.collapsed;
    for (list, output) in [(&lay.inputs, false), (&lay.outputs, true)] {
        for (i, p) in list.iter().enumerate() {
            if collapsed && i > 0 {
                continue;
            }
            let hit = Rect::from_center_size(p.pos, Vec2::splat(pin + 6.0));
            let resp = ui.interact(hit, base_id.with(("pin", &p.key, output)), Sense::drag());
            let col = pin_color(pal, p.ty);
            let r = Rect::from_center_size(p.pos, Vec2::splat(pin));
            let connected = output || v.connected.contains(&p.key);
            if p.ty == PinType::Bool && !connected {
                ui.painter().rect_filled(r, w::SQUARE, pal.sheet);
                ui.painter()
                    .rect_stroke(r, w::SQUARE, Stroke::new(1.5, col), StrokeKind::Inside);
            } else {
                ui.painter().rect_filled(r, w::SQUARE, if connected { col } else { pal.faded(col, 0.45) });
            }
            if resp.hovered() {
                ui.painter()
                    .rect_stroke(r.expand(2.0), w::SQUARE, Stroke::new(1.0, pal.contrast), StrokeKind::Outside);
                let value = v.probe.and_then(|pr| if output { pr.outputs.get(i) } else { pr.inputs.get(i) });
                let tip = match value {
                    Some(val) => format!("{} = {}", p.label, fmt_value(*val)),
                    None => p.label.clone(),
                };
                resp.clone().on_hover_text(tip);
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
            if resp.drag_started() {
                out.pin_drag_started = Some(PinEnd {
                    node: node.id,
                    key: p.key.clone(),
                    output,
                });
            }
        }
    }
}
