use egui::{Pos2, pos2, vec2};
use egui_kittest::Harness;

use shocking_vrc_app::app_state::DeviceSlotInfo;
use shocking_vrc_core::cli::EngineMode;
use shocking_vrc_core::graph::kind::{ContactParams, DeviceParams};
use shocking_vrc_core::graph::{Graph, Node, NodeId, NodeKind};
use shocking_vrc_gui::app::{Ctx, UiState};
use shocking_vrc_gui::bridge::{Bridge, Command, GraphDocShared, Snapshot};
use shocking_vrc_gui::graph_editor::node_ui;
use shocking_vrc_gui::model::Section;
use shocking_vrc_gui::theme::{self, Palette, ThemeState};
use shocking_vrc_gui::views;

const MAC: &str = "AA:BB:CC:DD:EE:FF";
const CONTACT: NodeId = NodeId(1);
const DEVICE: NodeId = NodeId(2);

struct Scene {
    state: UiState,
    bridge: Bridge,
    rx: tokio::sync::mpsc::UnboundedReceiver<Command>,
    snap: Snapshot,
    log: shocking_vrc_app::logger::LogBuffer,
    pal: Palette,
}

fn graph() -> Graph {
    let mut g = Graph::default();
    let mut c = Node::new(NodeKind::Contact(ContactParams::default()), [40.0, 40.0]);
    c.id = CONTACT;
    g.add(c);
    let mut d = Node::new(NodeKind::Device(DeviceParams::bound_to(MAC)), [420.0, 40.0]);
    d.id = DEVICE;
    g.add(d);
    g
}

fn harness() -> Harness<'static, Scene> {
    harness_sized(1280.0)
}

fn harness_sized(width: f32) -> Harness<'static, Scene> {
    let (bridge, rx) = Bridge::detached();
    if let Ok(mut d) = bridge.shared.graph_doc.write() {
        *d = GraphDocShared {
            graph: std::sync::Arc::new(graph()),
            rev: 1,
            replaced: 0,
        };
    }
    let mut snap = Snapshot::empty_for_test();
    snap.mode = EngineMode::Graph;
    snap.devices = vec![DeviceSlotInfo {
        mac: MAC.into(),
        name: "Coyote".into(),
        battery: Some(80),
        connected: true,
        reconnecting: false,
    }];
    let scene = Scene {
        state: UiState::default_for(Section::Graph),
        bridge,
        rx,
        snap,
        log: Default::default(),
        pal: *ThemeState::new("terminal").palette(),
    };
    let mut h = Harness::builder()
        .with_size(vec2(width, 860.0))
        .build_ui_state(
            |ui, s: &mut Scene| {
                theme::install_style(ui.ctx(), &s.pal);
                let mut cx = Ctx {
                    pal: &s.pal,
                    snap: &s.snap,
                    bridge: &s.bridge,
                    ui_state: &mut s.state,
                    log: &s.log,
                };
                views::graph::show(ui, &mut cx);
            },
            scene,
        );
    h.run_steps(3);
    h
}

fn pin(h: &Harness<Scene>, node: NodeId, key: &str, output: bool) -> Pos2 {
    let ed = &h.state().state.graph;
    let n = ed.doc.node(node).expect("node");
    let l = node_ui::layout(n);
    let slot = if output { l.output(key) } else { l.input(key) }.expect("pin");
    to_screen(h, slot.pos)
}

fn to_screen(h: &Harness<Scene>, p: Pos2) -> Pos2 {
    let ed = &h.state().state.graph;
    let v = ed.doc.view;
    let r = ed.canvas_rect;
    pos2(r.left() + v.pan[0] + p.x * v.zoom, r.top() + v.pan[1] + p.y * v.zoom)
}

fn drag(h: &mut Harness<Scene>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.step();
    h.drag_at(from);
    h.step();
    for k in 1..=6 {
        let t = k as f32 / 6.0;
        h.hover_at(from + (to - from) * t);
        h.step();
    }
    h.drop_at(to);
    h.step();
    h.step();
}

#[test]
fn the_editor_adopts_the_runtime_graph() {
    let h = harness();
    let ed = &h.state().state.graph;
    assert!(ed.initialized());
    assert_eq!(ed.doc.nodes.len(), 2);
    assert!(ed.canvas_rect.width() > 200.0, "canvas laid out: {:?}", ed.canvas_rect);
}

#[test]
fn the_canvas_follows_the_window_size() {
    let mut h = harness();
    let small = h.state().state.graph.canvas_rect;
    h.set_size(vec2(1920.0, 1080.0));
    h.run_steps(3);
    let large = h.state().state.graph.canvas_rect;
    assert!(large.width() > small.width() + 500.0, "{small:?} → {large:?}");
    assert!(large.height() > small.height() + 150.0, "{small:?} → {large:?}");
    h.set_size(vec2(1000.0, 700.0));
    h.run_steps(3);
    let shrunk = h.state().state.graph.canvas_rect;
    assert!(shrunk.right() <= 1000.0 && shrunk.bottom() <= 700.0, "{shrunk:?}");
}

