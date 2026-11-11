use egui::Context;

use crate::app::Ctx;
use crate::bridge::Command;
use crate::theme::widgets::{self as w, font, space};

pub fn show(ctx: &Context, cx: &mut Ctx) {
    let state = cx.bridge.shared.update.read().map(|u| (*u).clone()).unwrap_or_default();

    let Some(release) = state.available.clone() else {
        return;
    };
    if state.dismissed && !state.downloading && state.pending.is_none() && state.error.is_none() {
        return;
    }

    let pal = cx.pal;
    let installing = state.pending.is_some();
    let title = if installing {
        "Installing update"
    } else if state.downloading {
        "Downloading update"
    } else if state.error.is_some() {
        "Download failed"
    } else {
        "Update available"
    };

    let dismissed = super::modal(ctx, pal, "update", title, 420.0, |ui| {
        ui.horizontal(|ui| {
            w::body(ui, pal, format!("v{}", shocking_vrc_core::VERSION));
            w::dim(ui, pal, "→");
            ui.label(
                egui::RichText::new(&release.tag)
                    .font(font::mono_big())
                    .color(pal.accent),
            );
        });
        ui.add_space(space::ROW_GAP);

        if installing {
            w::gauge(ui, pal, 1.0, pal.accent, 12.0);
            ui.add_space(4.0);
            w::dim(ui, pal, "Download complete. Restarting with the new version…");
            return;
        }

        if state.downloading {
            let progress = state.progress.unwrap_or(shocking_vrc_core::DownloadProgress {
                downloaded: 0,
                total: (release.asset_size > 0).then_some(release.asset_size),
            });
            w::gauge(ui, pal, progress.ratio() as f32, pal.accent, 12.0);
            ui.add_space(4.0);
            w::dim(ui, pal, progress.label());
            ui.add_space(4.0);
            w::dim(ui, pal, "The app restarts by itself once this finishes.");
            return;
        }

        if let Some(err) = &state.error {
            ui.label(egui::RichText::new(err).font(font::small()).color(pal.error));
            ui.add_space(4.0);
            w::dim(ui, pal, "Details are in the Log section.");
            ui.add_space(space::ITEM_GAP);
            ui.horizontal(|ui| {
                if w::cta(ui, pal, "Retry").clicked() {
                    cx.bridge.send(Command::StartUpdateDownload);
                }
                if w::button(ui, pal, "Later").clicked() {
                    cx.bridge.send(Command::DismissUpdate);
                }
            });
            return;
        }

        w::dim(
            ui,
            pal,
            format!(
                "{} · {:.1} MB. The new binary replaces this one and the app restarts.",
                release.asset_name,
                release.asset_size as f64 / (1024.0 * 1024.0)
            ),
        );
        ui.add_space(space::ITEM_GAP);
        ui.horizontal(|ui| {
            if w::cta(ui, pal, "Update now").clicked() {
                cx.bridge.send(Command::StartUpdateDownload);
            }
            if w::button(ui, pal, "Later").clicked() {
                cx.bridge.send(Command::DismissUpdate);
            }
            if w::button(ui, pal, "Skip this version")
                .on_hover_text("Do not offer this release again")
                .clicked()
            {
                cx.bridge.send(Command::SkipUpdateVersion);
            }
        });
    });

    if dismissed && !state.downloading && !installing {
        cx.bridge.send(Command::DismissUpdate);
    }
}
