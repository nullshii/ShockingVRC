use std::collections::{HashMap, HashSet};

use egui::emath::TSTransform;
use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use shocking_vrc_core::graph::kind::{GroupPort, NodeKind};
use shocking_vrc_core::graph::{Graph, NodeId, PinRef, PinType, parse_graph};

use super::node_ui::{self, DeviceView, NodeLayout, NodeView};
use super::{AddMenu, Env, GRID, GraphEditor, Interaction, PinEnd, scope_of};
use crate::bridge::ToastKind;
use crate::theme::Palette;
use crate::theme::widgets::{self as w, font};

pub const MIN_ZOOM: f32 = 0.25;
pub const MAX_ZOOM: f32 = 2.0;
const PIN_SNAP: f32 = 14.0;

pub fn bezier(a: Pos2, b: Pos2) -> [Pos2; 4] {
    let dx = ((b.x - a.x).abs() * 0.5).max(40.0);
    [a, a + vec2(dx, 0.0), b - vec2(dx, 0.0), b]
}

fn bezier_at(p: &[Pos2; 4], t: f32) -> Pos2 {
    let u = 1.0 - t;
    let a = u * u * u;
    let b = 3.0 * u * u * t;
    let c = 3.0 * u * t * t;
    let d = t * t * t;
    pos2(
        a * p[0].x + b * p[1].x + c * p[2].x + d * p[3].x,
        a * p[0].y + b * p[1].y + c * p[2].y + d * p[3].y,
    )
}

fn sample(p: &[Pos2; 4], n: usize) -> Vec<Pos2> {
    (0..=n).map(|i| bezier_at(p, i as f32 / n as f32)).collect()
}

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t - p).length()
}

fn segments_cross(a: Pos2, b: Pos2, c: Pos2, d: Pos2) -> bool {
    let orient = |p: Pos2, q: Pos2, r: Pos2| (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
    let (o1, o2, o3, o4) = (orient(a, b, c), orient(a, b, d), orient(c, d, a), orient(c, d, b));
    o1 * o2 < 0.0 && o3 * o4 < 0.0
}

fn brightness(v: f32) -> f32 {
    (v.abs()).clamp(0.0, 1.0)
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

struct Frame<'a> {
    scope: &'a Graph,
    layouts: HashMap<NodeId, NodeLayout>,
}

fn wire_under(frame: &Frame, p: Pos2, skip: NodeId, radius: f32) -> Option<usize> {
    for (i, e) in frame.scope.edges.iter().enumerate() {
        if e.from.node == skip || e.to.node == skip {
            continue;
        }
        let (Some(a), Some(b)) = (
            frame.layouts.get(&e.from.node).and_then(|l| l.output(&e.from.pin)),
            frame.layouts.get(&e.to.node).and_then(|l| l.input(&e.to.pin)),
        ) else {
            continue;
        };
        let pts = sample(&bezier(a.pos, b.pos), 24);
        if pts.windows(2).any(|s| dist_to_segment(p, s[0], s[1]) < radius) {
            return Some(i);
        }
    }
    None
}

fn pin_under(frame: &Frame, p: Pos2, want_output: bool, radius: f32) -> Option<PinEnd> {
    let mut best: Option<(f32, PinEnd)> = None;
    for (id, l) in &frame.layouts {
        let list = if want_output { &l.outputs } else { &l.inputs };
        for s in list {
            let d = (s.pos - p).length();
            if d < radius && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                best = Some((
                    d,
                    PinEnd {
                        node: *id,
                        key: s.key.clone(),
                        output: want_output,
                    },
                ));
            }
        }
    }
    best.map(|(_, p)| p)
}

fn paint_wire(ui: &Ui, a: Pos2, b: Pos2, color: Color32, width: f32, zoom: f32) {
    let dx = ((b.x - a.x).abs() * 0.5).max(40.0 * zoom);
    ui.painter().add(egui::epaint::CubicBezierShape::from_points_stroke(
        [a, a + vec2(dx, 0.0), b - vec2(dx, 0.0), b],
        false,
        Color32::TRANSPARENT,
        Stroke::new(width, color),
    ));
}

