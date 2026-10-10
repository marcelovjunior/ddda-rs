//! Ambient enemy identities and their installed resources. These are actors,
//! not door-owned occupancy proxies. Supported action clocks run headless;
//! full world sensing, flight integration and collision remain incomplete.

pub use ddai;

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

    /// Preserve the layout's mUseFirstSetThinkTbl / mStartTableNo.
    /// The bounded em8300 table has eight pointers at 0x0165d670;
    /// unsupported indices are refused instead of substituted or clamped.
    // Translated from 0x00a9be40 / 0x00aa1b80 (DDDA.exe 2017-12-13 Steam).
    pub fn think_root(self, exe: &ddexe::Exe<'_>, first_table: Option<i32>) -> Result<u32, String> {
        if self == Self::Em8300 {
            if let Some(index) = first_table {
                if !(0..8).contains(&index) {
                    return Err(format!(
                        "em8300 first think table {index} outside supported table"
                    ));
                }
                return Ok(exe.u32(0x0165_d670 + index as u32 * 4)?);
            }
        }
        // 8602's vf33 unconditionally installs its fixed table.
        Ok(self.default_think_root())
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
            .map(|i| Action::read(exe, &names, base + i * 0x44))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    pub path: String,
    pub class: String,
}

/// One species' immutable resources shared by every host-owned instance.
/// All paths, motion tracks and action descriptors come from the install.
#[derive(Debug, Clone)]
pub struct Data {
    pub species: Species,
    pub model: String,
    pub skeleton: mtlmt::pose::Skeleton,
    pub banks: Vec<crate::body::MotionBank>,
    pub actions: Vec<Action>,
    pub common_actions: Vec<Action>,
}

impl Data {
    pub fn load(vfs: &ddvfs::Vfs, exe: &ddexe::Exe<'_>, species: Species) -> Result<Self, String> {
        let resources = species.resources(exe)?;
        let model = resources
            .iter()
            .find(|r| r.class == "rModel")
            .ok_or("ambient species has no model")?
            .path
            .clone();
        let skeleton = crate::body::load::skeleton(vfs, &model)?;
        // 0x008892b0 preserves resource-list order for motion banks.
        let banks = resources
            .iter()
            .filter(|r| r.class == "rMotionList")
            .map(|r| crate::body::load::bank(vfs, &r.path))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            species,
            model,
            skeleton,
            banks,
            actions: species.actions(exe)?,
            common_actions: {
                let names = exe.class_names();
                (0..79)
                    .map(|i| Action::read(exe, &names, 0x0186_35d0 + i * 0x44))
                    .collect::<Result<_, _>>()?
            },
        })
    }

    pub fn action(&self, id: i32) -> Option<&Action> {
        let bank = (id as u32 >> 24) & 255;
        let index = (id as u32 & 65535) as usize;
        match bank {
            0 => self.actions.get(index),
            1 => self.common_actions.get(index),
            _ => None,
        }
    }

    pub fn motion(&self, id: u32) -> Option<&mtlmt::Motion> {
        self.banks
            .get((id >> 8) as usize)?
            .list
            .motions
            .get((id & 255) as usize)?
            .as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub class: String,
    pub argument: u32,
    /// Descriptor +0x14..+0x20 -> cAction +4..+0x10 (0x004c9e50).
    pub parameters: [u32; 4],
}

impl Action {
    fn read(
        exe: &ddexe::Exe<'_>,
        names: &std::collections::HashMap<u32, String>,
        row: u32,
    ) -> Result<Self, String> {
        let dti = exe.u32(row)?;
        Ok(Self {
            class: names
                .get(&dti)
                .ok_or_else(|| format!("ambient action: unknown DTI {dti:#x}"))?
                .clone(),
            argument: exe.u32(row + 4)?,
            parameters: [
                exe.u32(row + 0x14)?,
                exe.u32(row + 0x18)?,
                exe.u32(row + 0x1c)?,
                exe.u32(row + 0x20)?,
            ],
        })
    }

    /// Shared enemy action bank 1, installed by uEnemy::vf05 0x00aa8830.
    /// Count 79 at 0x00aa88aa. Encoded action bank/index split: 0x007f1ea0.
    pub fn common(exe: &ddexe::Exe<'_>, index: u32) -> Result<Self, String> {
        if index >= 79 {
            return Err(format!("common enemy action {index} outside bank"));
        }
        Self::read(exe, &exe.class_names(), 0x0186_35d0 + index * 0x44)
    }
}

