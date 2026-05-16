// AppWidgetProvider for the starred-preset home-screen widget
// (W-series). Re-renders the widget chrome on add / resize /
// system update: a titled card whose body is a collection
// (`R.id.widget_list`) backed by MeditateWidgetService's factory.
//
// Tap deep-link (W-3): a collection can't give each row its own
// PendingIntent, so the Android pattern is one *template*
// PendingIntent set on the list plus a per-row *fill-in* Intent
// (set in the factory) that is merged into the template at click
// time. The template launches the app's launcher activity (the
// Slint NativeActivity, resolved via PackageManager so no
// hard-coded generated class name — same trick as
// MeditateSessionService.buildContentIntent); the fill-in carries
// `preset_uuid`, which `android_main` reads on cold start.
//
// FLAG_MUTABLE is mandatory on API 31+ for a template whose
// fill-in must actually merge; below 31 the flag doesn't exist
// and 0 is the correct value (mutable was the pre-31 default).

package io.github.janekbt.Meditate

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.widget.RemoteViews

class MeditateWidgetProvider : AppWidgetProvider() {

    override fun onUpdate(
        context: Context,
        manager: AppWidgetManager,
        widgetIds: IntArray,
    ) {
        for (id in widgetIds) {
            manager.updateAppWidget(id, buildRemoteViews(context, id))
        }
        // Re-render done; also force a data reload in case the
        // projection changed while no instance existed to be poked.
        manager.notifyAppWidgetViewDataChanged(widgetIds, R.id.widget_list)
    }

    private fun buildRemoteViews(context: Context, widgetId: Int): RemoteViews {
        val views = RemoteViews(context.packageName, R.layout.widget_root)

        // Each widget instance gets its own adapter; a per-id data
        // URI keeps the platform from recycling one factory across
        // instances (the documented RemoteViewsService idiom).
        val svc = Intent(context, MeditateWidgetService::class.java).apply {
            putExtra(AppWidgetManager.EXTRA_APPWIDGET_ID, widgetId)
            data = Uri.parse(toUri(Intent.URI_INTENT_SCHEME))
        }
        views.setRemoteAdapter(R.id.widget_list, svc)
        views.setEmptyView(R.id.widget_list, R.id.widget_empty)

        // Launcher intent for the Slint NativeActivity. No
        // SINGLE_TOP/CLEAR_TOP: the deep-link is honored at
        // process start (android_main reads getIntent()); a
        // warm process is just foregrounded (W-3 documents this).
        val launch = context.packageManager
            .getLaunchIntentForPackage(context.packageName)
            ?: Intent()
        val mutableFlag =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S)
                PendingIntent.FLAG_MUTABLE
            else 0
        val template = PendingIntent.getActivity(
            context,
            0,
            launch,
            PendingIntent.FLAG_UPDATE_CURRENT or mutableFlag,
        )
        views.setPendingIntentTemplate(R.id.widget_list, template)
        return views
    }
}
