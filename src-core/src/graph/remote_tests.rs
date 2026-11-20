use std::time::{Duration, Instant};

use super::device::{ChannelFrame, SEGMENTS};
use super::draw::{DrawRect, DrawZoneParams, FaderParams, FillShape, ShapeFillParams, StampParams, XyPadParams};

use super::safety::{
    self, MergeMode, REMOTE_FRAME_TIMEOUT, RemoteChannelPolicy, RemoteInput, RemoteOverlay, SafeChannel,
};
use super::*;

const DT: f32 = 0.025;

fn wire(g: &mut Graph, from: NodeId, out: &str, to: NodeId, input: &str) {
    g.connect(PinRef::new(from, out), PinRef::new(to, input))
        .unwrap_or_else(|e| panic!("{out} → {input}: {e}"));
}

fn run(kind: NodeKind, ticks: usize) -> (GraphEvaluator, NodeId) {
    let mut g = Graph::default();
    let id = g.add_kind(kind, [0.0, 0.0]);
    let mut ev = GraphEvaluator::new(&g);
    for _ in 0..ticks {
        ev.step(DT);
    }
    (ev, id)
}

fn out(ev: &GraphEvaluator, id: NodeId, key: &str) -> f32 {
    ev.output_by_key(id, key).unwrap_or_else(|| panic!("no output {key}"))
}

#[test]
fn the_catalog_is_unchanged_while_remote_is_off() {
    let off = catalog_with(false);
    assert_eq!(off.len(), 64, "the pre-remote catalog has 64 entries");
    assert!(off.iter().all(|e| e.category != Category::Remote));
    assert_eq!(off.first().map(|e| e.label), Some("OSC Float"));
    assert_eq!(off.last().map(|e| e.label), Some("Frame / Comment"));
    for e in &off {
        assert!(!(e.make)().is_remote(), "{} is a remote node", e.label);
    }
}

#[test]
fn remote_entries_appear_only_when_enabled() {
    let off: Vec<&str> = catalog_with(false).iter().map(|e| e.label).collect();
    let on = catalog_with(true);
    let remote: Vec<&str> = on
        .iter()
        .filter(|e| e.category == Category::Remote)
        .map(|e| e.label)
        .collect();
    assert_eq!(
        remote,
        [
            "Partner Device",
            "Draw Zone",
            "Shape Fill",
            "Stamp Pattern",
            "XY Pad",
            "Segment Painter",
            "Envelope / Fader"
        ]
    );
    let rest: Vec<&str> = on
        .iter()
        .filter(|e| e.category != Category::Remote)
        .map(|e| e.label)
        .collect();
    assert_eq!(rest, off, "the other entries keep their order");
    assert_eq!(on.last().map(|e| e.label), Some("Frame / Comment"));
}

#[test]
fn saved_remote_nodes_load_inert_while_off_and_revive_when_on() {
    let mut g = Graph::default();
    let pad = g.add_kind(NodeKind::XyPad(XyPadParams::default()), [0.0, 0.0]);
    let json = serde_json::to_value(&g.node(pad).unwrap().kind).unwrap();

    let inert = kind_from_value(json.clone(), false);
    assert!(inert.is_missing(), "a remote node is a placeholder while remote is off");
    let saved = serde_json::to_value(Node::new(inert.clone(), [0.0, 0.0])).unwrap();
    assert_eq!(saved["kind"], json, "nothing is lost when saved again");

    let mut graph = Graph::default();
    graph.add(Node::new(inert, [0.0, 0.0]));
    let mut ev = GraphEvaluator::new(&graph);
    ev.step(DT);
    assert!(ev.partner_frames().is_empty());
    assert_eq!(graph.revive_remote(), 1);
    assert!(matches!(graph.nodes[0].kind, NodeKind::XyPad(_)));
    assert!(graph.has_remote_nodes());
}

