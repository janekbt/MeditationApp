// Audio bridge for Phase 5 bell-cue + preview playback. Called
// from Rust via JNI (see `meditate-android/src/audio.rs`) through
// the same app-classloader -> loadClass -> call_static_method
// path the foreground service and haptics bridge use.
//
// Player slots mirror GTK's sound.rs: the preview (`play`, a new
// tap replaces the last), and the session bells on core's
// FireChannel (`playBell`): Starting and End each replace only
// themselves, Interval stacks so bells that coincide (or a box-
// breath cue over the last one's tail) all ring through. minSdk
// is 26, so MediaPlayer + AudioAttributes are always available.
//
// USAGE_ALARM + CONTENT_TYPE_SONIFICATION: a meditation bell is
// a deliberate, time-critical cue (same rationale as the
// haptics' USAGE_ALARM) — it must ring even when media volume is
// low or the ringer is down, like an alarm clock, rather than
// being routed/ducked as background media.
//
// Bell volumes are absolute (meditate_core::bell_volume): while a
// bell plays, the alarm stream sits at its top step and the bell is
// scaled by its own gain, so the user's alarm volume doesn't change
// how loud it rings. The user's step is saved first and put back
// when the bell ends; if the app dies mid-bell, recoverAlarmVolume()
// puts it back on the next start. No flags: the system volume panel
// never shows and no click plays.

package io.github.janekbt.Meditate

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.media.MediaPlayer
import android.os.Build
import android.util.Log

object MeditateAudio {
    private const val TAG = "MeditateAudio"

    // Guarded by `lock`; touched from the JNI thread (Rust) and
    // the MediaPlayer completion callback (main looper).
    private val lock = Any()
    private var preview: MediaPlayer? = null
    private var starting: MediaPlayer? = null
    private var end: MediaPlayer? = null
    private val intervals = mutableListOf<MediaPlayer>()
    // True while the alarm stream is at its top step for a bell.
    private var raised = false

    // meditate_core::session::FireChannel, as numbered by the Rust
    // side's `bell_channel_slot`.
    const val CHANNEL_STARTING = 0
    const val CHANNEL_END = 1
    const val CHANNEL_INTERVAL = 2

    private const val PREFS = "meditate_audio"
    // The user's alarm step while it's raised; survives a crash.
    private const val KEY_RESTORE = "alarm_restore_index"

    private val attrs = AudioAttributes.Builder()
        .setUsage(AudioAttributes.USAGE_ALARM)
        .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
        .build()

    @JvmStatic
    // Returns the clip duration in ms (0 if unknown / on
    // failure). Rust uses it to schedule the preview pill's
    // auto-revert — the Android equivalent of GTK reverting the
    // Play icon on the MediaFile's notify::ended.
    fun play(context: Context, path: String, gain: Float): Long {
        val app = context.applicationContext
        synchronized(lock) {
            // A new preview replaces the old one; the stream stays raised.
            releasePreviewLocked()
            val mp = startLocked(app, path, gain) ?: return 0L
            preview = mp
            // Valid after prepare(); -1 for unseekable/live
            // streams (not the case for our bundled OGGs).
            return mp.duration.toLong().coerceAtLeast(0L)
        }
    }

    // A session bell on core's channel (CHANNEL_*). Starting and End
    // replace their own previous bell only; Interval bells stack.
    @JvmStatic
    fun playBell(context: Context, channel: Int, path: String, gain: Float) {
        val app = context.applicationContext
        synchronized(lock) {
            when (channel) {
                CHANNEL_STARTING -> { releasePlayer(starting); starting = null }
                CHANNEL_END -> { releasePlayer(end); end = null }
            }
            val mp = startLocked(app, path, gain) ?: return
            when (channel) {
                CHANNEL_STARTING -> starting = mp
                CHANNEL_END -> end = mp
                else -> intervals.add(mp)
            }
        }
    }

    // Prepare and start a player at `gain`; it forgets itself and
    // releases when it ends. Null when the file won't play.
    private fun startLocked(app: Context, path: String, gain: Float): MediaPlayer? {
        val mp = MediaPlayer()
        try {
            mp.setAudioAttributes(attrs)
            // The bell's own volume at the stream's top step
            // (meditate_core::bell_volume::BellVolume::absolute_gain).
            mp.setVolume(gain, gain)
            mp.setDataSource(path)
            mp.setOnCompletionListener {
                synchronized(lock) { finishedLocked(app, mp) }
            }
            mp.setOnErrorListener { _, what, extra ->
                Log.w(TAG, "MediaPlayer error what=$what extra=$extra")
                synchronized(lock) { finishedLocked(app, mp) }
                true
            }
            mp.prepare()
            raiseLocked(app)
            mp.start()
            return mp
        } catch (e: Exception) {
            Log.w(TAG, "play failed path=$path: $e")
            runCatching { mp.release() }
            restoreIfIdleLocked(app)
            return null
        }
    }

