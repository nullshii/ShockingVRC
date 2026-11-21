use egui::{Color32, Ui};

use shocking_vrc_core::graph::safety::{MergeMode, RemoteChannelPolicy};
use shocking_vrc_remote::link::OfferedRole;
use shocking_vrc_remote::protocol::ClientKind;
use shocking_vrc_remote::service::{ConnectionMode, SessionStatus, VdsState};
use shocking_vrc_remote::store::Lifetime;

use crate::app::Ctx;
use crate::bridge::{Bridge, Command, Snapshot};
use crate::theme::Palette;
use crate::theme::widgets::{self as w, font, space};

const LABEL_W: f32 = 132.0;
const TWO_COLUMN_MIN_W: f32 = 520.0;

type Sheet = fn(&mut Ui, &mut Ctx);

fn role_label(s: &SessionStatus) -> &'static str {
    match (s.i_control, s.i_receive) {
        (true, true) => "both ways",
        (true, false) => "you control them",
        (false, true) => "they control you",
        _ => "idle",
    }
}

fn session_line(s: &SessionStatus) -> String {
    let lat = s.latency_ms.map(|l| format!(" · {l} ms")).unwrap_or_default();
    format!("{} · {}{}", s.partner, s.path.label(), lat)
}

pub fn topbar_badge(ui: &mut Ui, pal: &Palette, snap: &Snapshot, bridge: &Bridge) -> bool {
    let st = &snap.remote.service;
    let mut open = false;
    if st.sessions.is_empty() {
        let (text, color) = match (&st.consent, &st.connecting) {
            (Some(c), _) => (format!("REMOTE · {} is asking", c.partner), pal.warning),
            (None, Some(n)) => (format!("REMOTE · connecting to {n}"), pal.dim),
            _ => return false,
        };
        open = w::tag(ui, pal, &text, color)
            .interact(egui::Sense::click())
            .on_hover_text("Open the Remote section")
            .clicked();
        return open;
    }
    for s in &st.sessions {
        let out = if s.i_receive {
            snap.remote
                .my_remote_output
                .iter()
                .fold([0u8; 2], |acc, (_, o)| [acc[0].max(o[0]), acc[1].max(o[1])])
        } else {
            s.partner_output
                .slots
                .iter()
                .fold([0u8; 2], |acc, (_, o)| [acc[0].max(o[0]), acc[1].max(o[1])])
        };
        let text = format!("REMOTE · {} · A {} B {}", session_line(s), out[0], out[1]);
        let fill = if s.i_receive && !s.live {
            pal.warning
        } else {
            pal.accent
        };
        if w::badge(ui, pal, &text, fill)
            .interact(egui::Sense::click())
            .on_hover_text(format!("{} — {}", role_label(s), s.fingerprint))
            .clicked()
        {
            open = true;
        }
        ui.add_space(6.0);
    }
    if w::danger_button(ui, pal, "STOP REMOTE", true)
        .on_hover_text("End every remote session now")
        .clicked()
    {
        bridge.send(Command::RemoteStop);
    }
    open
}

pub fn show(ui: &mut Ui, cx: &mut Ctx) {
    if !cx.snap.remote_enabled {
        w::empty_state(ui, cx.pal, "Partner remote is off — turn it on in Setup.");
        return;
    }
    let gap = space::SHEET_GAP;
    let col_w = (ui.available_width() - gap) * 0.5;
    let rows: [[Sheet; 2]; 3] = [[sessions, join], [invite, limits], [connection, session_log]];
    if col_w < TWO_COLUMN_MIN_W {
        let full = ui.available_width();
        for (i, sheet) in rows.iter().flatten().enumerate() {
            if i > 0 {
                ui.add_space(gap);
            }
            w::column(ui, full, |ui| sheet(ui, cx));
        }
        return;
    }
    for (i, [left, right]) in rows.iter().enumerate() {
        if i > 0 {
            ui.add_space(gap);
        }
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            w::column(ui, col_w, |ui| left(ui, cx));
            w::column(ui, col_w, |ui| right(ui, cx));
        });
    }
}

