package com.sequensss.app

import android.os.Bundle
import android.view.KeyEvent
import android.view.WindowManager
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import com.google.androidgamesdk.GameActivity

/** Host for the Rust library (libseq_android.so, entry point `android_main`). */
class MainActivity : GameActivity() {
    companion object {
        init {
            System.loadLibrary("seq_android")
        }
    }

    // Document pickers (registered before onCreate, as the API requires).

    /** A file to copy into the app folder (samples). */
    val openDocument = registerForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        PlatformBridge.onPicked(this, uri)
    }

    /** A document to use where it is (projects). */
    val openInPlace = registerForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        PlatformBridge.onDocument(this, uri)
    }

    /** A new document: the user chooses the folder and the name (Save as…, exports). */
    val createDocument = registerForActivityResult(PlatformBridge.CreateDocument()) { uri ->
        PlatformBridge.onDocument(this, uri)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        // Before super.onCreate: the Rust code starts during onCreate and lists the MIDI ports.
        MidiBridge.init(applicationContext)
        PlatformBridge.init(this)
        super.onCreate(savedInstanceState)
        // A sequencer that is playing must not let the screen go to sleep.
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    }

    override fun onPause() {
        // The app may be killed in the background without warning.
        PlatformBridge.nativeOnBackground()
        super.onPause()
    }

    /** Back goes to the Rust code (the native side would otherwise ignore it). */
    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        if (event.keyCode == KeyEvent.KEYCODE_BACK) {
            if (event.action == KeyEvent.ACTION_UP && !event.isCanceled) PlatformBridge.onBack()
            return true
        }
        return super.dispatchKeyEvent(event)
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) hideSystemBars()
    }

    /** Immersive full screen: system bars hidden, shown again with a swipe. */
    private fun hideSystemBars() {
        WindowCompat.setDecorFitsSystemWindows(window, false)
        val controller = WindowInsetsControllerCompat(window, window.decorView)
        controller.hide(WindowInsetsCompat.Type.systemBars())
        controller.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
    }
}
