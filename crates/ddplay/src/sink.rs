//! The host audio sink: the runtime's mastering-voice blocks
//! (`ddruntime::audio::output`, 48 kHz stereo float, one block per 10 ms
//! pass) to the default output device through `cpal` (WASAPI shared mode
//! on Windows).
//!
//! The device's mix format is converted at this edge only:
//! - channels: the two channels go to the device's first two (front left
//!   and right in every WAVEFORMATEXTENSIBLE layout); further device
//!   channels stay silent; a mono device gets 0.5 * (L + R), the matrix
//!   XAudio2 itself uses for stereo into one channel (ddaudio's
//!   `default_matrix(2, 1)`, measured on the host);
//! - rate: `ddaudio`'s resampler when the device does not run at the
//!   block rate;
//! - range: clipped to [-1, 1], as a mastering voice clips its final
//!   output ("XAudio2 Voices").
//!
//! A slow device drops the oldest audio beyond `MAX_QUEUE`; a starved one
//! plays silence. Neither stalls the guest's audio thread (the runtime's
//! channel never blocks).
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};
use ddruntime::audio::AudioBlock;
use std::collections::VecDeque;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

/// Queue bound: 100 ms of device audio.
const MAX_QUEUE_MS: usize = 100;

/// Plays `blocks` on the default output device from a thread of its own.
/// Without a device the blocks are drained and dropped.
pub fn start(blocks: Receiver<AudioBlock>) {
    std::thread::Builder::new()
        .name("audio sink".into())
        .spawn(move || {
            if let Err(e) = run(&blocks) {
                eprintln!("audio: no output device ({e}); audio is discarded");
                while blocks.recv().is_ok() {}
            }
        })
        .expect("the audio sink thread");
}

fn run(blocks: &Receiver<AudioBlock>) -> Result<(), String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no default output device")?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    let format = config.sample_format();
    let config: cpal::StreamConfig = config.into();
    let queue = Arc::new(Mutex::new(VecDeque::<f32>::new()));
    let stream = match format {
        cpal::SampleFormat::F32 => stream::<f32>(&device, &config, queue.clone()),
        cpal::SampleFormat::I16 => stream::<i16>(&device, &config, queue.clone()),
        cpal::SampleFormat::I32 => stream::<i32>(&device, &config, queue.clone()),
        cpal::SampleFormat::U16 => stream::<u16>(&device, &config, queue.clone()),
        f => return Err(format!("unsupported device sample format {f}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    let (channels, rate) = (config.channels as usize, config.sample_rate);
    eprintln!("audio: {channels} channels at {rate} Hz ({format})");
    let max = MAX_QUEUE_MS * rate as usize / 1000 * channels;
    let mut edge = Edge::new(channels, rate);
    while let Ok(block) = blocks.recv() {
        let out = edge.convert(&block);
        let mut q = queue.lock().unwrap();
        q.extend(out);
        let excess = q.len().saturating_sub(max);
        q.drain(..excess - excess % channels);
    }
    Ok(())
}

fn stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    queue: Arc<Mutex<VecDeque<f32>>>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let mut q = queue.lock().unwrap();
                for x in data.iter_mut() {
                    *x = T::from_sample(q.pop_front().unwrap_or(0.0));
                }
            },
            |e| eprintln!("audio: stream error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

/// Channel, rate and range conversion from the blocks to the device.
pub struct Edge {
    channels: usize,
    rate: u32,
    resampler: ddaudio::resample::Resampler,
    /// Fractional output frames carried between blocks.
    carry: f64,
}

impl Edge {
    pub fn new(channels: usize, rate: u32) -> Self {
        Self {
            channels,
            rate,
            resampler: Default::default(),
            carry: 0.0,
        }
    }

    pub fn convert(&mut self, block: &AudioBlock) -> Vec<f32> {
        let src = block.channels as usize;
        let frames = if block.sample_rate == self.rate {
            block.samples.clone()
        } else {
            self.resampler.push(&block.samples);
            let step = block.sample_rate as f64 / self.rate as f64;
            let n = self.carry + (block.samples.len() / src) as f64 / step;
            self.carry = n.fract();
            self.resampler.render(block.channels, step, n as usize).0
        };
        let mut out = Vec::with_capacity(frames.len() / src * self.channels);
        for f in frames.chunks_exact(src) {
            let (l, r) = (f[0], f.get(1).copied().unwrap_or(f[0]));
            if self.channels == 1 {
                out.push((0.5 * l + 0.5 * r).clamp(-1.0, 1.0));
                continue;
            }
            out.push(l.clamp(-1.0, 1.0));
            out.push(r.clamp(-1.0, 1.0));
            out.extend(std::iter::repeat_n(0.0, self.channels - 2));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(samples: Vec<f32>) -> AudioBlock {
        AudioBlock {
            sample_rate: 48000,
            channels: 2,
            samples,
        }
    }

    #[test]
    fn maps_channels_and_clips() {
        let b = block(vec![0.5, -0.25, 2.0, -3.0]);
        assert_eq!(Edge::new(2, 48000).convert(&b), vec![0.5, -0.25, 1.0, -1.0]);
        assert_eq!(
            Edge::new(6, 48000).convert(&b),
            vec![0.5, -0.25, 0.0, 0.0, 0.0, 0.0, 1.0, -1.0, 0.0, 0.0, 0.0, 0.0]
        );
        assert_eq!(Edge::new(1, 48000).convert(&b), vec![0.125, -0.5]);
    }

    #[test]
    fn converts_the_rate_continuously() {
        let mut e = Edge::new(2, 44100);
        let mut total = 0;
        for _ in 0..100 {
            total += e.convert(&block(vec![0.1; 960])).len() / 2;
        }
        // 1 s of 48 kHz input gives 1 s at 44.1 kHz.
        assert_eq!(total, 44100);
    }
}
