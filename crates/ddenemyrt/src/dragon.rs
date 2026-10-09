//! Dragon AI definitions read from the player's executable.
//!
//! This is the sequence-4 data boundary, not yet a dragon action simulator.
//! The entrance FSM is interpreted by ddai; movement and attack action
//! implementations must supply its completion and motion feedback.

use ddai::{Error, thinktbl::Tables};

/// uEm5800 initialization 0x00a49f00, sequence-4 branch: assigns this root
/// to the think manager's current, default and base table pointers.
pub const OPENING_THINK_ROOT: u32 = 0x0169_9518;
/// Action descriptors in the supported executable (0x44 bytes each, DTI
/// at +0x28), shared by uEm5800 action dispatch. 0x00a3f89f passes the DTI
/// column 0x0185bd20 to 0x007f19c0, which advances by 0x44 per record.
pub const ACTION_TABLE: u32 = 0x0185_bcf8;
/// Embedded count passed in EDI at 0x00a3f8a4 (DDDA.exe 2017-12-13 Steam).
pub const ACTION_COUNT: u32 = 93;

/// uEm5800::vf05 0x00a49ed0 passes this table and count to 0x00889de0
/// (DDDA.exe 2017-12-13 Steam). Each descriptor occupies three words.
pub const RESOURCE_TABLE: u32 = 0x0185_d5d8;
pub const RESOURCE_COUNT: u32 = 57;

/// A resource registration, preserving executable order without assuming
/// that registration indices are motion-bank or collision-set indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DragonResource {
    pub path: String,
    pub class: String,
    /// Flags after the executable's static initializer has run.
    pub flags: u32,
}

// Translated from 0x01363d50..0x0136412c (DDDA.exe 2017-12-13 Steam).
// Startup enables binding for the first 49 descriptors; the remaining
// effect/sound resources are preload-only. Disk flag words are zero.
fn initialized_resource_flags(index: u32, disk_flags: u32) -> u32 {
    if index < 49 {
        (disk_flags & !4) | 3
    } else {
        (disk_flags & !5) | 2
    }
}

#[derive(Debug, Clone)]
pub struct DragonAiData {
    pub tables: Tables,
    pub action_classes: Vec<String>,
    pub resources: Vec<DragonResource>,
}

impl DragonAiData {
    /// Reads the Cassardis sequence's reachable think records and dragon
    /// action class names. No bulk game records are compiled into the port.
    pub fn opening(exe: &ddexe::Exe<'_>) -> Result<Self, Error> {
        let tables = Tables::read_roots(exe, vec![OPENING_THINK_ROOT], &[])?;
        let names = exe.class_names();
        let mut action_classes = Vec::new();
        // Translated from 0x00a3f89f / 0x007f19c0 (DDDA.exe 2017-12-13 Steam).
        for index in 0..ACTION_COUNT {
            let address = ACTION_TABLE + index * 0x44;
            let dti = exe.u32(address + 0x28).map_err(|e| Error::Data(e.0))?;
            action_classes.push(names.get(&dti).cloned().ok_or_else(|| {
                Error::Data(format!(
                    "dragon action {}: unknown DTI {dti:#x}",
                    action_classes.len()
                ))
            })?);
        }
        let mut resources = Vec::new();
        // Translated from 0x00889de0 (DDDA.exe 2017-12-13 Steam): null
        // path entries are skipped, and surviving entries retain order.
        for index in 0..RESOURCE_COUNT {
            let address = RESOURCE_TABLE + index * 12;
            let path = exe.u32(address).map_err(|e| Error::Data(e.0))?;
            if path == 0 {
                continue;
            }
            let dti = exe.u32(address + 4).map_err(|e| Error::Data(e.0))?;
            resources.push(DragonResource {
                path: exe.cstr(path).map_err(|e| Error::Data(e.0))?.to_owned(),
                class: names
                    .get(&dti)
                    .cloned()
                    .ok_or_else(|| Error::Data(format!("dragon resource {index}: unknown DTI {dti:#x}")))?,
                flags: initialized_resource_flags(index, exe.u32(address + 8).map_err(|e| Error::Data(e.0))?),
            });
        }
        Ok(Self {
            tables,
            action_classes,
            resources,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::initialized_resource_flags;

    #[test]
    fn static_initialization_preserves_unrelated_flag_bits() {
        assert_eq!(initialized_resource_flags(48, 0), 3);
        assert_eq!(initialized_resource_flags(49, 0), 2);
        assert_eq!(initialized_resource_flags(48, 0xffff_ffff), 0xffff_fffb);
        assert_eq!(initialized_resource_flags(49, 0xffff_ffff), 0xffff_fffa);
    }
}
