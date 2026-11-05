use egui::Color32;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeDef {
    pub id: &'static str,
    pub name: &'static str,
    pub base: Oklch,
    pub contrast: Oklch,
    pub accent: Oklch,
}

pub type Oklch = [f32; 3];

pub const THEMES: [ThemeDef; 9] = [
    ThemeDef {
        id: "terminal",
        name: "Terminal",
        base: [0.17, 0.012, 265.0],
        contrast: [0.86, 0.015, 265.0],
        accent: [0.80, 0.16, 158.0],
    },
    ThemeDef {
        id: "ember",
        name: "Ember",
        base: [0.16, 0.022, 40.0],
        contrast: [0.87, 0.020, 50.0],
        accent: [0.72, 0.18, 42.0],
    },
    ThemeDef {
        id: "flare",
        name: "Flare",
        base: [0.15, 0.016, 95.0],
        contrast: [0.88, 0.020, 95.0],
        accent: [0.86, 0.17, 96.0],
    },
    ThemeDef {
        id: "cobalt",
        name: "Cobalt",
        base: [0.16, 0.022, 250.0],
        contrast: [0.87, 0.020, 250.0],
        accent: [0.74, 0.15, 235.0],
    },
    ThemeDef {
        id: "orchid",
        name: "Orchid",
        base: [0.16, 0.022, 330.0],
        contrast: [0.88, 0.015, 330.0],
        accent: [0.75, 0.19, 340.0],
    },
    ThemeDef {
        id: "wire",
        name: "Wire",
        base: [0.13, 0.0, 0.0],
        contrast: [0.74, 0.0, 0.0],
        accent: [0.98, 0.0, 0.0],
    },
    ThemeDef {
        id: "void",
        name: "Void",
        base: [0.06, 0.0, 0.0],
        contrast: [0.92, 0.010, 300.0],
        accent: [0.72, 0.20, 300.0],
    },
    ThemeDef {
        id: "paper",
        name: "Paper",
        base: [0.97, 0.006, 95.0],
        contrast: [0.26, 0.012, 95.0],
        accent: [0.52, 0.14, 155.0],
    },
    ThemeDef {
        id: "blueprint",
        name: "Blueprint",
        base: [0.95, 0.012, 240.0],
        contrast: [0.28, 0.030, 250.0],
        accent: [0.50, 0.16, 245.0],
    },
];

pub fn theme_by_id(id: &str) -> &'static ThemeDef {
    THEMES.iter().find(|t| t.id == id).unwrap_or(&THEMES[0])
}

pub fn oklch_to_color(c: Oklch) -> Color32 {
    let [l, chroma, hue] = c;
    let h = hue * std::f32::consts::PI / 180.0;
    let a = chroma * h.cos();
    let b = chroma * h.sin();
    let (r, g, bl) = oklab_to_linear_srgb(l, a, b);
    Color32::from_rgb(encode(r), encode(g), encode(bl))
}

