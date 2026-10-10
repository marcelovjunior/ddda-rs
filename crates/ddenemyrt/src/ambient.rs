//! Ambient enemy identities and their installed resources. These are actors,
//! not door-owned occupancy proxies. Motion/AI execution is still pending.

/// Concrete unit identity; a layout class alone does not identify a variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Species {
    Em8300,
    Em8602,
}

impl Species {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "em8300" => Some(Self::Em8300),
            "em8602" => Some(Self::Em8602),
            _ => None,
        }
    }

    /// The byte at unit +0x2d, not the rLayout record kind or base class.
    // Translated from 0x00a9bd00 / 0x00aa0000 (DDDA.exe 2017-12-13 Steam).
    pub fn unit_kind(self) -> u8 {
        match self {
            Self::Em8300 => 0x65,
            Self::Em8602 => 0x6d,
        }
    }

    /// vf33's ordinary think root. Em8300's explicitly selected quest
    /// tables are separate and must not silently fall back to this root.
    // Translated from 0x00a9be40 / 0x00aa1b80 (DDDA.exe 2017-12-13 Steam).
    pub fn default_think_root(self) -> u32 {
        match self {
            Self::Em8300 => 0x0164_e5d0,
            Self::Em8602 => 0x0164_5228,
        }
    }

    /// Resource-list rows passed to 0x008892b0 by the unit's vf33.
    /// Paths and class names are bulk data, read from the installed exe.
    pub fn resources(self, exe: &ddexe::Exe<'_>) -> Result<Vec<Resource>, String> {
        let (base, count) = match self {
            Self::Em8300 => (0x0186_1b68, 17),
            Self::Em8602 => (0x0186_27e0, 15),
        };
        let names = exe.class_names();
        (0..count)
            .map(|i| {
                let row = base + i * 12;
                let path = exe.u32(row)?;
                let dti = exe.u32(row + 4)?;
                Ok(Resource {
                    path: exe.cstr(path)?.to_owned(),
                    class: names
                        .get(&dti)
                        .ok_or_else(|| format!("ambient resource: unknown DTI {dti:#x}"))?
                        .clone(),
                })
            })
            .collect()
    }

    /// Family action descriptors passed to 0x007f19c0 (stride 0x44).
    /// vf33 counts are 18 / 15; DTI and the action-specific argument are the first two words.
    /// These offsets describe the file-backed descriptors; do not substitute
    /// the layout of an allocated cAction object (its motion is at +0x48).
    pub fn actions(self, exe: &ddexe::Exe<'_>) -> Result<Vec<Action>, String> {
        let (base, count) = match self {
            Self::Em8300 => (0x0186_16a0, 18),
            Self::Em8602 => (0x0186_2270, 15),
        };
        let names = exe.class_names();
        (0..count)
            .map(|i| {
                let row = base + i * 0x44;
                let dti = exe.u32(row)?;
                Ok(Action {
                    class: names
                        .get(&dti)
                        .ok_or_else(|| format!("ambient action: unknown DTI {dti:#x}"))?
                        .clone(),
                    argument: exe.u32(row + 4)?,
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    pub path: String,
    pub class: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub class: String,
    pub argument: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concrete_bird_kind_is_not_its_parent_kind() {
        assert_eq!(Species::from_name("em8602").unwrap().unit_kind(), 0x6d);
        assert_ne!(Species::Em8602.unit_kind(), 0x6b);
        assert_eq!(Species::Em8300.unit_kind(), 0x65);
        assert_eq!(Species::from_name("em8600"), None);
    }
}
