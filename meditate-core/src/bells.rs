//! Per-session bell schedule + per-tick decision.
//!
//! Two schedule shapes — `Interval` (recurring, jittered) and `Fixed`
//! (one-shot at a target) — wrapped in an `ActiveBell` that also
//! carries the sound / vibration UUIDs and the signal-mode the shell
//! needs to dispatch the actual playback when this bell fires.
//!
//! Pure decision logic. The shell that owns the audio device and the
//! vibration motor passes `tick(elapsed, rng)` once per session tick,
//! reads the boolean it returns, and dispatches side effects (play
//! sound, fire vibration) externally. RNG is supplied by the caller —
//! the schedule doesn't carry its own state — so the shell stays
//! free to pick its own deterministic source for tests / replay.

use crate::db::{
    BellSound, BoxBreathPhaseId, Database, IntervalBell, IntervalBellKind, SessionMode,
    VibrationPattern,
};
use crate::seeds::{BUNDLED_BOWL_UUID, BUNDLED_PATTERN_PULSE_UUID};
use crate::settings_keys::{read_bool, read_signal_mode, read_str};

/// What channels a bell or phase plays through. Mirrors the
/// Sound / Vibration / Both `Adw.ToggleGroup` segments in the timer
/// setup. Used as the persisted enum behind every per-bell signal-mode
/// setting key + the `interval_bells.signal_mode` column +
/// the `box_breath_phases.signal_mode` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalMode {
    Sound,
    Vibration,
    Both,
}

impl SignalMode {
    pub fn as_db_str(self) -> &'static str {
        match self {
            SignalMode::Sound     => "sound",
            SignalMode::Vibration => "vibration",
            SignalMode::Both      => "both",
        }
    }
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "sound"     => Some(SignalMode::Sound),
            "vibration" => Some(SignalMode::Vibration),
            "both"      => Some(SignalMode::Both),
            _           => None,
        }
    }

    /// Does this mode include the sound channel? `Sound | Both`.
    pub fn includes_sound(self) -> bool {
        matches!(self, SignalMode::Sound | SignalMode::Both)
    }

    /// Does this mode include the vibration channel? `Vibration | Both`.
    pub fn includes_vibration(self) -> bool {
        matches!(self, SignalMode::Vibration | SignalMode::Both)
    }
}

// ── Bell-scheduling helpers ─────────────────────────────────────────
// Pure functions the running tick uses to decide when each configured
// bell should fire. random_unit is a caller-supplied [0, 1) random for
// the jittered intervals — keeps the helpers deterministic + testable
// while letting the shell choose its RNG (xorshift, system time, ...).

/// Compute the elapsed-secs boundary when the next ring of an
/// interval bell should fire.
///
/// `last_ring_secs` is the elapsed-secs boundary of the previous ring
/// (use 0 for the first ring of a session). `base_min` and `jitter_pct`
/// come from the bell row. `random_unit` is a caller-supplied uniform
/// in `[0, 1)`.
///
/// With `jitter_pct == 0` the offset is exactly `base_min * 60` and
/// `random_unit` is ignored. With non-zero jitter the offset is in
/// `[base * (1 - j/100), base * (1 + j/100)]`, picked linearly from
/// `random_unit`.
pub fn next_interval_ring_secs(
    last_ring_secs: u64,
    base_min: u32,
    jitter_pct: u32,
    random_unit: f64,
) -> u64 {
    let base_secs = (base_min as u64).saturating_mul(60).max(1);
    if jitter_pct == 0 {
        return last_ring_secs + base_secs;
    }
    let span = base_secs as f64 * (jitter_pct as f64) / 100.0;
    // [0, 1) → [-span, +span). Centre (0.5) lands on zero offset.
    let offset = (random_unit - 0.5) * 2.0 * span;
    let next_secs = ((base_secs as f64) + offset).round().max(1.0) as u64;
    last_ring_secs + next_secs
}

/// Compute the elapsed-secs boundary for a "T minutes from session
/// start" bell, or `None` if the bell would overlap the starting bell
/// (offset==0) or the completion sound (offset>=target).
///
/// In stopwatch mode, `total_target_secs` is `None` — only the
/// zero-offset overlap rule applies.
pub fn fixed_from_start_target_secs(
    offset_min: u32,
    total_target_secs: Option<u64>,
) -> Option<u64> {
    let offset_secs = (offset_min as u64) * 60;
    if offset_secs == 0 {
        return None;
    }
    match total_target_secs {
        Some(t) if offset_secs >= t => None,
        _ => Some(offset_secs),
    }
}

/// Compute the elapsed-secs boundary for a "T minutes before session
/// end" bell. Only meaningful in countdown mode — stopwatch mode has
/// no end so the shell skips this kind altogether. Returns `None` if
/// the bell would overlap the completion sound (offset==0) or land
/// at/before session start (offset>=total).
pub fn fixed_from_end_target_secs(
    offset_min: u32,
    total_target_secs: u64,
) -> Option<u64> {
    let offset_secs = (offset_min as u64) * 60;
    if offset_secs == 0 || offset_secs >= total_target_secs {
        return None;
    }
    Some(total_target_secs - offset_secs)
}

/// One bell's per-session schedule. Built once at the moment a session
/// enters Running (after prep, if any) and mutated in place by
/// `ActiveBell::tick` thereafter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BellSchedule {
    /// Recurring interval bell — every `base_min` minutes (give or
    /// take `jitter_pct`%). `next_ring_secs` is the absolute elapsed-
    /// since-Running mark when the next ring is due.
    Interval {
        base_min: u32,
        jitter_pct: u32,
        next_ring_secs: u64,
    },
    /// One-shot bell pinned to a known offset from session start
    /// (fixed-from-start) or session end (fixed-from-end, resolved to
    /// an absolute target at build time). Flips `fired` on first
    /// match so subsequent ticks skip it.
    Fixed {
        target_secs: u64,
        fired: bool,
    },
}

/// Per-bell active state for the running tick. Sound, vibration
/// pattern, and signal-mode travel with the schedule so the shell's
/// dispatch loop has everything it needs without a second DB lookup.
#[derive(Debug, Clone)]
pub struct ActiveBell {
    pub sound_uuid: crate::db::BellSoundUuid,
    pub vibration_pattern_uuid: crate::db::VibrationPatternUuid,
    pub signal_mode: SignalMode,
    pub schedule: BellSchedule,
}

impl ActiveBell {
    /// Per-tick decision: did this bell's ring boundary just get crossed?
    /// Mutates the schedule so the next tick won't double-fire:
    ///   - `Interval`: rerolls `next_ring_secs` using one draw from
    ///     `rng()` for the jitter pick.
    ///   - `Fixed`: flips `fired` to true.
    /// Returns `true` if the caller should dispatch this bell's
    /// playback now. `rng()` is only called on an `Interval` fire;
    /// `Fixed` and the no-fire branches never touch it.
    pub fn tick(&mut self, elapsed_secs: u64, rng: &mut impl FnMut() -> f64) -> bool {
        match &mut self.schedule {
            BellSchedule::Interval {
                base_min,
                jitter_pct,
                next_ring_secs,
            } => {
                if elapsed_secs >= *next_ring_secs {
                    let r = rng();
                    *next_ring_secs =
                        next_interval_ring_secs(*next_ring_secs, *base_min, *jitter_pct, r);
                    true
                } else {
                    false
                }
            }
            BellSchedule::Fixed { target_secs, fired } => {
                if !*fired && elapsed_secs >= *target_secs {
                    *fired = true;
                    true
                } else {
                    false
                }
            }
        }
    }
}

