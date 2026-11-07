// Foreground service that keeps a meditation session alive across
// screen-off and app-switch. Without it, locking the screen during a
// session lets the OS background-throttle the app: Slint stops
// ticking, end-bells (Phase 5) wouldn't fire on time, and the OS
// would eventually kill the process.
//
// Type is `mediaPlayback`: Phase 1 has no audio yet, but Phase 5
// brings real bell-cue playback and migrating service types
// requires a permission promotion + reinstall. Eating the future-
// permission cost up front means the Phase-5 audio drop-in is
// purely an internal change, no manifest tweak.
//
// Lifecycle: the Slint side (Rust) drives this — `start(context)`
// is called when AppState transitions Idle → Active, `stop(context)`
// when Active → Idle / Finished. The service itself doesn't drive
// the timer; the Rust tick loop continues to do that whenever the
// app's process is alive, and the foreground service is what keeps
// the process alive.

package io.github.janekbt.Meditate

import android.Manifest
import android.app.Activity
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.drawable.Icon
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.media.MediaMetadata
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import android.view.KeyEvent
import java.io.File

class MeditateSessionService : Service() {
    companion object {
        // Distinct constants for the two intent actions so a future
        // mid-session-update intent can be added without colliding.
        const val ACTION_START_SESSION = "io.github.janekbt.Meditate.START_SESSION"
        const val ACTION_STOP_SESSION = "io.github.janekbt.Meditate.STOP_SESSION"
        const val ACTION_PAUSE_SESSION = "io.github.janekbt.Meditate.PAUSE_SESSION"
        const val ACTION_RESUME_SESSION = "io.github.janekbt.Meditate.RESUME_SESSION"
        const val ACTION_SHOW_BUTTONS = "io.github.janekbt.Meditate.SHOW_BUTTONS"
        const val ACTION_TAP = "io.github.janekbt.Meditate.TAP"
        const val ACTION_VOID_CONTROLS = "io.github.janekbt.Meditate.VOID_CONTROLS"
        const val EXTRA_PAYLOAD = "payload"
        const val EXTRA_CODE = "code"

        // Button codes, as in src/app.rs (end_button_code, MEDIA_*).
        const val MEDIA_PAUSE = 0
        const val MEDIA_RESUME = 1
        const val MEDIA_SAVE = 2
        const val MEDIA_ADD = 3
        const val MEDIA_DISCARD = 4
        const val MEDIA_HEADPHONES = 5
        // Void-Controls gestures, as in src/app.rs (VOID_*).
        const val VOID_FACE_DOWN = 6
        const val VOID_FACE_UP = 7
        const val VOID_SHAKE = 8

        // Void-Controls detection, tuned on the FP5: a side counts once
        // gravity stays this far along the screen's axis this long; a
        // shake is this many readings this hard within the window, and
        // turns are ignored for a while after it.
        const val FACE_MS2 = 8f
        const val FACE_HOLD_MS = 500L
        const val SHAKE_MS2 = 25f
        const val SHAKE_RELEASE_MS2 = 15f
        const val SHAKE_PEAKS = 3
        const val SHAKE_WINDOW_MS = 1000L
        const val SHAKE_QUIET_MS = 2000L

        // A tap wakes the phone this long, so a paused session (no
        // session wake lock) still reaches the next Rust tick.
        const val TAP_WAKE_MS = 10_000L

        // Persistent notification slot. Stable across start/stop so
        // a stop-then-start within the same process re-uses the
        // same notification ID without leaving an orphan in the
        // shade.
        const val NOTIFICATION_ID = 1
        const val CHANNEL_ID = "meditate_session"

        // Wake-lock window and its renewal period (see wakeLock).
        const val WAKE_LOCK_WINDOW_MS = 60 * 60 * 1000L
        const val WAKE_LOCK_RENEW_MS = 30 * 60 * 1000L

        // "Meditation complete": its own slot and channel. The session
        // notification is silent/low and is removed with the service,
        // while this one has to outlive the session and show in the
        // status bar. Silent too, because the end bell already rang.
        const val COMPLETE_NOTIFICATION_ID = 2
        const val COMPLETE_CHANNEL_ID = "meditate_complete"

        // requestPermissions request code. Value is arbitrary — we
        // never read the callback (no override of
        // onRequestPermissionsResult on the NativeActivity), the
        // grant just lands in the package's permission state and the
        // *next* start() call picks it up. 4242 is just a number.
        const val PERMISSION_REQUEST_CODE = 4242

        /// Kicks the foreground service from Rust via JNI. Called
        /// from `meditate-android/src/service.rs::start`. The
        /// `Context` argument is the host activity (NativeActivity);
        /// `startForegroundService` requires a Context with
        /// FOREGROUND_SERVICE permission, which the manifest grants
        /// app-wide.
        ///
        /// Also opportunistically requests POST_NOTIFICATIONS on
        /// Android 13+ if it hasn't been granted yet. The dialog is
        /// async; the service still starts in the background, just
        /// without a visible notification until the user grants. We
        /// don't gate service start on the result — silent operation
        /// is the documented degraded mode.
        @JvmStatic
        fun start(context: Context) {
            ensureNotificationPermission(context)
            val intent = Intent(context, MeditateSessionService::class.java).apply {
                action = ACTION_START_SESSION
            }
            context.startForegroundService(intent)
        }

        private fun ensureNotificationPermission(context: Context) {
            // POST_NOTIFICATIONS only became a runtime permission in
            // Android 13 (TIRAMISU, API 33). On 26..=32 the manifest
            // entry is enough; the system grants it at install time.
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return
            val granted = context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) ==
                PackageManager.PERMISSION_GRANTED
            if (granted) return
            // requestPermissions needs an Activity host — only the
            // Activity can route the result back through Android's
            // permission UI. Rust always passes the NativeActivity
            // here, but the cast is defensive in case some future
            // entry point passes a service / application Context.
            val activity = context as? Activity ?: return
            activity.requestPermissions(
                arrayOf(Manifest.permission.POST_NOTIFICATIONS),
                PERMISSION_REQUEST_CODE,
            )
        }

