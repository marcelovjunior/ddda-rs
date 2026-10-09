//! NPC layout candidates and explicit intel lifetimes. Loading a layout is
//! distinct from resolving its placement into an active NPC.

use ddspawn::{gpl::GroupList, Gate, World};
use ddstory::Target;

#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub stage: u32,
    pub target: Target,
    pub npc: i32,
    pub use_24_schedule: u8,
    pub authored_action: String,
    pub transform: mtlot::Placement,
    /// Preserve unported actor options for the materialization adapter.
    pub record: mtlot::Record,
}

impl Placement {
    /// cSetInfoNpc::vf08 (0x007a32f0) copies mNpcId (+0xc8) to
    /// intel+0x3e4. 0x007624d0 copies SetInfo+4 to intel+0x5c8;
    /// that is the layout record id, independent of mNpcId/mSetID.
    pub fn from_record(stage: u32, group: u32, record: mtlot::Record) -> Result<Self, String> {
        if record.class != "cSetInfoNpc" || record.id < 0 {
            return Err("expected NPC layout record with nonnegative record id".into());
        }
        let int = |name| match record.get(name) {
            Some(mtlot::Value::Int(value)) => Ok(*value),
            _ => Err(format!("NPC layout missing {name}")),
        };
        let npc = i32::try_from(int("mNpcId")?).map_err(|_| "NPC id out of range")?;
        let use_24_schedule = u8::try_from(int("mUse24Schedule")?).map_err(|_| "NPC schedule mode out of range")?;
        let authored_action = match record.get("FSMPath") {
            Some(mtlot::Value::Str(path)) => String::from_utf8(path.clone()).map_err(|_| "NPC FSM path is not UTF-8")?,
            _ => return Err("NPC layout missing FSMPath".into()),
        };
        let transform = record.placement().ok_or("NPC layout missing transform")?;
        Ok(Self {
            stage, target: Target { kind: 5, group, index: record.id as u32 },
            npc, use_24_schedule, authored_action, transform, record,
        })
    }
}

pub fn group_list(vfs: &ddvfs::Vfs, stage: u32) -> Result<GroupList, String> {
    let path = format!(r"scr\st{stage:03}\etc\st{stage:03}_n");
    GroupList::parse(&vfs.read(&path, mtarc::type_hash("rLayoutGroupParamList"))?)
        .map_err(|e| format!("{path}: {e}"))
}

#[derive(Debug, Default)]
pub struct Candidates {
    pub gates: Vec<(u32, Gate)>,
    pub placements: Vec<Placement>,
}

/// Read one cell's eligible NPC layout candidates. cLayoutSetNpc vf12
/// at vtable 0x01593f30 resolves to the common gate 0x0075faa0. The
/// `_n` path follows the group format at 0x01562268. Materialization's
/// name-family, persistent NPC life and archive readiness checks in
/// 0x00762000 remain the actor adapter's responsibility; candidates do
/// not by themselves establish presence or controller readiness.
pub fn candidates(
    vfs: &ddvfs::Vfs, stage: u32, list: &GroupList, cell: (u16, u16), world: &World<'_>,
) -> Result<Candidates, String> {
    let mut out = Candidates::default();
    for group in &list.groups {
        if !group.cells.iter().any(|c| (c.m, c.n) == cell) { continue; }
        let gate = ddspawn::gate(group, world);
        let eligible = gate == Gate::Spawn;
        out.gates.push((group.slot, gate));
        if !eligible { continue; }
        let path = ddspawn::layout_path(stage, cell.0, cell.1, 'n', group.slot);
        let bytes = vfs.read(&path, mtarc::type_hash("rLayout"))?;
        for record in mtlot::parse(&bytes).map_err(|e| format!("{path}: {e}"))? {
            if record.class == "cSetInfoNpc" {
                out.placements.push(Placement::from_record(stage, group.slot, record)?);
            }
        }
    }
    Ok(out)
}
