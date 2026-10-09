//! Wolf and harpy table registration. Bulk records remain in the installed exe.
use super::Tables;
use crate::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Wolf,
    Harpy,
}

impl Family {
    /// Constructor defaults: uEmWolfBase::vf33 0x008f06f0 and
    /// uEm0600::vf33 0x0093cbd0 (2017-12-13 Steam).
    pub fn default_table(self) -> u32 {
        match self {
            Self::Wolf => 0x017a_b188,
            Self::Harpy => 0x0177_3ec0,
        }
    }

    /// Reads the default graph and directly installed grabbed table.
    /// Indexed placement overrides are not exposed by this entry point.
    pub fn tables(self, exe: &ddexe::Exe<'_>) -> Result<Tables, Error> {
        // 0x008f3830 / 0x0093b470 pass these to common grabbed-table
        // selection 0x00aad940 immediately before running cThinkMgr.
        let grabbed = match self {
            Self::Wolf => 0x017c_27b8,
            Self::Harpy => 0x0177_9c20,
        };
        Tables::read_roots(exe, vec![self.default_table()], &[grabbed])
    }

    /// Action registration 0x008f06f0 / 0x0093a6d0 calls 0x007f19c0
    /// with EDI counts 113 / 76 and a pointer to the DTI field. That
    /// field advances by 0x44 bytes per action; it is not the record base.
    pub fn action_classes(self, exe: &ddexe::Exe<'_>) -> Result<Vec<String>, Error> {
        let (first_dti, count) = match self {
            Self::Wolf => (0x0183_86b0, 113),
            Self::Harpy => (0x0184_1370, 76),
        };
        let names = exe.class_names();
        (0..count)
            .map(|i| {
                let dti = exe.u32(first_dti + i * 0x44).map_err(|e| Error::Data(e.0))?;
                names
                    .get(&dti)
                    .cloned()
                    .ok_or_else(|| Error::Data(format!("{self:?} action {i}: no class for DTI {dti:#010x}")))
            })
            .collect()
    }
}

/// Harpy unit callbacks below 1000; common world queries use 0x008ab100.
/// +0x62b0 is the clamped state set by callback command 0; +0x5860 is
/// passed explicitly until its gameplay meaning has been established.
#[derive(Debug, Clone, Copy, Default)]
pub struct HarpyChecks {
    pub state: u32,
    pub value_5860: f32,
}

impl HarpyChecks {
    // Translated from 0x0093bd60 (DDDA.exe 2017-12-13 Steam).
    pub fn check(&self, id: u32, arg: u32) -> Option<bool> {
        match id {
            0 => Some(self.state == 0),
            1 => Some(f64::from(self.value_5860) >= f64::from(arg)),
            2..1000 => Some(false),
            _ => None,
        }
    }

    // Translated from 0x0093bda0 (DDDA.exe 2017-12-13 Steam).
    pub fn set(&mut self, id: u32, arg: u32) -> Option<u32> {
        if id >= 1000 {
            return None;
        }
        if id == 0 {
            self.state = arg.min(1);
        }
        Some(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harpy_state_and_unsigned_threshold_callbacks() {
        let mut h = HarpyChecks::default();
        assert_eq!(h.check(0, 99), Some(true));
        assert_eq!(h.set(0, u32::MAX), Some(0));
        assert_eq!(h.state, 1);
        assert_eq!(h.check(0, 0), Some(false));
        h.set(0, 0);
        assert_eq!(h.check(0, 0), Some(true));
        h.value_5860 = 100.0;
        assert_eq!(h.check(1, 100), Some(true));
        assert_eq!(h.check(1, 101), Some(false));
        // x87 converts the unsigned argument exactly, not via f32.
        h.value_5860 = 16_777_216.0;
        assert_eq!(h.check(1, 16_777_217), Some(false));
        h.value_5860 = f32::NAN;
        assert_eq!(h.check(1, 0), Some(false));
        assert_eq!(h.check(2, 0), Some(false));
        assert_eq!(h.check(1000, 0), None);
        assert_eq!(h.set(1000, 0), None);
    }
}
