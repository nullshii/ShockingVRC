use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, vec2};

use shocking_vrc_app::app_state::DeviceSlotInfo;
use shocking_vrc_app::remote_runtime::MAX_REMOTE_SLOTS;
use shocking_vrc_core::cli::EngineMode;
use shocking_vrc_core::graph::safety::{MergeMode, RemoteChannelPolicy};
use shocking_vrc_remote::link::OfferedRole;
use shocking_vrc_remote::protocol::ClientKind;
use shocking_vrc_remote::service::{ConnectionMode, RemoteStatus, SessionStatus, VdsState};
use shocking_vrc_remote::store::Lifetime;

use crate::app::{Ctx, GraphTab, InviteMode, RemoteTab};
use crate::bridge::{Bridge, Command, Snapshot, ToastKind};
use crate::model::Section;
use crate::theme::Palette;
use crate::theme::widgets::{self as w, font, space};

const LABEL_W: f32 = 112.0;
const TWO_COLUMN_MIN_W: f32 = 440.0;
const FOUR_STAT_MIN_W: f32 = 640.0;

fn role_label(s: &SessionStatus) -> &'static str {
    match (s.i_control, s.i_receive) {
        (true, true) => "both ways",
        (true, false) => "you control them",
        (false, true) => "they control you",
        _ => "idle",
    }
}

fn short_fp(fp: &str) -> &str {
    if fp.len() > 9 { &fp[..9] } else { fp }
}

fn session_out(snap: &Snapshot, s: &SessionStatus) -> [u8; 2] {
    if s.i_receive {
        snap.remote
            .my_remote_output
            .iter()
            .fold([0u8; 2], |acc, (_, o)| [acc[0].max(o[0]), acc[1].max(o[1])])
    } else {
        s.partner_output
            .slots
            .iter()
            .fold([0u8; 2], |acc, (_, o)| [acc[0].max(o[0]), acc[1].max(o[1])])
    }
}

pub fn has_topbar_badge(snap: &Snapshot) -> bool {
    let st = &snap.remote.service;
    !st.sessions.is_empty() || st.consent.is_some() || st.connecting.is_some()
}

pub fn topbar_accent(pal: &Palette, snap: &Snapshot) -> Color32 {
    let st = &snap.remote.service;
    if st.sessions.iter().any(|s| s.i_receive && !s.live) || st.consent.is_some() {
        pal.warning
    } else if !st.sessions.is_empty() {
        pal.accent
    } else {
        pal.dim
    }
}

pub fn topbar_badge(ui: &mut Ui, pal: &Palette, snap: &Snapshot, bridge: &Bridge, on_remote: bool) -> bool {
    let st = &snap.remote.service;
    let mut open = false;

    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if !st.sessions.is_empty()
                && w::small_danger_button(ui, pal, "STOP REMOTE")
                    .on_hover_text("End every remote session now")
                    .clicked()
            {
                bridge.send(Command::RemoteStop);
            }
            if !on_remote
                && w::small_button(ui, pal, "Remote ->")
                    .on_hover_text("Open the Remote section")
                    .clicked()
            {
                open = true;
            }

            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true), |ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 4.0);
                if st.sessions.is_empty() {
                    match (&st.consent, &st.connecting) {
                        (Some(c), _) => {
                            if w::badge(ui, pal, "ASKING", pal.warning)
                                .interact(Sense::click())
                                .on_hover_text("Open Remote to accept or decline")
                                .clicked()
                            {
                                open = true;
                            }
                            ui.label(egui::RichText::new(&c.partner).font(font::small()).color(pal.contrast));
                            ui.label(
                                egui::RichText::new("wants to connect")
                                    .font(font::small())
                                    .color(pal.dim),
                            );
                        }
                        (None, Some(n)) => {
                            if w::tag(ui, pal, "CONNECTING", pal.dim)
                                .interact(Sense::click())
                                .clicked()
                            {
                                open = true;
                            }
                            ui.label(egui::RichText::new(n).font(font::small()).color(pal.contrast));
                        }
                        _ => {}
                    }
                    return;
                }

                for (idx, s) in st.sessions.iter().enumerate() {
                    if idx > 0 {
                        ui.label(egui::RichText::new("|").font(font::small()).color(pal.hair));
                    }
                    let stalled = s.i_receive && !s.live;
                    let (pill, fill) = if stalled {
                        ("STALLED", pal.warning)
                    } else {
                        ("LIVE", pal.accent)
                    };
                    if w::badge(ui, pal, pill, fill)
                        .interact(Sense::click())
                        .on_hover_text(format!("{} — {}", role_label(s), s.fingerprint))
                        .clicked()
                    {
                        open = true;
                    }
                    let name_resp = ui
                        .add(
                            egui::Label::new(egui::RichText::new(&s.partner).font(font::small()).color(pal.contrast))
                                .sense(Sense::click()),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text("Open the Remote section");
                    if name_resp.clicked() {
                        open = true;
                    }
                    let lat = s.latency_ms.map(|l| format!(" · {l} ms")).unwrap_or_default();
                    ui.label(
                        egui::RichText::new(format!("{} · {}{lat}", role_label(s), s.path.label()))
                            .font(font::small())
                            .color(pal.dim),
                    );
                    let out = session_out(snap, s);
                    let active_out = out[0] > 0 || out[1] > 0;
                    ui.label(
                        egui::RichText::new(format!("A {}  B {}", out[0], out[1]))
                            .font(font::small())
                            .color(if active_out { pal.accent } else { pal.dim }),
                    );
                }
            });
        });
    });

    open
}

pub fn show(ui: &mut Ui, cx: &mut Ctx) {
    if !cx.snap.remote_enabled {
        w::sheet(ui, cx.pal, |ui| {
            w::heading_with_suffix(ui, cx.pal, "Partner remote", Some("disabled"));
            w::empty_state(ui, cx.pal, "Partner remote is off — enable it in Setup to pair with a partner.");
            ui.add_space(4.0);
            if w::button(ui, cx.pal, "Open Setup ->").clicked() {
                cx.ui_state.section = Section::Setup;
            }
        });
        return;
    }

    overview_strip(ui, cx);

    let st = &cx.snap.remote.service;
    if !st.sessions.is_empty() || st.connecting.is_some() || st.last_error.is_some() {
        ui.add_space(space::SHEET_GAP);
        live_session_card(ui, cx);
    }

    ui.add_space(space::SHEET_GAP);
    tab_bar(ui, cx);
    ui.add_space(space::ITEM_GAP);

    match cx.ui_state.remote_form.tab {
        RemoteTab::Connect => tab_connect(ui, cx),
        RemoteTab::Limits => tab_limits(ui, cx),
        RemoteTab::Network => tab_network(ui, cx),
    }
}

