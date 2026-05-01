//! Unscented Kalman Filter for 1-D contact depth signal.
//!
//! State vector: `x = [p, v, a, j]^T`  (position, velocity, acceleration, jerk)
//! Process model (constant-jerk):
//!   p_{k+1} = p + v*dt + 0.5*a*dt^2 + (1/6)*j*dt^3
//!   v_{k+1} = v + a*dt + 0.5*j*dt^2
//!   a_{k+1} = a + j*dt
//!   j_{k+1} = j
//! Measurement model: z = p (we only observe position / contact depth).
//!
//! `Q` (process noise covariance) is built from a single scalar `q`
//! (spectral density of the jerk derivative — i.e. how "wobbly" the motion is).
//! `R` is a scalar measurement-noise variance.
//!
//! This is a true UKF (not an EKF/KF) per requirement, even though the model
//! happens to be linear — UKF reduces to KF in the linear case but the
//! sigma-point machinery is implemented honestly so it can be swapped to a
//! non-linear measurement later (e.g. `z = clamp(p,0,1)`).

const N: usize = 4;
const SIGMA_COUNT: usize = 2 * N + 1;

#[derive(Debug, Clone, Copy)]
pub struct UkfParams {
    pub q: f32,
    pub r: f32,
    pub alpha: f32,
    pub beta: f32,
    pub kappa: f32,
}

impl Default for UkfParams {
    fn default() -> Self {
        Self {
            q: 50.0,
            r: 0.0025,
            alpha: 0.5,
            beta: 2.0,
            kappa: 0.0,
        }
    }
}

type Mat4 = [[f32; N]; N];
type Vec4 = [f32; N];

#[inline]
fn mat4_zero() -> Mat4 {
    [[0.0; N]; N]
}

#[inline]
fn vec4_zero() -> Vec4 {
    [0.0; N]
}

fn cholesky4(m: &Mat4) -> Mat4 {
    let mut l = mat4_zero();
    for i in 0..N {
        for j in 0..=i {
            let mut sum = m[i][j];
            for k in 0..j {
                sum -= l[i][k] * l[j][k];
            }
            if i == j {
                l[i][j] = sum.max(1e-9).sqrt();
            } else {
                l[i][j] = sum / l[j][j];
            }
        }
    }
    l
}

fn sigma_points(x: &Vec4, p: &Mat4, lambda: f32) -> [Vec4; SIGMA_COUNT] {
    let scale = (N as f32 + lambda).max(1e-9);
    let mut scaled = mat4_zero();
    for i in 0..N {
        for j in 0..N {
            scaled[i][j] = scale * p[i][j];
        }
    }
    let l = cholesky4(&scaled);

    let mut pts = [vec4_zero(); SIGMA_COUNT];
    pts[0] = *x;
    for i in 0..N {
        let col = [l[0][i], l[1][i], l[2][i], l[3][i]];
        for k in 0..N {
            pts[1 + i][k] = x[k] + col[k];
            pts[1 + N + i][k] = x[k] - col[k];
        }
    }
    pts
}

fn weights(lambda: f32, alpha: f32, beta: f32) -> ([f32; SIGMA_COUNT], [f32; SIGMA_COUNT]) {
    let denom = (N as f32 + lambda).max(1e-9);
    let mut wm = [0.0f32; SIGMA_COUNT];
    let mut wc = [0.0f32; SIGMA_COUNT];
    wm[0] = lambda / denom;
    wc[0] = lambda / denom + (1.0 - alpha * alpha + beta);
    let w_other = 0.5 / denom;
    for i in 1..SIGMA_COUNT {
        wm[i] = w_other;
        wc[i] = w_other;
    }
    (wm, wc)
}

/// Constant-jerk propagation of a single sigma point.
fn propagate(x: &Vec4, dt: f32) -> Vec4 {
    let (p, v, a, j) = (x[0], x[1], x[2], x[3]);
    let dt2 = dt * dt;
    let dt3 = dt2 * dt;
    [
        p + v * dt + 0.5 * a * dt2 + (1.0 / 6.0) * j * dt3,
        v + a * dt + 0.5 * j * dt2,
        a + j * dt,
        j,
    ]
}

/// Build process-noise Q for constant-jerk model from scalar spectral density `q`.
/// Standard discrete-time approximation: Q = q * G * G^T with G = [dt^3/6, dt^2/2, dt, 1]^T.
fn build_q(q_scalar: f32, dt: f32) -> Mat4 {
    let dt2 = dt * dt;
    let dt3 = dt2 * dt;
    let g = [dt3 / 6.0, dt2 / 2.0, dt, 1.0];
    let mut out = mat4_zero();
    for i in 0..N {
        for j in 0..N {
            out[i][j] = q_scalar * g[i] * g[j];
        }
    }
    out
}

/// 1-D UKF tracking position with hidden velocity / acceleration / jerk.
#[derive(Debug, Clone)]
pub struct UnscentedKalman {
    pub params: UkfParams,
    x: Vec4,
    p: Mat4,
    initialised: bool,
    last_time: f32,
}

