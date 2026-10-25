use egui::Context;

use shocking_vrc_core::cli::EngineMode;

use crate::app::Ctx;
use crate::bridge::Command;
use crate::theme::widgets::{self as w, font, space};

#[derive(Debug, Clone)]
pub enum GraphDialog {
    SavePreset {
        name: String,
        author: String,
        fragment: bool,
    },
    ConfirmApply {
        id: String,
    },
    ConfirmDelete {
        id: String,
        name: String,
    },
    Rename {
        id: String,
        name: String,
    },
    ExportGraph {
        name: String,
    },
    ImportGraph {
        path: String,
    },
    ImportPreset {
        path: String,
    },
}

impl GraphDialog {
    pub fn save_preset(nickname: &str, fragment: bool) -> Self {
        GraphDialog::SavePreset {
            name: String::new(),
            author: nickname.to_string(),
            fragment,
        }
    }
}

pub fn show(ctx: &Context, cx: &mut Ctx) {
    mode_confirm(ctx, cx);
    let Some(mut dialog) = cx.ui_state.graph_dialog.clone() else {
        return;
    };
    let pal = cx.pal;
    let mut close = false;

    let dismissed = match &mut dialog {
        GraphDialog::SavePreset { name, author, fragment } => {
            let title = if *fragment { "Save selection as fragment" } else { "Save graph as preset" };
            let ed = &cx.ui_state.graph;
            let count = if *fragment { ed.selection.len() } else { ed.doc.nodes.len() };
            let mut commit = false;
            let d = super::modal(ctx, pal, "graph_preset_save", title, 420.0, |ui| {
                w::dim(ui, pal, "Stored in graph-presets/user/ as one JSON file per preset.");
                ui.add_space(space::ITEM_GAP);
                ui.horizontal(|ui| {
                    w::field_label(ui, pal, "Name", 72.0, true);
                    let r = w::text_field(ui, pal, name, "(random)", 300.0);
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        commit = true;
                    }
                });
                ui.horizontal(|ui| {
                    w::field_label(ui, pal, "Author", 72.0, true);
                    w::text_field(ui, pal, author, "(no author)", 300.0);
                });
                ui.add_space(space::ITEM_GAP);
                w::caption(ui, pal, "What gets saved");
                ui.add_space(2.0);
                for line in [
                    format!("{count} node(s), their settings, wires and layout"),
                    "Device nodes as numbered slots — never a MAC".to_string(),
                ] {
                    ui.label(egui::RichText::new(line).font(font::small()).color(pal.dim));
                }
                ui.add_space(space::ITEM_GAP);
                ui.horizontal(|ui| {
                    if w::cta(ui, pal, "Save").clicked() {
                        commit = true;
                    }
                    if w::button(ui, pal, "Cancel").clicked() {
                        close = true;
                    }
                });
            });
            if commit {
                let n = if name.trim().is_empty() {
                    shocking_vrc_app::graph_presets::random_graph_preset_name()
                } else {
                    name.trim().to_string()
                };
                let a = author.trim().to_string();
                cx.ui_state.nickname = a.clone();
                if let Err(e) = shocking_vrc_app::prefs::update_ui_prefs(|p| p.nickname = a.clone()) {
                    log::warn!("[prefs] Failed to save nickname: {e}");
                }
                let preset = if *fragment {
                    cx.ui_state.graph.preset_from_selection(&n, &a)
                } else {
                    Some(cx.ui_state.graph.preset_from_graph(&n, &a))
                };
                if let Some(p) = preset {
                    cx.send(Command::SaveGraphPreset(Box::new(p)));
                }
                close = true;
            }
            d
        }
        GraphDialog::ConfirmApply { id } => {
            let entry = cx
                .bridge
                .shared
                .graph_presets
                .read()
                .ok()
                .and_then(|p| p.entries.iter().find(|e| &e.id == id).cloned());
            let Some(entry) = entry else {
                cx.ui_state.graph_dialog = None;
                return;
            };
            let mut apply = false;
            let d = super::modal(ctx, pal, "graph_preset_apply", "Apply preset", 400.0, |ui| {
                ui.label(
                    egui::RichText::new(format!("“{}”", entry.name()))
                        .font(font::mono_big())
                        .color(pal.contrast),
                );
                ui.add_space(4.0);
                w::dim(
                    ui,
                    pal,
                    "Replaces the whole working graph. Undo brings it back; graph.json is only written by auto-save or Save.",
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Device outputs drop to 0 and ramp back in.")
                        .font(font::small())
                        .color(pal.warning),
                );
                ui.add_space(space::ITEM_GAP);
                ui.horizontal(|ui| {
                    if w::cta(ui, pal, "Apply").clicked() {
                        apply = true;
                    }
                    if w::button(ui, pal, "Cancel").clicked() {
                        close = true;
                    }
                });
            });
            if apply {
                cx.ui_state.graph.apply_preset(&entry.preset, &cx.snap.devices);
                cx.bridge.shared.notify(
                    crate::bridge::ToastKind::Success,
                    format!("“{}” applied — Ctrl+Z to undo", entry.name()),
                );
                close = true;
            }
            d
        }
        GraphDialog::ConfirmDelete { id, name } => {
            let mut delete = false;
            let d = super::modal(ctx, pal, "graph_preset_delete", "Delete preset", 360.0, |ui| {
                ui.label(egui::RichText::new(format!("“{name}”")).font(font::mono_big()).color(pal.contrast));
                ui.add_space(4.0);
                w::dim(ui, pal, "This removes the file from graph-presets/user/. It cannot be undone.");
                ui.add_space(space::ITEM_GAP);
                ui.horizontal(|ui| {
                    if w::danger_button(ui, pal, "Delete", true).clicked() {
                        delete = true;
                    }
                    if w::button(ui, pal, "Cancel").clicked() {
                        close = true;
                    }
                });
            });
            if delete {
                cx.send(Command::DeleteGraphPreset(id.clone()));
                if cx.ui_state.graph.sel_preset.as_deref() == Some(id.as_str()) {
                    cx.ui_state.graph.sel_preset = None;
                }
                close = true;
            }
            d
        }
        GraphDialog::Rename { id, name } => {
            let mut commit = false;
            let d = super::modal(ctx, pal, "graph_preset_rename", "Rename preset", 400.0, |ui| {
                ui.horizontal(|ui| {
                    w::field_label(ui, pal, "Name", 72.0, true);
                    let r = w::text_field(ui, pal, name, "new name", 300.0);
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        commit = true;
                    }
                });
                ui.add_space(space::ITEM_GAP);
                ui.horizontal(|ui| {
                    if w::cta(ui, pal, "Rename").clicked() {
                        commit = true;
                    }
                    if w::button(ui, pal, "Cancel").clicked() {
                        close = true;
                    }
                });
            });
            if commit && !name.trim().is_empty() {
                cx.send(Command::RenameGraphPreset(id.clone(), name.trim().to_string()));
                cx.ui_state.graph.sel_preset = Some(shocking_vrc_app::presets::slugify(name));
                close = true;
            }
            d
        }
        GraphDialog::ExportGraph { name } => {
            let mut commit = false;
            let d = super::modal(ctx, pal, "graph_export", "Export graph", 400.0, |ui| {
                w::dim(ui, pal, "Writes a copy of the working graph to graphs/<name>.json.");
                ui.add_space(space::ITEM_GAP);
                ui.horizontal(|ui| {
                    w::field_label(ui, pal, "Name", 72.0, true);
                    let r = w::text_field(ui, pal, name, "my-graph", 300.0);
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        commit = true;
                    }
                });
                ui.add_space(space::ITEM_GAP);
                ui.horizontal(|ui| {
                    if w::cta(ui, pal, "Export").clicked() {
                        commit = true;
                    }
                    if w::button(ui, pal, "Cancel").clicked() {
                        close = true;
                    }
                });
            });
            if commit {
                let n = if name.trim().is_empty() { "graph".to_string() } else { name.trim().to_string() };
                cx.send(Command::ExportGraph(n));
                close = true;
            }
            d
        }
        GraphDialog::ImportGraph { path } => import_dialog(ctx, cx, false, path, &mut close),
        GraphDialog::ImportPreset { path } => import_dialog(ctx, cx, true, path, &mut close),
    };

    if dismissed || close {
        cx.ui_state.graph_dialog = None;
    } else if cx.ui_state.graph_dialog.is_some() {
        cx.ui_state.graph_dialog = Some(dialog);
    }
}

