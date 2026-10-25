use egui::Ui;

use shocking_vrc_core::cli::EngineMode;

use crate::app::Ctx;
use crate::bridge::Command;
use crate::theme::widgets::{self as w, font, space};

pub fn show(ui: &mut Ui, cx: &mut Ctx) -> bool {
    let mut start_tutorial = false;

    let gap = space::SHEET_GAP;
    let col_w = (ui.available_width() - gap) * 0.5;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        w::column(ui, col_w, |ui| connection(ui, cx));
        w::column(ui, col_w, |ui| osc(ui, cx));
    });

    ui.add_space(space::SHEET_GAP);
    preferences(ui, cx);

    ui.add_space(space::SHEET_GAP);
    if help(ui, cx) {
        start_tutorial = true;
    }

    start_tutorial
}

fn connection(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    w::sheet(ui, pal, |ui| {
        w::heading(ui, pal, "Connection");

        let vrc = cx.snap.vrchat_found;
        status_line(
            ui,
            cx,
            vrc,
            if vrc {
                "VRChat detected".to_string()
            } else {
                "VRChat not found — enable OSC in game".to_string()
            },
        );

        let count = cx.snap.devices.iter().filter(|d| d.connected).count();
        status_line(
            ui,
            cx,
            count > 0,
            match count {
                0 => "Coyote searching…".to_string(),
                1 => "Coyote connected".to_string(),
                n => format!("{n} Coyotes connected"),
            },
        );

        status_line(
            ui,
            cx,
            !cx.snap.avatar_zones.is_empty(),
            format!("{} avatar zone(s) discovered", cx.snap.avatar_zones.len()),
        );

        ui.add_space(space::ITEM_GAP);
        w::dim(
            ui,
            pal,
            "Devices are found and reconnected automatically — there is nothing to pair.",
        );
    });
}

fn status_line(ui: &mut Ui, cx: &Ctx, ok: bool, text: String) {
    let pal = cx.pal;
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(r, w::SQUARE, if ok { pal.success } else { pal.warning });
        ui.label(
            egui::RichText::new(text)
                .font(font::body())
                .color(pal.contrast),
        );
    });
}

fn osc(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    w::sheet(ui, pal, |ui| {
        w::heading(ui, pal, "OSC & OSCQuery");

        let port_line = if cx.snap.osc_port == 0 {
            "selecting…".to_string()
        } else if cx.snap.oscquery_port == Some(cx.snap.osc_port) {
            format!("{}", cx.snap.osc_port)
        } else {
            format!("{} (fixed, legacy mode)", cx.snap.osc_port)
        };

        ui.horizontal(|ui| {
            w::field_label(ui, pal, "Port", 96.0, false);
            w::body(ui, pal, port_line);
        });
        if let Some(http) = cx.snap.oscquery_port
            && Some(cx.snap.osc_port) != Some(http) {
                ui.horizontal(|ui| {
                    w::field_label(ui, pal, "OSCQuery", 96.0, false);
                    w::body(ui, pal, http.to_string());
                });
            }

        ui.add_space(space::ITEM_GAP);
        w::dim(
            ui,
            pal,
            "A free port is chosen at startup, used for both OSC (UDP) and OSCQuery (HTTP), \
             then advertised to VRChat over mDNS. Just enable OSC in VRChat — it will find \
             this app on its own.",
        );

        ui.add_space(space::ITEM_GAP);
        w::caption(ui, pal, "Custom DGB contacts");
        ui.add_space(2.0);
        for line in [
            "1.  Add a VRC Contact Receiver to your avatar",
            "2.  Set Receiver Type to Proximity",
            "3.  Set Parameter to DGB/YourContactName",
            "4.  Upload the avatar",
        ] {
            ui.label(
                egui::RichText::new(line)
                    .font(font::small())
                    .color(pal.dim),
            );
        }
    });
}

fn preferences(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    w::sheet(ui, pal, |ui| {
        w::heading(ui, pal, "Preferences");

        mode_row(ui, cx);
        ui.add_space(space::ROW_GAP);

        toggle_row(
            ui,
            cx,
            "Auto-save",
            "Write this device's config to devices/ on every change",
            |s| s.auto_save,
            |s, v| s.auto_save = v,
            Command::SetAutoSave,
        );

        ui.add_space(space::ROW_GAP);

        toggle_row(
            ui,
            cx,
            "Alarm section",
            "Adds the wake-up alarm to the navigation. Turning it off disarms the alarm.",
            |s| s.alarm_section_visible,
            |s, v| s.alarm_section_visible = v,
            Command::SetAlarmTabVisible,
        );

        ui.add_space(space::ROW_GAP);

        toggle_row(
            ui,
            cx,
            "Spoof guard",
            "Ignore contacts that jump from idle straight to a high level (avatar mods that shock without touch)",
            |s| s.spoof_guard,
            |s, v| s.spoof_guard = v,
            Command::SetSpoofGuard,
        );

        ui.add_space(space::ROW_GAP);
        ui.horizontal(|ui| {
            w::field_label(ui, pal, "Nickname", 120.0, true);
            let resp = w::text_field(
                ui,
                pal,
                &mut cx.ui_state.nickname,
                "(no author)",
                220.0,
            );
            if resp.lost_focus() {
                let nick = cx.ui_state.nickname.clone();
                if let Err(e) = shocking_vrc_app::prefs::update_ui_prefs(|p| p.nickname = nick) {
                    log::warn!("[prefs] Failed to save nickname: {e}");
                }
            }
            ui.add_space(6.0);
            w::dim(ui, pal, "Credited as the author on presets you save");
        });
    });
}