        /// Posted from Rust when a session reaches its planned end
        /// while the app is in the background (GTK sends the same
        /// notification when its window isn't focused). `duration`
        /// is the formatted session length. Tapping opens the app and
        /// dismisses the notification.
        @JvmStatic
        fun notifyComplete(context: Context, duration: String) {
            val mgr = context.getSystemService(NotificationManager::class.java)
            val channel = NotificationChannel(
                COMPLETE_CHANNEL_ID,
                context.getString(R.string.notification_channel_complete),
                NotificationManager.IMPORTANCE_DEFAULT,
            ).apply {
                setSound(null, null)
                enableVibration(false)
            }
            mgr.createNotificationChannel(channel)
            val notification = Notification.Builder(context, COMPLETE_CHANNEL_ID)
                .setContentTitle(context.getString(R.string.notification_complete_title))
                .setContentText(context.getString(R.string.notification_complete_body, duration))
                .setSmallIcon(R.drawable.ic_launcher_foreground)
                .setAutoCancel(true)
                .setContentIntent(launchIntent(context))
                .build()
            mgr.notify(COMPLETE_NOTIFICATION_ID, notification)
        }

        /// Withdraws a stale "Meditation complete" once the app is back
        /// in front or the next session starts.
        @JvmStatic
        fun clearComplete(context: Context) {
            context.getSystemService(NotificationManager::class.java)
                .cancel(COMPLETE_NOTIFICATION_ID)
        }

