use egui::{Align2, Rect, Sense, Stroke, StrokeKind, Ui, vec2};

use shocking_vrc_core::cli::{ChannelStatus, EngineMode};
use shocking_vrc_core::raw_to_hz;

use crate::app::{Ctx, short_mac};
use crate::bridge::Command;
use crate::model::Channel;
use crate::theme::widgets::{self as w, font, space};

pub fn show(ui: &mut Ui, cx: &mut Ctx) {
    overview(ui, cx);
    ui.add_space(space::SHEET_GAP);

    if cx.snap.devices.len() > 1 {
        devices(ui, cx);
        ui.add_space(space::SHEET_GAP);
    }

    let gap = space::SHEET_GAP;
    let col_w = (ui.available_width() - gap) * 0.5;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for ch in Channel::ALL {
            w::column(ui, col_w, |ui| channel_panel(ui, cx, ch));
        }
    });
}

fn overview(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    w::sheet(ui, pal, |ui| {
        let connected = cx.snap.active().map(|d| d.connected).unwrap_or(false);
        w::heading_with_suffix(
            ui,
            pal,
            "Overview",
            Some(if connected { "linked" } else { "waiting" }),
        );

        let cols = 4.0;
        let gap = 8.0;
        let cw = (ui.available_width() - gap * (cols - 1.0)) / cols;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;

            w::column(ui, cw, |ui| {
                let (label, color) = match cx.snap.active() {
                    Some(d) if d.connected => (d.name.clone(), pal.success),
                    Some(d) if d.reconnecting => ("Reconnecting…".into(), pal.warning),
                    Some(_) => ("Offline".into(), pal.error),
                    None => ("Searching…".into(), pal.dim),
                };
                w::stat_cell(ui, pal, "Coyote", &label, color);
            });

            w::column(ui, cw, |ui| {
                let battery = cx.snap.active().and_then(|d| d.battery);
                let (txt, color) = match battery {
                    Some(b) => (format!("{b}%"), pal.gradient(b as f32 / 100.0)),
                    None => ("—".to_string(), pal.dim),
                };
                w::stat_cell(ui, pal, "Battery", &txt, color);
            });

            w::column(ui, cw, |ui| {
                w::stat_cell(
                    ui,
                    pal,
                    "VRChat",
                    if cx.snap.vrchat_found {
                        "online"
                    } else {
                        "offline"
                    },
                    if cx.snap.vrchat_found {
                        pal.success
                    } else {
                        pal.error
                    },
                );
            });

            w::column(ui, cw, |ui| {
                let n = cx.snap.avatar_zones.len();
                w::stat_cell(
                    ui,
                    pal,
                    "Avatar zones",
                    &n.to_string(),
                    if n > 0 { pal.accent } else { pal.dim },
                );
            });
        });

        if !cx.snap.has_device() {
            ui.add_space(space::ITEM_GAP);
            w::dim(
                ui,
                pal,
                "No Coyote yet — the app keeps scanning in the background. \
                 Turn the device on and it will attach on its own.",
            );
        }
    });
}

fn devices(ui: &mut Ui, cx: &mut Ctx) {
    let pal = cx.pal;
    w::sheet(ui, pal, |ui| {
        w::heading_with_suffix(
            ui,
            pal,
            "Devices",
            Some(&format!("{} connected", cx.snap.devices.len())),
        );
        w::dim(
            ui,
            pal,
            match cx.snap.mode {
                EngineMode::Classic => "Each device keeps its own configuration. Click to switch.",
                EngineMode::Graph => "One graph drives every device. Click to see a device's output.",
            },
        );
        ui.add_space(space::ROW_GAP);

        for (i, dev) in cx.snap.devices.iter().enumerate() {
            let active = i == cx.snap.active_device;
            let (rect, resp) = w::row(ui, pal, active, 34.0);
            let inner = rect.shrink2(vec2(10.0, 0.0));

            let mut x = inner.left();
            let cell = |ui: &Ui, x: &mut f32, width: f32, text: &str, color, font| {
                w::text_in(
                    ui,
                    Rect::from_min_size(egui::pos2(*x, inner.top()), vec2(width, inner.height())),
                    text,
                    font,
                    color,
                    Align2::LEFT_CENTER,
                );
                *x += width;
            };

            cell(
                ui,
                &mut x,
                34.0,
                &format!("D{}", i + 1),
                if active { pal.accent } else { pal.dim },
                font::small(),
            );
            cell(
                ui,
                &mut x,
                160.0,
                &dev.name,
                pal.contrast,
                font::body(),
            );
            cell(
                ui,
                &mut x,
                90.0,
                &short_mac(&dev.mac),
                pal.dim,
                font::small(),
            );

            let bar_w = 90.0;
            let bar = Rect::from_min_size(
                egui::pos2(x, inner.center().y - 5.0),
                vec2(bar_w, 10.0),
            );
            ui.painter().rect_filled(bar, w::SQUARE, pal.well);
            if let Some(b) = dev.battery {
                let f = (b as f32 / 100.0).clamp(0.0, 1.0);
                ui.painter().rect_filled(
                    Rect::from_min_size(bar.min, vec2(bar.width() * f, bar.height())),
                    w::SQUARE,
                    pal.gradient(f),
                );
            }
            ui.painter().rect_stroke(
                bar,
                w::SQUARE,
                Stroke::new(1.0, pal.hair),
                StrokeKind::Inside,
            );
            x += bar_w + 8.0;
            cell(
                ui,
                &mut x,
                46.0,
                &dev.battery.map(|b| format!("{b}%")).unwrap_or("—".into()),
                pal.dim,
                font::small(),
            );

            let (label, color) = if dev.reconnecting {
                ("reconnecting", pal.warning)
            } else if dev.connected {
                ("online", pal.success)
            } else {
                ("offline", pal.error)
            };
            w::text_in(
                ui,
                Rect::from_min_max(egui::pos2(x, inner.top()), inner.max),
                label,
                font::small(),
                color,
                Align2::RIGHT_CENTER,
            );

            if resp.clicked() {
                cx.send(Command::SwitchDevice(i));
            }
        }
    });
}

