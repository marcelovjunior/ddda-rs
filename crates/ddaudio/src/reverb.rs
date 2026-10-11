//! AudioReverb 2.7 (`XAudio2CreateReverb`, CLSID_AudioReverb version 7).
//!
//! Not implemented yet: the effect passes the input through (the brief
//! orders reverb last, after the rest is audible and compared).

pub struct Reverb {
    in_channels: u32,
    out_channels: u32,
}

impl Reverb {
    pub fn new(_rate: u32, in_channels: u32, out_channels: u32) -> Self {
        Self {
            in_channels,
            out_channels,
        }
    }
    pub fn set_parameters(&mut self, _bytes: &[u8]) {}
    pub fn process(&mut self, input: &[f32], frames: u32, _enabled: bool) -> Vec<f32> {
        let (i, o) = (self.in_channels as usize, self.out_channels as usize);
        let mut out = vec![0.0; frames as usize * o];
        for f in 0..frames as usize {
            for c in 0..o.min(i) {
                out[f * o + c] = input[f * i + c];
            }
        }
        out
    }
}
