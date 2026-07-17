// Keep-screen-awake during a running session — the Android
// analogue of the GTK shell's `gtk_application_inhibit` idle
// inhibitor. FLAG_KEEP_SCREEN_ON keeps the display on only while
// our window is foreground and only while the flag is set (no
// WAKE_LOCK permission, auto-released if the app is backgrounded),
// which matches the "prevent display sleep during a session"
// intent. Driven from Rust (src/screen.rs) via the app-classloader
// JNI bridge, in step with the session lifecycle: set on
// Idle/Finished → Active when the active mode's
// `*_keep_screen_awake` setting is on, cleared on every
// session-end path.
//
// Window flags must be touched on the UI thread.

package io.github.janekbt.Meditate

import android.app.Activity
import android.view.WindowManager

object MeditateScreen {
    @JvmStatic
    fun setKeepAwake(activity: Activity, on: Boolean) {
        activity.runOnUiThread {
            val flag = WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON
            if (on) {
                activity.window.addFlags(flag)
            } else {
                activity.window.clearFlags(flag)
            }
        }
    }
}