fn overview_strip(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let st = cx.snap.remote.service.clone();

    let suffix = if !st.sessions.is_empty() {
        if st.sessions.iter().any(|s| s.i_receive && !s.live) {
            "stalled"
        } else {
            "live"
        }
    } else if st.connecting.is_some() {
        "connecting"
    } else if matches!(st.vds, VdsState::Online) || st.listening.is_some() {
        "ready"
    } else {
        "offline"
    };

    w::sheet(ui, pal, |ui| {
        w::heading_with_suffix(ui, pal, "Overview", Some(suffix));
        let avail = ui.available_width();
        let gap = 8.0;
        if avail >= FOUR_STAT_MIN_W {
            let cw = (avail - gap * 3.0) / 4.0;
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                w::column(ui, cw, |ui| stat_session(ui, cx, &st));
                w::column(ui, cw, |ui| stat_server(ui, cx, &st));
                w::column(ui, cw, |ui| stat_lan(ui, cx, &st));
                w::column(ui, cw, |ui| stat_fingerprint(ui, cx, &st));
            });
        } else {
            let cw = (avail - gap) * 0.5;
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                w::column(ui, cw, |ui| stat_session(ui, cx, &st));
                w::column(ui, cw, |ui| stat_server(ui, cx, &st));
            });
            ui.add_space(gap);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                w::column(ui, cw, |ui| stat_lan(ui, cx, &st));
                w::column(ui, cw, |ui| stat_fingerprint(ui, cx, &st));
            });
        }
    });
}

fn stat_session(ui: &mut Ui, cx: &Ctx, st: &RemoteStatus) {
    let pal = cx.pal;
    let (value, color, meta): (String, Color32, Option<(String, Color32)>) = if st.sessions.len() == 1 {
        let s = &st.sessions[0];
        let lat = s.latency_ms.map(|l| format!(" · {l} ms")).unwrap_or_default();
        let c = if s.i_receive && !s.live {
            pal.warning
        } else {
            pal.accent
        };
        (s.partner.clone(), c, Some((format!("{}{lat}", s.path.label()), pal.dim)))
    } else if st.sessions.len() > 1 {
        (
            format!("{} live", st.sessions.len()),
            pal.accent,
            Some(("active".into(), pal.dim)),
        )
    } else if let Some(target) = &st.connecting {
        ("connecting…".into(), pal.warning, Some((target.clone(), pal.dim)))
    } else {
        let m = (!st.partners.is_empty()).then(|| (format!("{} trusted", st.partners.len()), pal.dim));
        ("idle".into(), pal.dim, m)
    };
    let meta_ref = meta.as_ref().map(|(t, c)| (t.as_str(), *c));
    w::stat_cell_with(ui, pal, "Session", &value, color, meta_ref, false);
}

fn stat_server(ui: &mut Ui, cx: &mut Ctx, st: &RemoteStatus) {
    let pal = cx.pal;
    let (value, color, meta, hover): (&str, Color32, Option<(&str, Color32)>, String) = match &st.vds {
        VdsState::Online => (
            "online",
            pal.success,
            Some(("VDS", pal.dim)),
            format!("Connected to {}", cx.snap.remote_settings.config.vds_url),
        ),
        VdsState::Connecting => (
            "connecting…",
            pal.warning,
            Some(("VDS", pal.dim)),
            "Connecting to your rendezvous server".into(),
        ),
        VdsState::Error(e) => ("error", pal.error, Some(("fix ->", pal.error)), e.clone()),
        VdsState::Off => (
            "off",
            pal.dim,
            Some(("set up ->", pal.dim)),
            "No rendezvous server configured — click to open Network".into(),
        ),
    };
    let resp = w::stat_cell_with(ui, pal, "Server", value, color, meta, true).on_hover_text(hover);
    if resp.clicked() {
        cx.ui_state.remote_form.tab = RemoteTab::Network;
    }
}

fn stat_lan(ui: &mut Ui, cx: &mut Ctx, st: &RemoteStatus) {
    let pal = cx.pal;
    if let Some(addr) = &st.listening {
        let port = addr
            .rsplit_once(':')
            .map(|(_, p)| format!(":{p}"))
            .unwrap_or_else(|| addr.clone());
        let resp = w::stat_cell_with(ui, pal, "LAN", &port, pal.success, Some(("copy", pal.dim)), true)
            .on_hover_text(format!("Listening on {addr} — click to copy"));
        if resp.clicked() {
            ui.ctx().copy_text(addr.clone());
            cx.bridge.shared.notify(ToastKind::Info, format!("Copied {addr}"));
        }
    } else {
        let resp = w::stat_cell_with(ui, pal, "LAN", "off", pal.dim, Some(("setup ->", pal.dim)), true)
            .on_hover_text("LAN listener is off — click to open Network");
        if resp.clicked() {
            cx.ui_state.remote_form.tab = RemoteTab::Network;
        }
    }
}

fn stat_fingerprint(ui: &mut Ui, cx: &Ctx, st: &RemoteStatus) {
    let pal = cx.pal;
    let fp = st.fingerprint.as_str();
    let display = if fp.is_empty() { "—" } else { short_fp(fp) };
    let clickable = !fp.is_empty();
    let resp = w::stat_cell_with(
        ui,
        pal,
        "Fingerprint",
        display,
        if clickable { pal.contrast } else { pal.dim },
        clickable.then_some(("copy", pal.dim)),
        clickable,
    );
    if clickable
        && resp
            .on_hover_text(format!("{fp} — click to copy your identity fingerprint"))
            .clicked()
    {
        ui.ctx().copy_text(fp.to_string());
        cx.bridge
            .shared
            .notify(ToastKind::Info, format!("Copied fingerprint {fp}"));
    }
}