/// Typed key identifying which translated string the shell should
/// render for an interval-bell row's title. Variants carry the
/// numbers to substitute into the gettext template at the call
/// site — keeps the english string fragments out of core (the
/// gettext catalogue lives in the shell) while letting the shell
/// match exhaustively on the variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BellTitleKey {
    /// Recurring bell with no jitter. e.g. "Every 5 min".
    EveryNMin { minutes: u32 },
    /// Recurring bell with jitter. e.g. "Every 5 min ±20%".
    EveryNMinWithJitter { minutes: u32, jitter_pct: u32 },
    /// One-shot bell at a fixed offset from start. e.g. "At 10 min".
    AtNMin { minutes: u32 },
    /// One-shot bell at a fixed offset before end. e.g. "5 min before end".
    NMinBeforeEnd { minutes: u32 },
}

/// Pick the right typed title key for an interval-bell row.
/// The shell maps each variant to a translated string at the call
/// site (see `feedback_meditate_i18n_typed_keys.md`).
pub fn bell_title_key(bell: &IntervalBell) -> BellTitleKey {
    match bell.kind {
        IntervalBellKind::Interval => {
            if bell.jitter_pct == 0 {
                BellTitleKey::EveryNMin { minutes: bell.minutes }
            } else {
                BellTitleKey::EveryNMinWithJitter {
                    minutes: bell.minutes,
                    jitter_pct: bell.jitter_pct,
                }
            }
        }
        IntervalBellKind::FixedFromStart => BellTitleKey::AtNMin { minutes: bell.minutes },
        IntervalBellKind::FixedFromEnd => BellTitleKey::NMinBeforeEnd { minutes: bell.minutes },
    }
}

/// A name looked up in a bell-sound or vibration-pattern library.
/// Two states: the row was found and we have its display name, or
/// the row is missing (uuid empty, or set but referent gone — e.g.
/// the user deleted a custom pattern or the bundled row got
/// tombstoned via sync).
///
/// Returned from `sound_name`, `pattern_name`, and their `resolve_*`
/// counterparts in place of the older `""`-as-missing sentinel.
/// Shells decide how to render `Missing` — typically a localized
/// "Missing" subtitle plus an a11y announcement so a screen-reader
/// user knows the row needs re-picking, not just that the subtitle
/// is blank. Keeping it a two-variant enum (rather than splitting
/// `Unset` vs `Missing`) reflects the current code's behaviour;
/// shells already conflate both into the same affordance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedName {
    Resolved(String),
    Missing,
}

impl ResolvedName {
    fn from_lookup<T: AsRef<str>>(uuid: &str, found: Option<T>) -> Self {
        match (uuid.is_empty(), found) {
            (true, _) => ResolvedName::Missing,
            (false, None) => ResolvedName::Missing,
            (false, Some(name)) => ResolvedName::Resolved(name.as_ref().to_owned()),
        }
    }
}

/// Look up a bell-sound's display name from a library snapshot.
/// `Missing` when the uuid is empty or stale (post-wipe or
/// post-sync the stored uuid may point at no row); shells map that
/// to a localized "Missing" affordance so the user can re-pick.
pub fn sound_name(uuid: &str, library: &[BellSound]) -> ResolvedName {
    let found = library.iter().find(|s| s.uuid == uuid).map(|s| s.name.clone());
    ResolvedName::from_lookup(uuid, found)
}

/// Same as `sound_name` but for vibration patterns.
pub fn pattern_name(uuid: &str, library: &[VibrationPattern]) -> ResolvedName {
    let found = library.iter().find(|p| p.uuid == uuid).map(|p| p.name.clone());
    ResolvedName::from_lookup(uuid, found)
}

/// Resolve a sound UUID to its display name by reading the
/// bell-sounds library straight from `db` and delegating to
/// `sound_name`. Used by Setup-view subtitle rows that have only
/// the uuid in hand — saves callers the boilerplate of "list_bell_
/// sounds → find by uuid → name". `Missing` when the uuid is empty
/// or the row is gone.
pub fn resolve_sound_name(db: &Database, uuid: &str) -> ResolvedName {
    if uuid.is_empty() {
        return ResolvedName::Missing;
    }
    let library = db.list_bell_sounds().unwrap_or_default();
    sound_name(uuid, &library)
}

/// Resolve a vibration-pattern UUID to its display name via
/// `find_vibration_pattern_by_uuid_from_db`. `Missing` when the
/// uuid is empty or the row is gone (matches `pattern_name`).
pub fn resolve_pattern_name(db: &Database, uuid: &str) -> ResolvedName {
    if uuid.is_empty() {
        return ResolvedName::Missing;
    }
    let found = crate::db::find_vibration_pattern_by_uuid_from_db(db, uuid)
        .ok()
        .flatten()
        .map(|p| p.name);
    ResolvedName::from_lookup(uuid, found)
}

/// Resolve the Box-Breath per-phase row's two subtitle names
/// (sound + vibration pattern) for `phase`. `None` when the
/// phase row doesn't exist (shouldn't happen — the seed inserts
/// all four — but the API stays total). Used by the Setup view's
/// `refresh_boxbreath_phase_subtitles` and the Android equivalent.
pub fn phase_cue_names(
    db: &Database,
    phase: BoxBreathPhaseId,
) -> Option<(ResolvedName, ResolvedName)> {
    let row = db.get_box_breath_phase(phase).ok().flatten()?;
    Some((
        resolve_sound_name(db, row.sound_uuid.as_str()),
        resolve_pattern_name(db, row.pattern_uuid.as_str()),
    ))
}

/// Clamp a persisted `SignalMode` down to `Sound` when the runtime
/// has no haptic capability. Mirrors the gtk-side UI behaviour:
/// the user can author Vibration/Both modes in setups they share
/// across devices, but a sound-only device must not silently fire
/// nothing for a "Vibration" row. Pure boolean fold.
pub fn clamp_signal_mode_for_haptic(mode: SignalMode, haptic_available: bool) -> SignalMode {
    if haptic_available { mode } else { SignalMode::Sound }
}

/// Resolve which channels (sound, vibration) actually fire for a
/// bell. Each channel needs both gates to be open:
/// 1. the per-bell `signal_mode` (the user's choice on this bell).
/// 2. the per-mode `signal_mode` override (the user's mode-wide
///    cap — Box-Breath cues are "Vibration", Timer bells are
///    "Both", etc.).
///
/// Returns `(sound_on, vibration_on)`. Shells dispatch their
/// native audio + haptic mechanisms based on the result.
pub fn channel_allowed(per_bell: SignalMode, per_mode: SignalMode) -> (bool, bool) {
    (
        per_bell.includes_sound() && per_mode.includes_sound(),
        per_bell.includes_vibration() && per_mode.includes_vibration(),
    )
}

