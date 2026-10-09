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
/// name-family, intel-pool capacity and archive readiness checks in
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

/// Recognized placement-name families. Entries are read from the player's
/// executable rather than duplicating its name table in the port.
#[derive(Debug)]
pub struct NameFamilies(Vec<(String, bool)>);

impl NameFamilies {
    /// Table at 0x0150f6f0, traversed by 0x007627e0 in 16-byte records
    /// until a null name pointer. 0x00762000 distinguishes MultiNpc by
    /// the selected record's address, 0x0150f700.
    pub fn from_exe(exe: &ddexe::Exe<'_>) -> Result<Self, ddexe::Error> {
        let mut address = 0x0150_f6f0;
        let mut entries = Vec::new();
        loop {
            let name = exe.u32(address)?;
            if name == 0 { return Ok(Self(entries)); }
            entries.push((exe.cstr(name)?, address == 0x0150_f700));
            address = address.checked_add(16)
                .ok_or_else(|| ddexe::Error("NPC name-family table overflow".into()))?;
        }
    }

    /// Translated from 0x007627e0 (DDDA.exe 2017-12-13 Steam): longest
    /// nonempty case-insensitive prefix wins, with first entry winning ties.
    /// Installed names are ASCII. Returns the MultiNpc flag, or None when
    /// this name cannot materialize as an NPC family.
    pub fn select(&self, name: &str) -> Option<bool> {
        let mut selected = None;
        let mut longest = 0;
        for (prefix, multi) in &self.0 {
            if prefix.len() > longest && name.as_bytes().get(..prefix.len())
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix.as_bytes())) {
                longest = prefix.len();
                selected = Some(*multi);
            }
        }
        selected
    }
}

#[cfg(test)]
mod name_family_tests {
    use super::NameFamilies;

    #[test]
    fn longest_prefix_first_tie_and_no_match() {
        let families = NameFamilies(vec![("".into(), true), ("N".into(), false),
            ("Npc".into(), true), ("NPC".into(), false)]);
        assert_eq!(families.select("nPc_suffixed"), Some(true));
        assert_eq!(families.select("n"), Some(false));
        assert_eq!(families.select(""), None);
        assert_eq!(families.select("OtherNpc"), None);
    }
}

/// Parsed NPC layouts for cells the adapter has actually loaded. This cache
/// avoids file reads on every host frame; it still produces only candidates.
#[derive(Debug)]
pub struct LoadedLayouts {
    pub stage: u32,
    groups: Vec<(ddspawn::gpl::Group, Vec<Placement>)>,
}

impl LoadedLayouts {
    pub fn read(vfs: &ddvfs::Vfs, stage: u32, list: &GroupList, cells: &[(u16, u16)]) -> Result<Self, String> {
        let mut groups = Vec::new();
        for group in &list.groups {
            let mut placements = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for cell in &group.cells {
                if !cells.contains(&(cell.m, cell.n)) || !seen.insert((cell.m, cell.n)) { continue; }
                let path = ddspawn::layout_path(stage, cell.m, cell.n, 'n', group.slot);
                let bytes = vfs.read(&path, mtarc::type_hash("rLayout"))?;
                for record in mtlot::parse(&bytes).map_err(|error| format!("{path}: {error}"))? {
                    if record.class == "cSetInfoNpc" {
                        placements.push(Placement::from_record(stage, group.slot, record)?);
                    }
                }
            }
            if !seen.is_empty() { groups.push((group.clone(), placements)); }
        }
        Ok(Self { stage, groups })
    }

    pub fn candidates(&self, world: &World<'_>) -> Candidates {
        let mut out = Candidates::default();
        for (group, placements) in &self.groups {
            let gate = ddspawn::gate(group, world);
            if gate == Gate::Spawn { out.placements.extend(placements.iter().cloned()); }
            out.gates.push((group.slot, gate));
        }
        out
    }
}