#[test]
fn draw_zone_plays_its_drawing_inside_the_zone_only() {
    let p = DrawZoneParams {
        length_s: 1.0,
        zone: DrawRect {
            t0: 0.5,
            t1: 1.0,
            lo: 0.0,
            hi: 1.0,
        },
        points: vec![[0.0, 1.0], [1.0, 1.0]],
    };
    let (ev, id) = run(NodeKind::DrawZone(p.clone()), 8);
    assert_eq!(out(&ev, id, "out"), 0.0, "first half of the loop is outside the zone");
    assert_eq!(out(&ev, id, "active"), 0.0);
    let (ev, id) = run(NodeKind::DrawZone(p), 28);
    assert!((out(&ev, id, "out") - 1.0).abs() < 1e-4);
    assert_eq!(out(&ev, id, "active"), 1.0);
    assert!((out(&ev, id, "phase") - 0.7).abs() < 0.01);
}

#[test]
fn draw_zone_loops() {
    let (ev, id) = run(NodeKind::DrawZone(DrawZoneParams::default()), 170);
    let phase = out(&ev, id, "phase");
    assert!(phase < 0.1, "4 s loop wrapped after 4.25 s: {phase}");
}

#[test]
fn shape_fill_stays_within_its_band() {
    for shape in FillShape::ALL {
        let p = ShapeFillParams {
            shape,
            zone: DrawRect {
                t0: 0.0,
                t1: 1.0,
                lo: 0.2,
                hi: 0.6,
            },
            ..ShapeFillParams::default()
        };
        let mut g = Graph::default();
        let id = g.add_kind(NodeKind::ShapeFill(p), [0.0, 0.0]);
        g.node_mut(id).unwrap().set_input_default("jitter", 0.5);
        let mut ev = GraphEvaluator::new(&g);
        let mut seen = (f32::MAX, f32::MIN);
        for _ in 0..120 {
            ev.step(DT);
            let v = out(&ev, id, "out");
            seen = (seen.0.min(v), seen.1.max(v));
        }
        assert!(seen.0 >= 0.2 - 1e-4 && seen.1 <= 0.6 + 1e-4, "{shape:?}: {seen:?}");
        assert!(seen.1 > seen.0, "{shape:?} moves");
    }
}

#[test]
fn square_fill_follows_duty() {
    let p = ShapeFillParams {
        shape: FillShape::Square,
        zone: DrawRect::default(),
        ..ShapeFillParams::default()
    };
    let mut g = Graph::default();
    let id = g.add_kind(NodeKind::ShapeFill(p), [0.0, 0.0]);
    g.node_mut(id).unwrap().set_input_default("duty", 0.25);
    g.node_mut(id).unwrap().set_input_default("rate", 1.0);
    let mut ev = GraphEvaluator::new(&g);
    let mut high = 0;
    for _ in 0..40 {
        ev.step(DT);
        if out(&ev, id, "out") > 0.5 {
            high += 1;
        }
    }
    assert!((9..=11).contains(&high), "{high} of 40 ticks high at 25 % duty");
}

#[test]
fn stamp_pattern_plays_the_lane() {
    let p = StampParams {
        length_s: 1.0,
        ..StampParams::default()
    };
    let (ev, id) = run(NodeKind::StampPattern(p.clone()), 8);
    assert_eq!(out(&ev, id, "out"), 0.0, "t = 0.2 is a gap");
    let (ev, id) = run(NodeKind::StampPattern(p), 15);
    assert!(out(&ev, id, "out") > 0.9, "t = 0.375 is the middle of the swell");
}

#[test]
fn xy_pad_outputs_its_position() {
    let (ev, id) = run(
        NodeKind::XyPad(XyPadParams {
            x: 0.25,
            y: 0.75,
            touching: true,
            spring: true,
        }),
        1,
    );
    assert_eq!(out(&ev, id, "x"), 0.25);
    assert_eq!(out(&ev, id, "y"), 0.75);
    assert_eq!(out(&ev, id, "touch"), 1.0);
}

#[test]
fn segment_painter_outputs_every_bar() {
    let mut p = super::draw::SegmentPainterParams::default();
    p.freq[1][2] = 0.9;
    p.int[0][3] = 0.1;
    let (ev, id) = run(NodeKind::SegmentPainter(p), 1);
    assert_eq!(out(&ev, id, "b.freq3"), 0.9);
    assert_eq!(out(&ev, id, "a.int4"), 0.1);
    assert_eq!(out(&ev, id, "a.freq1"), 0.5);
}

