use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, vec2};

use shocking_vrc_core::cli::{AlarmPhase, EngineMode};

use crate::app::Ctx;
use crate::bridge::{Command, ToastKind};
use crate::model::Section;
use crate::theme::Palette;
use crate::theme::widgets::{self as w, font, space};

const TWO_COLUMN_MIN_W: f32 = 360.0;
const FOUR_STAT_MIN_W: f32 = 640.0;
const LABEL_W: f32 = 128.0;

pub fn show(ui: &mut Ui, cx: &mut Ctx) -> bool {
    link_strip(ui, cx);

    ui.add_space(space::SHEET_GAP);
    settings_columns(ui, cx);

    ui.add_space(space::SHEET_GAP);
    tools_and_reference(ui, cx)
}

fn link_strip(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let vrc = cx.snap.vrchat_found;
    let connected = cx.snap.devices.iter().filter(|d| d.connected).count();
    let reconnecting = cx
        .snap
        .devices
        .iter()
        .filter(|d| !d.connected && d.reconnecting)
        .count();
    let legacy = cx.snap.osc_port != 0 && cx.snap.oscquery_port != Some(cx.snap.osc_port);

    let suffix = if vrc && connected > 0 {
        "ready"
    } else if legacy {
        "legacy"
    } else {
        "waiting"
    };

    w::sheet(ui, pal, |ui| {
        w::heading_with_suffix(ui, pal, "Link", Some(suffix));

        let avail = ui.available_width();
        let gap = 8.0;
        if avail >= FOUR_STAT_MIN_W {
            let cw = (avail - gap * 3.0) / 4.0;
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                w::column(ui, cw, |ui| stat_vrchat(ui, cx));
                w::column(ui, cw, |ui| stat_coyote(ui, cx, connected, reconnecting));
                w::column(ui, cw, |ui| stat_zones(ui, cx));
                w::column(ui, cw, |ui| stat_osc(ui, cx, legacy));
            });
        } else {
            let cw = (avail - gap) * 0.5;
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                w::column(ui, cw, |ui| stat_vrchat(ui, cx));
                w::column(ui, cw, |ui| stat_coyote(ui, cx, connected, reconnecting));
            });
            ui.add_space(gap);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                w::column(ui, cw, |ui| stat_zones(ui, cx));
                w::column(ui, cw, |ui| stat_osc(ui, cx, legacy));
            });
        }
    });
}

fn stat_vrchat(ui: &mut Ui, cx: &Ctx) {
    let pal = cx.pal;
    let vrc = cx.snap.vrchat_found;
    let resp = w::stat_cell_with(
        ui,
        pal,
        "VRChat",
        if vrc { "online" } else { "offline" },
        if vrc { pal.success } else { pal.error },
        None,
        false,
    );
    if !vrc {
        resp.on_hover_text("Enable OSC in VRChat's radial menu — auto-discovered via OSCQuery");
    }
}

fn stat_coyote(ui: &mut Ui, cx: &mut Ctx, connected: usize, reconnecting: usize) {
    let pal = cx.pal;
    let (value, color) = if connected > 0 {
        (format!("{connected} online"), pal.success)
    } else if reconnecting > 0 {
        ("reconnecting".to_string(), pal.warning)
    } else {
        ("searching…".to_string(), pal.dim)
    };
    let recon_tag = (reconnecting > 0 && connected > 0).then(|| format!("+{reconnecting} recon"));
    let meta = match &recon_tag {
        Some(tag) => Some((tag.as_str(), pal.warning)),
        None if cx.snap.has_device() => Some(("→", pal.dim)),
        None => None,
    };
    let clickable = cx.snap.has_device();
    let resp = w::stat_cell_with(ui, pal, "Coyote", &value, color, meta, clickable);
    let resp = if clickable {
        resp.on_hover_text("Open Status")
    } else {
        resp.on_hover_text("Devices attach and reconnect automatically over BLE")
    };
    if clickable && resp.clicked() {
        cx.ui_state.section = Section::Status;
    }
}

