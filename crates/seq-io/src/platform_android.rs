//! Android services (JNI bridge to `com.sequensss.app.PlatformBridge`): file picker,
//! native text input dialog, "Share" sheet, web links, back button, leaving the app,
//! going to the background.
//!
//! Everything is asynchronous: start the operation, then read the results every frame with
//! [`take_result`] (the back button comes as a result of kind `"back"`).

use crate::android_jni::{opt_string, with_class};
use jni::JNIEnv;
use jni::objects::{JClass, JObject, JValue};
use std::path::Path;
use std::sync::Mutex;

/// Run when the activity is paused (see [`on_background`]).
static BACKGROUND: Mutex<Option<Box<dyn Fn() + Send>>> = Mutex::new(None);

const CLASS: &str = "com.sequensss.app.PlatformBridge";

/// Result of an operation: `kind` = label given at launch, `value` = path of the copied file
/// or entered text (`None` = cancelled).
pub struct PlatformResult {
    pub kind: String,
    pub value: Option<String>,
}

fn log_err<T>(r: Result<T, String>) {
    if let Err(e) = r {
        eprintln!("PlatformBridge: {e}");
    }
}

/// Opens the Android picker; the chosen file is copied into `<app directory>/<subdir>/`.
pub fn pick(kind: &str, subdir: &str, mimes: &[&str]) {
    log_err(with_class(CLASS, |env, c| {
        let k = env.new_string(kind)?;
        let d = env.new_string(subdir)?;
        let arr = env.new_object_array(mimes.len() as i32, "java/lang/String", JObject::null())?;
        for (i, m) in mimes.iter().enumerate() {
            let s = env.new_string(m)?;
            env.set_object_array_element(&arr, i as i32, s)?;
        }
        env.call_static_method(
            c,
            "pick",
            "(Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;)V",
            &[JValue::Object(&k), JValue::Object(&d), JValue::Object(&arr)],
        )?;
        Ok(())
    }));
}

/// Native text input dialog (title, initial text, button labels).
pub fn prompt(kind: &str, title: &str, initial: &str, ok: &str, cancel: &str) {
    log_err(with_class(CLASS, |env, c| {
        let args = [kind, title, initial, ok, cancel].map(|s| env.new_string(s));
        let [a, b, i, o, n] = args;
        let (a, b, i, o, n) = (a?, b?, i?, o?, n?);
        env.call_static_method(
            c,
            "prompt",
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
            &[JValue::Object(&a), JValue::Object(&b), JValue::Object(&i), JValue::Object(&o), JValue::Object(&n)],
        )?;
        Ok(())
    }));
}

/// "Share" sheet for a file located in the app's external directory.
pub fn share(path: &Path, mime: &str, title: &str) {
    log_err(with_class(CLASS, |env, c| {
        let p = env.new_string(path.to_string_lossy())?;
        let m = env.new_string(mime)?;
        let t = env.new_string(title)?;
        env.call_static_method(
            c,
            "share",
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
            &[JValue::Object(&p), JValue::Object(&m), JValue::Object(&t)],
        )?;
        Ok(())
    }));
}

/// Opens a web page in the system browser.
pub fn open_url(url: &str) {
    log_err(with_class(CLASS, |env, c| {
        let u = env.new_string(url)?;
        env.call_static_method(c, "openUrl", "(Ljava/lang/String;)V", &[JValue::Object(&u)])?;
        Ok(())
    }));
}

/// Leaves the app (to the background, like the system's back button on the home screen).
pub fn leave() {
    log_err(with_class(CLASS, |env, c| {
        env.call_static_method(c, "leave", "()V", &[])?;
        Ok(())
    }));
}

/// Sets what to do when the app goes to the background (it may be killed there without
/// warning). Runs on the Android main thread, while the app is paused: keep it short.
pub fn on_background(f: impl Fn() + Send + 'static) {
    if let Ok(mut b) = BACKGROUND.lock() {
        *b = Some(Box::new(f));
    }
}

/// Called by `MainActivity.onPause` (through `PlatformBridge`).
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_sequensss_app_PlatformBridge_nativeOnBackground(_env: JNIEnv, _class: JClass) {
    if let Ok(b) = BACKGROUND.lock()
        && let Some(f) = b.as_ref()
    {
        f();
    }
}

/// Next available result (call every frame).
pub fn take_result() -> Option<PlatformResult> {
    let raw = with_class(CLASS, |env, c| {
        let r = env.call_static_method(c, "takeResult", "()Ljava/lang/String;", &[])?.l()?;
        opt_string(env, r)
    })
    .ok()
    .flatten()?;
    let (kind, value) = raw.split_once('\n').unwrap_or((raw.as_str(), ""));
    Some(PlatformResult { kind: kind.to_string(), value: (!value.is_empty()).then(|| value.to_string()) })
}
