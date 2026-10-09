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

#[derive(Debug, Clone)]
pub struct DragonAiData {
    pub tables: Tables,
    pub action_classes: Vec<String>,
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
        Ok(Self {
            tables,
            action_classes,
        })
    }
}
