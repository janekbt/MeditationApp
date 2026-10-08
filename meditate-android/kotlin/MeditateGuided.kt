// Guided-meditation track playback (Phase 6.5 GM-3). Separate
// MediaPlayer slot from MeditateAudio's bell player: the guided
// track is long-form spoken content (USAGE_MEDIA /
// CONTENT_TYPE_SPEECH — normal media routing/ducking), whereas
// the end-bell stays USAGE_ALARM. Driven from Rust (src/guided.rs)
// via the app-classloader JNI path, in step with the session:
// start on session start, pause/resume with it, stop on
// stop/finish. On natural end (or error) it writes the
// `<filesDir>/meditate/guided_eos` drop-file; the Rust tick loop
// polls it and forces the session into Overtime (the end bell
// then fires), robust to a probe-vs-actual length mismatch —
// mirrors GTK's gstreamer EOS → Session::enter_overtime.

package io.github.janekbt.Meditate

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.media.MediaPlayer
import android.util.Log
import java.io.File

object MeditateGuided {
    private const val TAG = "MeditateGuided"

    private val lock = Any()
    private var player: MediaPlayer? = null
    private var focusRequest: AudioFocusRequest? = null

    private val attrs = AudioAttributes.Builder()
        .setUsage(AudioAttributes.USAGE_MEDIA)
        .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
        .build()

    @JvmStatic
    fun startAudio(context: Context, path: String): Boolean {
        synchronized(lock) {
            releaseLocked(context)
            clearEos(context)
            requestFocus(context)
            val mp = MediaPlayer()
            try {
                mp.setAudioAttributes(attrs)
                mp.setDataSource(path)
                mp.setOnCompletionListener {
                    markEos(context)
                    synchronized(lock) { releaseLocked(context) }
                }
                mp.setOnErrorListener { _, what, extra ->
                    Log.w(TAG, "MediaPlayer error what=$what extra=$extra")
                    markEos(context)
                    synchronized(lock) { releaseLocked(context) }
                    true
                }
                mp.prepare()
                mp.start()
                player = mp
                return true
            } catch (e: Exception) {
                Log.w(TAG, "startAudio failed path=$path: $e")
                runCatching { mp.release() }
                releaseLocked(context)
                // Report it: the caller then starts no session, like
                // GTK's "Couldn't start playback".
                return false
            }
        }
    }

    @JvmStatic
    fun pauseAudio(context: Context) {
        synchronized(lock) {
            runCatching { player?.let { if (it.isPlaying) it.pause() } }
        }
    }

    @JvmStatic
    fun resumeAudio(context: Context) {
        synchronized(lock) {
            val mp = player ?: return
            // Denied (a call, another media app): the session pauses again.
            if (!requestFocus(context)) return markFocusLoss(context)
            runCatching { mp.setVolume(1f, 1f); if (!mp.isPlaying) mp.start() }
        }
    }

    @JvmStatic
    fun stopAudio(context: Context) {
        synchronized(lock) {
            releaseLocked(context)
        }
    }

    // Long-form USAGE_MEDIA content must participate in audio
    // focus: an incoming call or another media app should pause
    // the guide. On loss we drop `guided_focus_loss`; the Rust
    // tick loop routes it through the normal session-pause path
    // (which also pauses this player), so timer and audio stay in
    // step. No auto-resume — after a call the user resumes
    // deliberately, matching a meditation's flow.
    // A short sound (notification, navigation) only ducks the guide.
    private fun requestFocus(context: Context): Boolean {
        val am = context.getSystemService(Context.AUDIO_SERVICE)
            as? AudioManager ?: return true
        val req = focusRequest ?: AudioFocusRequest
            .Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(attrs)
            .setOnAudioFocusChangeListener { change ->
                when (change) {
                    AudioManager.AUDIOFOCUS_LOSS,
                    AudioManager.AUDIOFOCUS_LOSS_TRANSIENT,
                    -> markFocusLoss(context)
                    AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK -> setVolume(0.2f)
                    AudioManager.AUDIOFOCUS_GAIN -> setVolume(1f)
                }
            }
            .build()
        focusRequest = req
        return am.requestAudioFocus(req) == AudioManager.AUDIOFOCUS_REQUEST_GRANTED
    }

    private fun setVolume(v: Float) {
        synchronized(lock) { runCatching { player?.setVolume(v, v) } }
    }

    private fun abandonFocus(context: Context) {
        val req = focusRequest ?: return
        focusRequest = null
        val am = context.getSystemService(Context.AUDIO_SERVICE)
            as? AudioManager ?: return
        runCatching { am.abandonAudioFocusRequest(req) }
    }

    private fun markFocusLoss(context: Context) {
        runCatching {
            val f = File(
                File(context.filesDir, "meditate"),
                "guided_focus_loss",
            )
            f.parentFile?.mkdirs()
            f.writeText("1")
        }
    }

    private fun releaseLocked(context: Context) {
        abandonFocus(context)
        player?.let { mp ->
            runCatching { if (mp.isPlaying) mp.stop() }
            runCatching { mp.release() }
        }
        player = null
    }

    private fun eosFile(context: Context) =
        File(File(context.filesDir, "meditate"), "guided_eos")

    private fun markEos(context: Context) {
        runCatching {
            val f = eosFile(context)
            f.parentFile?.mkdirs()
            f.writeText("1")
        }
    }

    private fun clearEos(context: Context) {
        runCatching { eosFile(context).delete() }
    }
}
