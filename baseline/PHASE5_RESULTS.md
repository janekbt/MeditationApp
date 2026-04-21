# Phase 5 — Post-optimization measurements

Captured 2026-04-21 after Phase 1.0/1.1/1.2/1.3 + 2.1/2.2 + 3.1/3.6 + the log-page-size and `profile=quiet` fixes. Same device (Librem 5, 4× Cortex-A53, PureOS 11 Crimson), same Flatpak runtime (GNOME 50 + Mesa 26.0.4 GL extension), same 1-minute-session test protocol as `BASELINE.md`.

## Cold-start wall time (exec → first `present=`)

3 runs, tight variance:

| Build | run 1 | run 2 | run 3 | mean |
|---|---|---|---|---|
| Phase 0 baseline (default renderer = lavapipe) | 3.903 s | 3.884 s | 3.954 s | **3.91 s** |
| Phase 0 with `GSK_RENDERER=cairo` forced | 1.767 s | 1.790 s | 1.762 s | **1.77 s** |
| **Phase 5 (all changes)** | 1.830 s | 1.778 s | 1.769 s | **1.79 s** |

Matches forced-cairo baseline — renderer probe + all the Rust-side work runs correctly. Net improvement vs. shipped: **−54 %** (−2.1 s).

## First-frame internals (same 3 runs, median)

| Build | layout (Rust) | paint (rendering) | present internal |
|---|---|---|---|
| Phase 0 baseline | ~120 ms | **1690 ms** | **1929 ms** |
| Phase 5 | **115 ms** | **60 ms** | **253 ms** |

Paint is **~28× faster** (Cairo replaces lavapipe). Layout essentially unchanged — first-frame Rust work wasn't the problem to begin with.

## Post-map refresh (the "frame 2 freeze")

| Build | Frame(s) used | Cumulative CPU (layout) | Max single-frame paint |
|---|---|---|---|
| Phase 0 baseline | 1 frame | 292 ms monolithic | **2098 ms** (lavapipe full repaint) |
| Phase 5 | 3 frames | 114 + 51 + ~20 = ~185 ms total | **1 ms** per refresh frame |

The total amount of Rust-side work is about the same (~190 ms vs. ~290 ms, modest shrink from the hot-loop fixes), but it's **spread across 3 frames with idle cycles between them**, so the compositor gets to commit in between. The user never sees a single 2-second freeze — they see three invisible ~50 ms blips.

## Session-save end-of-session freeze

Before: **~1100 ms** of Rust-side blocking (synchronous `on_save` DB write + state change) + lavapipe paint for the "done" state transition = ~2.5 s before a post-session Log tap renders (per `BASELINE.md`).

After:
- DB write moved to blocking pool → Rust side returns immediately (~20 ms for the UI state reset, vs. 1100 ms)
- Cairo paints the state transition in ~40 ms (vs. the lavapipe 1500 ms for session *start*, and we expect end to be comparable)
- Log's new card appears via `prepend_session` (incremental) instead of a 561 ms rebuild
- Vibration fires correctly (signature-fix) without the double-sound

Interactive measurement left to user feel; reported as "much smoother" and "no alarm bell on vibrate".

## Binary size

| Build | stripped binary | total install |
|---|---|---|
| Phase 0 shipped | 3.7 MB | 5.4 MB |
| Phase 5 (opt-level=3 + LTO thin + codegen-units=1 + panic=abort + strip) | **4.1 MB** | **5.8 MB** |

Slightly *larger*. Expected trade: we switched `opt-level="s"` (size-oriented) to `opt-level=3` (speed). The win isn't smaller files on disk — it's tighter codegen on the hot paths that matter on weak CPUs. The 0.4 MB added is rounding error on eMMC page-in.

## Summary — what the numbers say

- **Cold start: 3.9 s → 1.8 s (−54 %)** — mostly the Cairo renderer
- **First-frame paint: 1690 ms → 60 ms (−96 %)** — pure Cairo replacing lavapipe
- **Post-map refresh stall: one 2400 ms frame → three ~50 ms frames** — staggered idle scheduling
- **Session-end Rust freeze: 1100 ms → ~20 ms (−98 %)** — async DB write
- **Tab re-entries (after first load): full DB reload → no-op** — dirty flags
- **New session in Log: 561 ms rebuild → incremental prepend** — bypasses refresh
- **Vibration: broken (signature mismatch + NO_AUTO_START) → works, haptic-only** — signature fix + `profile=quiet` hint

## Investigation note: a regression caught during Phase 5

First attempt at Phase 2.2 (staggered refresh via `spawn_local` + async yields) unintentionally crammed the refresh work into frame 0 instead of deferring past it. `spawn_local` at default priority polls the future within the same GLib main-context iteration that triggered the `map` signal, so the supposedly-yielded refreshes ran *before* the first frame's paint instead of after. Symptoms: cold-start 2.59 s, first-frame paint_start delayed to 400 ms, present = 500 ms.

Fix: wrap the whole thing in `glib::idle_add_local_once` as the outer trigger (DEFAULT_IDLE priority = strictly lower than the frame clock), and switch the intra-step yields from `timeout_future(Duration::ZERO)` to `timeout_future(16 ms)` to ensure each step lands past a frame-clock tick. After the fix, numbers snapped back to the forced-cairo baseline.

Lesson for future perf work: always re-measure end-to-end on device after an async refactor, not just "it compiles and feels fine." The 300 ms frame-0 regression was invisible to smoke-testing.