fn live_session_card(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let st = cx.snap.remote.service.clone();
    let stalled = st.sessions.iter().any(|s| s.i_receive && !s.live);
    let border = if stalled || st.connecting.is_some() {
        pal.warning
    } else if !st.sessions.is_empty() {
        pal.accent
    } else {
        pal.error
    };
    let suffix = if !st.sessions.is_empty() {
        format!("{} active", st.sessions.len())
    } else if st.connecting.is_some() {
        "connecting".into()
    } else {
        "error".into()
    };

    w::sheet_tinted(ui, pal, border, |ui| {
        w::heading_with_suffix(ui, pal, "Live session", Some(&suffix));

        if let Some(e) = &st.last_error {
            accent_callout(ui, pal, pal.error, |ui| {
                ui.label(egui::RichText::new(e).font(font::small()).color(pal.error));
            });
            if !st.sessions.is_empty() || st.connecting.is_some() {
                ui.add_space(space::ITEM_GAP);
            }
        }

        if st.sessions.is_empty() {
            if let Some(n) = &st.connecting {
                ui.horizontal(|ui| {
                    w::tag(ui, pal, "CONNECTING", pal.warning);
                    ui.add_space(4.0);
                    w::body(ui, pal, format!("Reaching {n}…"));
                });
            }
            return;
        }

        let multi = st.sessions.len() > 1;
        for (idx, s) in st.sessions.iter().enumerate() {
            if idx > 0 {
                ui.add_space(space::ITEM_GAP);
                w::dashed_hr(ui, pal.hair);
                ui.add_space(space::ITEM_GAP);
            }

            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if multi {
                        if w::small_button(ui, pal, "End session").clicked() {
                            cx.send(Command::RemoteStopSession(s.id));
                        }
                    } else if w::danger_button(ui, pal, "STOP REMOTE", true)
                        .on_hover_text("End session; your device goes back to local control at once")
                        .clicked()
                    {
                        cx.send(Command::RemoteStop);
                    }

                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true), |ui| {
                        ui.spacing_mut().item_spacing = vec2(8.0, 4.0);
                        ui.label(
                            egui::RichText::new(&s.partner)
                                .font(font::mono_big())
                                .color(pal.contrast),
                        );
                        let lat = s.latency_ms.map(|l| format!(" · {l} ms")).unwrap_or_default();
                        let fill = if s.i_receive && !s.live {
                            pal.warning
                        } else {
                            pal.accent
                        };
                        w::badge(ui, pal, &format!("{}{lat}", s.path.label()), fill);
                        w::tag(ui, pal, role_label(s), pal.dim);
                        if s.client == ClientKind::Web {
                            w::tag(ui, pal, "web panel", pal.dim);
                        }
                    });
                });
            });

            let ends = s
                .ends_at_ms
                .map(|t| {
                    let left = t.saturating_sub(shocking_vrc_remote::store::now_unix_ms()) / 1000;
                    format!(" · ends in {}:{:02}", left / 60, left % 60)
                })
                .unwrap_or_default();
            w::dim(ui, pal, format!("fp {} · {} s{ends}", s.fingerprint, s.secs));
            ui.add_space(space::ITEM_GAP);

            let avail = ui.available_width();
            if s.i_receive && s.i_control && avail >= TWO_COLUMN_MIN_W * 2.0 {
                let gap = space::SHEET_GAP;
                let cw = (avail - gap) * 0.5;
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    w::column(ui, cw, |ui| session_receiving(ui, cx, s));
                    w::column(ui, cw, |ui| session_controlling(ui, cx, s));
                });
            } else {
                if s.i_receive {
                    session_receiving(ui, cx, s);
                }
                if s.i_receive && s.i_control {
                    ui.add_space(space::ITEM_GAP);
                }
                if s.i_control {
                    session_controlling(ui, cx, s);
                }
            }
        }

        if multi {
            ui.add_space(space::ITEM_GAP);
            w::dashed_hr(ui, pal.hair);
            ui.add_space(space::ITEM_GAP);
            if w::danger_button(ui, pal, "STOP ALL REMOTE", true)
                .on_hover_text("End every session at once")
                .clicked()
            {
                cx.send(Command::RemoteStop);
            }
        }
    });
}

fn session_receiving(ui: &mut Ui, cx: &Ctx, s: &SessionStatus) {
    let pal = cx.pal;
    w::caption(ui, pal, "Incoming on your device");
    ui.add_space(2.0);
    let mine = &cx.snap.remote.my_remote_output;
    if mine.is_empty() {
        w::dim(ui, pal, "No allowed device connected (check Limits tab).");
    }
    for (slot, out) in mine {
        for (ch, v) in out.iter().enumerate() {
            let cap = cx.snap.remote_settings.slot_channels(*slot)[ch].cap.max(1);
            w::gauge_row(
                ui,
                pal,
                &format!("Device {} · {}", slot + 1, ["A", "B"][ch]),
                LABEL_W,
                *v as f32 / cap as f32,
                pal.accent,
                &format!("{v} / {cap}"),
                68.0,
            );
        }
    }
    if !s.live {
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new("No frames for over a second — output held at 0.")
                .font(font::small())
                .color(pal.warning),
        );
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        use shocking_vrc_remote::protocol::FeedbackKind;
        if w::small_button(ui, pal, "▲ More").clicked() {
            cx.bridge.send(Command::RemoteFeedback(FeedbackKind::More));
        }
        if w::small_button(ui, pal, "▼ Less").clicked() {
            cx.bridge.send(Command::RemoteFeedback(FeedbackKind::Less));
        }
        w::dim(ui, pal, format!("nudge {}", s.partner));
    });
}

fn session_controlling(ui: &mut Ui, cx: &mut Ctx, s: &SessionStatus) {
    let pal = cx.pal;
    w::caption(ui, pal, "Partner's limits & output");
    ui.add_space(2.0);
    match &s.partner_policy {
        Some(p) => {
            if p.slots.len() <= 1 {
                let ch_policies = p
                    .slots
                    .first()
                    .map(|slot| p.slot_channels(slot.slot))
                    .unwrap_or(p.channels);
                for (ch, c) in ch_policies.iter().enumerate() {
                    let out = match p.slots.first() {
                        Some(slot) => s
                            .partner_output
                            .slots
                            .iter()
                            .find(|(id, _)| *id == slot.slot)
                            .map(|(_, o)| o[ch])
                            .unwrap_or(0),
                        None => s.partner_output.slots.iter().map(|(_, o)| o[ch]).max().unwrap_or(0),
                    };
                    let cap = if c.allowed { c.cap } else { 0 };
                    w::gauge_row(
                        ui,
                        pal,
                        &format!("Channel {}", ["A", "B"][ch]),
                        LABEL_W,
                        out as f32 / cap.max(1) as f32,
                        pal.success,
                        &if c.allowed {
                            format!("{out} / {cap}")
                        } else {
                            "locked".into()
                        },
                        68.0,
                    );
                }
            } else {
                for slot_info in &p.slots {
                    let label = if slot_info.name.is_empty() {
                        format!("Device {}", slot_info.slot + 1)
                    } else {
                        format!("{} ({})", slot_info.name, slot_info.slot + 1)
                    };
                    for (ch, c) in p.slot_channels(slot_info.slot).iter().enumerate() {
                        let out = s
                            .partner_output
                            .slots
                            .iter()
                            .find(|(id, _)| *id == slot_info.slot)
                            .map(|(_, o)| o[ch])
                            .unwrap_or(0);
                        let cap = if c.allowed { c.cap } else { 0 };
                        w::gauge_row(
                            ui,
                            pal,
                            &format!("{label} · {}", ["A", "B"][ch]),
                            LABEL_W,
                            out as f32 / cap.max(1) as f32,
                            pal.success,
                            &if c.allowed {
                                format!("{out} / {cap}")
                            } else {
                                "locked".into()
                            },
                            68.0,
                        );
                    }
                }
            }
            ui.add_space(6.0);
            let names: Vec<String> = p
                .slots
                .iter()
                .map(|slot| {
                    if slot.name.is_empty() {
                        format!("Device {}", slot.slot + 1)
                    } else {
                        format!("{} ({})", slot.name, slot.slot + 1)
                    }
                })
                .collect();
            ui.horizontal_wrapped(|ui| {
                if cx.ui_state.mode == EngineMode::Graph
                    && w::small_button(ui, pal, "Open Remote Graph ->")
                        .on_hover_text("Switch to Graph -> Remote tab to drive their device")
                        .clicked()
                {
                    cx.ui_state.graph_tab = GraphTab::Remote;
                    cx.ui_state.section = Section::Graph;
                }
                w::dim(
                    ui,
                    pal,
                    if names.is_empty() {
                        "Partner allows no device.".to_string()
                    } else if cx.ui_state.mode == EngineMode::Graph {
                        format!("drivable: {}", names.join(", "))
                    } else {
                        format!("drivable: {} · switch to Graph mode to drive", names.join(", "))
                    },
                );
            });
        }
        None => w::dim(ui, pal, "Waiting for partner's limits…"),
    }
}

