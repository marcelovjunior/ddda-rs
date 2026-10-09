//! The game's movies (`movie\event\*.wmv`, `movie\title\*.wmv`): WMV video
//! with WMA audio, decoded at run time from the player's install with the
//! system's Media Foundation (Windows only). No transcoded copies exist on
//! disk.
//!
//! [`Movie`] reads one stream kind at a time: video frames as top-down
//! BGRA, or audio as interleaved `f32` PCM. Open the file twice to read
//! both.

#[cfg(windows)]
mod mf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// One decoded picture.
#[derive(Debug, Clone)]
pub struct Frame {
    /// Presentation time, seconds from the start.
    pub time: f64,
    /// `width * height * 4` bytes, top row first, B G R A (A is opaque).
    pub bgra: Vec<u8>,
}

/// A decoded run of audio.
#[derive(Debug, Clone)]
pub struct Audio {
    pub time: f64,
    /// Interleaved samples, `channels` per frame.
    pub samples: Vec<f32>,
}

/// What a movie's streams look like.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Info {
    pub width: u32,
    pub height: u32,
    /// Frames per second from the stream's frame rate.
    pub fps: f64,
    /// Seconds.
    pub duration: f64,
}

#[cfg(windows)]
pub use mf::{AudioReader, VideoReader};

/// The audio format [`AudioReader`] produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormat {
    pub channels: u16,
    pub sample_rate: u32,
}