#[test]
fn fader_envelope_rises_on_press_and_releases() {
    let pressed = FaderParams {
        level: 0.8,
        attack: 0.1,
        decay: 0.1,
        sustain: 0.5,
        release: 0.1,
        pressed: true,
    };
    let (ev, id) = run(NodeKind::Fader(pressed.clone()), 40);
    assert!((out(&ev, id, "env") - 0.5).abs() < 1e-3, "sustain");
    assert!((out(&ev, id, "out") - 0.4).abs() < 1e-3, "level × sustain");

    let mut g = Graph::default();
    let id = g.add_kind(NodeKind::Fader(pressed.clone()), [0.0, 0.0]);
    let mut ev = GraphEvaluator::new(&g);
    for _ in 0..40 {
        ev.step(DT);
    }
    g.node_mut(id).unwrap().kind = NodeKind::Fader(FaderParams {
        pressed: false,
        ..pressed
    });
    ev.rebuild(&g);
    for _ in 0..10 {
        ev.step(DT);
    }
    assert_eq!(out(&ev, id, "out"), 0.0, "released to zero");
}

#[test]
fn partner_device_emits_frames_for_its_slot() {
    let mut g = Graph::default();
    let pad = g.add_kind(
        NodeKind::XyPad(XyPadParams {
            x: 1.0,
            y: 0.5,
            ..XyPadParams::default()
        }),
        [0.0, 0.0],
    );
    let one = g.add_kind(NodeKind::Constant(ConstantParams { value: 0.6 }), [0.0, 0.0]);
    let partner = g.add_kind(
        NodeKind::PartnerDevice(PartnerDeviceParams {
            slot: 1,
            ..PartnerDeviceParams::default()
        }),
        [300.0, 0.0],
    );
    wire(&mut g, one, "out", partner, "a.power");
    wire(&mut g, pad, "x", partner, "a.freq");
    wire(&mut g, pad, "y", partner, "a.int");
    let mut ev = GraphEvaluator::new(&g);
    assert!(ev.is_valid());
    assert!(ev.frames().is_empty(), "a partner device never drives the local BLE");
    ev.step(DT);
    let f = &ev.partner_frames()[0];
    assert_eq!(f.slot, 1);
    assert!((f.channels[0].power - 0.6).abs() < 1e-6);
    assert_eq!(f.channels[0].intensity, [50.0; SEGMENTS]);
    assert_eq!(f.channels[0].limit_max, 200.0, "the Max pin defaults to the full range");
    assert_eq!(f.channels[1].power, 0.0);
}

fn full(power: f32) -> ChannelFrame {
    ChannelFrame {
        power,
        power_seg: [power; SEGMENTS],
        freq: [100.0; SEGMENTS],
        intensity: [100.0; SEGMENTS],
        limit_min: 50.0,
        limit_max: 200.0,
        ceiling: 200.0,
    }
}

#[test]
fn the_receivers_cap_always_wins() {
    let policy = RemoteChannelPolicy {
        cap: 25,
        freq_bounds: [40, 120],
        int_bounds: [10, 60],
        ..RemoteChannelPolicy::default()
    };
    let s = safety::remote_channel(&full(1.0), &policy, 1.0);
    assert_eq!(s.strength, 25);
    assert!(s.ceiling <= 25 && s.limit_max <= 25 && s.limit_min == 0);
    assert_eq!(s.freq, [100; 4]);
    assert_eq!(s.intensity, [60; 4]);

    let mut hostile = full(1.0);
    hostile.power = f32::INFINITY;
    hostile.limit_max = f32::NAN;
    hostile.freq = [1.0, 999.0, f32::NAN, 50.0];
    let s = safety::remote_channel(&hostile, &policy, 1.0);
    assert!(s.strength <= 25);
    assert!(s.freq.iter().all(|f| (40..=120).contains(f)), "{:?}", s.freq);

    let off = RemoteChannelPolicy {
        allowed: false,
        ..policy
    };
    assert_eq!(safety::remote_channel(&full(1.0), &off, 1.0).strength, 0);
}