fn tab_bar(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let st = &cx.snap.remote.service;
    let enabled_slots = cx.snap.remote_settings.slots.len();
    let vds_err = matches!(st.vds, VdsState::Error(_));
    let dirty_net = cx
        .ui_state
        .remote_form
        .connection
        .as_ref()
        .is_some_and(|c| *c != cx.snap.remote_settings.config);

    let connect_meta = {
        let n = st.partners.len() + st.invites.len();
        (n > 0).then(|| (n.to_string(), pal.dim))
    };
    let limits_meta = if enabled_slots == 0 {
        Some(("locked".to_string(), pal.warning))
    } else {
        Some((format!("{enabled_slots} dev"), pal.dim))
    };
    let net_meta = if dirty_net {
        Some(("unsaved".to_string(), pal.warning))
    } else if vds_err {
        Some(("error".to_string(), pal.error))
    } else {
        None
    };

    let tabs = [
        (RemoteTab::Connect, "Connect", connect_meta),
        (RemoteTab::Limits, "Limits", limits_meta),
        (RemoteTab::Network, "Network", net_meta),
    ];

    let cur = cx.ui_state.remote_form.tab;
    let start_x = ui.cursor().left();
    let full_w = ui.available_width();
    let resp_row = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (tab, title, meta) in &tabs {
            let active = cur == *tab;
            if remote_tab_button(ui, pal, title, meta.as_ref().map(|(t, c)| (t.as_str(), *c)), active) {
                cx.ui_state.remote_form.tab = *tab;
            }
        }
    });
    let y = resp_row.response.rect.bottom();
    w::dashed_line(
        ui.painter(),
        Pos2::new(start_x, y),
        Pos2::new(start_x + full_w, y),
        Stroke::new(1.0, pal.hair),
        4.0,
        4.0,
    );
}

fn remote_tab_button(ui: &mut Ui, pal: &Palette, title: &str, meta: Option<(&str, Color32)>, active: bool) -> bool {
    let label = title.to_uppercase();
    let f_title = font::small();
    let f_meta = font::tiny();
    let tw = ui
        .painter()
        .layout_no_wrap(label.clone(), f_title.clone(), pal.contrast)
        .size()
        .x;
    let mw = meta
        .map(|(m, _)| {
            ui.painter()
                .layout_no_wrap(m.to_string(), f_meta.clone(), pal.dim)
                .size()
                .x
                + 8.0
        })
        .unwrap_or(0.0);
    let size = vec2(tw + mw + 24.0, 30.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hovered = resp.hovered();
    let p = ui.painter();

    if active {
        p.rect_filled(rect, w::SQUARE, pal.sheet);
        p.rect_stroke(rect, w::SQUARE, Stroke::new(1.0, pal.hair), StrokeKind::Inside);
        p.hline(rect.x_range(), rect.bottom() - 1.0, Stroke::new(2.0, pal.accent));
    } else if hovered {
        p.rect_filled(rect, w::SQUARE, pal.well);
    }

    let fg = if active || hovered { pal.accent } else { pal.dim };
    let text_x = rect.left() + 12.0;
    p.text(Pos2::new(text_x, rect.center().y), Align2::LEFT_CENTER, &label, f_title, fg);
    if let Some((m, mc)) = meta {
        p.text(
            Pos2::new(text_x + tw + 8.0, rect.center().y),
            Align2::LEFT_CENTER,
            m,
            f_meta,
            mc,
        );
    }
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn tab_connect(ui: &mut Ui, cx: &mut Ctx) {
    let gap = space::SHEET_GAP;
    let col_w = (ui.available_width() - gap) * 0.5;
    if col_w < TWO_COLUMN_MIN_W {
        join_sheet(ui, cx);
        ui.add_space(gap);
        invite_sheet(ui, cx);
        return;
    }
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        w::column(ui, col_w, |ui| join_sheet(ui, cx));
        w::column(ui, col_w, |ui| invite_sheet(ui, cx));
    });
}

