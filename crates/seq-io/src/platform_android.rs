//! Android services (JNI bridge to `com.sequensss.app.PlatformBridge`): file picker,
//! native text input dialog, "Share" sheet, web links.
//!
//! Everything is asynchronous: start the operation, then read the results every frame with
//! [`take_result`].

use crate::android_jni::{opt_string, with_class};
use jni::objects::{JObject, JValue};
use std::path::Path;

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
