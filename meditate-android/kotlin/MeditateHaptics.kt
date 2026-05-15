// Haptics bridge for Phase 5 bell-cue vibration. Called from Rust
// via JNI (see `meditate-android/src/haptics.rs`) through the same
// app-classloader → loadClass → call_static_method path the
// foreground service uses.
//
// B-1 ships `vibrateOneShot` only — the minimal end-to-end proof
// that the JNI bridge + Vibrator path work. B-2 (vibration-pattern
// chooser) extends this with a waveform variant that takes the
// `(amplitude, duration_ms)` envelope `meditate_core::vibration`
// produces.
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

    @JvmStatic
    fun vibrateOneShot(context: Context, durationMs: Long) {
        if (durationMs <= 0L) return
        val vibrator = resolveVibrator(context)
        if (vibrator == null) {
            Log.w(TAG, "no Vibrator service resolved")
            return
        }
        if (!vibrator.hasVibrator()) {
            Log.w(TAG, "device reports hasVibrator()=false")
            return
        }
        val effect = VibrationEffect.createOneShot(
            durationMs,
            VibrationEffect.DEFAULT_AMPLITUDE,
        )
        // USAGE_ALARM: a bell cue is an intentional, time-critical
        // signal — not incidental touch feedback. Android 13+
        // classifies un-attributed vibrations and the device's
        // haptic / DND settings can silently drop them; the ALARM
        // usage is exempt from that suppression (same rationale as
        // an alarm-clock buzz). The attributed overload is API 33+;
        // minSdk-26..32 falls back to the bare call.
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
