use super::eval::ExternalSample;
use super::kind::*;
use super::*;
use crate::cli::config::{MotionNorms, UkfConfig};

const DT: f32 = 0.025;

fn device(g: &mut Graph, mac: &str) -> NodeId {
    g.add_kind(NodeKind::Device(DeviceParams::bound_to(mac)), [400.0, 0.0])
}

fn contact(g: &mut Graph) -> NodeId {
    g.add_kind(NodeKind::Contact(ContactParams::default()), [0.0, 0.0])
}

fn wire(g: &mut Graph, from: NodeId, out: &str, to: NodeId, input: &str) {
    g.connect(PinRef::new(from, out), PinRef::new(to, input))
        .unwrap_or_else(|e| panic!("{out} → {input}: {e}"));
}

fn feed(ev: &mut GraphEvaluator, level: f32) {
    for s in ev.external_mut() {
        *s = ExternalSample {
            value: level,
            fresh: level > 0.0,
        };
    }
}

#[test]
fn an_empty_graph_drives_nothing() {
    let mut ev = GraphEvaluator::new(&Graph::default());
    assert!(ev.step(DT).is_empty());
    assert!(ev.issues().iter().any(|i| i.severity == Severity::Info));
}

#[test]
fn contact_to_power_follows_the_level() {
    let mut g = Graph::default();
    let c = contact(&mut g);
    let d = device(&mut g, "AA");
    wire(&mut g, c, "level", d, "a.power");
    let mut ev = GraphEvaluator::new(&g);
    assert!(ev.is_valid());
    for level in [0.0, 0.3, 0.9] {
        feed(&mut ev, level);
        let f = &ev.step(DT)[0];
        assert!((f.channels[0].power - level).abs() < 1e-6);
        assert_eq!(f.channels[1].power, 0.0, "channel B is untouched");
        assert_eq!(f.channels[0].limit_max, 30.0, "default Limit Max");
    }
}

#[test]
fn an_unwired_device_is_reported() {
    let mut g = Graph::default();
    device(&mut g, "AA");
    let ev = GraphEvaluator::new(&g);
    assert!(ev.issues().iter().any(|i| i.severity == Severity::Warning));
}

#[test]
fn an_lfo_moves_the_frequency_and_four_pins_set_each_segment() {
    let mut g = Graph::default();
    let lfo = g.add_kind(NodeKind::Lfo(LfoParams::default()), [0.0, 0.0]);
    let d = device(&mut g, "AA");
    wire(&mut g, lfo, "out", d, "a.freq");
    let mut ev = GraphEvaluator::new(&g);
    let mut seen = Vec::new();
    for _ in 0..40 {
        seen.push(ev.step(DT)[0].channels[0].freq[3]);
    }
    let (lo, hi) = seen.iter().fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    assert!(hi - lo > 50.0, "frequency should sweep: {lo}..{hi}");

    let mut g = Graph::default();
    let mut params = DeviceParams::bound_to("AA");
    params.channels[0].set_expanded("freq", true);
    let d = g.add_kind(NodeKind::Device(params), [0.0, 0.0]);
    for (s, v) in [0.0f32, 0.3, 0.6, 1.0].iter().enumerate() {
        let k = g.add_kind(NodeKind::Constant(ConstantParams { value: *v }), [0.0, 0.0]);
        wire(&mut g, k, "out", d, &format!("a.freq{}", s + 1));
    }
    let mut ev = GraphEvaluator::new(&g);
    let f = ev.step(DT)[0].channels[0].freq;
    assert!(f[0] > f[1] && f[1] > f[2] && f[2] > f[3], "raw falls as Hz rises: {f:?}");
}

#[test]
fn a_single_frequency_pin_fills_the_packet_with_the_latest_samples() {
    let mut g = Graph::default();
    let s = g.add_kind(NodeKind::Slider(SliderParams::default()), [0.0, 0.0]);
    let d = device(&mut g, "AA");
    wire(&mut g, s, "out", d, "a.int");
    let mut ev = GraphEvaluator::new(&g);
    for v in [0.1f32, 0.2, 0.3, 0.4] {
        if let Some(n) = g.node_mut(s)
            && let NodeKind::Slider(p) = &mut n.kind
        {
            p.value = v;
        }
        ev.rebuild(&g);
        ev.step(DT);
    }
    let int = ev.frames()[0].channels[0].intensity;
    assert_eq!(int.map(|v| v.round() as i32), [10, 20, 30, 40], "oldest first");
}