/// Mutable action/think/motion state owned by one canonical ambient actor.
/// Target slots and condition flags are supplied by the host, not by rendering.
#[derive(Debug, Clone, PartialEq)]
pub struct Runtime {
    pub think: ddai::thinktbl::ThinkMgr,
    pub action: i32,
    pub motion: u32,
    pub frame: f32,
    pub motion_ended: bool,
    pub done: bool,
    pub body_flags: u32,
    pub retained_wait: bool,
    pub clear_body_motion_bytes: bool,
}

impl Runtime {
    // Translated from vf33 0x00a9be40 / 0x00aa1b80
    // (DDDA.exe 2017-12-13 Steam): initial action request and selected root.
    pub fn new(data: &Data, root: u32) -> Result<Self, String> {
        let mut state = Self {
            think: ddai::thinktbl::ThinkMgr::new(),
            action: -1,
            motion: 0,
            frame: 0.0,
            motion_ended: false,
            done: false,
            body_flags: 0,
            retained_wait: false,
            clear_body_motion_bytes: false,
        };
        let initial = match data.species {
            Species::Em8300 => 0x0100_0001,
            Species::Em8602 => 0x0100_0003,
        };
        // Initial common Wait enters before vf33 installs the table.
        state.start(data, initial)?;
        state.think.request_table(root);
        Ok(state)
    }

    pub fn start(&mut self, data: &Data, action: i32) -> Result<(), String> {
        let definition = data
            .action(action)
            .ok_or_else(|| format!("ambient action descriptor {action:#x} unavailable"))?;
        let request = enter_action(definition)
            .ok_or_else(|| format!("ambient action {action:#x} {} unimplemented", definition.class))?;
        if data.motion(request.id).is_none() {
            return Err(format!("ambient motion {:#x} unavailable", request.id));
        }
        // uObjModel::vf36 0x00886e70: all supported entries pass force=0,
        // so re-entering an action with the same motion preserves its clock.
        if self.action == -1 || self.motion != request.id {
            self.motion = request.id;
            self.frame = request.start_frame;
            self.motion_ended = false;
        }
        self.action = action;
        self.done = false;
        self.retained_wait = self.think.f120f;
        self.clear_body_motion_bytes = request.clear_body_motion_bytes;
        Ok(())
    }

    /// Ordinary no-damage/no-forced-action path. The caller must defer to
    /// those higher-priority controllers when they are active.
    // Translated from 0x00a9c7d0 / 0x00aa05c0 (DDDA.exe 2017-12-13 Steam).
    pub fn select(
        &mut self,
        data: &Data,
        tables: &ddai::thinktbl::Tables,
        callbacks: &mut dyn ddai::thinktbl::Host,
    ) -> Result<(), String> {
        if !self.done && !self.think.f1210 {
            return Ok(());
        }
        self.think.f1211 = self.done;
        self.think.reload_default();
        let next = self.think.think(tables, callbacks).map_err(|e| e.to_string())?;
        if next != -1 && (next != self.action || self.think.ea5) {
            self.start(data, next)?;
        }
        Ok(())
    }

    /// Phase 91 action update. Completion is latched until a new action,
    /// matching the source callbacks which only ever set their done byte.
    pub fn step_action(&mut self, data: &Data) -> Result<(), String> {
        let definition = data.action(self.action).ok_or("ambient action missing")?;
        let result = if definition.class == "cEmActWait" {
            step_common_wait(self.retained_wait, self.motion_ended)
        } else {
            step_action(&definition.class, self.motion_ended)
                .ok_or_else(|| format!("ambient action step {} unimplemented", definition.class))?
        };
        self.done |= result.done;
        self.body_flags |= result.body_flags;
        Ok(())
    }