        /// Launcher activity brought to front (SINGLE_TOP + CLEAR_TOP
        /// so the in-flight Slint UI isn't replaced by a duplicate).
        fun launchIntent(context: Context): PendingIntent {
            val launch = context.packageManager
                .getLaunchIntentForPackage(context.packageName)?.apply {
                    addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
                } ?: Intent()
            // FLAG_IMMUTABLE is mandatory on Android 12+ (API 31) for any
            // PendingIntent we don't need to mutate post-construction. We
            // never read extras out of this intent, so immutable is fine.
            return PendingIntent.getActivity(
                context,
                0,
                launch,
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
        }

        @JvmStatic
        fun stop(context: Context) {
            val intent = Intent(context, MeditateSessionService::class.java).apply {
                action = ACTION_STOP_SESSION
            }
            context.startService(intent)
        }

        /// Stop a service an earlier screen left running. stopService
        /// never creates one, so a cold start shows no media control.
        @JvmStatic
        fun stopLeftover(context: Context) {
            context.stopService(Intent(context, MeditateSessionService::class.java))
        }

        /// A paused session lets the phone sleep; the service and its
        /// notification stay.
        @JvmStatic
        fun pause(context: Context) {
            val intent = Intent(context, MeditateSessionService::class.java).apply {
                action = ACTION_PAUSE_SESSION
            }
            context.startService(intent)
        }

        /// The media control's buttons from Rust (service.rs
        /// show_buttons): the accent's ARGB, then "code<TAB>label" lines.
        @JvmStatic
        fun showButtons(context: Context, payload: String) {
            val intent = Intent(context, MeditateSessionService::class.java).apply {
                action = ACTION_SHOW_BUTTONS
                putExtra(EXTRA_PAYLOAD, payload)
            }
            context.startService(intent)
        }

        /// The Void-Controls switch from Rust (service.rs
        /// set_void_controls), "true" or "false".
        @JvmStatic
        fun voidControls(context: Context, on: String) {
            val intent = Intent(context, MeditateSessionService::class.java).apply {
                action = ACTION_VOID_CONTROLS
                putExtra(EXTRA_PAYLOAD, on == "true")
            }
            context.startService(intent)
        }

        @JvmStatic
        fun resume(context: Context) {
            val intent = Intent(context, MeditateSessionService::class.java).apply {
                action = ACTION_RESUME_SESSION
            }
            context.startService(intent)
        }
    }

    // MediaSession is the lever that pins our notification in the
    // Media-controls section of the notification shade (the slot
    // music players live in). Without it, the FGS notification lands
    // in the regular swipeable section — Android 14+ relaxed the
    // setOngoing-blocks-swipe rule, so Media is the only reliable
    // pin path. The session itself doesn't drive any audio playback
    // — Phase 5 will plug bell-cue audio into the same session.
    private var mediaSession: MediaSession? = null

    // Set by START, so a SHOW_BUTTONS racing a stop can't post a
    // notification for a session that is over.
    private var started = false
    private var buttons: List<Pair<Int, String>> = emptyList()
    private var accent: Int? = null
    // The header's title and line (the session's time), its bar, and
    // the lotus art drawn once per accent.
    private var title: String? = null
    private var line: String? = null
    private var total = 0L
    private var art: Bitmap? = null
    // Held in a field: a collected lock is released early.
    private var tapWakeLock: PowerManager.WakeLock? = null

    // Void-Controls: listening only while on during a session. A paused
    // session keeps the CPU awake while on, so turning face up still
    // reaches the listener.
    private var voidOn = false
    private var paused = false
    private val detector = VoidDetector()

    // The FGS keeps the *process* alive, but not the CPU: in Doze /
    // deep suspend the 200 ms Slint tick stops being scheduled and
    // interval/end bells fire late (CLOCK_BOOTTIME keeps elapsed
    // correct, so nothing is lost — just delayed). A partial wake
    // lock for the session's duration is the alarm-clock-grade
    // guarantee that the bell rings on time; battery cost over a
    // meditation-length window is negligible. The lock is taken
    // for an hour at a time and renewed every half hour while the
    // session runs, so a session of any length (up to 23 h, or an
    // open stopwatch / overtime) keeps it, and a leaked lock still
    // lapses within the hour.
    private var wakeLock: PowerManager.WakeLock? = null
    private val wakeLockHandler = Handler(Looper.getMainLooper())
    private val renewWakeLock = object : Runnable {
        override fun run() {
            val lock = wakeLock ?: return
            // Not reference-counted: acquiring again just restarts
            // the timeout window.
            runCatching { lock.acquire(WAKE_LOCK_WINDOW_MS) }
            wakeLockHandler.postDelayed(this, WAKE_LOCK_RENEW_MS)
        }
    }

