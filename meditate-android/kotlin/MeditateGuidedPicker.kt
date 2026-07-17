// Launches the SAF audio-picker Activity. Called from Rust
// (src/guided.rs) via the app-classloader JNI path. The picker
// must be its own Activity because NativeActivity can't receive
// onActivityResult; it hands the result back through a drop-file
// the Rust tick loop polls (same channel as the widget launch).
//
// `openFor` parameterizes the flow (BI): "guided" (default)
// lands the pick in guided/transient.<ext> + the guided_pick
// drop-file; "bell" lands in sounds/transient.<ext> +
// sound_pick, so the bell-import route never races a guided
// pick.

package io.github.janekbt.Meditate

import android.content.Context
import android.content.Intent

object MeditateGuidedPicker {
    const val EXTRA_TARGET = "target"

    @JvmStatic
    fun open(context: Context) {
        openFor(context, "guided")
    }

    @JvmStatic
    fun openFor(context: Context, target: String) {
        val i = Intent(context, MeditateFilePickerActivity::class.java)
            .putExtra(EXTRA_TARGET, target)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        context.startActivity(i)
    }

    /// Backup export (DP): CREATE_DOCUMENT flow copying the
    /// Rust-pre-written CSV at `srcPath` to the user's pick.
    @JvmStatic
    fun openExport(context: Context, srcPath: String, suggestedName: String) {
        val i = Intent(context, MeditateFilePickerActivity::class.java)
            .putExtra(EXTRA_TARGET, "export")
            .putExtra(MeditateFilePickerActivity.EXTRA_SRC_PATH, srcPath)
            .putExtra(
                MeditateFilePickerActivity.EXTRA_SUGGESTED_NAME,
                suggestedName,
            )
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        context.startActivity(i)
    }
}