fn sessions(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let st = cx.snap.remote.service.clone();
    w::sheet(ui, pal, |ui| {
        w::heading(ui, pal, "Session");
        if st.sessions.is_empty() {
            match &st.connecting {
                Some(n) => w::body(ui, pal, format!("Connecting to {n}…")),
                None => w::empty_state(ui, pal, "No partner connected"),
            }
        }
        for s in &st.sessions {
            ui.horizontal(|ui| {
                let fill = if s.i_receive && !s.live {
                    pal.warning
                } else {
                    pal.accent
                };
                w::badge(ui, pal, &session_line(s), fill);
                ui.add_space(6.0);
                w::tag(ui, pal, role_label(s), pal.dim);
                if s.client == ClientKind::Web {
                    ui.add_space(4.0);
                    w::tag(ui, pal, "web panel", pal.dim);
                }
            });
            ui.add_space(4.0);
            let ends = s
                .ends_at_ms
                .map(|t| {
                    let left = t.saturating_sub(shocking_vrc_remote::store::now_unix_ms()) / 1000;
                    format!(" · ends in {}:{:02}", left / 60, left % 60)
                })
                .unwrap_or_default();
            w::dim(ui, pal, format!("fingerprint {} · {} s{ends}", s.fingerprint, s.secs));
            ui.add_space(4.0);
            if s.i_receive {
                w::caption(ui, pal, "Remote output on your device");
                let mine = &cx.snap.remote.my_remote_output;
                if mine.is_empty() {
                    w::dim(ui, pal, "no allowed device connected");
                }
                for (slot, out) in mine {
                    for (ch, v) in out.iter().enumerate() {
                        let cap = cx.snap.remote_settings.channels[ch].cap.max(1);
                        w::gauge_row(
                            ui,
                            pal,
                            &format!("Device {} · {}", slot + 1, ["A", "B"][ch]),
                            LABEL_W,
                            *v as f32 / cap as f32,
                            pal.accent,
                            &format!("{v} / {cap}"),
                            72.0,
                        );
                    }
                }
                if !s.live {
                    ui.label(
                        egui::RichText::new("no frames for over a second — remote output held at 0")
                            .font(font::small())
                            .color(pal.warning),
                    );
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    use shocking_vrc_remote::protocol::FeedbackKind;
                    if w::button(ui, pal, "▲ More").clicked() {
                        cx.bridge.send(Command::RemoteFeedback(FeedbackKind::More));
                    }
                    if w::button(ui, pal, "▼ Less").clicked() {
                        cx.bridge.send(Command::RemoteFeedback(FeedbackKind::Less));
                    }
                    w::dim(ui, pal, format!("tell {} what you want", s.partner));
                });
            }
            if s.i_control {
                w::caption(ui, pal, "Partner's limits and output");
                match &s.partner_policy {
                    Some(p) => {
                        for (ch, c) in p.channels.iter().enumerate() {
                            let out = s.partner_output.slots.iter().map(|(_, o)| o[ch]).max().unwrap_or(0);
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
                                72.0,
                            );
                        }
                        let names: Vec<String> = p
                            .slots
                            .iter()
                            .map(|s| {
                                if s.name.is_empty() {
                                    format!("Device {}", s.slot + 1)
                                } else {
                                    format!("{} ({})", s.name, s.slot + 1)
                                }
                            })
                            .collect();
                        w::dim(
                            ui,
                            pal,
                            if names.is_empty() {
                                "the partner allows no device".to_string()
                            } else {
                                format!("drivable: {}", names.join(", "))
                            },
                        );
                        w::dim(ui, pal, "Draw in the Graph → Remote tab; its Partner Device drives them.");
                    }
                    None => w::dim(ui, pal, "waiting for the partner's limits…"),
                }
            }
            ui.add_space(4.0);
            if w::button(ui, pal, "End this session").clicked() {
                cx.send(Command::RemoteStopSession(s.id));
            }
            ui.add_space(space::ITEM_GAP);
            w::dashed_hr(ui, pal.hair);
            ui.add_space(space::ITEM_GAP);
        }
        if let Some(e) = &st.last_error {
            ui.label(egui::RichText::new(e).font(font::small()).color(pal.error));
            ui.add_space(4.0);
        }
        let any = !st.sessions.is_empty();
        if w::danger_button(ui, pal, "STOP REMOTE", any)
            .on_hover_text("End every session; your device goes back to local control at once")
            .clicked()
        {
            cx.send(Command::RemoteStop);
        }
    });
}

