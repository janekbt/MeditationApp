// Takes the home-screen widget's row tap (the template PendingIntent
// built in MeditateWidgetProvider). Not exported: the provider must
// be, for the launcher's update broadcasts, and any app could then
// have started a session through it.

package io.github.janekbt.Meditate

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import java.io.File

class MeditateWidgetLaunchReceiver : BroadcastReceiver() {

    override fun onReceive(context: Context, intent: Intent) {
        val uuid = intent.getStringExtra("preset_uuid")
        if (!uuid.isNullOrEmpty()) {
            writeLaunchFile(context, uuid)
        }
        // Foreground the app (cold start: this is the first
        // launch; warm: brings the running task forward). The
        // Rust side picks the uuid up from the drop file at
        // android_main and on every tick.
        try {
            val launch = context.packageManager
                .getLaunchIntentForPackage(context.packageName)
            if (launch != null) {
                launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                context.startActivity(launch)
            }
        } catch (e: Exception) {
            Log.w(TAG, "deep-link launch failed: $e")
        }
    }

    private fun writeLaunchFile(context: Context, uuid: String) {
        try {
            // Mirrors widget::LAUNCH_FILENAME + the open_database
            // layout (`<filesDir>/meditate/`). The dir normally
            // already exists (the user had to open the app to
            // star a preset); mkdirs is defensive.
            val dir = File(context.filesDir, "meditate")
            dir.mkdirs()
            MeditateDropFile.write(File(dir, "widget_launch"), uuid)
        } catch (e: Exception) {
            Log.w(TAG, "writeLaunchFile failed: $e")
        }
    }

    companion object {
        private const val TAG = "MeditateWidget"
    }
}
