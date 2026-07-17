// System window insets (Phase 8): status-bar / display-cutout top
// and navigation-bar bottom, in DENSITY-INDEPENDENT pixels (Slint
// logical px == dp). Replaces the hardcoded 36px/18px guesses that
// only fit the FP5 in portrait. Queried from Rust (src/insets.rs)
// at startup and re-queried whenever the Slint window resizes
// (rotation, multi-window) — the decorView's rootWindowInsets are
// current by then.

package io.github.janekbt.Meditate

import android.app.Activity
import android.os.Build
import android.util.Log
import android.view.WindowInsets

object MeditateInsets {
    private const val TAG = "MeditateInsets"

    /// "top,bottom" in dp, or "" when insets aren't available yet
    /// (pre-attach) — caller keeps its previous values then.
    @JvmStatic
    fun getInsets(activity: Activity): String = try {
        val wi = activity.window.decorView.rootWindowInsets
        if (wi == null) {
            ""
        } else {
            val density = activity.resources.displayMetrics.density
            val (topPx, bottomPx) = if (
                Build.VERSION.SDK_INT >= Build.VERSION_CODES.R
            ) {
                val bars = wi.getInsets(
                    WindowInsets.Type.systemBars()
                        or WindowInsets.Type.displayCutout(),
                )
                bars.top to bars.bottom
            } else {
                @Suppress("DEPRECATION")
                (wi.systemWindowInsetTop to wi.systemWindowInsetBottom)
            }
            "${topPx / density},${bottomPx / density}"
        }
    } catch (e: Exception) {
        Log.w(TAG, "getInsets failed: $e")
        ""
    }
}
