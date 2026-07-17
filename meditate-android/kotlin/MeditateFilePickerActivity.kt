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
import android.media.MediaExtractor
import android.media.MediaFormat
import android.net.Uri
import android.os.Bundle
import android.provider.OpenableColumns
import android.util.Log
import java.io.File

class MeditateFilePickerActivity : Activity() {

    private val REQ = 4011

    // "guided" (default) or "bell" — decides the transient-copy
    // directory and the drop-file name so the two import flows
    // can't consume each other's picks.
    private val target: String
        get() = intent.getStringExtra(MeditateGuidedPicker.EXTRA_TARGET)
            ?: "guided"

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (savedInstanceState != null) return // picker already up
        val i = when (target) {
            // Backup export (DP): let the user pick where the CSV
            // lands; the content is pre-written by Rust to the
            // src_path extra and copied over in onActivityResult.
            "export" -> Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
                type = "text/csv"
                addCategory(Intent.CATEGORY_OPENABLE)
                putExtra(
                    Intent.EXTRA_TITLE,
                    intent.getStringExtra(EXTRA_SUGGESTED_NAME)
                        ?: "meditate-sessions.csv",
                )
            }
            // CSV imports (DP): Meditate backup or Insight Timer
            // export. */* because CSV mime types are a mess across
            // file managers (text/csv, text/comma-separated-values,
            // application/csv, text/plain all occur in the wild).
            "import-meditate", "import-insight" ->
                Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                    type = "*/*"
                    addCategory(Intent.CATEGORY_OPENABLE)
                    addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                }
            // Audio picks (guided / bell import).
            else -> Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                type = "audio/*"
                addCategory(Intent.CATEGORY_OPENABLE)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
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
                when (target) {
                    "export" -> copyOutForExport(uri)
                    "import-meditate" -> copyInCsv(uri, "meditate")
                    "import-insight" -> copyInCsv(uri, "insight")
                    else -> copyAndProbe(uri)
                }
            } catch (e: Exception) {
                Log.w(TAG, "pick handling failed: $e")
            }
            runOnUiThread { finish() }
        }.start()
    }

    private fun copyAndProbe(uri: Uri) {
        val name = queryDisplayName(uri) ?: "Audio file"
        val subdir = if (target == "bell") "sounds" else "guided"
        val dropFile = if (target == "bell") "sound_pick" else "guided_pick"
        val dir = File(File(filesDir, "meditate"), subdir)
        dir.mkdirs()
        val dest = File(dir, "transient" + extOf(name))
        contentResolver.openInputStream(uri)?.use { input ->
            dest.outputStream().use { out -> input.copyTo(out) }
        } ?: run {
            Log.w(TAG, "openInputStream returned null")
            return
        }
        val durSecs = probeSecs(dest)
        File(File(filesDir, "meditate"), dropFile)
            .writeText("${dest.absolutePath}\n$name\n$durSecs")
    }

    // Export: copy the Rust-pre-written CSV (src_path extra) to
    // the user's chosen URI, then report via the export_result
    // drop-file ("ok" / "err:<msg>"). The temp file is removed
    // here so a consumed export can't be re-copied.
    private fun copyOutForExport(uri: Uri) {
        val srcPath = intent.getStringExtra(EXTRA_SRC_PATH)
        val result = try {
            val src = File(srcPath ?: throw IllegalStateException("no src"))
            contentResolver.openOutputStream(uri, "wt")?.use { out ->
                src.inputStream().use { it.copyTo(out) }
            } ?: throw IllegalStateException("openOutputStream null")
            src.delete()
            "ok"
        } catch (e: Exception) {
            Log.w(TAG, "export copy failed: $e")
            "err:" + (e.message ?: e.javaClass.simpleName)
        }
        File(File(filesDir, "meditate"), "export_result")
            .writeText(result)
    }

    // Import: copy the chosen document into app storage and drop
    // `csv_pick` = "<path>\n<kind>" for the Rust tick poll.
    private fun copyInCsv(uri: Uri, kind: String) {
        val dir = File(filesDir, "meditate")
        dir.mkdirs()
        val dest = File(dir, "import-transient.csv")
        contentResolver.openInputStream(uri)?.use { input ->
            dest.outputStream().use { out -> input.copyTo(out) }
        } ?: run {
            Log.w(TAG, "openInputStream returned null")
            return
        }
        File(dir, "csv_pick")
            .writeText("${dest.absolutePath}\n$kind")
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
    // Demuxes with MediaExtractor and walks every audio frame to
    // the LAST presentation timestamp. The container/header
    // duration (also what MediaMetadataRetriever and
    // MediaPlayer.getDuration return) is a bitrate*size estimate
    // that is badly wrong for headerless VBR MP3 (a 1:07 file
    // reported ~0:29). Walking the real frame timestamps is
    // accurate and cheap — it parses frame headers only, no
    // decode. Take max(header duration, last frame PTS).
    private fun probeSecs(f: File): Long {
        return try {
            val ex = MediaExtractor()
            ex.setDataSource(f.absolutePath)
            var bestUs = 0L
            for (i in 0 until ex.trackCount) {
                val fmt = ex.getTrackFormat(i)
                val mime = fmt.getString(MediaFormat.KEY_MIME)
                if (mime?.startsWith("audio/") != true) continue
                if (fmt.containsKey(MediaFormat.KEY_DURATION)) {
                    bestUs = maxOf(
                        bestUs,
                        fmt.getLong(MediaFormat.KEY_DURATION),
                    )
                }
                ex.selectTrack(i)
                var lastUs = 0L
                while (true) {
                    val t = ex.sampleTime
                    if (t < 0) break
                    lastUs = t
                    if (!ex.advance()) break
                }
                bestUs = maxOf(bestUs, lastUs)
                break
            }
            ex.release()
            if (bestUs > 0) (bestUs + 999_999) / 1_000_000 else 0L
        } catch (e: Exception) {
            Log.w(TAG, "duration probe failed: $e")
            0L
        }
    }

    companion object {
        private const val TAG = "MeditateGuided"
        const val EXTRA_SRC_PATH = "src_path"
        const val EXTRA_SUGGESTED_NAME = "suggested_name"
    }
}
