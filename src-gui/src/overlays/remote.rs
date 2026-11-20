use egui::Context;

use shocking_vrc_remote::protocol::ClientKind;
use shocking_vrc_remote::service::ConsentAnswer;

use crate::app::Ctx;
use crate::bridge::Command;
use crate::theme::widgets::{self as w, font, space};

pub fn show(ctx: &Context, cx: &mut Ctx) {
    consent(ctx, cx);
    scheme_prompt(ctx, cx);
}

fn consent(ctx: &Context, cx: &mut Ctx) {
    let Some(req) = cx.snap.remote.service.consent.clone() else {
        return;
    };
    let pal = cx.pal;
    let dismissed = super::modal(ctx, pal, "remote_consent", "Partner wants to connect", 420.0, |ui| {
        ui.label(
            egui::RichText::new(&req.partner)
                .font(font::mono_big())
                .color(pal.contrast),
        );
        ui.add_space(2.0);
        let who = match (req.client, req.fingerprint.is_empty()) {
            (ClientKind::Web, _) => "from the web control panel".to_string(),
            (_, true) => "no identity".to_string(),
            (_, false) => format!("fingerprint {}", req.fingerprint),
        };
        w::dim(ui, pal, who);
        if req.known {
            w::dim(ui, pal, "A trusted partner you paired with before.");
        }
        ui.add_space(space::ITEM_GAP);
        w::body(ui, pal, format!("They get: {}", their_side(req.role)));
        w::dim(
            ui,
            pal,
            "Your limits apply. STOP REMOTE, your panic and your alarm always override them.",
        );
        ui.add_space(space::ITEM_GAP);
        ui.horizontal(|ui| {
            if w::cta(ui, pal, "Accept").clicked() {
                cx.bridge.send(Command::RemoteConsent(ConsentAnswer::AcceptOnce));
            }
            if !req.fingerprint.is_empty() && w::button(ui, pal, "Accept & trust").clicked() {
                cx.bridge.send(Command::RemoteConsent(ConsentAnswer::AcceptAndTrust));
            }
            if w::danger_button(ui, pal, "Decline", true).clicked() {
                cx.bridge.send(Command::RemoteConsent(ConsentAnswer::Decline));
            }
        });
    });
    if dismissed {
        cx.bridge.send(Command::RemoteConsent(ConsentAnswer::Decline));
    }
}

fn their_side(role: shocking_vrc_remote::link::OfferedRole) -> &'static str {
    use shocking_vrc_remote::link::OfferedRole;
    match role {
        OfferedRole::Receiver => "control of your device",
        OfferedRole::Controller => "you controlling their device",
        OfferedRole::Both => "control both ways",
    }
}

fn scheme_prompt(ctx: &Context, cx: &mut Ctx) {
    if !cfg!(windows) || cx.snap.remote_settings.scheme_asked || cx.snap.remote.service.consent.is_some() {
        return;
    }
    let pal = cx.pal;
    let mut answer = None;
    let dismissed = super::modal(ctx, pal, "remote_scheme", "Open partner links here?", 400.0, |ui| {
        w::body(ui, pal, "Open shockingvrc:// links with ShockingVRC?");
        w::dim(
            ui,
            pal,
            "Adds a link handler for your Windows user only. You can always paste links instead.",
        );
        ui.add_space(space::ITEM_GAP);
        ui.horizontal(|ui| {
            if w::cta(ui, pal, "Yes").clicked() {
                answer = Some(true);
            }
            if w::button(ui, pal, "Not now").clicked() {
                answer = Some(false);
            }
        });
    });
    match (answer, dismissed) {
        (Some(true), _) => cx.bridge.send(Command::RemoteRegisterScheme),
        (Some(false), _) | (None, true) => {
            let mut s = cx.snap.remote_settings.clone();
            s.scheme_asked = true;
            cx.bridge.send(Command::SetRemoteSettings(Box::new(s)));
        }
        _ => {}
    }
}
