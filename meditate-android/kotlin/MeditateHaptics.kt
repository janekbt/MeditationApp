// Haptics bridge for Phase 5 bell-cue vibration. Called from Rust
// via JNI (see `meditate-android/src/haptics.rs`) through the same
// app-classloader → loadClass → call_static_method path the
// foreground service uses.
//
// `vibrateWaveform` plays the `(amplitude, duration_ms)` envelope
// `meditate_core::vibration::build_master_envelope` produces;
// `cancel` stops an in-flight vibration (preview Stop / supersede).
//
// minSdk is 26, so `VibrationEffect` is always available — no
// legacy `vibrate(long)` fallback needed. `VibratorManager` is
// API 31+, so the system-service lookup is version-split.

package io.github.janekbt.Meditate

import android.content.Context
import android.os.Build
import android.os.VibrationAttributes
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.util.Log

object MeditateHaptics {
    private const val TAG = "MeditateHaptics"

    // Waveform playback (B-2a). `timings[i]` is segment i's
    // duration in ms; `amplitudes[i]` is 0 (off) or 1..255.
    // Built Rust-side from `meditate_core::vibration::
    // build_master_envelope`. repeat = -1 → play once.
    @JvmStatic
    fun vibrateWaveform(
        context: Context,
        timings: LongArray,
        amplitudes: IntArray,
    ) {
        if (timings.isEmpty() || timings.size != amplitudes.size) return
        val vibrator = resolveVibrator(context)
        if (vibrator == null) {
            Log.w(TAG, "no Vibrator service resolved")
            return
        }
        if (!vibrator.hasVibrator()) {
            Log.w(TAG, "device reports hasVibrator()=false")
            return
        }
        emit(vibrator, VibrationEffect.createWaveform(timings, amplitudes, -1))
    }

    // Stop any in-flight vibration (preview supersede / Stop).
    @JvmStatic
    fun cancel(context: Context) {
        resolveVibrator(context)?.cancel()
    }

    // USAGE_ALARM: a bell cue is an intentional, time-critical
    // signal — not incidental touch feedback. Android 13+
    // classifies un-attributed vibrations and the device's
    // haptic / DND settings can silently drop them; the ALARM
    // usage is exempt (same rationale as an alarm-clock buzz).
    // The attributed overload is API 33+; minSdk-26..32 falls
    // back to the bare call.
    private fun emit(vibrator: Vibrator, effect: VibrationEffect) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            val attrs = VibrationAttributes.Builder()
                .setUsage(VibrationAttributes.USAGE_ALARM)
                .build()
            vibrator.vibrate(effect, attrs)
        } else {
            @Suppress("DEPRECATION")
            vibrator.vibrate(effect)
        }
    }

    // VibratorManager (API 31+) is the modern entry point;
    // getSystemService(VIBRATOR_SERVICE) is deprecated there but
    // still the only path on 26..=30. Returns null only if the
    // platform somehow exposes no vibrator service at all — callers
    // treat that as "no haptics", same as a device with no motor.
    private fun resolveVibrator(context: Context): Vibrator? {
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            val mgr = context.getSystemService(Context.VIBRATOR_MANAGER_SERVICE)
                as? VibratorManager
            mgr?.defaultVibrator
        } else {
            @Suppress("DEPRECATION")
            context.getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator
        }
    }
}
