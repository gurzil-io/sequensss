//! JNI access shared by the Kotlin bridges (MIDI, platform).

use jni::objects::{GlobalRef, JObject, JObjectArray, JString, JValue};
use jni::{JNIEnv, JavaVM};
use std::sync::{Mutex, OnceLock};

static VM: OnceLock<JavaVM> = OnceLock::new();
/// App classes already loaded (only a handful: no need for a hash map).
static CLASSES: Mutex<Vec<(&'static str, GlobalRef)>> = Mutex::new(Vec::new());

/// Runs `f` with an attached `JNIEnv` and the class `class_name` (e.g. "com.sequensss.app.MidiBridge"),
/// inside a local reference frame (no leaks on permanently attached threads).
/// A Java exception is cleared and turned into an error.
pub(crate) fn with_class<T>(
    class_name: &'static str,
    f: impl FnOnce(&mut JNIEnv, &GlobalRef) -> jni::errors::Result<T>,
) -> Result<T, String> {
    let vm = match VM.get() {
        Some(vm) => vm,
        None => {
            let ctx = ndk_context::android_context();
            let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }.map_err(|e| e.to_string())?;
            VM.get_or_init(|| vm)
        }
    };
    let mut env = vm.attach_current_thread_permanently().map_err(|e| e.to_string())?;
    let res = env.with_local_frame(16, |env| -> jni::errors::Result<T> {
        let cached = CLASSES.lock().ok().and_then(|c| c.iter().find(|(n, _)| *n == class_name).map(|(_, g)| g.clone()));
        let class = match cached {
            Some(c) => c,
            None => {
                // FindClass from a native thread cannot see the app's classes:
                // go through the activity's ClassLoader instead.
                let activity = unsafe { JObject::from_raw(ndk_context::android_context().context().cast()) };
                let loader = env.call_method(&activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?.l()?;
                let name = env.new_string(class_name)?;
                let class = env
                    .call_method(&loader, "loadClass", "(Ljava/lang/String;)Ljava/lang/Class;", &[JValue::Object(&name)])?
                    .l()?;
                let global = env.new_global_ref(class)?;
                if let Ok(mut c) = CLASSES.lock() {
                    c.push((class_name, global.clone()));
                }
                global
            }
        };
        f(env, &class)
    });
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    res.map_err(|e| e.to_string())
}

pub(crate) fn string_array(env: &mut JNIEnv, arr: JObject) -> jni::errors::Result<Vec<String>> {
    let arr = JObjectArray::from(arr);
    let n = env.get_array_length(&arr)?;
    let mut out = Vec::with_capacity(n as usize);
    for i in 0..n {
        let s = JString::from(env.get_object_array_element(&arr, i)?);
        out.push(env.get_string(&s)?.into());
    }
    Ok(out)
}

/// A nullable Java string: `None` if null.
pub(crate) fn opt_string(env: &mut JNIEnv, obj: JObject) -> jni::errors::Result<Option<String>> {
    if obj.is_null() {
        return Ok(None);
    }
    Ok(Some(env.get_string(&JString::from(obj))?.into()))
}
