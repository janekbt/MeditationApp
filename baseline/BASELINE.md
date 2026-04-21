# Phase 0 Baseline — MeditationApp on Librem 5

Captured 2026-04-21 at commit `39353b0` (v26.4.2), against the installed Flatpak (`io.github.janekbt.Meditate/aarch64/master`).

## Device

- **Librem 5**, i.MX 8M Quad (4× Cortex-A53 @ 1.5 GHz)
- PureOS 11 Crimson, kernel 6.12.80
- 3 GB RAM
- Phoc 0.33.0 / Phosh 0.34.0
- Host Mesa 22.3.6 (old)
- Flatpak runtime: **GNOME 50**, with **Mesa 26.0.4** via `org.freedesktop.Platform.GL.default/25.08`

## Installed app

- Install size: **5.4 MB**
- Binary size (stripped): **3.7 MB**
- Runtime: GNOME 50 (GTK 4.x modern, libadwaita 1.7+)

## Renderer diagnosis (load-bearing finding)

Without any override, GTK selects renderers in this order on-device:
1. Vulkan → **rejected**: "Not using Vulkan: device is CPU" (etnaviv GC7000 has no Mesa Vulkan driver; only a proprietary NXP blob exists)
2. GL (via ngl) → **rejected**: EGL reports "No EGL configuration available" despite EGL 1.5 being present and GLES 3.0 / GL 3.3 context creation all succeeding
3. **Fallback: `GskVulkanRenderer` on lavapipe (CPU software Vulkan)**

Forcing `GSK_RENDERER=cairo` bypasses lavapipe entirely and uses the Cairo software rasterizer. Mature, CPU-friendly, decisively faster on 2D UI.

Forcing GL with Mesa version overrides (`MESA_GLES_VERSION_OVERRIDE=3.0` etc.) actually *enables* GskGLRenderer on etnaviv, but first-frame paint climbs to ~2300 ms (vs ~254 ms for Cairo) — shader compilation and driver overhead dominate a UI workload. GL on etnaviv is a net loss for this app.

## Quantitative baseline (3 runs each, tight variance)

| Metric | Default (shipped) | `GSK_RENDERER=cairo` | Delta |
|---|---|---|---|
| Cold-start wall time (exec → first `present=`) | 3.90 ± 0.04 s | **1.77 ± 0.01 s** | −55% |
| First-frame internal paint (`present` ms) | 1929 ± 11 ms | **254 ± 2 ms** | −87% |
| Post-map "refresh bundle" paint (frame ~2) | ~2115 ms | **~32 ms** | −98% |
| Warm idle frame paint | — (no frames captured) | **~2 ms** | — |
| Layout→paint_start gap on post-map frame (= Rust CPU work) | ~290 ms | ~290 ms | 0 (identical) |

The identical ~290 ms layout→paint gap in both configs confirms the three-refreshes-plus-sound-preload bundle in `src/window/imp.rs:67–87` is a **CPU-side** stall, independent of renderer. This is what Phase 2.2 targets.

## Interactive measurements

Default renderer, user-driven taps, 2026-04-21 14:47. Timing was imperfect so per-tap attribution is approximate, but the envelope is clear.

| Event | Rust (layout) | Paint | Total |
|---|---|---|---|
| Post-map refresh bundle (startup) | 292 ms | 2098 ms | **2390 ms** |
| Cold Stats entry (worst tab switch) | 555 ms | 210 ms | **765 ms** |
| Typical warm tab switch | 0–40 ms | 37–90 ms | **37–91 ms** |
| Mixed (cold re-entry) | 167 ms | 184 ms | 351 ms |

**Takeaways:**
- Once lavapipe is "warm" (shaders/pipelines cached), subsequent paints drop to 50–200 ms — so lavapipe's real cost is not every frame, it's *first* paint and any full-window redraw (which is what the post-map refresh triggers). The 30× Cairo speedup on first frame doesn't necessarily apply to warm tab switches.
- The 555 ms Rust-side cost on cold Stats entry is `reload_all()` — 10 DB queries + widget rebuild. Phase 3.1 (dirty flag) eliminates this on re-entry; Phase 2.2 (stagger) spreads it out; Phase 3.2/3.3 (hot-loop allocs, xlabel reuse) trim the baseline.
- Warm tab switches are already ~50 ms — acceptable. The perceived sluggishness comes from startup + first-use of each view, not steady state.

### Session-save freeze (default renderer)

1-minute session, user tapped Stats then Log immediately after the end chime. Vibration was disabled by the user, confirmed by absence of any 2-second gap matching `call_sync`'s timeout.

| Event | Blocking source | Measured |
|---|---|---|
| Session start → running state visible | lavapipe full-paint | ~1500 ms |
| Timer ticks during session | small text diff, lavapipe warm | ~10 ms each |
| Session end → UI responds | Rust-side (`on_save` + state change) | **~1100 ms** |
| First tap (Stats) after end | lavapipe full-paint | ~650 ms |
| Second tap (Log) after end | Rust list rebuild + paint | 777 ms (561 ms Rust + 216 ms paint) |
| **Total "stuck" time end → Log rendered** | combined | **~2.5 s** |

**Takeaways:**
- Vibration is off on this user's device — **Phase 1.3 is not load-bearing for them**, but it is for anyone with vibration enabled on a device without feedbackd.
- The **~1100 ms Rust-side work at session end** is the Phase 2.1 target: `on_save` on the main thread does the DB insert (fsync-heavy without `synchronous=NORMAL`), triggers stats/log invalidation signals, and preps the sound pipeline.
- The **~561 ms Log list rebuild** on cold re-entry is Phase 3.5-adjacent: `log/imp.rs:refresh` currently clears and re-adds every card even when only one new session was appended. Incremental append would drop this to a few ms.
- Session *start* also has a big 1500 ms lavapipe paint (full-state transition). Phase 1.0 (renderer) eliminates this.

## Raw artifacts

- `frames-default.log` — 25 s frame log, default renderer
- `frames-cairo.log` — 25 s frame log, `GSK_RENDERER=cairo`
- `med_default_{1,2,3}.log` — 3 cold-start runs, default
- `med_cairo_{1,2,3}.log` — 3 cold-start runs, Cairo

## Implication for the attack plan

Phase 1.0 (renderer env-var) alone is the single biggest perceivable win. Every other code-level change is additive on top. Recommendation: ship 1.0 first as its own PR — it's low-risk (behind a runtime probe or gated to mobile devices) and users will feel a 2-second improvement in cold-start and a 2000 ms improvement in the post-startup freeze without any Rust changes at all.