#[test]
fn dragging_from_an_output_to_an_input_wires_them() {
    let mut h = harness();
    let from = pin(&h, CONTACT, "level", true);
    let to = pin(&h, DEVICE, "a.power", false);
    drag(&mut h, from, to);
    let edges = &h.state().state.graph.doc.edges;
    assert!(
        edges
            .iter()
            .any(|e| e.from.node == CONTACT && e.from.pin == "level" && e.to.node == DEVICE && e.to.pin == "a.power"),
        "no wire: {edges:?}"
    );
    let mut sent = false;
    while let Ok(cmd) = h.state_mut().rx.try_recv() {
        sent |= matches!(cmd, Command::SetGraph(_));
    }
    assert!(sent, "SetGraph was not sent");
}

#[test]
fn dropping_a_wire_on_empty_canvas_opens_the_add_menu() {
    let mut h = harness();
    let from = pin(&h, CONTACT, "level", true);
    let empty = to_screen(&h, pos2(260.0, 420.0));
    drag(&mut h, from, empty);
    let menu = h.state().state.graph.menu.clone();
    assert!(menu.is_some_and(|m| m.from_wire.is_some()), "menu should open with the wire attached");
}

#[test]
fn dragging_a_header_moves_the_node() {
    let mut h = harness();
    let before = h.state().state.graph.doc.node(CONTACT).unwrap().pos;
    let header = to_screen(&h, pos2(before[0] + 60.0, before[1] + 10.0));
    drag(&mut h, header, header + vec2(90.0, 60.0));
    let after = h.state().state.graph.doc.node(CONTACT).unwrap().pos;
    let zoom = h.state().state.graph.doc.view.zoom;
    assert!(
        (after[0] - before[0] - 90.0 / zoom).abs() < 3.0 && (after[1] - before[1] - 60.0 / zoom).abs() < 3.0,
        "moved {before:?} → {after:?}"
    );
    assert!(h.state().state.graph.can_undo(), "a move is undoable");
}

#[test]
fn the_wheel_zooms_around_the_pointer() {
    let mut h = harness();
    let z0 = h.state().state.graph.doc.view.zoom;
    let at = to_screen(&h, pos2(300.0, 300.0));
    h.hover_at(at);
    h.step();
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, 120.0),
        modifiers: egui::Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    });
    h.run_steps(8);
    let z1 = h.state().state.graph.doc.view.zoom;
    assert!(z1 > z0, "zoom {z0} → {z1}");
}

fn menu_rect(h: &Harness<Scene>) -> egui::Rect {
    h.ctx
        .memory(|m| m.area_rect(egui::Id::new(shocking_vrc_gui::graph_editor::menu::MENU_ID)))
        .expect("the add menu is open")
}

fn open_menu(h: &mut Harness<Scene>) {
    let at = to_screen(h, pos2(250.0, 380.0));
    h.hover_at(at);
    h.step();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }
    h.run_steps(3);
}

#[test]
fn the_add_menu_keeps_its_size_while_searching_and_after() {
    let mut h = harness();
    open_menu(&mut h);
    let full = menu_rect(&h);
    assert!(full.height() > 300.0, "menu opens at full height: {full:?}");

    h.state_mut().state.graph.menu.as_mut().unwrap().filter = "sequencer".into();
    h.run_steps(3);
    let filtered = menu_rect(&h);
    assert!((filtered.height() - full.height()).abs() < 2.0, "{full:?} → {filtered:?}");

    h.state_mut().state.graph.menu.as_mut().unwrap().filter.clear();
    h.run_steps(3);
    assert!((menu_rect(&h).height() - full.height()).abs() < 2.0, "it comes back too");

    h.state_mut().state.graph.menu.as_mut().unwrap().filter = "metronome".into();
    h.run_steps(3);
    h.state_mut().state.graph.menu = None;
    h.run_steps(2);
    open_menu(&mut h);
    let again = menu_rect(&h);
    assert!((again.height() - full.height()).abs() < 2.0, "reopened {again:?} vs {full:?}");
}

#[test]
fn right_click_on_the_canvas_opens_the_add_menu() {
    let mut h = harness();
    let at = to_screen(&h, pos2(250.0, 380.0));
    h.hover_at(at);
    h.step();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }
    h.step();
    assert!(h.state().state.graph.menu.is_some());
}