/// Same as `channel_allowed` but collapses the `(bool, bool)` pair
/// into the matching `SignalMode` variant. `None` means neither
/// channel fires — the caller skips emitting the Effect entirely.
pub fn effective_signal_mode(per_bell: SignalMode, per_mode: SignalMode) -> Option<SignalMode> {
    match channel_allowed(per_bell, per_mode) {
        (true, true) => Some(SignalMode::Both),
        (true, false) => Some(SignalMode::Sound),
        (false, true) => Some(SignalMode::Vibration),
        (false, false) => None,
    }
}

/// One audio + haptic cue config — used uniformly across starting
/// bell, end bell, and Box-Breath per-phase cues. The `signal_mode`
/// here is the *per-bell* / *per-phase* user choice; the per-mode
/// override gets ANDed in when the Session computes the effective
/// SignalMode at fire time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BellCue {
    pub sound_uuid: crate::db::BellSoundUuid,
    pub vibration_pattern_uuid: crate::db::VibrationPatternUuid,
    pub signal_mode: SignalMode,
}

/// Box-Breath per-phase cue config. Each phase carries its own
/// `BellCue` (or `None` when the phase row is disabled);
/// `master_enabled` is the user-facing "cues on / off" toggle the
/// Setup view shows above the per-phase rows.
#[derive(Debug, Clone, Default)]
pub struct BoxBreathCueConfig {
    pub master_enabled: bool,
    pub in_phase: Option<BellCue>,
    pub hold_in: Option<BellCue>,
    pub out_phase: Option<BellCue>,
    pub hold_out: Option<BellCue>,
}

impl BoxBreathCueConfig {
    /// Resolve a `BoxBreathPhaseId` to the configured cue, or
    /// `None` when the master toggle is off or the phase has no
    /// row configured.
    pub fn cue_for(&self, phase: BoxBreathPhaseId) -> Option<&BellCue> {
        if !self.master_enabled {
            return None;
        }
        match phase {
            BoxBreathPhaseId::In => self.in_phase.as_ref(),
            BoxBreathPhaseId::HoldIn => self.hold_in.as_ref(),
            BoxBreathPhaseId::Out => self.out_phase.as_ref(),
            BoxBreathPhaseId::HoldOut => self.hold_out.as_ref(),
        }
    }
}

// ── Cue config loaders ──────────────────────────────────────────────
//
// `*_cue_from_db` rebuild the per-cue config the Session needs at
// start time, from the settings rows the user has been editing in
// the Setup view. Centralised so the Android shell can build the
// same `BellCue` / `BoxBreathCueConfig` without re-implementing the
// per-key fallback / parse rules each time. Reads route through
// `settings_keys::read_*` so there's one canonical implementation
// of the get-or-default recipe.

/// Read the per-mode signal-mode override the user picked on the
/// Setup view's "Cues" toggle group. Defaults to `Both` (no extra
/// cap on top of per-bell signal_mode). Resolves the mode-keyed
/// setting internally via `settings_keys::signal_mode_key_for_mode`
/// so call sites stay in the `(db, SessionMode)` shape that every
/// other per-mode reader uses.
pub fn signal_mode_override_from_db(db: &Database, mode: SessionMode) -> SignalMode {
    let key = crate::settings_keys::signal_mode_key_for_mode(mode);
    read_signal_mode(db, key, SignalMode::Both)
}

/// Starting-bell cue config from the persisted settings rows.
/// `None` when the user disabled the master starting-bell toggle
/// (Session simply skips emitting `FireStartingBell` in that case).
pub fn starting_bell_cue_from_db(db: &Database) -> Option<BellCue> {
    if !read_bool(db, "starting_bell_active", false) {
        return None;
    }
    Some(BellCue {
        sound_uuid: read_str(db, "starting_bell_sound", BUNDLED_BOWL_UUID).into(),
        vibration_pattern_uuid: read_str(
            db, "starting_bell_pattern", BUNDLED_PATTERN_PULSE_UUID,
        ).into(),
        signal_mode: read_signal_mode(db, "starting_bell_signal_mode", SignalMode::Sound),
    })
}

/// End-bell cue config from the persisted settings rows. `None`
/// when the active session is stopwatch-only (no natural end) or
/// when the master end-bell toggle is off.
pub fn end_bell_cue_from_db(db: &Database, stopwatch_on: bool) -> Option<BellCue> {
    if stopwatch_on {
        return None;
    }
    if !read_bool(db, "end_bell_active", true) {
        return None;
    }
    Some(BellCue {
        sound_uuid: read_str(db, "end_bell_sound", BUNDLED_BOWL_UUID).into(),
        vibration_pattern_uuid: read_str(
            db, "end_bell_pattern", BUNDLED_PATTERN_PULSE_UUID,
        ).into(),
        signal_mode: read_signal_mode(db, "end_bell_signal_mode", SignalMode::Sound),
    })
}

/// Count of enabled, currently-firing interval bells. Used by the
/// Setup view's "Manage Bells" subtitle ("3 enabled" / "None enabled")
/// and by any shell surface that needs the same number. The
/// `stopwatch_on` filter mirrors `is_bell_inert_in_stopwatch` —
/// `FixedFromEnd` bells can't fire without a target, so they don't
/// contribute to the active count in stopwatch sessions.
pub fn interval_bells_count(db: &Database, stopwatch_on: bool) -> usize {
    db.list_interval_bells()
        .unwrap_or_default()
        .into_iter()
        .filter(|b| b.enabled)
        .filter(|b| !is_bell_inert_in_stopwatch(b.kind, stopwatch_on))
        .count()
}

/// Per-session bell schedule from the persisted state: respects the
/// master `interval_bells_active` toggle (empty schedule when off),
/// reads the interval-bell library, and delegates the per-row
/// schedule construction to `build_active_bells`. Returns `(bells,
/// seed)` where `seed` is the xorshift64 seed Session uses for
/// jitter draws — derived from `time::seed_now()` so multiple
/// sessions in the same process don't draw identical jitter.
pub fn session_bells_from_db(
    db: &Database,
    total_target_secs: Option<u64>,
    stopwatch_on: bool,
) -> (Vec<ActiveBell>, u64) {
    let seed = crate::time::seed_now();
    if !read_bool(db, "interval_bells_active", false) {
        return (Vec::new(), seed);
    }
    let rows = db.list_interval_bells().unwrap_or_default();
    build_active_bells(&rows, total_target_secs, stopwatch_on, seed)
}

/// Setup-view End Bell row's display state given the persisted
/// master toggle and the active mode's stopwatch flag. Pure
/// decision: stopwatch sessions have no natural end, so the row
/// shows insensitive + collapsed regardless of the persisted toggle;
/// the persisted value stays untouched so flipping stopwatch off
/// brings the previous expanded state back. Used by the gtk shell's
/// `refresh_end_bell_dependent_ui` and the Android equivalent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndBellRowState {
    /// `true` → row expanded + the user can collapse it; the master
    /// toggle has its persisted value. `false` → row collapsed +
    /// non-interactive (stopwatch mode).
    pub active: bool,
    /// `true` → the row reacts to taps; `false` → greyed-out chrome.
    /// Mirrors `active` today but exposed separately because the
    /// gtk shell calls `set_enable_expansion(active)` AND
    /// `set_sensitive(sensitive)` — distinct GTK methods.
    pub sensitive: bool,
}

