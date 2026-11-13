//! I/O: audio (cpal), MIDI (midir), samples, project files, exports.

#[cfg(target_os = "android")]
mod android_jni;
pub mod audio;
pub mod export;
/// Android services: file picker, text input, sharing, web links.
#[cfg(target_os = "android")]
#[path = "platform_android.rs"]
pub mod platform;
pub mod instruments;
pub mod loader;
pub mod midi;
pub mod project_file;
pub mod samples;

use seq_i18n::trf;

#[derive(Debug)]
pub enum IoError {
    Audio(String),
    Midi(String),
    File(std::io::Error),
    Wav(hound::Error),
    Ron(String),
    Other(String),
}

// Display written by hand (not via `thiserror`): the text follows the current language.
impl std::fmt::Display for IoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            IoError::Audio(e) => trf!("audio : {e}", "audio: {e}"),
            IoError::Midi(e) => trf!("MIDI : {e}", "MIDI: {e}"),
            IoError::File(e) => trf!("fichier : {e}", "file: {e}"),
            IoError::Wav(e) => trf!("WAV : {e}", "WAV: {e}"),
            IoError::Ron(e) => trf!("projet : {e}", "project: {e}"),
            IoError::Other(e) => return f.write_str(e),
        };
        f.write_str(&s)
    }
}

impl std::error::Error for IoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            IoError::File(e) => Some(e),
            IoError::Wav(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for IoError {
    fn from(e: std::io::Error) -> Self {
        IoError::File(e)
    }
}

impl From<hound::Error> for IoError {
    fn from(e: hound::Error) -> Self {
        IoError::Wav(e)
    }
}

pub type Result<T> = std::result::Result<T, IoError>;