impl UnscentedKalman {
    pub fn new(params: UkfParams) -> Self {
        let mut p = mat4_zero();
        p[0][0] = 1.0;
        p[1][1] = 10.0;
        p[2][2] = 100.0;
        p[3][3] = 1000.0;
        Self {
            params,
            x: vec4_zero(),
            p,
            initialised: false,
            last_time: 0.0,
        }
    }

    pub fn set_params(&mut self, params: UkfParams) {
        self.params = params;
    }

    /// Reset state and force re-initialisation on next update.
    pub fn reset(&mut self) {
        self.x = vec4_zero();
        self.p = mat4_zero();
        self.p[0][0] = 1.0;
        self.p[1][1] = 10.0;
        self.p[2][2] = 100.0;
        self.p[3][3] = 1000.0;
        self.initialised = false;
        self.last_time = 0.0;
    }

    /// Run a single predict + update step.
    /// `time` is in seconds. Returns the posterior state estimate.
    pub fn update(&mut self, time: f32, z: f32) -> UkfState {
        if !self.initialised {
            self.x[0] = z;
            self.x[1] = 0.0;
            self.x[2] = 0.0;
            self.x[3] = 0.0;
            self.last_time = time;
            self.initialised = true;
            return UkfState {
                position: z,
                velocity: 0.0,
                acceleration: 0.0,
                jerk: 0.0,
            };
        }

        let dt = (time - self.last_time).max(1e-4);
        self.last_time = time;

        let dt = dt.min(0.5);

        let lambda = self.params.alpha * self.params.alpha * (N as f32 + self.params.kappa) - N as f32;
        let (wm, wc) = weights(lambda, self.params.alpha, self.params.beta);

        let pts = sigma_points(&self.x, &self.p, lambda);
        let mut prop = [vec4_zero(); SIGMA_COUNT];
        for i in 0..SIGMA_COUNT {
            prop[i] = propagate(&pts[i], dt);
        }

        let mut x_pred = vec4_zero();
        for i in 0..SIGMA_COUNT {
            for k in 0..N {
                x_pred[k] += wm[i] * prop[i][k];
            }
        }

        let q_mat = build_q(self.params.q, dt);
        let mut p_pred = q_mat;
        for i in 0..SIGMA_COUNT {
            let mut d = vec4_zero();
            for k in 0..N {
                d[k] = prop[i][k] - x_pred[k];
            }
            for r in 0..N {
                for c in 0..N {
                    p_pred[r][c] += wc[i] * d[r] * d[c];
                }
            }
        }

        let pts2 = sigma_points(&x_pred, &p_pred, lambda);
        let mut z_sig = [0.0f32; SIGMA_COUNT];
        for i in 0..SIGMA_COUNT {
            z_sig[i] = pts2[i][0];
        }

        let mut z_pred = 0.0f32;
        for i in 0..SIGMA_COUNT {
            z_pred += wm[i] * z_sig[i];
        }

        let mut s = self.params.r;
        for i in 0..SIGMA_COUNT {
            let dz = z_sig[i] - z_pred;
            s += wc[i] * dz * dz;
        }

        let mut cxz = vec4_zero();
        for i in 0..SIGMA_COUNT {
            let dz = z_sig[i] - z_pred;
            for k in 0..N {
                cxz[k] += wc[i] * (pts2[i][k] - x_pred[k]) * dz;
            }
        }

        let s_inv = if s.abs() < 1e-12 { 0.0 } else { 1.0 / s };
        let k_gain: Vec4 = [cxz[0] * s_inv, cxz[1] * s_inv, cxz[2] * s_inv, cxz[3] * s_inv];

        let innov = z - z_pred;
        for k in 0..N {
            self.x[k] = x_pred[k] + k_gain[k] * innov;
        }

        for r in 0..N {
            for c in 0..N {
                self.p[r][c] = p_pred[r][c] - k_gain[r] * s * k_gain[c];
            }
        }

        UkfState {
            position: self.x[0],
            velocity: self.x[1],
            acceleration: self.x[2],
            jerk: self.x[3],
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UkfState {
    pub position: f32,
    pub velocity: f32,
    pub acceleration: f32,
    pub jerk: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_constant_velocity() {
        let mut ukf = UnscentedKalman::new(UkfParams::default());
        let mut t = 0.0f32;
        let v_true = 0.5f32;
        let mut last = UkfState::default();
        for i in 0..200 {
            t += 0.02;
            let p = v_true * t;
            last = ukf.update(t, p);
            if i > 100 {
                assert!((last.velocity - v_true).abs() < 0.05, "v={} expected {}", last.velocity, v_true);
            }
        }
        assert!(last.acceleration.abs() < 0.5);
    }

    #[test]
    fn smooths_noisy_position() {
        use rand::Rng;
        let mut rng = rand::rng();
        let mut ukf = UnscentedKalman::new(UkfParams {
            q: 1.0,
            r: 0.01,
            ..Default::default()
        });
        let mut t = 0.0f32;
        let mut max_err = 0.0f32;
        for _ in 0..400 {
            t += 0.02;
            let truth = 0.5 + 0.3 * (t * 1.5).sin();
            let noisy = truth + (rng.random::<f32>() - 0.5) * 0.2;
            let s = ukf.update(t, noisy);
            if t > 1.0 {
                max_err = max_err.max((s.position - truth).abs());
            }
        }
        assert!(max_err < 0.1, "max err={max_err}");
    }
}