    override fun onCreate() {
        super.onCreate()
        createChannel()
        mediaSession = MediaSession(this, "Meditate").apply {
            setPlaybackState(
                PlaybackState.Builder()
                    // STATE_PLAYING + isActive=true is the combination
                    // Android uses to decide "this is active media,
                    // pin the notification". Position is unknown
                    // because we're not actually streaming audio yet
                    // (Phase 1) — speed 1.0 keeps the position
                    // counter idle rather than scrubbing.
                    .setState(
                        PlaybackState.STATE_PLAYING,
                        PlaybackState.PLAYBACK_POSITION_UNKNOWN,
                        1.0f,
                    )
                    .build(),
            )
            setCallback(object : MediaSession.Callback() {
                override fun onCustomAction(action: String, extras: android.os.Bundle?) {
                    action.toIntOrNull()?.let { writeTap(it) }
                }

                override fun onMediaButtonEvent(mediaButtonIntent: Intent): Boolean {
                    val key = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                        mediaButtonIntent.getParcelableExtra(Intent.EXTRA_KEY_EVENT, KeyEvent::class.java)
                    } else {
                        @Suppress("DEPRECATION")
                        mediaButtonIntent.getParcelableExtra(Intent.EXTRA_KEY_EVENT)
                    } ?: return false
                    val toggle = key.keyCode in listOf(
                        KeyEvent.KEYCODE_MEDIA_PLAY_PAUSE,
                        KeyEvent.KEYCODE_HEADSETHOOK,
                        KeyEvent.KEYCODE_MEDIA_PLAY,
                        KeyEvent.KEYCODE_MEDIA_PAUSE,
                    )
                    if (!toggle) return false
                    if (key.action == KeyEvent.ACTION_DOWN && key.repeatCount == 0) {
                        writeTap(MEDIA_HEADPHONES)
                    }
                    return true
                }
            })
            isActive = true
        }
    }

    // Hands the tap to the Rust tick (service.rs take_tap), which
    // checks it against the buttons the session offers right now.
    private fun writeTap(code: Int) {
        runCatching {
            val lock = tapWakeLock ?: getSystemService(PowerManager::class.java)
                .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "Meditate:tap")
                .apply { setReferenceCounted(false) }
                .also { tapWakeLock = it }
            lock.acquire(TAP_WAKE_MS)
            val f = File(File(filesDir, "meditate"), "session_tap")
            f.parentFile?.mkdirs()
            MeditateDropFile.write(f, code.toString())
        }
    }

    // Android 13+ draws the media control from the playback state's
    // custom actions, the metadata (title, line, length, art) and the
    // playback position; older versions use the notification's text
    // and actions (buildNotification). The payload is the accent's
    // ARGB, the header (app.rs media_header), then the buttons.
    private fun showButtons(payload: String) {
        val lines = payload.split("\n")
        val newAccent = lines.first().toLongOrNull()?.toInt()
        val header = lines.getOrNull(1)?.split("\t")
        val newButtons = lines.drop(2).mapNotNull { line ->
            val (code, label) = line.split("\t", limit = 2).takeIf { it.size == 2 } ?: return@mapNotNull null
            code.toIntOrNull()?.let { it to label }
        }
        val session = mediaSession ?: return
        val newBar = header?.getOrNull(3).orEmpty()
        // Rust sends only a changed payload, and the bar's start is
        // exact (app.rs media_header), so a resent bar doesn't jump.
        buttons = newButtons
        session.setPlaybackState(playbackState(newBar))
        if (newAccent != null && newAccent != accent) {
            accent = newAccent
            art = lotusArt(newAccent)
        }
        if (header != null) {
            title = header.getOrNull(0)
            line = header.getOrNull(1)
            total = header.getOrNull(2)?.toLongOrNull() ?: 0L
        }
        session.setMetadata(
            MediaMetadata.Builder()
                .putString(MediaMetadata.METADATA_KEY_TITLE, title ?: getString(R.string.app_name))
                .putString(MediaMetadata.METADATA_KEY_ARTIST, line ?: getString(R.string.notification_session_running))
                // No length, no bar: a session without an end.
                .apply { if (total > 0) putLong(MediaMetadata.METADATA_KEY_DURATION, total * 1000) }
                .apply { art?.let { putBitmap(MediaMetadata.METADATA_KEY_ART, it) } }
                .build(),
        )
        getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, buildNotification())
    }

    // A moving bar plays from its start on the boot clock, which is
    // elapsedRealtime's clock; a standing one plays at speed 0 where it
    // stands. Always PLAYING: SystemUI drops a paused session's control
    // after 10 minutes, and a long pause or overtime still needs it.
    // No seek action: the bar can't be dragged.
    private fun playbackState(bar: String): PlaybackState {
        val now = android.os.SystemClock.elapsedRealtime()
        val start = bar.toLongOrNull()
        val stand = bar.removePrefix("p").takeIf { bar.startsWith("p") }?.toLongOrNull()
        val state = PlaybackState.Builder().setState(
            PlaybackState.STATE_PLAYING,
            when {
                start != null -> (now - start).coerceAtLeast(0)
                stand != null -> stand
                else -> PlaybackState.PLAYBACK_POSITION_UNKNOWN
            },
            if (start != null) 1.0f else 0.0f,
            now,
        )
        for ((code, label) in buttons) {
            state.addCustomAction(
                PlaybackState.CustomAction.Builder(code.toString(), label, buttonIcon(code)).build(),
            )
        }
        return state.build()
    }

    private fun buttonIcon(code: Int): Int = when (code) {
        MEDIA_PAUSE -> R.drawable.ic_pause
        MEDIA_RESUME -> R.drawable.ic_play
        MEDIA_SAVE -> R.drawable.ic_check
        MEDIA_ADD -> R.drawable.ic_add
        MEDIA_DISCARD -> R.drawable.ic_close
        else -> R.drawable.ic_close // every code is one of the above (media_buttons)
    }

    // The app icon's white lotus on the accent colour. Large on
    // purpose: MediaSession scales it down to what the system shows.
    // Drawn once per accent, since the metadata is resent with every line.
    private fun lotusArt(color: Int): Bitmap {
        val size = 1024
        val art = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(art)
        canvas.drawColor(color)
        // A little smaller and paler than the icon, still in the middle,
        // so the title and line over it stay readable.
        getDrawable(R.drawable.ic_launcher_foreground)?.mutate()?.apply {
            val inset = size / 8
            setBounds(inset, inset, size - inset, size - inset)
            alpha = 150
            draw(canvas)
        }
        return art
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START_SESSION -> {
                started = true
                paused = false
                startForeground(NOTIFICATION_ID, buildNotification())
                acquireWakeLock()
            }
            ACTION_PAUSE_SESSION -> {
                paused = true
                updateWakeLock()
            }
            ACTION_RESUME_SESSION -> {
                paused = false
                updateWakeLock()
            }
            ACTION_VOID_CONTROLS -> when {
                !started -> stopSelf()
                else -> setVoidControls(intent.getBooleanExtra(EXTRA_PAYLOAD, false))
            }
            // Either one reaching a stopped service would leave a fresh
            // instance with an active MediaSession behind.
            ACTION_SHOW_BUTTONS, ACTION_TAP -> when {
                !started -> stopSelf()
                intent.action == ACTION_TAP -> writeTap(intent.getIntExtra(EXTRA_CODE, -1))
                else -> intent.getStringExtra(EXTRA_PAYLOAD)?.let { showButtons(it) }
            }
            ACTION_STOP_SESSION -> {
                setVoidControls(false)
                started = false
                paused = false
                buttons = emptyList()
                releaseWakeLock()
                // STOP_FOREGROUND_REMOVE = clear the notification
                // even though the user might still be on the Done
                // screen — the timer's done, the notification's
                // role is over.
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
            }
        }
        // NOT_STICKY: if the OS kills us mid-session for memory
        // pressure, do NOT auto-restart. The Phase 3 crash-recovery
        // snapshot in meditate-core will resurface the in-flight
        // session on the next launch; we don't want a zombie
        // service running with no UI counterpart.
        return START_NOT_STICKY
    }

    // Task swiped away mid-session: the activity (and with it the
    // Slint tick loop driving the timer) is gone, but a started
    // FGS survives — leaving a wake lock + "session in progress"
    // notification pinned for a session nothing is advancing.
    // Stop instead; the core crash-recovery snapshot resurfaces
    // the in-flight session on next launch (same contract as
    // START_NOT_STICKY after an OOM kill). The guided track and
    // bells live in process-wide players that outlast the task, so
    // silence them too.
    override fun onTaskRemoved(rootIntent: Intent?) {
        setVoidControls(false)
        MeditateGuided.stopAudio(this)
        MeditateAudio.stop(this)
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
        super.onTaskRemoved(rootIntent)
    }

    override fun onDestroy() {
        setVoidControls(false)
        releaseWakeLock()
        // MediaSession holds a system-side binder; leaking it shows
        // up as a ghost media control until the next reboot. release()
        // also clears isActive/playback state so a stray scan can't
        // resurrect our pin slot.
        mediaSession?.release()
        mediaSession = null
        super.onDestroy()
    }

    private fun setVoidControls(on: Boolean) {
        val sensors = getSystemService(SensorManager::class.java)
        sensors.unregisterListener(detector)
        voidOn = on
        if (voidOn) {
            detector.reset()
            sensors.getDefaultSensor(Sensor.TYPE_ACCELEROMETER)?.let {
                sensors.registerListener(detector, it, SensorManager.SENSOR_DELAY_GAME)
            }
        }
        updateWakeLock()
    }

    // Awake while a session runs, and in a pause while Void-Controls
    // listen for the turn back.
    private fun updateWakeLock() {
        if (started && (!paused || voidOn)) acquireWakeLock() else releaseWakeLock()
    }

    // Turns and shakes from the accelerometer, written like media taps
    // for the Rust tick, which checks them against the session's buttons.
    private inner class VoidDetector : SensorEventListener {
        // 1 face up, -1 face down, 0 not known yet: the first side is
        // the baseline, so starting face down pauses nothing.
        private var side = 0
        private var candidate = 0
        private var candidateSince = 0L
        private val peaks = ArrayDeque<Long>()
        private var quietUntil = 0L
        // A peak counts when the force rises past the threshold and is
        // released only well below it, so one knock or a hard set-down
        // (several samples, maybe wobbling) is one peak.
        private var above = false

        fun reset() {
            side = 0
            candidate = 0
            peaks.clear()
            quietUntil = 0L
            above = false
        }

        override fun onSensorChanged(event: SensorEvent) {
            val t = event.timestamp / 1_000_000
            val (x, y, z) = event.values
            val force = x * x + y * y + z * z
            val hard = force > SHAKE_MS2 * SHAKE_MS2
            val rising = hard && !above
            if (hard) above = true
            else if (force < SHAKE_RELEASE_MS2 * SHAKE_RELEASE_MS2) above = false
            if (rising) {
                peaks.addLast(t)
                while (peaks.first() < t - SHAKE_WINDOW_MS) peaks.removeFirst()
                if (peaks.size >= SHAKE_PEAKS) {
                    // The side it ends on is the new baseline.
                    reset()
                    above = true
                    quietUntil = t + SHAKE_QUIET_MS
                    writeTap(VOID_SHAKE)
                    return
                }
            }
            if (t < quietUntil) return
            val now = if (z > FACE_MS2) 1 else if (z < -FACE_MS2) -1 else 0
            if (now == 0 || now == side) {
                candidate = 0
                return
            }
            if (now != candidate) {
                candidate = now
                candidateSince = t
                return
            }
            if (t - candidateSince < FACE_HOLD_MS) return
            val was = side
            side = now
            candidate = 0
            if (was != 0) writeTap(if (now < 0) VOID_FACE_DOWN else VOID_FACE_UP)
        }

        override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) {}
    }

    private fun acquireWakeLock() {
        if (wakeLock != null) return
        val pm = getSystemService(PowerManager::class.java)
        wakeLock = pm.newWakeLock(
            PowerManager.PARTIAL_WAKE_LOCK,
            "Meditate:session",
        ).apply {
            setReferenceCounted(false)
            acquire(WAKE_LOCK_WINDOW_MS)
        }
        wakeLockHandler.postDelayed(renewWakeLock, WAKE_LOCK_RENEW_MS)
    }

    private fun releaseWakeLock() {
        wakeLockHandler.removeCallbacks(renewWakeLock)
        wakeLock?.let { runCatching { if (it.isHeld) it.release() } }
        wakeLock = null
    }

    // No client binding — this is a started-service, not a bound-
    // service. Returning null is the documented signal.
    override fun onBind(intent: Intent?): IBinder? = null

    private fun createChannel() {
        val mgr = getSystemService(NotificationManager::class.java)
        val channel = NotificationChannel(
            CHANNEL_ID,
            getString(R.string.notification_channel_session),
            NotificationManager.IMPORTANCE_LOW,
        ).apply {
            description =
                getString(R.string.notification_channel_session_desc)
            // Silent: the notification is informational, not an
            // alert. Bell-fire audio (Phase 5) lives elsewhere.
            setSound(null, null)
            enableVibration(false)
        }
        mgr.createNotificationChannel(channel)
    }

    private fun buildNotification(): Notification {
        // MediaStyle + the session token is what lands us in the
        // shade's Media-controls section instead of the regular
        // dismissible list. setOngoing(true) still helps for the
        // (uncommon) case where the session is briefly inactive but
        // the service hasn't stopped yet.
        val style = Notification.MediaStyle()
        mediaSession?.let { style.setMediaSession(it.sessionToken) }
        style.setShowActionsInCompactView(*buttons.indices.take(3).toIntArray())
        val builder = Notification.Builder(this, CHANNEL_ID)
        for ((code, label) in buttons) {
            val tap = Intent(this, MeditateSessionService::class.java).apply {
                action = ACTION_TAP
                putExtra(EXTRA_CODE, code)
            }
            val pending = PendingIntent.getService(this, code, tap, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
            builder.addAction(
                Notification.Action.Builder(Icon.createWithResource(this, buttonIcon(code)), label, pending).build(),
            )
        }
        return builder
            .setContentTitle(title ?: getString(R.string.app_name))
            .setContentText(line ?: getString(R.string.notification_session_running))
            .setSmallIcon(R.drawable.ic_launcher_foreground)
            .setOngoing(true)
            .setVisibility(Notification.VISIBILITY_PUBLIC)
            .setCategory(Notification.CATEGORY_TRANSPORT)
            .setContentIntent(buildContentIntent())
            .setStyle(style)
            .build()
    }

    /// The PendingIntent that fires when the user taps the
    /// notification: the launcher activity, resolved via
    /// PackageManager so we don't need a hard-coded class name (the
    /// Slint NativeActivity's real Java class is generated at build
    /// time).
    private fun buildContentIntent(): PendingIntent = launchIntent(this)
}