#[test]
fn merge_modes_combine_local_and_remote() {
    let local = SafeChannel {
        strength: 20,
        ceiling: 40,
        ..SafeChannel::default()
    };
    let remote = SafeChannel {
        strength: 30,
        ceiling: 30,
        ..SafeChannel::default()
    };
    let p = |merge| RemoteChannelPolicy {
        merge,
        ..RemoteChannelPolicy::default()
    };
    let m = |mode, input| safety::merge_remote(local, input, &p(mode), false, false).strength;
    let live = RemoteInput::Live(remote);
    assert_eq!(m(MergeMode::RemoteOnly, live), 30);
    assert_eq!(m(MergeMode::LocalOnly, live), 20);
    assert_eq!(m(MergeMode::Max, live), 30);
    assert_eq!(m(MergeMode::SumCapped, live), 40, "20 + 30 capped at 40");
    assert_eq!(m(MergeMode::RemoteOnly, RemoteInput::Stale), 0);
    assert_eq!(m(MergeMode::Max, RemoteInput::Stale), 20);
    assert_eq!(m(MergeMode::RemoteOnly, RemoteInput::Inactive), 20);
    assert_eq!(
        safety::merge_remote(local, live, &p(MergeMode::RemoteOnly), true, false).strength,
        20,
        "the receiver's alarm overrides remote"
    );
    assert_eq!(
        safety::merge_remote(local, live, &p(MergeMode::Max), false, true).strength,
        0,
        "the receiver's panic overrides everything"
    );
}

#[test]
fn the_overlay_fails_safe_to_zero_on_frame_timeout() {
    let mut o = RemoteOverlay::default();
    o.set_policy(
        [RemoteChannelPolicy {
            cap: 40,
            merge: MergeMode::RemoteOnly,
            ramp_per_s: 0.0,
            ..RemoteChannelPolicy::default()
        }; 2],
    );
    o.set_active(true);
    let t0 = Instant::now();
    let local = [SafeChannel::default(); 2];
    o.submit([Some(full(1.0)), None], t0);
    o.mix(local, [false; 2], false, t0);
    let later = t0 + Duration::from_secs(3);
    o.submit([Some(full(1.0)), None], later);
    let out = o.mix(local, [false; 2], false, later);
    assert_eq!(out[0].strength, 40, "after the soft start");
    assert_eq!(out[1].strength, 0, "channel B gets no frame");

    let stale = later + REMOTE_FRAME_TIMEOUT + Duration::from_millis(1);
    let out = o.mix(local, [false; 2], false, stale);
    assert_eq!(out[0].strength, 0, "no frame for over a second is zero output");
    assert_eq!(o.output()[0].strength, 0);

    o.set_active(false);
    let quiet = SafeChannel {
        strength: 12,
        ..SafeChannel::default()
    };
    assert_eq!(
        o.mix([quiet; 2], [false; 2], false, stale)[0].strength,
        12,
        "stopped remote is local only"
    );
}

#[test]
fn the_overlay_soft_starts_and_limits_the_rise() {
    let mut o = RemoteOverlay::default();
    o.set_policy(
        [RemoteChannelPolicy {
            cap: 100,
            merge: MergeMode::RemoteOnly,
            ramp_per_s: 20.0,
            ..RemoteChannelPolicy::default()
        }; 2],
    );
    o.set_active(true);
    let t0 = Instant::now();
    let mut t = t0;
    let mut last = 0;
    for _ in 0..40 {
        o.submit([Some(full(1.0)), None], t);
        let s = o.mix([SafeChannel::default(); 2], [false; 2], false, t)[0].strength;
        assert!(s >= last, "monotonic rise");
        assert!(s as f32 <= 20.0 * t.duration_since(t0).as_secs_f32() + 1.0, "{s} rose too fast");
        last = s;
        t += Duration::from_millis(25);
    }
    assert!(last > 0);
}
