//! Motion events that switch hit volumes on.
//!
//! Every motion of an `rMotionList` (.lmt) carries an event block of four
//! groups (Riftstone's LMT notes, MIT; the PC loader is
//! `rMotionList::load` 0x00E9D330):
//!
//! ```text
//! group  72 bytes: u16 remap[32], u32 count, u32 list   (offsets are file offsets)
//! event  8 bytes:  u32 bits, u32 frames                 (run-length over the motion)
//! ```
//!
//! Group 0 drives collision. Each frame, `FUN_0076F430` (DDDA.exe
//! 2017-12-13 Steam) walks the model's 8 motion layers (uModel+0x380,
//! stride 0x140), asks `FUN_01057060` for the event bits at the layer's
//! current frame (truncated to an integer), and for each set bit i < 16
//! reads `remap[i]` = `rObjCollision.mResourceID * 1000 + SeqIndex number`.
//! The matching `rObjCollision` among the 8 the collision holds
//! (`cObjCollision`+0xC3C) activates that sequence entry (`FUN_0076F660`):
//! its group's nodes become live volumes carrying its attack
//! (`FUN_0076F7A0`), for this frame only.

/// One event group of one motion.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventGroup {
    pub remap: [u16; 32],
    /// (bits, frames) runs in order.
    pub events: Vec<(u32, u32)>,
}

/// The four event groups of one motion.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MotionEvents {
    pub groups: [EventGroup; 4],
}

/// Collision sequence activated by a motion event bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Activation {
    /// `rObjCollision.mResourceID` of the file holding the sequence index.
    pub resource_id: u32,
    /// Index into that file's `SeqIndex` table.
    pub seq: u32,
}

fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(o..o + 2)?.try_into().ok()?))
}
fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?))
}

/// The event blocks of every motion slot of an `.lmt` file (None for an
/// empty slot or a motion without the has-events flag 0x800000).
pub fn parse_lmt_events(d: &[u8]) -> Result<Vec<Option<MotionEvents>>, String> {
    if d.get(..4) != Some(b"LMT\0") {
        return Err("not an LMT".into());
    }
    let count = u16_at(d, 6).ok_or("truncated header")? as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let m = u32_at(d, 8 + 4 * i).ok_or("truncated offsets")? as usize;
        if m == 0 {
            out.push(None);
            continue;
        }
        let flags = u32_at(d, m + 0x30).ok_or("truncated motion")?;
        let ev = u32_at(d, m + 0x34).ok_or("truncated motion")? as usize;
        if flags & 0x80_0000 == 0 || ev == 0 {
            out.push(None);
            continue;
        }
        let mut me = MotionEvents::default();
        for (g, group) in me.groups.iter_mut().enumerate() {
            let base = ev + 72 * g;
            for (k, r) in group.remap.iter_mut().enumerate() {
                *r = u16_at(d, base + 2 * k).ok_or("truncated event group")?;
            }
            let n = u32_at(d, base + 64).ok_or("truncated event group")? as usize;
            let list = u32_at(d, base + 68).ok_or("truncated event group")? as usize;
            if n > d.len() / 8 {
                return Err(format!("motion {i} group {g}: event count {n}"));
            }
            for e in 0..n {
                let o = list + 8 * e;
                group.events.push((u32_at(d, o).ok_or("truncated events")?, u32_at(d, o + 4).ok_or("truncated events")?));
            }
        }
        out.push(Some(me));
    }
    Ok(out)
}

/// Translated from 0x01057060 (DDDA.exe 2017-12-13 Steam): OR of the bits
/// of the runs that end after `from`, walking until the running end passes
/// `to`. Called with `from == to == trunc(frame)` it yields the bits of the
/// run containing that frame.
pub fn event_bits(group: &EventGroup, from: i32, to: i32) -> u32 {
    let mut bits = 0u32;
    let mut end: i64 = 0;
    for &(b, frames) in &group.events {
        end += frames as i32 as i64;
        if (from as i64) < end {
            bits |= b;
        }
        if end > to as i64 {
            break;
        }
    }
    bits
}

/// Translated from the inner loop of 0x0076F430 (DDDA.exe 2017-12-13
/// Steam): the collision sequences group 0 of a motion activates at
/// `frame`. Only bits 0..15 are examined.
pub fn activations(events: &MotionEvents, frame: f32) -> Vec<Activation> {
    let g = &events.groups[0];
    let f = frame as i32; // CVTTSS2SI
    let bits = event_bits(g, f, f);
    (0..16)
        .filter(|i| bits >> i & 1 != 0)
        .map(|i| {
            let v = g.remap[i] as u32;
            Activation { resource_id: v / 1000, seq: v % 1000 }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_lengths_and_remap() {
        let mut g = EventGroup::default();
        g.remap[0] = 3;
        g.remap[2] = 1_017;
        g.events = vec![(0, 10), (0b101, 4), (0, 20)];
        let me = MotionEvents { groups: [g, EventGroup::default(), EventGroup::default(), EventGroup::default()] };
        assert!(activations(&me, 9.9).is_empty());
        assert_eq!(
            activations(&me, 10.0),
            vec![Activation { resource_id: 0, seq: 3 }, Activation { resource_id: 1, seq: 17 }]
        );
        assert_eq!(activations(&me, 13.5).len(), 2);
        assert!(activations(&me, 14.0).is_empty());
    }
}