#[test]
fn ukf_is_only_applied_when_placed() {
    let mut g = Graph::default();
    let c = contact(&mut g);
    let ukf = g.add_kind(NodeKind::Ukf(UkfNodeParams::default()), [100.0, 0.0]);
    let curve = g.add_kind(NodeKind::Curve(CurveParams::default()), [200.0, 0.0]);
    let d = device(&mut g, "AA");
    wire(&mut g, c, "level", ukf, "in");
    wire(&mut g, ukf, "speed_n", curve, "in");
    wire(&mut g, curve, "out", d, "a.int");
    wire(&mut g, c, "level", d, "a.power");
    let mut ev = GraphEvaluator::new(&g);
    let mut moved = false;
    for k in 0..60 {
        feed(&mut ev, (k as f32 * 0.05).sin().abs());
        let f = ev.step(DT)[0].clone();
        moved |= f.channels[0].intensity[3] > 1.0;
        let raw = ev.output_by_key(c, "level").unwrap();
        let ch = f.channels[0];
        assert_eq!(ch.power_seg[3], raw, "power is unfiltered without a UKF in its path");
        assert_eq!(ch.power, ch.power_seg.iter().copied().fold(0.0, f32::max));
    }
    assert!(moved, "the speed output drives intensity");
}

#[test]
fn two_ukf_nodes_with_different_tuning_disagree() {
    let mut g = Graph::default();
    let c = contact(&mut g);
    let soft = g.add_kind(
        NodeKind::Ukf(UkfNodeParams {
            ukf: UkfConfig {
                q: 0.5,
                r: 0.2,
                ..UkfConfig::default()
            },
            norms: MotionNorms {
                speed: 20.0,
                ..MotionNorms::default()
            },
        }),
        [0.0, 0.0],
    );
    let sharp = g.add_kind(NodeKind::Ukf(UkfNodeParams::default()), [0.0, 0.0]);
    wire(&mut g, c, "level", soft, "in");
    wire(&mut g, c, "level", sharp, "in");
    let mut ev = GraphEvaluator::new(&g);
    let mut diff = 0.0f32;
    for k in 0..80 {
        feed(&mut ev, if (k / 10) % 2 == 0 { 0.1 } else { 0.9 });
        ev.step(DT);
        let a = ev.output_by_key(soft, "speed_n").unwrap();
        let b = ev.output_by_key(sharp, "speed_n").unwrap();
        diff = diff.max((a - b).abs());
        let la = ev.output_by_key(soft, "level").unwrap();
        let lb = ev.output_by_key(sharp, "level").unwrap();
        diff = diff.max((la - lb).abs());
    }
    assert!(diff > 0.05, "tuning must matter, max diff {diff}");
}

#[test]
fn non_finite_values_are_sanitised_at_every_output() {
    let mut g = Graph::default();
    let big = g.add_kind(NodeKind::Constant(ConstantParams { value: 1e30 }), [0.0, 0.0]);
    let mul = g.add_kind(NodeKind::Math(MathParams { op: MathOp::Multiply }), [0.0, 0.0]);
    let d = device(&mut g, "AA");
    wire(&mut g, big, "out", mul, "a");
    wire(&mut g, big, "out", mul, "b");
    wire(&mut g, mul, "out", d, "a.power");
    wire(&mut g, mul, "out", d, "a.freq");
    let mut ev = GraphEvaluator::new(&g);
    let f = ev.step(DT)[0].clone();
    assert_eq!(ev.output_by_key(mul, "out"), Some(0.0), "inf → 0");
    assert!(f.channels[0].power.is_finite());
    assert!(f.channels[0].freq.iter().all(|v| v.is_finite()));
}

#[test]
fn a_loop_in_a_file_is_an_error_and_drives_zero() {
    let mut g = Graph::default();
    let a = g.add_kind(NodeKind::Math(MathParams { op: MathOp::Add }), [0.0, 0.0]);
    let b = g.add_kind(NodeKind::Math(MathParams { op: MathOp::Add }), [0.0, 0.0]);
    let d = device(&mut g, "AA");
    wire(&mut g, a, "out", b, "a");
    g.edges.push(Edge {
        from: PinRef::new(b, "out"),
        to: PinRef::new(a, "a"),
    });
    wire(&mut g, b, "out", d, "a.power");
    let mut ev = GraphEvaluator::new(&g);
    assert!(!ev.is_valid());
    assert!(ev.issues().iter().any(|i| i.severity == Severity::Error));
    assert_eq!(ev.step(DT)[0].channels[0].power, 0.0);
}

#[test]
fn feedback_through_a_delay_counts_ticks() {
    let mut g = Graph::default();
    let add = g.add_kind(NodeKind::Math(MathParams { op: MathOp::Add }), [0.0, 0.0]);
    let delay = g.add_kind(NodeKind::Delay(DelayParams::default()), [0.0, 0.0]);
    g.node_mut(add).unwrap().set_input_default("b", 1.0);
    wire(&mut g, add, "out", delay, "in");
    wire(&mut g, delay, "out", add, "a");
    let mut ev = GraphEvaluator::new(&g);
    assert!(ev.is_valid());
    for _ in 0..5 {
        ev.step(DT);
    }
    assert_eq!(ev.output_by_key(add, "out"), Some(5.0));
}

