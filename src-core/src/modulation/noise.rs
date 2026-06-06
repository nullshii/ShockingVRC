fn hash_1d(mut n: i32) -> f32 {
    n = (n << 13) ^ n;
    let nn = n.wrapping_mul(
        n.wrapping_mul(n.wrapping_mul(15731).wrapping_add(789221))
            .wrapping_add(1376312589),
    );
    1.0 - (nn & 0x7fffffff) as f32 / 1073741824.0
}

fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + t * (b - a)
}

pub fn perlin_1d(x: f32) -> f32 {
    let xi = x.floor() as i32;
    let xf = x - x.floor();
    let u = fade(xf);

    let g0 = hash_1d(xi);
    let g1 = hash_1d(xi + 1);

    lerp(g0 * xf, g1 * (xf - 1.0), u)
}

pub fn simplex_1d(x: f32) -> f32 {
    let i0 = x.floor() as i32;
    let i1 = i0 + 1;
    let x0 = x - i0 as f32;
    let x1 = x0 - 1.0;

    let mut t0 = 1.0 - x0 * x0;
    t0 = t0 * t0;
    let n0 = t0 * t0 * hash_1d(i0) * x0;

    let mut t1 = 1.0 - x1 * x1;
    t1 = t1 * t1;
    let n1 = t1 * t1 * hash_1d(i1) * x1;

    (n0 + n1) * 2.5
}

pub fn value_noise_1d(x: f32) -> f32 {
    let xi = x.floor() as i32;
    let xf = x - x.floor();
    let u = fade(xf);

    let v0 = hash_1d(xi);
    let v1 = hash_1d(xi + 1);
    lerp(v0, v1, u)
}

pub fn fractal_1d(x: f32) -> f32 {
    let mut value = 0.0f32;
    let mut amplitude = 1.0f32;
    let mut frequency = 1.0f32;
    let mut max_amp = 0.0f32;

    for _ in 0..4 {
        value += perlin_1d(x * frequency) * amplitude;
        max_amp += amplitude;
        amplitude *= 0.5;
        frequency *= 2.0;
    }

    value / max_amp
}
