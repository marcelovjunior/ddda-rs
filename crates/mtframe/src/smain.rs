//! `sMain`, the application singleton that owns the clock
//! (docs/FRAME.md 1a; names from its DTI property list, `sMain::vf03`
//! 0x00DBEB70).

use core::mem::offset_of;
use ddnative_base::mem::Mem;

/// The global `sMain*` (its constructor 0x00DBE810 stores `this` here).
pub const INSTANCE: u32 = 0x018D_0B28;

/// `*INSTANCE` as this thread sees it (ddnative-base sched.rs, L172: the
/// parallel shadow's lanes see their private copy).
pub fn instance(m: &dyn Mem) -> u32 {
    match ddnative_base::sched::view() {
        0 => m.u32(INSTANCE),
        private => private,
    }
}

/// The timing fields of `sMain`, +0x00..+0xA0 (the object is larger;
/// [`QpcLayout`] is the counter block at +0x1BFE0).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct MainLayout {
    pub _head: [u32; 8],
    /// +0x20 `mTimer`: elapsed milliseconds since `timer_origin`
    pub timer_ms: u64,
    /// +0x28: the counter value `ResetTimer` (0x00DBE210) took as zero
    pub timer_origin: u64,
    /// +0x30 `mFrameTimer`: `mTimer` × `mFps` / 1000
    pub frame_timer: u32,
    pub _34: u32,
    /// +0x38 `mFps`: the base rate (30.0 in the game)
    pub fps: f32,
    /// +0x3C `mMaxFps`: the cap
    pub max_fps: f32,
    /// +0x40 `mFrameWait`: fixed-step mode
    pub frame_wait: u8,
    pub _41: [u8; 3],
    /// +0x44 `mActualFps`: the measured rate (one-second window)
    pub actual_fps: f32,
    pub _48: f32,
    pub _4c: u32,
    /// +0x50: counter ticks elapsed, accumulated frame by frame
    pub ticks: u64,
    /// +0x58: `ticks` at the last frame boundary
    pub boundary: u64,
    /// +0x60: the counter value seen last
    pub last_count: u64,
    /// +0x68: steps left while paused (counts down each frame)
    pub pause_steps: u32,
    /// +0x6C `mPause`
    pub pause: u8,
    pub _6d: [u8; 3],
    /// +0x70 `mDeltaTime`: this frame's step in base frames
    pub delta_time: f32,
    /// +0x74 `mDeltaTimeBorder`: a longer step is a hitch
    pub delta_time_border: f32,
    /// +0x78 `mDeltaTimeLimite`: the longest step
    pub delta_time_limit: f32,
    /// +0x7C: 30 / max(1, `mActualFps`), the averaged step
    pub average_step: f32,
    /// +0x80 `mGlobalSpeed`
    pub global_speed: f32,
    /// +0x84 `mDelayFrame`: base frames dropped by hitches
    pub delay_frame: f32,
    pub _88: [u32; 4],
    /// +0x98: copied from +0x9C at the start of every frame
    pub previous_9c: u32,
    pub _9c: u32,
}

/// The performance-counter block of `sMain` (+0x1BFE0..+0x1C038).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct QpcLayout {
    /// +0x1BFE0: the counter in seconds (count × `seconds_per_tick`)
    pub seconds: f64,
    /// +0x1BFE8: `seconds` at the previous reading
    pub previous_seconds: f64,
    /// +0x1BFF0: the difference, as f32
    pub seconds_delta: f32,
    pub _1bff4: [u32; 3],
    /// +0x1C000: the counter
    pub count: u64,
    /// +0x1C008: `count` at the previous reading
    pub previous_count: u64,
    /// +0x1C010: low word of `count − previous_count`
    pub count_delta: u32,
    pub _1c014: [u32; 5],
    /// +0x1C028: 1 / `frequency`
    pub seconds_per_tick: f64,
    /// +0x1C030: counter ticks per second (from 0x0130A5C0)
    pub frequency: i64,
}

/// Where [`QpcLayout`] starts in `sMain`.
pub const QPC: u32 = 0x1_BFE0;

/// `sMain` in guest memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Main(pub u32);

macro_rules! at {
    ($field:ident) => {
        offset_of!(MainLayout, $field) as u32
    };
}

impl Main {
    /// The running instance, `*INSTANCE`.
    pub fn get(m: &dyn Mem) -> Main {
        Main(instance(m))
    }
    pub fn field(self, offset: u32) -> u32 {
        self.0.wrapping_add(offset)
    }
    pub fn fps(self, m: &dyn Mem) -> f32 {
        f32::from_bits(m.u32(self.field(at!(fps))))
    }
    pub fn delta_time_bits(self, m: &dyn Mem) -> u32 {
        m.u32(self.field(at!(delta_time)))
    }
    pub fn delta_time(self, m: &dyn Mem) -> f32 {
        f32::from_bits(self.delta_time_bits(m))
    }
    pub fn pause_steps(self, m: &dyn Mem) -> u32 {
        m.u32(self.field(at!(pause_steps)))
    }
    pub fn pause(self, m: &dyn Mem) -> bool {
        m.u8(self.field(at!(pause))) != 0
    }
    /// Paused with no steps left: the test the per-frame systems make
    /// before they advance (`pause_steps == 0 && pause`).
    pub fn halted(self, m: &dyn Mem) -> bool {
        self.pause_steps(m) == 0 && self.pause(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_match_the_offsets_the_code_uses() {
        assert_eq!(offset_of!(MainLayout, timer_ms), 0x20);
        assert_eq!(offset_of!(MainLayout, frame_timer), 0x30);
        assert_eq!(offset_of!(MainLayout, fps), 0x38);
        assert_eq!(offset_of!(MainLayout, frame_wait), 0x40);
        assert_eq!(offset_of!(MainLayout, actual_fps), 0x44);
        assert_eq!(offset_of!(MainLayout, ticks), 0x50);
        assert_eq!(offset_of!(MainLayout, boundary), 0x58);
        assert_eq!(offset_of!(MainLayout, last_count), 0x60);
        assert_eq!(offset_of!(MainLayout, pause_steps), 0x68);
        assert_eq!(offset_of!(MainLayout, pause), 0x6C);
        assert_eq!(offset_of!(MainLayout, delta_time), 0x70);
        assert_eq!(offset_of!(MainLayout, average_step), 0x7C);
        assert_eq!(offset_of!(MainLayout, delay_frame), 0x84);
        assert_eq!(offset_of!(MainLayout, previous_9c), 0x98);
        assert_eq!(core::mem::size_of::<MainLayout>(), 0xA0);
        assert_eq!(QPC + offset_of!(QpcLayout, count) as u32, 0x1_C000);
        assert_eq!(QPC + offset_of!(QpcLayout, count_delta) as u32, 0x1_C010);
        assert_eq!(QPC + offset_of!(QpcLayout, seconds_per_tick) as u32, 0x1_C028);
        assert_eq!(QPC + offset_of!(QpcLayout, frequency) as u32, 0x1_C030);
    }
}