#[test]
fn state_survives_edits_elsewhere_and_resets_on_its_own_edit() {
    let mut g = Graph::default();
    let k = g.add_kind(NodeKind::Constant(ConstantParams { value: 1.0 }), [0.0, 0.0]);
    let avg = g.add_kind(NodeKind::MovingAverage(MovingAverageParams { window_ms: 250.0 }), [0.0, 0.0]);
    let other = g.add_kind(NodeKind::Constant(ConstantParams { value: 0.0 }), [0.0, 0.0]);
    wire(&mut g, k, "out", avg, "in");
    let mut ev = GraphEvaluator::new(&g);
    for _ in 0..4 {
        ev.step(DT);
    }
    let warm = ev.output_by_key(avg, "out").unwrap();

    if let NodeKind::Constant(p) = &mut g.node_mut(other).unwrap().kind {
        p.value = 0.7;
    }
    let r = ev.rebuild(&g);
    assert!(!r.topology_changed, "a parameter edit is not a topology change");
    ev.step(DT);
    assert!(ev.output_by_key(avg, "out").unwrap() >= warm);

    if let NodeKind::MovingAverage(p) = &mut g.node_mut(avg).unwrap().kind {
        p.window_ms = 1000.0;
    }
    ev.rebuild(&g);
    ev.step(DT);
    assert_eq!(ev.output_by_key(avg, "out"), Some(1.0), "fresh buffer: one sample of 1.0");

    g.add_kind(NodeKind::Time(NoParams {}), [0.0, 0.0]);
    assert!(ev.rebuild(&g).topology_changed);
}

#[test]
fn a_group_evaluates_like_its_contents() {
    let mut g = Graph::default();
    let k = g.add_kind(NodeKind::Constant(ConstantParams { value: 0.25 }), [0.0, 0.0]);
    let m = g.add_kind(NodeKind::Math(MathParams { op: MathOp::Multiply }), [100.0, 0.0]);
    g.node_mut(m).unwrap().set_input_default("b", 2.0);
    let d = device(&mut g, "AA");
    wire(&mut g, k, "out", m, "a");
    wire(&mut g, m, "out", d, "a.power");
    let mut flat = GraphEvaluator::new(&g);
    let expect = flat.step(DT)[0].channels[0].power;

    let sel = [m].into_iter().collect();
    g.group(&sel, "double").expect("group");
    let mut grouped = GraphEvaluator::new(&g);
    assert!(grouped.is_valid());
    assert_eq!(grouped.step(DT)[0].channels[0].power, expect);
    assert_eq!(expect, 0.5);
}

#[test]
fn placeholders_output_nothing_but_the_rest_runs() {
    let json = r#"{"nodes":[
        {"id":1,"kind":{"type":"Hologram","x":1}},
        {"id":2,"kind":{"type":"Constant","value":0.4}},
        {"id":3,"kind":{"type":"Device","binding":{"Mac":"AA"}}}
    ],"edges":[
        {"from":{"node":1,"pin":"out"},"to":{"node":3,"pin":"b.power"}},
        {"from":{"node":2,"pin":"out"},"to":{"node":3,"pin":"a.power"}}
    ]}"#;
    let g = parse_graph(json).unwrap();
    let mut ev = GraphEvaluator::new(&g);
    assert!(ev.issues().iter().any(|i| i.message.contains("Hologram")));
    let f = ev.step(DT)[0].clone();
    assert!((f.channels[0].power - 0.4).abs() < 1e-6);
    assert_eq!(f.channels[1].power, 0.0);
}

#[test]
fn a_latched_panic_node_requests_panic() {
    let mut g = Graph::default();
    g.add_kind(NodeKind::Panic(PanicParams { latched: true }), [0.0, 0.0]);
    let mut ev = GraphEvaluator::new(&g);
    ev.step(DT);
    assert!(ev.panic_requested());
}

#[test]
fn one_contact_drives_two_devices_with_different_curves() {
    let mut g = Graph::default();
    let c = contact(&mut g);
    let ease = g.add_kind(
        NodeKind::Curve(CurveParams {
            curve: Curve::preset(CurvePreset::EaseIn),
        }),
        [0.0, 0.0],
    );
    let d1 = device(&mut g, "AA");
    let d2 = device(&mut g, "BB");
    wire(&mut g, c, "level", d1, "a.power");
    wire(&mut g, c, "level", ease, "in");
    wire(&mut g, ease, "out", d2, "a.power");
    let mut ev = GraphEvaluator::new(&g);
    feed(&mut ev, 0.5);
    let frames = ev.step(DT).to_vec();
    assert_eq!(frames.len(), 2);
    let by = |mac: &str| frames.iter().find(|f| f.binding.mac() == Some(mac)).unwrap();
    assert!((by("AA").channels[0].power - 0.5).abs() < 1e-6);
    assert!((by("BB").channels[0].power - 0.25).abs() < 1e-2);
}

