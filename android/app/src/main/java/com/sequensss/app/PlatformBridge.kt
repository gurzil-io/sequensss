package com.sequensss.app

import android.app.Activity
import android.app.AlertDialog
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import android.text.InputType
import android.util.Log
import android.view.inputmethod.EditorInfo
import android.widget.EditText
import android.widget.FrameLayout
import androidx.activity.result.contract.ActivityResultContract
import androidx.core.content.FileProvider
import java.io.File
import java.lang.ref.WeakReference
import java.util.concurrent.ConcurrentLinkedQueue

/**
 * Android services for the Rust code (crates/seq-io/src/platform_android.rs):
 * file picker, documents (Storage Access Framework), text input dialog, sharing, web links,
 * back button, leaving the app, going to the background.
 *
 * Dialogs are asynchronous: the result goes into a queue that Rust reads every
 * frame (`takeResult`), as "kind\nvalue" ("kind\n" alone = cancelled). Documents are read
 * and written synchronously, on the Rust thread that asks.
 */
object PlatformBridge {
    private const val TAG = "sequensss"
    private var activity: WeakReference<MainActivity>? = null
    private val results = ConcurrentLinkedQueue<String>()

    /** Kind of the pending pick (one at a time) and its destination subfolder. */
    @Volatile private var pickKind = ""
    @Volatile private var pickDir = ""

    /** Kind of the pending document picker (one at a time), and whether to keep its permission. */
    @Volatile private var docKind = ""
    @Volatile private var docKeep = false

    /** Persisted document permissions kept at most (Android caps them; the oldest go first). */
    private const val MAX_KEPT = 100

    private var appContext: Context? = null