/// Compose the Setup-view End Bell row state from the persisted
/// master toggle and the active mode's stopwatch flag. Stopwatch
/// forces inactive + insensitive regardless of the persisted value.
pub fn end_bell_row_state(db: &Database, stopwatch_on: bool) -> EndBellRowState {
    if stopwatch_on {
        return EndBellRowState { active: false, sensitive: false };
    }
    let persisted_on = read_bool(db, "end_bell_active", true);
    EndBellRowState { active: persisted_on, sensitive: true }
}

/// Per-row switch state for an entry in the interval-bell library
/// list. The persisted `enabled` flag survives a stopwatch toggle
/// (so the user's intent returns when stopwatch flips off); the row
/// just renders inert when the bell can't fire in the current mode.
/// Parallels `end_bell_row_state` shape so every per-row switch
/// surface shares the same struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BellRowSwitchState {
    /// `true` → the switch shows ON; `false` → OFF. Composed from
    /// the persisted `enabled` AND the not-inert predicate so
    /// inert-in-stopwatch bells visually flip OFF without touching
    /// the persisted flag.
    pub active: bool,
    /// `true` → the switch reacts to taps; `false` → greyed out
    /// (inert in this session-mode context).
    pub sensitive: bool,
}

/// Compose the per-row switch state for an interval-bell library
/// entry. Inert-in-stopwatch bells visually flip OFF + insensitive
/// without touching the persisted `enabled` flag.
pub fn bell_row_switch_state(
    enabled: bool,
    kind: IntervalBellKind,
    stopwatch_on: bool,
) -> BellRowSwitchState {
    let inert = is_bell_inert_in_stopwatch(kind, stopwatch_on);
    BellRowSwitchState {
        active: enabled && !inert,
        sensitive: !inert,
    }
}

/// Box-Breath per-phase cue config from the settings + phase rows.
/// Always returns a value (with `master_enabled` reflecting the
/// user's toggle); Session checks the master flag + the per-phase
/// `Option<BellCue>` internally before emitting `FireBoxBreathCue`.
pub fn box_breath_cues_from_db(db: &Database) -> BoxBreathCueConfig {
    let master_enabled = read_bool(db, "boxbreath_cues_active", false);
    let load = |phase: BoxBreathPhaseId| -> Option<BellCue> {
        let row = db.get_box_breath_phase(phase).ok().flatten()?;
        if !row.enabled {
            return None;
        }
        Some(BellCue {
            sound_uuid: row.sound_uuid,
            vibration_pattern_uuid: row.pattern_uuid,
            signal_mode: row.signal_mode,
        })
    };
    BoxBreathCueConfig {
        master_enabled,
        in_phase: load(BoxBreathPhaseId::In),
        hold_in: load(BoxBreathPhaseId::HoldIn),
        out_phase: load(BoxBreathPhaseId::Out),
        hold_out: load(BoxBreathPhaseId::HoldOut),
    }
}

// ── Bell editor invariants ──────────────────────────────────────────

/// Minimum minutes for an Interval / Fixed-from-start / Fixed-from-
/// end bell. Below 1 the schedule would either never fire (Interval
/// 0 → divide by zero on jitter) or fire instantly at start
/// (Fixed-from-start 0).
pub const BELL_MINUTES_MIN: u32 = 1;

/// Upper bound for the bell-minutes SpinRow. Two hours is plenty
/// for the longest meditation in any reasonable session.
pub const BELL_MINUTES_MAX: u32 = 120;

/// Lower bound for the bell-jitter SpinRow (0 = exact timing).
pub const BELL_JITTER_PCT_MIN: u32 = 0;

/// Upper bound for the bell-jitter SpinRow. 50% means each
/// Interval bell's next ring can land in [0.5×base, 1.5×base].
pub const BELL_JITTER_PCT_MAX: u32 = 50;

/// Defaults for a freshly-created interval bell. Used by every
/// shell's "Create new bell" affordance so an Android user adding
/// a bell gets the same starting point a gtk user does.
pub const DEFAULT_NEW_BELL_KIND: IntervalBellKind = IntervalBellKind::Interval;
pub const DEFAULT_NEW_BELL_MINUTES: u32 = 5;
pub const DEFAULT_NEW_BELL_JITTER_PCT: u32 = 0;
pub const DEFAULT_NEW_BELL_SIGNAL_MODE: SignalMode = SignalMode::Sound;

/// Whether a bell of `kind` is inert in a stopwatch session.
/// Fixed-from-end bells can't fire when there's no end to count
/// backwards from — every UI surface that shows / counts bells
/// should consult this predicate to mute them visually, and
/// `build_active_bells` already skips them at construction time.
pub fn is_bell_inert_in_stopwatch(kind: IntervalBellKind, stopwatch_on: bool) -> bool {
    stopwatch_on && kind == IntervalBellKind::FixedFromEnd
}

/// Build per-session bell schedules from raw `interval_bells` DB
/// rows. Skips disabled rows; also skips `FixedFromEnd` rows when
/// `stopwatch_on` is true (no end to count backwards from).
/// Interval rows get an initial jittered roll for `next_ring_secs`
/// using xorshift64 seeded from `seed`; the advanced state is
/// returned so the caller (typically `Session`) can continue the
/// same deterministic sequence on subsequent reroll draws.
///
/// `total_target_secs` is the planned session duration: required for
/// `FixedFromEnd` resolution, ignored otherwise.
pub(crate) fn build_active_bells(
    rows: &[IntervalBell],
    total_target_secs: Option<u64>,
    stopwatch_on: bool,
    seed: u64,
) -> (Vec<ActiveBell>, u64) {
    let mut state = seed.max(1);
    let mut bells = Vec::new();
    for row in rows {
        if !row.enabled {
            continue;
        }
        // Stopwatch sessions mute fixed-from-end bells — there's
        // no end to count backwards from. Mirrors the gtk UI's
        // grey-out at the same condition.
        if stopwatch_on && row.kind == IntervalBellKind::FixedFromEnd {
            continue;
        }
        let schedule = match row.kind {
            IntervalBellKind::Interval => {
                let (r, next) = xorshift64(state);
                state = next;
                let next_ring =
                    next_interval_ring_secs(0, row.minutes, row.jitter_pct, r);
                BellSchedule::Interval {
                    base_min: row.minutes,
                    jitter_pct: row.jitter_pct,
                    next_ring_secs: next_ring,
                }
            }
            IntervalBellKind::FixedFromStart => {
                match fixed_from_start_target_secs(row.minutes, total_target_secs) {
                    Some(t) => BellSchedule::Fixed { target_secs: t, fired: false },
                    None => continue,
                }
            }
            IntervalBellKind::FixedFromEnd => {
                let Some(total) = total_target_secs else { continue; };
                match fixed_from_end_target_secs(row.minutes, total) {
                    Some(t) => BellSchedule::Fixed { target_secs: t, fired: false },
                    None => continue,
                }
            }
        };
        bells.push(ActiveBell {
            sound_uuid: row.sound_uuid.clone(),
            vibration_pattern_uuid: row.vibration_pattern_uuid.clone(),
            signal_mode: row.signal_mode,
            schedule,
        });
    }
    (bells, state)
}

