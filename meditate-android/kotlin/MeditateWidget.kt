// JNI entry point for the home-screen widget refresh (W-series).
// Called from Rust (`meditate-android/src/widget.rs`) through the
// same app-classloader -> loadClass -> call_static_method path the
// foreground service / audio / haptics bridges use, right after
// the app rewrites `<filesDir>/meditate/widget_presets.json`.
//
// The projection file is the source of truth; this just nudges
// every installed widget instance to re-pull it via
// `notifyAppWidgetViewDataChanged`, which drives
// `MeditateWidgetFactory.onDataSetChanged()`. No-op (cheap, no
// throw) when the user has not placed a widget — `getAppWidgetIds`
// returns empty and we bail before touching the manager.

package io.github.janekbt.Meditate

import android.appwidget.AppWidgetManager
import android.content.ComponentName
import android.content.Context
import android.util.Log

object MeditateWidget {
    private const val TAG = "MeditateWidget"

    @JvmStatic
    fun refresh(context: Context) {
        try {
            val mgr = AppWidgetManager.getInstance(context) ?: return
            val component =
                ComponentName(context, MeditateWidgetProvider::class.java)
            val ids = mgr.getAppWidgetIds(component)
            if (ids == null || ids.isEmpty()) return
            // R.id.widget_list is the AdapterView the factory backs;
            // this is what re-invokes the factory's data reload.
            mgr.notifyAppWidgetViewDataChanged(ids, R.id.widget_list)
        } catch (e: Exception) {
            // An accessory must never crash the host app on a
            // refresh hiccup — log and move on (mirrors the
            // swallow-and-log contract on the Rust side).
            Log.w(TAG, "refresh failed: $e")
        }
    }
}