    fun init(a: MainActivity) {
        activity = WeakReference(a)
        appContext = a.applicationContext
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

    /** ACTION_CREATE_DOCUMENT with a MIME type chosen per call: input = (suggested name, MIME). */
    class CreateDocument : ActivityResultContract<Pair<String, String>, Uri?>() {
        override fun createIntent(context: Context, input: Pair<String, String>): Intent =
            Intent(Intent.ACTION_CREATE_DOCUMENT)
                .addCategory(Intent.CATEGORY_OPENABLE)
                .setType(input.second)
                .putExtra(Intent.EXTRA_TITLE, input.first)

        override fun parseResult(resultCode: Int, intent: Intent?): Uri? =
            if (resultCode == Activity.RESULT_OK) intent?.data else null
    }

    /** Picker for a document to use where it is; its URI is returned. */
    @JvmStatic fun openDocument(kind: String, mimes: Array<String>, keep: Boolean) {
        val a = activity?.get() ?: return post(kind, null)
        docKind = kind
        docKeep = keep
        a.runOnUiThread {
            try {
                a.openInPlace.launch(mimes)
            } catch (e: Exception) {
                Log.w(TAG, "openDocument: $e")
                post(kind, null)
            }
        }
    }

    /** Picker for a new document (folder and name chosen by the user); its URI is returned. */
    @JvmStatic fun createDocument(kind: String, name: String, mime: String, keep: Boolean) {
        val a = activity?.get() ?: return post(kind, null)
        docKind = kind
        docKeep = keep
        a.runOnUiThread {
            try {
                a.createDocument.launch(Pair(name, mime))
            } catch (e: Exception) {
                Log.w(TAG, "createDocument: $e")
                post(kind, null)
            }
        }
    }

    /** Called by MainActivity with the chosen document (null = cancelled). */
    fun onDocument(a: MainActivity, uri: Uri?) {
        val kind = docKind
        if (uri == null) return post(kind, null)
        if (docKeep) keepPermission(a, uri)
        post(kind, uri.toString())
    }

    /** Keeps the read / write permission across restarts, so Save can write back later. */
    private fun keepPermission(a: MainActivity, uri: Uri) {
        val cr = a.contentResolver
        val rw = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
        try {
            cr.takePersistableUriPermission(uri, rw)
        } catch (e: SecurityException) {
            // Read-only document: Save will fall back to Save as….
            try {
                cr.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
            } catch (e2: SecurityException) {
                Log.w(TAG, "permission: $e2")
            }
        }
        // Forget the oldest documents beyond the limit.
        val kept = cr.persistedUriPermissions
        if (kept.size > MAX_KEPT) {
            kept.sortedBy { it.persistedTime }.take(kept.size - MAX_KEPT).filter { it.uri != uri }.forEach {
                val flags = (if (it.isReadPermission) Intent.FLAG_GRANT_READ_URI_PERMISSION else 0) or
                    (if (it.isWritePermission) Intent.FLAG_GRANT_WRITE_URI_PERMISSION else 0)
                try {
                    cr.releasePersistableUriPermission(it.uri, flags)
                } catch (e: SecurityException) {
                    Log.w(TAG, "release: $e")
                }
            }
        }
    }

    private fun resolver() = (activity?.get() ?: appContext)?.contentResolver

    /** Display name of a document, if its provider gives one. */
    @JvmStatic fun displayName(uri: String): String? = try {
        resolver()?.query(Uri.parse(uri), arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
            if (c.moveToFirst()) c.getString(0) else null
        }
    } catch (e: Exception) {
        Log.w(TAG, "displayName: $e")
        null
    }

    /** Text of a document: "ok:<text>" or "error:<message>". */
    @JvmStatic fun readText(uri: String): String = try {
        val cr = resolver() ?: throw IllegalStateException("no activity")
        val text = cr.openInputStream(Uri.parse(uri)).use { it!!.readBytes() }.toString(Charsets.UTF_8)
        "ok:$text"
    } catch (e: Exception) {
        Log.w(TAG, "readText: $e")
        "error:" + message(e)
    }

    /** Replaces the contents of a document. Returns null, or the error. */
    @JvmStatic fun writeText(uri: String, text: String): String? = write(uri) { it.write(text.toByteArray(Charsets.UTF_8)) }

    /** Copies one of the app's files into a document. Returns null, or the error. */
    @JvmStatic fun copyToUri(path: String, uri: String): String? = write(uri) { out ->
        File(path).inputStream().use { it.copyTo(out) }
    }

    private fun write(uri: String, body: (java.io.OutputStream) -> Unit): String? = try {
        val cr = resolver() ?: throw IllegalStateException("no activity")
        val u = Uri.parse(uri)
        // "wt" truncates what was there; some providers only know "w".
        val out = try {
            cr.openOutputStream(u, "wt")
        } catch (e: IllegalArgumentException) {
            cr.openOutputStream(u, "w")
        } catch (e: java.io.FileNotFoundException) {
            cr.openOutputStream(u, "w")
        } ?: throw java.io.FileNotFoundException(uri)
        out.use(body)
        null
    } catch (e: Exception) {
        Log.w(TAG, "write: $e")
        message(e)
    }

    private fun message(e: Exception): String = e.localizedMessage ?: e.javaClass.simpleName

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

    /** Back button: Rust closes what is open, asks to save, or calls [leave]. */
    fun onBack() = post("back", "1")

    /** Leaves the app (to the background, like back on the home screen). */
    @JvmStatic fun leave() {
        val a = activity?.get() ?: return
        a.runOnUiThread { a.moveTaskToBack(true) }
    }

    /** The app goes to the background: Rust keeps a copy of the unsaved work. */
    @JvmStatic external fun nativeOnBackground()

    /** Opens a web page (release notes) in the browser. */
    @JvmStatic fun openUrl(url: String) {
        val a = activity?.get() ?: return
        a.runOnUiThread {
            try {
                val view = Intent(Intent.ACTION_VIEW, Uri.parse(url)).apply {
                    addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                }
                a.startActivity(view)
            } catch (e: Exception) {
                Log.w(TAG, "openUrl: $e")
            }
        }
    }
}
