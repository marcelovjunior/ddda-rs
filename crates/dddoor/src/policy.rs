//! Authored door conditions. Missing observations stay unresolved.

#[derive(Debug, Clone, Copy)]
pub struct Policy {
    pub mode: u32,
    pub argument: u32,
    pub angle_type: u32,
    pub flags: u32,
}
impl Policy {
    /// 0x00c4fec0 copies mFree00/01 into actor+0x2598/+0x259c,
    /// mAngleType to +0x25a0, and mFlag to +0x25b0.
    pub fn from_record(record: &mtlot::Record) -> Result<Self, String> {
        let read = |name| match record.get(name) {
            Some(mtlot::Value::Int(v)) => u32::try_from(*v).map_err(|_| format!("invalid {name}")),
            _ => Err(format!("door record missing {name}")),
        };
        Ok(Self { mode: read("mFree00")?, argument: read("mFree01")?,
            angle_type: read("mAngleType")?, flags: read("mFlag")? })
    }

    // Translated from 0x00c3ef00 (DDDA.exe 2017-12-13 Steam).
    pub fn resolve(&self, state: Observed) -> Option<Availability> {
        if state.permanently_open? { return Some(Availability::Open); }
        if state.permanently_unlocked { return Some(Availability::Unlocked); }
        Some(match self.mode {
            1 => if state.key_present? { Availability::Key } else { Availability::Locked },
            2 => if !state.link_resolved || state.link_present { Availability::Locked } else { Availability::Unlocked },
            3 => if state.progress_gate? { Availability::Unlocked } else { Availability::Locked },
            4 if !(5..=21).contains(&state.hour) && !state.night_bypass => Availability::Night,
            5 if !state.script_controller || !state.script_open => Availability::Locked,
            _ => Availability::Unlocked,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability { Open, Unlocked, Locked, Key, Night }

/// Each observation names a traced input, without using absent state as
/// permission. `fresh` applies only to an explicitly new session.
#[derive(Debug, Clone, Copy)]
pub struct Observed {
    pub permanently_open: Option<bool>,
    pub permanently_unlocked: bool,
    pub key_present: Option<bool>,
    pub link_resolved: bool,
    pub link_present: bool,
    pub progress_gate: Option<bool>,
    pub hour: u32,
    pub night_bypass: bool,
    pub script_controller: bool,
    pub script_open: bool,
}
impl Observed {
    pub fn fresh(hour: u32) -> Self {
        Self { permanently_open: Some(false), permanently_unlocked: false, key_present: None,
            link_resolved: false, link_present: false, progress_gate: None, hour,
            night_bypass: false, script_controller: false, script_open: false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_persistence_is_not_permission_and_night_hours_are_inclusive() {
        let p = Policy { mode: 4, argument: 0, angle_type: 0, flags: 0 };
        let mut observed = Observed::fresh(5);
        assert_eq!(p.resolve(observed), Some(Availability::Unlocked));
        observed.hour = 21;
        assert_eq!(p.resolve(observed), Some(Availability::Unlocked));
        observed.hour = 22;
        assert_eq!(p.resolve(observed), Some(Availability::Night));
        observed.night_bypass = true;
        assert_eq!(p.resolve(observed), Some(Availability::Unlocked));
        observed.permanently_open = None;
        assert_eq!(p.resolve(observed), None);
    }
}

