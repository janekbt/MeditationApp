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

use crate::MaterialSchemes;

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

/// The Log's sixteen label colours for one mode, from that mode's
/// primary: places 0..8 turn its hue in 45 degree steps at its own
/// saturation and lightness; places 8..16 are turned a further 22.5
/// degrees and moved away from the card (darker in light mode,
/// lighter in dark mode) so label text stays readable.
pub fn label_palette(primary: Color, dark: bool) -> [Color; 16] {
    let (h, s, l) = to_hsl(primary);
    let l2 = if dark { l + 0.45 * (1.0 - l) } else { l * 0.7 };
    std::array::from_fn(|i| match i {
        0 => primary,
        1..8 => hsl(h + 45.0 * i as f32, s, l),
        _ => hsl(h + 22.5 + 45.0 * (i - 8) as f32, s, l2),
    })
}

/// Color -> HSL, h in degrees, s/l in 0..=1 (inverse of `hsl`).
fn to_hsl(c: Color) -> (f32, f32, f32) {
    let [r, g, b] = [c.red(), c.green(), c.blue()].map(|v| f32::from(v) / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let (d, l) = (max - min, (max + min) / 2.0);
    if d == 0.0 {
        return (0.0, 0.0, l);
    }
    let h = if max == r {
        (g - b) / d
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (60.0 * h, d / (1.0 - (2.0 * l - 1.0).abs()), l)
}

/// The accent picked in Preferences: the system's colour scheme or
/// one of `ACCENTS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accent {
    System,
    Fixed(usize),
}

/// The accent a stored `android_accent` value stands for. A pick
/// made since System exists is stored as `system` or `+N`; a plain
/// `N` (or no value) is from before, and turns into System where the
/// phone has a scheme.
pub fn picked_accent(stored: &str, has_system: bool) -> Accent {
    let fixed = |n: &str| Accent::Fixed(n.parse().ok().filter(|&i| i < ACCENTS.len()).unwrap_or(0));
    match stored.strip_prefix('+') {
        Some(n) => fixed(n),
        None if has_system => Accent::System,
        None => fixed(stored),
    }
}

/// The value to store for `accent`. The previous release reads `+N`
/// as N and `system` as Blue.
pub fn stored_value(accent: Accent) -> String {
    match accent {
        Accent::System => "system".to_string(),
        Accent::Fixed(i) => format!("+{i}"),
    }
}

/// The picker's circles, in order.
pub fn accent_row(has_system: bool) -> Vec<Accent> {
    has_system
        .then_some(Accent::System)
        .into_iter()
        .chain((0..ACCENTS.len()).map(Accent::Fixed))
        .collect()
}

/// The circle `stored` rings in the row for `has_system`.
pub fn accent_ring(stored: &str, has_system: bool) -> usize {
    let picked = picked_accent(stored, has_system);
    accent_row(has_system).iter().position(|a| *a == picked).unwrap_or(0)
}

/// The Material 3 roles Android 14+ reports, as `(MaterialScheme field,
/// light resource, dark resource)`. Inverse roles come from the
/// tonal palettes, the surface tint is the primary; shadow and scrim
/// have no system colour.
macro_rules! system_roles {
    ($m:ident) => {
        $m! {
            primary "system_primary_light" "system_primary_dark";
            surfaceTint "system_primary_light" "system_primary_dark";
            onPrimary "system_on_primary_light" "system_on_primary_dark";
            primaryContainer "system_primary_container_light" "system_primary_container_dark";
            onPrimaryContainer "system_on_primary_container_light" "system_on_primary_container_dark";
            secondary "system_secondary_light" "system_secondary_dark";
            onSecondary "system_on_secondary_light" "system_on_secondary_dark";
            secondaryContainer "system_secondary_container_light" "system_secondary_container_dark";
            onSecondaryContainer "system_on_secondary_container_light" "system_on_secondary_container_dark";
            tertiary "system_tertiary_light" "system_tertiary_dark";
            onTertiary "system_on_tertiary_light" "system_on_tertiary_dark";
            tertiaryContainer "system_tertiary_container_light" "system_tertiary_container_dark";
            onTertiaryContainer "system_on_tertiary_container_light" "system_on_tertiary_container_dark";
            error "system_error_light" "system_error_dark";
            onError "system_on_error_light" "system_on_error_dark";
            errorContainer "system_error_container_light" "system_error_container_dark";
            onErrorContainer "system_on_error_container_light" "system_on_error_container_dark";
            background "system_background_light" "system_background_dark";
            onBackground "system_on_background_light" "system_on_background_dark";
            surface "system_surface_light" "system_surface_dark";
            onSurface "system_on_surface_light" "system_on_surface_dark";
            surfaceVariant "system_surface_variant_light" "system_surface_variant_dark";
            onSurfaceVariant "system_on_surface_variant_light" "system_on_surface_variant_dark";
            outline "system_outline_light" "system_outline_dark";
            outlineVariant "system_outline_variant_light" "system_outline_variant_dark";
            inverseSurface "system_neutral1_800" "system_neutral1_100";
            inverseOnSurface "system_neutral1_50" "system_neutral1_800";
            inversePrimary "system_accent1_200" "system_accent1_600";
            primaryFixed "system_primary_fixed" "system_primary_fixed";
            onPrimaryFixed "system_on_primary_fixed" "system_on_primary_fixed";
            primaryFixedDim "system_primary_fixed_dim" "system_primary_fixed_dim";
            onPrimaryFixedVariant "system_on_primary_fixed_variant" "system_on_primary_fixed_variant";
            secondaryFixed "system_secondary_fixed" "system_secondary_fixed";
            onSecondaryFixed "system_on_secondary_fixed" "system_on_secondary_fixed";
            secondaryFixedDim "system_secondary_fixed_dim" "system_secondary_fixed_dim";
            onSecondaryFixedVariant "system_on_secondary_fixed_variant" "system_on_secondary_fixed_variant";
            tertiaryFixed "system_tertiary_fixed" "system_tertiary_fixed";
            onTertiaryFixed "system_on_tertiary_fixed" "system_on_tertiary_fixed";
            tertiaryFixedDim "system_tertiary_fixed_dim" "system_tertiary_fixed_dim";
            onTertiaryFixedVariant "system_on_tertiary_fixed_variant" "system_on_tertiary_fixed_variant";
            surfaceDim "system_surface_dim_light" "system_surface_dim_dark";
            surfaceBright "system_surface_bright_light" "system_surface_bright_dark";
            surfaceContainerLowest "system_surface_container_lowest_light" "system_surface_container_lowest_dark";
            surfaceContainerLow "system_surface_container_low_light" "system_surface_container_low_dark";
            surfaceContainer "system_surface_container_light" "system_surface_container_dark";
            surfaceContainerHigh "system_surface_container_high_light" "system_surface_container_high_dark";
            surfaceContainerHighest "system_surface_container_highest_light" "system_surface_container_highest_dark";
        }
    };
}

macro_rules! define_system_schemes {
    ($($material:ident $light:literal $dark:literal;)*) => {
        /// `baseline` with every role the system reports, `None` unless it
        /// reports all of them; shadow and scrim stay as shipped.
        pub fn system_schemes<E>(
            baseline: &MaterialSchemes,
            mut lookup: impl FnMut(&str) -> Result<Option<Color>, E>,
        ) -> Result<Option<MaterialSchemes>, E> {
            let mut schemes = baseline.clone();
            $(
                let (Some(light), Some(dark)) = (lookup($light)?, lookup($dark)?) else {
                    return Ok(None);
                };
                schemes.light.$material = light;
                schemes.dark.$material = dark;
            )*
            Ok(Some(schemes))
        }
    };
}
system_roles!(define_system_schemes);

/// The scheme for `accent`: Blue is the shipped baseline verbatim,
/// the other colours regenerate the accent families over the baseline
/// neutrals, and System is the phone's (from `system_schemes`).
pub fn accent_schemes(baseline: &MaterialSchemes, accent: Accent, system: Option<&MaterialSchemes>) -> MaterialSchemes {
    let idx = match (accent, system) {
        (Accent::System, Some(system)) => return system.clone(),
        (Accent::Fixed(idx), _) if idx > 0 && idx < ACCENTS.len() => idx,
        _ => return baseline.clone(),
    };
    let hue = ACCENTS[idx].1;
    let mut schemes = baseline.clone();
    for (scheme, dark) in [
        (&mut schemes.light, false),
        (&mut schemes.dark, true),
    ] {
        let f = accent_fields(hue, dark);
        scheme.primary = f.primary;
        scheme.surfaceTint = f.primary;
        scheme.onPrimary = f.on_primary;
        scheme.primaryContainer = f.primary_container;
        scheme.onPrimaryContainer = f.on_primary_container;
        scheme.secondary = f.secondary;
        scheme.onSecondary = f.on_secondary;
        scheme.secondaryContainer = f.secondary_container;
        scheme.onSecondaryContainer = f.on_secondary_container;
        scheme.tertiary = f.tertiary;
        scheme.onTertiary = f.on_tertiary;
        scheme.tertiaryContainer = f.tertiary_container;
        scheme.onTertiaryContainer = f.on_tertiary_container;
        scheme.inversePrimary = f.inverse_primary;
        scheme.primaryFixed = f.primary_fixed;
        scheme.onPrimaryFixed = f.on_primary_fixed;
        scheme.primaryFixedDim = f.primary_fixed_dim;
        scheme.onPrimaryFixedVariant = f.on_primary_fixed_variant;
        scheme.secondaryFixed = f.secondary_fixed;
        scheme.onSecondaryFixed = f.on_secondary_fixed;
        scheme.secondaryFixedDim = f.secondary_fixed_dim;
        scheme.onSecondaryFixedVariant =
            f.on_secondary_fixed_variant;
        scheme.tertiaryFixed = f.tertiary_fixed;
        scheme.onTertiaryFixed = f.on_tertiary_fixed;
        scheme.tertiaryFixedDim = f.tertiary_fixed_dim;
        scheme.onTertiaryFixedVariant =
            f.on_tertiary_fixed_variant;
    }
    schemes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hue_and_lightness(c: Color) -> (f32, f32) {
        let (r, g, b) = (c.red() as f32 / 255.0, c.green() as f32 / 255.0, c.blue() as f32 / 255.0);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let d = max - min;
        let h = if d == 0.0 { 0.0 }
            else if max == r { 60.0 * ((g - b) / d).rem_euclid(6.0) }
            else if max == g { 60.0 * ((b - r) / d + 2.0) }
            else { 60.0 * ((r - g) / d + 4.0) };
        (h, (max + min) / 2.0)
    }

    fn assert_hue(c: Color, want: f32) {
        let (h, _) = hue_and_lightness(c);
        let off = (h - want).rem_euclid(360.0);
        assert!(off.min(360.0 - off) < 3.0, "hue {h}, want {want}");
    }

    #[test]
    fn label_colours_start_at_the_accent_and_step_round_the_wheel() {
        let light_primary = accent_fields(140.0, false).primary;
        let dark_primary = accent_fields(140.0, true).primary;
        let light = label_palette(light_primary, false);
        let dark = label_palette(dark_primary, true);
        assert_eq!(light[0], light_primary);
        assert_eq!(dark[0], dark_primary);
        let hues = [140.0, 185.0, 230.0, 275.0, 320.0, 5.0, 50.0, 95.0];
        for (k, want) in hues.iter().enumerate() {
            for p in [&light, &dark] {
                assert_hue(p[k], *want);
                assert_hue(p[k + 8], want + 22.5);
            }
            let l = |c| hue_and_lightness(c).1;
            assert!(l(light[k + 8]) < l(light[k]) - 0.05, "place {} not darker in light mode", k + 8);
            assert!(l(dark[k]) > l(light[k]) + 0.05, "place {k} not lighter in dark mode");
            assert!(l(dark[k + 8]) > l(dark[k]) + 0.03, "place {} not lighter in dark mode", k + 8);
        }
    }

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

    #[test]
    fn a_missing_pick_is_system_where_the_system_has_a_scheme() {
        assert_eq!(picked_accent("", true), Accent::System);
    }

    #[test]
    fn a_missing_pick_is_blue_without_a_system_scheme() {
        assert_eq!(picked_accent("", false), Accent::Fixed(0));
    }

    #[test]
    fn the_system_circle_comes_first_only_with_a_system_scheme() {
        use Accent::*;
        let fixed = [Fixed(0), Fixed(1), Fixed(2), Fixed(3), Fixed(4), Fixed(5)];
        assert_eq!(accent_row(false), fixed);
        let mut with_system = vec![System];
        with_system.extend(fixed);
        assert_eq!(accent_row(true), with_system);
    }

    #[test]
    fn picking_stores_values_the_previous_release_reads() {
        assert_eq!(stored_value(Accent::System), "system");
        assert_eq!(stored_value(Accent::Fixed(1)), "+1");
        // The previous release reads with `parse().unwrap_or(0)`.
        assert_eq!("+1".parse::<usize>(), Ok(1));
        assert!("system".parse::<usize>().is_err());
        assert_eq!(picked_accent("+1", true), Accent::Fixed(1));
        assert_eq!(picked_accent("system", true), Accent::System);
    }

    #[test]
    fn a_synced_old_pick_reads_like_an_old_pick() {
        assert_eq!(picked_accent("2", false), Accent::Fixed(2));
        assert_eq!(picked_accent("2", true), Accent::System);
    }

    #[test]
    fn a_system_pick_shows_blue_without_a_system_scheme() {
        let picked = picked_accent("system", false);
        assert_eq!(picked, Accent::Fixed(0));
        assert_eq!(accent_row(false).iter().position(|a| *a == picked), Some(0));
    }

    #[test]
    fn an_old_pick_is_system_where_the_system_has_a_scheme() {
        for old in ["0", "4", "9", "x"] {
            assert_eq!(picked_accent(old, true), Accent::System, "{old}");
        }
    }

    #[test]
    fn a_pick_made_now_is_kept() {
        assert_eq!(picked_accent("+2", true), Accent::Fixed(2));
        assert_eq!(picked_accent("+0", true), Accent::Fixed(0));
        assert_eq!(picked_accent("+2", false), Accent::Fixed(2));
        assert_eq!("+2".parse::<usize>(), Ok(2));
    }

    #[test]
    fn an_old_pick_below_android_14_keeps_its_colour() {
        assert_eq!(picked_accent("4", false), Accent::Fixed(4));
        assert_eq!(picked_accent("9", false), Accent::Fixed(0));
        assert_eq!(picked_accent("+9", true), Accent::Fixed(0));
    }

    /// A distinct colour per resource name.
    fn fake(name: &str) -> Color {
        let h = name.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
        Color::from_argb_encoded(0xff00_0000 | (h & 0x00ff_ffff))
    }

    fn read(baseline: &MaterialSchemes, lookup: impl Fn(&str) -> Option<Color>) -> Option<MaterialSchemes> {
        system_schemes(baseline, |n| Ok::<_, ()>(lookup(n))).unwrap()
    }

    #[test]
    fn the_system_scheme_takes_every_role_from_the_system() {
        let mut baseline = MaterialSchemes::default();
        baseline.light.shadow = Color::from_rgb_u8(1, 2, 3);
        baseline.dark.scrim = Color::from_rgb_u8(4, 5, 6);
        let s = read(&baseline, |n| Some(fake(n))).expect("all roles reported");
        assert_eq!(s.light.primary, fake("system_primary_light"));
        assert_eq!(s.dark.primary, fake("system_primary_dark"));
        assert_eq!(s.light.surfaceTint, fake("system_primary_light"));
        assert_eq!(s.light.onSurfaceVariant, fake("system_on_surface_variant_light"));
        assert_eq!(s.dark.surfaceContainerHighest, fake("system_surface_container_highest_dark"));
        assert_eq!(s.light.inverseSurface, fake("system_neutral1_800"));
        assert_eq!(s.light.inverseOnSurface, fake("system_neutral1_50"));
        assert_eq!(s.dark.inverseOnSurface, fake("system_neutral1_800"));
        assert_eq!(s.light.inversePrimary, fake("system_accent1_200"));
        assert_eq!(s.dark.inversePrimary, fake("system_accent1_600"));
        assert_eq!(s.light.primaryFixed, fake("system_primary_fixed"));
        assert_eq!(s.dark.onTertiaryFixedVariant, fake("system_on_tertiary_fixed_variant"));
        assert_eq!(s.light.shadow, Color::from_rgb_u8(1, 2, 3));
        assert_eq!(s.dark.scrim, Color::from_rgb_u8(4, 5, 6));
    }

    #[test]
    fn a_missing_role_means_no_system_scheme() {
        let baseline = MaterialSchemes::default();
        assert!(read(&baseline, |n| (n != "system_outline_dark").then(|| fake(n))).is_none());
        assert_eq!(system_schemes(&baseline, |_| Err("jni")).err(), Some("jni"));
    }

    #[test]
    fn the_ring_marks_the_accent_that_shows() {
        assert_eq!(accent_ring("4", true), 0);
        assert_eq!(accent_ring("+2", true), 3);
        assert_eq!(accent_ring("system", true), 0);
        assert_eq!(accent_ring("4", false), 4);
        assert_eq!(accent_ring("system", false), 0);
    }

    #[test]
    fn system_puts_every_role_on_the_material_scheme() {
        let baseline = MaterialSchemes::default();
        let system = read(&baseline, |n| Some(fake(n))).unwrap();
        assert_eq!(accent_schemes(&baseline, Accent::System, Some(&system)), system);
        // Without a scheme, System keeps the shipped colours.
        assert_eq!(accent_schemes(&baseline, Accent::System, None), baseline);
        assert_eq!(accent_schemes(&baseline, Accent::Fixed(0), Some(&system)), baseline);
    }
}
