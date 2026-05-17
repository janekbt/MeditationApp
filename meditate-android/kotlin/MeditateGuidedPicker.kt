// Launches the Guided-mode SAF picker Activity. Called from Rust
// (src/guided.rs) via the app-classloader JNI path. The picker
// must be its own Activity because NativeActivity can't receive
// onActivityResult; it hands the result back through a drop-file
// the Rust tick loop polls (same channel as the widget launch).

package io.github.janekbt.Meditate

import android.content.Context
import android.content.Intent

object MeditateGuidedPicker {
    @JvmStatic
    fun open(context: Context) {
        val i = Intent(context, MeditateFilePickerActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        context.startActivity(i)
    }
}
