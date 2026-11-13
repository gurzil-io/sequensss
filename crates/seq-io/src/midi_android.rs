//! Android MIDI backend: JNI bridge to `com.sequensss.app.MidiBridge` (Kotlin, `MidiManager`).
//!
//! - Port names are built on the Kotlin side ("device" or "device · port").
//! - MIDI in: Kotlin calls `MidiBridge.nativeOnMidi(handle, bytes…)` from its MIDI thread;
//!   `handle` points to an `InState` (producer to the engine + stream parser).
//! - MIDI out: `send` is called by the MIDI out thread (not the audio thread) when the
//!   message is due. No future timestamp is passed to `MidiInputPort.send`: honouring it is
//!   left to each device's service (USB, BLE, virtual), with no guarantee on precision.
//! - Hot-plug: `devices_generation()` changes each time a device is added or removed.

use super::{MidiInConn, MidiInTx, MidiParser, push_in};
use crate::android_jni::{opt_string, string_array, with_class};
use crate::{IoError, Result};
use jni::objects::{GlobalRef, JByteArray, JObject, JValue};
use jni::sys::{jint, jlong};
use jni::JNIEnv;
use seq_i18n::trf;
use std::sync::Mutex;

const BRIDGE_CLASS: &str = "com.sequensss.app.MidiBridge";

fn with_bridge<T>(f: impl FnOnce(&mut JNIEnv, &GlobalRef) -> jni::errors::Result<T>) -> Result<T> {
    with_class(BRIDGE_CLASS, f).map_err(|e| IoError::Midi(trf!("JNI : {e}", "JNI: {e}")))
}

fn list(method: &str) -> Vec<String> {
    with_bridge(|env, c| {
        let arr = env.call_static_method(c, method, "()[Ljava/lang/String;", &[])?.l()?;
        string_array(env, arr)
    })
    .unwrap_or_default()
}

/// Ports we receive from (the devices' output ports).
pub fn list_in_ports() -> Vec<String> {
    list("listSources")
}

/// Ports we send to (the devices' input ports).
pub fn list_out_ports() -> Vec<String> {
    list("listDestinations")
}

pub fn devices_generation() -> u64 {
    with_bridge(|env, c| env.call_static_method(c, "generation", "()J", &[])?.j()).unwrap_or(0) as u64
}

/// State of an input, shared with the Kotlin callback through a pointer (`handle`).
struct InState {
    tx: MidiInTx,
    parser: Mutex<MidiParser>,
}

pub struct InConn {
    handle: jlong,
}

impl InConn {
    pub fn close(self) {
        // Kotlin guarantees no callback is running or will follow after closeSource.
        let _ = with_bridge(|env, c| env.call_static_method(c, "closeSource", "(J)V", &[JValue::Long(self.handle)]).map(|_| ()));
        drop(unsafe { Box::from_raw(self.handle as *mut InState) });
    }
}

pub fn connect_input(name: &str, tx: MidiInTx) -> Result<MidiInConn> {
    let state = Box::new(InState { tx, parser: Mutex::new(MidiParser::default()) });
    let handle = Box::into_raw(state) as jlong;
    let err = with_bridge(|env, c| {
        let jname = env.new_string(name)?;
        let r = env
            .call_static_method(c, "openSource", "(Ljava/lang/String;J)Ljava/lang/String;", &[
                JValue::Object(&jname),
                JValue::Long(handle),
            ])?
            .l()?;
        opt_string(env, r)
    });
    match err {
        Ok(None) => Ok(MidiInConn { conn: InConn { handle }, port: name.to_string() }),
        Ok(Some(e)) => {
            drop(unsafe { Box::from_raw(handle as *mut InState) });
            Err(IoError::Midi(trf!("{name} : {e}", "{name}: {e}")))
        }
        Err(e) => {
            drop(unsafe { Box::from_raw(handle as *mut InState) });
            Err(e)
        }
    }
}

/// Called by `MidiBridge` (Kotlin) for each received packet.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_sequensss_app_MidiBridge_nativeOnMidi(
    env: JNIEnv,
    _class: JObject,
    handle: jlong,
    data: JByteArray,
    offset: jint,
    count: jint,
) {
    if handle == 0 || count <= 0 {
        return;
    }
    let state = unsafe { &*(handle as *const InState) };
    let mut buf = [0i8; 256];
    let n = (count as usize).min(buf.len());
    if env.get_byte_array_region(&data, offset, &mut buf[..n]).is_err() {
        return;
    }
    let bytes: &[u8] = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast(), n) };
    if let Ok(mut p) = state.parser.lock() {
        p.feed(bytes, |m| push_in(&state.tx, m));
    }
}

pub struct OutConn {
    id: jint,
}

impl OutConn {
    pub fn send(&mut self, bytes: &[u8]) -> std::result::Result<(), ()> {
        with_bridge(|env, c| {
            let arr = env.byte_array_from_slice(bytes)?;
            env.call_static_method(c, "send", "(I[B)Z", &[JValue::Int(self.id), JValue::Object(&arr)])?.z()
        })
        .ok()
        .filter(|&ok| ok)
        .map(|_| ())
        .ok_or(())
    }
}

impl Drop for OutConn {
    fn drop(&mut self) {
        let id = self.id;
        let _ = with_bridge(|env, c| env.call_static_method(c, "closeDestination", "(I)V", &[JValue::Int(id)]).map(|_| ()));
    }
}

pub fn open_out(name: &str) -> Result<OutConn> {
    let id = with_bridge(|env, c| {
        let jname = env.new_string(name)?;
        env.call_static_method(c, "openDestination", "(Ljava/lang/String;)I", &[JValue::Object(&jname)])?.i()
    })?;
    if id < 0 {
        return Err(IoError::Midi(trf!("impossible d'ouvrir {name}", "cannot open {name}")));
    }
    Ok(OutConn { id })
}