fn stat_zones(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let n = cx.snap.avatar_zones.len();
    let target = if cx.ui_state.mode == EngineMode::Graph {
        Section::Graph
    } else {
        Section::Zones
    };
    let resp = w::stat_cell_with(
        ui,
        pal,
        "Avatar zones",
        &n.to_string(),
        if n > 0 { pal.accent } else { pal.dim },
        Some(("→", pal.dim)),
        true,
    )
    .on_hover_text(format!("Open {}", target.title()));
    if resp.clicked() {
        cx.ui_state.section = target;
    }
}

fn stat_osc(ui: &mut Ui, cx: &Ctx, legacy: bool) {
    let pal = cx.pal;
    let port = cx.snap.osc_port;
    let (value, color, meta) = if port == 0 {
        ("selecting…".to_string(), pal.dim, None)
    } else if legacy {
        (format!(":{port}"), pal.warning, Some(("legacy", pal.warning)))
    } else {
        (format!(":{port}"), pal.contrast, Some(("UDP+HTTP", pal.dim)))
    };
    let clickable = port != 0;
    let resp = w::stat_cell_with(ui, pal, "OSC port", &value, color, meta, clickable);
    let hover = match (port, cx.snap.oscquery_port) {
        (0, _) => "Selecting a free UDP/HTTP port".to_string(),
        (p, Some(http)) if http != p => format!("OSC UDP :{p} · OSCQuery HTTP :{http} — click to copy"),
        (p, _) => format!("OSC UDP & OSCQuery HTTP :{p} — click to copy"),
    };
    if resp.on_hover_text(hover).clicked() && clickable {
        ui.ctx().copy_text(port.to_string());
        cx.bridge
            .shared
            .notify(ToastKind::Info, format!("Copied OSC port {port}"));
    }
}

fn settings_columns(ui: &mut Ui, cx: &mut Ctx) {
    let gap = space::SHEET_GAP;
    let col_w = (ui.available_width() - gap) * 0.5;
    if col_w < TWO_COLUMN_MIN_W {
        engine_sheet(ui, cx);
        ui.add_space(gap);
        workspace_sheet(ui, cx);
        return;
    }
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        w::column(ui, col_w, |ui| engine_sheet(ui, cx));
        w::column(ui, col_w, |ui| workspace_sheet(ui, cx));
    });
}

fn engine_sheet(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let cur = cx.ui_state.mode;
    let suffix = match cur {
        EngineMode::Classic => "classic",
        EngineMode::Graph => "graph",
    };

    w::sheet(ui, pal, |ui| {
        w::heading_with_suffix(ui, pal, "Engine & Safety", Some(suffix));

        w::caption(ui, pal, "Pipeline");
        ui.add_space(2.0);

        control_row(
            ui,
            pal,
            "Mode",
            "Classic maps zones onto channels; Graph wires custom nodes",
            |ui| {
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
            },
            |_| {},
        );

        if cur == EngineMode::Graph {
            ui.add_space(space::ROW_GAP);
            graph_caps_block(ui, cx);
        }

        ui.add_space(space::ITEM_GAP);
        w::caption(ui, pal, "Safety & storage");
        ui.add_space(2.0);

        control_row(
            ui,
            pal,
            "Spoof guard",
            "Ignore contacts that jump from idle straight to a high level",
            |ui| {
                if let Some(next) = w::toggle(ui, pal, cx.ui_state.spoof_guard) {
                    cx.ui_state.spoof_guard = next;
                    cx.send(Command::SetSpoofGuard(next));
                }
            },
            |_| {},
        );

        ui.add_space(space::ROW_GAP);

        let auto_save = cx.ui_state.auto_save;
        let mut save_now = false;
        let mut toggled = None;
        control_row(
            ui,
            pal,
            "Auto-save",
            "Write configuration changes to disk automatically",
            |ui| {
                toggled = w::toggle(ui, pal, auto_save);
            },
            |ui| {
                if !auto_save
                    && w::small_button(ui, pal, "Save now")
                        .on_hover_text("Write the current configuration to disk")
                        .clicked()
                {
                    save_now = true;
                }
            },
        );
        if let Some(next) = toggled {
            cx.ui_state.auto_save = next;
            cx.send(Command::SetAutoSave(next));
        }
        if save_now {
            cx.send(match cur {
                EngineMode::Classic => Command::SaveConfig,
                EngineMode::Graph => Command::SaveGraph,
            });
        }
    });
}