pub fn show(ui: &mut Ui, rect: Rect, ed: &mut GraphEditor, env: &Env) {
    let pal = env.pal;
    let ctx = ui.ctx().clone();
    ed.canvas_rect = rect;
    if std::mem::take(&mut ed.frame_request) {
        frame_all(ed);
    }

    let bg_id = ui.id().with("graph_canvas");
    let bg = ui.interact(rect, bg_id, Sense::click_and_drag());
    ui.painter().rect_filled(rect, w::SQUARE, pal.sheet);

    let view = ed.scope().view;
    let zoom = view.zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    let mut transform = TSTransform::new(rect.min.to_vec2() + vec2(view.pan[0], view.pan[1]), zoom);
    let pointer = ctx.pointer_latest_pos();
    let over_canvas = pointer.is_some_and(|p| rect.contains(p)) && ed.menu.is_none();
    let (scroll, zoom_delta, mid_down, ptr_delta, space) = ctx.input(|i| {
        (
            i.smooth_scroll_delta,
            i.zoom_delta(),
            i.pointer.middle_down(),
            i.pointer.delta(),
            i.key_down(egui::Key::Space),
        )
    });
    let mut view_moved = false;
    if over_canvas && let Some(p) = pointer {
        let factor = zoom_delta * (scroll.y * 0.0015).exp();
        if (factor - 1.0).abs() > 1e-4 {
            let at = transform.inverse() * p;
            let nz = (transform.scaling * factor).clamp(MIN_ZOOM, MAX_ZOOM);
            transform.scaling = nz;
            transform.translation = p.to_vec2() - at.to_vec2() * nz;
            view_moved = true;
        }
        if scroll.x != 0.0 {
            transform.translation.x += scroll.x;
            view_moved = true;
        }
        if mid_down && ptr_delta != egui::Vec2::ZERO {
            transform.translation += ptr_delta;
            view_moved = true;
        }
    }
    if matches!(ed.interaction, Interaction::Pan) && ptr_delta != egui::Vec2::ZERO {
        transform.translation += ptr_delta;
        view_moved = true;
    }
    if let Some(id) = ed.focus_request.take()
        && let Some(n) = ed.scope().node(id)
    {
        let l = node_ui::layout(n);
        transform.translation = rect.center().to_vec2() - l.rect.center().to_vec2() * transform.scaling;
        ed.selection = [id].into_iter().collect();
        view_moved = true;
    }
    if view_moved {
        let pan = transform.translation - rect.min.to_vec2();
        let v = &mut ed.scope_mut().view;
        v.pan = [pan.x, pan.y];
        v.zoom = transform.scaling;
        ed.view_changed();
    }
    let zoom = transform.scaling;
    let to_canvas = transform.inverse();
    let pointer_canvas = pointer.map(|p| to_canvas * p);
    ed.pointer_canvas = pointer_canvas.filter(|_| over_canvas);

    let visible = to_canvas * rect;
    let to_screen = |p: Pos2| transform * p;
    let radius = |px: f32| px / zoom;
    let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("graph_nodes"));
    cui.set_clip_rect(rect.intersect(ui.clip_rect()));

    let step = if zoom < 0.5 { GRID * 4.0 } else { GRID * 2.0 };
    let grid = Stroke::new(1.0, pal.faded(pal.hair, 0.6));
    let mut x = (visible.left() / step).floor() * step;
    while x < visible.right() {
        cui.painter().vline(to_screen(pos2(x, 0.0)).x, rect.y_range(), grid);
        x += step;
    }
    let mut y = (visible.top() / step).floor() * step;
    while y < visible.bottom() {
        cui.painter().hline(rect.x_range(), to_screen(pos2(0.0, y)).y, grid);
        y += step;
    }

    let scope = scope_of(&ed.doc, &ed.path).clone();
    let frame = Frame {
        layouts: scope.nodes.iter().map(|n| (n.id, node_ui::layout(n))).collect(),
        scope: &scope,
    };
    let mut connected: HashMap<NodeId, HashSet<String>> = HashMap::new();
    for e in &scope.edges {
        connected.entry(e.to.node).or_default().insert(e.to.pin.clone());
    }
    let probe = env.snap.graph.probe.clone();
    let group_probes: HashMap<NodeId, shocking_vrc_core::graph::NodeProbe> = scope
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Group(_)))
        .filter_map(|n| super::node_probe(&probe, &ed.path, n).map(|p| (n.id, p)))
        .collect();
    let time = ctx.input(|i| i.time);
    let path = ed.path.clone();
    let flat_of = |id: NodeId| {
        let mut p = path.clone();
        p.push(id);
        shocking_vrc_core::graph::flat_id(&p)
    };

    let splice = match (&ed.interaction, ed.selection.len(), pointer_canvas) {
        (Interaction::MoveNodes { moved: true }, 1, Some(_)) => {
            let id = *ed.selection.iter().next().expect("one selected");
            let has_io = frame
                .layouts
                .get(&id)
                .is_some_and(|l| !l.inputs.is_empty() && !l.outputs.is_empty());
            let centre = frame.layouts.get(&id).map(|l| l.rect.center());
            match (has_io, centre) {
                (true, Some(c)) => wire_under(&frame, c, id, radius(10.0)).map(|e| (id, e)),
                _ => None,
            }
        }
        _ => None,
    };

    let mut order: Vec<NodeId> = scope
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Frame(_)))
        .map(|n| n.id)
        .collect();

    for (i, e) in scope.edges.iter().enumerate() {
        let (Some(a), Some(b)) = (
            frame.layouts.get(&e.from.node).and_then(|l| l.output(&e.from.pin)),
            frame.layouts.get(&e.to.node).and_then(|l| l.input(&e.to.pin)),
        ) else {
            continue;
        };
        if !Rect::from_two_pos(a.pos, b.pos).expand(80.0).intersects(visible) {
            continue;
        }
        let out_idx = frame
            .layouts
            .get(&e.from.node)
            .and_then(|l| l.outputs.iter().position(|o| o.key == e.from.pin));
        let value = out_idx
            .and_then(|k| {
                group_probes
                    .get(&e.from.node)
                    .or_else(|| probe.nodes.get(&flat_of(e.from.node)))
                    .and_then(|p| p.outputs.get(k))
            })
            .copied()
            .unwrap_or(0.0);
        let type_col = node_ui::pin_color(pal, a.ty);
        let lit = brightness(value);
        let mut col = lerp_color(pal.hair, type_col, 0.35 + 0.65 * lit);
        if splice.is_some_and(|(_, ei)| ei == i) {
            col = pal.contrast;
        }
        let width = (1.4 + lit * 1.2) * zoom.sqrt().clamp(0.7, 1.4);
        let (sa, sb) = (to_screen(a.pos), to_screen(b.pos));
        paint_wire(&cui, sa, sb, col, width, zoom);
        if lit > 0.02 {
            let pts = bezier(a.pos, b.pos);
            let phase = ((time * 0.6 + (i as f64) * 0.137).fract()) as f32;
            let dot = to_screen(bezier_at(&pts, phase));
            let size = (4.0 * zoom).clamp(3.0, 6.0);
            cui.painter()
                .rect_filled(Rect::from_center_size(dot, vec2(size, size)), w::SQUARE, type_col);
        }
    }

    order.extend(
        scope
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::Frame(_)) && !ed.selection.contains(&n.id))
            .map(|n| n.id),
    );
    order.extend(
        scope
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::Frame(_)) && ed.selection.contains(&n.id))
            .map(|n| n.id),
    );

    let issues = &env.snap.graph.issues;
    let empty = HashSet::new();
    let mut changed: Vec<shocking_vrc_core::graph::Node> = Vec::new();
    let mut header_events: Vec<(NodeId, egui::Response)> = Vec::new();
    let mut pin_started: Option<PinEnd> = None;
    let mut enter: Option<NodeId> = None;
    let mut pick: Option<NodeId> = None;
    let mut add_port: Option<NodeId> = None;
    let mut resize: Option<(NodeId, egui::Vec2)> = None;
    let wire_target = match (&ed.interaction, pointer_canvas) {
        (Interaction::Wire { from }, Some(p)) => {
            pin_under(&frame, p, !from.output, radius(PIN_SNAP)).map(|t| t.node)
        }
        _ => None,
    };

    for id in &order {
        let Some(lay) = frame.layouts.get(id) else { continue };
        if !lay.rect.expand(24.0).intersects(visible) {
            continue;
        }
        let screen = lay.to_screen(&transform);
        let Some(original) = scope.node(*id) else { continue };
        let is_group = matches!(original.kind, NodeKind::Group(_));
        let mut node = original.clone();
        let flat = flat_of(*id);
        let device = match &node.kind {
            NodeKind::Device(d) => Some(match d.binding.mac() {
                Some(mac) => {
                    let info = env.snap.devices.iter().find(|x| x.mac.eq_ignore_ascii_case(mac));
                    let outputs = env
                        .snap
                        .graph
                        .outputs
                        .iter()
                        .find(|o| o.mac.eq_ignore_ascii_case(mac))
                        .map(|o| o.channels);
                    DeviceView {
                        online: info.is_some_and(|i| i.connected),
                        battery: info.and_then(|i| i.battery),
                        outputs,
                        caps: env.snap.graph.caps,
                    }
                }
                None => DeviceView {
                    online: false,
                    battery: None,
                    outputs: None,
                    caps: env.snap.graph.caps,
                },
            }),
            _ => None,
        };
        let view = NodeView {
            pal,
            probe: group_probes.get(id).or_else(|| probe.nodes.get(&flat)),
            connected: connected.get(id).unwrap_or(&empty),
            selected: ed.selection.contains(id),
            hovered_wire_target: wire_target == Some(*id),
            device,
            traces: &ed.traces,
            flat,
            scope: probe.scopes.get(&flat),
            issue: issues.iter().find(|i| i.node == Some(*id) && ed.path.is_empty()),
            zoom,
        };
        let resp = node_ui::draw(&mut cui, &mut node, &screen, &view, &mut ed.typing);
        if node != *original && !is_group {
            changed.push(node);
        }
        if let Some(h) = resp.header {
            header_events.push((*id, h));
        }
        if resp.pin_drag_started.is_some() {
            pin_started = resp.pin_drag_started;
        }
        if resp.enter_group {
            enter = Some(*id);
        }
        if resp.pick_device {
            pick = Some(*id);
        }
        if resp.add_port {
            add_port = Some(*id);
        }
        if let Some(d) = resp.resize {
            resize = Some((*id, d));
        }
    }

    if !changed.is_empty() {
        ed.edit(|g| {
            for n in changed {
                if let Some(slot) = g.node_mut(n.id) {
                    *slot = n;
                }
            }
            g.prune_dangling_edges();
        });
    }
    if let Some((id, d)) = resize {
        ed.edit(|g| {
            if let Some(n) = g.node_mut(id)
                && let NodeKind::Frame(p) = &mut n.kind
            {
                p.size = [(p.size[0] + d.x).max(80.0), (p.size[1] + d.y).max(60.0)];
            }
        });
    }
    if let Some(id) = add_port {
        ed.edit(|g| {
            let Some(n) = g.node_mut(id) else { return };
            let prefix = if matches!(n.kind, NodeKind::GroupInput(_)) { "in" } else { "out" };
            if let NodeKind::GroupInput(p) | NodeKind::GroupOutput(p) = &mut n.kind {
                let k = (1..)
                    .map(|i| format!("{prefix}{i}"))
                    .find(|k| !p.ports.iter().any(|x| &x.key == k))
                    .unwrap_or_default();
                p.ports.push(GroupPort {
                    label: k.clone(),
                    key: k,
                });
            }
        });
    }
    if let Some(id) = enter {
        ed.enter_group(id);
    }
    if let Some(id) = pick {
        ed.selection = [id].into_iter().collect();
        ed.side_tab = super::SideTab::Inspector;
    }

    let shift = ctx.input(|i| i.modifiers.shift);
    let command = ctx.input(|i| i.modifiers.command);

    for (id, h) in &header_events {
        if h.clicked() {
            if shift || command {
                if !ed.selection.remove(id) {
                    ed.selection.insert(*id);
                }
            } else {
                ed.selection = [*id].into_iter().collect();
            }
            ed.menu = None;
        }
        if h.double_clicked() {
            ed.enter_group(*id);
        }
        if h.drag_started_by(egui::PointerButton::Primary) {
            if !ed.selection.contains(id) {
                if !shift {
                    ed.selection.clear();
                }
                ed.selection.insert(*id);
            }
            ed.begin_edit();
            ed.interaction = Interaction::MoveNodes { moved: false };
        }
        if h.dragged_by(egui::PointerButton::Primary)
            && matches!(ed.interaction, Interaction::MoveNodes { .. })
        {
            let d = h.drag_delta() / zoom;
            if d != egui::Vec2::ZERO {
                let mut moving: HashSet<NodeId> = ed.selection.iter().copied().collect();
                for sid in ed.selection.iter() {
                    if let Some(fl) = frame.layouts.get(sid)
                        && matches!(scope.node(*sid).map(|n| &n.kind), Some(NodeKind::Frame(_)))
                    {
                        for (oid, ol) in &frame.layouts {
                            if fl.rect.contains_rect(ol.rect) {
                                moving.insert(*oid);
                            }
                        }
                    }
                }
                ed.edit(|g| {
                    for n in g.nodes.iter_mut().filter(|n| moving.contains(&n.id)) {
                        n.pos[0] += d.x;
                        n.pos[1] += d.y;
                    }
                });
                ed.interaction = Interaction::MoveNodes { moved: true };
            }
        }
        if h.drag_stopped() && matches!(ed.interaction, Interaction::MoveNodes { .. }) {
            if ed.snap_grid {
                let sel: HashSet<NodeId> = ed.selection.iter().copied().collect();
                ed.edit(|g| {
                    for n in g.nodes.iter_mut().filter(|n| sel.contains(&n.id)) {
                        n.pos = [(n.pos[0] / GRID).round() * GRID, (n.pos[1] / GRID).round() * GRID];
                    }
                });
            }
            if let Some((nid, ei)) = splice {
                splice_into(ed, env, &scope, nid, ei);
            }
            ed.interaction = Interaction::None;
        }
    }

    if let Some(start) = pin_started {
        let mut from = start.clone();
        if !start.output {
            let to = PinRef::new(start.node, start.key.clone());
            if let Some(e) = scope.source_of(&to).cloned() {
                ed.edit(|g| g.disconnect_input(&to));
                from = PinEnd {
                    node: e.from.node,
                    key: e.from.pin,
                    output: true,
                };
            }
        }
        ed.interaction = Interaction::Wire { from };
    }
    if let (Interaction::Wire { from }, Some(p)) = (ed.interaction.clone(), pointer_canvas) {
        let anchor = frame.layouts.get(&from.node).and_then(|l| {
            if from.output {
                l.output(&from.key)
            } else {
                l.input(&from.key)
            }
        });
        if let Some(a) = anchor {
            let target = pin_under(&frame, p, !from.output, radius(PIN_SNAP));
            let end = target
                .as_ref()
                .and_then(|t| frame.layouts.get(&t.node).and_then(|l| if t.output { l.output(&t.key) } else { l.input(&t.key) }))
                .map(|s| s.pos)
                .unwrap_or(p);
            let col = node_ui::pin_color(pal, a.ty);
            let (sa, se) = (to_screen(a.pos), to_screen(end));
            if from.output {
                paint_wire(&cui, sa, se, col, 2.0, zoom);
            } else {
                paint_wire(&cui, se, sa, col, 2.0, zoom);
            }
            if ctx.input(|i| i.pointer.primary_released()) {
                match target {
                    Some(t) => {
                        let (out, inp) = if from.output { (from, t) } else { (t, from) };
                        let result = ed.edit(|g| {
                            g.connect(PinRef::new(out.node, out.key.clone()), PinRef::new(inp.node, inp.key.clone()))
                        });
                        if let Err(e) = result {
                            env.toast(ToastKind::Error, format!("Not connected: {e}"));
                        }
                    }
                    None => {
                        if let Some(sp) = pointer {
                            ed.menu = Some(AddMenu {
                                screen: sp,
                                at: p,
                                filter: String::new(),
                                from_wire: Some(from),
                                focus: true,
                            });
                        }
                    }
                }
                ed.interaction = Interaction::None;
            }
        }
    }

    if bg.drag_started_by(egui::PointerButton::Primary)
        && matches!(ed.interaction, Interaction::None)
        && let Some(p) = pointer_canvas
    {
        ed.interaction = if space {
            Interaction::Pan
        } else if command {
            Interaction::Cut { points: vec![p] }
        } else {
            Interaction::BoxSelect {
                start: p,
                additive: shift,
            }
        };
    }
    let released = ctx.input(|i| i.pointer.primary_released());
    match ed.interaction.clone() {
        Interaction::BoxSelect { start, additive } => {
            if let Some(p) = pointer_canvas {
                let r = Rect::from_two_pos(start, p);
                let sr = Rect::from_two_pos(to_screen(start), to_screen(p));
                cui.painter().rect_filled(sr, w::SQUARE, pal.faded(pal.accent, 0.85));
                cui.painter()
                    .rect_stroke(sr, w::SQUARE, Stroke::new(1.0, pal.accent), StrokeKind::Inside);
                if released {
                    if !additive {
                        ed.selection.clear();
                    }
                    for (id, l) in &frame.layouts {
                        let inside = match scope.node(*id).map(|n| &n.kind) {
                            Some(NodeKind::Frame(_)) => r.contains_rect(l.rect),
                            _ => r.intersects(l.rect),
                        };
                        if inside {
                            ed.selection.insert(*id);
                        }
                    }
                    ed.interaction = Interaction::None;
                }
            }
        }
        Interaction::Cut { mut points } => {
            if let Some(p) = pointer_canvas
                && points.last().is_none_or(|l| (*l - p).length() > 4.0 / zoom)
            {
                points.push(p);
            }
            let shown: Vec<Pos2> = points.iter().map(|p| to_screen(*p)).collect();
            cui.painter().add(egui::Shape::dashed_line(
                &shown,
                Stroke::new(1.5, pal.error),
                6.0,
                4.0,
            ));
            if released {
                let mut cut = Vec::new();
                for e in &scope.edges {
                    let (Some(a), Some(b)) = (
                        frame.layouts.get(&e.from.node).and_then(|l| l.output(&e.from.pin)),
                        frame.layouts.get(&e.to.node).and_then(|l| l.input(&e.to.pin)),
                    ) else {
                        continue;
                    };
                    let wire = sample(&bezier(a.pos, b.pos), 24);
                    let hit = points.windows(2).any(|c| {
                        wire.windows(2).any(|s| segments_cross(c[0], c[1], s[0], s[1]))
                    });
                    if hit {
                        cut.push(e.clone());
                    }
                }
                if !cut.is_empty() {
                    ed.edit(|g| g.edges.retain(|e| !cut.contains(e)));
                }
                ed.interaction = Interaction::None;
            } else {
                ed.interaction = Interaction::Cut { points };
            }
        }
        Interaction::Pan if released || !ctx.input(|i| i.pointer.primary_down()) => {
            ed.interaction = Interaction::None;
        }
        Interaction::MoveNodes { .. } if !ctx.input(|i| i.pointer.primary_down()) => {
            ed.interaction = Interaction::None;
        }
        Interaction::Wire { .. } if !ctx.input(|i| i.pointer.primary_down()) => {
            ed.interaction = Interaction::None;
        }
        _ => {}
    }
    if bg.clicked() {
        if !shift {
            ed.selection.clear();
        }
        ed.menu = None;
        ed.typing = None;
    }
    if bg.secondary_clicked()
        && let (Some(sp), Some(p)) = (pointer, pointer_canvas)
    {
        ed.menu = Some(AddMenu {
            screen: sp,
            at: p,
            filter: String::new(),
            from_wire: None,
            focus: true,
        });
    }

    if over_canvas {
        shortcuts(ed, env, &ctx, &frame);
    }

    if scope.nodes.is_empty() {
        w::text_in(
            ui,
            rect,
            "right-click or Shift+A to add a node — connect a Coyote and its Device node appears here",
            font::small(),
            pal.dim,
            Align2::CENTER_CENTER,
        );
    }

    w::dashed_rect(ui, rect, pal.hair);
    if ed.minimap {
        minimap(ui, rect, ed, &frame, &transform, pal);
    }
}

