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
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.os.Build
import android.os.IBinder
import android.os.PowerManager

class MeditateSessionService : Service() {
    companion object {
        // Distinct constants for the two intent actions so a future
        // mid-session-update intent can be added without colliding.
        const val ACTION_START_SESSION = "io.github.janekbt.Meditate.START_SESSION"
        const val ACTION_STOP_SESSION = "io.github.janekbt.Meditate.STOP_SESSION"

        // Persistent notification slot. Stable across start/stop so
        // a stop-then-start within the same process re-uses the
        // same notification ID without leaving an orphan in the
        // shade.
        const val NOTIFICATION_ID = 1
        const val CHANNEL_ID = "meditate_session"

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

        @JvmStatic
        fun stop(context: Context) {
            val intent = Intent(context, MeditateSessionService::class.java).apply {
                action = ACTION_STOP_SESSION
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

    // The FGS keeps the *process* alive, but not the CPU: in Doze /
    // deep suspend the 200 ms Slint tick stops being scheduled and
    // interval/end bells fire late (CLOCK_BOOTTIME keeps elapsed
    // correct, so nothing is lost — just delayed). A partial wake
    // lock for the session's duration is the alarm-clock-grade
    // guarantee that the bell rings on time; battery cost over a
    // meditation-length window is negligible. 4 h timeout is a
    // leak backstop far above any real session.
    private var wakeLock: PowerManager.WakeLock? = null

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
            isActive = true
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START_SESSION -> {
                startForeground(NOTIFICATION_ID, buildNotification())
                if (wakeLock == null) {
                    val pm = getSystemService(PowerManager::class.java)
                    wakeLock = pm.newWakeLock(
                        PowerManager.PARTIAL_WAKE_LOCK,
                        "Meditate:session",
                    ).apply { acquire(4 * 60 * 60 * 1000L) }
                }
            }
            ACTION_STOP_SESSION -> {
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

    override fun onDestroy() {
        releaseWakeLock()
        // MediaSession holds a system-side binder; leaking it shows
        // up as a ghost media control until the next reboot. release()
        // also clears isActive/playback state so a stray scan can't
        // resurrect our pin slot.
        mediaSession?.release()
        mediaSession = null
        super.onDestroy()
    }

    private fun releaseWakeLock() {
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
        // `android.R.drawable.ic_media_play` is a system-provided
        // play-triangle drawable — works without us shipping a
        // custom resource, and reads as "session in progress" at a
        // glance in the notification shade. Phase 8's theming pass
        // can swap to a meditate-branded vector drawable.
        //
        // MediaStyle + the session token is what lands us in the
        // shade's Media-controls section instead of the regular
        // dismissible list. setOngoing(true) still helps for the
        // (uncommon) case where the session is briefly inactive but
        // the service hasn't stopped yet.
        val style = Notification.MediaStyle()
        mediaSession?.let { style.setMediaSession(it.sessionToken) }
        return Notification.Builder(this, CHANNEL_ID)
            .setContentTitle(getString(R.string.app_name))
            .setContentText(getString(R.string.notification_session_running))
            .setSmallIcon(android.R.drawable.ic_media_play)
            .setOngoing(true)
            .setCategory(Notification.CATEGORY_TRANSPORT)
            .setContentIntent(buildContentIntent())
            .setStyle(style)
            .build()
    }

    /// Builds the PendingIntent that fires when the user taps the
    /// notification. Resolves the app's launcher activity via
    /// PackageManager so we don't need a hard-coded class name (the
    /// Slint NativeActivity's real Java class is generated at build
    /// time). SINGLE_TOP + CLEAR_TOP brings the existing instance
    /// forward rather than spawning a duplicate that would lose the
    /// in-flight Slint UI state.
    private fun buildContentIntent(): PendingIntent {
        val launch = packageManager.getLaunchIntentForPackage(packageName)?.apply {
            addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
        } ?: Intent()
        // FLAG_IMMUTABLE is mandatory on Android 12+ (API 31) for any
        // PendingIntent we don't need to mutate post-construction. We
        // never read extras out of this intent, so immutable is fine.
        return PendingIntent.getActivity(
            this,
            0,
            launch,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
    }
}
