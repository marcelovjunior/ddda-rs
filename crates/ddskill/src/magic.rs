//! `rMagicActParamList` (`map\0`, version 4). Read from the installation,
//! never an embedded spell table. Serialized order: 0x00CC71A0,
//! 0x00825F80, 0x00826E90 and 0x008253C0 (DDDA.exe 2017-12-13 Steam).
use crate::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct ShellSpec {
    pub group: i32,
    pub index: i32,
    pub position_type: i32,
    pub joint: i32,
    pub offset: [f32; 3],
    pub scale: [f32; 3],
    pub flags: u32,
    pub delay: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    pub chant_frames: f32,
    /// Signed change, usually negative. Applied at release by 0x00B47890.
    pub stamina: f32,
    pub stamina_loop: f32,
    pub motion_speed: f32,
    pub lock_on_count: u32,
    pub lock_on_begin_distance: f32,
    pub lock_on_end_distance: f32,
    pub multi_set_type: i32,
    pub multi_shell_wait: f32,
    pub multi_motion_frame: i32,
    pub multi_motion_blend: i32,
    pub camera: u32,
    pub shells: [ShellSpec; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Magic {
    pub effect: i32,
    pub manual_type: i32,
    pub sound: i32,
    /// Start chant, chant loop, release, end. -1 means no motion.
    pub motions: [i32; 4],
    pub blends: [f32; 4],
    pub move_while_casting: bool,
    pub chant_loop_frame: f32,
    pub chant_joint: i32,
    pub chant_offset: [f32; 3],
    pub flags: u32,
    pub levels: Vec<Level>,
    pub execution_type: i32,
    pub shell_control_type: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MagicList {
    pub entries: Vec<Magic>,
}

/// Cast time with the caller's resolved Articulacy value (augment 81) and
/// actor casting-rate multiplier. Equipment's extra modifier, when present,
/// must already be included by the caller.
// Translated from 0x00B49B60 (DDDA.exe 2017-12-13 Steam).
pub fn chant_time(frames: f32, articulacy: f32, rate: f32) -> f32 {
    ((100.0 - articulacy) * 0.01).max(0.0) * frames * rate
}

/// Signed stamina delta when a spell fires. Conservation is augment 82.
// Translated from 0x00B47CFA..0x00B47D64 (DDDA.exe 2017-12-13 Steam).
pub fn cast_stamina(delta: f32, conservation: f32) -> f32 {
    delta * (1.0 - conservation * 0.01)
}

/// Weapon skills charge when event group 1 bit 30 goes from set to clear.
/// Caller passes the current and previous words of the bound motion layer.
// Translated from 0x00B60F10 (DDDA.exe 2017-12-13 Steam).
pub fn weapon_cost_edge(previous: u32, current: u32) -> bool {
    (current ^ previous) & previous & 0x4000_0000 != 0
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], Error> {
        let end = self
            .at
            .checked_add(n)
            .ok_or_else(|| Error("magic size overflow".into()))?;
        let out = self
            .bytes
            .get(self.at..end)
            .ok_or_else(|| Error(format!("truncated magic at {}", self.at)))?;
        self.at = end;
        Ok(out)
    }
    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32, Error> {
        Ok(self.u32()? as i32)
    }
    fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_bits(self.u32()?))
    }
    fn vec3(&mut self) -> Result<[f32; 3], Error> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }
    fn shell(&mut self) -> Result<ShellSpec, Error> {
        Ok(ShellSpec {
            group: self.i32()?,
            index: self.i32()?,
            position_type: self.i32()?,
            joint: self.i32()?,
            offset: self.vec3()?,
            scale: self.vec3()?,
            flags: self.u32()?,
            delay: self.f32()?,
        })
    }
    fn level(&mut self) -> Result<Level, Error> {
        Ok(Level {
            chant_frames: self.f32()?,
            stamina: self.f32()?,
            stamina_loop: self.f32()?,
            motion_speed: self.f32()?,
            lock_on_count: self.u32()?,
            lock_on_begin_distance: self.f32()?,
            lock_on_end_distance: self.f32()?,
            multi_set_type: self.i32()?,
            multi_shell_wait: self.f32()?,
            multi_motion_frame: self.i32()?,
            multi_motion_blend: self.i32()?,
            camera: self.u32()?,
            shells: [self.shell()?, self.shell()?, self.shell()?],
        })
    }
    fn magic(&mut self) -> Result<Magic, Error> {
        let effect = self.i32()?;
        let manual_type = self.i32()?;
        let sound = self.i32()?;
        let mut motions = [0; 4];
        let mut blends = [0.; 4];
        for i in 0..4 {
            motions[i] = self.i32()?;
            blends[i] = self.f32()?;
        }
        let move_while_casting = match self.take(1)?[0] {
            0 => false,
            1 => true,
            _ => return Err(Error("invalid magic bool".into())),
        };
        self.take(17 * 4)?; // movement / shell-control motions, +58..98
        let flags = self.u32()?;
        let chant_loop_frame = self.f32()?;
        self.take(4)?; // shot loop frame
        let chant_joint = self.i32()?;
        let chant_offset = self.vec3()?;
        let count = self.u32()?;
        if count > 3 {
            return Err(Error(format!(
                "magic level count {count} exceeds the object's three slots"
            )));
        }
        let levels = (0..count).map(|_| self.level()).collect::<Result<Vec<_>, _>>()?;
        let execution_type = self.i32()?;
        let shell_control_type = self.i32()?;
        self.take(40)?; // remaining targeting/camera and notice settings
        Ok(Magic {
            effect,
            manual_type,
            sound,
            motions,
            blends,
            move_while_casting,
            chant_loop_frame,
            chant_joint,
            chant_offset,
            flags,
            levels,
            execution_type,
            shell_control_type,
        })
    }
}

impl MagicList {
    pub const RESOURCE_CLASS: &'static str = "rMagicActParamList";
    pub const STAFF_PATH: &'static str = r"param\mag_act_param_07";
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let mut r = Reader { bytes, at: 0 };
        if r.take(4)? != b"map\0" || r.u32()? != 4 {
            return Err(Error("expected map version 4".into()));
        }
        let count = r.u32()? as usize;
        // A zero-level record still contains 193 bytes. Bound before allocating.
        if count > bytes.len().saturating_sub(12) / 193 {
            return Err(Error("magic count exceeds file".into()));
        }
        let entries = (0..count).map(|_| r.magic()).collect::<Result<Vec<_>, _>>()?;
        if r.at != bytes.len() {
            return Err(Error("trailing magic bytes".into()));
        }
        Ok(Self { entries })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_and_malformed_lists() {
        let mut b = b"map\0\x04\0\0\0\0\0\0\0".to_vec();
        assert!(MagicList::parse(&b).unwrap().entries.is_empty());
        for n in 0..b.len() {
            assert!(MagicList::parse(&b[..n]).is_err());
        }
        b.push(0);
        assert!(MagicList::parse(&b).is_err());
        b.pop();
        b[8] = 255;
        assert!(MagicList::parse(&b).is_err());
    }
    #[test]
    fn cast_modifiers_and_weapon_cost_edge() {
        assert_eq!(chant_time(80.0, 10.0, 0.5), 36.0);
        assert_eq!(chant_time(80.0, 110.0, 1.0), 0.0);
        assert_eq!(cast_stamina(-120.0, 15.0), -102.0);
        assert!(weapon_cost_edge(1 << 30, 0));
        assert!(!weapon_cost_edge(0, 1 << 30));
        assert!(!weapon_cost_edge(1 << 30, 1 << 30));
        assert!(!weapon_cost_edge(1 << 29, 0));
    }
}