#[test]
fn duplicate_device_nodes_do_not_both_drive() {
    let mut g = Graph::default();
    device(&mut g, "AA");
    device(&mut g, "aa");
    let ev = GraphEvaluator::new(&g);
    assert_eq!(ev.frames().len(), 1);
}

#[test]
fn every_catalog_node_evaluates_without_panicking() {
    let mut g = Graph::default();
    let lfo = g.add_kind(NodeKind::Lfo(LfoParams::default()), [0.0, 0.0]);
    for e in catalog() {
        let id = g.add_kind((e.make)(), [0.0, 0.0]);
        let inputs = g.node(id).unwrap().inputs();
        for p in inputs.iter().take(2) {
            let _ = g.connect(PinRef::new(lfo, "out"), PinRef::new(id, p.key.to_string()));
        }
    }
    let mut ev = GraphEvaluator::new(&g);
    for k in 0..200 {
        feed(&mut ev, (k % 7) as f32 / 7.0);
        ev.step(if k % 50 == 0 { 0.0 } else { DT });
    }
    let probe = ev.probe();
    for n in probe.nodes.values() {
        assert!(n.outputs.iter().chain(&n.inputs).all(|v| v.is_finite()));
    }
}

#[test]
fn a_metronome_keeps_time_and_its_beat_is_long_enough_to_send() {
    for (hz, secs, expect) in [(2.0f32, 10.0f32, 20usize), (10.0, 2.0, 20)] {
        let mut g = Graph::default();
        let m = g.add_kind(NodeKind::Metronome(NoParams {}), [0.0, 0.0]);
        g.node_mut(m).unwrap().set_input_default("hz", hz);
        let mut ev = GraphEvaluator::new(&g);
        let ticks = (secs / DT).round() as usize;
        let (mut gate_ticks, mut gate_rises, mut beat_rises, mut run) = (0, 0, 0, 0);
        let mut runs = Vec::new();
        let (mut last_gate, mut last_beat) = (false, false);
        for _ in 0..ticks {
            ev.step(DT);
            let gate = ev.output_by_key(m, "gate").unwrap() > 0.5;
            let beat = ev.output_by_key(m, "beat").unwrap() > 0.5;
            gate_ticks += gate as usize;
            gate_rises += (gate && !last_gate) as usize;
            beat_rises += (beat && !last_beat) as usize;
            if beat {
                run += 1;
            } else if run > 0 {
                runs.push(run);
                run = 0;
            }
            (last_gate, last_beat) = (gate, beat);
        }
        assert!(gate_rises.abs_diff(expect) <= 1, "{hz} Hz: {gate_rises} gates");
        assert!(beat_rises.abs_diff(expect) <= 1, "{hz} Hz: {beat_rises} beats");
        let duty = gate_ticks as f32 / ticks as f32;
        assert!((duty - 0.5).abs() < 0.06, "{hz} Hz: duty {duty}");
        assert!(
            runs.iter().all(|r| *r >= 2 && *r as f32 * DT <= 0.13),
            "{hz} Hz: a beat must last 50–125 ms to reach a packet: {runs:?}"
        );
    }
}

#[test]
fn a_gate_faster_than_a_packet_keeps_its_rhythm_inside_the_packet() {
    use super::safety::{AlarmOverlay, safe_channel};
    let mut g = Graph::default();
    let m = g.add_kind(NodeKind::Metronome(NoParams {}), [0.0, 0.0]);
    g.node_mut(m).unwrap().set_input_default("hz", 10.0);
    let d = device(&mut g, "AA");
    wire(&mut g, m, "gate", d, "a.power");
    let mut ev = GraphEvaluator::new(&g);
    for k in 0..40 {
        let f = ev.step(DT)[0].channels[0];
        if k < 4 {
            continue;
        }
        let s = safe_channel(&f, 200, AlarmOverlay::default(), 1.0);
        assert_eq!(s.strength, 30, "tick {k}: the packet plays at the peak power");
        let loud = s.intensity.iter().filter(|v| **v > 90).count();
        let silent = s.intensity.iter().filter(|v| **v < 10).count();
        assert_eq!((loud, silent), (2, 2), "tick {k}: {:?} should be on-on-off-off", s.intensity);
    }
}
