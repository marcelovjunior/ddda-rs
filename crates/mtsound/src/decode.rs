//! Device-independent decoding. No mixer, filesystem access, or simulation.
use std::io::Cursor;
use symphonia::core::{audio::SampleBuffer, errors::Error, io::MediaSourceStream, probe::Hint};

#[derive(Clone, Debug)]
pub struct Pcm {
    pub channels: u16,
    pub sample_rate: u32,
    /// Interleaved normalized samples, in decoder channel order.
    pub samples: Vec<f32>,
}

/// Decode an installed loose Vorbis stream with Lewton. The sample limit
/// belongs to the caller's memory budget, not to game behavior.
pub fn vorbis(bytes: &[u8], sample_limit: usize) -> Result<Pcm, String> {
    let mut reader =
        lewton::inside_ogg::OggStreamReader::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut pcm = Pcm {
        channels: reader.ident_hdr.audio_channels.into(),
        sample_rate: reader.ident_hdr.audio_sample_rate,
        samples: Vec::new(),
    };
    while let Some(packet) = reader.read_dec_packet_itl().map_err(|e| e.to_string())? {
        if packet.len() > sample_limit.saturating_sub(pcm.samples.len()) {
            return Err("decoded audio exceeds sample limit".into());
        }
        pcm.samples.extend(packet.into_iter().map(|x| x as f32 / 32768.0));
    }
    valid(pcm)
}

/// Decode a reconstructed SPAC RIFF with Symphonia (Microsoft ADPCM/PCM).
pub fn wave(bytes: &[u8], sample_limit: usize) -> Result<Pcm, String> {
    crate::riff_chunks(bytes)?;
    let source = MediaSourceStream::new(Box::new(Cursor::new(bytes.to_vec())), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("wav");
    let mut format = symphonia::default::get_probe()
        .format(&hint, source, &Default::default(), &Default::default())
        .map_err(|e| e.to_string())?
        .format;
    let track = format.default_track().ok_or("wave has no audio track")?;
    let id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &Default::default())
        .map_err(|e| e.to_string())?;
    let mut pcm = Pcm {
        channels: 0,
        sample_rate: 0,
        samples: Vec::new(),
    };
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.to_string()),
        };
        if packet.track_id() != id {
            continue;
        }
        let audio = decoder.decode(&packet).map_err(|e| e.to_string())?;
        let spec = *audio.spec();
        let channels = spec.channels.count() as u16;
        if pcm.channels != 0 && (pcm.channels != channels || pcm.sample_rate != spec.rate) {
            return Err("wave changes audio format".into());
        }
        pcm.channels = channels;
        pcm.sample_rate = spec.rate;
        let mut samples = SampleBuffer::<f32>::new(audio.capacity() as u64, spec);
        samples.copy_interleaved_ref(audio);
        if samples.len() > sample_limit.saturating_sub(pcm.samples.len()) {
            return Err("decoded audio exceeds sample limit".into());
        }
        pcm.samples.extend_from_slice(samples.samples());
    }
    valid(pcm)
}

fn valid(pcm: Pcm) -> Result<Pcm, String> {
    if pcm.channels == 0
        || pcm.sample_rate == 0
        || pcm.samples.is_empty()
        || pcm.samples.len() % pcm.channels as usize != 0
        || pcm.samples.iter().any(|x| !x.is_finite())
    {
        return Err("invalid decoded audio".into());
    }
    Ok(pcm)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pcm_wave_and_limits() {
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&40_u32.to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16_u32.to_le_bytes());
        for x in [1_u16, 1] {
            b.extend_from_slice(&x.to_le_bytes());
        }
        for x in [8000_u32, 16000] {
            b.extend_from_slice(&x.to_le_bytes());
        }
        for x in [2_u16, 16] {
            b.extend_from_slice(&x.to_le_bytes());
        }
        b.extend_from_slice(b"data");
        b.extend_from_slice(&4_u32.to_le_bytes());
        for x in [16384_i16, -16384] {
            b.extend_from_slice(&x.to_le_bytes());
        }
        let p = wave(&b, 2).unwrap();
        assert_eq!((p.channels, p.sample_rate), (1, 8000));
        assert_eq!(p.samples, [0.5, -0.5]);
        assert!(wave(&b, 1).is_err());
        assert!(wave(&b[..b.len() - 1], 2).is_err());
        assert!(vorbis(&b, 2).is_err());
    }
}
