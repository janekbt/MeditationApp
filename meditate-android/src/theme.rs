//! Accent theming: rebuild `MaterialPalette.schemes` from the picked
//! accent. The Material lib's `schemes` is an `in` property, the
//! intended theming hook. Blue is the shipped baseline verbatim, the
//! other fixed accents are Material's tonal-spot scheme from their
//! seed (every role, surfaces included), System is the phone's own.

use material_colors::{color::Argb, hct::Hct, scheme::variant::SchemeTonalSpot};
use slint::Color;

use crate::MaterialSchemes;

/// Accent catalogue: (name, seed). Index 0 is the baseline blue,
/// whose seed is unused: it restores the shipped schemes. The seeds
/// are the swatches the HSL approximation used to show, so each
/// accent keeps its hue.
pub const ACCENTS: &[(&str, u32)] = &[
    ("Blue", 0xff44_5e91),
    ("Green", 0xff32_8c50),
    ("Teal", 0xff32_8c8c),
    ("Purple", 0xff66_328c),
    ("Rose", 0xff8c_3241),
    ("Amber", 0xff8c_6e32),
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

/// Swatch colour for the Preferences picker circle: the light primary
/// of the scheme picking `accent` would show.
pub fn swatch(baseline: &MaterialSchemes, accent: Accent, system: Option<&MaterialSchemes>) -> Color {
    accent_schemes(baseline, accent, system).light.primary
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
/// the other colours are the tonal-spot scheme from their seed, and
/// System is the phone's (from `system_schemes`).
pub fn accent_schemes(baseline: &MaterialSchemes, accent: Accent, system: Option<&MaterialSchemes>) -> MaterialSchemes {
    match (accent, system) {
        (Accent::System, Some(system)) => system.clone(),
        (Accent::Fixed(idx), _) if idx > 0 && idx < ACCENTS.len() => {
            tonal_spot(Color::from_argb_encoded(ACCENTS[idx].1))
        }
        _ => baseline.clone(),
    }
}

fn argb(a: Argb) -> Color {
    Color::from_argb_u8(a.alpha, a.red, a.green, a.blue)
}

/// Material's tonal-spot scheme (2023 spec, standard contrast) from
/// `seed`, light and dark, every role.
pub fn tonal_spot(seed: Color) -> MaterialSchemes {
    let seed = Hct::new(Argb::from_u32(seed.as_argb_encoded()));
    let scheme = |dark| {
        let m = SchemeTonalSpot::new(seed, dark, None).scheme;
        crate::MaterialScheme {
        background: argb(m.background()),
        onBackground: argb(m.on_background()),
        surface: argb(m.surface()),
        surfaceDim: argb(m.surface_dim()),
        surfaceBright: argb(m.surface_bright()),
        surfaceContainerLowest: argb(m.surface_container_lowest()),
        surfaceContainerLow: argb(m.surface_container_low()),
        surfaceContainer: argb(m.surface_container()),
        surfaceContainerHigh: argb(m.surface_container_high()),
        surfaceContainerHighest: argb(m.surface_container_highest()),
        onSurface: argb(m.on_surface()),
        surfaceVariant: argb(m.surface_variant()),
        onSurfaceVariant: argb(m.on_surface_variant()),
        inverseSurface: argb(m.inverse_surface()),
        inverseOnSurface: argb(m.inverse_on_surface()),
        outline: argb(m.outline()),
        outlineVariant: argb(m.outline_variant()),
        shadow: argb(m.shadow()),
        scrim: argb(m.scrim()),
        surfaceTint: argb(m.surface_tint()),
        primary: argb(m.primary()),
        onPrimary: argb(m.on_primary()),
        primaryContainer: argb(m.primary_container()),
        onPrimaryContainer: argb(m.on_primary_container()),
        inversePrimary: argb(m.inverse_primary()),
        secondary: argb(m.secondary()),
        onSecondary: argb(m.on_secondary()),
        secondaryContainer: argb(m.secondary_container()),
        onSecondaryContainer: argb(m.on_secondary_container()),
        tertiary: argb(m.tertiary()),
        onTertiary: argb(m.on_tertiary()),
        tertiaryContainer: argb(m.tertiary_container()),
        onTertiaryContainer: argb(m.on_tertiary_container()),
        error: argb(m.error()),
        onError: argb(m.on_error()),
        errorContainer: argb(m.error_container()),
        onErrorContainer: argb(m.on_error_container()),
        primaryFixed: argb(m.primary_fixed()),
        primaryFixedDim: argb(m.primary_fixed_dim()),
        onPrimaryFixed: argb(m.on_primary_fixed()),
        onPrimaryFixedVariant: argb(m.on_primary_fixed_variant()),
        secondaryFixed: argb(m.secondary_fixed()),
        secondaryFixedDim: argb(m.secondary_fixed_dim()),
        onSecondaryFixed: argb(m.on_secondary_fixed()),
        onSecondaryFixedVariant: argb(m.on_secondary_fixed_variant()),
        tertiaryFixed: argb(m.tertiary_fixed()),
        tertiaryFixedDim: argb(m.tertiary_fixed_dim()),
        onTertiaryFixed: argb(m.on_tertiary_fixed()),
        onTertiaryFixedVariant: argb(m.on_tertiary_fixed_variant()),
        }
    };
    MaterialSchemes { light: scheme(false), dark: scheme(true) }
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
        let light_primary = hsl(140.0, 0.48, 0.37);
        let dark_primary = hsl(140.0, 0.48, 0.78);
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

    fn c(argb: u32) -> Color {
        Color::from_argb_encoded(argb)
    }

    #[test]
    fn a_seed_gives_materials_tonal_spot_scheme() {
        // Values from material-color-utilities' 2023 Swift test
        // (SchemeTonalSpotTests, seed 0xfffa2bec).
        let s = tonal_spot(c(0xfffa_2bec));
        let (l, d) = (&s.light, &s.dark);
        let light = [
            (l.primary, 0xff81_4c77), (l.onPrimary, 0xffff_ffff),
            (l.primaryContainer, 0xffff_d7f3), (l.onPrimaryContainer, 0xff34_0830),
            (l.secondary, 0xff6e_5868), (l.onSecondary, 0xffff_ffff),
            (l.secondaryContainer, 0xfff8_daee), (l.onSecondaryContainer, 0xff27_1624),
            (l.tertiary, 0xff81_5343), (l.onTertiary, 0xffff_ffff),
            (l.tertiaryContainer, 0xffff_dbd0), (l.onTertiaryContainer, 0xff32_1207),
            (l.error, 0xffba_1a1a), (l.onError, 0xffff_ffff),
            (l.errorContainer, 0xffff_dad6), (l.onErrorContainer, 0xff41_0002),
            (l.background, 0xffff_f7f9), (l.onBackground, 0xff20_1a1e),
            (l.surface, 0xffff_f7f9), (l.onSurface, 0xff20_1a1e),
            (l.surfaceVariant, 0xffee_dee7), (l.onSurfaceVariant, 0xff4e_444b),
            (l.outline, 0xff80_747b), (l.outlineVariant, 0xffd2_c2cb),
            (l.shadow, 0xff00_0000), (l.scrim, 0xff00_0000),
            (l.inverseSurface, 0xff36_2e33), (l.inverseOnSurface, 0xfffa_edf3),
            (l.inversePrimary, 0xfff3_b2e4),
        ];
        let dark = [
            (d.primary, 0xfff3_b2e4), (d.onPrimary, 0xff4d_1f47),
            (d.primaryContainer, 0xff67_355e), (d.onPrimaryContainer, 0xffff_d7f3),
            (d.secondary, 0xffdb_bed1), (d.onSecondary, 0xff3e_2a39),
            (d.secondaryContainer, 0xff55_4050), (d.onSecondaryContainer, 0xfff8_daee),
            (d.tertiary, 0xfff5_b9a5), (d.onTertiary, 0xff4c_2619),
            (d.tertiaryContainer, 0xff66_3c2d), (d.onTertiaryContainer, 0xffff_dbd0),
            (d.error, 0xffff_b4ab), (d.onError, 0xff69_0005),
            (d.errorContainer, 0xff93_000a), (d.onErrorContainer, 0xffff_dad6),
            (d.background, 0xff18_1216), (d.onBackground, 0xffec_dfe5),
            (d.surface, 0xff18_1216), (d.onSurface, 0xffec_dfe5),
            (d.surfaceVariant, 0xff4e_444b), (d.onSurfaceVariant, 0xffd2_c2cb),
            (d.outline, 0xff9a_8d95), (d.outlineVariant, 0xff4e_444b),
            (d.shadow, 0xff00_0000), (d.scrim, 0xff00_0000),
            (d.inverseSurface, 0xffec_dfe5), (d.inverseOnSurface, 0xff36_2e33),
            (d.inversePrimary, 0xff81_4c77),
        ];
        for (i, (got, want)) in light.iter().chain(dark.iter()).enumerate() {
            assert_eq!(*got, c(*want), "role {i}");
        }
        // The roles the reference test doesn't list: the crate's own
        // value for the same role, so a swapped field fails.
        for (ours, dark) in [(l, false), (d, true)] {
            let m = SchemeTonalSpot::new(Hct::new(Argb::from_u32(0xfffa_2bec)), dark, None).scheme;
            let rest = [
                (ours.surfaceTint, m.surface_tint()), (ours.surfaceDim, m.surface_dim()),
                (ours.surfaceBright, m.surface_bright()),
                (ours.surfaceContainerLowest, m.surface_container_lowest()),
                (ours.surfaceContainerLow, m.surface_container_low()),
                (ours.surfaceContainer, m.surface_container()),
                (ours.surfaceContainerHigh, m.surface_container_high()),
                (ours.surfaceContainerHighest, m.surface_container_highest()),
                (ours.primaryFixed, m.primary_fixed()), (ours.primaryFixedDim, m.primary_fixed_dim()),
                (ours.onPrimaryFixed, m.on_primary_fixed()),
                (ours.onPrimaryFixedVariant, m.on_primary_fixed_variant()),
                (ours.secondaryFixed, m.secondary_fixed()), (ours.secondaryFixedDim, m.secondary_fixed_dim()),
                (ours.onSecondaryFixed, m.on_secondary_fixed()),
                (ours.onSecondaryFixedVariant, m.on_secondary_fixed_variant()),
                (ours.tertiaryFixed, m.tertiary_fixed()), (ours.tertiaryFixedDim, m.tertiary_fixed_dim()),
                (ours.onTertiaryFixed, m.on_tertiary_fixed()),
                (ours.onTertiaryFixedVariant, m.on_tertiary_fixed_variant()),
            ];
            for (i, (got, want)) in rest.iter().enumerate() {
                assert_eq!(*got, argb(*want), "dark {dark}, role {i}");
            }
            // Containers step from the surface, as Material's ladder does.
            assert_ne!(ours.surfaceContainerLow, ours.surfaceContainer);
        }
    }

    const ROSE_SEED: u32 = 0xff8c_3241;

    #[test]
    fn a_fixed_accent_takes_every_role_from_its_seed() {
        let baseline = MaterialSchemes::default();
        let rose = accent_schemes(&baseline, Accent::Fixed(4), None);
        assert_eq!(rose, tonal_spot(c(ROSE_SEED)));
        assert_ne!(rose.light.surface, baseline.light.surface);
    }

    #[test]
    fn blue_stays_the_shipped_scheme() {
        let mut baseline = MaterialSchemes::default();
        baseline.light.primary = c(0xff44_5e91);
        assert_eq!(accent_schemes(&baseline, Accent::Fixed(0), None), baseline);
    }

    #[test]
    fn a_circle_shows_its_accents_light_primary() {
        let mut baseline = MaterialSchemes::default();
        baseline.light.primary = c(0xff44_5e91);
        assert_eq!(swatch(&baseline, Accent::Fixed(0), None), c(0xff44_5e91));
        assert_eq!(swatch(&baseline, Accent::Fixed(4), None), tonal_spot(c(ROSE_SEED)).light.primary);
    }
}
