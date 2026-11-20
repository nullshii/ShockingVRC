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

async fn timed_session(secs: u64) -> (RemoteService, RemoteService, u64) {
    let receiver = reachable("Receiver").await;
    let invite = receiver
        .create_invite(InviteRequest {
            role: OfferedRole::Receiver,
            lifetime: Lifetime::Minutes10,
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
    let expires = crate::store::now_unix() + secs;
    receiver.set_invite_expiry(&invite.id, expires);
    receiver.answer_consent(ConsentAnswer::AcceptOnce);
    until("the session", Duration::from_secs(10), || {
        receiver.status().receiving().is_some() && controller.status().controlling().is_some()
    })
    .await;
    (receiver, controller, expires * 1000)
}

async fn assert_ends_on_time(receiver: &RemoteService, controller: &RemoteService, deadline_ms: u64) {
    until("the time limit", Duration::from_secs(15), || {
        receiver.status().sessions.is_empty() && controller.status().sessions.is_empty()
    })
    .await;
    let ended = crate::store::now_unix_ms();
    assert!(ended >= deadline_ms, "ended {} ms early", deadline_ms - ended);
    assert!(ended < deadline_ms + 500, "ran {} ms past the limit", ended - deadline_ms);
    assert!(receiver.export_log().contains("time is up"));
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(receiver.status().sessions.is_empty(), "nothing comes back after the limit");
    assert!(controller.status().sessions.is_empty());
    assert!(controller.status().connecting.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_timed_session_ends_when_its_invite_expires() {
    let (receiver, controller, deadline_ms) = timed_session(3).await;
    let s = controller.status();
    assert_eq!(
        s.controlling().unwrap().ends_at_ms,
        Some(deadline_ms),
        "the controller knows the limit"
    );
    assert_ends_on_time(&receiver, &controller, deadline_ms).await;
    receiver.shutdown();
    controller.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reconnecting_does_not_extend_a_timed_session() {
    let (receiver, controller, deadline_ms) = timed_session(5).await;
    let first = receiver.status().receiving().unwrap().id;
    controller.drop_connections();
    until("the resumed session", Duration::from_secs(10), || {
        receiver.status().receiving().is_some_and(|s| s.id != first)
    })
    .await;
    assert_eq!(
        receiver.status().receiving().unwrap().ends_at_ms,
        Some(deadline_ms),
        "a resumed session keeps the original limit"
    );
    assert_ends_on_time(&receiver, &controller, deadline_ms).await;
    receiver.shutdown();
    controller.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_resume_after_the_limit_is_refused() {
    let (receiver, controller, deadline_ms) = timed_session(4).await;
    let port: u16 = receiver
        .status()
        .listening
        .and_then(|a| a.rsplit(':').next().and_then(|p| p.parse().ok()))
        .unwrap();
    receiver.stop_network();
    tokio::time::sleep(Duration::from_millis(200)).await;
    controller.drop_connections();
    let wait = deadline_ms.saturating_sub(crate::store::now_unix_ms()) + 300;
    tokio::time::sleep(Duration::from_millis(wait)).await;
    receiver.set_config(RemoteConfig {
        lan_port: port,
        public_addr: format!("127.0.0.1:{port}"),
        ..config("Receiver")
    });
    until("the late resume to be refused", Duration::from_secs(10), || {
        receiver.export_log().contains("session expired")
    })
    .await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(
        receiver.status().sessions.is_empty(),
        "a late resume does not reopen the session"
    );
    assert!(controller.status().sessions.is_empty());
    receiver.shutdown();
    controller.shutdown();
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

fn vds() -> Option<String> {
    std::env::var("SVRC_VDS").ok().filter(|v| !v.is_empty())
}

fn internet(name: &str, vds: &str) -> RemoteConfig {
    RemoteConfig {
        mode: ConnectionMode::Internet,
        name: name.into(),
        vds_url: vds.into(),
        lan_port: 0,
        jitter_ms: 0,
        ..RemoteConfig::default()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn two_apps_pair_through_the_rendezvous_server() {
    let Some(vds) = vds() else { return };
    let receiver = RemoteService::start(internet("Receiver", &vds), None);
    until("the receiver to be online", Duration::from_secs(10), || {
        matches!(receiver.status().vds, crate::service::VdsState::Online)
    })
    .await;
    let invite = receiver
        .create_invite(InviteRequest {
            role: OfferedRole::Receiver,
            lifetime: Lifetime::Minutes10,
            passphrase: None,
            label: String::new(),
        })
        .unwrap();
    let controller = RemoteService::start(internet("Controller", &vds), None);
    controller.connect_link(&invite.link, None).unwrap();
    until("the consent prompt", Duration::from_secs(30), || {
        receiver.status().consent.is_some()
    })
    .await;
    receiver.answer_consent(ConsentAnswer::AcceptOnce);
    until("the session", Duration::from_secs(15), || {
        controller.status().controlling().is_some()
    })
    .await;
    assert_eq!(controller.status().controlling().unwrap().path, crate::transport::PathKind::P2p);
    let mut got = None;
    let start = Instant::now();
    while got.is_none() && start.elapsed() < Duration::from_secs(5) {
        controller.send_frame(frame(0.7));
        tokio::time::sleep(Duration::from_millis(25)).await;
        got = receiver.take_inbound();
    }
    assert_eq!(got.expect("frames over WebRTC").slots[0].a.unwrap().p, [0.7; 4]);
    controller.stop_all("done");
    until("the end", Duration::from_secs(5), || receiver.status().sessions.is_empty()).await;
    receiver.shutdown();
    controller.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn web_panel_host() {
    let Some(vds) = vds() else { return };
    let receiver = RemoteService::start(internet("Web test receiver", &vds), None);
    receiver.set_policy(PolicySnapshot {
        slots: vec![SlotInfo {
            slot: 0,
            name: "Coyote".into(),
        }],
        ..PolicySnapshot::default()
    });
    until("online", Duration::from_secs(10), || {
        matches!(receiver.status().vds, crate::service::VdsState::Online)
    })
    .await;
    let code = receiver.create_web_code(Lifetime::Minutes10).await.unwrap();
    println!("WEB CODE {code}");
    let start = Instant::now();
    let mut frames = 0u32;
    while start.elapsed() < Duration::from_secs(240) {
        if receiver.status().consent.is_some() {
            println!("CONSENT {:?}", receiver.status().consent);
            receiver.answer_consent(ConsentAnswer::AcceptOnce);
        }
        if let Some(f) = receiver.take_inbound() {
            frames += 1;
            if frames % 40 == 1 {
                println!("FRAME #{frames} {:?}", f.slots.first().and_then(|s| s.a));
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    println!("LOG {}", receiver.export_log());
    receiver.shutdown();
}
