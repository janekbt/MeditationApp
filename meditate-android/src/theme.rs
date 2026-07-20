//! Accent theming (Phase 8): rebuild `MaterialPalette.schemes`
//! from a picked accent colour. The Material lib's `schemes` is an
//! `in` property — the intended theming hook — so no vendored
//! patch: Rust reads the baseline (blue) schemes at startup and
//! swaps the accent-family fields (primary / secondary / tertiary
//! plus their fixed variants, surfaceTint, inversePrimary) for the
//! selected seed, leaving every neutral (surfaces, outlines,
//! error) untouched so contrast stays canonical.
//!
//! Tone generation approximates the Material-3 HCT ladder with an
//! HSL lightness ladder per tone — not colorimetrically exact HCT,
//! but structure-faithful (tone 40 light primary / 80 dark primary
//! / 90 containers / 10 on-containers) and visually consistent
//! across the six seeds. Secondary = same hue desaturated;
//! tertiary = hue shifted +60°, mid-saturation — the same
//! relationships the M3 baseline uses.

use slint::Color;

/// Accent catalogue: (name, seed hue in degrees). Index 0 is the
/// baseline blue — selecting it restores the untouched shipped
/// schemes rather than a regenerated approximation.
pub const ACCENTS: &[(&str, f32)] = &[
    ("Blue", 220.0), // baseline — shipped scheme used verbatim
    ("Green", 140.0),
    ("Teal", 180.0),
    ("Purple", 275.0),
    ("Rose", 350.0),
    ("Amber", 40.0),
];

/// HSL → Color. h in degrees, s/l in 0..=1.
fn hsl(h: f32, s: f32, l: f32) -> Color {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let to8 = |v: f32| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    Color::from_rgb_u8(to8(r1), to8(g1), to8(b1))
}

/// M3-ish tone → HSL lightness. Tones brighten slightly slower
/// than linear at the top so containers stay pastel, not washed.
fn tone_l(tone: f32) -> f32 {
    (tone / 100.0).powf(1.08)
}

/// One tonal family (a hue+saturation pair rendered at the M3
/// reference tones the scheme fields need).
struct Family {
    hue: f32,
    sat: f32,
}

impl Family {
    fn t(&self, tone: f32) -> Color {
        // Saturation eases off toward the extremes like real
        // tonal palettes do.
        let edge = ((tone - 50.0).abs() / 50.0).powi(2);
        let s = self.sat * (1.0 - 0.35 * edge);
        hsl(self.hue, s, tone_l(tone))
    }
}

/// The accent-family field values for one mode.
pub struct AccentFields {
    pub primary: Color,
    pub on_primary: Color,
    pub primary_container: Color,
    pub on_primary_container: Color,
    pub secondary: Color,
    pub on_secondary: Color,
    pub secondary_container: Color,
    pub on_secondary_container: Color,
    pub tertiary: Color,
    pub on_tertiary: Color,
    pub tertiary_container: Color,
    pub on_tertiary_container: Color,
    pub inverse_primary: Color,
    pub primary_fixed: Color,
    pub on_primary_fixed: Color,
    pub primary_fixed_dim: Color,
    pub on_primary_fixed_variant: Color,
    pub secondary_fixed: Color,
    pub on_secondary_fixed: Color,
    pub secondary_fixed_dim: Color,
    pub on_secondary_fixed_variant: Color,
    pub tertiary_fixed: Color,
    pub on_tertiary_fixed: Color,
    pub tertiary_fixed_dim: Color,
    pub on_tertiary_fixed_variant: Color,
}

/// Generate the accent fields for `hue`, per mode. Tone mapping
/// mirrors the M3 scheme tables; the `fixed` family is
/// mode-independent by spec (same values in light + dark).
pub fn accent_fields(hue: f32, dark: bool) -> AccentFields {
    let p = Family { hue, sat: 0.48 };
    let s = Family { hue, sat: 0.18 };
    let t = Family { hue: hue + 60.0, sat: 0.30 };

    let (pri, on_pri, cont, on_cont) = if dark {
        (80.0, 20.0, 30.0, 90.0)
    } else {
        (40.0, 100.0, 90.0, 10.0)
    };
    let inv_pri = if dark { 40.0 } else { 80.0 };

    AccentFields {
        primary: p.t(pri),
        on_primary: p.t(on_pri),
        primary_container: p.t(cont),
        on_primary_container: p.t(on_cont),
        secondary: s.t(pri),
        on_secondary: s.t(on_pri),
        secondary_container: s.t(cont),
        on_secondary_container: s.t(on_cont),
        tertiary: t.t(pri),
        on_tertiary: t.t(on_pri),
        tertiary_container: t.t(cont),
        on_tertiary_container: t.t(on_cont),
        inverse_primary: p.t(inv_pri),
        primary_fixed: p.t(90.0),
        on_primary_fixed: p.t(10.0),
        primary_fixed_dim: p.t(80.0),
        on_primary_fixed_variant: p.t(30.0),
        secondary_fixed: s.t(90.0),
        on_secondary_fixed: s.t(10.0),
        secondary_fixed_dim: s.t(80.0),
        on_secondary_fixed_variant: s.t(30.0),
        tertiary_fixed: t.t(90.0),
        on_tertiary_fixed: t.t(10.0),
        tertiary_fixed_dim: t.t(80.0),
        on_tertiary_fixed_variant: t.t(30.0),
    }
}

/// Swatch colour for the Preferences picker circle — the accent's
/// tone-40 primary (readable on light and dark list surfaces).
/// Index 0 (baseline blue) uses the shipped primary directly so
/// the swatch matches exactly what selecting it restores.
pub fn swatch(idx: usize, baseline_primary: Color) -> Color {
    if idx == 0 {
        baseline_primary
    } else {
        Family { hue: ACCENTS[idx].1, sat: 0.48 }.t(40.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_light_and_dark_use_m3_tone_relationship() {
        let l = accent_fields(140.0, false);
        let d = accent_fields(140.0, true);
        // dark primary is the light fixed-dim (both tone 80).
        assert_eq!(d.primary, l.primary_fixed_dim);
        // fixed family is mode-independent.
        assert_eq!(l.primary_fixed, d.primary_fixed);
        assert_eq!(l.on_tertiary_fixed_variant, d.on_tertiary_fixed_variant);
    }

    #[test]
    fn on_colors_contrast_with_their_base() {
        // Coarse sanity: tone distance ≥ 60 between a colour and
        // its on-colour ⇒ big lightness gap.
        let f = accent_fields(275.0, false);
        let lum = |c: Color| {
            0.299 * f32::from(c.red())
                + 0.587 * f32::from(c.green())
                + 0.114 * f32::from(c.blue())
        };
        assert!((lum(f.primary) - lum(f.on_primary)).abs() > 90.0);
        assert!(
            (lum(f.primary_container) - lum(f.on_primary_container)).abs()
                > 90.0
        );
    }

    #[test]
    fn catalogue_has_six_named_accents_blue_first() {
        assert_eq!(ACCENTS.len(), 6);
        assert_eq!(ACCENTS[0].0, "Blue");
    }
}