fn join_sheet(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let st = cx.snap.remote.service.clone();

    if let Some(link) = cx.bridge.shared.remote_ui().pending_link
        && cx.ui_state.remote_form.join_link.is_empty()
    {
        cx.ui_state.remote_form.join_link = link;
    }

    w::sheet(ui, pal, |ui| {
        let form = &mut cx.ui_state.remote_form;
        let pass_req = if form.join_link.trim().is_empty() {
            None
        } else {
            Some(shocking_vrc_remote::link::needs_passphrase(form.join_link.trim()))
        };
        let needs_pass = matches!(pass_req, Some(Ok(true)));
        let link_valid = matches!(pass_req, Some(Ok(_)));

        let heading_suffix = match &pass_req {
            None => None,
            Some(Ok(true)) => Some("encrypted link"),
            Some(Ok(false)) => Some("open link"),
            Some(Err(_)) => Some("check link"),
        };
        w::heading_with_suffix(ui, pal, "Join a partner", heading_suffix);

        w::caption(ui, pal, "Invite link");
        ui.add_space(2.0);
        let full_w = ui.available_width();
        w::text_field(ui, pal, &mut form.join_link, "paste a shockingvrc://2/… link", full_w);

        if needs_pass {
            ui.add_space(space::ROW_GAP);
            let has_pass = !form.join_pass.trim().is_empty();
            let bar_c = if has_pass { pal.success } else { pal.warning };
            accent_callout(ui, pal, bar_c, |ui| {
                ui.horizontal(|ui| {
                    w::caption(ui, pal, "Passphrase");
                    w::tag(
                        ui,
                        pal,
                        if has_pass { "ready" } else { "required" },
                        if has_pass { pal.success } else { pal.warning },
                    );
                });
                ui.add_space(2.0);
                let fw = ui.available_width();
                w::text_field(ui, pal, &mut form.join_pass, "enter the passphrase shared with this link", fw);
            });
        }

        ui.add_space(space::ITEM_GAP);
        let has_link = !form.join_link.trim().is_empty();
        let ready = has_link && (!needs_pass || !form.join_pass.trim().is_empty());
        ui.horizontal(|ui| {
            let clicked = if ready && link_valid {
                w::cta(ui, pal, "Connect ->").clicked()
            } else {
                w::button_enabled(ui, pal, "Connect ->", ready).clicked()
            };
            if clicked {
                let link = std::mem::take(&mut form.join_link);
                let passphrase = std::mem::take(&mut form.join_pass);
                cx.bridge.send(Command::RemoteConnectLink { link, passphrase });
            }
            if has_link && w::small_button(ui, pal, "Clear").clicked() {
                form.join_link.clear();
                form.join_pass.clear();
            }
            let status_hint = match &pass_req {
                None => Some(("Paste an invite link from your partner", pal.dim)),
                Some(Ok(true)) if form.join_pass.trim().is_empty() => {
                    Some(("Passphrase required to unlock link", pal.warning))
                }
                Some(Ok(true)) => Some(("Ready to connect", pal.success)),
                Some(Ok(false)) => Some(("No passphrase needed", pal.success)),
                Some(Err(e)) => Some((
                    match e {
                        shocking_vrc_remote::link::LinkError::Scheme => "Expected shockingvrc://2/…",
                        _ => "Link looks incomplete",
                    },
                    pal.warning,
                )),
            };
            if let Some((txt, col)) = status_hint {
                ui.label(egui::RichText::new(txt).font(font::small()).color(col));
            }
        });

        ui.add_space(space::SHEET_GAP);
        w::dashed_hr(ui, pal.hair);
        ui.add_space(space::ITEM_GAP);

        ui.horizontal(|ui| {
            w::caption(ui, pal, "Trusted partners");
            if !st.partners.is_empty() {
                w::dim(ui, pal, format!("({})", st.partners.len()));
            }
        });
        ui.add_space(4.0);

        if st.partners.is_empty() {
            w::empty_state(ui, pal, "None yet — pair with an Indefinite invite or choose Accept & trust.");
            return;
        }

        for p in &st.partners {
            item_box(ui, pal, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        if w::small_danger_button(ui, pal, "Revoke").clicked() {
                            cx.send(Command::RemoteRevokePartner(p.pk.clone()));
                        }
                        if p.can_dial && w::small_button(ui, pal, "Connect").clicked() {
                            cx.send(Command::RemoteConnectPartner(p.pk.clone()));
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true), |ui| {
                            ui.label(egui::RichText::new(&p.name).font(font::body()).color(pal.contrast));
                            w::tag(ui, pal, &p.my_role.label().to_lowercase(), pal.dim);
                        });
                    });
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&p.fingerprint).font(font::tiny()).color(pal.dim));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(v) = w::toggle(ui, pal, p.auto_accept) {
                            cx.send(Command::RemoteAutoAccept(p.pk.clone(), v));
                        }
                        ui.label(egui::RichText::new("Auto-accept").font(font::small()).color(pal.dim));
                    });
                });
            });
            ui.add_space(6.0);
        }
    });
}

fn invite_sheet(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let st = cx.snap.remote.service.clone();
    let vds_online = matches!(st.vds, VdsState::Online);
    let results = cx.bridge.shared.remote_ui();

    w::sheet(ui, pal, |ui| {
        let mode = cx.ui_state.remote_form.invite_mode;
        let suffix = match mode {
            InviteMode::Link => "app link",
            InviteMode::WebCode => "browser code",
        };
        w::heading_with_suffix(ui, pal, "Invite", Some(suffix));

        let mode_idx = match mode {
            InviteMode::Link => 0,
            InviteMode::WebCode => 1,
        };
        if let Some(i) = w::segmented(ui, pal, &["App link", "Web control code"], mode_idx) {
            cx.ui_state.remote_form.invite_mode = if i == 0 { InviteMode::Link } else { InviteMode::WebCode };
        }
        ui.add_space(space::ITEM_GAP);

        match cx.ui_state.remote_form.invite_mode {
            InviteMode::Link => invite_link_body(ui, cx, &results.link),
            InviteMode::WebCode => invite_web_body(ui, cx, vds_online, &results.code),
        }

        ui.add_space(space::SHEET_GAP);
        w::dashed_hr(ui, pal.hair);
        ui.add_space(space::ITEM_GAP);

        ui.horizontal(|ui| {
            w::caption(ui, pal, "Active invites");
            if !st.invites.is_empty() {
                w::dim(ui, pal, format!("({})", st.invites.len()));
            }
        });
        ui.add_space(4.0);

        if st.invites.is_empty() {
            w::empty_state(ui, pal, "No active invites");
            return;
        }

        for i in &st.invites {
            item_box(ui, pal, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if w::small_danger_button(ui, pal, "Revoke").clicked() {
                            cx.send(Command::RemoteRevokeInvite(i.id.clone()));
                        }
                        ui.add_space(6.0);
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true), |ui| {
                            let label = if i.label.is_empty() { "Invite" } else { i.label.as_str() };
                            ui.label(egui::RichText::new(label).font(font::body()).color(pal.contrast));
                            if i.trusted {
                                w::tag(ui, pal, "auto-accept", pal.warning)
                                    .on_hover_text("Connects without asking you to accept");
                            }
                            if i.web_code {
                                w::tag(ui, pal, "web code", pal.dim);
                            }
                        });
                    });
                });
                let uses = i.uses_left.map(|u| format!(" · {u} use(s)")).unwrap_or_default();
                w::dim(
                    ui,
                    pal,
                    format!(
                        "{} · {}{uses}",
                        i.lifetime.label().to_lowercase(),
                        i.role.label().to_lowercase()
                    ),
                );
            });
            ui.add_space(6.0);
        }
    });
}

