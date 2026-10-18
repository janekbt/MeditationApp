// AppWidgetProvider for the starred-preset home-screen widget
// (W-series). Re-renders the widget chrome on add / resize /
// system update: a titled card whose body is a collection
// (`R.id.widget_list`) backed by MeditateWidgetService's factory.
//
// Tap deep-link (W-3/W-4). A collection can't give each row its
// own PendingIntent, so the Android pattern is one *template*
// PendingIntent on the list + a per-row *fill-in* Intent (set in
// the factory) merged into it at click time. The template is a
// *broadcast* to MeditateWidgetLaunchReceiver (not exported, so
// no other app can start a session), not a direct activity
// launch: NativeActivity never forwards onNewIntent to native
// code, so a warm process could never read the tapped uuid off
// an Intent. Instead the receiver writes the uuid to the drop file
// the Rust side polls (`<filesDir>/meditate/widget_launch`) and
// then foregrounds the app — one channel that works whether the
// app was dead or already running.
//
// startActivity from a receiver is normally blocked by the
// background-activity-start rules, but a PendingIntent the system
// fires because the user tapped a widget grants a short BAL
// window, so this launch is allowed.
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

        // Broadcast template to the launch receiver; the row's
        // fill-in (set in the factory) supplies preset_uuid.
        val tap = Intent(context, MeditateWidgetLaunchReceiver::class.java)
        val mutableFlag =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S)
                PendingIntent.FLAG_MUTABLE
            else 0
        val template = PendingIntent.getBroadcast(
            context,
            0,
            tap,
            PendingIntent.FLAG_UPDATE_CURRENT or mutableFlag,
        )
        views.setPendingIntentTemplate(R.id.widget_list, template)

        // Title → just open the app (no preset, no autostart):
        // a direct activity launch, distinct request code so it
        // doesn't collide with the broadcast template, IMMUTABLE
        // since nothing fills it in.
        val openApp = context.packageManager
            .getLaunchIntentForPackage(context.packageName)
            ?.apply { addFlags(Intent.FLAG_ACTIVITY_NEW_TASK) }
            ?: Intent()
        val openPi = PendingIntent.getActivity(
            context,
            1,
            openApp,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        views.setOnClickPendingIntent(R.id.widget_title, openPi)
        return views
    }
}
