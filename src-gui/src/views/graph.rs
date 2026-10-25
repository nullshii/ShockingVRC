use egui::{Rect, Ui, pos2, vec2};

use shocking_vrc_core::graph::NodeKind;

use crate::app::{Ctx, UiState};
use crate::bridge::Command;
use crate::graph_editor::{Env, GraphEditor, canvas, menu, side};
use crate::overlays::graph_dialogs::GraphDialog;
use crate::theme::widgets::{self as w, font, space};

const SIDE_W: f32 = 340.0;

pub fn show(ui: &mut Ui, cx: &mut Ctx) {
    let env = Env {
        pal: cx.pal,
        snap: cx.snap,
        bridge: cx.bridge,
    };
    let UiState {
        graph: ed,
        graph_dialog,
        nickname,
        ..
    } = &mut *cx.ui_state;

    ed.frame_begin(ui.ctx());
    ed.sync(&env.bridge.shared.graph_doc());
    ed.record_traces(env.snap);
    hotkeys(ui.ctx(), ed, &env);

    toolbar(ui, ed, &env, graph_dialog);
    ui.add_space(space::ROW_GAP);

    let top = ui.cursor().min;
    let width = ui.available_width();
    let avail_h = ui.available_height();
    let height = if avail_h.is_finite() { avail_h.max(320.0) } else { 640.0 };
    let side_w = SIDE_W.min(width * 0.38);
    let canvas_rect = Rect::from_min_size(top, vec2(width - side_w - space::SHEET_GAP, height));
    let side_rect = Rect::from_min_size(pos2(canvas_rect.right() + space::SHEET_GAP, top.y), vec2(side_w, height));
    ui.allocate_rect(Rect::from_min_size(top, vec2(width, height)), egui::Sense::hover());

    canvas::show(ui, canvas_rect, ed, &env);

    ed.side_rect = side_rect;
    let mut side_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(side_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    side::show(&mut side_ui, ed, &env, graph_dialog, nickname);

    menu::show(ui.ctx(), ed, &env);
    ed.flush(env.bridge, ui.ctx());
}

fn toolbar(ui: &mut Ui, ed: &mut GraphEditor, env: &Env, dialog: &mut Option<GraphDialog>) {
    let pal = env.pal;
    let g = &env.snap.graph;
    ui.horizontal(|ui| {
        if w::cta(ui, pal, "+ Add").on_hover_text("Shift+A or right-click on the canvas").clicked() {
            let centre = ed.view_centre();
            let r = ed.canvas_rect;
            ed.menu = Some(crate::graph_editor::AddMenu {
                screen: if r.is_finite() { r.left_top() + vec2(12.0, 12.0) } else { pos2(40.0, 140.0) },
                at: centre,
                filter: String::new(),
                from_wire: None,
                focus: true,
            });
        }
        if w::button_enabled(ui, pal, "Undo", ed.can_undo()).on_hover_text("Ctrl+Z").clicked() {
            ed.undo();
        }
        if w::button_enabled(ui, pal, "Redo", ed.can_redo()).on_hover_text("Ctrl+Shift+Z").clicked() {
            ed.redo();
        }
        ui.add_space(6.0);
        if w::button(ui, pal, "Save").on_hover_text("Write graph.json now").clicked() {
            ed.flush(env.bridge, ui.ctx());
            env.bridge.send(Command::SaveGraph);
        }
        if w::button(ui, pal, "Load").on_hover_text("Reload graph.json (undoable)").clicked() {
            env.bridge.send(Command::ReloadGraph);
        }
        if w::button(ui, pal, "Export…").clicked() {
            *dialog = Some(GraphDialog::ExportGraph { name: String::new() });
        }
        if w::button(ui, pal, "Import…").clicked() {
            *dialog = Some(GraphDialog::ImportGraph { path: String::new() });
        }
        ui.add_space(6.0);
        if w::button(ui, pal, "Frame").on_hover_text("F").clicked() {
            canvas::frame_all(ed);
        }
        let snap_on = ed.snap_grid;
        if let Some(i) = w::segmented(ui, pal, &["Snap", "Free"], if snap_on { 0 } else { 1 }) {
            ed.snap_grid = i == 0;
        }
        if let Some(i) = w::segmented(ui, pal, &["Map", "—"], if ed.minimap { 0 } else { 1 }) {
            ed.minimap = i == 0;
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let latched = g.ui_panic;
            let label = if latched { "RELEASE" } else { "PANIC" };
            let resp = if latched {
                w::cta(ui, pal, label)
            } else {
                w::danger_button(ui, pal, label, true)
            };
            if resp.on_hover_text("F12 — every device to 0 until released").clicked() {
                env.bridge.send(Command::GraphPanic(!latched));
            }
            ui.add_space(8.0);
            let (text, color) = if g.panic {
                ("panic — outputs held at 0".to_string(), pal.error)
            } else if !g.valid && g.tick > 0 {
                ("graph invalid — output 0".to_string(), pal.error)
            } else {
                let errors = g.issues.iter().filter(|i| i.severity == shocking_vrc_core::graph::Severity::Error).count();
                let warns = g.issues.iter().filter(|i| i.severity == shocking_vrc_core::graph::Severity::Warning).count();
                match (errors, warns) {
                    (0, 0) => ("ok".to_string(), pal.success),
                    (0, n) => (format!("{n} warning(s)"), pal.warning),
                    (n, _) => (format!("{n} error(s)"), pal.error),
                }
            };
            ui.label(egui::RichText::new(text).font(font::small()).color(color));
        });
    });

    if !ed.path.is_empty() {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if w::button(ui, pal, "◂ Up").on_hover_text("Tab").clicked() {
                ed.exit_group();
            }
            let mut crumbs = vec!["graph".to_string()];
            let mut g = &ed.doc;
            for id in &ed.path {
                let Some(n) = g.node(*id) else { break };
                crumbs.push(n.display_title());
                if let NodeKind::Group(p) = &n.kind {
                    g = &p.graph;
                }
            }
            w::dim(ui, pal, crumbs.join("  ›  "));
        });
    }
}

