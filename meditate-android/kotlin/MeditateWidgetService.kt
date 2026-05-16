// RemoteViewsService + Factory backing the widget's preset list
// (W-series). The factory runs in this app's process (same UID as
// the Rust side), so it reads the projection straight off the
// app-private files dir — no ContentProvider / IPC. The Rust
// `widget::publish` writes `<filesDir>/meditate/widget_presets.json`
// atomically (tmp + rename) before poking the manager, so a read
// here never sees a half-written file.
//
// Each row's fill-in Intent carries `preset_uuid`; merged into
// the provider's launcher template at tap time it deep-links the
// app to auto-start that preset's session (W-3).

package io.github.janekbt.Meditate

import android.content.Context
import android.content.Intent
import android.util.Log
import android.widget.RemoteViews
import android.widget.RemoteViewsService
import org.json.JSONObject
import java.io.File

class MeditateWidgetService : RemoteViewsService() {
    override fun onGetViewFactory(intent: Intent): RemoteViewsFactory =
        PresetFactory(applicationContext)
}

private data class WidgetRow(
    val uuid: String,
    val name: String,
    val subtitle: String,
)

private class PresetFactory(
    private val context: Context,
) : RemoteViewsService.RemoteViewsFactory {

    private val TAG = "MeditateWidget"

    // Mirrors widget::PROJECTION_FILENAME + the open_database
    // layout (`<filesDir>/meditate/`).
    private val projectionFile =
        File(File(context.filesDir, "meditate"), "widget_presets.json")

    @Volatile
    private var rows: List<WidgetRow> = emptyList()

    override fun onCreate() {}

    override fun onDataSetChanged() {
        rows = try {
            if (!projectionFile.exists()) {
                emptyList()
            } else {
                val json = JSONObject(projectionFile.readText())
                val arr = json.optJSONArray("presets")
                if (arr == null) {
                    emptyList()
                } else {
                    buildList {
                        for (i in 0 until arr.length()) {
                            val o = arr.getJSONObject(i)
                            add(
                                WidgetRow(
                                    uuid = o.optString("uuid"),
                                    name = o.optString("name"),
                                    subtitle = o.optString("subtitle"),
                                )
                            )
                        }
                    }
                }
            }
        } catch (e: Exception) {
            Log.w(TAG, "projection parse failed: $e")
            emptyList()
        }
    }

    override fun onDestroy() {}

    override fun getCount(): Int = rows.size

    override fun getViewAt(position: Int): RemoteViews {
        val view = RemoteViews(context.packageName, R.layout.widget_item)
        val row = rows.getOrNull(position) ?: return view
        view.setTextViewText(R.id.widget_item_name, row.name)
        // The subtitle TextView collapses itself when blank
        // (layout uses 0-height on empty) — but presets always
        // carry at least a timing part, so this is defensive.
        view.setTextViewText(R.id.widget_item_subtitle, row.subtitle)
        // Fill-in merged into the provider's launcher template.
        val fillIn = Intent().apply {
            putExtra("preset_uuid", row.uuid)
        }
        view.setOnClickFillInIntent(R.id.widget_item_root, fillIn)
        return view
    }

    override fun getLoadingView(): RemoteViews? = null

    override fun getViewTypeCount(): Int = 1

    override fun getItemId(position: Int): Long = position.toLong()

    override fun hasStableIds(): Boolean = true
}
