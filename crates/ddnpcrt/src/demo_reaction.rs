//! Native Demo damage-action phases, independent of combat valuation.
//!
//! This helper is not connected to the host. Its requests still require the
//! actual body adapter: common damage entry (including goods/collision flags),
//! motion launch, physics and horizontal-velocity update are not performed
//! here. Call `step` after the common cPlAction frame update and supply that
//! body's current layer flags/ground byte. Unknown observations cannot land
//! or finish an action. No elapsed-time completion or HP write exists here.
// Translated from 0x0080E4E0/0x0080E4B0 and 0x0080E520/0x0080E560
// (DDDA.exe 2017-12-13 Steam). Native common calls are 0x0080E190 and
// 0x00B73490; their effects remain explicit adapter requests.

use crate::Error;

/// Exact uNpc::vf36 arguments, in native order; motion includes its bank.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    pub number: u16,
    pub force: bool,
    pub flags: u32,
    pub interpolation: f32,
    pub start_frame: f32,
    pub rate: f32,
}

fn motion(number: u16, force: bool, flags: u32, interpolation: f32) -> Request {
    Request::Motion(Motion {
        number,
        force,
        flags,
        interpolation,
        start_frame: 0.0,
        rate: 1.0,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// Adapter must execute native 0x0080E190, including 0x0080D500.
    /// This is not permission to manufacture ground or held-goods facts.
    DamageBaseEntry,
    Motion(Motion),
    /// Body+0x3D08/0x3D0C and the Y lanes at +0x2530/+0x2540.
    /// All values come from the body's damage adjustment slots18/19/1A.
    Launch {
        horizontal_speed: f32,
        vertical_speed: f32,
        vertical_acceleration: f32,
    },
    /// Zero the native horizontal scalar/acceleration and velocity vectors.
    StopVelocity,
    /// Masked writes to body+0x273C; preserve every unrelated bit.
    BodyFlags {
        mask: u32,
        value: bool,
    },
    /// Set body+0x1EE4 to zero on actual landing.
    ClearLandingByte,
    /// Set bit0 of body+0x35E8 after the landing motion finishes.
    MarkLandingMotionFinished,
    /// Native 0x00B73490, including yaw projection and scalar deceleration.
    /// A real physics adapter must port/perform this call, not discard it.
    UpdateHorizontalVelocity,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Inputs {
    /// Actual layer flags. Native 0x01057100 tests flags &5, not a timer.
    pub layer_flags: Option<u32>,
    /// Actual body+0x1ED0 byte; the landing branch requires exactly1.
    pub ground_byte: Option<u8>,
    /// Actual body+0x2E90. The authored Down property initializes it;
    /// the action reads the body's current value at the landing endpoint.
    pub down: Option<bool>,
    /// Actual damage adjustment values from body+0x3348, slots18/19/1A.
    /// Required only on the initial blow update, never replaced by defaults.
    pub blow_adjustment: Option<[f32; 3]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Extra,
    Launch,
    InitialMotion,
    Airborne,
    LandingMotion,
    RecoveryMotion,
    Finished,
    Down,
}

pub struct Reaction {
    phase: Phase,
    /// Native cEventAct+0x50, distinct from the blow-specific +0x7C.
    done: bool,
    down_ready: bool,
}

impl Reaction {
    /// Ordered entry requests. The adapter must run the common base entry
    /// before the motion request; this helper does not prove it happened.
    pub fn start(kind: u32) -> Result<(Self, Vec<Request>), Error> {
        let (phase, entry_motion) = match kind {
            3 => (Phase::Extra, motion(0x115, false, 0, 0.0)), // 0x0080E4E0
            4 => (Phase::Launch, motion(0x102, true, 1, 0.0)), // 0x0080E520
            _ => return Err(Error::Unimplemented(format!("Demo reaction action kind{kind}"))),
        };
        Ok((
            Self {
                phase,
                done: false,
                down_ready: false,
            },
            vec![Request::DamageBaseEntry, entry_motion],
        ))
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn done(&self) -> bool {
        self.done
    }
    pub fn down_ready(&self) -> bool {
        self.down_ready
    }

    /// Native 0x0056EBB0: either action+0x50, or kind4's authored Down
    /// condition together with action+0x7C. This never sets a death flag.
    pub fn interface_done(&self, authored_down: bool) -> bool {
        self.done || (authored_down && self.down_ready)
    }

    pub fn step(&mut self, inputs: Inputs) -> Result<Vec<Request>, Error> {
        let terminal = inputs.layer_flags.is_some_and(|flags| flags & 5 != 0);
        let mut requests = Vec::new();
        if self.phase == Phase::Extra {
            if terminal {
                requests.push(Request::BodyFlags {
                    mask: 0x10,
                    value: false,
                });
                self.done = true;
            }
            return Ok(requests);
        }
        match self.phase {
            Phase::Launch => {
                let [horizontal_speed, vertical_speed, vertical_acceleration] = inputs
                    .blow_adjustment
                    .filter(|values| values.iter().all(|value| value.is_finite()))
                    .ok_or_else(|| {
                        Error::Data("Blow action needs actual damage adjustment slots18/19/1A".into())
                    })?;
                requests.push(Request::Launch {
                    horizontal_speed,
                    vertical_speed,
                    vertical_acceleration,
                });
                requests.push(Request::BodyFlags {
                    mask: 1,
                    value: false,
                });
                self.phase = Phase::InitialMotion;
            }
            Phase::InitialMotion if terminal => {
                requests.push(motion(0x10A, true, 1, 0.0));
                self.phase = Phase::Airborne;
            }
            Phase::Airborne if inputs.ground_byte == Some(1) => {
                requests.push(Request::StopVelocity);
                requests.push(motion(0x10B, true, 0, 4.0));
                requests.push(Request::BodyFlags { mask: 1, value: true });
                requests.push(Request::ClearLandingByte);
                self.phase = Phase::LandingMotion;
            }
            Phase::LandingMotion if terminal => {
                let down = inputs
                    .down
                    .ok_or_else(|| Error::Data("Blow landing needs actual body Down value".into()))?;
                requests.push(Request::MarkLandingMotionFinished);
                if down {
                    self.down_ready = true;
                    self.phase = Phase::Down;
                } else {
                    requests.push(motion(0x103, true, 0, 10.0));
                    self.phase = Phase::RecoveryMotion;
                }
            }
            Phase::RecoveryMotion if terminal => {
                requests.push(Request::BodyFlags {
                    mask: 0x10,
                    value: false,
                });
                self.done = true;
                self.down_ready = true;
                self.phase = Phase::Finished;
            }
            _ => {}
        }
        // 0x0080E560 calls this in every blow phase, including5/10.
        requests.push(Request::UpdateHorizontalVelocity);
        Ok(requests)
    }
}