fn oklab_to_linear_srgb(l_: f32, a: f32, b: f32) -> (f32, f32, f32) {
    let l = (l_ + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m = (l_ - 0.105_561_35 * a - 0.063_854_17 * b).powi(3);
    let s = (l_ - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    (
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    )
}

fn encode(x: f32) -> u8 {
    let x = if x <= 0.003_130_8 {
        12.92 * x
    } else {
        1.055 * x.max(0.0).powf(1.0 / 2.4) - 0.055
    };
    (x.clamp(0.0, 1.0) * 255.0).round() as u8
}

pub fn mix_oklab(a: Oklch, b: Oklch, a_share: f32) -> Oklch {
    let t = 1.0 - a_share.clamp(0.0, 1.0);
    [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp_hue(a[2], b[2], t)]
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn lerp_hue(a: f32, b: f32, t: f32) -> f32 {
    let d = ((b - a) % 360.0 + 540.0) % 360.0 - 180.0;
    a + d * t
}

pub fn lerp_oklch(a: Oklch, b: Oklch, t: f32) -> Oklch {
    [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp_hue(a[2], b[2], t)]
}

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub base_oklch: Oklch,
    pub contrast_oklch: Oklch,
    pub accent_oklch: Oklch,

    pub base: Color32,
    pub contrast: Color32,
    pub accent: Color32,
    pub dim: Color32,
    pub hair: Color32,
    pub sheet: Color32,
    pub well: Color32,
    pub marker: Color32,

    pub success: Color32,
    pub warning: Color32,
    pub error: Color32,
    pub light: bool,
}

impl Palette {
    pub fn from_oklch(base: Oklch, contrast: Oklch, accent: Oklch) -> Self {
        let light = base[0] > contrast[0];
        let l = if light { 0.55 } else { 0.78 };
        Self {
            base_oklch: base,
            contrast_oklch: contrast,
            accent_oklch: accent,
            base: oklch_to_color(base),
            contrast: oklch_to_color(contrast),
            accent: oklch_to_color(accent),
            dim: oklch_to_color(mix_oklab(contrast, base, 0.58)),
            hair: oklch_to_color(mix_oklab(contrast, base, 0.42)),
            sheet: oklch_to_color(mix_oklab(contrast, base, if light { 0.03 } else { 0.05 })),
            well: oklch_to_color(mix_oklab(contrast, base, if light { 0.10 } else { 0.12 })),
            marker: oklch_to_color(mix_oklab(accent, base, 0.26)),
            success: oklch_to_color([l, 0.16, 150.0]),
            warning: oklch_to_color([l, 0.15, 80.0]),
            error: oklch_to_color([l, 0.19, 25.0]),
            light,
        }
    }

    pub fn from_theme(t: &ThemeDef) -> Self {
        Self::from_oklch(t.base, t.contrast, t.accent)
    }

    pub fn lerp_to(&self, other: &Palette, t: f32) -> Self {
        Self::from_oklch(
            lerp_oklch(self.base_oklch, other.base_oklch, t),
            lerp_oklch(self.contrast_oklch, other.contrast_oklch, t),
            lerp_oklch(self.accent_oklch, other.accent_oklch, t),
        )
    }

    pub fn on_solid(&self) -> Color32 {
        self.base
    }

    pub fn faded(&self, c: Color32, amount: f32) -> Color32 {
        let t = amount.clamp(0.0, 1.0);
        Color32::from_rgb(
            lerp(c.r() as f32, self.base.r() as f32, t) as u8,
            lerp(c.g() as f32, self.base.g() as f32, t) as u8,
            lerp(c.b() as f32, self.base.b() as f32, t) as u8,
        )
    }

    pub fn gradient(&self, t: f32) -> Color32 {
        let t = t.clamp(0.0, 1.0);
        let (a, b, local) = if t < 0.5 {
            (self.error, self.warning, t * 2.0)
        } else {
            (self.warning, self.success, (t - 0.5) * 2.0)
        };
        Color32::from_rgb(
            lerp(a.r() as f32, b.r() as f32, local) as u8,
            lerp(a.g() as f32, b.g() as f32, local) as u8,
            lerp(a.b() as f32, b.b() as f32, local) as u8,
        )
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::from_theme(&THEMES[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_matches_known_srgb_anchors() {
        let cases: [(Oklch, [u8; 3], &str); 5] = [
            ([1.0, 0.0, 0.0], [255, 255, 255], "white"),
            ([0.0, 0.0, 0.0], [0, 0, 0], "black"),
            ([0.62796, 0.25768, 29.234], [255, 0, 0], "sRGB red"),
            ([0.86644, 0.29483, 142.495], [0, 255, 0], "sRGB green"),
            ([0.45201, 0.31321, 264.052], [0, 0, 255], "sRGB blue"),
        ];
        for (oklch, expected, name) in cases {
            let c = oklch_to_color(oklch);
            let got = [c.r(), c.g(), c.b()];
            for i in 0..3 {
                assert!(
                    (got[i] as i32 - expected[i] as i32).abs() <= 2,
                    "{name}: got {got:?}, expected {expected:?}"
                );
            }
        }
    }

    #[test]
    fn the_terminal_accent_is_a_green() {
        let c = oklch_to_color(theme_by_id("terminal").accent);
        assert!(
            c.g() > c.r() && c.g() > c.b(),
            "expected a green, got #{:02x}{:02x}{:02x}",
            c.r(),
            c.g(),
            c.b()
        );
    }

    #[test]
    fn light_palettes_are_detected() {
        assert!(Palette::from_theme(theme_by_id("paper")).light);
        assert!(Palette::from_theme(theme_by_id("blueprint")).light);
        assert!(!Palette::from_theme(theme_by_id("terminal")).light);
        assert!(!Palette::from_theme(theme_by_id("void")).light);
    }

    #[test]
    fn dim_sits_between_base_and_contrast() {
        for t in THEMES {
            let p = Palette::from_theme(&t);
            let lum = |c: egui::Color32| 0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32;
            let (lo, hi) = if p.light {
                (lum(p.contrast), lum(p.base))
            } else {
                (lum(p.base), lum(p.contrast))
            };
            let d = lum(p.dim);
            assert!(d > lo - 1.0 && d < hi + 1.0, "{}: dim {d} outside [{lo}, {hi}]", t.id);
        }
    }

    #[test]
    fn hue_interpolation_takes_the_short_way_round() {
        assert!((lerp_hue(350.0, 10.0, 0.5) - 360.0).abs() < 1e-3);
        assert!((lerp_hue(10.0, 350.0, 0.5) - 0.0).abs() < 1e-3);
    }

    #[test]
    fn every_theme_id_is_unique_and_resolvable() {
        for t in THEMES {
            assert_eq!(theme_by_id(t.id).id, t.id);
        }
        assert_eq!(theme_by_id("nonsense").id, "terminal", "falls back safely");
    }
}
