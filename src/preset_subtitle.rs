//! gtk-shell side of the preset-row subtitle. The structural
//! decomposition lives in `meditate_core::format::preset_subtitle_parts`;
//! this module maps the typed parts onto gettext-translated strings
//! at the i18n boundary. Both the Setup view's home-page starred list
//! and the standalone preset chooser render the same subtitle, so the
//! helper lives at crate level instead of inside `timer::imp`.

use std::collections::HashMap;

use meditate_core::db::Preset;
use meditate_core::format::{
    preset_subtitle_parts, BellsPart, BoxBreathAfter, TimingPart,
};

use crate::i18n::gettext;

/// Compose the preset-row subtitle as
/// "<timing> · <label name> · <bells>" (parts joined with " · ").
/// `label_names` is a pre-resolved uuid → name map (the caller does
/// one DB roundtrip per rebuild instead of one per row). Returns an
/// empty string when the preset's `config_json` is unparseable.
pub fn preset_subtitle(p: &Preset, label_names: &HashMap<String, String>) -> String {
    let Some(parts) = preset_subtitle_parts(&p.config_json) else {
        return String::new();
    };

    let render_duration = |mins: u32| {
        gettext("{n} min").replace("{n}", &mins.to_string())
    };

    let mut out: Vec<String> = Vec::new();
    match parts.timing {
        TimingPart::Stopwatch => out.push(gettext("Stopwatch")),
        TimingPart::Duration { mins } => out.push(render_duration(mins)),
        TimingPart::BoxBreath {
            inhale_secs,
            hold_full_secs,
            exhale_secs,
            hold_empty_secs,
            after,
        } => {
            out.push(format!(
                "{}-{}-{}-{}",
                inhale_secs, hold_full_secs, exhale_secs, hold_empty_secs,
            ));
            match after {
                BoxBreathAfter::Stopwatch => out.push(gettext("Stopwatch")),
                BoxBreathAfter::Duration { mins } => out.push(render_duration(mins)),
            }
        }
    }
    if let Some(uuid) = parts.label_uuid.as_ref() {
        if let Some(name) = label_names.get(uuid) {
            out.push(name.clone());
        }
    }
    match parts.bells {
        Some(BellsPart::One) => out.push(gettext("1 bell")),
        Some(BellsPart::Many(n)) => {
            out.push(gettext("{n} bells").replace("{n}", &n.to_string()))
        }
        None => {}
    }
    out.join(" · ")
}