fn import_dialog(ctx: &Context, cx: &mut Ctx, is_preset: bool, path: &mut String, close: &mut bool) -> bool {
    let pal = cx.pal;
    let mut commit = false;
    let title = if is_preset { "Import graph preset" } else { "Import graph" };
    let files = shocking_vrc_app::graph_store::list_graph_files();
    let d = super::modal(ctx, pal, "graph_import", title, 460.0, |ui| {
        w::dim(
            ui,
            pal,
            if is_preset {
                "Copies a preset file into graph-presets/user/."
            } else {
                "Replaces the working graph (undo brings it back)."
            },
        );
        ui.add_space(space::ITEM_GAP);
        ui.horizontal(|ui| {
            w::field_label(ui, pal, "File", 72.0, true);
            let r = w::text_field(ui, pal, path, "graphs/my-graph.json", 340.0);
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                commit = true;
            }
        });
        if !files.is_empty() {
            ui.add_space(4.0);
            w::caption(ui, pal, "In graphs/");
            egui::ScrollArea::vertical()
                .max_height(150.0)
                .id_salt("graph_files")
                .show(ui, |ui| {
                    for f in &files {
                        let shown = f.display().to_string();
                        let (rect, resp) = w::row(ui, pal, shown == *path, 20.0);
                        w::text_in(
                            ui,
                            rect.shrink2(egui::vec2(8.0, 0.0)),
                            &shown,
                            font::small(),
                            pal.contrast,
                            egui::Align2::LEFT_CENTER,
                        );
                        if resp.clicked() {
                            *path = shown;
                        }
                    }
                });
        }
        ui.add_space(space::ITEM_GAP);
        ui.horizontal(|ui| {
            if w::cta(ui, pal, "Import").clicked() {
                commit = true;
            }
            if w::button(ui, pal, "Cancel").clicked() {
                *close = true;
            }
        });
    });
    if commit && !path.trim().is_empty() {
        let p = std::path::PathBuf::from(path.trim());
        cx.send(if is_preset {
            Command::ImportGraphPreset(p)
        } else {
            Command::ImportGraph(p)
        });
        *close = true;
    }
    d
}

