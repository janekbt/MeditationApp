// Drop files: how the Kotlin side hands a result (a picked file, an
// import outcome, a widget tap) to the Rust tick loop, which polls
// every 200 ms, reads the file and deletes it. Written to a sibling
// temp file and renamed into place, so a tick can never read a file
// that is created but not written yet (and lose the event).

package io.github.janekbt.Meditate

import java.io.File

object MeditateDropFile {
    fun write(file: File, text: String) {
        val tmp = File(file.parentFile, file.name + ".tmp")
        tmp.writeText(text)
        if (!tmp.renameTo(file)) {
            // rename(2) replaces atomically; this is only a fallback.
            file.writeText(text)
            tmp.delete()
        }
    }
}
