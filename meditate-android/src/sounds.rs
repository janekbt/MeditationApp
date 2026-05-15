//! Bundled bell-sound audio (B-3a).
//!
//! The GTK shell ships these as GResource OGGs; Android has no
//! GResource, and this xbuild setup has no non-Slint asset
//! pipeline, so the bytes are `include_bytes!`'d into the binary
//! (1.2 MB total — same "compiled in" model Slint uses for the
//! UI SVGs) and extracted to `<data_dir>/sounds/` on startup.
//! The `bell_sounds` rows are then seeded with those absolute
//! file paths so `MediaPlayer` (B-3b) can play them by path.
//!
//! UUIDs + display names mirror the GTK shell's
//! `BUNDLED_BELL_SOUNDS` (`meditate-gtk/src/db/mod.rs`) so a
//! setting that points at `BUNDLED_BOWL_UUID` resolves
//! identically on both shells. "Kanshō" is spelled "Kansho"
//! here — the FP5 Android-15 font's coverage of combining /
//! extended-Latin glyphs is unreliable (same rule that bans the
//! ✓/→ glyphs); the macron would tofu in the chooser.

#![cfg(target_os = "android")]

use std::path::Path;

struct Bundled {
    uuid: &'static str,
    name: &'static str,
    file: &'static str,
    mime: &'static str,
    bytes: &'static [u8],
}

const BUNDLED: &[Bundled] = &[
    Bundled {
        uuid: "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0001",
        name: "Singing Bowl",
        file: "bowl.ogg",
        mime: "audio/ogg",
        bytes: include_bytes!("../assets/sounds/bowl.ogg"),
    },
    Bundled {
        uuid: "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0002",
        name: "Bell",
        file: "bell.ogg",
        mime: "audio/ogg",
        bytes: include_bytes!("../assets/sounds/bell.ogg"),
    },
    Bundled {
        uuid: "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0003",
        name: "Gong",
        file: "gong.ogg",
        mime: "audio/ogg",
        bytes: include_bytes!("../assets/sounds/gong.ogg"),
    },
    Bundled {
        uuid: "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0004",
        name: "Tibetan Singing Bowl",
        file: "tibetan-bowl-medium.ogg",
        mime: "audio/ogg",
        bytes: include_bytes!("../assets/sounds/tibetan-bowl-medium.ogg"),
    },
    Bundled {
        uuid: "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0005",
        name: "Inkin",
        file: "inkin.ogg",
        mime: "audio/ogg",
        bytes: include_bytes!("../assets/sounds/inkin.ogg"),
    },
    Bundled {
        uuid: "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0006",
        name: "Tingsha",
        file: "tingsha.ogg",
        mime: "audio/ogg",
        bytes: include_bytes!("../assets/sounds/tingsha.ogg"),
    },
    Bundled {
        uuid: "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0007",
        name: "Kansho",
        file: "kansho.ogg",
        mime: "audio/ogg",
        bytes: include_bytes!("../assets/sounds/kansho.ogg"),
    },
    Bundled {
        uuid: "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0008",
        name: "Burmese Brass Bell",
        file: "burmese-brass.ogg",
        mime: "audio/ogg",
        bytes: include_bytes!("../assets/sounds/burmese-brass.ogg"),
    },
    Bundled {
        uuid: "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c0009",
        name: "Chau Gong",
        file: "chau-gong.ogg",
        mime: "audio/ogg",
        bytes: include_bytes!("../assets/sounds/chau-gong.ogg"),
    },
    Bundled {
        uuid: "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c000a",
        name: "Crystal Bowl",
        file: "crystal-bowl.ogg",
        mime: "audio/ogg",
        bytes: include_bytes!("../assets/sounds/crystal-bowl.ogg"),
    },
    Bundled {
        uuid: "f0c2e8a1-3a72-4d4f-9c8b-1b0e5d8c000b",
        name: "Woodblock",
        file: "woodblock.ogg",
        mime: "audio/ogg",
        bytes: include_bytes!("../assets/sounds/woodblock.ogg"),
    },
];

/// Extract every bundled OGG to `<data_dir>/sounds/` and seed the
/// `bell_sounds` table with their absolute paths.
///
/// Extraction is idempotent and runs on every startup (a file is
/// rewritten only when missing or its size differs from the
/// embedded copy), so the audio always exists on disk even though
/// `seed_bell_sounds_with_paths` itself is one-shot
/// (`BELLS_SEEDED_KEY`). The data dir is process-stable, so the
/// once-seeded absolute paths stay valid across runs.
///
/// Failures are logged, never fatal: a missing bell sound must
/// not stop the app from opening (the chooser just shows fewer
/// rows / a Missing affordance).
pub fn extract_and_seed(db: &meditate_core::Database, data_dir: &Path) {
    let dir = data_dir.join("sounds");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        meditate_core::log(
            "sounds.extract",
            &format!("create_dir_all {dir:?} FAILED: {e:?}"),
        );
        return;
    }

    let mut paths: Vec<(&str, &str, String, &str)> =
        Vec::with_capacity(BUNDLED.len());
    for b in BUNDLED {
        let p = dir.join(b.file);
        let up_to_date = std::fs::metadata(&p)
            .map(|m| m.len() == b.bytes.len() as u64)
            .unwrap_or(false);
        if !up_to_date {
            if let Err(e) = std::fs::write(&p, b.bytes) {
                meditate_core::log(
                    "sounds.extract",
                    &format!("write {p:?} FAILED: {e:?}"),
                );
                continue;
            }
        }
        paths.push((b.uuid, b.name, p.to_string_lossy().into_owned(), b.mime));
    }

    let rows: Vec<(&str, &str, &str, &str)> = paths
        .iter()
        .map(|(u, n, p, m)| (*u, *n, p.as_str(), *m))
        .collect();
    if let Err(e) = db.seed_bell_sounds_with_paths(&rows) {
        meditate_core::log(
            "sounds.seed",
            &format!("seed_bell_sounds_with_paths FAILED: {e:?}"),
        );
    }
}
