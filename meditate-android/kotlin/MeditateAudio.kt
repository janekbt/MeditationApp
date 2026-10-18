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
// Bell volumes are absolute (meditate_core::bell_volume): while bells
// play, the alarm stream sits at the step the loudest of them needs
// and each player is trimmed by its gain for that step, so the user's
// alarm volume doesn't change how loud a bell rings, and the slider
// still works on a device that ignores the gain. The user's step is
// saved first and put back when the last bell ends; if the app dies
// mid-bell, recoverAlarmVolume() puts it back on the next start. No
// flags: the system volume panel never shows and no click plays.

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
    // Each live player's alarm step and its gain at every stream step
    // (meditate_core::bell_volume::AlarmLevel).
    private class Level(val step: Int, val gains: FloatArray)
    private val levels = HashMap<MediaPlayer, Level>()
    // True while the app holds the alarm stream for its bells.
    private var raised = false
    // The step the app last set, and whether the user moved the alarm
    // volume since: then theirs stays until the bells end.
    private var lastSet = -1
    private var takenOver = false

    // meditate_core::session::FireChannel, as numbered by the Rust
    // side's `bell_channel_slot`.
    const val CHANNEL_STARTING = 0
    const val CHANNEL_END = 1
    const val CHANNEL_INTERVAL = 2

    private const val PREFS = "meditate_audio"
    // The user's alarm step while it's raised; survives a crash.
    private const val KEY_RESTORE = "alarm_restore_index"
    // The step the app set; restoring only when it is still there.
    private const val KEY_SET = "alarm_set_index"

    private val attrs = AudioAttributes.Builder()
        .setUsage(AudioAttributes.USAGE_ALARM)
        .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
        .build()

    @JvmStatic
    // Returns the clip duration in ms (0 if unknown / on
    // failure). Rust uses it to schedule the preview pill's
    // auto-revert — the Android equivalent of GTK reverting the
    // Play icon on the MediaFile's notify::ended.
    fun play(context: Context, path: String, step: Int, gains: FloatArray): Long {
        val app = context.applicationContext
        synchronized(lock) {
            // A new preview replaces the old one; the stream stays raised.
            releasePreviewLocked()
            val mp = startLocked(app, path, Level(step, gains)) ?: return 0L
            preview = mp
            // Valid after prepare(); -1 for unseekable/live
            // streams (not the case for our bundled OGGs).
            return mp.duration.toLong().coerceAtLeast(0L)
        }
    }

    // A session bell on core's channel (CHANNEL_*). Starting and End
    // replace their own previous bell only; Interval bells stack.
    @JvmStatic
    fun playBell(context: Context, channel: Int, path: String, step: Int, gains: FloatArray) {
        val app = context.applicationContext
        synchronized(lock) {
            when (channel) {
                CHANNEL_STARTING -> { releasePlayer(starting); starting = null }
                CHANNEL_END -> { releasePlayer(end); end = null }
            }
            val mp = startLocked(app, path, Level(step, gains)) ?: return
            when (channel) {
                CHANNEL_STARTING -> starting = mp
                CHANNEL_END -> end = mp
                else -> intervals.add(mp)
            }
        }
    }

    // Prepare and start a player at its level; it forgets itself and
    // releases when it ends. Null when the file won't play.
    private fun startLocked(app: Context, path: String, level: Level): MediaPlayer? {
        val mp = MediaPlayer()
        try {
            mp.setAudioAttributes(attrs)
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
            levels[mp] = level
            applyLocked(app)
            mp.start()
            return mp
        } catch (e: Exception) {
            Log.w(TAG, "play failed path=$path: $e")
            releasePlayer(mp)
            settleLocked(app)
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
        settleLocked(app)
    }

    // Live change while a volume slider is dragged: applies to the
    // preview ringing right now, if any.
    @JvmStatic
    fun setVolume(context: Context, step: Int, gains: FloatArray) {
        synchronized(lock) {
            val mp = preview ?: return
            levels[mp] = Level(step, gains)
            applyLocked(context.applicationContext)
        }
    }

    // The alarm stream's steps and how loud each plays on the speaker:
    // [min, max, dB of min, ..., dB of max]. No dB before Android 9
    // (no getStreamVolumeDb) or when they can't be read; empty on
    // failure. The caller falls back to an even curve.
    @JvmStatic
    fun alarmSteps(context: Context): FloatArray {
        return runCatching {
            val am = audioManager(context)
            val max = am.getStreamMaxVolume(AudioManager.STREAM_ALARM)
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.P) {
                return floatArrayOf(1f, max.toFloat())
            }
            val min = am.getStreamMinVolume(AudioManager.STREAM_ALARM)
            val speaker = AudioDeviceInfo.TYPE_BUILTIN_SPEAKER
            val db = runCatching {
                (min..max).map { am.getStreamVolumeDb(AudioManager.STREAM_ALARM, it, speaker) }
            }.getOrElse { emptyList() }
            floatArrayOf(min.toFloat(), max.toFloat()) + db.toFloatArray()
        }.getOrElse {
            Log.w(TAG, "alarmSteps failed: $it")
            FloatArray(0)
        }
    }

    // App start: if a bell was cut off by the app dying, the alarm
    // stream is still at the step the app set; put the user's step back.
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
        levels.remove(mp)
        runCatching { if (mp.isPlaying) mp.stop() }
        runCatching { mp.release() }
    }

    private fun isIdleLocked() =
        preview == null && starting == null && end == null && intervals.isEmpty()

    // After a player went away: put the user's step back once nothing
    // rings, else let the stream follow the bells still ringing.
    private fun settleLocked(context: Context) {
        if (isIdleLocked()) restoreLocked(context) else applyLocked(context)
    }

    private fun audioManager(context: Context) =
        context.getSystemService(Context.AUDIO_SERVICE) as AudioManager

    private fun prefs(context: Context) =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    // Put the alarm stream at the step the loudest live player needs
    // and trim every player for the step the stream really is at, so
    // a step the system refuses leaves bells quieter, never louder.
    // The user's step is saved on the first raise; the step about to
    // be set is saved before setting it, so a crash in between is
    // still put back.
    private fun applyLocked(context: Context) {
        if (levels.isEmpty()) return
        runCatching {
            val am = audioManager(context)
            val current = am.getStreamVolume(AudioManager.STREAM_ALARM)
            if (!raised) {
                raised = true
                takenOver = false
                lastSet = current
                prefs(context).edit().putInt(KEY_RESTORE, current).putInt(KEY_SET, current).commit()
            }
            if (current != lastSet) takenOver = true
            val want = levels.values.maxOf { it.step }
            if (!takenOver && want != current) {
                // Going up, trim first so a quiet bell never peaks.
                if (want > current) trimLocked(want)
                prefs(context).edit().putInt(KEY_SET, want).commit()
                am.setStreamVolume(AudioManager.STREAM_ALARM, want, 0)
                lastSet = am.getStreamVolume(AudioManager.STREAM_ALARM)
                if (lastSet != want) prefs(context).edit().putInt(KEY_SET, lastSet).commit()
            }
        }.onFailure { Log.w(TAG, "set alarm volume failed: $it") }
        trimLocked(
            runCatching { audioManager(context).getStreamVolume(AudioManager.STREAM_ALARM) }
                .getOrDefault(lastSet)
        )
    }

    // Every player's gain with the stream at `step`; an unknown step
    // trims each to its own step's gain.
    private fun trimLocked(step: Int) {
        levels.forEach { (mp, level) ->
            val gain = level.gains.getOrElse(step) { level.gains.getOrElse(level.step) { 1f } }
            runCatching { mp.setVolume(gain, gain) }
        }
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
            // Left by a version that raised to the top step: no KEY_SET.
            val set = prefs.getInt(KEY_SET, am.getStreamMaxVolume(AudioManager.STREAM_ALARM))
            if (am.getStreamVolume(AudioManager.STREAM_ALARM) == set) {
                am.setStreamVolume(AudioManager.STREAM_ALARM, saved, 0)
            }
            prefs.edit().remove(KEY_RESTORE).remove(KEY_SET).commit()
        }.onFailure { Log.w(TAG, "restore alarm volume failed: $it") }
    }
}