    /// Phase 92; canonical clock and installed root travel, before collision.
    // Translated from 0x010629f0 / 0x0105b740 (DDDA.exe 2017-12-13 Steam).
    pub fn advance(
        &mut self,
        data: &Data,
        position: &mut [f32; 3],
        yaw: f32,
        delta: f32,
    ) -> Result<(), String> {
        let motion = data.motion(self.motion).ok_or("ambient motion missing")?;
        let root = ddsim::RootMotion::from_motion(motion);
        let looping = motion.loop_frame >= 0;
        let travel = if looping {
            root.delta(self.frame, delta)
        } else {
            root.delta_once(self.frame, delta)
        };
        let (sin, cos) = yaw.sin_cos();
        position[0] += travel[0] * cos + travel[2] * sin;
        position[1] += travel[1];
        position[2] += -travel[0] * sin + travel[2] * cos;
        let last = motion.frame_count.saturating_sub(1) as f32;
        let next = self.frame + delta;
        if looping {
            let start = motion.loop_frame as f32;
            self.frame = if next > last {
                start + (next - last).rem_euclid((last - start).max(1.0))
            } else {
                next
            };
            self.motion_ended = next > last;
        } else {
            self.frame = next.min(last);
            self.motion_ended = next >= last;
        }
        Ok(())
    }
}

/// A sourced motion request emitted on entry to a supported ambient action.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionRequest {
    pub id: u32,
    pub start_frame: f32,
    pub blend_frames: f32,
    pub rate: f32,
    /// WaitAir and FlyStart clear the body's +0x1b6d / +0x1e80 bytes.
    /// Their wider physical meaning remains unresolved.
    pub clear_body_motion_bytes: bool,
}

