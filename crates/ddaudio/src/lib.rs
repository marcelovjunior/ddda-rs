//! ddaudio: what `xaudio2_7.dll` does between SubmitSourceBuffer and the
//! speakers. Decodes the source formats the game submits, converts sample
//! rates, applies volumes, matrices and effects through the voice graph,
//! and produces the mastering voice's float output, one 10 ms pass at a
//! time ("XAudio2 Audio Graph": the Windows quantum is 10 ms).
//!
//! Headless and independent of the runtime: `ddruntime::audio` owns the
//! guest-visible graph and buffer timing and calls in here to render.
pub mod format;
pub mod mixer;
pub mod resample;
pub mod reverb;

pub use format::Format;
pub use mixer::{Effect, EffectHost, EffectKind, Kind, Mixer, NoGuestEffects, Node, Send, VoiceId};
