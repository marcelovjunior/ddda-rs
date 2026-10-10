//! The world's own enemies: which placement groups spawn where and when.
//!
//! A stage lists its groups in `scr\st<S>\etc\st<S>_e` ([`gpl`]); group N
//! places its units in the layouts `st<S>_<A>m<B>n_eN` of the cells it
//! names (`mtlot`). `sSetManager` (DTI 0x0198BE44) spawns a group's units
//! when the group's conditions hold ([`gate`], `cLayoutSetCharaBase::vf12`
//! 0x0075FAA0) and skips the units its kill record marks dead
//! ([`respawn`], 0x004A6830 / 0x004A6510 by the placement's `mSetID`); the
//! record ages with the day counter and frees the group again.
//!
//! [`spawns`] yields the placements to spawn for a stage cell, given the
//! story scene number, the hour, the layout flags and the kill records:
//! inputs -> state -> spawns, one call per cell the host loads. Nothing
//! here assumes one player.

pub mod gpl;
pub mod respawn;
pub mod save;

use gpl::{Group, GroupList};
use respawn::Record;

/// What the spawn test reads from the world.
pub struct World<'a> {
    /// SceNo (sGameSys +0x30).
    pub scene: i32,
    /// GameHour (sGameSys +0xB876C).
    pub hour: u32,
    /// The stage's layout flag `n` (sGameSys +0xADEE8 + (stage index *
    /// 0x53 + n / 32) * 4, MSB first; st100 flags 128..255 at +0xB3330:
    /// `FUN_0075C110`).
    pub flag: &'a dyn Fn(u16) -> bool,
}

/// Why a group does (not) spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    Spawn,
    /// +0x34 bit 2.
    Disabled,
    /// The first / second layout flag is clear.
    Flag(u16),
    /// SceNo outside [begin, end).
    Scene,
    /// The hour outside the window.
    Hour,
    /// Conditions the port does not evaluate (named in the group).
    Unported(Vec<&'static str>),
}

/// The hours of a window (0x0075FAA0: from `begin` up to, not including,
/// `end`, wrapping at 24; both must be below 24 or the window is ignored).
pub fn hour_mask(begin: u16, end: u16) -> Option<u32> {
    if begin >= 24 || end >= 24 {
        return None;
    }
    let mut m = 0u32;
    let mut h = begin;
    while h != end {
        m |= 1 << h;
        h += 1;
        if h > 23 {
            h = 0;
        }
    }
    Some(m)
}

/// `cLayoutSetCharaBase::vf12` (0x0075FAA0): may the group spawn now?
/// Checked in the game's order: disabled, the two layout flags, the SceNo
/// window, the hour window, then the conditions not ported here.
// Translated from 0x0075FAA0 (DDDA.exe 2017-12-13 Steam).
pub fn gate(g: &Group, w: &World) -> Gate {
    if g.disabled {
        return Gate::Disabled;
    }
    for f in [g.layout_flag, g.layout_flag2].into_iter().flatten() {
        if !(w.flag)(f) {
            return Gate::Flag(f);
        }
    }
    if let Some((b, e)) = g.scene_window {
        if w.scene < b || w.scene >= e {
            return Gate::Scene;
        }
    }
    if let Some((b, e)) = g.hours {
        if let Some(m) = hour_mask(b, e) {
            if m & (1 << (w.hour & 31)) == 0 {
                return Gate::Hour;
            }
        }
    }
    if !g.other_conditions.is_empty() {
        return Gate::Unported(g.other_conditions.clone());
    }
    Gate::Spawn
}

