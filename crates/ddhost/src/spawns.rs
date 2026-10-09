//! The world's own enemies in the host's fight (ddspawn).
//!
//! For each loaded stage cell the host asks [`spawn_cells`] which placement
//! groups spawn (the group list `st<S>_e`, gated by the story's SceNo, hour
//! and layout flags, minus the units the kill records mark dead) and adds
//! them: goblins (`cSetInfoEnemy0100`, uEm0100) join the fight as running
//! enemies; every other kind is returned as a placeholder with its class and
//! position, for the view to show.
//!
//! *Placeholders*: the cells loaded around a position are its 3x3 block
//! (the streaming radius is not traced); the facing is the placement's
//! `mAngle` y (the Euler order is not traced).

use ddenemyrt::fight::Fight;
use ddspawn::gpl::GroupList;
use ddspawn::respawn::Record;
use ddspawn::{Gate, Spawn, World};

/// A stage's enemy group list from the install.
pub fn group_list(vfs: &ddvfs::Vfs, stage: u32) -> Result<GroupList, String> {
    let path = format!(r"scr\st{stage:03}\etc\st{stage:03}_e");
    let b = vfs.read(&path, mtarc::type_hash("rLayoutGroupParamList"))?;
    GroupList::parse(&b).map_err(|e| format!("{path}: {e}"))
}

/// The stage-frame cell `(m, n)` holding a position (origin cell 50m50n,
/// docs/WORLD.md: x = (n - 50) * 10000 + local, z = (m - 50) * 10000 +
/// local).
pub fn cell_of(pos: [f32; 3]) -> (u16, u16) {
    let m = (pos[2] / 10000.0).floor() as i32 + 50;
    let n = (pos[0] / 10000.0).floor() as i32 + 50;
    (m.clamp(0, 99) as u16, n.clamp(0, 99) as u16)
}

/// The 3x3 cells around a position (*placeholder* streaming radius).
pub fn cells_around(pos: [f32; 3]) -> Vec<(u16, u16)> {
    let (m, n) = cell_of(pos);
    let mut out = vec![];
    for dm in -1i32..=1 {
        for dn in -1i32..=1 {
            let (a, b) = (m as i32 + dm, n as i32 + dn);
            if (0..100).contains(&a) && (0..100).contains(&b) {
                out.push((a as u16, b as u16));
            }
        }
    }
    out
}

/// The spawn test's inputs from the story state: SceNo, hour, and the
/// stage's layout flags.
pub fn world_inputs(world: &ddstory::world::World, stage: u32) -> (i32, u32, impl Fn(u16) -> bool + '_) {
    (world.sce_no as i32, world.hour, move |n: u16| {
        world.lot(stage, usize::from(n))
    })
}

/// What [`spawn_cells`] did.
#[derive(Debug, Default)]
pub struct CellSpawns {
    /// Goblins added to the fight: (owner id, placement).
    pub goblins: Vec<(u32, Spawn)>,
    /// Enemies without a runtime here.
    pub placeholders: Vec<Spawn>,
    /// Every group seen and its gate.
    pub gates: Vec<((u16, u16), u32, Gate)>,
}

/// Spawns the groups of `cells` into `fight`: goblins run, the rest are
/// returned as placeholders. `records` are the stage's kill records by
/// group slot.
pub fn spawn_cells(
    fight: &mut Fight,
    vfs: &ddvfs::Vfs,
    stage: u32,
    list: &GroupList,
    cells: &[(u16, u16)],
    world: &World,
    records: &[Record],
) -> Result<CellSpawns, String> {
    let read = |p: &str| vfs.read(p, mtarc::type_hash("rLayout")).ok();
    let mut out = CellSpawns::default();
    for &cell in cells {
        let (gates, spawns) = ddspawn::spawns(stage, list, cell, &read, world, records)?;
        out.gates
            .extend(gates.into_iter().map(|(g, gate)| (cell, g, gate)));
        for s in spawns {
            if s.class == "cSetInfoEnemy0100" {
                let owner = fight.add_goblin(s.position, s.angle[1]);
                out.goblins.push((owner, s));
            } else {
                out.placeholders.push(s);
            }
        }
    }
    Ok(out)
}