fn join(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let st = cx.snap.remote.service.clone();
    w::sheet(ui, pal, |ui| {
        w::heading(ui, pal, "Join a partner");
        if let Some(link) = cx.bridge.shared.remote_ui().pending_link
            && cx.ui_state.remote_form.join_link.is_empty()
        {
            cx.ui_state.remote_form.join_link = link;
        }
        let form = &mut cx.ui_state.remote_form;
        let width = ui.available_width();
        w::text_field(ui, pal, &mut form.join_link, "paste a shockingvrc://2/… link", width);
        ui.add_space(4.0);
        let needs_pass = shocking_vrc_remote::link::needs_passphrase(&form.join_link).unwrap_or(false);
        ui.horizontal(|ui| {
            w::field_label(ui, pal, "Passphrase", 96.0, needs_pass);
            let field_w = (ui.available_width() - 110.0).max(80.0);
            w::text_field(
                ui,
                pal,
                &mut form.join_pass,
                if needs_pass {
                    "required for this link"
                } else {
                    "only if the link has one"
                },
                field_w,
            );
            let ready = !form.join_link.trim().is_empty() && (!needs_pass || !form.join_pass.is_empty());
            if w::button_enabled(ui, pal, "Connect", ready).clicked() {
                let link = std::mem::take(&mut form.join_link);
                let passphrase = std::mem::take(&mut form.join_pass);
                cx.bridge.send(Command::RemoteConnectLink { link, passphrase });
            }
        });
        ui.add_space(space::ITEM_GAP);
        w::caption(ui, pal, "Trusted partners");
        if st.partners.is_empty() {
            w::empty_state(ui, pal, "none yet — an indefinite invite creates one");
        }
        for p in &st.partners {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&p.name).font(font::body()).color(pal.contrast));
                ui.add_space(6.0);
                w::dim(ui, pal, format!("{} · {}", p.fingerprint, p.my_role.label().to_lowercase()));
            });
            ui.horizontal(|ui| {
                if p.can_dial && w::button(ui, pal, "Connect").clicked() {
                    cx.send(Command::RemoteConnectPartner(p.pk.clone()));
                }
                w::field_label(ui, pal, "Auto-accept", 96.0, false);
                if let Some(v) = w::toggle(ui, pal, p.auto_accept) {
                    cx.send(Command::RemoteAutoAccept(p.pk.clone(), v));
                }
                ui.add_space(6.0);
                if w::danger_button(ui, pal, "Revoke", true).clicked() {
                    cx.send(Command::RemoteRevokePartner(p.pk.clone()));
                }
            });
            ui.add_space(4.0);
        }
    });
}

fn trust_button(ui: &mut Ui, pal: &Palette, on: bool, what: &str) -> bool {
    w::toggle_button(ui, pal, "Trust this user", on)
        .on_hover_text(if on {
            format!("On: whoever uses this {what} connects without asking you to accept")
        } else {
            format!("Off: you accept or decline each time someone uses this {what}")
        })
        .clicked()
}