#[test]
#[ignore]
fn two_hundred_nodes_frame_time() {
    use shocking_vrc_core::graph::PinRef;
    use shocking_vrc_core::graph::kind::{MathOp, MathParams};
    let mut g = graph();
    let mut prev = CONTACT;
    for i in 0..200u64 {
        let mut n = Node::new(
            NodeKind::Math(MathParams { op: MathOp::Add }),
            [40.0 + (i % 20) as f32 * 230.0, 200.0 + (i / 20) as f32 * 120.0],
        );
        n.id = NodeId(1000 + i);
        let id = g.add(n);
        let out = if prev == CONTACT { "level" } else { "out" };
        let _ = g.connect(PinRef::new(prev, out), PinRef::new(id, "a"));
        prev = id;
    }
    let mut h = harness();
    {
        let s = h.state_mut();
        if let Ok(mut d) = s.bridge.shared.graph_doc.write() {
            d.graph = std::sync::Arc::new(g);
            d.rev = 2;
            d.replaced = 1;
        }
    }
    h.run_steps(3);
    for (label, zoom) in [("fit (all on screen)", 0.25f32), ("zoomed in (culled)", 1.0)] {
        h.state_mut().state.graph.doc.view.zoom = zoom;
        h.run_steps(3);
        let n = 60;
        let t0 = std::time::Instant::now();
        for _ in 0..n {
            h.step();
        }
        let per = t0.elapsed() / n;
        println!("{} nodes, {label}: {per:?} per frame", h.state().state.graph.doc.nodes.len());
    }
}

#[test]
fn delete_spares_a_connected_devices_node() {
    let mut h = harness();
    h.state_mut().state.graph.selection = [CONTACT, DEVICE].into_iter().collect();
    let at = to_screen(&h, pos2(250.0, 380.0));
    h.hover_at(at);
    h.step();
    h.key_press(egui::Key::Delete);
    h.run_steps(3);
    let doc = &h.state().state.graph.doc;
    assert!(doc.node(CONTACT).is_none(), "the contact is deleted");
    assert!(doc.node(DEVICE).is_some(), "the connected device's node stays");
    h.state_mut().state.graph.undo();
    assert!(h.state().state.graph.doc.node(CONTACT).is_some(), "and undo restores it");
}

fn side_overflow(h: &Harness<Scene>) -> Option<String> {
    let side = h.state().state.graph.side_rect;
    let over: Vec<f32> = h.ctx.viewport(|v| {
        v.prev_pass
            .widgets
            .layers()
            .flat_map(|(_, rects)| rects.iter())
            .filter(|w| w.rect.left() >= side.left() - 0.5)
            .map(|w| w.rect.right() - side.right())
            .filter(|d| *d > 0.5)
            .collect()
    });
    let worst = over.iter().copied().fold(0.0f32, f32::max);
    (!over.is_empty()).then(|| format!("{} widget(s) past the edge, worst by {worst:.0} px", over.len()))
}

#[test]
fn the_side_panel_fits_every_inspector() {
    use shocking_vrc_core::graph::kind::{GroupParams, GroupPort, PortsParams, catalog};
    use shocking_vrc_gui::graph_editor::SideTab;

    let mut g = graph();
    let mut ids = vec![CONTACT, DEVICE];
    for (i, e) in catalog().into_iter().enumerate() {
        let mut n = Node::new((e.make)(), [40.0, 400.0 + i as f32 * 40.0]);
        n.id = NodeId(100 + i as u64);
        ids.push(g.add(n));
    }
    let port = |key: &str| GroupPort { key: key.into(), label: "a fairly long port label".into() };
    let mut inner = Graph::default();
    inner.add(Node::new(NodeKind::GroupInput(PortsParams { ports: vec![port("in1"), port("in2")] }), [0.0, 0.0]));
    inner.add(Node::new(NodeKind::GroupOutput(PortsParams { ports: vec![port("out1")] }), [300.0, 0.0]));
    let mut group = Node::new(
        NodeKind::Group(GroupParams { name: "Group".into(), graph: Box::new(inner.clone()) }),
        [40.0, 300.0],
    );
    group.id = NodeId(90);
    ids.push(g.add(group));
    for (i, n) in inner.nodes.into_iter().enumerate() {
        let mut n = n;
        n.id = NodeId(95 + i as u64);
        ids.push(g.add(n));
    }

    for width in [1280.0, 900.0] {
        let mut h = harness_sized(width);
        {
            let s = h.state_mut();
            if let Ok(mut d) = s.bridge.shared.graph_doc.write() {
                d.graph = std::sync::Arc::new(g.clone());
                d.rev = 2;
                d.replaced = 1;
            }
        }
        h.run_steps(3);
        let mut problems = Vec::new();
        let mut check = |h: &mut Harness<Scene>, what: &str| {
            h.run_steps(3);
            if let Some(p) = side_overflow(h) {
                problems.push(format!("{width} px, {what}: {p}"));
            }
        };
        check(&mut h, "graph summary");
        for id in &ids {
            h.state_mut().state.graph.selection = [*id].into_iter().collect();
            let label = h.state().state.graph.doc.node(*id).map(|n| n.kind.label()).unwrap_or_default();
            check(&mut h, &label);
        }
        h.state_mut().snap.devices[0].connected = false;
        h.state_mut().state.graph.selection = [DEVICE].into_iter().collect();
        check(&mut h, "offline device");
        h.state_mut().state.graph.selection = [CONTACT, DEVICE].into_iter().collect();
        check(&mut h, "two nodes");
        for tab in [SideTab::Presets, SideTab::Devices] {
            h.state_mut().state.graph.side_tab = tab;
            check(&mut h, &format!("{tab:?} tab"));
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }
}