fn splice_into(ed: &mut GraphEditor, env: &Env, scope: &Graph, node: NodeId, edge: usize) {
    let Some(e) = scope.edges.get(edge).cloned() else { return };
    let Some(n) = scope.node(node) else { return };
    let (Some(input), Some(output)) = (n.inputs().into_iter().next(), n.outputs().into_iter().next()) else {
        return;
    };
    let result = ed.edit(|g| {
        let snapshot = g.edges.clone();
        g.edges.retain(|x| *x != e);
        let a = g.connect(e.from.clone(), PinRef::new(node, input.key.to_string()));
        let b = g.connect(PinRef::new(node, output.key.to_string()), e.to.clone());
        if a.is_err() || b.is_err() {
            g.edges = snapshot;
            return Err(());
        }
        Ok(())
    });
    if result.is_err() {
        env.toast(ToastKind::Error, "Cannot splice there — it would make a loop");
    }
}

fn shortcuts(ed: &mut GraphEditor, env: &Env, ctx: &egui::Context, frame: &Frame) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    let events = ctx.input(|i| i.events.clone());
    for ev in events {
        match ev {
            egui::Event::Copy => {
                if let Some(frag) = ed.copy_selection()
                    && let Ok(json) = serde_json::to_string(&frag)
                {
                    ctx.copy_text(json);
                }
            }
            egui::Event::Cut => {
                if let Some(frag) = ed.copy_selection()
                    && let Ok(json) = serde_json::to_string(&frag)
                {
                    ctx.copy_text(json);
                }
                ed.delete_selection(env);
            }
            egui::Event::Paste(text) => {
                let at = ed.pointer_canvas;
                match parse_graph(&text) {
                    Ok(g) if !g.nodes.is_empty() => ed.paste(Some(g), at),
                    _ => ed.paste(None, at),
                }
            }
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                use egui::Key;
                match (key, modifiers.command, modifiers.shift) {
                    (Key::Delete | Key::Backspace | Key::X, false, _) => ed.delete_selection(env),
                    (Key::A, false, true) => {
                        if let (Some(sp), Some(p)) = (ctx.pointer_latest_pos(), ed.pointer_canvas) {
                            ed.menu = Some(AddMenu {
                                screen: sp,
                                at: p,
                                filter: String::new(),
                                from_wire: None,
                                focus: true,
                            });
                        }
                    }
                    (Key::A, true, _) => {
                        ed.selection = frame.scope.nodes.iter().map(|n| n.id).collect();
                    }
                    (Key::D, true, _) | (Key::D, false, true) => ed.duplicate(),
                    (Key::Z, true, false) => ed.undo(),
                    (Key::Z, true, true) | (Key::Y, true, _) => ed.redo(),
                    (Key::F, false, _) => frame_view(ed, frame),
                    (Key::H, false, _) => {
                        let sel: HashSet<NodeId> = ed.selection.iter().copied().collect();
                        if !sel.is_empty() {
                            ed.edit(|g| {
                                for n in g.nodes.iter_mut().filter(|n| sel.contains(&n.id)) {
                                    n.collapsed = !n.collapsed;
                                }
                            });
                        }
                    }
                    (Key::G, true, false) => ed.group_selection(env),
                    (Key::G, true, true) => ed.ungroup_selection(),
                    (Key::Tab, false, _) => {
                        let single = (ed.selection.len() == 1)
                            .then(|| *ed.selection.iter().next().expect("one"));
                        let is_group = single.is_some_and(|id| {
                            matches!(frame.scope.node(id).map(|n| &n.kind), Some(NodeKind::Group(_)))
                        });
                        match single {
                            Some(id) if is_group => ed.enter_group(id),
                            _ => ed.exit_group(),
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

fn frame_view(ed: &mut GraphEditor, frame: &Frame) {
    let rects: Vec<Rect> = frame
        .layouts
        .iter()
        .filter(|(id, _)| ed.selection.is_empty() || ed.selection.contains(id))
        .map(|(_, l)| l.rect)
        .collect();
    let Some(first) = rects.first().copied() else { return };
    let bounds = rects.iter().fold(first, |a, r| a.union(*r)).expand(40.0);
    let area = ed.canvas_rect;
    if !area.is_finite() || area.width() < 10.0 {
        return;
    }
    let zoom = (area.width() / bounds.width())
        .min(area.height() / bounds.height())
        .clamp(MIN_ZOOM, 1.25);
    let pan = area.size() * 0.5 - bounds.center().to_vec2() * zoom;
    let v = &mut ed.scope_mut().view;
    v.zoom = zoom;
    v.pan = [pan.x, pan.y];
    ed.view_changed();
}

pub fn frame_all(ed: &mut GraphEditor) {
    let scope = scope_of(&ed.doc, &ed.path).clone();
    let frame = Frame {
        layouts: scope.nodes.iter().map(|n| (n.id, node_ui::layout(n))).collect(),
        scope: &scope,
    };
    let sel = std::mem::take(&mut ed.selection);
    frame_view(ed, &frame);
    ed.selection = sel;
}

fn minimap(ui: &mut Ui, rect: Rect, ed: &mut GraphEditor, frame: &Frame, transform: &TSTransform, pal: &Palette) {
    if frame.layouts.is_empty() {
        return;
    }
    let size = vec2(168.0, 104.0);
    let mm = Rect::from_min_size(rect.right_bottom() - size - vec2(10.0, 10.0), size);
    let mut mui = ui.new_child(egui::UiBuilder::new().max_rect(mm));
    mui.set_clip_rect(mm);
    let visible = transform.inverse() * rect;
    let bounds = frame
        .layouts
        .values()
        .fold(visible, |a, l| a.union(l.rect))
        .expand(40.0);
    let scale = (mm.width() / bounds.width()).min(mm.height() / bounds.height());
    let map = |p: Pos2| mm.min + (p - bounds.min) * scale;
    let painter = mui.painter().clone();
    painter.rect_filled(mm, w::SQUARE, pal.base);
    for (id, l) in &frame.layouts {
        let r = Rect::from_min_max(map(l.rect.min), map(l.rect.max));
        let col = if ed.selection.contains(id) { pal.accent } else { pal.dim };
        painter.rect_filled(r, w::SQUARE, pal.faded(col, 0.3));
    }
    let v = Rect::from_min_max(map(visible.min), map(visible.max));
    painter.rect_stroke(v, w::SQUARE, Stroke::new(1.0, pal.contrast), StrokeKind::Inside);
    painter.rect_stroke(mm, w::SQUARE, Stroke::new(1.0, pal.hair), StrokeKind::Inside);
    let resp = mui.interact(mm, ui.id().with("minimap_click"), Sense::click_and_drag());
    if (resp.clicked() || resp.dragged())
        && let Some(p) = resp.interact_pointer_pos()
    {
        let target = bounds.min + (p - mm.min) / scale;
        let pan = rect.size() * 0.5 - target.to_vec2() * transform.scaling;
        let view = &mut ed.scope_mut().view;
        view.pan = [pan.x, pan.y];
        ed.view_changed();
    }
}

pub fn thumbnail(ui: &mut Ui, pal: &Palette, rect: Rect, graph: &Graph) {
    ui.painter().rect_filled(rect, w::SQUARE, pal.well);
    ui.painter()
        .rect_stroke(rect, w::SQUARE, Stroke::new(1.0, pal.hair), StrokeKind::Inside);
    let layouts: HashMap<NodeId, NodeLayout> = graph.nodes.iter().map(|n| (n.id, node_ui::layout(n))).collect();
    let Some(bounds) = layouts.values().map(|l| l.rect).reduce(|a, b| a.union(b)) else {
        w::text_in(ui, rect, "empty", font::tiny(), pal.dim, Align2::CENTER_CENTER);
        return;
    };
    let inner = rect.shrink(8.0);
    let bounds = bounds.expand(10.0);
    let scale = (inner.width() / bounds.width()).min(inner.height() / bounds.height());
    let offset = inner.center() - bounds.center().to_vec2() * scale;
    let map = |p: Pos2| pos2(offset.x + p.x * scale, offset.y + p.y * scale);
    let painter = ui.painter().with_clip_rect(rect);
    for e in &graph.edges {
        if let (Some(a), Some(b)) = (
            layouts.get(&e.from.node).and_then(|l| l.output(&e.from.pin)),
            layouts.get(&e.to.node).and_then(|l| l.input(&e.to.pin)),
        ) {
            let col = node_ui::pin_color(pal, if a.ty == PinType::Bool { PinType::Bool } else { PinType::Float });
            painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                bezier(map(a.pos), map(b.pos)),
                false,
                Color32::TRANSPARENT,
                Stroke::new(1.0, pal.faded(col, 0.3)),
            ));
        }
    }
    for (id, l) in &layouts {
        let r = Rect::from_min_max(map(l.rect.min), map(l.rect.max));
        let is_device = graph.node(*id).is_some_and(|n| n.kind.is_device());
        painter.rect_filled(r, w::SQUARE, if is_device { pal.faded(pal.success, 0.4) } else { pal.sheet });
        painter.rect_stroke(r, w::SQUARE, Stroke::new(1.0, pal.hair), StrokeKind::Inside);
    }
}