fn mode_row(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let cur = cx.ui_state.mode;
    ui.horizontal(|ui| {
        w::field_label(ui, pal, "Mode", 120.0, true);
        let sel = match cur {
            EngineMode::Classic => 0,
            EngineMode::Graph => 1,
        };
        if let Some(i) = w::segmented(ui, pal, &["Classic", "Graph"], sel) {
            let want = if i == 0 { EngineMode::Classic } else { EngineMode::Graph };
            if want != cur {
                cx.ui_state.mode_confirm = Some(want);
            }
        }
        ui.add_space(6.0);
        w::dim(ui, pal, "Classic maps zones onto channels; Graph lets you wire your own nodes");
    });
    if cur != EngineMode::Graph {
        return;
    }
    for (ch, label) in ["Graph cap A", "Graph cap B"].iter().enumerate() {
        ui.add_space(space::ROW_GAP);
        ui.horizontal(|ui| {
            w::field_label(ui, pal, label, 120.0, true);
            let v = cx.ui_state.graph_caps[ch];
            if let Some(d) = w::stepper(ui, pal, &v.to_string(), 48.0) {
                let next = (v as i32 + d * 5).clamp(0, 200) as u8;
                cx.ui_state.graph_caps[ch] = next;
                cx.send(Command::SetGraphCaps(cx.ui_state.graph_caps));
            }
            ui.add_space(6.0);
            w::dim(ui, pal, "Hard ceiling after the graph — no node or limit pin goes above it");
        });
    }
}

fn toggle_row(
    ui: &mut Ui,
    cx: &mut Ctx,
    label: &str,
    hint: &str,
    get: impl Fn(&crate::app::UiState) -> bool,
    set: impl Fn(&mut crate::app::UiState, bool),
    cmd: impl Fn(bool) -> Command,
) {
    let pal = cx.pal;
    ui.horizontal(|ui| {
        w::field_label(ui, pal, label, 120.0, true);
        let on = get(cx.ui_state);
        if let Some(next) = w::toggle(ui, pal, on)
            && next != on {
                set(cx.ui_state, next);
                cx.send(cmd(next));
            }
        ui.add_space(6.0);
        w::dim(ui, pal, hint);
    });
}

fn help(ui: &mut Ui, cx: &mut Ctx) -> bool {
    let pal = cx.pal;
    let mut start = false;
    w::sheet(ui, pal, |ui| {
        w::heading(ui, pal, "Help");
        ui.horizontal(|ui| {
            if w::cta(ui, pal, "Open the guided tour").clicked() {
                start = true;
            }
            if w::button(ui, pal, "Check for updates").clicked() {
                cx.send(Command::CheckUpdate);
                cx.bridge.shared.notify(
                    crate::bridge::ToastKind::Info,
                    "Checking for updates…",
                );
            }
            if w::button(ui, pal, "About").clicked() {
                cx.ui_state.show_about = true;
            }
        });

        ui.add_space(space::ITEM_GAP);
        w::caption(ui, pal, "Keyboard");
        ui.add_space(2.0);
        for (keys, what) in [
            ("1 – 9", "jump to a section"),
            ("T", "open the theme panel"),
            ("Esc", "close the topmost dialog"),
        ] {
            ui.horizontal(|ui| {
                w::field_label(ui, pal, keys, 80.0, false);
                w::dim(ui, pal, what);
            });
        }
        if cx.ui_state.mode == EngineMode::Graph {
            for (keys, what) in [
                ("F12", "panic — every device to 0 until released"),
                ("Shift+A", "add a node on the canvas"),
            ] {
                ui.horizontal(|ui| {
                    w::field_label(ui, pal, keys, 80.0, false);
                    w::dim(ui, pal, what);
                });
            }
        }

        ui.add_space(space::ITEM_GAP);
        w::caption(ui, pal, "Safety");
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new(
                "Start with a low power ceiling — 20 to 30 — and raise it slowly. \
                 Presets and zone sets never change your limits.",
            )
            .font(font::small())
            .color(pal.warning),
        );
    });
    start
}