fn invite(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let st = cx.snap.remote.service.clone();
    let vds_online = matches!(st.vds, VdsState::Online);
    w::sheet(ui, pal, |ui| {
        w::heading(ui, pal, "Invite");
        let form = &mut cx.ui_state.remote_form;
        w::caption(ui, pal, "What they get");
        let roles: Vec<&str> = OfferedRole::ALL.iter().map(|r| r.label()).collect();
        let sel = OfferedRole::ALL.iter().position(|r| *r == form.role).unwrap_or(0);
        if let Some(i) = w::segmented(ui, pal, &roles, sel) {
            form.role = OfferedRole::ALL[i];
        }
        ui.add_space(space::ROW_GAP);
        w::caption(ui, pal, "Key lifetime");
        let lifetimes: Vec<&str> = Lifetime::ALL.iter().map(|l| l.label()).collect();
        let sel = Lifetime::ALL.iter().position(|l| *l == form.lifetime).unwrap_or(0);
        if let Some(i) = w::segmented(ui, pal, &lifetimes, sel) {
            form.lifetime = Lifetime::ALL[i];
        }
        ui.add_space(space::ROW_GAP);
        let pairing = form.lifetime == Lifetime::Indefinite;
        ui.horizontal(|ui| {
            w::field_label(ui, pal, "Passphrase", 96.0, pairing);
            let fw = ui.available_width();
            w::text_field(
                ui,
                pal,
                &mut form.passphrase,
                if pairing {
                    "required — share it separately"
                } else {
                    "optional — share it separately"
                },
                fw,
            );
        });
        ui.horizontal(|ui| {
            w::field_label(ui, pal, "Label", 96.0, false);
            let fw = ui.available_width();
            w::text_field(ui, pal, &mut form.label, "who it is for (only you see this)", fw);
        });
        ui.add_space(4.0);
        let ok = !pairing || form.passphrase.trim().chars().count() >= 6;
        if !ok {
            w::dim(ui, pal, "An indefinite pairing needs a passphrase of 6+ characters.");
        }
        ui.horizontal(|ui| {
            if w::button_enabled(ui, pal, "Create link", ok).clicked() {
                cx.bridge.send(Command::RemoteCreateInvite {
                    role: form.role,
                    lifetime: form.lifetime,
                    passphrase: std::mem::take(&mut form.passphrase),
                    label: std::mem::take(&mut form.label),
                    trusted: form.trusted,
                });
            }
            if trust_button(ui, pal, form.trusted, "link") {
                form.trusted = !form.trusted;
            }
        });
        let results = cx.bridge.shared.remote_ui();
        if let Some(link) = &results.link {
            ui.add_space(space::ITEM_GAP);
            w::caption(ui, pal, "New link — a fresh secret every time");
            let mut shown = link.clone();
            let width = ui.available_width();
            ui.add(
                egui::TextEdit::multiline(&mut shown)
                    .font(font::tiny())
                    .desired_width(width)
                    .desired_rows(3),
            );
            ui.horizontal(|ui| {
                if w::cta(ui, pal, "Copy").clicked() {
                    ui.ctx().copy_text(link.clone());
                }
                if w::button(ui, pal, "Done").clicked() {
                    cx.bridge.send(Command::RemoteDismissResult);
                }
            });
        }

        ui.add_space(space::ITEM_GAP);
        w::caption(ui, pal, "Web control code");
        ui.horizontal_wrapped(|ui| {
            let short = [Lifetime::Minutes10, Lifetime::Hour1, Lifetime::Hours24];
            let labels: Vec<&str> = short.iter().map(|l| l.label()).collect();
            let sel = short
                .iter()
                .position(|l| *l == cx.ui_state.remote_form.web_lifetime)
                .unwrap_or(1);
            if let Some(i) = w::segmented(ui, pal, &labels, sel) {
                cx.ui_state.remote_form.web_lifetime = short[i];
            }
            if w::button_enabled(ui, pal, "Create code", vds_online)
                .on_hover_text("Lets someone drive you from a browser at your rendezvous server")
                .clicked()
            {
                cx.send(Command::RemoteCreateWebCode {
                    lifetime: cx.ui_state.remote_form.web_lifetime,
                    trusted: cx.ui_state.remote_form.web_trusted,
                });
            }
            if trust_button(ui, pal, cx.ui_state.remote_form.web_trusted, "code") {
                cx.ui_state.remote_form.web_trusted = !cx.ui_state.remote_form.web_trusted;
            }
        });
        if !vds_online {
            w::dim(ui, pal, "Needs your rendezvous server to be online (Connection below).");
        }
        if let Some(code) = &results.code {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(code).font(font::mono_big()).color(pal.contrast));
                if w::button(ui, pal, "Copy").clicked() {
                    ui.ctx().copy_text(code.clone());
                }
            });
            w::dim(
                ui,
                pal,
                format!(
                    "Open {} in a browser and enter this code.",
                    cx.snap.remote_settings.config.vds_url
                ),
            );
        }

        ui.add_space(space::ITEM_GAP);
        w::caption(ui, pal, "Active invites");
        if st.invites.is_empty() {
            w::empty_state(ui, pal, "none");
        }
        for i in &st.invites {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if w::danger_button(ui, pal, "Revoke", true).clicked() {
                        cx.send(Command::RemoteRevokeInvite(i.id.clone()));
                    }
                    ui.add_space(8.0);
                    ui.with_layout(
                        egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
                        |ui| {
                            let label = if i.label.is_empty() { "invite" } else { i.label.as_str() };
                            ui.label(egui::RichText::new(label).font(font::body()).color(pal.contrast));
                            ui.add_space(6.0);
                            if i.trusted {
                                w::tag(ui, pal, "trusted", pal.warning)
                                    .on_hover_text("Connects without asking you to accept");
                                ui.add_space(6.0);
                            }
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
                        },
                    );
                });
            });
        }
    });
}

