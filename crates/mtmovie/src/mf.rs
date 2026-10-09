//! Media Foundation source readers (`IMFSourceReader`), one per stream kind.

use std::path::Path;
use std::sync::Once;

use windows::core::{GUID, HSTRING};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

use crate::{Audio, AudioFormat, Error, Frame, Info, Result};

fn err(what: &str) -> impl Fn(windows::core::Error) -> Error + '_ {
    move |e| Error(format!("{what}: {e}"))
}

/// COM on this thread and Media Foundation once per process.
fn startup() -> Result<()> {
    static MF: Once = Once::new();
    // S_FALSE / RPC_E_CHANGED_MODE: COM is already up on this thread.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let mut out = Ok(());
    MF.call_once(|| out = unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.map_err(err("MFStartup")));
    out
}

/// A source reader with only `stream` selected, decoding to `subtype`.
fn reader(path: &Path, stream: MF_SOURCE_READER_CONSTANTS, major: GUID, subtype: GUID) -> Result<IMFSourceReader> {
    startup()?;
    let url = HSTRING::from(path.as_os_str());
    unsafe {
        let mut attrs: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attrs, 1).map_err(err("MFCreateAttributes"))?;
        let attrs = attrs.ok_or_else(|| Error("MFCreateAttributes: none".into()))?;
        // colour conversion to RGB32 in the reader
        attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1).map_err(err("attributes"))?;
        let r = MFCreateSourceReaderFromURL(&url, &attrs).map_err(err("open"))?;
        r.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false).map_err(err("deselect"))?;
        r.SetStreamSelection(stream.0 as u32, true).map_err(err("select"))?;
        let mt = MFCreateMediaType().map_err(err("MFCreateMediaType"))?;
        mt.SetGUID(&MF_MT_MAJOR_TYPE, &major).map_err(err("major type"))?;
        mt.SetGUID(&MF_MT_SUBTYPE, &subtype).map_err(err("subtype"))?;
        r.SetCurrentMediaType(stream.0 as u32, None, &mt).map_err(err("output type"))?;
        Ok(r)
    }
}

/// The source's duration in seconds (`MF_PD_DURATION`, 100 ns units).
fn duration(r: &IMFSourceReader) -> f64 {
    unsafe {
        r.GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
            .map(|pv| pv.Anonymous.Anonymous.Anonymous.uhVal as f64 / 1e7)
            .unwrap_or(0.0)
    }
}

/// The next sample of `stream`: its time (s) and bytes; `None` at the end.
fn next(r: &IMFSourceReader, stream: MF_SOURCE_READER_CONSTANTS) -> Result<Option<(f64, Vec<u8>)>> {
    loop {
        let (mut flags, mut time) = (0u32, 0i64);
        let mut sample: Option<IMFSample> = None;
        unsafe {
            r.ReadSample(stream.0 as u32, 0, None, Some(&mut flags), Some(&mut time), Some(&mut sample))
                .map_err(err("ReadSample"))?;
        }
        if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            return Ok(None);
        }
        // gaps and format notices come without a sample
        let Some(sample) = sample else { continue };
        unsafe {
            let buf = sample.ConvertToContiguousBuffer().map_err(err("buffer"))?;
            let (mut p, mut len) = (std::ptr::null_mut::<u8>(), 0u32);
            buf.Lock(&mut p, None, Some(&mut len)).map_err(err("lock"))?;
            let bytes = std::slice::from_raw_parts(p, len as usize).to_vec();
            buf.Unlock().map_err(err("unlock"))?;
            return Ok(Some((time as f64 / 1e7, bytes)));
        }
    }
}

/// The video stream as top-down BGRA frames.
pub struct VideoReader {
    reader: IMFSourceReader,
    pub info: Info,
    /// Bytes per source row; negative for bottom-up rows.
    stride: i32,
}

impl VideoReader {
    pub fn open(path: &Path) -> Result<Self> {
        let s = MF_SOURCE_READER_FIRST_VIDEO_STREAM;
        let reader = reader(path, s, MFMediaType_Video, MFVideoFormat_RGB32)?;
        let mt = unsafe { reader.GetCurrentMediaType(s.0 as u32) }.map_err(err("video type"))?;
        let size = unsafe { mt.GetUINT64(&MF_MT_FRAME_SIZE) }.map_err(err("frame size"))?;
        let (width, height) = ((size >> 32) as u32, size as u32);
        let rate = unsafe { mt.GetUINT64(&MF_MT_FRAME_RATE) }.unwrap_or(30 << 32 | 1);
        let (num, den) = ((rate >> 32) as f64, (rate as u32).max(1) as f64);
        let stride = unsafe { mt.GetUINT32(&MF_MT_DEFAULT_STRIDE) }.map(|v| v as i32).unwrap_or(width as i32 * 4);
        let info = Info { width, height, fps: num / den, duration: duration(&reader) };
        Ok(VideoReader { reader, info, stride })
    }

    /// The next frame, or `None` after the last.
    pub fn next(&mut self) -> Result<Option<Frame>> {
        let Some((time, bytes)) = next(&self.reader, MF_SOURCE_READER_FIRST_VIDEO_STREAM)? else {
            return Ok(None);
        };
        let (w, h) = (self.info.width as usize, self.info.height as usize);
        let row = w * 4;
        let pitch = self.stride.unsigned_abs() as usize;
        if bytes.len() < pitch * (h - 1) + row {
            return Err(Error(format!("frame of {} bytes for {w}x{h} (stride {})", bytes.len(), self.stride)));
        }
        let mut bgra = vec![0u8; row * h];
        for y in 0..h {
            let src = if self.stride < 0 { h - 1 - y } else { y } * pitch;
            bgra[y * row..(y + 1) * row].copy_from_slice(&bytes[src..src + row]);
        }
        // RGB32 leaves the fourth byte undefined: opaque
        for px in bgra.chunks_exact_mut(4) {
            px[3] = 255;
        }
        Ok(Some(Frame { time, bgra }))
    }
}

/// The audio stream as interleaved `f32` PCM.
pub struct AudioReader {
    reader: IMFSourceReader,
    pub format: AudioFormat,
    pub duration: f64,
}

impl AudioReader {
    pub fn open(path: &Path) -> Result<Self> {
        let s = MF_SOURCE_READER_FIRST_AUDIO_STREAM;
        let reader = reader(path, s, MFMediaType_Audio, MFAudioFormat_Float)?;
        let mt = unsafe { reader.GetCurrentMediaType(s.0 as u32) }.map_err(err("audio type"))?;
        let channels = unsafe { mt.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS) }.map_err(err("channels"))? as u16;
        let sample_rate = unsafe { mt.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND) }.map_err(err("rate"))?;
        let duration = duration(&reader);
        Ok(AudioReader { reader, format: AudioFormat { channels, sample_rate }, duration })
    }

    pub fn next(&mut self) -> Result<Option<Audio>> {
        Ok(next(&self.reader, MF_SOURCE_READER_FIRST_AUDIO_STREAM)?.map(|(time, bytes)| Audio {
            time,
            samples: bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect(),
        }))
    }

    /// All of it.
    pub fn read_all(&mut self) -> Result<Vec<f32>> {
        let mut out = Vec::new();
        while let Some(a) = self.next()? {
            out.extend(a.samples);
        }
        Ok(out)
    }
}