/// Removal conditions for an already materialized group. This is not
/// `gate != Spawn`: each removal condition has its own enable bit, and
/// scene/hour removal reads the raw window even if its spawn gate is off.
// Translated from 0x0075f860 (DDDA.exe 2017-12-13 Steam).
pub fn should_despawn(g: &Group, w: &World) -> bool {
    if g.despawn & 1 != 0
        && (!(w.flag)(g.layout_flags as u16) || g.layout_flag2.is_some_and(|f| !(w.flag)(f)))
    {
        return true;
    }
    if g.despawn & 2 != 0 && (w.scene < g.s.0[13] as i32 || w.scene >= g.s.0[14] as i32) {
        return true;
    }
    if g.despawn & 4 != 0 {
        let raw = g.s.0[19];
        if let Some(mask) = hour_mask(raw as u16, (raw >> 16) as u16) {
            return mask & (1 << (w.hour & 31)) == 0;
        }
    }
    false
}

/// Each group seen and its gate.
pub type Gates = Vec<(u32, Gate)>;

/// A placement to spawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Spawn {
    pub stage: u32,
    pub group: u32,
    pub cell: (u16, u16),
    /// The record id: the unit's bit in the group's kill record.
    pub set_id: i32,
    /// `rLayout` record kind and class (`cSetInfoEnemy0100` = a goblin).
    pub kind: u32,
    pub class: &'static str,
    pub name: String,
    pub position: [f32; 3],
    pub angle: [f32; 3],
    pub record: mtlot::Record,
}

/// The layout of group `group` in cell `<m>m<n>n` of stage `stage`, as the
/// engine names it (format 0x01562268 `scr\st%03d\etc\st%03d_%02dm%02dn%s%02d`):
/// `scr\st100\etc\st100_64m53n_e09`.
pub fn layout_path(stage: u32, m: u16, n: u16, list: char, group: u32) -> String {
    format!(r"scr\st{stage:03}\etc\st{stage:03}_{m:02}m{n:02}n_{list}{group:02}")
}

/// The enemy groups of `list` with placements in cell (m, n), their gate,
/// and the placements of those that spawn (dead units left out).
/// `read_lot` returns a layout's bytes; `records` are the stage's kill
/// records by slot (missing slots count as clean).
pub fn spawns(
    stage: u32,
    list: &GroupList,
    cell: (u16, u16),
    read_lot: &dyn Fn(&str) -> Option<Vec<u8>>,
    world: &World,
    records: &[Record],
) -> Result<(Gates, Vec<Spawn>), String> {
    let mut gates = vec![];
    let mut out = vec![];
    for g in &list.groups {
        if !g.cells.iter().any(|c| (c.m, c.n) == cell) {
            continue;
        }
        let gate = gate(g, world);
        let ok = gate == Gate::Spawn;
        gates.push((g.slot, gate));
        if !ok {
            continue;
        }
        let path = layout_path(stage, cell.0, cell.1, 'e', g.slot);
        let Some(bytes) = read_lot(&path) else { continue };
        let recs = mtlot::parse(&bytes).map_err(|e| format!("{path}: {e}"))?;
        let rec = records.get(g.slot as usize).copied().unwrap_or_default();
        for r in recs {
            // The unit's bit: the s32 the record loader (0x00CC1F60) reads
            // first, 1..n in a group's layout (*assumed* to be the +4 the
            // spawn passes to 0x004A6830; the coordinate block's own
            // `mSetID` is -1 in these files).
            let set_id = r.id;
            if set_id >= 0 && rec.dead(set_id as u32) {
                continue;
            }
            let Some(p) = r.placement() else { continue };
            out.push(Spawn {
                stage,
                group: g.slot,
                cell,
                set_id,
                kind: r.kind,
                class: r.class,
                name: p.name,
                position: p.position,
                angle: p.angle,
                record: r,
            });
        }
    }
    Ok((gates, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hour_windows_wrap() {
        let night = hour_mask(20, 4).unwrap();
        assert!(
            night & (1 << 22) != 0
                && night & (1 << 2) != 0
                && night & (1 << 4) == 0
                && night & (1 << 12) == 0
        );
        let day = hour_mask(4, 20).unwrap();
        assert!(day & (1 << 4) != 0 && day & (1 << 19) != 0 && day & (1 << 20) == 0);
        assert_eq!(hour_mask(24, 3), None);
        assert_eq!(hour_mask(5, 5), Some(0));
    }
}