fn graph_caps_block(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let start_y = ui.cursor().top();
    let left_x = ui.cursor().left();
    let indent = 12.0;
    let inner_w = (ui.available_width() - indent).max(120.0);
    let label_w = (LABEL_W - indent - 8.0).max(48.0);

    ui.horizontal(|ui| {
        ui.add_space(indent);
        w::column(ui, inner_w, |ui| {
            for (ch, label) in ["Cap A", "Cap B"].iter().enumerate() {
                let v = cx.ui_state.graph_caps[ch];
                let danger = v > 80;
                let mut slider = w::Slider::new(v as f32, 0.0, 200.0);
                if danger {
                    slider = slider.fill(pal.error);
                }
                if let Some(next) = w::slider_row(
                    ui,
                    pal,
                    label,
                    label_w,
                    slider,
                    &v.to_string(),
                    36.0,
                    Some(if danger { pal.error } else { pal.contrast }),
                ) {
                    let clamped = next.round().clamp(0.0, 200.0) as u8;
                    if clamped != v {
                        cx.ui_state.graph_caps[ch] = clamped;
                        cx.send(Command::SetGraphCaps(cx.ui_state.graph_caps));
                    }
                }
            }
        });
    });

    let end_y = ui.cursor().top() - ui.spacing().item_spacing.y;
    let bar_color = if cx.ui_state.graph_caps.iter().any(|&v| v > 80) {
        pal.error
    } else {
        pal.accent
    };
    let bar_rect = Rect::from_min_max(
        Pos2::new(left_x + 2.0, start_y + 2.0),
        Pos2::new(left_x + 4.0, end_y.max(start_y + 2.0)),
    );
    ui.painter().rect_filled(bar_rect, w::SQUARE, bar_color);
}

fn workspace_sheet(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let active_modules = usize::from(cx.ui_state.alarm_section_visible) + usize::from(cx.snap.remote_enabled);
    let suffix = format!("{active_modules}/2 modules");

    w::sheet(ui, pal, |ui| {
        w::heading_with_suffix(ui, pal, "Workspace", Some(&suffix));

        w::caption(ui, pal, "Modules");
        ui.add_space(2.0);

        let alarm_visible = cx.ui_state.alarm_section_visible;
        let mut alarm_toggled = None;
        let mut open_alarm = false;
        control_row(
            ui,
            pal,
            "Alarm",
            "Wake-up alarm tab — turning off disarms any armed alarm",
            |ui| {
                alarm_toggled = w::toggle(ui, pal, alarm_visible);
            },
            |ui| {
                if alarm_visible {
                    if w::small_button(ui, pal, "Open")
                        .on_hover_text("Jump to Alarm [9]")
                        .clicked()
                    {
                        open_alarm = true;
                    }
                    match cx.snap.alarm_status.phase {
                        AlarmPhase::Ringing => {
                            w::badge(ui, pal, "RINGING", pal.error);
                        }
                        AlarmPhase::Snoozed => {
                            w::tag(ui, pal, "snoozed", pal.warning);
                        }
                        AlarmPhase::Idle if cx.snap.alarm.enabled => {
                            let label = cx.snap.alarm.time_label();
                            w::tag(ui, pal, &label, pal.warning);
                        }
                        AlarmPhase::Idle => {}
                    }
                }
            },
        );
        if let Some(next) = alarm_toggled {
            cx.ui_state.alarm_section_visible = next;
            cx.send(Command::SetAlarmTabVisible(next));
        }
        if open_alarm {
            cx.ui_state.section = Section::Alarm;
        }

        ui.add_space(space::ROW_GAP);

        let remote_enabled = cx.snap.remote_enabled;
        let mut remote_toggled = None;
        let mut open_remote = false;
        control_row(
            ui,
            pal,
            "Partner remote",
            "Partner control tab — off disables all remote listening and sending",
            |ui| {
                remote_toggled = w::toggle(ui, pal, remote_enabled);
            },
            |ui| {
                if remote_enabled {
                    if w::small_button(ui, pal, "Open")
                        .on_hover_text("Jump to Remote [0]")
                        .clicked()
                    {
                        open_remote = true;
                    }
                    let sessions = cx.snap.remote.service.sessions.len();
                    if sessions > 0 {
                        w::badge(ui, pal, &format!("{sessions} live"), pal.accent);
                    } else if cx.snap.remote.service.consent.is_some() {
                        w::tag(ui, pal, "asking", pal.warning);
                    }
                }
            },
        );
        if let Some(next) = remote_toggled {
            cx.send(Command::SetRemoteEnabled(next));
        }
        if open_remote {
            cx.ui_state.section = Section::Remote;
        }

        ui.add_space(space::ITEM_GAP);
        w::caption(ui, pal, "Identity");
        ui.add_space(2.0);

        control_row(
            ui,
            pal,
            "Author",
            "Credited on waveform and graph presets you save",
            |ui| {
                let field_w = ui.available_width().max(100.0);
                let resp = w::text_field(ui, pal, &mut cx.ui_state.nickname, "(no author)", field_w);
                if resp.changed() {
                    cx.ui_state.nickname_dirty = true;
                }
                if resp.lost_focus() && cx.ui_state.nickname_dirty {
                    let nick = cx.ui_state.nickname.clone();
                    cx.ui_state.nickname_dirty = false;
                    match shocking_vrc_app::prefs::update_ui_prefs(|p| p.nickname = nick) {
                        Ok(()) => cx.bridge.shared.notify(ToastKind::Info, "Author saved"),
                        Err(e) => log::warn!("[prefs] Failed to save nickname: {e}"),
                    }
                }
            },
            |_| {},
        );
    });
}