fn mode_confirm(ctx: &Context, cx: &mut Ctx) {
    let Some(mode) = cx.ui_state.mode_confirm else {
        return;
    };
    let pal = cx.pal;
    let mut go = false;
    let mut cancel = false;
    let title = match mode {
        EngineMode::Graph => "Switch to Graph mode",
        EngineMode::Classic => "Switch to Classic mode",
    };
    let dismissed = super::modal(ctx, pal, "mode_confirm", title, 420.0, |ui| {
        let body = match mode {
            EngineMode::Graph => {
                "Your node graph drives every device. Zones, Channels, Tuning, Modulation and Presets are hidden — they keep their settings for when you come back."
            }
            EngineMode::Classic => {
                "Zones, Channels, Tuning, Modulation and Presets drive every device again. Your graph is kept as it is."
            }
        };
        w::dim(ui, pal, body);
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("All outputs go to 0 first, then ramp back in over 2 s.")
                .font(font::small())
                .color(pal.warning),
        );
        ui.add_space(space::ITEM_GAP);
        ui.horizontal(|ui| {
            if w::cta(ui, pal, "Switch").clicked() {
                go = true;
            }
            if w::button(ui, pal, "Cancel").clicked() {
                cancel = true;
            }
        });
    });
    if go {
        cx.send(Command::SetMode(mode));
        cx.ui_state.mode_confirm = None;
    } else if cancel || dismissed {
        cx.ui_state.mode_confirm = None;
    }
}