fn invite_link_body(ui: &mut Ui, cx: &mut Ctx, created_link: &Option<String>) {
    let pal = cx.pal;
    let form = &mut cx.ui_state.remote_form;

    w::caption(ui, pal, "Access");
    ui.add_space(2.0);
    let roles: Vec<&str> = OfferedRole::ALL.iter().map(|r| r.label()).collect();
    let sel = OfferedRole::ALL.iter().position(|r| *r == form.role).unwrap_or(0);
    if let Some(i) = w::segmented(ui, pal, &roles, sel) {
        form.role = OfferedRole::ALL[i];
    }

    ui.add_space(space::ITEM_GAP);
    w::caption(ui, pal, "Key lifetime");
    ui.add_space(2.0);
    let lifetimes: Vec<&str> = Lifetime::ALL.iter().map(|l| l.label()).collect();
    let sel = Lifetime::ALL.iter().position(|l| *l == form.lifetime).unwrap_or(0);
    if let Some(i) = w::segmented(ui, pal, &lifetimes, sel) {
        form.lifetime = Lifetime::ALL[i];
    }

    ui.add_space(space::ITEM_GAP);
    let pairing = form.lifetime == Lifetime::Indefinite;
    let pass_len = form.passphrase.trim().chars().count();
    let ok = !pairing || pass_len >= 6;

    if pairing {
        let bar_c = if ok { pal.success } else { pal.warning };
        accent_callout(ui, pal, bar_c, |ui| {
            ui.horizontal(|ui| {
                w::caption(ui, pal, "Passphrase");
                let (tag_txt, tag_col) = if ok {
                    ("ready".to_string(), pal.success)
                } else {
                    (format!("required · {pass_len}/6+"), pal.warning)
                };
                w::tag(ui, pal, &tag_txt, tag_col);
            });
            ui.add_space(2.0);
            let fw = ui.available_width();
            w::text_field(
                ui,
                pal,
                &mut form.passphrase,
                "6+ characters — share separately with your partner",
                fw,
            );
        });
    } else {
        ui.horizontal(|ui| {
            w::field_label(ui, pal, "Passphrase", 96.0, false);
            let fw = ui.available_width();
            w::text_field(ui, pal, &mut form.passphrase, "optional — share separately", fw);
        });
    }

    ui.add_space(space::ROW_GAP);
    ui.horizontal(|ui| {
        w::field_label(ui, pal, "Label", 96.0, false);
        let fw = ui.available_width();
        w::text_field(ui, pal, &mut form.label, "private note (only you see this)", fw);
    });

    ui.add_space(space::ITEM_GAP);
    control_row(ui, pal, "Auto-accept", 96.0, "Connect without asking you to accept", |ui| {
        if let Some(v) = w::toggle(ui, pal, form.trusted) {
            form.trusted = v;
        }
    });

    ui.add_space(space::ITEM_GAP);
    let create_clicked = if ok {
        w::cta(ui, pal, "Create link").clicked()
    } else {
        w::button_enabled(ui, pal, "Create link", false).clicked()
    };
    if create_clicked {
        cx.bridge.send(Command::RemoteCreateInvite {
            role: form.role,
            lifetime: form.lifetime,
            passphrase: std::mem::take(&mut form.passphrase),
            label: std::mem::take(&mut form.label),
            trusted: form.trusted,
        });
    }

    if let Some(link) = created_link {
        ui.add_space(space::ITEM_GAP);
        accent_callout(ui, pal, pal.accent, |ui| {
            w::caption(ui, pal, "New link — fresh secret");
            ui.add_space(2.0);
            let mut shown = link.clone();
            let width = ui.available_width();
            ui.add(
                egui::TextEdit::multiline(&mut shown)
                    .font(font::tiny())
                    .desired_width(width)
                    .desired_rows(3),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if w::cta(ui, pal, "Copy link").clicked() {
                    ui.ctx().copy_text(link.clone());
                    cx.bridge.shared.notify(ToastKind::Info, "Copied invite link");
                }
                if w::small_button(ui, pal, "Dismiss").clicked() {
                    cx.bridge.send(Command::RemoteDismissResult);
                }
            });
        });
    }
}

fn invite_web_body(ui: &mut Ui, cx: &mut Ctx, vds_online: bool, created_code: &Option<String>) {
    let pal = cx.pal;
    w::dim(
        ui,
        pal,
        "Generate a 16-character code so a partner can drive you from any browser at your server.",
    );
    ui.add_space(space::ITEM_GAP);

    w::caption(ui, pal, "Code lifetime");
    ui.add_space(2.0);
    let short = [Lifetime::Minutes10, Lifetime::Hour1, Lifetime::Hours24];
    let labels: Vec<&str> = short.iter().map(|l| l.label()).collect();
    let sel = short
        .iter()
        .position(|l| *l == cx.ui_state.remote_form.web_lifetime)
        .unwrap_or(1);
    if let Some(i) = w::segmented(ui, pal, &labels, sel) {
        cx.ui_state.remote_form.web_lifetime = short[i];
    }

    ui.add_space(space::ITEM_GAP);
    control_row(ui, pal, "Auto-accept", 96.0, "Connect without asking you to accept", |ui| {
        if let Some(v) = w::toggle(ui, pal, cx.ui_state.remote_form.web_trusted) {
            cx.ui_state.remote_form.web_trusted = v;
        }
    });

    ui.add_space(space::ITEM_GAP);
    ui.horizontal(|ui| {
        let clicked = if vds_online {
            w::cta(ui, pal, "Create code").clicked()
        } else {
            w::button_enabled(ui, pal, "Create code", false).clicked()
        };
        if clicked {
            cx.send(Command::RemoteCreateWebCode {
                lifetime: cx.ui_state.remote_form.web_lifetime,
                trusted: cx.ui_state.remote_form.web_trusted,
            });
        }
        if !vds_online {
            if w::small_button(ui, pal, "Configure server ->").clicked() {
                cx.ui_state.remote_form.tab = RemoteTab::Network;
            }
            w::dim(ui, pal, "Needs rendezvous server online");
        }
    });

    if let Some(code) = created_code {
        ui.add_space(space::ITEM_GAP);
        accent_callout(ui, pal, pal.accent, |ui| {
            w::caption(ui, pal, "Active web control code");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(code).font(font::mono_big()).color(pal.contrast));
                ui.add_space(8.0);
                if w::cta(ui, pal, "Copy").clicked() {
                    ui.ctx().copy_text(code.clone());
                    cx.bridge.shared.notify(ToastKind::Info, "Copied control code");
                }
            });
            ui.add_space(4.0);
            let url = cx.snap.remote_settings.config.vds_url.clone();
            w::dim(ui, pal, format!("Open {url} in a browser and enter this code."));
        });
    }
}

fn slot_display_name(devices: &[DeviceSlotInfo], slot: usize) -> String {
    devices
        .get(slot)
        .map(|d| d.name.trim())
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("Device {}", slot + 1))
}