fn limits(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let mut s = cx.snap.remote_settings.clone();
    let mut changed = false;
    w::sheet(ui, pal, |ui| {
        w::heading(ui, pal, "Your limits");
        w::dim(
            ui,
            pal,
            "Only you set these. They apply on your side after every other safety step; \
             your panic, alarm and Stop always win.",
        );
        ui.add_space(space::ROW_GAP);
        w::caption(ui, pal, "Devices a partner may drive");
        ui.horizontal_wrapped(|ui| {
            let count = cx.snap.devices.len().max(1);
            for slot in 0..count.max(s.slots.iter().map(|x| *x as usize + 1).max().unwrap_or(1)) {
                let name = cx
                    .snap
                    .devices
                    .get(slot)
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| format!("Device {}", slot + 1));
                let on = s.allows(slot);
                let label = format!("{} {}", if on { "■" } else { "□" }, name);
                if w::segmented(ui, pal, &[label.as_str()], if on { 0 } else { 1 }).is_some() {
                    if on {
                        s.slots.retain(|x| *x as usize != slot);
                    } else {
                        s.slots.push(slot as u8);
                    }
                    changed = true;
                }
                ui.add_space(4.0);
            }
        });
        for ch in 0..2 {
            ui.add_space(space::ITEM_GAP);
            changed |= channel_limits(ui, pal, ["A", "B"][ch], &mut s.channels[ch]);
        }
        ui.add_space(space::ITEM_GAP);
        w::caption(ui, pal, "Avatar stop parameter");
        let form = &mut cx.ui_state.remote_form;
        let buf = form.stop_param.get_or_insert_with(|| s.stop_param.clone());
        let width = ui.available_width();
        let resp = w::text_field(ui, pal, buf, "/avatar/parameters/RemoteStop", width);
        if resp.lost_focus() && *buf != s.stop_param {
            s.stop_param = buf.trim().to_string();
            changed = true;
        }
        w::dim(ui, pal, "Turning this bool on in VRChat stops remote instantly (a safe word).");
    });
    if changed {
        cx.send(Command::SetRemoteSettings(Box::new(s)));
    }
}