fn channel_panel(ui: &mut Ui, cx: &mut Ctx, ch: Channel) {
    let pal = cx.pal;
    let st: &ChannelStatus = cx.snap.channel_status(ch);
    let limit_max = match cx.snap.mode {
        EngineMode::Classic => cx.snap.channel(ch).limits.max,
        EngineMode::Graph => cx
            .snap
            .active()
            .and_then(|d| cx.snap.graph.outputs.iter().find(|o| o.mac == d.mac))
            .map(|o| o.channels[if ch == Channel::A { 0 } else { 1 }].safe.limit_max)
            .unwrap_or(0),
    };

    w::sheet(ui, pal, |ui| {
        w::heading_with_suffix(
            ui,
            pal,
            &format!("Channel {}", ch.label()),
            Some(if st.strength > 0 { "live" } else { "idle" }),
        );

        let pct = st.raw_level.clamp(0.0, 1.0) * 100.0;
        ui.horizontal(|ui| {
            w::caption(ui, pal, "Input");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("{pct:.0}%"))
                        .font(font::body())
                        .color(if pct > 0.0 { pal.success } else { pal.dim }),
                );
            });
        });
        w::gauge(ui, pal, st.raw_level, pal.success, 10.0);
        ui.add_space(space::ROW_GAP);

        let ratio = if limit_max > 0 {
            (st.strength as f32 / limit_max as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        ui.horizontal(|ui| {
            w::caption(ui, pal, "Power");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("{} / {limit_max}", st.strength))
                        .font(font::body())
                        .color(if st.strength > 0 {
                            pal.contrast
                        } else {
                            pal.dim
                        }),
                );
            });
        });
        w::gauge(ui, pal, ratio, pal.gradient(1.0 - ratio), 10.0);
        ui.add_space(space::ITEM_GAP);

        w::caption(ui, pal, "Frequency");
        ui.add_space(2.0);
        let seg_w = (ui.available_width() - 6.0 * 3.0) / 4.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (i, raw) in st.frequency.iter().enumerate() {
                let (rect, _) = ui.allocate_exact_size(vec2(seg_w, 34.0), Sense::hover());
                w::dashed_rect(ui, rect, pal.hair);
                w::text_in(
                    ui,
                    Rect::from_min_size(rect.min + vec2(6.0, 3.0), vec2(rect.width(), 11.0)),
                    &format!("s{i}"),
                    font::tiny(),
                    pal.dim,
                    Align2::LEFT_CENTER,
                );
                w::text_in(
                    ui,
                    Rect::from_min_size(
                        rect.min + vec2(6.0, 15.0),
                        vec2(rect.width() - 12.0, 15.0),
                    ),
                    &format!("{:.0} Hz", raw_to_hz(*raw)),
                    font::body(),
                    if st.strength > 0 { pal.accent } else { pal.dim },
                    Align2::LEFT_CENTER,
                );
            }
        });
        ui.add_space(space::ITEM_GAP);

        w::caption(ui, pal, "Active zones");
        ui.add_space(2.0);
        if st.active_zones.is_empty() {
            w::empty_state(ui, pal, "none — waiting for contact");
        } else {
            for (id, level) in &st.active_zones {
                let (rect, _) = ui.allocate_exact_size(
                    vec2(ui.available_width(), 20.0),
                    Sense::hover(),
                );
                let f = level.clamp(0.0, 1.0);
                ui.painter().rect_filled(
                    Rect::from_min_size(rect.min, vec2(rect.width() * f, rect.height())),
                    w::SQUARE,
                    pal.faded(pal.accent, 0.85),
                );
                w::text_in(
                    ui,
                    rect.shrink2(vec2(6.0, 0.0)),
                    &id.to_string(),
                    font::small(),
                    pal.contrast,
                    Align2::LEFT_CENTER,
                );
                w::text_in(
                    ui,
                    rect.shrink2(vec2(6.0, 0.0)),
                    &format!("{:.0}%", level * 100.0),
                    font::small(),
                    pal.accent,
                    Align2::RIGHT_CENTER,
                );
            }
        }
    });
}
