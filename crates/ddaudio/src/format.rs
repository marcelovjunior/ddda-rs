//! Source formats and their decoders.
//!
//! Layouts: `WAVEFORMATEX` and `ADPCMWAVEFORMAT` / `ADPCMCOEFSET` (SDK
//! `mmreg.h`). The MS ADPCM block layout and adaptation table are the ones
//! Microsoft published in "Multimedia Standards Update: New Multimedia Data
//! Types and Data Techniques" (rev. 3.0, 1994, section "Microsoft ADPCM").
//! The integer-to-float scales are checked bit-exactly against the host's
//! XAudio2 by the oracle (`tests/oracle.rs`).
//!
//! The game's source voices (census in the L19 brief):
//! MS ADPCM mono 48 kHz, 70-byte blocks of 128 samples, the seven standard
//! coefficient pairs (0x00DCE1F0); 16-bit PCM 48 kHz with 1, 2 or 6
//! channels (0x00DCE2E0, 0x00DC7BA0); PCM 48 kHz with a run-time channel
//! count and bit depth (0x00DCE530). Float and the other PCM depths are
//! decoded because the runtime accepts them, not because the game uses them.

pub const WAVE_FORMAT_PCM: u16 = 1;
pub const WAVE_FORMAT_ADPCM: u16 = 2;
pub const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;

