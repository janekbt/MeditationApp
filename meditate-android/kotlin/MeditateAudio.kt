// Audio bridge for Phase 5 bell-cue + preview playback. Called
// from Rust via JNI (see `meditate-android/src/audio.rs`) through
// the same app-classloader -> loadClass -> call_static_method
// path the foreground service and haptics bridge use.
//
// One MediaPlayer slot: a new `play` stops + releases the
// previous one, so preview taps and the session-end cue never
// stack (mirrors the GTK preview slot's mono behaviour). minSdk
// is 26, so MediaPlayer + AudioAttributes are always available.
//
// USAGE_ALARM + CONTENT_TYPE_SONIFICATION: a meditation bell is
// a deliberate, time-critical cue (same rationale as the
// haptics' USAGE_ALARM) — it must ring even when media volume is
// low or the ringer is down, like an alarm clock, rather than
// being routed/ducked as background media.

package io.github.janekbt.Meditate

import android.content.Context
import android.media.AudioAttributes
import android.media.MediaPlayer
import android.util.Log

object MeditateAudio {
    private const val TAG = "MeditateAudio"

    // Guarded by `lock`; touched from the JNI thread (Rust) and
    // the MediaPlayer completion callback (main looper).
    private val lock = Any()
    private var player: MediaPlayer? = null

    private val attrs = AudioAttributes.Builder()
        .setUsage(AudioAttributes.USAGE_ALARM)
        .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
        .build()

    @JvmStatic
    // Returns the clip duration in ms (0 if unknown / on
    // failure). Rust uses it to schedule the preview pill's
    // auto-revert — the Android equivalent of GTK reverting the
    // Play icon on the MediaFile's notify::ended.
    fun play(context: Context, path: String): Long {
        synchronized(lock) {
            releaseLocked()
            val mp = MediaPlayer()
            try {
                mp.setAudioAttributes(attrs)
                mp.setDataSource(path)
                mp.setOnCompletionListener {
                    synchronized(lock) { releaseLocked() }
                }
                mp.setOnErrorListener { _, what, extra ->
                    Log.w(TAG, "MediaPlayer error what=$what extra=$extra")
                    synchronized(lock) { releaseLocked() }
                    true
                }
                mp.prepare()
                mp.start()
                player = mp
                // Valid after prepare(); -1 for unseekable/live
                // streams (not the case for our bundled OGGs).
                return mp.duration.toLong().coerceAtLeast(0L)
            } catch (e: Exception) {
                Log.w(TAG, "play failed path=$path: $e")
                runCatching { mp.release() }
                return 0L
            }
        }
    }

    @JvmStatic
    fun stop(context: Context) {
        synchronized(lock) { releaseLocked() }
    }

    private fun releaseLocked() {
        player?.let { mp ->
            runCatching { if (mp.isPlaying) mp.stop() }
            runCatching { mp.release() }
        }
        player = null
    }
}