fn tab_limits(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let mut s = cx.snap.remote_settings.clone();
    let mut changed = false;

    let max_slot = s.slots.iter().map(|x| *x as usize + 1).max().unwrap_or(1);
    let count = cx.snap.devices.len().max(max_slot).clamp(1, MAX_REMOTE_SLOTS);
    let mut enabled_slots: Vec<u8> = s
        .slots
        .iter()
        .copied()
        .filter(|&slot| (slot as usize) < MAX_REMOTE_SLOTS)
        .collect();
    enabled_slots.sort_unstable();
    enabled_slots.dedup();

    w::sheet(ui, pal, |ui| {
        let suffix = format!("{}/{count} enabled", enabled_slots.len());
        w::heading_with_suffix(ui, pal, "Allowed devices", Some(&suffix));
        w::dim(
            ui,
            pal,
            "Only you set these limits. They apply locally after every safety step; panic, alarm and STOP REMOTE always win.",
        );
        ui.add_space(space::ITEM_GAP);

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for slot in 0..count {
                let name = slot_display_name(&cx.snap.devices, slot);
                let on = s.allows(slot);
                let label = format!("{} {}", if on { "■" } else { "□" }, name);
                if w::segmented(ui, pal, &[label.as_str()], if on { 0 } else { 1 }).is_some() {
                    if on {
                        s.slots.retain(|x| *x as usize != slot);
                    } else {
                        s.slots.push(slot as u8);
                        s.slots.sort_unstable();
                        s.slots.dedup();
                    }
                    changed = true;
                }
            }
        });

        if enabled_slots.len() > 1 {
            if !enabled_slots.contains(&cx.ui_state.remote_form.limit_slot) {
                cx.ui_state.remote_form.limit_slot = enabled_slots[0];
            }
            let active_slot = cx.ui_state.remote_form.limit_slot;
            ui.add_space(space::ITEM_GAP);
            w::caption(ui, pal, "Editing limits for");
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 4.0);
                for &dev_slot in &enabled_slots {
                    let name = slot_display_name(&cx.snap.devices, dev_slot as usize);
                    let tab_label = if name.starts_with("Device ") {
                        name
                    } else {
                        format!("Device {} · {}", dev_slot + 1, name)
                    };
                    let selected = dev_slot == active_slot;
                    if w::segmented(ui, pal, &[tab_label.as_str()], if selected { 0 } else { 1 }).is_some() {
                        cx.ui_state.remote_form.limit_slot = dev_slot;
                    }
                }
            });
        }
    });

    ui.add_space(space::SHEET_GAP);

    if enabled_slots.is_empty() {
        w::sheet(ui, pal, |ui| {
            w::heading_with_suffix(ui, pal, "Channel limits", Some("locked"));
            w::empty_state(
                ui,
                pal,
                "No device enabled for remote control — enable a device above to configure Channel A & B limits.",
            );
        });
    } else {
        let slot = if enabled_slots.len() == 1 {
            enabled_slots[0] as usize
        } else {
            cx.ui_state.remote_form.limit_slot as usize
        };
        let gap = space::SHEET_GAP;
        let col_w = (ui.available_width() - gap) * 0.5;
        ui.push_id(slot, |ui| {
            if col_w < TWO_COLUMN_MIN_W {
                for ch in 0..2 {
                    if ch > 0 {
                        ui.add_space(gap);
                    }
                    changed |= channel_limits_sheet(ui, pal, ["A", "B"][ch], &mut s.slot_channels_mut(slot)[ch]);
                }
            } else {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for ch in 0..2 {
                        w::column(ui, col_w, |ui| {
                            changed |=
                                channel_limits_sheet(ui, pal, ["A", "B"][ch], &mut s.slot_channels_mut(slot)[ch]);
                        });
                    }
                });
            }
        });
    }

    ui.add_space(space::SHEET_GAP);

    w::sheet(ui, pal, |ui| {
        w::heading_with_suffix(ui, pal, "Safe word", Some("VRChat OSC"));
        let form = &mut cx.ui_state.remote_form;
        let buf = form.stop_param.get_or_insert_with(|| s.stop_param.clone());
        ui.horizontal(|ui| {
            w::field_label(ui, pal, "Stop parameter", 128.0, false);
            let fw = ui.available_width();
            let resp = w::text_field(ui, pal, buf, "/avatar/parameters/RemoteStop", fw);
            if resp.lost_focus() && *buf != s.stop_param {
                s.stop_param = buf.trim().to_string();
                changed = true;
            }
        });
        ui.add_space(2.0);
        w::dim(
            ui,
            pal,
            "Turning this avatar bool on in VRChat stops every remote session instantly.",
        );
    });

    if changed {
        cx.send(Command::SetRemoteSettings(Box::new(s)));
    }
}

fn channel_limits_sheet(ui: &mut Ui, pal: &Palette, name: &str, p: &mut RemoteChannelPolicy) -> bool {
    let mut changed = false;
    let suffix = if p.allowed {
        format!("cap {}", p.cap)
    } else {
        "locked".into()
    };

    w::sheet(ui, pal, |ui| {
        w::heading_with_suffix(ui, pal, &format!("Channel {name}"), Some(&suffix));
        control_row(
            ui,
            pal,
            "Allow remote",
            LABEL_W,
            "Let a connected partner drive this channel",
            |ui| {
                if let Some(v) = w::toggle(ui, pal, p.allowed) {
                    p.allowed = v;
                    changed = true;
                }
            },
        );
        if !p.allowed {
            ui.add_space(4.0);
            w::empty_state(ui, pal, "Remote output is blocked on this channel.");
            return;
        }
        ui.add_space(space::ITEM_GAP);
        let danger_cap = p.cap > 80;
        let mut cap_slider = w::Slider::new(p.cap as f32, 0.0, 200.0);
        if danger_cap {
            cap_slider = cap_slider.fill(pal.error);
        }
        if let Some(v) = w::slider_row(
            ui,
            pal,
            "Power cap",
            LABEL_W,
            cap_slider,
            &format!("{}", p.cap),
            64.0,
            Some(if danger_cap { pal.error } else { pal.contrast }),
        ) {
            p.cap = v.round() as u8;
            changed = true;
        }

        let ramp_text = if p.ramp_per_s <= 0.0 {
            "off".to_string()
        } else {
            format!("{:.0}/s", p.ramp_per_s)
        };
        if let Some(v) = w::slider_row(
            ui,
            pal,
            "Rise limit",
            LABEL_W,
            w::Slider::new(p.ramp_per_s, 0.0, 200.0),
            &ramp_text,
            64.0,
            None,
        ) {
            p.ramp_per_s = v.round();
            changed = true;
        }

        ui.add_space(space::ROW_GAP);
        ui.horizontal(|ui| {
            w::field_label(ui, pal, "Merge mode", LABEL_W, false);
            let labels: Vec<&str> = MergeMode::ALL.iter().map(|m| m.label()).collect();
            let sel = MergeMode::ALL.iter().position(|m| *m == p.merge).unwrap_or(2);
            if let Some(i) = w::segmented(ui, pal, &labels, sel) {
                p.merge = MergeMode::ALL[i];
                changed = true;
            }
        });
    });
    changed
}

fn tab_network(ui: &mut Ui, cx: &mut Ctx) {
    let gap = space::SHEET_GAP;
    let col_w = (ui.available_width() - gap) * 0.5;
    if col_w < TWO_COLUMN_MIN_W {
        connection_sheet(ui, cx);
        ui.add_space(gap);
        session_log_sheet(ui, cx);
        return;
    }
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        w::column(ui, col_w, |ui| connection_sheet(ui, cx));
        w::column(ui, col_w, |ui| session_log_sheet(ui, cx));
    });
}