#[derive(Clone, Debug, PartialEq)]
pub struct Format {
    pub tag: u16,
    pub channels: u32,
    pub rate: u32,
    pub block_align: u32,
    pub bits: u32,
    /// ADPCM: decoded samples (frames) per block.
    pub samples_per_block: u32,
    /// ADPCM: (iCoef1, iCoef2) pairs.
    pub coefs: Vec<(i16, i16)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FormatError {
    Truncated,
    Unsupported(u16),
    Invalid(&'static str),
}

fn u16_at(b: &[u8], o: usize) -> Result<u16, FormatError> {
    b.get(o..o + 2)
        .map(|x| u16::from_le_bytes([x[0], x[1]]))
        .ok_or(FormatError::Truncated)
}

impl Format {
    /// 16-bit integer PCM.
    pub fn pcm16(channels: u32, rate: u32) -> Self {
        Self::pcm(channels, rate, 16)
    }
    pub fn pcm(channels: u32, rate: u32, bits: u32) -> Self {
        Self {
            tag: WAVE_FORMAT_PCM,
            channels,
            rate,
            block_align: channels * bits / 8,
            bits,
            samples_per_block: 1,
            coefs: vec![],
        }
    }
    pub fn float(channels: u32, rate: u32) -> Self {
        Self {
            tag: WAVE_FORMAT_IEEE_FLOAT,
            bits: 32,
            ..Self::pcm(channels, rate, 32)
        }
    }

    /// Parses a `WAVEFORMATEX` (plus the `ADPCMWAVEFORMAT` extension for
    /// tag 2). `bytes` must hold the whole structure including `cbSize`
    /// extra bytes for ADPCM.
    pub fn parse(bytes: &[u8]) -> Result<Self, FormatError> {
        let tag = u16_at(bytes, 0)?;
        let channels = u16_at(bytes, 2)? as u32;
        let rate = bytes
            .get(4..8)
            .map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
            .ok_or(FormatError::Truncated)?;
        let block_align = u16_at(bytes, 12)? as u32;
        let bits = u16_at(bytes, 14)? as u32;
        if channels == 0 || block_align == 0 {
            return Err(FormatError::Invalid("zero channels or block align"));
        }
        match tag {
            WAVE_FORMAT_PCM => {
                if !matches!(bits, 8 | 16 | 24 | 32) || block_align != channels * bits / 8 {
                    return Err(FormatError::Invalid("PCM depth/alignment"));
                }
                Ok(Self::pcm(channels, rate, bits))
            }
            WAVE_FORMAT_IEEE_FLOAT => {
                if bits != 32 || block_align != channels * 4 {
                    return Err(FormatError::Invalid("float depth/alignment"));
                }
                Ok(Self::float(channels, rate))
            }
            WAVE_FORMAT_ADPCM => {
                let extra = u16_at(bytes, 16)? as usize;
                if extra < 4 {
                    return Err(FormatError::Invalid("ADPCM cbSize"));
                }
                let samples_per_block = u16_at(bytes, 18)? as u32;
                let count = u16_at(bytes, 20)? as usize;
                if 4 + 4 * count > extra {
                    return Err(FormatError::Invalid("ADPCM coefficient count"));
                }
                let coefs = (0..count)
                    .map(|i| {
                        Ok((
                            u16_at(bytes, 22 + 4 * i)? as i16,
                            u16_at(bytes, 24 + 4 * i)? as i16,
                        ))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let f = Self {
                    tag,
                    channels,
                    rate,
                    block_align,
                    bits,
                    samples_per_block,
                    coefs,
                };
                // A block holds a 7-byte header per channel, then two
                // header samples and one nibble per further sample.
                let header = 7 * channels;
                if bits != 4
                    || channels > 2
                    || block_align <= header
                    || samples_per_block < 2
                    || (samples_per_block - 2) * channels > (block_align - header) * 2
                {
                    return Err(FormatError::Invalid("ADPCM block geometry"));
                }
                Ok(f)
            }
            t => Err(FormatError::Unsupported(t)),
        }
    }

    /// Frames (samples per channel) held by `bytes` bytes of data.
    pub fn frames(&self, bytes: u64) -> u64 {
        (bytes / self.block_align as u64) * self.samples_per_block as u64
    }

    /// Decodes frames `first .. first + count` of `data` (a whole source
    /// buffer) and appends them, interleaved, to `out`.
    /// Frames past the end of `data` decode as silence.
    pub fn decode(&self, data: &[u8], first: u64, count: u64, out: &mut Vec<f32>) {
        self.decode_with_scratch(data, first, count, out, &mut Vec::new());
    }

    /// Like `decode`, retaining the ADPCM block workspace between calls.
    /// Neither workspace allocates again while its existing capacity suffices.
    pub fn decode_with_scratch(
        &self,
        data: &[u8],
        first: u64,
        count: u64,
        out: &mut Vec<f32>,
        block: &mut Vec<i16>,
    ) {
        let ch = self.channels as usize;
        out.reserve(count as usize * ch);
        match self.tag {
            WAVE_FORMAT_ADPCM => self.decode_adpcm(data, first, count, out, block),
            _ => {
                let align = self.block_align as usize;
                for frame in first..first + count {
                    let o = frame as usize * align;
                    match data.get(o..o + align) {
                        Some(b) => {
                            for c in 0..ch {
                                out.push(self.sample(b, c));
                            }
                        }
                        None => out.extend(std::iter::repeat_n(0.0, ch)),
                    }
                }
            }
        }
    }

    fn sample(&self, frame: &[u8], c: usize) -> f32 {
        match (self.tag, self.bits) {
            (WAVE_FORMAT_IEEE_FLOAT, _) => {
                let o = 4 * c;
                f32::from_le_bytes([frame[o], frame[o + 1], frame[o + 2], frame[o + 3]])
            }
            (_, 8) => (frame[c] as f32 - 128.0) / 128.0,
            (_, 16) => i16::from_le_bytes([frame[2 * c], frame[2 * c + 1]]) as f32 / 32768.0,
            (_, 24) => {
                let o = 3 * c;
                let v = i32::from_le_bytes([0, frame[o], frame[o + 1], frame[o + 2]]) >> 8;
                v as f32 / 8388608.0
            }
            _ => {
                let o = 4 * c;
                let v = i32::from_le_bytes([frame[o], frame[o + 1], frame[o + 2], frame[o + 3]]);
                v as f32 / 2147483648.0
            }
        }
    }

    fn decode_adpcm(&self, data: &[u8], first: u64, count: u64, out: &mut Vec<f32>, block: &mut Vec<i16>) {
        let spb = self.samples_per_block as u64;
        let ch = self.channels as usize;
        block.resize(spb as usize * ch, 0);
        let mut frame = first;
        let end = first + count;
        while frame < end {
            let index = frame / spb;
            let o = (index * self.block_align as u64) as usize;
            let in_block = (frame % spb) as usize;
            let n = ((spb - frame % spb).min(end - frame)) as usize;
            match data.get(o..o + self.block_align as usize) {
                Some(b) => {
                    self.adpcm_block(b, block);
                    for s in &block[in_block * ch..(in_block + n) * ch] {
                        out.push(*s as f32 / 32768.0);
                    }
                }
                None => out.extend(std::iter::repeat_n(0.0, n * ch)),
            }
            frame += n as u64;
        }
    }

    /// Decodes one MS ADPCM block into `out` (interleaved i16).
    pub fn adpcm_block(&self, b: &[u8], out: &mut [i16]) {
        const ADAPTATION: [i32; 16] = [
            230, 230, 230, 230, 307, 409, 512, 614, 768, 614, 512, 409, 307, 230, 230, 230,
        ];
        let ch = self.channels as usize;
        let rd = |o: usize| i16::from_le_bytes([b[o], b[o + 1]]) as i32;
        let mut coef = [(0i32, 0i32); 2];
        let mut delta = [0i64; 2];
        let mut s1 = [0i32; 2];
        let mut s2 = [0i32; 2];
        for c in 0..ch {
            let p = b[c] as usize;
            let (c1, c2) = self.coefs.get(p).copied().unwrap_or((0, 0));
            coef[c] = (c1 as i32, c2 as i32);
            delta[c] = rd(ch + 2 * c) as i64;
            s1[c] = rd(3 * ch + 2 * c);
            s2[c] = rd(5 * ch + 2 * c);
            // The header's second sample is the first in time.
            out[c] = s2[c] as i16;
            out[ch + c] = s1[c] as i16;
        }
        let total = self.samples_per_block as usize * ch;
        let mut k = 2 * ch;
        let mut byte = 7 * ch;
        while k < total {
            let v = b[byte];
            byte += 1;
            for nibble in [v >> 4, v & 15] {
                if k >= total {
                    break;
                }
                let c = k % ch;
                let signed = ((nibble as i32) << 28) >> 28;
                let predict = (s1[c] * coef[c].0 + s2[c] * coef[c].1) >> 8;
                let sample = (predict as i64 + signed as i64 * delta[c]).clamp(-32768, 32767) as i32;
                s2[c] = s1[c];
                s1[c] = sample;
                // 32-bit wrapping, like the host's XAudio2 on step sizes
                // that overflow (oracle `adpcm_step_overflow_matches_the_host`).
                delta[c] = ((ADAPTATION[nibble as usize].wrapping_mul(delta[c] as i32) >> 8).max(16)) as i64;
                out[k] = sample as i16;
                k += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ADPCMWAVEFORMAT 0x00DCE1F0 builds on its stack.
    pub(crate) fn game_adpcm() -> Vec<u8> {
        let mut b = vec![];
        for w in [2u16, 1] {
            b.extend(w.to_le_bytes());
        }
        b.extend(48000u32.to_le_bytes());
        b.extend(0x668au32.to_le_bytes());
        for w in [70u16, 4, 32, 128, 7] {
            b.extend(w.to_le_bytes());
        }
        for (a, c) in [
            (256, 0),
            (512, -256),
            (0, 0),
            (192, 64),
            (240, 0),
            (460, -208),
            (392, -232),
        ] {
            b.extend((a as i16).to_le_bytes());
            b.extend((c as i16).to_le_bytes());
        }
        b
    }

    #[test]
    fn parses_the_games_adpcm_format() {
        let f = Format::parse(&game_adpcm()).unwrap();
        assert_eq!(
            (f.channels, f.rate, f.block_align, f.samples_per_block),
            (1, 48000, 70, 128)
        );
        assert_eq!(f.coefs[5], (460, -208));
        assert_eq!(f.frames(70 * 3), 384);
    }

    #[test]
    fn adpcm_header_samples_and_nibbles() {
        let f = Format::parse(&game_adpcm()).unwrap();
        let mut block = vec![0u8; 70];
        block[0] = 1; // coefficient pair (512, -256)
        block[1..3].copy_from_slice(&16i16.to_le_bytes()); // delta
        block[3..5].copy_from_slice(&200i16.to_le_bytes()); // sample 1
        block[5..7].copy_from_slice(&100i16.to_le_bytes()); // sample 2
        block[7] = 0x1f; // nibbles +1, -1
        let mut out = vec![0i16; 128];
        f.adpcm_block(&block, &mut out);
        // (200*512 - 100*256) >> 8 = 300, + 1*16; delta stays at its floor 16;
        // (316*512 - 200*256) >> 8 = 432, - 16.
        assert_eq!(&out[..4], &[100, 200, 316, 416]);
    }

    #[test]
    fn pcm_scales() {
        let f = Format::pcm16(2, 48000);
        let data: Vec<u8> = [i16::MIN, i16::MAX, 0, 16384]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let mut out = vec![];
        f.decode(&data, 0, 3, &mut out);
        assert_eq!(out, vec![-1.0, 32767.0 / 32768.0, 0.0, 0.5, 0.0, 0.0]);
        let f = Format::pcm(1, 48000, 8);
        out.clear();
        f.decode(&[0, 128, 255], 0, 3, &mut out);
        assert_eq!(out, vec![-1.0, 0.0, 127.0 / 128.0]);
    }
}
