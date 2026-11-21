use seq_model::SynthParams;

/// Instrument rendered on the audio thread.
///
/// The engine splits the buffer at event boundaries (sub-block rendering), so
/// `note_on`/`note_off` always take effect at the start of the next `process`.
pub trait Instrument: Send {
    fn note_on(&mut self, key: u8, vel: u8);
    /// Note with its own pan, -1.0 (left) ..= 1.0 (right), on top of the channel pan.
    /// Instruments without per-voice pan play it centred.
    fn note_on_panned(&mut self, key: u8, vel: u8, _pan: f32) {
        self.note_on(key, vel);
    }
    fn note_off(&mut self, key: u8);
    /// Silences everything, quickly (choke, stop).
    fn all_off(&mut self);
    /// Overwrites `out_l`/`out_r` (same length) with the rendered output.
    fn process(&mut self, out_l: &mut [f32], out_r: &mut [f32]);
    /// Updates a synth's parameters without recreating the instrument.
    fn set_synth_params(&mut self, _params: &SynthParams) {}
}

/// Left / right gains of a voice panned to `pan` (-1.0 ..= 1.0): a balance law, unity at
/// the centre (a centred note sounds exactly as without note pan), the far side fading out.
pub(crate) fn voice_gains(pan: f32) -> (f32, f32) {
    let p = pan.clamp(-1.0, 1.0);
    ((1.0 - p).min(1.0), (1.0 + p).min(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_gains_balance() {
        assert_eq!(voice_gains(0.0), (1.0, 1.0));
        assert_eq!(voice_gains(-1.0), (1.0, 0.0));
        assert_eq!(voice_gains(1.0), (0.0, 1.0));
        assert_eq!(voice_gains(0.25), (0.75, 1.0));
        assert_eq!(voice_gains(-3.0), (1.0, 0.0));
    }
}