fn channel_limits(ui: &mut Ui, pal: &Palette, name: &str, p: &mut RemoteChannelPolicy) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        w::field_label(ui, pal, &format!("Channel {name}"), LABEL_W, true);
        if let Some(v) = w::toggle(ui, pal, p.allowed) {
            p.allowed = v;
            changed = true;
        }
    });
    if !p.allowed {
        w::dim(ui, pal, "Remote is ignored on this channel.");
        return changed;
    }
    let row = |ui: &mut Ui, label: &str, v: f32, lo: f32, hi: f32, text: String| {
        w::slider_row(ui, pal, label, LABEL_W, w::Slider::new(v, lo, hi), &text, 72.0, None)
    };
    if let Some(v) = row(ui, "Power cap", p.cap as f32, 0.0, 200.0, format!("{}", p.cap)) {
        p.cap = v.round() as u8;
        changed = true;
    }
    if let Some(v) = row(
        ui,
        "Freq min",
        p.freq_bounds[0] as f32,
        10.0,
        240.0,
        format!("{}", p.freq_bounds[0]),
    ) {
        p.freq_bounds[0] = (v.round() as u8).min(p.freq_bounds[1]);
        changed = true;
    }
    if let Some(v) = row(
        ui,
        "Freq max",
        p.freq_bounds[1] as f32,
        10.0,
        240.0,
        format!("{}", p.freq_bounds[1]),
    ) {
        p.freq_bounds[1] = (v.round() as u8).max(p.freq_bounds[0]);
        changed = true;
    }
    if let Some(v) = row(
        ui,
        "Intensity min",
        p.int_bounds[0] as f32,
        0.0,
        100.0,
        format!("{} %", p.int_bounds[0]),
    ) {
        p.int_bounds[0] = (v.round() as u8).min(p.int_bounds[1]);
        changed = true;
    }
    if let Some(v) = row(
        ui,
        "Intensity max",
        p.int_bounds[1] as f32,
        0.0,
        100.0,
        format!("{} %", p.int_bounds[1]),
    ) {
        p.int_bounds[1] = (v.round() as u8).max(p.int_bounds[0]);
        changed = true;
    }
    let ramp_text = if p.ramp_per_s <= 0.0 {
        "off".to_string()
    } else {
        format!("{:.0}/s", p.ramp_per_s)
    };
    if let Some(v) = row(ui, "Rise limit", p.ramp_per_s, 0.0, 200.0, ramp_text) {
        p.ramp_per_s = v.round();
        changed = true;
    }
    ui.horizontal(|ui| {
        w::field_label(ui, pal, "Merge with local", LABEL_W, false);
        let labels: Vec<&str> = MergeMode::ALL.iter().map(|m| m.label()).collect();
        let sel = MergeMode::ALL.iter().position(|m| *m == p.merge).unwrap_or(2);
        if let Some(i) = w::segmented(ui, pal, &labels, sel) {
            p.merge = MergeMode::ALL[i];
            changed = true;
        }
    });
    changed
}