fn control_row(
    ui: &mut Ui,
    pal: &Palette,
    label: &str,
    tooltip: &str,
    add_control: impl FnOnce(&mut Ui),
    add_trailing: impl FnOnce(&mut Ui),
) {
    let row_h = 26.0;
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, row_h), Sense::hover());

    let label_w = LABEL_W.min(width * 0.46);
    let label_rect = Rect::from_min_size(rect.min, vec2(label_w, row_h));
    let label_resp = ui.interact(label_rect, ui.id().with(("row_lbl", label, rect.top() as i32)), Sense::hover());
    w::text_in(ui, label_rect, label, font::body(), pal.contrast, Align2::LEFT_CENTER);
    if !tooltip.is_empty() {
        label_resp.on_hover_text(tooltip);
    }

    let body_rect = Rect::from_min_max(Pos2::new(rect.left() + label_w, rect.top()), rect.max);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(body_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    add_control(&mut child);
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        add_trailing(ui);
    });
}

fn tools_and_reference(ui: &mut Ui, cx: &mut Ctx) -> bool {
    let pal = cx.pal;
    let mut start_tutorial = false;
    let version = format!("v{}", shocking_vrc_core::VERSION);

    let (update_tag, update_checking) = cx
        .bridge
        .shared
        .update
        .read()
        .map(|u| (u.available.as_ref().map(|r| r.tag.clone()), u.checking))
        .unwrap_or((None, false));

    w::sheet(ui, pal, |ui| {
        w::heading_with_suffix(ui, pal, "Tools & Reference", Some(&version));

        ui.horizontal(|ui| {
            let tour_resp = if !cx.ui_state.has_seen_tutorial {
                w::cta(ui, pal, "Guided tour")
            } else {
                w::button(ui, pal, "Guided tour")
            };
            if tour_resp.clicked() {
                start_tutorial = true;
            }

            if let Some(tag) = &update_tag {
                if w::cta(ui, pal, &format!("Update {tag}")).clicked()
                    && let Ok(mut u) = cx.bridge.shared.update.write()
                {
                    u.dismissed = false;
                }
            } else if update_checking {
                w::button_enabled(ui, pal, "Checking…", false);
            } else if w::button(ui, pal, "Check for updates").clicked() {
                cx.send(Command::CheckUpdate);
                cx.bridge.shared.notify(ToastKind::Info, "Checking for updates…");
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if w::button(ui, pal, "About").clicked() {
                    cx.ui_state.show_about = true;
                }
                if theme_swatch_button(ui, pal, cx.ui_state.show_theme_panel).clicked() {
                    cx.ui_state.show_theme_panel = !cx.ui_state.show_theme_panel;
                }
            });
        });

        ui.add_space(space::ITEM_GAP);
        w::dashed_hr(ui, pal.hair);
        ui.add_space(space::ITEM_GAP);

        let avail = ui.available_width();
        let pad = 16.0;
        let col_w = (avail - pad * 2.0 - 1.0) * 0.5;
        if col_w < TWO_COLUMN_MIN_W {
            keyboard_reference(ui, cx);
            ui.add_space(space::ITEM_GAP);
            w::dashed_hr(ui, pal.hair);
            ui.add_space(space::ITEM_GAP);
            dgb_contacts_reference(ui, cx);
        } else {
            let top_y = ui.cursor().top();
            let mid_x = ui.cursor().left() + col_w + pad;
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                w::column(ui, col_w, |ui| keyboard_reference(ui, cx));
                ui.add_space(pad * 2.0 + 1.0);
                w::column(ui, col_w, |ui| dgb_contacts_reference(ui, cx));
            });
            let bottom_y = ui.cursor().top() - ui.spacing().item_spacing.y;
            w::dashed_line(
                ui.painter(),
                Pos2::new(mid_x, top_y),
                Pos2::new(mid_x, bottom_y.max(top_y + 1.0)),
                Stroke::new(1.0, pal.hair),
                4.0,
                4.0,
            );
        }
    });

    start_tutorial
}

