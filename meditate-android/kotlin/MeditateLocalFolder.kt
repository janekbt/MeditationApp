// Guided Local Folder (GitHub #4): walks the folder the user chose
// (a document tree with persisted read access), reads file lengths,
// and resolves a file for playing. Called from Rust
// (src/local_folder.rs) over JNI; results come back through drop
// files the Rust tick loop polls, like the picker's. Nothing is
// copied.

package io.github.janekbt.Meditate

import android.content.Context
import android.net.Uri
import android.provider.DocumentsContract
import android.provider.DocumentsContract.Document
import android.util.Log
import java.io.File

object MeditateLocalFolder {
    private const val TAG = "MeditateLocalFolder"
    private const val SCAN_FILE = "local_folder_scan"
    private const val LENGTHS_FILE = "local_folder_lengths"

    private val lock = Any()
    // Every length read in this run of the app, "id\tsecs" per line
    // (0 = unreadable). Rewritten whole after each read; applying a
    // line twice does no harm, so a result is never lost to a race
    // with the Rust side taking the file.
    private val lengths = StringBuilder()

    private fun dropFile(context: Context, name: String) =
        File(File(context.filesDir, "meditate"), name)

    // Walk the tree on a worker thread. Drop file: "ok" then one
    // "rel_path\tsize\tmtime_secs" line per file, or "err" when the
    // folder can't be read.
    @JvmStatic
    fun scan(context: Context, tree: String) {
        val app = context.applicationContext
        Thread {
            val text = try {
                val treeUri = Uri.parse(tree)
                val out = StringBuilder("ok\n")
                walk(app, treeUri, DocumentsContract.getTreeDocumentId(treeUri), "", out)
                out.toString()
            } catch (e: Exception) {
                Log.w(TAG, "scan failed: $e")
                "err"
            }
            MeditateDropFile.write(dropFile(app, SCAN_FILE), text)
        }.start()
    }

    private fun walk(context: Context, treeUri: Uri, parentId: String, prefix: String, out: StringBuilder) {
        val children = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, parentId)
        val projection = arrayOf(
            Document.COLUMN_DOCUMENT_ID,
            Document.COLUMN_DISPLAY_NAME,
            Document.COLUMN_MIME_TYPE,
            Document.COLUMN_SIZE,
            Document.COLUMN_LAST_MODIFIED,
        )
        val cursor = context.contentResolver.query(children, projection, null, null, null)
            ?: throw IllegalStateException("query null")
        val folders = ArrayList<Pair<String, String>>()
        cursor.use { c ->
            while (c.moveToNext()) {
                val id = c.getString(0) ?: continue
                val name = c.getString(1) ?: continue
                if (name.contains('\t') || name.contains('\n')) continue
                val rel = prefix + name
                if (c.getString(2) == Document.MIME_TYPE_DIR) {
                    folders.add(id to "$rel/")
                } else {
                    val size = if (c.isNull(3)) 0L else c.getLong(3)
                    val mtime = if (c.isNull(4)) 0L else c.getLong(4) / 1000
                    out.append(rel).append('\t').append(size).append('\t').append(mtime).append('\n')
                }
            }
        }
        // A subfolder that can't be read is skipped; only the top
        // failing means the folder is unreachable.
        for ((id, sub) in folders) {
            runCatching { walk(context, treeUri, id, sub, out) }
        }
    }

    // Read the lengths of `rels` (ids alongside) one by one on a
    // worker thread. Drop file: every result so far, see `lengths`.
    @JvmStatic
    fun readLengths(context: Context, tree: String, ids: LongArray, rels: Array<String>) {
        val app = context.applicationContext
        Thread {
            for (i in ids.indices) {
                val uri = runCatching { resolve(app, tree, rels[i]) }.getOrNull()
                val secs = if (uri == null) {
                    0L
                } else {
                    MeditateFilePickerActivity.probeWith { it.setDataSource(app, uri, null) }.first
                }
                synchronized(lock) {
                    lengths.append(ids[i]).append('\t').append(secs).append('\n')
                    MeditateDropFile.write(dropFile(app, LENGTHS_FILE), lengths.toString())
                }
            }
        }.start()
    }

    // The content address of `rel` under `tree`, as a string for
    // MeditateGuided.startAudio; null when it is gone.
    @JvmStatic
    fun documentAddress(context: Context, tree: String, rel: String): String? =
        runCatching { resolve(context, tree, rel)?.toString() }
            .onFailure { Log.w(TAG, "resolve failed: $it") }
            .getOrNull()

    private fun resolve(context: Context, tree: String, rel: String): Uri? {
        val treeUri = Uri.parse(tree)
        var docId = DocumentsContract.getTreeDocumentId(treeUri)
        for (segment in rel.split('/')) {
            docId = child(context, treeUri, docId, segment) ?: return null
        }
        return DocumentsContract.buildDocumentUriUsingTree(treeUri, docId)
    }

    private fun child(context: Context, treeUri: Uri, parentId: String, name: String): String? {
        val children = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, parentId)
        val projection = arrayOf(Document.COLUMN_DOCUMENT_ID, Document.COLUMN_DISPLAY_NAME)
        context.contentResolver.query(children, projection, null, null, null)?.use { c ->
            while (c.moveToNext()) {
                if (c.getString(1) == name) return c.getString(0)
            }
        }
        return null
    }
}