/// Supported family actions only. Unsupported classes return None rather
/// than playing a guessed idle or reporting a completed action.
pub fn enter_action(action: &Action) -> Option<MotionRequest> {
    // Translated from 0x00a9dcf0, 0x00aa0d40, 0x00aa0e20
    // (DDDA.exe 2017-12-13 Steam). FlyStart inherits WaitAir's vf05.
    let clear_body_motion_bytes = match action.class.as_str() {
        "cEm8300ActWait" | "cEm8600ActWait" | "cEmActWait" => false,
        "cEm8600ActWaitAir" | "cEm8600ActFlyStart" => true,
        _ => return None,
    };
    Some(MotionRequest {
        id: if action.class == "cEmActWait" {
            action.parameters[0]
        } else {
            action.argument
        },
        start_frame: 0.0,
        blend_frames: 5.0,
        rate: 1.0,
        clear_body_motion_bytes,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionStep {
    pub done: bool,
    /// OR into both body +0x2360 and +0x2370 (not a replacement).
    pub body_flags: u32,
}

/// `motion_ended` is the engine's motion flag (+6 bit2, 0x010570f0),
/// not elapsed wall time. A looping motion and a completed action differ.
pub fn step_action(class: &str, motion_ended: bool) -> Option<ActionStep> {
    // Translated from vf06 entries at 0x00907620, 0x009c5b10,
    // 0x0095bb80 (DDDA.exe 2017-12-13 Steam). These shared functions have
    // Ghidra names from unrelated families; ambient vtables select them.
    Some(match class {
        "cEm8300ActWait" => ActionStep {
            done: motion_ended,
            body_flags: 1,
        },
        "cEm8600ActWait" | "cEm8600ActWaitAir" => ActionStep {
            done: true,
            body_flags: 0,
        },
        "cEm8600ActFlyStart" => ActionStep {
            done: motion_ended,
            body_flags: 0,
        },
        _ => return None,
    })
}

/// cEmActWait samples think +0x120f on entry, then checks that retained
/// value every step. It must not use the current think flag retroactively.
// Translated from 0x0089c2f0 / 0x0089c3b0 (DDDA.exe 2017-12-13 Steam).
pub fn step_common_wait(wait_for_motion_end_on_entry: bool, motion_ended: bool) -> ActionStep {
    ActionStep {
        done: !wait_for_motion_end_on_entry || motion_ended,
        body_flags: 1,
    }
}

/// Mutable position/heading of an ambient bird's FlyLoop action.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flight {
    pub position: [f32; 3],
    pub yaw: f32,
    pub phase: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct FlightInputs {
    /// Unit delta in 30-Hz frame units, not seconds.
    pub delta: f32,
    /// Unit +0x6024, established by the think result's height + initial y.
    pub target_height: f32,
    /// Unit +0x602c, sampled as rand * 6.5 + 18.5 by 0x00aa05c0.
    pub speed: f32,
    /// Unit +0x5c10, updated from the selected target by 0x00aa09b0.
    pub target_yaw: f32,
    pub body_status: u32,
    pub motion_ended: bool,
}

impl Flight {
    /// FlyLoop vf06 position update. Target selection is supplied by the
    /// host; this function never invents a player or a target position.
    /// The phase increment really is per action update, not delta-scaled.
    // Translated from 0x00aa1030 and turn helper 0x00aa0a80
    // (DDDA.exe 2017-12-13 Steam), checked against the disassembly.
    pub fn step(&mut self, inputs: FlightInputs) -> ActionStep {
        if inputs.body_status & (0x800 | 0x1000) != 0 {
            return ActionStep {
                done: false,
                body_flags: 0,
            };
        }
        let (sin, cos) = self.yaw.sin_cos();
        self.position[0] += sin * inputs.speed * inputs.delta;
        self.position[2] += cos * inputs.speed * inputs.delta;
        let distance = inputs.target_height - self.position[1];
        if distance.abs() >= f32::EPSILON {
            let step = ((-1.0 / distance.abs()) * 16.0 * inputs.delta + 4.0).max(0.0) * inputs.delta;
            self.position[1] += if distance < 0.0 { -step } else { step };
        }
        let difference = inputs.target_yaw - self.yaw;
        let mut wrapped = difference - (difference * 0.159_154_94) as i32 as f32 * std::f32::consts::TAU;
        if difference < 0.0 {
            wrapped += std::f32::consts::TAU;
        }
        if wrapped >= std::f32::consts::PI {
            wrapped -= std::f32::consts::TAU;
        }
        let turn = (wrapped * 0.125 * inputs.delta).clamp(-0.418_879_03, 0.418_879_03);
        self.yaw = if wrapped.abs() - turn.abs() <= 0.0 {
            inputs.target_yaw
        } else {
            self.yaw + turn
        };
        // f64 add and f32 round before sin, at 0x00aa1249..0x00aa125d.
        self.phase = (f64::from(self.phase) + 0.139_626_344_045_003_26) as f32;
        self.position[1] += self.phase.sin();
        ActionStep {
            done: inputs.motion_ended,
            body_flags: 0,
        }
    }
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

    #[test]
    fn bird_wait_can_rethink_before_its_motion_ends() {
        assert!(step_common_wait(false, false).done);
        assert!(!step_common_wait(true, false).done);
        assert!(step_common_wait(true, true).done);
        let bird = step_action("cEm8600ActWait", false).unwrap();
        assert!(bird.done);
        assert_eq!(bird.body_flags, 0);
        let ground_wait = step_action("cEm8300ActWait", false).unwrap();
        assert!(!ground_wait.done);
        assert_eq!(ground_wait.body_flags, 1);
        assert!(step_action("cEm8300ActWait", true).unwrap().done);
        assert!(!step_action("cEm8600ActFlyStart", false).unwrap().done);
        assert!(step_action("cEm8600ActFlyStart", true).unwrap().done);
        assert_eq!(step_action("cEm8600ActFlyLoop", true), None);
    }

    #[test]
    fn bird_takeoff_uses_descriptor_motion_and_air_entry() {
        let action = Action {
            class: "cEm8600ActFlyStart".into(),
            argument: 0x96,
            parameters: [0; 4],
        };
        let request = enter_action(&action).unwrap();
        assert_eq!(request.id, 0x96);
        assert_eq!(
            (request.start_frame, request.blend_frames, request.rate),
            (0.0, 5.0, 1.0)
        );
        assert!(request.clear_body_motion_bytes);
        assert_eq!(
            enter_action(&Action {
                class: "cEmActDmgShrink".into(),
                argument: 1,
                parameters: [0; 4]
            }),
            None
        );
    }

    #[test]
    fn flight_uses_old_heading_before_turn_and_holds_when_disabled() {
        let mut flight = Flight {
            position: [0.0; 3],
            yaw: 0.0,
            phase: 0.0,
        };
        let mut input = FlightInputs {
            delta: 1.0,
            target_height: 100.0,
            speed: 20.0,
            target_yaw: std::f32::consts::FRAC_PI_2,
            body_status: 0x800,
            motion_ended: true,
        };
        let original = flight;
        assert!(!flight.step(input).done);
        assert_eq!(flight, original);
        input.body_status = 0;
        assert!(flight.step(input).done);
        assert_eq!(flight.position[0], 0.0);
        assert_eq!(flight.position[2], 20.0);
        assert!((flight.yaw - std::f32::consts::FRAC_PI_2 * 0.125).abs() < 1e-6);
        assert!((flight.position[1] - (3.84 + flight.phase.sin())).abs() < 1e-6);
    }
}
