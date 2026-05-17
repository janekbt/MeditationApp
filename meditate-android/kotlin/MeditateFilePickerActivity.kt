// Transparent, no-UI Activity that runs the system audio picker
// (ACTION_OPEN_DOCUMENT) and hands the result back to Rust.
// NativeActivity can't get onActivityResult, so this standalone
// Activity owns the request/result, copies the chosen file into
// app storage, probes its duration, and writes the drop-file
// `<filesDir>/meditate/guided_pick` (3 lines: path / name /
// duration_secs) that src/guided.rs::take_pending_pick reads on
// the next tick. The copy runs off the main thread (a guided
// meditation can be tens of MB → ANR if copied on the UI
// thread); the Activity stays invisible until it finishes.

package io.github.janekbt.Meditate

import android.app.Activity
import android.content.Intent
import android.media.MediaMetadataRetriever
import android.net.Uri
import android.os.Bundle
import android.provider.OpenableColumns
import android.util.Log
import java.io.File

class MeditateFilePickerActivity : Activity() {

    private val REQ = 4011

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (savedInstanceState != null) return // picker already up
        val i = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            type = "audio/*"
            addCategory(Intent.CATEGORY_OPENABLE)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        try {
            startActivityForResult(i, REQ)
        } catch (e: Exception) {
            Log.w(TAG, "no document picker: $e")
            finish()
        }
    }

    override fun onActivityResult(
        req: Int,
        res: Int,
        data: Intent?,
    ) {
        super.onActivityResult(req, res, data)
        val uri = if (req == REQ && res == Activity.RESULT_OK) {
            data?.data
        } else {
            null
        }
        if (uri == null) {
            finish() // cancelled / error → leave the row unset
            return
        }
        // Copy off the main thread (large files would ANR).
        Thread {
            try {
                copyAndProbe(uri)
            } catch (e: Exception) {
                Log.w(TAG, "pick handling failed: $e")
            }
            runOnUiThread { finish() }
        }.start()
    }

    private fun copyAndProbe(uri: Uri) {
        val name = queryDisplayName(uri) ?: "Guided file"
        val dir = File(File(filesDir, "meditate"), "guided")
        dir.mkdirs()
        val dest = File(dir, "transient" + extOf(name))
        contentResolver.openInputStream(uri)?.use { input ->
            dest.outputStream().use { out -> input.copyTo(out) }
        } ?: run {
            Log.w(TAG, "openInputStream returned null")
            return
        }
        val durSecs = probeSecs(dest)
        File(File(filesDir, "meditate"), "guided_pick")
            .writeText("${dest.absolutePath}\n$name\n$durSecs")
    }

    private fun queryDisplayName(uri: Uri): String? {
        contentResolver.query(
            uri,
            arrayOf(OpenableColumns.DISPLAY_NAME),
            null, null, null,
        )?.use { c ->
            if (c.moveToFirst() && c.columnCount > 0) {
                return c.getString(0)
            }
        }
        return null
    }

    private fun extOf(n: String): String {
        val dot = n.lastIndexOf('.')
        return if (dot in 1 until n.length) n.substring(dot) else ""
    }

    // Whole seconds, ceil — mirrors GTK's probe_duration_secs.
    private fun probeSecs(f: File): Long {
        return try {
            val r = MediaMetadataRetriever()
            r.setDataSource(f.absolutePath)
            val ms = r.extractMetadata(
                MediaMetadataRetriever.METADATA_KEY_DURATION,
            )?.toLongOrNull() ?: 0L
            r.release()
            (ms + 999) / 1000
        } catch (e: Exception) {
            Log.w(TAG, "duration probe failed: $e")
            0L
        }
    }

    private companion object {
        const val TAG = "MeditateGuided"
    }
}
