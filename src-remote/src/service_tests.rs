use std::time::{Duration, Instant};

use crate::link::OfferedRole;
use crate::protocol::{PolicySnapshot, SlotFrame, SlotInfo, WireChannel};
use crate::service::{ConnectionMode, ConsentAnswer, InviteRequest, RemoteConfig, RemoteService};
use crate::store::Lifetime;

async fn until<F: FnMut() -> bool>(what: &str, timeout: Duration, mut f: F) {
    let start = Instant::now();
    while !f() {
        assert!(start.elapsed() < timeout, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn config(name: &str) -> RemoteConfig {
    RemoteConfig {
        mode: ConnectionMode::DirectHybrid,
        name: name.into(),
        lan_port: 0,
        jitter_ms: 0,
        ..RemoteConfig::default()
    }
}

async fn reachable(name: &str) -> RemoteService {
    let svc = RemoteService::start(config(name), None);
    until("the LAN listener", Duration::from_secs(5), || svc.status().listening.is_some()).await;
    let port: u16 = svc
        .status()
        .listening
        .and_then(|a| a.rsplit(':').next().and_then(|p| p.parse().ok()))
        .unwrap();
    svc.set_config(RemoteConfig {
        public_addr: format!("127.0.0.1:{port}"),
        ..config(name)
    });
    svc
}

fn frame(power: f32) -> Vec<SlotFrame> {
    vec![SlotFrame {
        slot: 0,
        a: Some(WireChannel {
            p: [power; 4],
            f: [100.0; 4],
            i: [100.0; 4],
            m: 200.0,
        }),
        b: None,
    }]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_controller_pairs_over_the_lan_and_drives_the_receiver() {
    let receiver = reachable("Receiver").await;
    receiver.set_policy(PolicySnapshot {
        slots: vec![SlotInfo {
            slot: 0,
            name: "Coyote".into(),
        }],
        ..PolicySnapshot::default()
    });
    let invite = receiver
        .create_invite(InviteRequest {
            role: OfferedRole::Receiver,
            lifetime: Lifetime::SingleUse,
            passphrase: None,
            label: "test".into(),
        })
        .unwrap();
    let controller = RemoteService::start(config("Controller"), None);
    controller.connect_link(&invite.link, None).unwrap();

    until("the consent prompt", Duration::from_secs(15), || {
        receiver.status().consent.is_some()
    })
    .await;
    let ask = receiver.status().consent.unwrap();
    assert_eq!(ask.partner, "Controller");
    assert_eq!(ask.role, OfferedRole::Receiver);
    receiver.answer_consent(ConsentAnswer::AcceptOnce);

    until("both sessions", Duration::from_secs(10), || {
        receiver.status().receiving().is_some() && controller.status().controlling().is_some()
    })
    .await;
    let c = controller.status();
    let s = c.controlling().unwrap();
    assert_eq!(s.partner, "Receiver");
    assert!(!s.i_receive, "the receiver offered only to be driven");
    assert_eq!(
        s.partner_policy.as_ref().map(|p| p.slots.len()),
        Some(1),
        "the controller sees the limits"
    );
    assert!(receiver.status().invites.is_empty(), "a single-use invite is spent");

    let mut got = None;
    let start = Instant::now();
    while got.is_none() && start.elapsed() < Duration::from_secs(5) {
        controller.send_frame(frame(0.5));
        tokio::time::sleep(Duration::from_millis(25)).await;
        got = receiver.take_inbound();
    }
    let got = got.expect("frames reach the receiver");
    assert_eq!(got.slots[0].a.unwrap().p, [0.5; 4]);
    assert!(receiver.inbound_live());

    until("latency", Duration::from_secs(3), || {
        controller.status().controlling().and_then(|s| s.latency_ms).is_some()
    })
    .await;

    receiver.stop_all("test kill switch");
    until("both ends to stop", Duration::from_secs(5), || {
        receiver.status().sessions.is_empty() && controller.status().sessions.is_empty()
    })
    .await;
    assert!(receiver.take_inbound().is_none(), "nothing is played after stop");
    assert!(!receiver.inbound_live());
    receiver.shutdown();
    controller.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_declined_partner_gets_nothing_and_a_used_link_fails() {
    let receiver = reachable("Receiver").await;
    let invite = receiver
        .create_invite(InviteRequest {
            role: OfferedRole::Receiver,
            lifetime: Lifetime::SingleUse,
            passphrase: None,
            label: String::new(),
        })
        .unwrap();
    let controller = RemoteService::start(config("Controller"), None);
    controller.connect_link(&invite.link, None).unwrap();
    until("the consent prompt", Duration::from_secs(15), || {
        receiver.status().consent.is_some()
    })
    .await;
    receiver.answer_consent(ConsentAnswer::Decline);
    until("the refusal", Duration::from_secs(10), || {
        controller.status().last_error.is_some_and(|e| e.contains("declined"))
    })
    .await;
    assert!(receiver.status().sessions.is_empty());
    receiver.shutdown();
    controller.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn indefinite_pairing_needs_a_passphrase_and_then_trusts() {
    let receiver = reachable("Receiver").await;
    let no_pass = receiver.create_invite(InviteRequest {
        role: OfferedRole::Receiver,
        lifetime: Lifetime::Indefinite,
        passphrase: None,
        label: String::new(),
    });
    assert!(no_pass.is_err(), "indefinite keys require a passphrase");
    let invite = receiver
        .create_invite(InviteRequest {
            role: OfferedRole::Both,
            lifetime: Lifetime::Indefinite,
            passphrase: Some("open sesame".into()),
            label: String::new(),
        })
        .unwrap();
    let controller = RemoteService::start(config("Controller"), None);
    assert!(controller.connect_link(&invite.link, None).is_err(), "the passphrase is needed");
    assert!(controller.connect_link(&invite.link, Some("nope".into())).is_err());
    controller
        .connect_link(&invite.link, Some("open sesame".into()))
        .unwrap();
    until("the consent prompt", Duration::from_secs(15), || {
        receiver.status().consent.is_some()
    })
    .await;
    receiver.answer_consent(ConsentAnswer::AcceptOnce);
    until("the session", Duration::from_secs(10), || {
        controller.status().sessions.len() == 1 && receiver.status().sessions.len() == 1
    })
    .await;
    let s = controller.status();
    assert!(s.sessions[0].i_control && s.sessions[0].i_receive, "both ways");
    assert_eq!(s.partners.len(), 1, "the joiner remembers the pairing");
    assert!(s.partners[0].can_dial);
    assert_eq!(receiver.status().partners.len(), 1, "and so does the inviter");

    controller.stop_all("done");
    until("the end", Duration::from_secs(5), || receiver.status().sessions.is_empty()).await;

    let pk = controller.status().partners[0].pk.clone();
    controller.connect_partner(&pk).unwrap();
    until("the reconnect prompt", Duration::from_secs(15), || {
        receiver.status().consent.is_some()
    })
    .await;
    assert!(receiver.status().consent.unwrap().known, "a known partner is flagged");
    receiver.answer_consent(ConsentAnswer::AcceptOnce);
    until("the second session", Duration::from_secs(10), || {
        receiver.status().sessions.len() == 1
    })
    .await;

    let theirs = receiver.status().partners[0].pk.clone();
    receiver.revoke_partner(&theirs);
    until("the revocation to end it", Duration::from_secs(5), || {
        receiver.status().sessions.is_empty()
    })
    .await;
    receiver.shutdown();
    controller.shutdown();
}
