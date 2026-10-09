//! Object groups and the original name-to-class table.

use ddspawn::{gpl::GroupList, Gate, World};

#[derive(Debug, Clone)]
pub struct Family {
    pub name: String,
    pub class_dti: u32,
}

#[derive(Debug)]
pub struct Families(pub Vec<Family>);

impl Families {
    /// Table 0x0150fe40, 32-byte rows terminated by a null name.
    /// 0x0075f0a0 resolves the longest name prefix; 0x00762d30 uses +8
    /// for the concrete object class. Bulk names stay in the install.
    pub fn read(exe: &ddexe::Exe<'_>) -> Result<Self, ddexe::Error> {
        let mut rows = Vec::new();
        let mut address = 0x0150_fe40u32;
        loop {
            let pointer = exe.u32(address)?;
            if pointer == 0 { return Ok(Self(rows)); }
            rows.push(Family { name: exe.cstr(pointer)?, class_dti: exe.u32(address + 8)? });
            address = address.checked_add(32).ok_or_else(|| ddexe::Error("object table overflow".into()))?;
        }
    }

    // Translated from 0x0075f0a0 (DDDA.exe 2017-12-13 Steam).
    pub fn select(&self, name: &str) -> Option<&Family> {
        let mut found: Option<&Family> = None;
        for row in &self.0 {
            if !row.name.is_empty()
                && found.is_none_or(|old| row.name.len() > old.name.len())
                && name.as_bytes().get(..row.name.len()).is_some_and(|p| p.eq_ignore_ascii_case(row.name.as_bytes())) {
                found = Some(row);
            }
        }
        found
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id {
    pub stage: u32,
    pub group: u32,
    pub record: i32,
}

#[derive(Debug, Clone)]
pub struct Placement {
    pub id: Id,
    pub object: u32,
    pub transform: mtlot::Placement,
    pub record: mtlot::Record,
}

impl Placement {
    pub fn from_record(stage: u32, group: u32, record: mtlot::Record, families: &Families) -> Result<Option<Self>, String> {
        let transform = record.placement().ok_or("object record has no transform")?;
        let Some(family) = families.select(&transform.name) else { return Ok(None) };
        // uOmObj7010 DTI; class selection is authored by the table, not
        // inferred from DoorMsg fields or an object-number interval.
        if family.class_dti != 0x019a_5b64 { return Ok(None); }
        let object = family.name.strip_prefix("om").ok_or("door family has no om prefix")?
            .parse().map_err(|_| "invalid door object number")?;
        Ok(Some(Self { id: Id { stage, group, record: record.id }, object, transform, record }))
    }
}

#[derive(Debug, Clone)]
pub struct Resources {
    pub model: String,
    pub motion: String,
    pub collision: [String; 2],
}

/// 0x0049edf0 rounds the final decimal object digit down to 0 or 5 for
/// the folder; 0x00c50da0/0x00c51700 build the model/motion/h00/e00 paths.
/// Paths only: missing optional collision resources are not fabricated.
pub fn resources(object: u32) -> Resources {
    let folder = object - object % 5;
    let root = format!(r"model\om\om{folder:04}");
    Resources {
        model: format!(r"{root}\model\om{object:04}"),
        motion: format!(r"{root}\motion\om{object:04}"),
        collision: [format!(r"{root}\collision\om{object:04}_h00"),
            format!(r"{root}\collision\om{object:04}_e00")],
    }
}

pub fn groups(vfs: &ddvfs::Vfs, stage: u32) -> Result<GroupList, String> {
    let path = format!(r"scr\st{stage:03}\etc\st{stage:03}_p");
    GroupList::parse(&vfs.read(&path, mtarc::type_hash("rLayoutGroupParamList"))?).map_err(|e| e.to_string())
}

#[derive(Debug, Default)]
pub struct Candidates {
    pub gates: Vec<(u32, Gate)>,
    pub doors: Vec<Placement>,
}

/// cLayoutSetOm vf12 (vtable0x01593fa8) shares 0x0075faa0's group gate.
/// This returns eligible authored candidates, not a claim that the
/// original's persistent object records or archive readiness permit spawn.
pub fn candidates(vfs: &ddvfs::Vfs, families: &Families, stage: u32, groups: &GroupList,
    cells: &[(u16, u16)], world: &World<'_>) -> Result<Candidates, String> {
    let mut out = Candidates::default();
    for group in &groups.groups {
        let matching: Vec<_> = group.cells.iter().filter(|c| cells.contains(&(c.m, c.n))).collect();
        if matching.is_empty() { continue; }
        let gate = ddspawn::gate(group, world);
        let spawn = gate == Gate::Spawn;
        out.gates.push((group.slot, gate));
        if !spawn { continue; }
        for cell in matching {
            let path = ddspawn::layout_path(stage, cell.m, cell.n, 'p', group.slot);
            let bytes = vfs.read(&path, mtarc::type_hash("rLayout"))?;
            for record in mtlot::parse(&bytes).map_err(|e| format!("{path}: {e}"))? {
                if let Some(door) = Placement::from_record(stage, group.slot, record, families)? {
                    out.doors.push(door);
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn name_resolution_longest_prefix_first_tie() {
        let f = Families(vec![Family { name: "".into(), class_dti: 0 },
            Family { name: "om7".into(), class_dti: 1 },
            Family { name: "om7121".into(), class_dti: 2 },
            Family { name: "OM7121".into(), class_dti: 3 }]);
        assert_eq!(f.select("OM7121_left").unwrap().class_dti, 2);
        assert!(f.select("other").is_none());
    }
}