fn connection_sheet(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let st = cx.snap.remote.service.clone();
    let current = cx.snap.remote_settings.clone();
    let form = &mut cx.ui_state.remote_form;
    let cfg = form.connection.get_or_insert_with(|| current.config.clone());
    let dirty = *cfg != current.config;

    let suffix = if dirty {
        "unsaved"
    } else {
        match st.vds {
            VdsState::Online => "online",
            VdsState::Connecting => "connecting",
            VdsState::Error(_) => "error",
            VdsState::Off => "local",
        }
    };

    w::sheet(ui, pal, |ui| {
        w::heading_with_suffix(ui, pal, "Connection", Some(suffix));

        w::caption(ui, pal, "Transport mode");
        ui.add_space(2.0);
        let labels: Vec<&str> = ConnectionMode::ALL.iter().map(|m| m.label()).collect();
        let sel = ConnectionMode::ALL.iter().position(|m| *m == cfg.mode).unwrap_or(0);
        if let Some(i) = w::segmented(ui, pal, &labels, sel) {
            cfg.mode = ConnectionMode::ALL[i];
        }
        ui.add_space(2.0);
        w::dim(
            ui,
            pal,
            match cfg.mode {
                ConnectionMode::Automatic => "Race every path and keep the first that authenticates.",
                ConnectionMode::Internet => "WebRTC through your rendezvous server; TURN relay as fallback.",
                ConnectionMode::LanOnly => "Host candidates only, found by mDNS or the link. Nothing leaves the LAN.",
                ConnectionMode::DirectHybrid => "Direct LAN/public first, then ICE, then the TURN relay.",
            },
        );

        ui.add_space(space::ITEM_GAP);
        let fw = (ui.available_width() - LABEL_W - 8.0).max(80.0);
        ui.horizontal(|ui| {
            w::field_label(ui, pal, "Your name", LABEL_W, false);
            w::text_field(ui, pal, &mut cfg.name, "shown to your partner", fw);
        });
        ui.add_space(space::ROW_GAP);
        ui.horizontal(|ui| {
            w::field_label(ui, pal, "Server URL", LABEL_W, false);
            w::text_field(ui, pal, &mut cfg.vds_url, "https://your-vds.example (optional)", fw);
        });
        ui.add_space(space::ROW_GAP);
        ui.horizontal(|ui| {
            w::field_label(ui, pal, "Server pin", LABEL_W, false);
            w::text_field(ui, pal, &mut cfg.vds_pin, "only for an IP-only server", fw);
        });
        ui.add_space(space::ROW_GAP);
        ui.horizontal(|ui| {
            w::field_label(ui, pal, "LAN port", LABEL_W, false);
            let mut port = if cfg.lan_port == 0 {
                String::new()
            } else {
                cfg.lan_port.to_string()
            };
            if w::text_field(ui, pal, &mut port, "random", 84.0).changed() {
                cfg.lan_port = port.trim().parse().unwrap_or(0);
            }
            w::field_label(ui, pal, "Public", 52.0, false);
            let rest = ui.available_width();
            w::text_field(ui, pal, &mut cfg.public_addr, "host:port (optional)", rest);
        });

        ui.add_space(space::ROW_GAP);
        if let Some(v) = w::slider_row(
            ui,
            pal,
            "Jitter buffer",
            LABEL_W,
            w::Slider::new(cfg.jitter_ms as f32, 0.0, 300.0),
            &format!("{} ms", cfg.jitter_ms),
            64.0,
            None,
        ) {
            cfg.jitter_ms = v.round() as u32;
        }

        ui.add_space(space::ITEM_GAP);
        ui.horizontal(|ui| {
            let apply_clicked = if dirty {
                w::cta(ui, pal, "Apply").clicked()
            } else {
                w::button_enabled(ui, pal, "Apply", false).clicked()
            };
            if apply_clicked {
                let mut s = current.clone();
                s.config = cfg.clone();
                cx.bridge.send(Command::SetRemoteSettings(Box::new(s)));
            }
            if w::button_enabled(ui, pal, "Revert", dirty).clicked() {
                *cfg = current.config.clone();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if w::small_button(ui, pal, "Register shockingvrc://")
                    .on_hover_text("Adds a per-user Windows link handler (HKCU)")
                    .clicked()
                {
                    cx.bridge.send(Command::RemoteRegisterScheme);
                }
            });
        });

        if let VdsState::Error(e) = &st.vds {
            ui.add_space(space::ITEM_GAP);
            accent_callout(ui, pal, pal.error, |ui| {
                ui.label(
                    egui::RichText::new(format!("Server error: {e}"))
                        .font(font::small())
                        .color(pal.error),
                );
            });
        }
    });
}

fn session_log_sheet(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let log = &cx.snap.remote.service.log;
    let suffix = format!("{} events", log.len());
    w::sheet(ui, pal, |ui| {
        w::heading_with_suffix(ui, pal, "Session log", Some(&suffix));
        ui.horizontal(|ui| {
            if w::small_button(ui, pal, "Export").clicked() {
                cx.send(Command::RemoteExportLog);
            }
            w::dim(ui, pal, shocking_vrc_app::remote_runtime::REMOTE_LOG_FILE);
        });
        ui.add_space(6.0);
        if log.is_empty() {
            w::empty_state(ui, pal, "No remote events yet");
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("remote_log")
            .max_height(360.0)
            .show(ui, |ui| {
                for e in log.iter().take(120) {
                    let secs = e.at % 86_400;
                    let t = format!("{:02}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(t).font(font::tiny()).color(pal.dim));
                        ui.add(
                            egui::Label::new(egui::RichText::new(&e.text).font(font::small()).color(pal.contrast))
                                .wrap(),
                        );
                    });
                }
            });
    });
}

fn control_row(ui: &mut Ui, pal: &Palette, label: &str, label_w: f32, hint: &str, control: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        w::field_label(ui, pal, label, label_w, false);
        control(ui);
        ui.add_space(6.0);
        ui.label(egui::RichText::new(hint).font(font::small()).color(pal.dim));
    });
}

fn item_box(ui: &mut Ui, pal: &Palette, add: impl FnOnce(&mut Ui)) {
    let resp = egui::Frame::NONE
        .fill(pal.well)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_min_width((ui.available_width()).max(1.0));
            add(ui);
        });
    ui.painter()
        .rect_stroke(resp.response.rect, w::SQUARE, Stroke::new(1.0, pal.hair), StrokeKind::Inside);
}

fn accent_callout(ui: &mut Ui, pal: &Palette, color: Color32, add: impl FnOnce(&mut Ui)) {
    let resp = egui::Frame::NONE
        .fill(pal.faded(color, 0.92))
        .inner_margin(egui::Margin {
            left: 12,
            right: 10,
            top: 8,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.set_min_width((ui.available_width()).max(1.0));
            add(ui);
        });
    let r = resp.response.rect;
    ui.painter()
        .rect_filled(Rect::from_min_size(r.min, vec2(3.0, r.height())), w::SQUARE, color);
    ui.painter()
        .rect_stroke(r, w::SQUARE, Stroke::new(1.0, pal.faded(color, 0.55)), StrokeKind::Inside);
}