fn theme_swatch_button(ui: &mut Ui, pal: &Palette, open: bool) -> egui::Response {
    let font = font::body();
    let galley = ui
        .painter()
        .layout_no_wrap("Theme".to_string(), font.clone(), pal.contrast);
    let sw_w = 10.0;
    let sw_gap = 4.0;
    let text_to_sw = 10.0;
    let swatches_w = sw_w * 3.0 + sw_gap * 2.0;
    let size = vec2(galley.size().x + text_to_sw + swatches_w + 20.0, galley.size().y + 9.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hovered = resp.hovered();
    let p = ui.painter();

    let (fill, fg, border) = match (open, hovered) {
        (_, true) => (pal.well, pal.contrast, pal.accent),
        (true, false) => (pal.well, pal.accent, pal.accent),
        (false, false) => (Color32::TRANSPARENT, pal.contrast, pal.hair),
    };
    if fill != Color32::TRANSPARENT {
        p.rect_filled(rect, w::SQUARE, fill);
    }
    p.rect_stroke(rect, w::SQUARE, Stroke::new(1.0, border), StrokeKind::Inside);

    let text_x = rect.left() + 10.0;
    p.text(Pos2::new(text_x, rect.center().y), Align2::LEFT_CENTER, "Theme", font, fg);

    let mut x = rect.right() - 10.0 - sw_w;
    for c in [pal.base, pal.contrast, pal.accent] {
        let sw = Rect::from_min_size(Pos2::new(x, rect.center().y - sw_w * 0.5), vec2(sw_w, sw_w));
        p.rect_filled(sw, w::SQUARE, c);
        p.rect_stroke(sw, w::SQUARE, Stroke::new(1.0, pal.dim), StrokeKind::Inside);
        x -= sw_w + sw_gap;
    }

    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn reference_header(ui: &mut Ui, pal: &Palette, title: &str, add_right: impl FnOnce(&mut Ui)) {
    let h = 22.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    w::text_in(ui, rect, &title.to_uppercase(), font::tiny(), pal.dim, Align2::LEFT_CENTER);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    add_right(&mut child);
}

fn keyboard_reference(ui: &mut Ui, cx: &Ctx) {
    let pal = cx.pal;
    ui.spacing_mut().item_spacing.y = 5.0;
    reference_header(ui, pal, "Keyboard", |_| {});

    let mut col1: Vec<(&str, &str)> = vec![("1–9", "Switch section"), ("T", "Theme panel"), ("Esc", "Close dialog")];
    if cx.snap.remote_enabled {
        col1.insert(1, ("0", "Remote section"));
    }

    if cx.ui_state.mode == EngineMode::Graph {
        let col2: [(&str, &str); 2] = [("F12", "Panic (hold 0)"), ("Shift+A", "Add node")];
        let gap = 12.0;
        let half_w = (ui.available_width() - gap) * 0.5;
        if half_w >= 170.0 {
            let rows = col1.len().max(col2.len());
            for r in 0..rows {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    w::column(ui, half_w, |ui| {
                        if let Some(&(k, action)) = col1.get(r) {
                            keycap_row(ui, pal, k, action);
                        }
                    });
                    w::column(ui, half_w, |ui| {
                        if let Some(&(k, action)) = col2.get(r) {
                            keycap_row(ui, pal, k, action);
                        }
                    });
                });
            }
            return;
        }
        col1.extend_from_slice(&col2);
    }

    for (k, action) in col1 {
        keycap_row(ui, pal, k, action);
    }
}

