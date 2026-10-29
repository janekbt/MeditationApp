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
// Bells play as media (GitHub #10), which Android sends where music
// would play: only to connected headphones, else to the speaker.
// "Play Bells as Alarm" (Preferences) uses USAGE_ALARM +
// CONTENT_TYPE_SONIFICATION instead, for devices where media bells
// fail: alarms ring on every device, but Android plays them on the
// speaker and the headphones at once, so from Android 9 an alarm bell
// is aimed at the headphones media would play on. A media bell keeps
// Android's own route; the app only reads it to pick the curve. No
// audio focus is asked for, so music from another app keeps playing.
//
// Bell volumes are absolute (meditate_core::bell_volume): while bells
// play, their stream (alarm or media) sits at the step the loudest of
// them needs and each player is trimmed by its gain for that step, so
// the user's volume doesn't change how loud a bell rings, and the
// slider still works on a device that ignores the gain. The user's
// step is saved first and put back when the stream's last bell ends;
// if the app dies mid-bell, recoverAlarmVolume() puts it back on the
// next start. Android keeps a media step per output, so media is put
// back only on the output it was set on. No flags: the system volume
// panel never shows and no click plays.

package io.github.janekbt.Meditate

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioDeviceInfo
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import android.media.MediaPlayer
import android.os.Build
import android.os.Handler
import android.os.Looper
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

    // A stream the app sets for its bells, and the keys that let a
    // crash put the user's step back. Android keeps a media step per
    // output, so media keeps one pair per output (`route`): a bell on the
    // speaker never overwrites the step kept for headphones that went
    // away mid-bell. The alarm has one output, "".
    private class Hold(
        val stream: Int, val attrs: AudioAttributes,
        private val restoreKey: String, private val setKey: String,
        val route: (Context) -> String,
    ) {
        // True while the app holds the stream for its bells.
        var raised = false
        // The step the app last set, and whether the user moved the
        // volume since: then theirs stays until the bells end.
        var lastSet = -1
        var takenOver = false
        // The output the step was set on.
        var output = ""

        fun keyRestore(out: String) = if (out.isEmpty()) restoreKey else "$restoreKey/$out"
        fun keySet(out: String) = if (out.isEmpty()) setKey else "$setKey/$out"
    }

    // Each live player's stream, the output it started on, its step and
    // its gain at every stream step (meditate_core::bell_volume::AlarmLevel).
    private class Level(val hold: Hold, val out: String, val step: Int, val gains: FloatArray)
    private val levels = HashMap<MediaPlayer, Level>()

    // meditate_core::session::FireChannel, as numbered by the Rust
    // side's `bell_channel_slot`.
    const val CHANNEL_STARTING = 0
    const val CHANNEL_END = 1
    const val CHANNEL_INTERVAL = 2

    private const val PREFS = "meditate_audio"
    private const val SPEAKER = "speaker"

    // Wake-up lead-in (Preferences): before a bell on a Bluetooth output,
    // core's noise and its silence (meditate_core::lead_in, at this rate)
    // play on the bell's own stream and output; the bell starts where
    // they finished playing. Bells due meanwhile on that output wait for
    // the same noise. LATE_MS after the planned end they start anyway.
    private const val LEAD_RATE = 48000
    // meditate_core::lead_in::LATE_MS: a cold A2DP link stalls the track before it plays.
    private const val LATE_MS = 5000L
    private class Lead(val track: AudioTrack, val out: String, val until: Long) {
        val waiting = mutableListOf<MediaPlayer>()
    }
    private val leads = mutableListOf<Lead>()

    private val attrs = AudioAttributes.Builder()
        .setUsage(AudioAttributes.USAGE_ALARM)
        .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
        .build()
    private val mediaAttrs = AudioAttributes.Builder()
        .setUsage(AudioAttributes.USAGE_MEDIA)
        .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
        .build()

    private val wired = intArrayOf(
        AudioDeviceInfo.TYPE_WIRED_HEADPHONES, AudioDeviceInfo.TYPE_WIRED_HEADSET,
    )
    // USB-C adapters without a mic report as USB devices.
    private val usb = intArrayOf(AudioDeviceInfo.TYPE_USB_HEADSET, AudioDeviceInfo.TYPE_USB_DEVICE)
    private val bluetooth = intArrayOf(
        AudioDeviceInfo.TYPE_BLUETOOTH_A2DP, AudioDeviceInfo.TYPE_HEARING_AID,
        AudioDeviceInfo.TYPE_BLE_HEADSET, AudioDeviceInfo.TYPE_BLE_SPEAKER,
    )
    private val main = Handler(Looper.getMainLooper())

    // The alarm keys stay as they were, so a crash left by an older
    // version is still put back.
    private val alarm = Hold(AudioManager.STREAM_ALARM, attrs, "alarm_restore_index", "alarm_set_index") { "" }
    private val media = Hold(AudioManager.STREAM_MUSIC, mediaAttrs, "media_restore_index", "media_set_index", ::route)
    private val holds = listOf(alarm, media)

    @JvmStatic
    // Returns the clip duration plus its lead-in wait in ms (0 if
    // unknown / on failure). Rust uses it to bound the preview pill's
    // auto-revert — the Android equivalent of GTK reverting the
    // Play icon on the MediaFile's notify::ended.
    fun play(
        context: Context, path: String,
        asMedia: Boolean, step: Int, gains: FloatArray, headStep: Int, headGains: FloatArray, lead: ShortArray,
    ): Long {
        val app = context.applicationContext
        synchronized(lock) {
            // A new preview replaces the old one; the stream stays raised.
            releasePreviewLocked()
            val (mp, wait) = startLocked(app, path, asMedia, step, gains, headStep, headGains, lead) ?: return 0L
            preview = mp
            // Valid after prepare(); -1 for unseekable/live
            // streams (not the case for our bundled OGGs).
            return mp.duration.toLong().coerceAtLeast(0L) + wait
        }
    }

    // A session bell on core's channel (CHANNEL_*). Starting and End
    // replace their own previous bell only; Interval bells stack.
    // Returns the ms it waits for its lead-in, for its vibration.
    @JvmStatic
    fun playBell(
        context: Context, channel: Int, path: String,
        asMedia: Boolean, step: Int, gains: FloatArray, headStep: Int, headGains: FloatArray, lead: ShortArray,
    ): Long {
        val app = context.applicationContext
        synchronized(lock) {
            when (channel) {
                CHANNEL_STARTING -> { releasePlayer(starting); starting = null }
                CHANNEL_END -> { releasePlayer(end); end = null }
            }
            val (mp, wait) = startLocked(app, path, asMedia, step, gains, headStep, headGains, lead) ?: return 0L
            when (channel) {
                CHANNEL_STARTING -> starting = mp
                CHANNEL_END -> end = mp
                else -> intervals.add(mp)
            }
            return wait
        }
    }

    // The headphones media plays on now, if any: the device Android
    // routes media to (Android 13+, the last one connected), else wired,
    // then USB, then Bluetooth. The FP5 answers the route with nothing
    // while a Bluetooth box is connected; the output list names it.
    private fun headphones(context: Context): AudioDeviceInfo? = runCatching {
        val am = audioManager(context)
        val types = wired + usb + bluetooth
        val routed = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU)
            am.getAudioDevicesForAttributes(mediaAttrs).takeIf { it.isNotEmpty() } else null
        if (routed != null) {
            routed.firstOrNull { it.type in types }
        } else {
            val outs = am.getDevices(AudioManager.GET_DEVICES_OUTPUTS)
            listOf(wired, usb, bluetooth).firstNotNullOfOrNull { group -> outs.firstOrNull { it.type in group } }
        }
    }.onFailure { Log.w(TAG, "headphones lookup failed: $it") }.getOrNull()

    private fun key(device: AudioDeviceInfo): String =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) "${device.type}/${device.address}" else "${device.type}"

    // The output media plays on now: headphones, or the speaker.
    private fun route(context: Context): String = outOf(headphones(context))

    private fun outOf(phones: AudioDeviceInfo?): String = phones?.let { key(it) } ?: SPEAKER

    // Headphones pulled or plugged in under a bell: it stops at once,
    // whatever its stream, as Android would move it to the other output
    // at a step meant for this one, or ring it where it shouldn't. A
    // failed register leaves the bells working; only devices coming and
    // going go unnoticed until the next try.
    private var watching = false

    private fun watchLocked(app: Context) {
        if (watching) return
        runCatching { audioManager(app).registerAudioDeviceCallback(object : android.media.AudioDeviceCallback() {
            override fun onAudioDevicesAdded(added: Array<out AudioDeviceInfo>) = outputChanged(app)
            override fun onAudioDevicesRemoved(removed: Array<out AudioDeviceInfo>) = outputChanged(app)
        }, main) }.onSuccess { watching = true }.onFailure { Log.w(TAG, "device callback failed: $it") }
    }

    private fun outputChanged(app: Context) {
        synchronized(lock) {
            // An output coming back gets the step kept for it.
            if (!media.raised) restoreSaved(app, media)
            if (levels.isEmpty()) return
            val now = route(app)
            levels.filterValues { it.out != now }.keys.toList().forEach { mp -> finishedLocked(app, mp) }
        }
    }

    // Prepare and start a player at its level: as an alarm, or as media
    // on the curve of where media plays now. It forgets itself and
    // releases when it ends. With its lead-in wait in ms; null when the
    // file won't play.
    private fun startLocked(
        app: Context, path: String,
        asMedia: Boolean, step: Int, gains: FloatArray, headStep: Int, headGains: FloatArray, lead: ShortArray,
    ): Pair<MediaPlayer, Long>? {
        val hold = if (asMedia) media else alarm
        val phones = headphones(app)
        val out = outOf(phones)
        val level = levelFor(hold, out, step, gains, headStep, headGains)
        val mp = MediaPlayer()
        try {
            mp.setAudioAttributes(hold.attrs)
            mp.setDataSource(path)
            // Android rings an alarm on the speaker too; aimed at the
            // headphones it rings only there. Only a player with its file
            // takes the aim.
            if (hold === alarm && phones != null && Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                if (!mp.setPreferredDevice(phones)) Log.w(TAG, "alarm not aimed at ${phones.type}")
            }
            mp.setOnCompletionListener {
                synchronized(lock) { finishedLocked(app, mp) }
            }
            mp.setOnErrorListener { _, what, extra ->
                Log.w(TAG, "MediaPlayer error what=$what extra=$extra")
                synchronized(lock) { finishedLocked(app, mp) }
                true
            }
            mp.prepare()
            if (!hold.raised) hold.output = if (hold === media) out else ""
            watchLocked(app)
            levels[mp] = level
            applyLocked(app, hold)
            val wait = if (phones != null && phones.type in bluetooth && lead.isNotEmpty()) leadLocked(hold, phones, out, lead, mp) else null
            if (wait == null) mp.start()
            return mp to (wait ?: 0L)
        } catch (e: Exception) {
            Log.w(TAG, "play failed path=$path: $e")
            releasePlayer(mp)
            settleLocked(app)
            return null
        }
    }

    // Hold `mp` back for a lead-in on `out`: join the one running there,
    // or play core's samples on a track of its own. The ms it waits; null
    // when the track won't play (the bell starts at once, without one).
    private fun leadLocked(hold: Hold, phones: AudioDeviceInfo, out: String, lead: ShortArray, mp: MediaPlayer): Long? {
        val now = android.os.SystemClock.elapsedRealtime()
        // Past its planned end it is late, not over: the bell still waits.
        leads.firstOrNull { it.out == out }?.let { it.waiting.add(mp); return (it.until - now).coerceAtLeast(0L) }
        var made: AudioTrack? = null
        return runCatching {
            val track = AudioTrack.Builder()
                .setAudioAttributes(hold.attrs)
                .setAudioFormat(
                    AudioFormat.Builder()
                        .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                        .setSampleRate(LEAD_RATE)
                        .setChannelMask(AudioFormat.CHANNEL_OUT_STEREO)
                        .build(),
                )
                .setTransferMode(AudioTrack.MODE_STATIC)
                .setBufferSizeInBytes(lead.size * 2)
                .build()
            made = track
            check(track.write(lead, 0, lead.size) == lead.size) { "short write" }
            if (!track.setPreferredDevice(phones)) Log.w(TAG, "lead-in not aimed at ${phones.type}")
            val wait = lead.size / 2 * 1000L / LEAD_RATE
            val l = Lead(track, out, now + wait)
            l.waiting.add(mp)
            leads.add(l)
            track.setNotificationMarkerPosition(lead.size / 2)
            track.setPlaybackPositionUpdateListener(object : AudioTrack.OnPlaybackPositionUpdateListener {
                override fun onMarkerReached(t: AudioTrack) = synchronized(lock) { wakeLocked(l) }
                override fun onPeriodicNotification(t: AudioTrack) {}
            }, main)
            main.postDelayed({ synchronized(lock) { wakeLocked(l) } }, wait + LATE_MS)
            track.play()
            wait
        }.getOrElse {
            Log.w(TAG, "lead-in failed: $it")
            leads.removeAll { it.track === made }
            releaseTrack(made)
            null
        }
    }

    // A lead-in played out (or ran late): its bells start, its noise goes.
    private fun wakeLocked(l: Lead) {
        if (!leads.remove(l)) return
        releaseTrack(l.track)
        l.waiting.forEach { if (levels.containsKey(it)) runCatching { it.start() } }
    }

    private fun releaseTrack(t: AudioTrack?) {
        t ?: return
        runCatching { t.stop() }
        runCatching { t.release() }
    }

    // A bell's level: in headphones on the headphone curve, else on its
    // stream's own (the alarm, or media on the speaker).
    private fun levelFor(hold: Hold, out: String, step: Int, gains: FloatArray, headStep: Int, headGains: FloatArray) =
        if (hold === media && out != SPEAKER) Level(hold, out, headStep, headGains) else Level(hold, out, step, gains)

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
    // preview ringing right now, if any, on the stream it plays on.
    @JvmStatic
    fun setVolume(context: Context, asMedia: Boolean, step: Int, gains: FloatArray, headStep: Int, headGains: FloatArray) {
        synchronized(lock) {
            val mp = preview ?: return
            val old = levels[mp] ?: return
            // The switch changed (a sync) since the preview started: its
            // steps are for the other stream.
            if ((old.hold === media) != asMedia) return
            levels[mp] = levelFor(old.hold, old.out, step, gains, headStep, headGains)
            applyLocked(context.applicationContext, old.hold)
        }
    }

    // A stream's steps and how loud each plays on `device`:
    // [min, max, dB of min, ..., dB of max]. No dB before Android 9
    // (no getStreamVolumeDb) or when they can't be read; empty on
    // failure. The caller falls back to an even curve.
    private fun steps(context: Context, stream: Int, device: Int, what: String): FloatArray {
        return runCatching {
            val am = audioManager(context)
            val max = am.getStreamMaxVolume(stream)
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.P) {
                return floatArrayOf(1f, max.toFloat())
            }
            val min = am.getStreamMinVolume(stream)
            val db = runCatching {
                (min..max).map { am.getStreamVolumeDb(stream, it, device) }
            }.getOrElse { emptyList() }
            floatArrayOf(min.toFloat(), max.toFloat()) + db.toFloatArray()
        }.getOrElse {
            Log.w(TAG, "$what failed: $it")
            FloatArray(0)
        }
    }

    // The alarm stream on the speaker.
    @JvmStatic
    fun alarmSteps(context: Context): FloatArray =
        steps(context, AudioManager.STREAM_ALARM, AudioDeviceInfo.TYPE_BUILTIN_SPEAKER, "alarmSteps")

    // The media stream on the speaker.
    @JvmStatic
    fun mediaSteps(context: Context): FloatArray =
        steps(context, AudioManager.STREAM_MUSIC, AudioDeviceInfo.TYPE_BUILTIN_SPEAKER, "mediaSteps")

    // The media stream on headphones, readable without any connected.
    // A USB DAC or a Bluetooth device may have its own curve elsewhere;
    // the cut is by step, so a bell never goes above the safety step.
    @JvmStatic
    fun headphoneSteps(context: Context): FloatArray =
        steps(context, AudioManager.STREAM_MUSIC, AudioDeviceInfo.TYPE_WIRED_HEADPHONES, "headphoneSteps")

    // App start: if a bell was cut off by the app dying, its stream is
    // still at the step the app set; put the user's step back.
    @JvmStatic
    fun recoverAlarmVolume(context: Context) {
        synchronized(lock) {
            if (isIdleLocked()) holds.forEach { restoreSaved(context.applicationContext, it) }
            // Puts back a step kept for an output that isn't current yet.
            watchLocked(context.applicationContext)
        }
    }

    // Whether the preview still plays: it ends on its own, or stops when
    // headphones are plugged in or pulled.
    @JvmStatic
    fun previewPlaying(): Boolean = synchronized(lock) { preview != null }

    // Stop everything: the preview and every session bell.
    @JvmStatic
    fun stop(context: Context) {
        synchronized(lock) {
            releaseAllLocked()
            settleLocked(context.applicationContext)
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
        leads.filter { it.waiting.remove(mp) && it.waiting.isEmpty() }.forEach { leads.remove(it); releaseTrack(it.track) }
        runCatching { if (mp.isPlaying) mp.stop() }
        runCatching { mp.release() }
    }

    private fun isIdleLocked() =
        preview == null && starting == null && end == null && intervals.isEmpty()

    // After a player went away: put each stream's user step back once
    // none of its bells rings, else let it follow the ones still ringing.
    private fun settleLocked(context: Context) {
        for (hold in holds) {
            if (levels.values.none { it.hold === hold }) restoreLocked(context, hold) else applyLocked(context, hold)
        }
    }

    private fun audioManager(context: Context) =
        context.getSystemService(Context.AUDIO_SERVICE) as AudioManager

    private fun prefs(context: Context) =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    // Put the stream at the step the loudest of its live players needs
    // and trim every one for the step the stream really is at, so a
    // step the system refuses leaves bells quieter, never louder. The
    // user's step is saved on the first raise, unless a crash left one
    // that wasn't put back yet; the step about to be set is saved
    // before setting it, so a crash in between is still put back.
    private fun applyLocked(context: Context, hold: Hold) {
        val live = levels.values.filter { it.hold === hold }
        if (live.isEmpty()) return
        runCatching {
            val am = audioManager(context)
            val current = am.getStreamVolume(hold.stream)
            val keyRestore = hold.keyRestore(hold.output)
            val keySet = hold.keySet(hold.output)
            if (!hold.raised) {
                hold.raised = true
                hold.takenOver = false
                hold.lastSet = current
                val prefs = prefs(context)
                val edit = prefs.edit()
                // Left by a crash or by an output that went away: theirs,
                // while the stream is still at the app's step; else the
                // current step is the user's.
                if (prefs.getInt(keySet, -1) != current) edit.putInt(keyRestore, current)
                edit.putInt(keySet, current)
                edit.commit()
            }
            if (current != hold.lastSet) hold.takenOver = true
            // Media moved to another output: a step set now would land there.
            if (hold.route(context) != hold.output) hold.takenOver = true
            val want = live.maxOf { it.step }
            if (!hold.takenOver && want != current) {
                // Going up, trim first so a quiet bell never peaks.
                if (want > current) trimLocked(hold, want)
                prefs(context).edit().putInt(keySet, want).commit()
                am.setStreamVolume(hold.stream, want, 0)
                hold.lastSet = am.getStreamVolume(hold.stream)
                if (hold.lastSet != want) prefs(context).edit().putInt(keySet, hold.lastSet).commit()
            }
        }.onFailure { Log.w(TAG, "set volume failed: $it") }
        trimLocked(
            hold,
            runCatching { audioManager(context).getStreamVolume(hold.stream) }.getOrDefault(hold.lastSet)
        )
    }

    // Every player of the stream's gain with it at `step`; an unknown
    // step trims each to its own step's gain.
    private fun trimLocked(hold: Hold, step: Int) {
        levels.forEach { (mp, level) ->
            if (level.hold !== hold) return@forEach
            val gain = level.gains.getOrElse(step) { level.gains.getOrElse(level.step) { 1f } }
            runCatching { mp.setVolume(gain, gain) }
        }
    }

    private fun restoreLocked(context: Context, hold: Hold) {
        if (!hold.raised) return
        hold.raised = false
        restoreSaved(context, hold)
    }

    // Put the saved step back, unless the stream isn't at the app's step
    // (the user moved it while the bell rang, or media is still on another
    // output for a moment): then theirs stays, and the keys too, until the
    // next raise overwrites them. Media on the output it plays on now,
    // from that output's keys; another output's kept step waits until it
    // is current (outputChanged).
    private fun restoreSaved(context: Context, hold: Hold) {
        runCatching {
            val prefs = prefs(context)
            val output = hold.route(context)
            val keyRestore = hold.keyRestore(output)
            val keySet = hold.keySet(output)
            val saved = prefs.getInt(keyRestore, -1)
            if (saved < 0) return
            val am = audioManager(context)
            // Left by a version that raised to the top step: no keySet.
            val set = prefs.getInt(keySet, am.getStreamMaxVolume(hold.stream))
            if (am.getStreamVolume(hold.stream) != set) return
            am.setStreamVolume(hold.stream, saved, 0)
            prefs.edit().remove(keyRestore).remove(keySet).commit()
        }.onFailure { Log.w(TAG, "restore volume failed: $it") }
    }
}
