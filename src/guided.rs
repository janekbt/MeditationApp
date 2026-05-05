//! Guided-meditation file library + playback.
//!
//! This module owns the user-imported guided-meditation tracks: the
//! file picker for transient "Open File" plays, the import flow that
//! transcodes a picked file to OGG and stashes it under the per-device
//! data dir, and the eventual chooser page for managing the saved
//! library. The on-disk row lives in `meditate_core::db::guided_files`
//! and rides sync via `guided_file_*` events.
//!
//! Phase 3 lands the smallest piece: a synchronous duration probe.
//! Subsequent phases build the file picker → transcode → DB-insert
//! pipeline and the Setup-view UI on top of this.

use std::path::Path;

/// Probe the duration of an audio file in seconds, using a paused
/// gstreamer `playbin` pipeline. Synchronous — for typical guided-
/// meditation files (tens of MB) this returns within a few hundred
/// milliseconds even on the Librem 5; the import / Open-File code
/// blocks the UI for that window.
///
/// Why playbin instead of pbutils' Discoverer: pbutils requires the
/// `gstreamer-plugins-base-dev` system package to build against, which
/// isn't part of Debian's `libgstreamer-plugins-base1.0-0` runtime
/// metadata package. Using only the core `gstreamer` crate avoids the
/// extra dev-package dependency on contributors' machines and matches
/// the approach the existing bell-sound transcode pipeline already
/// uses.
///
/// Pattern:
/// 1. Build a `playbin` element with our file URI as input.
/// 2. Replace its sinks with `fakesink` so transitioning to PAUSED
///    doesn't open audio devices or render frames.
/// 3. Set state to PAUSED — playbin negotiates the pipeline up to the
///    sinks and the duration query becomes resolvable.
/// 4. Wait for the state transition (or fail on timeout / bus error).
/// 5. `query_duration` and convert ns → s.
/// 6. Tear down to NULL.
///
/// Returns `Err` with a human-readable string for any failure path
/// (gst init, element creation, state transition, query). Callers
/// surface the error as a toast on the file picker.
pub fn probe_duration_secs(path: &Path) -> Result<u32, String> {
    use gstreamer as gst;
    use gst::prelude::*;

    gst::init().map_err(|e| format!("gst init failed: {e}"))?;

    // playbin takes a URI rather than a path. Use the canonical
    // file:// URI for an absolute path; relative paths get an
    // absolute-canonical step first via std::fs::canonicalize so the
    // resulting URI is well-formed.
    let abs = path
        .canonicalize()
        .map_err(|e| format!("canonicalize {}: {e}", path.display()))?;
    let uri = format!("file://{}", abs.to_string_lossy());

    let pipeline = gst::ElementFactory::make("playbin")
        .property("uri", &uri)
        .build()
        .map_err(|e| format!("create playbin: {e}"))?;

    // Swap in fakesinks so PAUSED doesn't try to open an audio device
    // (we're only probing) and doesn't allocate a video surface for
    // files that happen to have a video stream.
    let audio_sink = gst::ElementFactory::make("fakesink")
        .property("sync", false)
        .build()
        .map_err(|e| format!("create audio fakesink: {e}"))?;
    let video_sink = gst::ElementFactory::make("fakesink")
        .property("sync", false)
        .build()
        .map_err(|e| format!("create video fakesink: {e}"))?;
    pipeline.set_property("audio-sink", &audio_sink);
    pipeline.set_property("video-sink", &video_sink);

    pipeline
        .set_state(gst::State::Paused)
        .map_err(|e| format!("set state Paused: {e}"))?;

    // Block until the asynchronous PAUSED transition completes.
    // 5 s is generous — on the Librem 5 a 30-min OGG resolves in
    // tens of milliseconds; if we hit 5 s something is structurally
    // wrong (corrupted file, missing decoder for the format).
    let timeout = gst::ClockTime::from_seconds(5);
    let (state_change, _, _) = pipeline.state(timeout);
    state_change.map_err(|e| format!("waiting for Paused: {e}"))?;

    // Drain any error messages on the bus so a decoder failure
    // surfaces here rather than silently returning duration=0.
    if let Some(bus) = pipeline.bus() {
        while let Some(msg) = bus.pop_filtered(&[gst::MessageType::Error]) {
            use gst::MessageView::Error;
            if let Error(err) = msg.view() {
                let _ = pipeline.set_state(gst::State::Null);
                return Err(format!(
                    "{} ({})",
                    err.error(),
                    err.debug().unwrap_or_default()
                ));
            }
        }
    }

    let duration: Option<gst::ClockTime> = pipeline.query_duration();
    let _ = pipeline.set_state(gst::State::Null);

    let nanos = duration
        .ok_or_else(|| format!("duration unknown for {}", path.display()))?
        .nseconds();
    // Round to the nearest second. A 9.8 s file is reported as 10 s
    // even though it ends slightly earlier — close enough for a hero
    // countdown that's only valid at second resolution anyway.
    Ok((nanos.div_ceil(1_000_000_000)) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Probe one of the bundled bell sounds — `bell.ogg` is ~9.83 s
    /// post-trim. The duration is rounded to the next whole second
    /// (probe ceilings sub-second tails) so we expect 10.
    ///
    /// Skipped silently if `bell.ogg` is missing from the working tree
    /// (cargo-released crates don't ship the sound assets); on a
    /// repo-local `cargo test` it always runs.
    #[test]
    fn probe_duration_secs_returns_close_to_known_bell_length() {
        let p = std::path::Path::new("data/sounds/bell.ogg");
        if !p.exists() {
            // Defensive — the assert below would also fail, but a
            // skip-with-message reads better than a `canonicalize`
            // panic on machines without the asset.
            eprintln!("skipping: data/sounds/bell.ogg not present");
            return;
        }
        let secs = probe_duration_secs(p).expect("probe should succeed");
        assert!(
            (9..=11).contains(&secs),
            "bell.ogg ≈ 9.83 s — probe returned {secs}",
        );
    }
}
