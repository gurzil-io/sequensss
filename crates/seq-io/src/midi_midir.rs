//! Desktop MIDI backend: midir (WinMM on Windows, ALSA on Linux, CoreMIDI on macOS).

use super::{MidiInConn, MidiInTx, push_in};
use crate::{IoError, Result};
use midir::{MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use seq_i18n::trf;

pub fn list_in_ports() -> Vec<String> {
    let Ok(mi) = MidiInput::new("sequensss-list") else { return Vec::new() };
    mi.ports().iter().filter_map(|p| mi.port_name(p).ok()).collect()
}

pub fn list_out_ports() -> Vec<String> {
    let Ok(mo) = MidiOutput::new("sequensss-list") else { return Vec::new() };
    mo.ports().iter().filter_map(|p| mo.port_name(p).ok()).collect()
}

/// midir has no hot-plug notifications: the list is refreshed manually.
pub fn devices_generation() -> u64 {
    0
}

pub struct InConn(MidiInputConnection<()>);

impl InConn {
    pub fn close(self) {
        self.0.close();
    }
}

pub fn connect_input(name: &str, tx: MidiInTx) -> Result<MidiInConn> {
    let mi = MidiInput::new("sequensss-in").map_err(|e| IoError::Midi(e.to_string()))?;
    let port = mi
        .ports()
        .into_iter()
        .find(|p| mi.port_name(p).map(|n| n == name).unwrap_or(false))
        .ok_or_else(|| IoError::Midi(trf!("port introuvable : {name}", "port not found: {name}")))?;
    let conn = mi
        .connect(&port, "sequensss-in", move |_stamp, msg, _| push_in(&tx, msg), ())
        .map_err(|e| IoError::Midi(trf!(
            "impossible d'ouvrir {name} (déjà utilisé par une autre appli ?) : {e}",
            "cannot open {name} (already used by another app?): {e}"
        )))?;
    Ok(MidiInConn { conn: InConn(conn), port: name.to_string() })
}

pub struct OutConn(MidiOutputConnection);

impl OutConn {
    pub fn send(&mut self, bytes: &[u8]) -> std::result::Result<(), ()> {
        self.0.send(bytes).map_err(|_| ())
    }
}

pub fn open_out(name: &str) -> Result<OutConn> {
    let mo = MidiOutput::new("sequensss-out").map_err(|e| IoError::Midi(e.to_string()))?;
    let port = mo
        .ports()
        .into_iter()
        .find(|p| mo.port_name(p).map(|n| n == name).unwrap_or(false))
        .ok_or_else(|| IoError::Midi(trf!("port introuvable : {name}", "port not found: {name}")))?;
    mo.connect(&port, "sequensss-out")
        .map(OutConn)
        .map_err(|e| IoError::Midi(trf!(
            "impossible d'ouvrir {name} (déjà utilisé par une autre appli ?) : {e}",
            "cannot open {name} (already used by another app?): {e}"
        )))
}
