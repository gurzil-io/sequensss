use seq_model::SynthParams;

/// Instrument rendered on the audio thread.
///
/// The engine splits the buffer at event boundaries (sub-block rendering), so
/// `note_on`/`note_off` always take effect at the start of the next `process`.
pub trait Instrument: Send {
    fn note_on(&mut self, key: u8, vel: u8);
    fn note_off(&mut self, key: u8);
    /// Silences everything, quickly (choke, stop).
    fn all_off(&mut self);
    /// Overwrites `out_l`/`out_r` (same length) with the rendered output.
    fn process(&mut self, out_l: &mut [f32], out_r: &mut [f32]);
    /// Updates a synth's parameters without recreating the instrument.
    fn set_synth_params(&mut self, _params: &SynthParams) {}
}