    // One player ended on its own: drop it from its slot, and put the
    // alarm volume back once nothing else is ringing.
    private fun finishedLocked(app: Context, mp: MediaPlayer) {
        if (preview === mp) preview = null
        if (starting === mp) starting = null
        if (end === mp) end = null
        intervals.remove(mp)
        releasePlayer(mp)
        restoreIfIdleLocked(app)
    }

    // Live change while a volume slider is dragged: applies to the
    // preview ringing right now, if any.
    @JvmStatic
    fun setVolume(context: Context, gain: Float) {
        synchronized(lock) {
            runCatching { preview?.setVolume(gain, gain) }
        }
    }

    // How loud the quietest and loudest alarm steps play on the
    // speaker, in dB: [quietest, loudest]. Empty before Android 9
    // (no getStreamVolumeDb) or on failure; the caller falls back.
    @JvmStatic
    fun alarmRangeDb(context: Context): FloatArray {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.P) return FloatArray(0)
        return runCatching {
            val am = audioManager(context)
            val min = am.getStreamMinVolume(AudioManager.STREAM_ALARM)
            val max = am.getStreamMaxVolume(AudioManager.STREAM_ALARM)
            val speaker = AudioDeviceInfo.TYPE_BUILTIN_SPEAKER
            floatArrayOf(
                am.getStreamVolumeDb(AudioManager.STREAM_ALARM, min, speaker),
                am.getStreamVolumeDb(AudioManager.STREAM_ALARM, max, speaker),
            )
        }.getOrElse {
            Log.w(TAG, "alarmRangeDb failed: $it")
            FloatArray(0)
        }
    }

    // App start: if a bell was cut off by the app dying, the alarm
    // stream is still at its top step; put the user's step back.
    @JvmStatic
    fun recoverAlarmVolume(context: Context) {
        synchronized(lock) {
            if (isIdleLocked()) restoreSaved(context.applicationContext)
        }
    }

    // Stop everything: the preview and every session bell.
    @JvmStatic
    fun stop(context: Context) {
        synchronized(lock) {
            releaseAllLocked()
            restoreLocked(context.applicationContext)
        }
    }

    private fun releaseAllLocked() {
        releasePreviewLocked()
        releasePlayer(starting)
        starting = null
        releasePlayer(end)
        end = null
        intervals.forEach { releasePlayer(it) }
        intervals.clear()
    }

    private fun releasePreviewLocked() {
        releasePlayer(preview)
        preview = null
    }

    private fun releasePlayer(mp: MediaPlayer?) {
        mp ?: return
        runCatching { if (mp.isPlaying) mp.stop() }
        runCatching { mp.release() }
    }

    private fun isIdleLocked() =
        preview == null && starting == null && end == null && intervals.isEmpty()

    private fun restoreIfIdleLocked(context: Context) {
        if (isIdleLocked()) restoreLocked(context)
    }

    private fun audioManager(context: Context) =
        context.getSystemService(Context.AUDIO_SERVICE) as AudioManager

    private fun prefs(context: Context) =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    // Save the user's alarm step, then raise the stream to its top.
    private fun raiseLocked(context: Context) {
        if (raised) return
        raised = true
        runCatching {
            val am = audioManager(context)
            val current = am.getStreamVolume(AudioManager.STREAM_ALARM)
            prefs(context).edit().putInt(KEY_RESTORE, current).commit()
            val top = am.getStreamMaxVolume(AudioManager.STREAM_ALARM)
            if (current != top) am.setStreamVolume(AudioManager.STREAM_ALARM, top, 0)
        }.onFailure { Log.w(TAG, "raise alarm volume failed: $it") }
    }

    private fun restoreLocked(context: Context) {
        if (!raised) return
        raised = false
        restoreSaved(context)
    }

    // Put the saved step back, unless the user moved the alarm volume
    // while the bell rang: then theirs stays.
    private fun restoreSaved(context: Context) {
        runCatching {
            val prefs = prefs(context)
            val saved = prefs.getInt(KEY_RESTORE, -1)
            if (saved < 0) return
            val am = audioManager(context)
            val top = am.getStreamMaxVolume(AudioManager.STREAM_ALARM)
            if (am.getStreamVolume(AudioManager.STREAM_ALARM) == top) {
                am.setStreamVolume(AudioManager.STREAM_ALARM, saved, 0)
            }
            prefs.edit().remove(KEY_RESTORE).commit()
        }.onFailure { Log.w(TAG, "restore alarm volume failed: $it") }
    }
}