fn connection(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    let st = cx.snap.remote.service.clone();
    let current = cx.snap.remote_settings.clone();
    w::sheet(ui, pal, |ui| {
        w::heading(ui, pal, "Connection");
        let form = &mut cx.ui_state.remote_form;
        let cfg = form.connection.get_or_insert_with(|| current.config.clone());
        let labels: Vec<&str> = ConnectionMode::ALL.iter().map(|m| m.label()).collect();
        let sel = ConnectionMode::ALL.iter().position(|m| *m == cfg.mode).unwrap_or(0);
        if let Some(i) = w::segmented(ui, pal, &labels, sel) {
            cfg.mode = ConnectionMode::ALL[i];
        }
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
        ui.add_space(space::ROW_GAP);
        let fw = (ui.available_width() - 104.0).max(80.0);
        ui.horizontal(|ui| {
            w::field_label(ui, pal, "Your name", 96.0, false);
            w::text_field(ui, pal, &mut cfg.name, "shown to your partner", fw);
        });
        ui.horizontal(|ui| {
            w::field_label(ui, pal, "Server", 96.0, false);
            w::text_field(ui, pal, &mut cfg.vds_url, "https://your-vds.example (optional)", fw);
        });
        ui.horizontal(|ui| {
            w::field_label(ui, pal, "Server pin", 96.0, false);
            w::text_field(ui, pal, &mut cfg.vds_pin, "only for an IP-only server", fw);
        });
        ui.horizontal(|ui| {
            w::field_label(ui, pal, "LAN port", 96.0, false);
            let mut port = if cfg.lan_port == 0 {
                String::new()
            } else {
                cfg.lan_port.to_string()
            };
            if w::text_field(ui, pal, &mut port, "random", 90.0).changed() {
                cfg.lan_port = port.trim().parse().unwrap_or(0);
            }
            w::field_label(ui, pal, "Public", 56.0, false);
            let rest = ui.available_width();
            w::text_field(ui, pal, &mut cfg.public_addr, "host:port (optional)", rest);
        });
        if let Some(v) = w::slider_row(
            ui,
            pal,
            "Jitter buffer",
            96.0,
            w::Slider::new(cfg.jitter_ms as f32, 0.0, 300.0),
            &format!("{} ms", cfg.jitter_ms),
            72.0,
            None,
        ) {
            cfg.jitter_ms = v.round() as u32;
        }
        ui.add_space(4.0);
        let dirty = *cfg != current.config;
        ui.horizontal(|ui| {
            if w::button_enabled(ui, pal, "Apply", dirty).clicked() {
                let mut s = current.clone();
                s.config = cfg.clone();
                cx.bridge.send(Command::SetRemoteSettings(Box::new(s)));
            }
            if w::button_enabled(ui, pal, "Revert", dirty).clicked() {
                *cfg = current.config.clone();
            }
        });
        ui.add_space(space::ITEM_GAP);
        let line = |ui: &mut Ui, label: &str, text: String, color: Color32| {
            ui.horizontal(|ui| {
                w::field_label(ui, pal, label, 96.0, false);
                ui.label(egui::RichText::new(text).font(font::small()).color(color));
            });
        };
        line(ui, "Fingerprint", st.fingerprint.clone(), pal.contrast);
        line(
            ui,
            "LAN",
            st.listening.clone().unwrap_or_else(|| "not listening".into()),
            if st.listening.is_some() { pal.success } else { pal.dim },
        );
        let (vds, color) = match &st.vds {
            VdsState::Off => ("off".to_string(), pal.dim),
            VdsState::Connecting => ("connecting…".to_string(), pal.warning),
            VdsState::Online => ("online".to_string(), pal.success),
            VdsState::Error(e) => (e.clone(), pal.error),
        };
        line(ui, "Server", vds, color);
        ui.add_space(space::ITEM_GAP);
        ui.horizontal(|ui| {
            if w::button(ui, pal, "Open shockingvrc:// links with this app")
                .on_hover_text("Adds a per-user link handler (Windows registry, HKCU)")
                .clicked()
            {
                cx.send(Command::RemoteRegisterScheme);
            }
        });
    });
}

fn session_log(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    w::sheet(ui, pal, |ui| {
        w::heading(ui, pal, "Session log");
        ui.horizontal(|ui| {
            if w::button(ui, pal, "Export").clicked() {
                cx.send(Command::RemoteExportLog);
            }
            w::dim(ui, pal, shocking_vrc_app::remote_runtime::REMOTE_LOG_FILE);
        });
        ui.add_space(4.0);
        let log = &cx.snap.remote.service.log;
        if log.is_empty() {
            w::empty_state(ui, pal, "nothing yet");
        }
        egui::ScrollArea::vertical()
            .id_salt("remote_log")
            .max_height(220.0)
            .show(ui, |ui| {
                for e in log.iter().take(80) {
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