fn hotkeys(ctx: &egui::Context, ed: &mut GraphEditor, env: &Env) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    let events = ctx.input(|i| i.events.clone());
    for ev in &events {
        let egui::Event::Key {
            key,
            pressed,
            repeat: false,
            ..
        } = ev
        else {
            continue;
        };
        if *key == egui::Key::F12 && *pressed {
            env.bridge.send(Command::GraphPanic(!env.snap.graph.ui_panic));
        }
        let name = key.name();
        let mut edits: Vec<(shocking_vrc_core::graph::NodeId, NodeKind)> = Vec::new();
        for n in &ed.doc.nodes {
            match &n.kind {
                NodeKind::Toggle(p) if !p.key.is_empty() && p.key.eq_ignore_ascii_case(name) => {
                    let mut q = p.clone();
                    if q.momentary {
                        q.on = *pressed;
                    } else if *pressed {
                        q.on = !q.on;
                    } else {
                        continue;
                    }
                    edits.push((n.id, NodeKind::Toggle(q)));
                }
                NodeKind::Slider(p) if *pressed => {
                    let up = !p.key_up.is_empty() && p.key_up.eq_ignore_ascii_case(name);
                    let down = !p.key_down.is_empty() && p.key_down.eq_ignore_ascii_case(name);
                    if up || down {
                        let mut q = p.clone();
                        let d = if up { q.step } else { -q.step };
                        q.value = (q.value + d).clamp(q.min.min(q.max), q.max.max(q.min));
                        edits.push((n.id, NodeKind::Slider(q)));
                    }
                }
                _ => {}
            }
        }
        if !edits.is_empty() {
            let path = std::mem::take(&mut ed.path);
            ed.edit(|g| {
                for (id, kind) in edits {
                    if let Some(n) = g.node_mut(id) {
                        n.kind = kind;
                    }
                }
            });
            ed.path = path;
        }
    }
}
