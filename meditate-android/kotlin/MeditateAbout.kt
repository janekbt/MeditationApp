// About + diagnostics helpers (parity: GTK's AdwAboutDialog with
// its Troubleshooting/debug-info view). Small platform verbs the
// Rust side can't reach directly: the real installed versionName,
// clipboard writes, the share sheet, and opening a URL in the
// browser. Driven via the app-classloader JNI bridge
// (src/about.rs), same pattern as MeditateScreen / MeditateKeychain.

package io.github.janekbt.Meditate

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.util.Log

object MeditateAbout {
    private const val TAG = "MeditateAbout"

    /// The installed APK's versionName — the one source of truth
    /// (build.gradle), not a hardcoded mirror that can drift.
    @JvmStatic
    fun versionName(context: Context): String = try {
        context.packageManager
            .getPackageInfo(context.packageName, 0)
            .versionName ?: "?"
    } catch (e: Exception) {
        Log.w(TAG, "versionName failed: $e")
        "?"
    }

    /// Full BCP-47 tag ("de-DE", "pt-BR", …) for bundled-
    /// translation selection (P8 i18n) — regional variants like
    /// pt-BR/zh-CN need the region to pick the right bundle.
    @JvmStatic
    fun localeTag(): String =
        java.util.Locale.getDefault().toLanguageTag() ?: "en"

    /// "24" when the system uses 24-hour time, else
    /// "12|<AM>|<PM>" with the locale's day-period markers —
    /// the Rust side renders log-card times accordingly.
    @JvmStatic
    fun timeFormat(context: Context): String = try {
        if (android.text.format.DateFormat.is24HourFormat(context)) {
            "24"
        } else {
            val sym = java.text.DateFormatSymbols.getInstance()
            "12|" + sym.amPmStrings[0] + "|" + sym.amPmStrings[1]
        }
    } catch (e: Exception) {
        Log.w(TAG, "timeFormat failed: $e")
        "24"
    }

    /// The locale's digit-grouping separator ("." for de,
    /// "\u202F" for fr, "," for en …) — Rust groups large
    /// stat numbers with it.
    @JvmStatic
    fun groupingSeparator(): String = try {
        java.text.DecimalFormatSymbols.getInstance()
            .groupingSeparator.toString()
    } catch (e: Exception) {
        Log.w(TAG, "groupingSeparator failed: $e")
        ""
    }

    @JvmStatic
    fun copyText(context: Context, label: String, text: String) {
        try {
            val cm = context.getSystemService(Context.CLIPBOARD_SERVICE)
                as ClipboardManager
            cm.setPrimaryClip(ClipData.newPlainText(label, text))
        } catch (e: Exception) {
            Log.w(TAG, "copyText failed: $e")
        }
    }

    @JvmStatic
    fun shareText(context: Context, subject: String, text: String) {
        try {
            val send = Intent(Intent.ACTION_SEND).apply {
                type = "text/plain"
                putExtra(Intent.EXTRA_SUBJECT, subject)
                putExtra(Intent.EXTRA_TEXT, text)
            }
            context.startActivity(
                Intent.createChooser(send, subject)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            )
        } catch (e: Exception) {
            Log.w(TAG, "shareText failed: $e")
        }
    }

    @JvmStatic
    fun openUrl(context: Context, url: String) {
        try {
            context.startActivity(
                Intent(Intent.ACTION_VIEW, Uri.parse(url))
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            )
        } catch (e: Exception) {
            Log.w(TAG, "openUrl failed: $e")
        }
    }
}
