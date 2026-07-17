// Nextcloud app-password storage (Phase 7 SY-1) — the Android
// analogue of the GTK shell's oo7/libsecret keyring. The password
// is encrypted with an AES-256-GCM key that lives in the Android
// Keystore (hardware-backed where available, never extractable);
// the ciphertext + IV land in `<filesDir>/meditate/sync_secret.json`
// together with the url + username they belong to, mirroring
// libsecret's attribute-matched lookup: `readPassword` returns null
// when the stored attributes don't match the requested account, so
// a stale secret can't silently authenticate against a different
// server. Single slot — the app supports exactly one Nextcloud
// account, same as GTK.
//
// Driven from Rust (src/keychain.rs) via the app-classloader JNI
// bridge, same pattern as MeditateScreen / MeditateGuided.

package io.github.janekbt.Meditate

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import android.util.Log
import org.json.JSONObject
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

object MeditateKeychain {
    private const val TAG = "MeditateKeychain"
    private const val KEY_ALIAS = "meditate_sync_password"
    private const val FILE_NAME = "sync_secret.json"
    private const val GCM_TAG_BITS = 128

    private val lock = Any()

    @JvmStatic
    fun storePassword(
        context: Context,
        url: String,
        username: String,
        password: String,
    ): Boolean = synchronized(lock) {
        try {
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.ENCRYPT_MODE, obtainKey())
            val ct = cipher.doFinal(password.toByteArray(Charsets.UTF_8))
            val json = JSONObject()
                .put("url", url)
                .put("username", username)
                .put("iv", Base64.encodeToString(cipher.iv, Base64.NO_WRAP))
                .put("ct", Base64.encodeToString(ct, Base64.NO_WRAP))
            secretFile(context).apply {
                parentFile?.mkdirs()
                writeText(json.toString())
            }
            true
        } catch (e: Exception) {
            Log.w(TAG, "storePassword failed: $e")
            false
        }
    }

    // Empty string = "no password stored for this account". Rust
    // maps that to None (JNI null-vs-string plumbing is fussier
    // than an empty-string sentinel; a real app-password is never
    // empty — core's prepare_save rejects empty input).
    @JvmStatic
    fun readPassword(
        context: Context,
        url: String,
        username: String,
    ): String = synchronized(lock) {
        try {
            val f = secretFile(context)
            if (!f.exists()) return ""
            val json = JSONObject(f.readText())
            if (json.optString("url") != url ||
                json.optString("username") != username
            ) {
                return "" // stored secret belongs to another account
            }
            val iv = Base64.decode(json.getString("iv"), Base64.NO_WRAP)
            val ct = Base64.decode(json.getString("ct"), Base64.NO_WRAP)
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(
                Cipher.DECRYPT_MODE,
                obtainKey(),
                GCMParameterSpec(GCM_TAG_BITS, iv),
            )
            String(cipher.doFinal(ct), Charsets.UTF_8)
        } catch (e: Exception) {
            // Key rotated / file corrupt / Keystore hiccup — treat
            // as "no password" so the UI prompts for re-entry
            // (mirrors GTK's KeyringFailed -> re-enter flow).
            Log.w(TAG, "readPassword failed: $e")
            ""
        }
    }

    @JvmStatic
    fun clearPassword(context: Context): Unit = synchronized(lock) {
        runCatching { secretFile(context).delete() }
    }

    private fun secretFile(context: Context) =
        File(File(context.filesDir, "meditate"), FILE_NAME)

    private fun obtainKey(): SecretKey {
        val ks = KeyStore.getInstance("AndroidKeyStore")
        ks.load(null)
        (ks.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }
        val gen = KeyGenerator.getInstance(
            KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore",
        )
        gen.init(
            KeyGenParameterSpec.Builder(
                KEY_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return gen.generateKey()
    }
}
