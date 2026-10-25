use egui::{Color32, Pos2, Rect, Stroke, Ui};

use super::Palette;

const NEAR: f32 = 0.34;
const FAR: f32 = 6.4;

const EDGES: [(usize, usize); 12] = [
    (0, 1),
    (1, 3),
    (3, 2),
    (2, 0),
    (4, 5),
    (5, 7),
    (7, 6),
    (6, 4),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

#[derive(Clone, Copy)]
struct Box3 {
    x: f32,
    y: f32,
    z: f32,
    w: f32,
    h: f32,
    d: f32,
    rx: f32,
    ry: f32,
    rz: f32,
    vx: f32,
    vy: f32,
    vz: f32,
    accent: bool,
}

struct Rng(u32);

impl Rng {
    fn next_f32(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / 16_777_216.0
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + self.next_f32() * (b - a)
    }
}

pub struct Backdrop {
    boxes: Vec<Box3>,
    rng: Rng,
    spread_x: f32,
    spread_y: f32,
    focal: f32,
    size: egui::Vec2,
    pub speed: f32,
    pub density: f32,
}

impl Default for Backdrop {
    fn default() -> Self {
        Self::new()
    }
}

impl Backdrop {
    pub fn new() -> Self {
        Self {
            boxes: Vec::new(),
            rng: Rng(0x9E37_79B9),
            spread_x: 1.0,
            spread_y: 1.0,
            focal: 1.0,
            size: egui::Vec2::ZERO,
            speed: 1.0,
            density: 1.0,
        }
    }

    #[allow(clippy::approx_constant)]
    fn make_box(&mut self, anywhere: bool) -> Box3 {
        let s = self.rng.range(0.09, 0.30);
        Box3 {
            x: self.rng.range(-self.spread_x, self.spread_x),
            y: self.rng.range(-self.spread_y, self.spread_y),
            z: if anywhere {
                self.rng.range(NEAR, FAR)
            } else {
                self.rng.range(FAR * 0.82, FAR)
            },
            w: s * self.rng.range(0.55, 1.7),
            h: s * self.rng.range(0.55, 1.7),
            d: s * self.rng.range(0.55, 1.7),
            rx: self.rng.range(0.0, 6.28),
            ry: self.rng.range(0.0, 6.28),
            rz: self.rng.range(0.0, 6.28),
            vx: self.rng.range(-0.22, 0.22),
            vy: self.rng.range(-0.22, 0.22),
            vz: self.rng.range(-0.14, 0.14),
            accent: self.rng.next_f32() < 0.18,
        }
    }

    fn target_count(&self) -> usize {
        let area = self.size.x * self.size.y;
        ((area / 26_000.0).clamp(10.0, 90.0) * self.density).round() as usize
    }

    fn resize(&mut self, size: egui::Vec2) {
        if (size.x - self.size.x).abs() < 1.0 && (size.y - self.size.y).abs() < 1.0 {
            return;
        }
        self.size = size;
        self.focal = size.x.min(size.y) * 0.95;
        self.spread_x = size.x * FAR / (2.0 * self.focal) * 1.2;
        self.spread_y = size.y * FAR / (2.0 * self.focal) * 1.2;
        self.reflow();
    }

    fn reflow(&mut self) {
        let want = self.target_count();
        while self.boxes.len() < want {
            let b = self.make_box(true);
            self.boxes.push(b);
        }
        self.boxes.truncate(want);
    }

    fn advance(&mut self, dt: f32) {
        if self.speed <= 0.0 {
            return;
        }
        let mut respawn = Vec::new();
        for (i, b) in self.boxes.iter_mut().enumerate() {
            b.z -= 0.5 * self.speed * dt;
            b.rx += b.vx * dt;
            b.ry += b.vy * dt;
            b.rz += b.vz * dt;
            if b.z <= NEAR {
                respawn.push(i);
            }
        }
        for i in respawn {
            let fresh = self.make_box(false);
            self.boxes[i] = fresh;
        }
    }

    pub fn paint(&mut self, ui: &Ui, rect: Rect, pal: &Palette, dt: f32) {
        self.resize(rect.size());
        self.density = self.density.clamp(0.0, 2.0);
        if self.target_count() != self.boxes.len() {
            self.reflow();
        }
        self.advance(dt.min(0.05));

        let painter = ui.painter().with_clip_rect(rect);
        let cx = rect.center().x;
        let cy = rect.center().y;
        let f = self.focal;

        let mut px = [0.0f32; 8];
        let mut py = [0.0f32; 8];

        for b in &self.boxes {
            if b.z <= NEAR {
                continue;
            }
            let mut alpha =
                ((FAR - b.z) / 1.3).min(1.0) * ((b.z - NEAR) / 0.9).min(1.0);
            if alpha <= 0.01 {
                continue;
            }
            alpha *= 0.5;

            let (sx, cx1) = b.rx.sin_cos();
            let (sy, cy1) = b.ry.sin_cos();
            let (sz, cz1) = b.rz.sin_cos();
            let (hw, hh, hd) = (b.w / 2.0, b.h / 2.0, b.d / 2.0);
            let mut ok = true;

            for v in 0..8 {
                let x = if v & 1 != 0 { hw } else { -hw };
                let y = if v & 2 != 0 { hh } else { -hh };
                let z = if v & 4 != 0 { hd } else { -hd };
                let y1 = y * cx1 - z * sx;
                let z1 = y * sx + z * cx1;
                let x2 = x * cy1 + z1 * sy;
                let z2 = -x * sy + z1 * cy1;
                let x3 = x2 * cz1 - y1 * sz;
                let y3 = x2 * sz + y1 * cz1;
                let wz = b.z + z2;
                if wz <= 0.05 {
                    ok = false;
                    break;
                }
                px[v] = cx + (b.x + x3) / wz * f;
                py[v] = cy + (b.y + y3) / wz * f;
            }
            if !ok {
                continue;
            }

            let base = if b.accent { pal.accent } else { pal.contrast };
            let color = Color32::from_rgba_unmultiplied(
                base.r(),
                base.g(),
                base.b(),
                (alpha * 255.0) as u8,
            );
            let stroke = Stroke::new((1.1 / b.z).clamp(0.5, 1.6), color);
            for (a, c) in EDGES {
                painter.line_segment(
                    [Pos2::new(px[a], py[a]), Pos2::new(px[c], py[c])],
                    stroke,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn density_scales_the_population() {
        let mut b = Backdrop::new();
        b.size = egui::vec2(1000.0, 800.0);
        b.density = 1.0;
        let full = b.target_count();
        b.density = 0.5;
        assert!(b.target_count() < full);
        b.density = 0.0;
        assert_eq!(b.target_count(), 0);
    }

    #[test]
    fn a_still_field_does_not_drift() {
        let mut b = Backdrop::new();
        b.size = egui::vec2(900.0, 700.0);
        b.focal = 600.0;
        b.spread_x = 5.0;
        b.spread_y = 4.0;
        b.reflow();
        b.speed = 0.0;
        let before: Vec<f32> = b.boxes.iter().map(|x| x.z).collect();
        b.advance(0.1);
        let after: Vec<f32> = b.boxes.iter().map(|x| x.z).collect();
        assert_eq!(before, after, "reduced motion must hold the field still");
    }

    #[test]
    fn boxes_respawn_at_the_far_plane() {
        let mut b = Backdrop::new();
        b.size = egui::vec2(900.0, 700.0);
        b.focal = 600.0;
        b.spread_x = 5.0;
        b.spread_y = 4.0;
        b.reflow();
        for bx in &mut b.boxes {
            bx.z = NEAR + 0.001;
        }
        b.advance(0.05);
        assert!(
            b.boxes.iter().all(|x| x.z >= FAR * 0.82),
            "a box that passes the camera comes back at the far plane"
        );
    }
}
