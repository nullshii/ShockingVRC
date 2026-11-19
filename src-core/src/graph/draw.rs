use std::f32::consts::TAU;

use serde::{Deserialize, Serialize};

pub const MAX_DRAW_POINTS: usize = 256;
pub const MAX_STAMP_POINTS: usize = 32;
pub const MAX_STAMPS: usize = 16;
pub const MAX_PLACED: usize = 64;
pub const MIN_LENGTH_S: f32 = 0.25;
pub const MAX_LENGTH_S: f32 = 120.0;

fn fin(v: f32, d: f32) -> f32 {
    if v.is_finite() { v } else { d }
}

fn unit(v: f32) -> f32 {
    fin(v, 0.0).clamp(0.0, 1.0)
}

fn length(v: f32) -> f32 {
    fin(v, 4.0).clamp(MIN_LENGTH_S, MAX_LENGTH_S)
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DrawRect {
    pub t0: f32,
    pub t1: f32,
    pub lo: f32,
    pub hi: f32,
}

impl Default for DrawRect {
    fn default() -> Self {
        Self {
            t0: 0.0,
            t1: 1.0,
            lo: 0.0,
            hi: 1.0,
        }
    }
}

impl DrawRect {
    pub fn from_corners(a: [f32; 2], b: [f32; 2]) -> Self {
        Self {
            t0: a[0].min(b[0]),
            t1: a[0].max(b[0]),
            lo: a[1].min(b[1]),
            hi: a[1].max(b[1]),
        }
        .sanitized()
    }

    pub fn sanitized(self) -> Self {
        let (t0, t1) = (unit(self.t0), unit(self.t1));
        let (lo, hi) = (unit(self.lo), unit(self.hi));
        let mut r = Self {
            t0: t0.min(t1),
            t1: t0.max(t1),
            lo: lo.min(hi),
            hi: lo.max(hi),
        };
        if r.t1 - r.t0 < 0.01 {
            r.t1 = (r.t0 + 0.01).min(1.0);
            r.t0 = r.t1 - 0.01;
        }
        r
    }

    pub fn local_t(&self, t: f32) -> Option<f32> {
        let span = self.t1 - self.t0;
        (span > 0.0 && t >= self.t0 && t <= self.t1).then(|| ((t - self.t0) / span).clamp(0.0, 1.0))
    }

    pub fn level(&self, v: f32) -> f32 {
        self.lo + (self.hi - self.lo) * unit(v)
    }
}

pub fn sample_points(points: &[[f32; 2]], x: f32) -> f32 {
    match points {
        [] => 0.0,
        [only] => only[1],
        _ => {
            if x <= points[0][0] {
                return points[0][1];
            }
            for w in points.windows(2) {
                let (a, b) = (w[0], w[1]);
                if x <= b[0] {
                    let span = b[0] - a[0];
                    let k = if span <= 1e-6 { 1.0 } else { (x - a[0]) / span };
                    return a[1] + (b[1] - a[1]) * k;
                }
            }
            points[points.len() - 1][1]
        }
    }
}

pub fn sample_levels(levels: &[f32], x: f32) -> f32 {
    match levels.len() {
        0 => 0.0,
        1 => levels[0],
        n => {
            let pos = unit(x) * (n - 1) as f32;
            let i = (pos.floor() as usize).min(n - 2);
            let k = pos - i as f32;
            levels[i] + (levels[i + 1] - levels[i]) * k
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DrawZoneParams {
    pub length_s: f32,
    pub zone: DrawRect,
    pub points: Vec<[f32; 2]>,
}

impl Default for DrawZoneParams {
    fn default() -> Self {
        Self {
            length_s: 4.0,
            zone: DrawRect {
                t0: 0.1,
                t1: 0.9,
                lo: 0.0,
                hi: 0.8,
            },
            points: vec![[0.0, 0.0], [0.3, 0.8], [0.6, 0.4], [1.0, 0.0]],
        }
    }
}

impl DrawZoneParams {
    pub fn sanitize(&mut self) {
        self.length_s = length(self.length_s);
        self.zone = self.zone.sanitized();
        self.points.retain(|p| p[0].is_finite() && p[1].is_finite());
        for p in &mut self.points {
            *p = [unit(p[0]), unit(p[1])];
        }
        self.points.sort_by(|a, b| a[0].total_cmp(&b[0]));
        self.points.truncate(MAX_DRAW_POINTS);
    }

    pub fn sample(&self, t: f32) -> (f32, bool) {
        match self.zone.local_t(t) {
            Some(x) => (self.zone.level(sample_points(&self.points, x)), true),
            None => (0.0, false),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FillShape {
    #[default]
    Sine,
    Triangle,
    Saw,
    Square,
    PulseTrain,
    RampUp,
    RampDown,
    RandomWalk,
}

impl FillShape {
    pub const ALL: [FillShape; 8] = [
        FillShape::Sine,
        FillShape::Triangle,
        FillShape::Saw,
        FillShape::Square,
        FillShape::PulseTrain,
        FillShape::RampUp,
        FillShape::RampDown,
        FillShape::RandomWalk,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FillShape::Sine => "sine",
            FillShape::Triangle => "triangle",
            FillShape::Saw => "saw",
            FillShape::Square => "square",
            FillShape::PulseTrain => "pulse train",
            FillShape::RampUp => "ramp up",
            FillShape::RampDown => "ramp down",
            FillShape::RandomWalk => "random walk",
        }
    }

    pub fn value(self, p: f32, duty: f32, walk: f32) -> f32 {
        let p = p.rem_euclid(1.0);
        let duty = unit(duty);
        match self {
            FillShape::Sine => 0.5 - 0.5 * (TAU * p).cos(),
            FillShape::Triangle => 1.0 - (2.0 * p - 1.0).abs(),
            FillShape::Saw | FillShape::RampUp => p,
            FillShape::RampDown => 1.0 - p,
            FillShape::Square => {
                if p < duty {
                    1.0
                } else {
                    0.0
                }
            }
            FillShape::PulseTrain => {
                let width = (duty * 0.25).max(0.02);
                if (p * 4.0).fract() < width * 4.0 { 1.0 } else { 0.0 }
            }
            FillShape::RandomWalk => unit(walk),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShapeFillParams {
    pub length_s: f32,
    pub zone: DrawRect,
    pub shape: FillShape,
    pub seed: u32,
}

impl Default for ShapeFillParams {
    fn default() -> Self {
        Self {
            length_s: 4.0,
            zone: DrawRect {
                t0: 0.0,
                t1: 1.0,
                lo: 0.1,
                hi: 0.7,
            },
            shape: FillShape::Sine,
            seed: 1,
        }
    }
}

impl ShapeFillParams {
    pub fn sanitize(&mut self) {
        self.length_s = length(self.length_s);
        self.zone = self.zone.sanitized();
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StampDef {
    pub name: String,
    pub levels: Vec<f32>,
}

impl Default for StampDef {
    fn default() -> Self {
        Self {
            name: "Pulse".into(),
            levels: vec![0.0, 1.0, 1.0, 0.0],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlacedStamp {
    pub stamp: u16,
    pub start: f32,
    pub len: f32,
    pub gain: f32,
}

impl Default for PlacedStamp {
    fn default() -> Self {
        Self {
            stamp: 0,
            start: 0.0,
            len: 0.125,
            gain: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StampParams {
    pub length_s: f32,
    pub snap: f32,
    pub stamps: Vec<StampDef>,
    pub lane: Vec<PlacedStamp>,
}

impl Default for StampParams {
    fn default() -> Self {
        Self {
            length_s: 4.0,
            snap: 0.0625,
            stamps: vec![
                StampDef::default(),
                StampDef {
                    name: "Swell".into(),
                    levels: vec![0.0, 0.3, 0.7, 1.0, 0.7, 0.3, 0.0],
                },
                StampDef {
                    name: "Spike".into(),
                    levels: vec![1.0, 0.4, 0.1, 0.0],
                },
            ],
            lane: vec![
                PlacedStamp {
                    stamp: 0,
                    start: 0.0,
                    len: 0.125,
                    gain: 0.8,
                },
                PlacedStamp {
                    stamp: 1,
                    start: 0.25,
                    len: 0.25,
                    gain: 1.0,
                },
                PlacedStamp {
                    stamp: 2,
                    start: 0.625,
                    len: 0.125,
                    gain: 0.9,
                },
            ],
        }
    }
}

impl StampParams {
    pub fn sanitize(&mut self) {
        self.length_s = length(self.length_s);
        self.snap = fin(self.snap, 0.0).clamp(0.0, 0.5);
        self.stamps.truncate(MAX_STAMPS);
        if self.stamps.is_empty() {
            self.stamps.push(StampDef::default());
        }
        for s in &mut self.stamps {
            s.levels.truncate(MAX_STAMP_POINTS);
            for v in &mut s.levels {
                *v = unit(*v);
            }
            if s.levels.is_empty() {
                s.levels.push(0.0);
            }
        }
        let n = self.stamps.len() as u16;
        self.lane.truncate(MAX_PLACED);
        for p in &mut self.lane {
            p.stamp = p.stamp.min(n - 1);
            p.start = unit(p.start);
            p.len = fin(p.len, 0.125).clamp(0.01, 1.0);
            p.gain = unit(p.gain);
        }
    }

    pub fn snapped(&self, v: f32) -> f32 {
        if self.snap > 0.0 {
            ((v / self.snap).round() * self.snap).clamp(0.0, 1.0)
        } else {
            unit(v)
        }
    }

    pub fn sample(&self, t: f32) -> f32 {
        let mut out = 0.0f32;
        for p in &self.lane {
            if p.len <= 0.0 || t < p.start || t >= p.start + p.len {
                continue;
            }
            let Some(def) = self.stamps.get(p.stamp as usize) else {
                continue;
            };
            out = out.max(p.gain * sample_levels(&def.levels, (t - p.start) / p.len));
        }
        out.clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct XyPadParams {
    pub x: f32,
    pub y: f32,
    pub touching: bool,
    pub spring: bool,
}

impl Default for XyPadParams {
    fn default() -> Self {
        Self {
            x: 0.5,
            y: 0.0,
            touching: false,
            spring: true,
        }
    }
}

impl XyPadParams {
    pub fn sanitize(&mut self) {
        self.x = unit(self.x);
        self.y = unit(self.y);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SegmentPainterParams {
    pub freq: [[f32; 4]; 2],
    pub int: [[f32; 4]; 2],
}

impl Default for SegmentPainterParams {
    fn default() -> Self {
        Self {
            freq: [[0.5; 4]; 2],
            int: [[1.0, 0.75, 0.5, 0.75]; 2],
        }
    }
}

impl SegmentPainterParams {
    pub fn sanitize(&mut self) {
        for row in self.freq.iter_mut().chain(self.int.iter_mut()) {
            for v in row.iter_mut() {
                *v = unit(*v);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FaderParams {
    pub level: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub pressed: bool,
}

impl Default for FaderParams {
    fn default() -> Self {
        Self {
            level: 0.5,
            attack: 0.3,
            decay: 0.3,
            sustain: 0.7,
            release: 0.8,
            pressed: false,
        }
    }
}

impl FaderParams {
    pub fn sanitize(&mut self) {
        self.level = unit(self.level);
        self.sustain = unit(self.sustain);
        for v in [&mut self.attack, &mut self.decay, &mut self.release] {
            *v = fin(*v, 0.2).clamp(0.0, 30.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zone_outputs_nothing_outside_its_span() {
        let p = DrawZoneParams::default();
        assert_eq!(p.sample(0.05), (0.0, false));
        assert_eq!(p.sample(0.95), (0.0, false));
        let (v, inside) = p.sample(0.1 + 0.8 * 0.3);
        assert!(inside);
        assert!((v - 0.8 * 0.8).abs() < 1e-4, "{v}");
    }

    #[test]
    fn rect_from_corners_orders_and_clamps() {
        let r = DrawRect::from_corners([0.9, 1.4], [0.2, -0.1]);
        assert_eq!((r.t0, r.t1, r.lo, r.hi), (0.2, 0.9, 0.0, 1.0));
        let thin = DrawRect::from_corners([0.5, 0.0], [0.5, 1.0]);
        assert!(thin.t1 > thin.t0);
    }

    #[test]
    fn shapes_stay_in_unit_range() {
        for s in FillShape::ALL {
            for k in 0..=40 {
                let v = s.value(k as f32 / 40.0, 0.4, 0.3);
                assert!((0.0..=1.0).contains(&v), "{s:?} {v}");
            }
        }
    }

    #[test]
    fn stamps_play_where_placed_and_snap() {
        let p = StampParams::default();
        assert_eq!(p.sample(0.2), 0.0, "gap between stamps");
        assert!(p.sample(0.375) > 0.9, "middle of the swell");
        assert_eq!(p.snapped(0.27), 0.25);
    }

    #[test]
    fn sanitising_drops_bad_points() {
        let mut p = DrawZoneParams {
            points: vec![[0.9, 0.2], [f32::NAN, 1.0], [0.1, 3.0]],
            ..Default::default()
        };
        p.sanitize();
        assert_eq!(p.points, vec![[0.1, 1.0], [0.9, 0.2]]);
    }
}
