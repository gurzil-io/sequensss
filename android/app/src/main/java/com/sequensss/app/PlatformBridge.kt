package com.sequensss.app

import android.app.AlertDialog
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import android.text.InputType
import android.util.Log
import android.view.inputmethod.EditorInfo
import android.widget.EditText
import android.widget.FrameLayout
import androidx.core.content.FileProvider
import java.io.File
import java.lang.ref.WeakReference
import java.util.concurrent.ConcurrentLinkedQueue

/**
 * Android services for the Rust code (crates/seq-io/src/platform_android.rs):
 * file picker, text input dialog, sharing.
 *
 * Operations are asynchronous: the result goes into a queue that Rust reads every
 * frame (`takeResult`), as "kind\nvalue" ("kind\n" alone = cancelled).
 */
object PlatformBridge {
    private const val TAG = "sequensss"
    private var activity: WeakReference<MainActivity>? = null
    private val results = ConcurrentLinkedQueue<String>()

    /** Kind of the pending pick (one at a time) and its destination subfolder. */
    @Volatile private var pickKind = ""
    @Volatile private var pickDir = ""

    fun init(a: MainActivity) {
        activity = WeakReference(a)
    }

    @JvmStatic fun takeResult(): String? = results.poll()

    private fun post(kind: String, value: String?) {
        results.add(kind + "\n" + (value ?: ""))
    }

    /**
     * Opens the Android picker. The chosen file is copied into `<files>/<subdir>/`
     * and its path is returned.
     */
    @JvmStatic fun pick(kind: String, subdir: String, mimes: Array<String>) {
        val a = activity?.get() ?: return post(kind, null)
        pickKind = kind
        pickDir = subdir
        a.runOnUiThread {
            try {
                a.openDocument.launch(mimes)
            } catch (e: Exception) {
                Log.w(TAG, "picker: $e")
                post(kind, null)
            }
        }
    }

    /** Called by MainActivity with the chosen document (null = cancelled). */
    fun onPicked(a: MainActivity, uri: Uri?) {
        val kind = pickKind
        if (uri == null) return post(kind, null)
        val dir = File(a.getExternalFilesDir(null), pickDir).apply { mkdirs() }
        // Copy off the main thread (a WAV can be large).
        Thread {
            try {
                val dest = uniqueFile(dir, displayName(a, uri))
                a.contentResolver.openInputStream(uri).use { input ->
                    dest.outputStream().use { out -> input!!.copyTo(out) }
                }
                post(kind, dest.absolutePath)
            } catch (e: Exception) {
                Log.w(TAG, "copy: $e")
                post(kind, null)
            }
        }.start()
    }

    private fun displayName(a: MainActivity, uri: Uri): String {
        var name: String? = null
        a.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
            if (c.moveToFirst()) name = c.getString(0)
        }
        return (name ?: "file").replace(Regex("[/\\\\:*?\"<>|]"), "_")
    }

    private fun uniqueFile(dir: File, name: String): File {
        var f = File(dir, name)
        if (!f.exists()) return f
        val base = name.substringBeforeLast('.')
        val ext = name.substringAfterLast('.', "")
        var i = 2
        while (f.exists()) {
            f = File(dir, if (ext.isEmpty()) "$base ($i)" else "$base ($i).$ext")
            i++
        }
        return f
    }

    /** Native text input dialog (the soft keyboard cannot type into egui). */
    @JvmStatic fun prompt(kind: String, title: String, initial: String, ok: String, cancel: String) {
        val a = activity?.get() ?: return post(kind, null)
        a.runOnUiThread {
            val input = EditText(a).apply {
                setText(initial)
                setSelection(initial.length)
                inputType = InputType.TYPE_CLASS_TEXT
                isSingleLine = true
            }
            val pad = (20 * a.resources.displayMetrics.density).toInt()
            val frame = FrameLayout(a).apply {
                setPadding(pad, pad / 2, pad, 0)
                addView(input)
            }
            val dialog = AlertDialog.Builder(a)
                .setTitle(title)
                .setView(frame)
                .setPositiveButton(ok) { _, _ -> post(kind, input.text.toString()) }
                .setNegativeButton(cancel) { _, _ -> post(kind, null) }
                .setOnCancelListener { post(kind, null) }
                .create()
            // The keyboard's "✓ / OK" key confirms (in landscape the keyboard hides the buttons).
            input.imeOptions = EditorInfo.IME_ACTION_DONE
            input.setOnEditorActionListener { _, actionId, _ ->
                if (actionId == EditorInfo.IME_ACTION_DONE) {
                    post(kind, input.text.toString())
                    dialog.dismiss()
                    true
                } else {
                    false
                }
            }
            dialog.show()
            input.requestFocus()
        }
    }

    /** Android share sheet for one of the app's files. */
    @JvmStatic fun share(path: String, mime: String, title: String) {
        val a = activity?.get() ?: return
        a.runOnUiThread {
            try {
                val uri = FileProvider.getUriForFile(a, a.packageName + ".files", File(path))
                val send = Intent(Intent.ACTION_SEND).apply {
                    type = mime
                    putExtra(Intent.EXTRA_STREAM, uri)
                    addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                }
                a.startActivity(Intent.createChooser(send, title))
            } catch (e: Exception) {
                Log.w(TAG, "share: $e")
            }
        }
    }
}