// ── xorshift64 ──────────────────────────────────────────────────────
//
// Stateless deterministic RNG used by the jitter-interval scheduling
// above and by the per-tick draws in `session::Session`. Callers
// thread the `u64` state through successive calls rather than
// holding mutable global state — keeps the algorithm pure and lets
// each session pick its own seed for tests / replay. Not crypto;
// don't use this for anything that needs to resist prediction.

/// One step of xorshift64 returning a unit-uniform `f64` in
/// `[0, 1)` and the advanced state. Caller threads `state` through
/// successive calls.
///
/// A `0` seed is internally bumped to `1` — xorshift64 outputs `0`
/// forever from a `0` seed, which would be a silent footgun.
pub fn xorshift64(state: u64) -> (f64, u64) {
    let mut s = state.max(1);
    s ^= s << 13;
    s ^= s >> 7;
    s ^= s << 17;
    // Top 53 bits → f64 in [0, 1) without losing precision.
    let unit = (s >> 11) as f64 / (1u64 << 53) as f64;
    (unit, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_bell(target_secs: u64) -> ActiveBell {
        ActiveBell {
            sound_uuid: "sound-uuid".into(),
            vibration_pattern_uuid: "pattern-uuid".into(),
            signal_mode: SignalMode::Sound,
            schedule: BellSchedule::Fixed { target_secs, fired: false },
        }
    }

    fn interval_bell(base_min: u32, jitter_pct: u32, next_ring_secs: u64) -> ActiveBell {
        ActiveBell {
            sound_uuid: "sound-uuid".into(),
            vibration_pattern_uuid: "pattern-uuid".into(),
            signal_mode: SignalMode::Sound,
            schedule: BellSchedule::Interval { base_min, jitter_pct, next_ring_secs },
        }
    }

    /// RNG fixture that returns a fixed value and counts how many times
    /// it was called. Used to prove tick consumes randomness only when
    /// it actually rerolls.
    struct CountedRng {
        value: f64,
        calls: u32,
    }
    impl CountedRng {
        fn new(value: f64) -> Self { Self { value, calls: 0 } }
        fn closure(&mut self) -> impl FnMut() -> f64 + '_ {
            || {
                self.calls += 1;
                self.value
            }
        }
    }

    // ── Fixed ──────────────────────────────────────────────────────────

    #[test]
    fn fixed_does_not_fire_before_target() {
        let mut bell = fixed_bell(60);
        let mut rng = CountedRng::new(0.5);
        assert!(!bell.tick(59, &mut rng.closure()));
        assert!(matches!(bell.schedule, BellSchedule::Fixed { fired: false, .. }));
        assert_eq!(rng.calls, 0);
    }

    #[test]
    fn fixed_fires_at_target_and_marks_fired() {
        let mut bell = fixed_bell(60);
        let mut rng = CountedRng::new(0.5);
        assert!(bell.tick(60, &mut rng.closure()));
        assert!(matches!(bell.schedule, BellSchedule::Fixed { fired: true, .. }));
        assert_eq!(rng.calls, 0, "fixed bells must not consume rng draws");
    }

    #[test]
    fn fixed_fires_at_first_tick_past_target() {
        // Tick rate is per-second on the gtk shell, so elapsed steps
        // by ≥ 1s; the bell fires the first tick where elapsed crosses
        // target, even if elapsed jumped past it (e.g. backgrounded
        // app catching up).
        let mut bell = fixed_bell(60);
        let mut rng = CountedRng::new(0.5);
        assert!(bell.tick(75, &mut rng.closure()));
    }

    #[test]
    fn fixed_does_not_re_fire_after_initial_fire() {
        let mut bell = fixed_bell(60);
        let mut rng = CountedRng::new(0.5);
        assert!(bell.tick(60, &mut rng.closure()));
        // Subsequent ticks past the target must NOT fire again.
        assert!(!bell.tick(61, &mut rng.closure()));
        assert!(!bell.tick(120, &mut rng.closure()));
        assert!(!bell.tick(3600, &mut rng.closure()));
        assert_eq!(rng.calls, 0);
    }

    // ── Interval ───────────────────────────────────────────────────────

    #[test]
    fn interval_does_not_fire_before_next_ring() {
        let mut bell = interval_bell(5, 0, 300);
        let mut rng = CountedRng::new(0.5);
        assert!(!bell.tick(299, &mut rng.closure()));
        // Schedule unchanged, no rng consumed.
        assert!(matches!(
            bell.schedule,
            BellSchedule::Interval { next_ring_secs: 300, .. }
        ));
        assert_eq!(rng.calls, 0);
    }

    #[test]
    fn interval_fires_at_next_ring_and_rerolls() {
        let mut bell = interval_bell(5, 0, 300);
        let mut rng = CountedRng::new(0.5);
        assert!(bell.tick(300, &mut rng.closure()));
        // 5 min × 60 s = 300 s base. With jitter 0% the next ring is
        // exactly 300 s after the current one — so 600 s.
        assert!(matches!(
            bell.schedule,
            BellSchedule::Interval { next_ring_secs: 600, .. }
        ));
        assert_eq!(rng.calls, 1, "interval reroll must consume exactly one rng draw");
    }

    #[test]
    fn interval_with_jitter_picks_a_value_inside_the_window() {
        // jitter_pct=20 means the next ring is in [base * 0.8, base * 1.2].
        // For base=600 s (10 min), the window is [480, 720]. Test by
        // pinning rng to 0.0 (lower bound) and 1.0 (upper bound) and
        // checking the rerolled next_ring.
        let mut bell_lo = interval_bell(10, 20, 600);
        let mut rng_lo = CountedRng::new(0.0);
        assert!(bell_lo.tick(600, &mut rng_lo.closure()));
        if let BellSchedule::Interval { next_ring_secs, .. } = bell_lo.schedule {
            assert!(
                (1080..=1200).contains(&next_ring_secs),
                "lower-rng next_ring fell outside [1080, 1200]: got {next_ring_secs}"
            );
        } else {
            panic!("schedule shape changed");
        }

        let mut bell_hi = interval_bell(10, 20, 600);
        let mut rng_hi = CountedRng::new(1.0);
        assert!(bell_hi.tick(600, &mut rng_hi.closure()));
        if let BellSchedule::Interval { next_ring_secs, .. } = bell_hi.schedule {
            assert!(
                (1200..=1320).contains(&next_ring_secs),
                "upper-rng next_ring fell outside [1200, 1320]: got {next_ring_secs}"
            );
        } else {
            panic!("schedule shape changed");
        }
    }

    #[test]
    fn interval_rerolls_only_once_per_tick_even_when_far_past() {
        // If the app was backgrounded and the tick catches up by
        // multiple ring intervals, we still fire once and reroll once.
        // Catching up is the shell's responsibility (call tick again
        // with the new elapsed), not an internal loop.
        let mut bell = interval_bell(5, 0, 300);
        let mut rng = CountedRng::new(0.5);
        assert!(bell.tick(900, &mut rng.closure()));
        assert_eq!(rng.calls, 1);
        // The next_ring rerolls relative to the prior value (300),
        // not the actual elapsed (900). Catch-up firing happens on
        // the next tick when the shell calls again.
        if let BellSchedule::Interval { next_ring_secs, .. } = bell.schedule {
            assert_eq!(next_ring_secs, 600);
        }
    }

    // ── Cross-shape ────────────────────────────────────────────────────

    // ── bell_title_key + label lookups ────────────────────────────────

    fn interval_row(kind: IntervalBellKind, minutes: u32, jitter_pct: u32) -> IntervalBell {
        IntervalBell {
            id: 0,
            uuid: "row".into(),
            kind,
            minutes,
            jitter_pct,
            sound_uuid: "s".into(),
            vibration_pattern_uuid: "p".into(),
            signal_mode: SignalMode::Sound,
            enabled: true,
            created_iso: "1970-01-01T00:00:00".into(),
        }
    }

    #[test]
    fn bell_title_key_interval_no_jitter() {
        let key = bell_title_key(&interval_row(IntervalBellKind::Interval, 5, 0));
        assert_eq!(key, BellTitleKey::EveryNMin { minutes: 5 });
    }

    #[test]
    fn bell_title_key_interval_with_jitter() {
        let key = bell_title_key(&interval_row(IntervalBellKind::Interval, 10, 20));
        assert_eq!(
            key,
            BellTitleKey::EveryNMinWithJitter { minutes: 10, jitter_pct: 20 }
        );
    }

    #[test]
    fn bell_title_key_fixed_from_start() {
        let key = bell_title_key(&interval_row(IntervalBellKind::FixedFromStart, 7, 0));
        assert_eq!(key, BellTitleKey::AtNMin { minutes: 7 });
    }

    #[test]
    fn bell_title_key_fixed_from_end() {
        let key = bell_title_key(&interval_row(IntervalBellKind::FixedFromEnd, 3, 0));
        assert_eq!(key, BellTitleKey::NMinBeforeEnd { minutes: 3 });
    }

    #[test]
    fn sound_name_finds_match_by_uuid() {
        use crate::db::BellSoundCategory;
        let lib = vec![BellSound {
            id: 0,
            uuid: "u1".into(),
            name: "Bowl".into(),
            file_path: "s1.ogg".into(),
            is_bundled: true,
            mime_type: "audio/ogg".into(),
            category: BellSoundCategory::General,
            created_iso: "1970-01-01T00:00:00".into(),
        }];
        assert_eq!(sound_name("u1", &lib), ResolvedName::Resolved("Bowl".into()));
    }

    #[test]
    fn sound_name_is_missing_on_empty_or_stale_uuid() {
        assert_eq!(sound_name("", &[]), ResolvedName::Missing);
        assert_eq!(sound_name("stale", &[]), ResolvedName::Missing);
    }

    #[test]
    fn clamp_signal_mode_for_haptic_collapses_to_sound_when_unavailable() {
        assert_eq!(
            clamp_signal_mode_for_haptic(SignalMode::Vibration, false),
            SignalMode::Sound,
        );
        assert_eq!(
            clamp_signal_mode_for_haptic(SignalMode::Both, false),
            SignalMode::Sound,
        );
        assert_eq!(
            clamp_signal_mode_for_haptic(SignalMode::Sound, false),
            SignalMode::Sound,
        );
    }

    #[test]
    fn clamp_signal_mode_for_haptic_passes_through_when_available() {
        for m in [SignalMode::Sound, SignalMode::Vibration, SignalMode::Both] {
            assert_eq!(clamp_signal_mode_for_haptic(m, true), m);
        }
    }

    #[test]
    fn channel_allowed_both_bell_both_mode_fires_everything() {
        assert_eq!(channel_allowed(SignalMode::Both, SignalMode::Both), (true, true));
    }

    #[test]
    fn channel_allowed_sound_bell_vibration_mode_fires_nothing() {
        // Per-bell wants sound, per-mode wants vibration — AND yields
        // nothing. Mirrors the "user disabled audio in this mode" path.
        assert_eq!(channel_allowed(SignalMode::Sound, SignalMode::Vibration), (false, false));
    }

    #[test]
    fn is_bell_inert_in_stopwatch_targets_only_fixed_from_end() {
        // Only FixedFromEnd goes inert with stopwatch on.
        assert!(is_bell_inert_in_stopwatch(IntervalBellKind::FixedFromEnd, true));
        // The other two kinds are fine in stopwatch.
        assert!(!is_bell_inert_in_stopwatch(IntervalBellKind::Interval, true));
        assert!(!is_bell_inert_in_stopwatch(IntervalBellKind::FixedFromStart, true));
        // Nothing's inert when stopwatch is off.
        assert!(!is_bell_inert_in_stopwatch(IntervalBellKind::FixedFromEnd, false));
        assert!(!is_bell_inert_in_stopwatch(IntervalBellKind::Interval, false));
    }

    #[test]
    fn channel_allowed_intersection_is_minimum() {
        // Per-bell Both AND per-mode Sound → sound only.
        assert_eq!(channel_allowed(SignalMode::Both, SignalMode::Sound), (true, false));
        // Per-bell Both AND per-mode Vibration → vibration only.
        assert_eq!(channel_allowed(SignalMode::Both, SignalMode::Vibration), (false, true));
        // Symmetric.
        assert_eq!(channel_allowed(SignalMode::Sound, SignalMode::Both), (true, false));
        assert_eq!(channel_allowed(SignalMode::Vibration, SignalMode::Both), (false, true));
    }

    #[test]
    fn effective_signal_mode_collapses_to_variant_or_none() {
        assert_eq!(
            effective_signal_mode(SignalMode::Both, SignalMode::Both),
            Some(SignalMode::Both)
        );
        assert_eq!(
            effective_signal_mode(SignalMode::Both, SignalMode::Sound),
            Some(SignalMode::Sound)
        );
        assert_eq!(
            effective_signal_mode(SignalMode::Sound, SignalMode::Vibration),
            None
        );
    }

    fn cue(uuid: &str) -> BellCue {
        BellCue {
            sound_uuid: uuid.into(),
            vibration_pattern_uuid: "pattern".into(),
            signal_mode: SignalMode::Both,
        }
    }

    #[test]
    fn box_breath_cue_master_off_suppresses_every_phase() {
        let cfg = BoxBreathCueConfig {
            master_enabled: false,
            in_phase: Some(cue("a")),
            hold_in: Some(cue("b")),
            out_phase: Some(cue("c")),
            hold_out: Some(cue("d")),
        };
        for phase in crate::db::BoxBreathPhaseId::all() {
            assert!(cfg.cue_for(*phase).is_none());
        }
    }

    #[test]
    fn box_breath_cue_master_on_returns_per_phase_or_none() {
        let cfg = BoxBreathCueConfig {
            master_enabled: true,
            in_phase: Some(cue("in")),
            hold_in: None,
            out_phase: Some(cue("out")),
            hold_out: None,
        };
        assert_eq!(
            cfg.cue_for(crate::db::BoxBreathPhaseId::In).map(|c| c.sound_uuid.as_str()),
            Some("in"),
        );
        assert!(cfg.cue_for(crate::db::BoxBreathPhaseId::HoldIn).is_none());
        assert_eq!(
            cfg.cue_for(crate::db::BoxBreathPhaseId::Out).map(|c| c.sound_uuid.as_str()),
            Some("out"),
        );
        assert!(cfg.cue_for(crate::db::BoxBreathPhaseId::HoldOut).is_none());
    }

    // ── build_active_bells ─────────────────────────────────────────────

    fn row(
        kind: IntervalBellKind,
        minutes: u32,
        jitter_pct: u32,
        enabled: bool,
    ) -> IntervalBell {
        IntervalBell {
            id: 0,
            uuid: "row-uuid".into(),
            kind,
            minutes,
            jitter_pct,
            sound_uuid: "row-sound".into(),
            vibration_pattern_uuid: "row-pattern".into(),
            signal_mode: SignalMode::Sound,
            enabled,
            created_iso: "1970-01-01T00:00:00".into(),
        }
    }

    #[test]
    fn build_skips_disabled_rows() {
        let rows = vec![
            row(IntervalBellKind::Interval, 5, 0, false),
            row(IntervalBellKind::Interval, 10, 0, true),
        ];
        let (bells, _) = build_active_bells(&rows, Some(1800), false, 42);
        assert_eq!(bells.len(), 1);
        assert!(matches!(
            bells[0].schedule,
            BellSchedule::Interval { base_min: 10, .. }
        ));
    }

    #[test]
    fn build_skips_fixed_from_end_when_stopwatch_on() {
        let rows = vec![
            row(IntervalBellKind::FixedFromEnd, 2, 0, true),
            row(IntervalBellKind::FixedFromStart, 5, 0, true),
        ];
        let (bells, _) = build_active_bells(&rows, None, true, 42);
        // FixedFromEnd dropped; FixedFromStart survives (it doesn't
        // need a session target).
        assert_eq!(bells.len(), 1);
        assert!(matches!(
            bells[0].schedule,
            BellSchedule::Fixed { target_secs: 300, .. }
        ));
    }

    #[test]
    fn build_advances_seed_for_each_interval_row() {
        let rows = vec![
            row(IntervalBellKind::Interval, 5, 50, true),
            row(IntervalBellKind::Interval, 10, 50, true),
            row(IntervalBellKind::Interval, 15, 50, true),
        ];
        let (_, seed_after) = build_active_bells(&rows, Some(3600), false, 42);
        // Three interval rows → three xorshift draws → state must
        // have moved off the seed.
        assert_ne!(seed_after, 42);
    }

    #[test]
    fn build_same_seed_yields_same_initial_schedules() {
        let rows = vec![
            row(IntervalBellKind::Interval, 5, 50, true),
            row(IntervalBellKind::Interval, 10, 50, true),
        ];
        let (a, sa) = build_active_bells(&rows, Some(3600), false, 12345);
        let (b, sb) = build_active_bells(&rows, Some(3600), false, 12345);
        assert_eq!(sa, sb);
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.schedule, y.schedule);
        }
    }

    #[test]
    fn build_fixed_rows_consume_no_rng_state() {
        let interval_only = vec![row(IntervalBellKind::Interval, 5, 0, true)];
        let mixed = vec![
            row(IntervalBellKind::FixedFromStart, 2, 0, true),
            row(IntervalBellKind::Interval, 5, 0, true),
            row(IntervalBellKind::FixedFromEnd, 1, 0, true),
        ];
        let (_, sa) = build_active_bells(&interval_only, Some(600), false, 7);
        let (_, sb) = build_active_bells(&mixed, Some(600), false, 7);
        // The Interval row is the only consumer of rng — both runs
        // must end at the same state.
        assert_eq!(sa, sb);
    }

    #[test]
    fn no_fire_consumes_no_rng_draw() {
        // Across both schedule shapes: a tick that doesn't fire never
        // calls rng(). Important for shells with a single shared RNG —
        // sequential bells in the dispatch loop must not perturb each
        // other's draws based on whether earlier bells fired.
        let mut interval = interval_bell(5, 20, 300);
        let mut fixed = fixed_bell(120);
        let mut rng = CountedRng::new(0.5);
        let mut closure = rng.closure();
        assert!(!interval.tick(60, &mut closure));
        assert!(!fixed.tick(60, &mut closure));
        drop(closure);
        assert_eq!(rng.calls, 0);
    }

    // ── *_cue_from_db loaders ───────────────────────────────────────

    #[test]
    fn starting_bell_cue_from_db_is_none_when_master_off() {
        let db = Database::open_in_memory().unwrap();
        assert!(starting_bell_cue_from_db(&db).is_none());
    }

    #[test]
    fn starting_bell_cue_from_db_returns_persisted_when_master_on() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting("starting_bell_active", "true").unwrap();
        db.set_setting("starting_bell_sound", "custom-sound-uuid").unwrap();
        db.set_setting("starting_bell_pattern", "custom-pattern-uuid").unwrap();
        db.set_setting("starting_bell_signal_mode", "vibration").unwrap();
        let cue = starting_bell_cue_from_db(&db).expect("master is on");
        assert_eq!(cue.sound_uuid, "custom-sound-uuid");
        assert_eq!(cue.vibration_pattern_uuid, "custom-pattern-uuid");
        assert_eq!(cue.signal_mode, SignalMode::Vibration);
    }

    #[test]
    fn starting_bell_cue_from_db_falls_back_to_bundled_defaults() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting("starting_bell_active", "true").unwrap();
        let cue = starting_bell_cue_from_db(&db).expect("master is on");
        assert_eq!(cue.sound_uuid, BUNDLED_BOWL_UUID);
        assert_eq!(cue.vibration_pattern_uuid, BUNDLED_PATTERN_PULSE_UUID);
        assert_eq!(cue.signal_mode, SignalMode::Sound);
    }

    #[test]
    fn end_bell_cue_from_db_is_none_for_stopwatch_session() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting("end_bell_active", "true").unwrap();
        assert!(end_bell_cue_from_db(&db, true).is_none());
    }

    #[test]
    fn end_bell_cue_from_db_defaults_master_to_on() {
        // First-launch state — no `end_bell_active` row at all.
        // The default in core matches the gtk shell's prior default
        // (`get_setting("end_bell_active", "true")`).
        let db = Database::open_in_memory().unwrap();
        let cue = end_bell_cue_from_db(&db, false).expect("default-on");
        assert_eq!(cue.signal_mode, SignalMode::Sound);
    }

    #[test]
    fn box_breath_cues_from_db_reflects_master_toggle() {
        let db = Database::open_in_memory().unwrap();
        let off = box_breath_cues_from_db(&db);
        assert!(!off.master_enabled);
        db.set_setting("boxbreath_cues_active", "true").unwrap();
        let on = box_breath_cues_from_db(&db);
        assert!(on.master_enabled);
    }

    #[test]
    fn signal_mode_override_from_db_defaults_to_both() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(
            signal_mode_override_from_db(&db, SessionMode::Timer),
            SignalMode::Both,
        );
    }

    #[test]
    fn signal_mode_override_from_db_reads_persisted_value() {
        let db = Database::open_in_memory().unwrap();
        db.set_setting("timer_signal_mode", "sound").unwrap();
        assert_eq!(
            signal_mode_override_from_db(&db, SessionMode::Timer),
            SignalMode::Sound,
        );
    }

    #[test]
    fn session_bells_from_db_is_empty_when_master_off() {
        let db = Database::open_in_memory().unwrap();
        let (bells, _seed) = session_bells_from_db(&db, Some(600), false);
        assert!(bells.is_empty(), "master off must yield empty schedule");
    }

    // ── next_interval_ring_secs ────────────────────────────────────

    #[test]
    fn next_interval_ring_with_zero_jitter_is_exactly_base_minutes() {
        // No jitter → next ring is last_ring + base_min*60 regardless
        // of random_unit. random_unit gets ignored entirely.
        assert_eq!(next_interval_ring_secs(0, 5, 0, 0.0), 300);
        assert_eq!(next_interval_ring_secs(0, 5, 0, 0.5), 300);
        assert_eq!(next_interval_ring_secs(0, 5, 0, 0.999), 300);
        assert_eq!(next_interval_ring_secs(300, 5, 0, 0.5), 600);
    }

    #[test]
    fn next_interval_ring_with_random_unit_at_centre_is_exactly_base() {
        assert_eq!(next_interval_ring_secs(0, 9, 30, 0.5), 540);
        assert_eq!(next_interval_ring_secs(540, 9, 30, 0.5), 1080);
    }

    #[test]
    fn next_interval_ring_random_unit_at_zero_lands_at_lower_bound() {
        // random_unit=0.0 → -span offset from base. For base=9 ±30%,
        // span = 540 * 0.30 = 162, so 540 - 162 = 378.
        assert_eq!(next_interval_ring_secs(0, 9, 30, 0.0), 378);
    }

    #[test]
    fn next_interval_ring_random_unit_just_below_one_lands_near_upper_bound() {
        // random_unit just below 1.0 → just below +span. For base=9 ±30%,
        // upper bound is 540 + 162 = 702.
        let v = next_interval_ring_secs(0, 9, 30, 0.9999);
        assert!(v <= 702 && v >= 700, "got {}", v);
    }

    #[test]
    fn next_interval_ring_stays_within_jitter_window_for_every_unit() {
        let base = 9 * 60u64;
        let jitter_pct = 30u32;
        let span = base as f64 * jitter_pct as f64 / 100.0;
        let lo = (base as f64 - span).round() as u64;
        let hi = (base as f64 + span).round() as u64;
        for i in 0..=10 {
            let u = (i as f64) / 10.0;
            let v = next_interval_ring_secs(0, 9, jitter_pct, u);
            assert!(v >= lo && v <= hi,
                "u={} produced {} outside [{}, {}]", u, v, lo, hi);
        }
    }

    #[test]
    fn next_interval_ring_zero_minutes_clamps_to_one_second() {
        // base=0 doesn't make sense (UI prevents it via SpinRow min),
        // but the helper still has to return a usable u64 — clamping
        // to 1 second is the harmless choice.
        assert_eq!(next_interval_ring_secs(0, 0, 0, 0.5), 1);
    }

    // ── fixed_from_start_target_secs ──────────────────────────────

    #[test]
    fn fixed_from_start_returns_offset_when_inside_target() {
        assert_eq!(fixed_from_start_target_secs(10, Some(1800)), Some(600));
    }

    #[test]
    fn fixed_from_start_returns_offset_in_stopwatch_mode() {
        assert_eq!(fixed_from_start_target_secs(10, None), Some(600));
    }

    #[test]
    fn fixed_from_start_is_none_at_zero_offset() {
        assert_eq!(fixed_from_start_target_secs(0, Some(1800)), None);
        assert_eq!(fixed_from_start_target_secs(0, None), None);
    }

    #[test]
    fn fixed_from_start_is_none_at_or_beyond_target() {
        assert_eq!(fixed_from_start_target_secs(30, Some(1800)), None);
        assert_eq!(fixed_from_start_target_secs(45, Some(1800)), None);
    }

    // ── fixed_from_end_target_secs ────────────────────────────────

    #[test]
    fn fixed_from_end_returns_target_minus_offset() {
        assert_eq!(fixed_from_end_target_secs(5, 1800), Some(1500));
    }

    #[test]
    fn fixed_from_end_is_none_at_zero_offset() {
        assert_eq!(fixed_from_end_target_secs(0, 1800), None);
    }

    #[test]
    fn fixed_from_end_is_none_at_or_beyond_total() {
        assert_eq!(fixed_from_end_target_secs(30, 1800), None);
        assert_eq!(fixed_from_end_target_secs(45, 1800), None);
    }

    // ── xorshift64 ───────────────────────────────────────────────

    #[test]
    fn xorshift64_unit_is_in_half_open_zero_one() {
        let mut state = 12345u64;
        for _ in 0..10_000 {
            let (u, next) = xorshift64(state);
            assert!((0.0..1.0).contains(&u), "unit out of [0,1): {u}");
            state = next;
        }
    }

    #[test]
    fn xorshift64_same_seed_yields_same_sequence() {
        let mut a = 42u64;
        let mut b = 42u64;
        for _ in 0..100 {
            let (ua, na) = xorshift64(a);
            let (ub, nb) = xorshift64(b);
            assert_eq!(ua, ub);
            assert_eq!(na, nb);
            a = na;
            b = nb;
        }
    }

    #[test]
    fn xorshift64_zero_seed_is_bumped_to_one() {
        // Without the bump, xorshift64 would stay 0 forever and
        // every draw would be 0.0. Verify the first call already
        // recovers.
        let (u, next) = xorshift64(0);
        assert!(u > 0.0, "zero seed should not produce a 0 unit");
        assert!(next != 0, "zero seed should not stay at 0");
    }

    #[test]
    fn xorshift64_state_does_not_collapse_over_long_runs() {
        // Rough quality check: 10k draws on a single seed shouldn't
        // produce the same unit twice in adjacent positions or
        // collapse to a constant. Not crypto — but if the algorithm
        // were broken (e.g. all-zero state), this would catch it.
        let mut state = 0xDEAD_BEEFu64;
        let mut prev = -1.0;
        let mut all_same = true;
        for _ in 0..10_000 {
            let (u, next) = xorshift64(state);
            if prev != -1.0 && (u - prev).abs() > 0.0 {
                all_same = false;
            }
            prev = u;
            state = next;
        }
        assert!(!all_same, "xorshift64 collapsed to a constant");
    }
}
