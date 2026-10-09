//! Wolf and harpy table registration. Bulk records remain in the installed exe.
use super::Tables;
use crate::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Wolf,
    Harpy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub class: String,
    /// Registration word +4, passed to action vf13 by 0x007f1ea0.
    pub variant: u32,
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
    /// field begins the 0x44-byte registration record.
    pub fn action_classes(self, exe: &ddexe::Exe<'_>) -> Result<Vec<String>, Error> {
        Ok(self.actions(exe)?.into_iter().map(|a| a.class).collect())
    }

    pub fn actions(self, exe: &ddexe::Exe<'_>) -> Result<Vec<Action>, Error> {
        let (first_dti, count) = match self {
            Self::Wolf => (0x0183_86b0, 113),
            Self::Harpy => (0x0184_1370, 76),
        };
        let names = exe.class_names();
        (0..count)
            .map(|i| {
                let dti = exe.u32(first_dti + i * 0x44).map_err(|e| Error::Data(e.0))?;
                let class = names.get(&dti).cloned().ok_or_else(|| {
                    Error::Data(format!("{self:?} action {i}: no class for DTI {dti:#010x}"))
                })?;
                let variant = exe.u32(first_dti + i * 0x44 + 4).map_err(|e| Error::Data(e.0))?;
                Ok(Action { class, variant })
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

/// Inputs for the first nine harpy think bytes. The unit writes these
/// from its current action and the preceding action's movement flags.
/// Later sensor/status bytes remain the caller's responsibility.
#[derive(Debug, Clone, Copy)]
pub struct HarpyThinkInputs {
    pub grounded: bool,
    /// Unit +0x2d70: 1 suspicious, 2 combat, otherwise idle.
    pub alert: u32,
    pub action: i32,
    /// Unit +0x5ca8: bit0 airborne movement, bit1 the second movement mode.
    pub movement_flags: u32,
    /// Result of shared helper 0x008ec740 (classification not yet named).
    pub movement_class: i32,
}

impl HarpyThinkInputs {
    // Translated from 0x0093b470 and action query 0x009381e0
    // (DDDA.exe 2017-12-13 Steam).
    pub fn write_flags(self, previous: u32) -> u32 {
        let mut flags = previous & !0x1ff;
        flags |= u32::from(self.grounded);
        flags |= match self.alert {
            1 => 1 << 2,
            2 => 1 << 3,
            _ => 1 << 1,
        };
        let hover = matches!(self.action, 15..=21 | 31 | 70..=72);
        flags |= u32::from(hover) << 4;
        flags |= (self.movement_flags & 1) << 5;
        flags |= u32::from(self.movement_class == 1) << 6;
        flags |= u32::from(self.movement_class == 2) << 7;
        flags |= ((self.movement_flags >> 1) & 1) << 8;
        flags
    }
}

/// Wolf-local checks. Sensor/joint/line-of-sight queries remain host work.
#[derive(Debug, Clone, Copy, Default)]
pub struct WolfChecks {
    /// Living same-class sensor results counted by 0x008a9f00; excludes
    /// the querying wolf, which check 0 explicitly adds back.
    pub living_packmates: u32,
    pub subtype: u8,
    pub motion: u16,
    /// Unit +0x604e, consumed once by check 6.
    pub pending_notice: bool,
}

impl WolfChecks {
    // Translated from 0x008f7440 (DDDA.exe 2017-12-13 Steam).
    pub fn check(&mut self, id: u32, arg: u32) -> Option<bool> {
        match id {
            0 => Some(self.living_packmates.wrapping_add(1) <= arg),
            2 => Some(u32::from(self.subtype) == arg),
            4 => Some(u32::from(self.motion) == arg),
            6 => Some(std::mem::take(&mut self.pending_notice)),
            7 => Some(matches!(self.subtype, 10 | 0x91)),
            1 | 3 | 5 | 1000.. => None,
            _ => Some(false),
        }
    }
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
    fn harpy_think_flags_follow_action_and_previous_flight_mode() {
        let mut input = HarpyThinkInputs {
            grounded: false,
            alert: 2,
            action: 29,
            movement_flags: 1,
            movement_class: 0,
        };
        assert_eq!(input.write_flags(0xffff_ffff), 0xffff_fe00 | (1 << 3) | (1 << 5));
        input.action = 15;
        assert_ne!(input.write_flags(0) & (1 << 4), 0);
        input.action = 22;
        assert_eq!(input.write_flags(0) & (1 << 4), 0);
        input.grounded = true;
        input.alert = 1;
        input.movement_flags = 2;
        input.movement_class = 2;
        assert_eq!(input.write_flags(0), 1 | (1 << 2) | (1 << 7) | (1 << 8));
    }

    #[test]
    fn wolf_pack_query_counts_self_and_notice_is_consumed() {
        let mut wolf = WolfChecks {
            living_packmates: 2,
            subtype: 8,
            motion: 0x101,
            pending_notice: true,
        };
        assert_eq!(wolf.check(0, 2), Some(false));
        assert_eq!(wolf.check(0, 3), Some(true));
        assert_eq!(wolf.check(2, 8), Some(true));
        assert_eq!(wolf.check(4, 0x101), Some(true));
        assert_eq!(wolf.check(6, 0), Some(true));
        assert_eq!(wolf.check(6, 0), Some(false));
        assert_eq!(wolf.check(7, 0), Some(false));
        wolf.subtype = 10;
        assert_eq!(wolf.check(7, 0), Some(true));
        assert_eq!(wolf.check(3, 0), None);
    }

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
