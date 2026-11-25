use egui::{Color32, Pos2, Rect, Stroke, Ui};

use super::Palette;
use crate::graphics::Mode;

const LITE_DOT_SPACING: f32 = 28.0;
const LITE_DOT_RADIUS: f32 = 1.15;
const LITE_BASE_OPACITY: f32 = 0.55;
const LITE_MASK_CENTER_X: f32 = 0.70;
const LITE_MASK_CENTER_Y: f32 = 0.30;
const LITE_MASK_FADE_STOP: f32 = 0.75;

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

fn lite_mask_alpha(pos: Pos2, rect: Rect) -> f32 {
    let w = rect.width();
    let h = rect.height();
    if w <= 0.0 || h <= 0.0 {
        return 0.0;
    }
    let cx = rect.left() + w * LITE_MASK_CENTER_X;
    let cy = rect.top() + h * LITE_MASK_CENTER_Y;
    let fx = w * LITE_MASK_CENTER_X.max(1.0 - LITE_MASK_CENTER_X);
    let fy = h * LITE_MASK_CENTER_Y.max(1.0 - LITE_MASK_CENTER_Y);
    let rx = fx * std::f32::consts::SQRT_2;
    let ry = fy * std::f32::consts::SQRT_2;
    let nx = (pos.x - cx) / rx;
    let ny = (pos.y - cy) / ry;
    let d = (nx * nx + ny * ny).sqrt();
    (1.0 - d / LITE_MASK_FADE_STOP).clamp(0.0, 1.0)
}

#[derive(Default)]
struct LiteGridCache {
    rect: Option<Rect>,
    hair: Option<Color32>,
    dots: Vec<(Pos2, Color32)>,
}

impl LiteGridCache {
    fn update(&mut self, rect: Rect, pal: &Palette) {
        if self.rect == Some(rect) && self.hair == Some(pal.hair) {
            return;
        }
        self.rect = Some(rect);
        self.hair = Some(pal.hair);
        self.dots.clear();

        let (r, g, b) = (pal.hair.r(), pal.hair.g(), pal.hair.b());
        let mut y = rect.top() + LITE_DOT_SPACING * 0.5;
        while y < rect.bottom() {
            let mut x = rect.left() + LITE_DOT_SPACING * 0.5;
            while x < rect.right() {
                let pos = Pos2::new(x, y);
                let mask = lite_mask_alpha(pos, rect);
                let alpha = (LITE_BASE_OPACITY * mask * 255.0).round() as u8;
                if alpha > 0 {
                    self.dots.push((pos, Color32::from_rgba_unmultiplied(r, g, b, alpha)));
                }
                x += LITE_DOT_SPACING;
            }
            y += LITE_DOT_SPACING;
        }
    }
}

pub struct Backdrop {
    boxes: Vec<Box3>,
    rng: Rng,
    spread_x: f32,
    spread_y: f32,
    focal: f32,
    size: egui::Vec2,
    lite_cache: LiteGridCache,
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
            lite_cache: LiteGridCache::default(),
            speed: 1.0,
            density: 1.0,
        }
    }

    pub fn is_animating(&self, mode: Mode) -> bool {
        mode == Mode::Full && self.speed > 0.0 && self.density > 0.0
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
        let speed = self.speed;
        for i in 0..self.boxes.len() {
            let b = &mut self.boxes[i];
            b.z -= 0.5 * speed * dt;
            b.rx += b.vx * dt;
            b.ry += b.vy * dt;
            b.rz += b.vz * dt;
            if b.z <= NEAR {
                let fresh = self.make_box(false);
                self.boxes[i] = fresh;
            }
        }
    }

    pub fn paint_mode(&mut self, ui: &Ui, rect: Rect, pal: &Palette, dt: f32, mode: Mode) {
        match mode {
            Mode::Full => self.paint(ui, rect, pal, dt),
            Mode::Lite => self.paint_lite(ui, rect, pal),
        }
    }

    pub fn paint_lite(&mut self, ui: &Ui, rect: Rect, pal: &Palette) {
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            return;
        }
        self.lite_cache.update(rect, pal);
        let painter = ui.painter().with_clip_rect(rect);
        for &(pos, color) in &self.lite_cache.dots {
            painter.circle_filled(pos, LITE_DOT_RADIUS, color);
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
            let mut alpha = ((FAR - b.z) / 1.3).min(1.0) * ((b.z - NEAR) / 0.9).min(1.0);
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
            let color = Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), (alpha * 255.0) as u8);
            let stroke = Stroke::new((1.1 / b.z).clamp(0.5, 1.6), color);
            for (a, c) in EDGES {
                painter.line_segment([Pos2::new(px[a], py[a]), Pos2::new(px[c], py[c])], stroke);
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

    #[test]
    fn lite_mode_never_animates_and_caches_fading_dot_grid() {
        let mut b = Backdrop::new();
        assert!(b.is_animating(Mode::Full));
        assert!(!b.is_animating(Mode::Lite));
        b.speed = 0.0;
        assert!(!b.is_animating(Mode::Full));

        let pal = Palette::default();
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 800.0));
        b.lite_cache.update(rect, &pal);
        assert!(!b.lite_cache.dots.is_empty());

        let center_mask = lite_mask_alpha(Pos2::new(700.0, 240.0), rect);
        let mid_mask = lite_mask_alpha(Pos2::new(400.0, 480.0), rect);
        let far_corner_mask = lite_mask_alpha(Pos2::new(0.0, 800.0), rect);
        assert!((center_mask - 1.0).abs() < 1e-5);
        assert!(mid_mask > 0.1 && mid_mask < center_mask);
        assert_eq!(far_corner_mask, 0.0);

        assert!(
            b.lite_cache
                .dots
                .iter()
                .all(|&(p, c)| c.a() > 0 && lite_mask_alpha(p, rect) > 0.0)
        );

        let first_ptr = b.lite_cache.dots.as_ptr();
        let first_len = b.lite_cache.dots.len();
        b.lite_cache.update(rect, &pal);
        assert_eq!(b.lite_cache.dots.as_ptr(), first_ptr);
        assert_eq!(b.lite_cache.dots.len(), first_len);
    }
}