fn keycap_row(ui: &mut Ui, pal: &Palette, keys: &str, action: &str) {
    let h = 21.0;
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, h), Sense::hover());

    let font = font::tiny();
    let gw = ui
        .painter()
        .layout_no_wrap(keys.to_string(), font.clone(), pal.contrast)
        .size()
        .x;
    let cap_w = (gw + 14.0).max(24.0);
    let cap = Rect::from_min_size(rect.min, vec2(cap_w, h));
    let p = ui.painter();
    p.rect_filled(cap, w::SQUARE, pal.well);
    p.rect_stroke(cap, w::SQUARE, Stroke::new(1.0, pal.hair), StrokeKind::Inside);
    w::text_in(ui, cap, keys, font, pal.contrast, Align2::CENTER_CENTER);

    let col_offset = 66.0_f32.max(cap_w + 8.0);
    let label_rect = Rect::from_min_max(Pos2::new(rect.left() + col_offset, rect.top()), rect.max);
    w::text_in(ui, label_rect, action, font::small(), pal.dim, Align2::LEFT_CENTER);
}

fn dgb_contacts_reference(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    ui.spacing_mut().item_spacing.y = 5.0;

    let mut copy_prefix = false;
    reference_header(ui, pal, "Custom DGB contacts", |ui| {
        if w::small_button(ui, pal, "Copy DGB/")
            .on_hover_text("Copy DGB/ parameter prefix to clipboard")
            .clicked()
        {
            copy_prefix = true;
        }
    });
    if copy_prefix {
        ui.ctx().copy_text("DGB/".to_string());
        cx.bridge.shared.notify(ToastKind::Info, "Copied DGB/ prefix");
    }

    for (step, prefix, highlight) in [
        ("1", "Add VRC Contact Receiver to avatar", None),
        ("2", "Set Receiver Type to ", Some("Proximity")),
        ("3", "Set Parameter to ", Some("DGB/YourContactName")),
        ("4", "Upload avatar to VRChat", None),
    ] {
        step_row(ui, pal, step, prefix, highlight);
    }
}

fn step_row(ui: &mut Ui, pal: &Palette, step: &str, prefix: &str, highlight: Option<&str>) {
    let h = 21.0;
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, h), Sense::hover());

    let badge = Rect::from_min_size(rect.min, vec2(24.0, h));
    let p = ui.painter();
    p.rect_filled(badge, w::SQUARE, pal.well);
    p.rect_stroke(badge, w::SQUARE, Stroke::new(1.0, pal.hair), StrokeKind::Inside);
    w::text_in(ui, badge, step, font::tiny(), pal.contrast, Align2::CENTER_CENTER);

    let mut x = badge.right() + 10.0;
    let prefix_galley = p.layout_no_wrap(prefix.to_string(), font::small(), pal.dim);
    let pw = prefix_galley.size().x;
    let prefix_rect =
        Rect::from_min_max(Pos2::new(x, rect.top()), Pos2::new((x + pw).min(rect.right()), rect.bottom()));
    w::text_in(ui, prefix_rect, prefix, font::small(), pal.dim, Align2::LEFT_CENTER);
    x += pw;

    if let Some(tok) = highlight
        && x < rect.right()
    {
        let tok_rect = Rect::from_min_max(Pos2::new(x, rect.top()), rect.max);
        w::text_in(ui, tok_rect, tok, font::small(), pal.contrast, Align2::LEFT_CENTER);
    }
}
