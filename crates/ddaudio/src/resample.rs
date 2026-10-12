//! Sample-rate conversion, continuous across buffers, loops and passes.
//!
//! Linear interpolation between consecutive input frames, the output one
//! input frame behind the read position (so a pass never needs a frame
//! the runtime's clock has not consumed yet). At a step of exactly 1
//! (source rate equal to the destination rate, frequency ratio 1) no
//! conversion runs and the input is copied unchanged and undelayed: "SRC
//! only occurs when necessary" ("XAudio2 Sample Rate Conversions").
//! Both paths are bit-identical to the host's XAudio2 2.9 in the oracle
//! (tests/oracle.rs: ratios 0.5 to 2 and ratio 1). What 2.9 does at the
//! pass where a voice switches between the two paths is not compared.

#[derive(Default)]
pub struct Resampler {
    /// Consumed frames not yet fully read, interleaved; `buf[0]` is
    /// absolute frame `first`.
    buf: Vec<f32>,
    first: u64,
    /// Absolute input position of the next output frame.
    pos: f64,
}

impl Resampler {
    pub fn push(&mut self, frames: &[f32]) {
        self.buf.extend_from_slice(frames);
    }

    /// Produces `frames` output frames reading the input at `step` input
    /// frames per output frame. Missing input (the voice ran dry) renders
    /// silence and does not advance the read position past the data.
    /// Returns the samples and whether any input contributed.
    pub fn render(&mut self, channels: u32, step: f64, frames: usize) -> (Vec<f32>, bool) {
        let ch = channels.max(1) as usize;
        let mut out = vec![0.0f32; frames * ch];
        let available = self.first + (self.buf.len() / ch) as u64;
        let mut produced = frames;
        let bypass = step == 1.0 && self.pos.fract() == 0.0;
        for k in 0..frames {
            if bypass {
                let i = self.pos as u64 + k as u64;
                if i >= available {
                    produced = k;
                    break;
                }
                let at = (i - self.first) as usize * ch;
                out[k * ch..][..ch].copy_from_slice(&self.buf[at..at + ch]);
                continue;
            }
            let t = self.pos + k as f64 * step;
            let i = t.floor() as u64;
            if i >= available {
                produced = k;
                break;
            }
            let f = (t - i as f64) as f32;
            let cur = &self.buf[(i - self.first) as usize * ch..][..ch];
            let o = &mut out[k * ch..][..ch];
            if i == self.first {
                // The frame before the oldest retained one is the history
                // dropped below; at the very start it is silence.
                for (y, x) in o.iter_mut().zip(cur) {
                    *y = x * f;
                }
                continue;
            }
            let prev = &self.buf[(i - 1 - self.first) as usize * ch..][..ch];
            for ((y, a), b) in o.iter_mut().zip(prev).zip(cur) {
                *y = if f == 0.0 { *a } else { a + (b - a) * f };
            }
        }
        self.pos = if produced == frames {
            self.pos + frames as f64 * step
        } else {
            self.pos + produced as f64 * step
        };
        // Keep the frame before the read position as interpolation history.
        let last = available.saturating_sub(1).max(self.first);
        let keep_from = (self.pos.floor() as u64)
            .saturating_sub(1)
            .clamp(self.first, last);
        let drop = (keep_from - self.first) as usize * ch;
        self.buf.drain(..drop);
        self.first = keep_from;
        (out, produced > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_step_copies() {
        let mut r = Resampler::default();
        let input: Vec<f32> = (0..960).map(|i| i as f32).collect();
        r.push(&input[..480]);
        let (a, valid) = r.render(1, 1.0, 480);
        assert!(valid);
        assert_eq!(&a[..], &input[..480]);
        r.push(&input[480..]);
        let (b, _) = r.render(1, 1.0, 480);
        assert_eq!(&b[..], &input[480..]);
    }

    #[test]
    fn half_step_interpolates_and_runs_dry_without_skipping() {
        let mut r = Resampler::default();
        r.push(&[0.0, 2.0, 4.0, 6.0]);
        // positions 0, .5, 1, 1.5, ...: outputs lag one frame
        let (a, _) = r.render(1, 0.5, 8);
        assert_eq!(a, vec![0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
        let (b, valid) = r.render(1, 0.5, 2);
        assert!(!valid);
        assert_eq!(b, vec![0.0, 0.0]);
        // New data continues where the old ran out.
        r.push(&[8.0]);
        let (c, _) = r.render(1, 0.5, 2);
        assert_eq!(c, vec![6.0, 7.0]);
    }
}
