use egui::{Align2, Rect, Sense, Ui, vec2};

use shocking_vrc_core::graph::kind::{Category, CatalogEntry, DeviceParams, catalog};
use shocking_vrc_core::graph::{NodeKind, PinRef};

use super::{Env, GraphEditor};
use crate::bridge::ToastKind;
use crate::theme::widgets::{self as w, font, space};

pub const MENU_ID: &str = "graph_add_menu";
const LIST_H: f32 = 340.0;

enum Pick {
    Kind(NodeKind),
}

fn matches(e: &CatalogEntry, needle: &str) -> bool {
    needle.is_empty()
        || e.label.to_lowercase().contains(needle)
        || e.keywords.contains(needle)
        || e.category.label().to_lowercase().contains(needle)
}

pub fn show(ctx: &egui::Context, ed: &mut GraphEditor, env: &Env) {
    let Some(menu) = ed.menu.as_mut() else { return };
    let pal = env.pal;
    let needle = menu.filter.trim().to_lowercase();
    let wire = menu.from_wire.clone();
    let entries: Vec<CatalogEntry> = catalog()
        .into_iter()
        .filter(|e| matches(e, &needle))
        .filter(|e| match &wire {
            Some(w) => {
                let k = (e.make)();
                if w.output {
                    !k.inputs().is_empty()
                } else {
                    !k.outputs().is_empty()
                }
            }
            None => true,
        })
        .filter(|e| !(!ed.path.is_empty() && e.category == Category::Output && e.label.starts_with("Device")))
        .collect();

    let known: Vec<String> = if wire.as_ref().is_none_or(|w| w.output) && ed.path.is_empty() {
        let in_graph = |m: &str| ed.doc.device_node_for(m).is_some();
        let mut macs: Vec<String> = env
            .bridge
            .shared
            .known_devices
            .read()
            .map(|k| k.clone())
            .unwrap_or_default();
        macs.extend(env.snap.devices.iter().map(|d| d.mac.clone()));
        macs.sort();
        macs.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        macs.into_iter()
            .filter(|m| !in_graph(m))
            .filter(|m| needle.is_empty() || "device coyote".contains(&needle) || m.to_lowercase().contains(&needle))
            .collect()
    } else {
        Vec::new()
    };

    let mut pick: Option<Pick> = None;
    let mut close = false;
    let screen = menu.screen;
    let focus = std::mem::take(&mut menu.focus);

    egui::Area::new(egui::Id::new(MENU_ID))
        .order(egui::Order::Foreground)
        .fixed_pos(screen)
        .constrain(true)
        .default_size(vec2(280.0, LIST_H + 140.0))
        .sizing_pass(focus)
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(pal.base)
                .inner_margin(egui::Margin::same(space::SHEET_PAD as i8 - 4))
                .show(ui, |ui| {
                    ui.set_width(260.0);
                    w::caption(ui, pal, if wire.is_some() { "Add & connect" } else { "Add node" });
                    ui.add_space(4.0);
                    let r = w::text_field(ui, pal, &mut menu.filter, "search…", 244.0);
                    if focus || ui.memory(|m| m.focused().is_none()) {
                        r.request_focus();
                    }
                    if r.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && let Some(e) = entries.first()
                    {
                        pick = Some(Pick::Kind((e.make)()));
                    }
                    ui.add_space(space::ROW_GAP);
                    egui::ScrollArea::vertical()
                        .max_height(LIST_H)
                        .min_scrolled_height(LIST_H)
                        .auto_shrink([false, false])
                        .id_salt("graph_add_list")
                        .show(ui, |ui| {
                            for cat in Category::ALL {
                                let in_cat: Vec<&CatalogEntry> =
                                    entries.iter().filter(|e| e.category == cat).collect();
                                let devices_here = cat == Category::Output && !known.is_empty();
                                if in_cat.is_empty() && !devices_here {
                                    continue;
                                }
                                ui.add_space(4.0);
                                w::caption(ui, pal, cat.label());
                                for e in in_cat {
                                    if row(ui, env, e.label) {
                                        pick = Some(Pick::Kind((e.make)()));
                                    }
                                }
                                if devices_here {
                                    for mac in &known {
                                        let label = format!("Device …{}", crate::app::short_mac(mac));
                                        if row(ui, env, &label) {
                                            pick = Some(Pick::Kind(NodeKind::Device(DeviceParams::bound_to(mac))));
                                        }
                                    }
                                }
                            }
                            if entries.is_empty() && known.is_empty() {
                                w::empty_state(ui, pal, "nothing matches");
                            }
                        });
                });
            let outer = ui.min_rect();
            w::dashed_rect(ui, outer, pal.hair);
        });

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close = true;
    }
    if ctx.input(|i| i.pointer.any_pressed())
        && let Some(p) = ctx.pointer_interact_pos()
        && let Some(area) = ctx.memory(|m| m.area_rect(egui::Id::new(MENU_ID)))
        && !area.contains(p)
    {
        close = true;
    }

    if let Some(Pick::Kind(kind)) = pick {
        let at = menu.at;
        let from = menu.from_wire.clone();
        ed.menu = None;
        let id = ed.add_node(kind, at);
        if let Some(from) = from {
            let node = ed.scope().node(id).cloned();
            if let Some(node) = node {
                let result = if from.output {
                    node.inputs().first().map(|p| {
                        ed.edit(|g| g.connect(PinRef::new(from.node, from.key.clone()), PinRef::new(id, p.key.to_string())))
                    })
                } else {
                    node.outputs().first().map(|p| {
                        ed.edit(|g| g.connect(PinRef::new(id, p.key.to_string()), PinRef::new(from.node, from.key.clone())))
                    })
                };
                if let Some(Err(e)) = result {
                    env.toast(ToastKind::Error, format!("Not connected: {e}"));
                }
            }
        }
    } else if close {
        ed.menu = None;
    }
}

fn row(ui: &mut Ui, env: &Env, label: &str) -> bool {
    let pal = env.pal;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, w::SQUARE, pal.well);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    w::text_in(
        ui,
        Rect::from_min_max(rect.min + vec2(8.0, 0.0), rect.max),
        label,
        font::small(),
        if resp.hovered() { pal.accent } else { pal.contrast },
        Align2::LEFT_CENTER,
    );
    resp.clicked()
}
