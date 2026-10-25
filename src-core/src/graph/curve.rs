use serde::{Deserialize, Serialize};

pub const MAX_POINTS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CurveInterp {
    #[default]
    Linear,
    Smooth,
    Step,
}

impl CurveInterp {
    pub const ALL: [CurveInterp; 3] = [CurveInterp::Linear, CurveInterp::Smooth, CurveInterp::Step];

    pub fn label(self) -> &'static str {
        match self {
            CurveInterp::Linear => "linear",
            CurveInterp::Smooth => "smooth",
            CurveInterp::Step => "step",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurvePreset {
    Linear,
    EaseIn,
    EaseOut,
    SCurve,
    Exponential,
    Log,
}

impl CurvePreset {
    pub const ALL: [CurvePreset; 6] = [
        CurvePreset::Linear,
        CurvePreset::EaseIn,
        CurvePreset::EaseOut,
        CurvePreset::SCurve,
        CurvePreset::Exponential,
        CurvePreset::Log,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CurvePreset::Linear => "linear",
            CurvePreset::EaseIn => "ease-in",
            CurvePreset::EaseOut => "ease-out",
            CurvePreset::SCurve => "S-curve",
            CurvePreset::Exponential => "exp",
            CurvePreset::Log => "log",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Curve {
    pub points: Vec<[f32; 2]>,
    pub interp: CurveInterp,
}

impl Default for Curve {
    fn default() -> Self {
        Self::preset(CurvePreset::Linear)
    }
}

impl Curve {
    pub fn preset(p: CurvePreset) -> Self {
        let sampled = |f: fn(f32) -> f32| -> Vec<[f32; 2]> {
            (0..=8)
                .map(|i| {
                    let x = i as f32 / 8.0;
                    [x, f(x)]
                })
                .collect()
        };
        match p {
            CurvePreset::Linear => Self {
                points: vec![[0.0, 0.0], [1.0, 1.0]],
                interp: CurveInterp::Linear,
            },
            CurvePreset::SCurve => Self {
                points: vec![[0.0, 0.0], [1.0, 1.0]],
                interp: CurveInterp::Smooth,
            },
            CurvePreset::EaseIn => Self {
                points: sampled(|x| x * x),
                interp: CurveInterp::Linear,
            },
            CurvePreset::EaseOut => Self {
                points: sampled(|x| 1.0 - (1.0 - x) * (1.0 - x)),
                interp: CurveInterp::Linear,
            },
            CurvePreset::Exponential => Self {
                points: sampled(|x| ((4.0 * x).exp() - 1.0) / (4.0f32.exp() - 1.0)),
                interp: CurveInterp::Linear,
            },
            CurvePreset::Log => Self {
                points: sampled(|x| (1.0 + 9.0 * x).ln() / 10.0f32.ln()),
                interp: CurveInterp::Linear,
            },
        }
    }

    pub fn sanitize(&mut self) {
        self.points.retain(|p| p[0].is_finite() && p[1].is_finite());
        for p in &mut self.points {
            p[0] = p[0].clamp(0.0, 1.0);
            p[1] = p[1].clamp(0.0, 1.0);
        }
        self.points.sort_by(|a, b| a[0].total_cmp(&b[0]));
        self.points.truncate(MAX_POINTS);
        if self.points.len() < 2 {
            *self = Self::default();
        }
    }

    pub fn eval(&self, x: f32) -> f32 {
        let pts = &self.points;
        match pts.len() {
            0 => return 0.0,
            1 => return pts[0][1],
            _ => {}
        }
        if !x.is_finite() || x <= pts[0][0] {
            return pts[0][1];
        }
        let last = pts[pts.len() - 1];
        if x >= last[0] {
            return last[1];
        }
        let i = pts.windows(2).position(|w| x < w[1][0]).unwrap_or(pts.len() - 2);
        let (a, b) = (pts[i], pts[i + 1]);
        let span = b[0] - a[0];
        if span <= 1e-6 {
            return b[1];
        }
        let t = (x - a[0]) / span;
        match self.interp {
            CurveInterp::Linear => a[1] + (b[1] - a[1]) * t,
            CurveInterp::Smooth => {
                let s = t * t * (3.0 - 2.0 * t);
                a[1] + (b[1] - a[1]) * s
            }
            CurveInterp::Step => a[1],
        }
    }

    pub fn add_point(&mut self, x: f32, y: f32) -> Option<usize> {
        if self.points.len() >= MAX_POINTS {
            return None;
        }
        let x = x.clamp(0.0, 1.0);
        let y = y.clamp(0.0, 1.0);
        let i = self.points.iter().position(|p| p[0] > x).unwrap_or(self.points.len());
        self.points.insert(i, [x, y]);
        Some(i)
    }

    pub fn move_point(&mut self, i: usize, x: f32, y: f32) {
        let n = self.points.len();
        if i >= n {
            return;
        }
        let lo = if i == 0 { 0.0 } else { self.points[i - 1][0] };
        let hi = if i + 1 == n { 1.0 } else { self.points[i + 1][0] };
        let x = if i == 0 || i + 1 == n {
            self.points[i][0]
        } else {
            x.clamp(lo, hi)
        };
        self.points[i] = [x, y.clamp(0.0, 1.0)];
    }

    pub fn remove_point(&mut self, i: usize) {
        if self.points.len() > 2 && i > 0 && i + 1 < self.points.len() {
            self.points.remove(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_is_identity() {
        let c = Curve::default();
        for x in [0.0, 0.25, 0.5, 1.0] {
            assert!((c.eval(x) - x).abs() < 1e-6);
        }
    }

    #[test]
    fn presets_are_monotone_and_span_the_unit_range() {
        for p in CurvePreset::ALL {
            let c = Curve::preset(p);
            assert!(c.eval(0.0).abs() < 1e-4, "{p:?} starts at 0");
            assert!((c.eval(1.0) - 1.0).abs() < 1e-4, "{p:?} ends at 1");
            let mut prev = -1.0;
            for i in 0..=50 {
                let v = c.eval(i as f32 / 50.0);
                assert!(v >= prev - 1e-6, "{p:?} dips at {i}");
                prev = v;
            }
        }
    }

    #[test]
    fn out_of_range_and_nan_inputs_hold_the_ends() {
        let c = Curve::preset(CurvePreset::EaseIn);
        assert_eq!(c.eval(-3.0), 0.0);
        assert_eq!(c.eval(9.0), 1.0);
        assert_eq!(c.eval(f32::NAN), 0.0);
    }

    #[test]
    fn sanitize_sorts_and_repairs() {
        let mut c = Curve {
            points: vec![[0.9, 2.0], [f32::NAN, 0.0], [0.1, -1.0]],
            interp: CurveInterp::Step,
        };
        c.sanitize();
        assert_eq!(c.points, vec![[0.1, 0.0], [0.9, 1.0]]);
        let mut broken = Curve {
            points: vec![],
            interp: CurveInterp::Linear,
        };
        broken.sanitize();
        assert_eq!(broken.points.len(), 2);
    }

    #[test]
    fn end_points_keep_their_x_when_dragged() {
        let mut c = Curve::default();
        c.move_point(0, 0.5, 0.3);
        assert_eq!(c.points[0], [0.0, 0.3]);
        let i = c.add_point(0.5, 0.5).unwrap();
        c.move_point(i, 2.0, 0.5);
        assert!(c.points[i][0] <= 1.0);
    }
}
