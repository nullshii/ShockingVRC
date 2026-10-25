use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, TAU};

use crate::dsp::UnscentedKalman;
use crate::modulation::evaluator::evaluate_with_accumulator;
use crate::modulation::noise;

use super::compile::{self, Extra, ExternalRequest, GraphIssue, Plan, Src};
use super::device::{ChannelFrame, DEFAULT_CHANNEL, DeviceFrame, MAX_CHANNELS, PinRole, SEGMENTS};
use super::kind::{
    CombineMode, CompareOp, EnvMode, LfoShape, LogicOp, NodeKind, NoiseKind, as_bool, from_bool,
    sanitised_ukf,
};
use super::model::{Graph, NodeId};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ExternalSample {
    pub value: f32,
    pub fresh: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AlarmSample {
    pub strength: f32,
    pub active: bool,
}

#[derive(Debug)]
enum NodeState {
    None,
    Ukf { filter: Box<UnscentedKalman>, t: f32 },
    Prev { prev: f32, init: bool },
    Value { y: f32, init: bool },
    Peak { value: f32, held: f32 },
    Window { buf: Vec<f32>, idx: usize, filled: usize, sum: f64 },
    Delay { buf: Vec<f32>, idx: usize },
    Lfo { phase: f32, cycles: f64, done: bool, last_reset: bool, held: f32, rng: u32 },
    Noise { pos: f64 },
    Env { level: f32, stage: u8, gate: bool, rel_rate: f32 },
    Hold { value: f32, last_trig: bool },
    Flag(bool),
    Metro { phase: f32 },
    Timer { elapsed: f32 },
    Seq { phase: f32, index: usize, last_reset: bool },
    Guard { blocked: bool, last: f32, init: bool },
    Deadman { last: f32, unchanged: f32, stale: f32, init: bool },
    Device {
        power: [[f32; SEGMENTS]; MAX_CHANNELS],
        freq: [[f32; SEGMENTS]; MAX_CHANNELS],
        int: [[f32; SEGMENTS]; MAX_CHANNELS],
        init: bool,
    },
    Scope { buf: Vec<[f32; 4]>, idx: usize, filled: usize },
}

impl NodeState {
    fn for_kind(kind: &NodeKind) -> Self {
        match kind {
            NodeKind::Ukf(p) => NodeState::Ukf {
                filter: Box::new(UnscentedKalman::new(sanitised_ukf(p.ukf).into())),
                t: 0.0,
            },
            NodeKind::Derivative(_) => NodeState::Prev { prev: 0.0, init: false },
            NodeKind::Integrate(_)
            | NodeKind::Smooth(_)
            | NodeKind::Slew(_)
            | NodeKind::RampLimiter(_) => NodeState::Value { y: 0.0, init: false },
            NodeKind::PeakHold(_) => NodeState::Peak { value: 0.0, held: 0.0 },
            NodeKind::MovingAverage(p) => NodeState::Window {
                buf: vec![0.0; p.samples()],
                idx: 0,
                filled: 0,
                sum: 0.0,
            },
            NodeKind::Delay(p) => NodeState::Delay {
                buf: vec![0.0; p.ticks()],
                idx: 0,
            },
            NodeKind::Lfo(p) => NodeState::Lfo {
                phase: 0.0,
                cycles: 0.0,
                done: false,
                last_reset: false,
                held: 0.0,
                rng: p.seed.max(1).wrapping_mul(2_654_435_761).max(1),
            },
            NodeKind::Noise(_) => NodeState::Noise { pos: 0.0 },
            NodeKind::Envelope(_) => NodeState::Env {
                level: 0.0,
                stage: 0,
                gate: false,
                rel_rate: 0.0,
            },
            NodeKind::SampleHold(_) => NodeState::Hold { value: 0.0, last_trig: false },
            NodeKind::Schmitt(_) | NodeKind::Compare(_) => NodeState::Flag(false),
            NodeKind::Metronome(_) => NodeState::Metro { phase: 0.0 },
            NodeKind::Timer(_) => NodeState::Timer { elapsed: 0.0 },
            NodeKind::Sequencer(_) => NodeState::Seq {
                phase: 0.0,
                index: 0,
                last_reset: false,
            },
            NodeKind::SpoofGuard(_) => NodeState::Guard {
                blocked: false,
                last: 0.0,
                init: false,
            },
            NodeKind::Deadman(_) => NodeState::Deadman {
                last: 0.0,
                unchanged: 0.0,
                stale: 0.0,
                init: false,
            },
            NodeKind::Device(_) => NodeState::Device {
                power: [[0.0; SEGMENTS]; MAX_CHANNELS],
                freq: [[0.0; SEGMENTS]; MAX_CHANNELS],
                int: [[0.0; SEGMENTS]; MAX_CHANNELS],
                init: false,
            },
            NodeKind::Scope(p) => NodeState::Scope {
                buf: vec![[0.0; 4]; p.samples()],
                idx: 0,
                filled: 0,
            },
            _ => NodeState::None,
        }
    }

    fn buffer_len(&self) -> usize {
        match self {
            NodeState::Window { buf, .. } | NodeState::Delay { buf, .. } => buf.len(),
            NodeState::Scope { buf, .. } => buf.len(),
            _ => 0,
        }
    }

    fn compatible(&self, fresh: &NodeState) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(fresh)
            && self.buffer_len() == fresh.buffer_len()
    }

    fn retune(&mut self, kind: &NodeKind) {
        if let (NodeState::Ukf { filter, .. }, NodeKind::Ukf(p)) = (self, kind) {
            filter.set_params(sanitised_ukf(p.ukf).into());
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NodeProbe {
    pub inputs: Vec<f32>,
    pub outputs: Vec<f32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScopeTrace {
    pub count: usize,
    pub samples: Vec<[f32; 4]>,
}

#[derive(Debug, Clone, Default)]
pub struct GraphProbe {
    pub tick: u64,
    pub nodes: HashMap<u64, NodeProbe>,
    pub scopes: HashMap<u64, ScopeTrace>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rebuild {
    pub topology_changed: bool,
}

pub struct GraphEvaluator {
    plan: Plan,
    values: Vec<f32>,
    in_values: Vec<f32>,
    states: Vec<NodeState>,
    frames: Vec<DeviceFrame>,
    external: Vec<ExternalSample>,
    alarm: AlarmSample,
    time: f64,
    tick: u64,
    panic: bool,
}

impl Default for GraphEvaluator {
    fn default() -> Self {
        Self::new(&Graph::default())
    }
}

impl GraphEvaluator {
    pub fn new(graph: &Graph) -> Self {
        let mut me = Self {
            plan: Plan::default(),
            values: Vec::new(),
            in_values: Vec::new(),
            states: Vec::new(),
            frames: Vec::new(),
            external: Vec::new(),
            alarm: AlarmSample::default(),
            time: 0.0,
            tick: 0,
            panic: false,
        };
        me.rebuild(graph);
        me
    }

    pub fn rebuild(&mut self, graph: &Graph) -> Rebuild {
        let plan = compile::compile(graph);
        let topology_changed = plan.signature != self.plan.signature;

        let mut old: HashMap<u64, NodeState> = self
            .plan
            .nodes
            .iter()
            .map(|n| n.flat)
            .zip(std::mem::take(&mut self.states))
            .collect();
        let states = plan
            .nodes
            .iter()
            .map(|n| {
                let fresh = NodeState::for_kind(&n.kind);
                match old.remove(&n.flat) {
                    Some(mut prev) if prev.compatible(&fresh) => {
                        prev.retune(&n.kind);
                        prev
                    }
                    _ => fresh,
                }
            })
            .collect();

        let mut old_ext: HashMap<u64, ExternalSample> = HashMap::new();
        for n in &self.plan.nodes {
            if let Extra::External(i) = n.extra
                && let Some(s) = self.external.get(i)
            {
                old_ext.insert(n.flat, *s);
            }
        }
        let mut external = vec![ExternalSample::default(); plan.external.len()];
        for n in &plan.nodes {
            if let Extra::External(i) = n.extra
                && let Some(s) = old_ext.get(&n.flat)
            {
                external[i] = *s;
            }
        }

        self.frames = plan
            .devices
            .iter()
            .map(|(node, binding)| DeviceFrame {
                node: *node,
                binding: binding.clone(),
                channels: [ChannelFrame::default(); MAX_CHANNELS],
            })
            .collect();
        self.values = vec![0.0; plan.value_count];
        self.in_values = vec![0.0; plan.inputs.len()];
        self.states = states;
        self.external = external;
        self.plan = plan;
        Rebuild { topology_changed }
    }

    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    pub fn issues(&self) -> &[GraphIssue] {
        &self.plan.issues
    }

    pub fn is_valid(&self) -> bool {
        self.plan.valid
    }

    pub fn external_requests(&self) -> &[ExternalRequest] {
        &self.plan.external
    }

    pub fn external_mut(&mut self) -> &mut [ExternalSample] {
        &mut self.external
    }

    pub fn fill_external(&mut self, mut sample: impl FnMut(&ExternalRequest) -> ExternalSample) {
        for (req, slot) in self.plan.external.iter().zip(self.external.iter_mut()) {
            let s = sample(req);
            *slot = ExternalSample {
                value: if s.value.is_finite() { s.value } else { 0.0 },
                fresh: s.fresh,
            };
        }
    }

    pub fn set_alarm(&mut self, alarm: AlarmSample) {
        self.alarm = alarm;
    }

    pub fn frames(&self) -> &[DeviceFrame] {
        &self.frames
    }

    pub fn panic_requested(&self) -> bool {
        self.panic
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn time(&self) -> f64 {
        self.time
    }

    pub fn output(&self, flat: u64, pin: usize) -> Option<f32> {
        let n = self.plan.nodes.iter().find(|n| n.flat == flat)?;
        (pin < n.out_len as usize).then(|| self.values[n.out_start as usize + pin])
    }

    pub fn output_by_key(&self, node: NodeId, key: &str) -> Option<f32> {
        let n = self.plan.nodes.iter().find(|n| n.flat == node.0)?;
        let i = n.kind.outputs().iter().position(|p| p.key == key)?;
        self.output(node.0, i)
    }

    pub fn step(&mut self, dt: f32) -> &[DeviceFrame] {
        let dt = if dt.is_finite() { dt.clamp(0.0, 0.5) } else { 0.0 };
        self.time += dt as f64;
        self.tick = self.tick.wrapping_add(1);
        self.panic = false;

        if !self.plan.valid {
            for f in &mut self.frames {
                f.zeroed();
            }
            return &self.frames;
        }

        let ctx = Ctx {
            dt,
            time: self.time,
            alarm: self.alarm,
        };
        let Self {
            plan,
            values,
            in_values,
            states,
            frames,
            external,
            panic,
            ..
        } = self;

        for (ni, node) in plan.nodes.iter().enumerate() {
            let is = node.in_start as usize;
            let il = node.in_len as usize;
            for k in 0..il {
                in_values[is + k] = match plan.inputs[is + k] {
                    Src::Slot(s) => values[s as usize],
                    Src::Const(c) => c,
                };
            }
            let os = node.out_start as usize;
            let ol = node.out_len as usize;
            let ins = &in_values[is..is + il];
            let outs = &mut values[os..os + ol];
            eval_node(node, ins, outs, &mut states[ni], external, frames, panic, &ctx);
            for v in outs.iter_mut() {
                if !v.is_finite() {
                    *v = 0.0;
                }
            }
        }

        for &ni in &plan.delays {
            let node = &plan.nodes[ni];
            let is = node.in_start as usize;
            let v = match plan.inputs[is] {
                Src::Slot(s) => values[s as usize],
                Src::Const(c) => c,
            };
            in_values[is] = v;
            if let NodeState::Delay { buf, idx } = &mut states[ni]
                && !buf.is_empty()
            {
                buf[*idx] = v;
                *idx = (*idx + 1) % buf.len();
            }
        }

        &self.frames
    }

    pub fn probe(&self) -> GraphProbe {
        let mut nodes = HashMap::with_capacity(self.plan.nodes.len());
        let mut scopes = HashMap::new();
        for (ni, n) in self.plan.nodes.iter().enumerate() {
            let is = n.in_start as usize;
            let os = n.out_start as usize;
            nodes.insert(
                n.flat,
                NodeProbe {
                    inputs: self.in_values[is..is + n.in_len as usize].to_vec(),
                    outputs: self.values[os..os + n.out_len as usize].to_vec(),
                },
            );
            if let (NodeState::Scope { buf, idx, filled }, NodeKind::Scope(p)) =
                (&self.states[ni], &n.kind)
            {
                let len = buf.len();
                let start = (*idx + len - *filled) % len.max(1);
                let samples = (0..*filled).map(|k| buf[(start + k) % len]).collect();
                scopes.insert(
                    n.flat,
                    ScopeTrace {
                        count: p.count.clamp(1, 4) as usize,
                        samples,
                    },
                );
            }
        }
        GraphProbe {
            tick: self.tick,
            nodes,
            scopes,
        }
    }
}

struct Ctx {
    dt: f32,
    time: f64,
    alarm: AlarmSample,
}

fn xorshift(state: &mut u32) -> f32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x.max(1);
    (x as f32 / u32::MAX as f32) * 2.0 - 1.0
}

fn lfo_shape(shape: LfoShape, p: f32, width: f32, cycles: f64, held: f32, seed: u32) -> f32 {
    match shape {
        LfoShape::Sine => (TAU * p).sin(),
        LfoShape::Triangle => 2.0 * (2.0 * p - 1.0).abs() - 1.0,
        LfoShape::Saw => 2.0 * p - 1.0,
        LfoShape::Square => {
            if p < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        LfoShape::Pulse => {
            if p < width {
                1.0
            } else {
                -1.0
            }
        }
        LfoShape::Noise => {
            let x = (cycles * 2.0) as f32 + seed as f32 * 17.31;
            (noise::fractal_1d(x) * 2.0).clamp(-1.0, 1.0)
        }
        LfoShape::SampleHold => held,
    }
}

#[allow(clippy::too_many_arguments)]
fn eval_node(
    node: &compile::CNode,
    ins: &[f32],
    outs: &mut [f32],
    state: &mut NodeState,
    external: &[ExternalSample],
    frames: &mut [DeviceFrame],
    panic: &mut bool,
    ctx: &Ctx,
) {
    let dt = ctx.dt;
    let i = |k: usize| ins.get(k).copied().unwrap_or(0.0);
    match &node.kind {
        NodeKind::OscFloat(_) | NodeKind::Contact(_) => {
            if let Extra::External(e) = node.extra
                && let Some(s) = external.get(e)
            {
                outs[0] = s.value;
                outs[1] = from_bool(s.fresh);
            }
        }
        NodeKind::Constant(p) => outs[0] = p.value,
        NodeKind::Time(_) => {
            outs[0] = ctx.time as f32;
            outs[1] = dt;
        }
        NodeKind::Slider(p) => outs[0] = p.value.clamp(p.min.min(p.max), p.max.max(p.min)),
        NodeKind::Toggle(p) => outs[0] = from_bool(p.on),
        NodeKind::Alarm(_) => {
            outs[0] = ctx.alarm.strength.clamp(0.0, 1.0);
            outs[1] = from_bool(ctx.alarm.active);
        }
        NodeKind::Ukf(p) => {
            if let NodeState::Ukf { filter, t } = state {
                *t += dt.max(1e-4);
                let s = filter.update(*t, i(0));
                let norms = p.norms.sanitised();
                outs[0] = s.position.clamp(0.0, 1.0);
                outs[1] = s.velocity;
                outs[2] = s.acceleration;
                outs[3] = s.jerk.abs();
                outs[4] = (s.velocity.abs() / norms.speed).clamp(0.0, 1.0);
                outs[5] = (s.acceleration.abs() / norms.acc).clamp(0.0, 1.0);
                outs[6] = (s.jerk.abs() / norms.recoil).clamp(0.0, 1.0);
            }
        }
        NodeKind::Derivative(_) => {
            if let NodeState::Prev { prev, init } = state {
                outs[0] = if *init && dt > 0.0 { (i(0) - *prev) / dt } else { 0.0 };
                *prev = i(0);
                *init = true;
            }
        }
        NodeKind::Integrate(p) => {
            if let NodeState::Value { y, .. } = state {
                let (lo, hi) = (p.min.min(p.max), p.max.max(p.min));
                if as_bool(i(1)) {
                    *y = 0.0f32.clamp(lo, hi);
                } else {
                    *y += (i(0) - p.leak * *y) * dt;
                    *y = y.clamp(lo, hi);
                }
                outs[0] = *y;
            }
        }
        NodeKind::Smooth(p) => {
            if let NodeState::Value { y, init } = state {
                let x = i(0);
                if !*init {
                    *y = x;
                    *init = true;
                }
                let tau = if x > *y { p.attack_ms } else { p.release_ms } / 1000.0;
                if tau <= 0.0 {
                    *y = x;
                } else {
                    *y += (x - *y) * (1.0 - (-dt / tau).exp());
                }
                outs[0] = *y;
            }
        }
        NodeKind::Slew(_) => {
            if let NodeState::Value { y, .. } = state {
                let delta = i(0) - *y;
                *y += if delta > 0.0 {
                    delta.min(i(1).max(0.0) * dt)
                } else {
                    delta.max(-i(2).max(0.0) * dt)
                };
                outs[0] = *y;
            }
        }
        NodeKind::MovingAverage(_) => {
            if let NodeState::Window { buf, idx, filled, sum } = state
                && !buf.is_empty()
            {
                let x = i(0);
                *sum -= buf[*idx] as f64;
                buf[*idx] = x;
                *sum += x as f64;
                *idx = (*idx + 1) % buf.len();
                if *idx == 0 {
                    *sum = buf.iter().map(|v| *v as f64).sum();
                }
                *filled = (*filled + 1).min(buf.len());
                outs[0] = (*sum / *filled as f64) as f32;
            }
        }
        NodeKind::PeakHold(_) => {
            if let NodeState::Peak { value, held } = state {
                let x = i(0);
                if x >= *value {
                    *value = x;
                    *held = 0.0;
                } else {
                    *held += dt;
                    if *held >= i(1).max(0.0) {
                        *value = (*value - i(2).max(0.0) * dt).max(x);
                    }
                }
                outs[0] = *value;
            }
        }
        NodeKind::Normalize(p) => {
            let x = if p.abs { i(0).abs() } else { i(0) };
            let v = x / p.divisor;
            outs[0] = match (p.clamp, p.abs) {
                (false, _) => v,
                (true, true) => v.clamp(0.0, 1.0),
                (true, false) => v.clamp(-1.0, 1.0),
            };
        }
        NodeKind::SpoofGuard(p) => {
            if let NodeState::Guard { blocked, last, init } = state {
                let x = i(0);
                let was_idle = !*init || *last <= p.release_level;
                *blocked = if x <= p.release_level {
                    false
                } else if was_idle {
                    x > p.jump_limit
                } else {
                    *blocked
                };
                *last = x;
                *init = true;
                outs[0] = if *blocked { 0.0 } else { x };
                outs[1] = from_bool(*blocked);
            }
        }
        NodeKind::Math(p) => outs[0] = p.op.apply(i(0), i(1)),
        NodeKind::Clamp(_) => {
            let (lo, hi) = (i(1).min(i(2)), i(1).max(i(2)));
            outs[0] = i(0).clamp(lo, hi);
        }
        NodeKind::Remap(p) => {
            let span = i(2) - i(1);
            let mut t = if span.abs() < 1e-9 { 0.0 } else { (i(0) - i(1)) / span };
            if p.clamp {
                t = t.clamp(0.0, 1.0);
            }
            outs[0] = i(3) + t * (i(4) - i(3));
        }
        NodeKind::Mix(_) => outs[0] = i(0) + (i(1) - i(0)) * i(2),
        NodeKind::Threshold(_) => {
            let (x, lo, hi) = (i(0), i(1), i(2));
            outs[0] = if lo <= 0.01 && hi >= 1.0 {
                x
            } else if x <= lo {
                0.0
            } else if hi <= lo {
                1.0
            } else {
                ((x - lo) / (hi - lo)).clamp(0.0, 1.0)
            };
        }
        NodeKind::Curve(p) => outs[0] = p.curve.eval(i(0)),
        NodeKind::Function(_) => {
            if let Extra::Modulation(cfg) = &node.extra {
                outs[0] = evaluate_with_accumulator(cfg, i(1), i(0));
            }
        }
        NodeKind::Combine(p) => {
            let n = ins.len();
            outs[0] = match p.mode {
                CombineMode::Max => ins.iter().copied().fold(f32::NEG_INFINITY, f32::max),
                CombineMode::Sum => ins.iter().sum::<f32>().min(1.0),
                CombineMode::Average => (ins.iter().sum::<f32>() / n.max(1) as f32).min(1.0),
                CombineMode::Weighted => ins
                    .iter()
                    .enumerate()
                    .map(|(k, x)| x * p.weights.get(k).copied().unwrap_or(1.0))
                    .sum(),
            };
        }
        NodeKind::Lfo(p) => {
            if let NodeState::Lfo { phase, cycles, done, last_reset, held, rng } = state {
                let reset = as_bool(i(4));
                if reset && !*last_reset {
                    *phase = 0.0;
                    *cycles = 0.0;
                    *done = false;
                }
                *last_reset = reset;
                let hz = i(0).max(0.0);
                if !(p.one_shot && *done) {
                    *phase += hz * dt;
                    *cycles += (hz * dt) as f64;
                    if *phase >= 1.0 {
                        if p.one_shot {
                            *phase = 1.0;
                            *done = true;
                        } else {
                            *phase -= phase.floor();
                            *held = xorshift(rng);
                        }
                    }
                }
                let at = (*phase + p.phase).rem_euclid(1.0);
                let v = lfo_shape(p.shape, at, i(3).clamp(0.0, 1.0), *cycles, *held, p.seed);
                outs[0] = i(2) + i(1) * v;
                outs[1] = at;
                outs[2] = from_bool(*done);
            }
        }
        NodeKind::Noise(p) => {
            if let NodeState::Noise { pos } = state {
                *pos += (i(0).max(0.0) * dt) as f64;
                let x = (*pos % 100_000.0) as f32 + p.seed as f32 * 37.13;
                let v = match p.kind {
                    NoiseKind::Perlin => noise::perlin_1d(x) * 2.0,
                    NoiseKind::Simplex => noise::simplex_1d(x),
                    NoiseKind::Value => noise::value_noise_1d(x),
                    NoiseKind::Fractal => noise::fractal_1d(x) * 2.0,
                }
                .clamp(-1.0, 1.0);
                outs[0] = v;
                outs[1] = (v + 1.0) * 0.5;
            }
        }
        NodeKind::Envelope(p) => {
            if let NodeState::Env { level, stage, gate, rel_rate } = state {
                let g = as_bool(i(0));
                let (a, d, s, r) = (i(1).max(0.0), i(2).max(0.0), i(3).clamp(0.0, 1.0), i(4).max(0.0));
                if g && !*gate {
                    *stage = 1;
                }
                if !g && *gate {
                    *stage = 4;
                    *rel_rate = *level / r.max(1e-3);
                }
                *gate = g;
                match *stage {
                    1 => {
                        *level = if a <= 0.0 { 1.0 } else { *level + dt / a };
                        if *level >= 1.0 {
                            *level = 1.0;
                            *stage = if p.mode == EnvMode::Adsr { 2 } else { 3 };
                        }
                    }
                    2 => {
                        *level -= dt * (1.0 - s) / d.max(1e-3);
                        if *level <= s {
                            *level = s;
                            *stage = 3;
                        }
                    }
                    3 => *level = if p.mode == EnvMode::Adsr { s } else { 1.0 },
                    4 => {
                        *level -= *rel_rate * dt;
                        if *level <= 0.0 || r <= 0.0 {
                            *level = 0.0;
                            *stage = 0;
                        }
                    }
                    _ => *level = 0.0,
                }
                outs[0] = *level;
            }
        }
        NodeKind::SampleHold(_) => {
            if let NodeState::Hold { value, last_trig } = state {
                let t = as_bool(i(1));
                if t && !*last_trig {
                    *value = i(0);
                }
                *last_trig = t;
                outs[0] = *value;
            }
        }
        NodeKind::Schmitt(_) => {
            if let NodeState::Flag(on) = state {
                if !*on && i(0) > i(1) {
                    *on = true;
                } else if *on && i(0) < i(2) {
                    *on = false;
                }
                outs[0] = from_bool(*on);
            }
        }
        NodeKind::Metronome(_) => {
            if let NodeState::Metro { phase } = state {
                let hz = i(0).max(0.0);
                let beat = (0.1 * hz).min(0.5);
                outs[0] = from_bool(*phase < i(1).clamp(0.0, 1.0));
                outs[1] = from_bool(*phase < beat);
                *phase += hz * dt;
                if *phase >= 1.0 {
                    *phase -= phase.floor();
                }
            }
        }
        NodeKind::Delay(_) => {
            if let NodeState::Delay { buf, idx } = state {
                outs[0] = buf.get(*idx).copied().unwrap_or(0.0);
            }
        }
        NodeKind::Timer(_) => {
            if let NodeState::Timer { elapsed } = state {
                if as_bool(i(0)) {
                    *elapsed = (*elapsed + dt).min(86_400.0);
                } else {
                    *elapsed = 0.0;
                }
                let (hold, ramp) = (i(1).max(0.0), i(2).max(0.0));
                outs[0] = if *elapsed < hold || !as_bool(i(0)) {
                    0.0
                } else if ramp <= 0.0 {
                    1.0
                } else {
                    ((*elapsed - hold) / ramp).clamp(0.0, 1.0)
                };
                outs[1] = *elapsed;
            }
        }
        NodeKind::Sequencer(p) => {
            if let NodeState::Seq { phase, index, last_reset } = state {
                let n = p.steps.len().max(1);
                let reset = as_bool(i(1));
                if reset && !*last_reset {
                    *phase = 0.0;
                    *index = 0;
                }
                *last_reset = reset;
                *phase += i(0).max(0.0) * dt;
                let mut guard = 0;
                while *phase >= 1.0 && guard < 64 {
                    *phase -= 1.0;
                    *index = (*index + 1) % n;
                    guard += 1;
                }
                if *phase >= 1.0 {
                    *phase -= phase.floor();
                }
                *index %= n;
                outs[0] = p.steps.get(*index).copied().unwrap_or(0.0);
                outs[1] = *index as f32;
            }
        }
        NodeKind::Compare(p) => {
            if let NodeState::Flag(on) = state {
                let (a, b, h) = (i(0), i(1), p.hysteresis * 0.5);
                *on = match p.op {
                    CompareOp::Greater => if *on { a > b - h } else { a > b + h },
                    CompareOp::GreaterEq => if *on { a >= b - h } else { a >= b + h },
                    CompareOp::Less => if *on { a < b + h } else { a < b - h },
                    CompareOp::LessEq => if *on { a <= b + h } else { a <= b - h },
                    CompareOp::Equal => (a - b).abs() <= h.max(1e-6),
                };
                outs[0] = from_bool(*on);
            }
        }
        NodeKind::Logic(p) => {
            let (a, b) = (as_bool(i(0)), as_bool(i(1)));
            outs[0] = from_bool(match p.op {
                LogicOp::And => a && b,
                LogicOp::Or => a || b,
                LogicOp::Xor => a ^ b,
                LogicOp::Not => !a,
            });
        }
        NodeKind::Switch(_) => {
            let n = ins.len().saturating_sub(1).max(1);
            let k = (i(0).floor().max(0.0) as usize).min(n - 1);
            outs[0] = i(1 + k);
        }
        NodeKind::Crossfade(p) => {
            let t = i(2).clamp(0.0, 1.0);
            outs[0] = if p.equal_power {
                i(0) * (t * FRAC_PI_2).cos() + i(1) * (t * FRAC_PI_2).sin()
            } else {
                i(0) + (i(1) - i(0)) * t
            };
        }
        NodeKind::Router(_) => {
            let n = outs.len().max(1);
            let k = (i(1).floor().max(0.0) as usize).min(n - 1);
            for (j, o) in outs.iter_mut().enumerate() {
                *o = if j == k { i(0) } else { 0.0 };
            }
        }
        NodeKind::Reroute(_) | NodeKind::GroupInput(_) | NodeKind::GroupOutput(_) => {
            for (o, x) in outs.iter_mut().zip(ins) {
                *o = *x;
            }
        }
        NodeKind::HardLimit(_) => outs[0] = i(0).clamp(0.0, i(1).max(0.0)),
        NodeKind::RampLimiter(_) => {
            if let NodeState::Value { y, .. } = state {
                let x = i(0);
                *y = if x > *y { x.min(*y + i(1).max(0.0) * dt) } else { x };
                outs[0] = *y;
            }
        }
        NodeKind::Deadman(_) => {
            if let NodeState::Deadman { last, unchanged, stale, init } = state {
                let x = i(0);
                if !*init || (x - *last).abs() > 1e-6 {
                    *unchanged = 0.0;
                    *last = x;
                    *init = true;
                } else {
                    *unchanged = (*unchanged + dt).min(86_400.0);
                }
                if as_bool(i(1)) {
                    *stale = 0.0;
                } else {
                    *stale = (*stale + dt).min(86_400.0);
                }
                let timeout = i(2).max(0.0);
                outs[0] = if *unchanged >= timeout || *stale >= timeout { 0.0 } else { x };
            }
        }
        NodeKind::Panic(p) => {
            outs[0] = from_bool(p.latched);
            if p.latched {
                *panic = true;
            }
        }
        NodeKind::Scope(_) => {
            if let NodeState::Scope { buf, idx, filled } = state
                && !buf.is_empty()
            {
                let mut s = [0.0f32; 4];
                for (k, v) in ins.iter().take(4).enumerate() {
                    s[k] = if v.is_finite() { *v } else { 0.0 };
                }
                buf[*idx] = s;
                *idx = (*idx + 1) % buf.len();
                *filled = (*filled + 1).min(buf.len());
            }
        }
        NodeKind::Device(p) => {
            let (Extra::Device(fi), NodeState::Device { power, freq, int, init }) = (&node.extra, state)
            else {
                return;
            };
            let Some(frame) = frames.get_mut(*fi) else {
                return;
            };
            let profile = p.profile();
            let mut k = 0usize;
            for (ci, ch) in profile.channels.iter().enumerate().take(MAX_CHANNELS) {
                let cp = p.channels.get(ci).unwrap_or(&DEFAULT_CHANNEL);
                let cf = &mut frame.channels[ci];
                for g in ch.groups {
                    let expanded = g.expandable && cp.is_expanded(g.key);
                    let width = if expanded { SEGMENTS } else { 1 };
                    let v0 = i(k);
                    match g.role {
                        PinRole::Power => {
                            let v = v0.clamp(0.0, 1.0);
                            let hist = &mut power[ci];
                            if *init {
                                hist.copy_within(1.., 0);
                                hist[SEGMENTS - 1] = v;
                            } else {
                                *hist = [v; SEGMENTS];
                            }
                            cf.power_seg = *hist;
                            cf.power = hist.iter().copied().fold(0.0, f32::max);
                        }
                        PinRole::LimitMin => cf.limit_min = v0,
                        PinRole::LimitMax => cf.limit_max = v0,
                        PinRole::Ceiling => cf.ceiling = v0,
                        PinRole::Frequency | PinRole::Intensity => {
                            let is_freq = g.role == PinRole::Frequency;
                            let map = |v: f32| {
                                if is_freq { cp.map_frequency(v) } else { cp.map_intensity(v) }
                            };
                            let hist = if is_freq { &mut freq[ci] } else { &mut int[ci] };
                            if expanded {
                                for (s, h) in hist.iter_mut().enumerate() {
                                    *h = map(i(k + s));
                                }
                            } else {
                                let v = map(v0);
                                if *init {
                                    hist.copy_within(1.., 0);
                                    hist[SEGMENTS - 1] = v;
                                } else {
                                    *hist = [v; SEGMENTS];
                                }
                            }
                            if is_freq {
                                cf.freq = *hist;
                            } else {
                                cf.intensity = *hist;
                            }
                        }
                    }
                    k += width;
                }
            }
            *init = true;
        }
        NodeKind::Frame(_) | NodeKind::Group(_) | NodeKind::Missing(_) => {}
    }
}
