//! One Arisen's replicated state: the public fields of `ddsim::Player`.
//!
//! Three private fields (`sprint_entry`, `jump_held`, the rate queue) stay
//! with the client's own prediction. They depend only on the inputs, which
//! the client also has, and settle once the input stops (ADR 0002 section
//! 8.6).
//!
//! On the wire a body is a mask of field groups and the groups that differ
//! from the baseline body (delta encoding against the snapshot the client
//! acked).

use ddid::{Reader, Result, Writer};
use ddsim::ledge::{Hang, Ledge};
use ddsim::attack::{Attack, ShotPhase};
use ddsim::{GrabPhase, Hold, JumpParam, Player, PlayerId, State};

#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub id: PlayerId,
    pub position: [f32; 3],
    pub yaw: f32,
    pub state: State,
    pub frame: f32,
    pub velocity: [f32; 3],
    pub jump: JumpParam,
    pub jumps_left: u8,
    pub double_jump: bool,
    pub relaxed: bool,
    pub ran_out: bool,
    pub body_class: u8,
    pub load_class: u8,
    pub speed: f32,
    pub drop_from: f32,
    pub stamina: f32,
    pub stamina_max: f32,
    pub unit_rate: f32,
    pub hold: Option<Hold>,
}

impl Body {
    pub fn of(p: &Player) -> Body {
        Body {
            id: p.id,
            position: p.position,
            yaw: p.yaw,
            state: p.state,
            frame: p.frame,
            velocity: p.velocity,
            jump: p.jump,
            jumps_left: p.jumps_left,
            double_jump: p.double_jump,
            relaxed: p.relaxed,
            ran_out: p.ran_out,
            body_class: p.body_class as u8,
            load_class: p.load_class as u8,
            speed: p.speed,
            drop_from: p.drop_from,
            stamina: p.stamina,
            stamina_max: p.stamina_max,
            unit_rate: p.unit_rate,
            hold: p.hold,
        }
    }

    /// Sets a simulated player to this body (the authority's state).
    pub fn apply(&self, p: &mut Player) {
        p.position = self.position;
        p.yaw = self.yaw;
        p.state = self.state;
        p.frame = self.frame;
        p.velocity = self.velocity;
        p.jump = self.jump;
        p.jumps_left = self.jumps_left;
        p.double_jump = self.double_jump;
        p.relaxed = self.relaxed;
        p.ran_out = self.ran_out;
        p.body_class = self.body_class as usize;
        p.load_class = self.load_class as usize;
        p.speed = self.speed;
        p.drop_from = self.drop_from;
        p.stamina = self.stamina;
        p.stamina_max = self.stamina_max;
        p.unit_rate = self.unit_rate;
        p.hold = self.hold;
    }

    /// A body between `a` and `b` at `t` in 0..1, for showing other
    /// players behind time. Position and yaw blend; the rest is `a`'s
    /// until `t` passes one half.
    pub fn lerp(a: &Body, b: &Body, t: f32) -> Body {
        let mut out = if t < 0.5 { a.clone() } else { b.clone() };
        out.position = std::array::from_fn(|i| a.position[i] + (b.position[i] - a.position[i]) * t);
        let mut d = (b.yaw - a.yaw) % std::f32::consts::TAU;
        if d > std::f32::consts::PI {
            d -= std::f32::consts::TAU;
        } else if d < -std::f32::consts::PI {
            d += std::f32::consts::TAU;
        }
        out.yaw = a.yaw + d * t;
        out
    }
}

const POSITION: u16 = 1;
const YAW: u16 = 2;
const STATE: u16 = 4;
const VELOCITY: u16 = 8;
const JUMP: u16 = 16;
const FLAGS: u16 = 32;
const MOTION: u16 = 64;
const STAMINA: u16 = 128;
const RATE: u16 = 256;
const HOLD: u16 = 512;
const ALL: u16 = 1023;

fn v3(w: &mut Writer, v: [f32; 3]) {
    w.f32(v[0]).f32(v[1]).f32(v[2]);
}

fn read_v3(r: &mut Reader) -> Result<[f32; 3]> {
    Ok([r.f32()?, r.f32()?, r.f32()?])
}

fn state(w: &mut Writer, s: State) {
    let (tag, a, b) = match s {
        State::Wait => (0, false, false),
        State::Walk { relaxed } => (1, relaxed, false),
        State::Run { relaxed } => (2, relaxed, false),
        State::SprintStart => (3, false, false),
        State::Sprint => (4, false, false),
        State::SprintStop => (5, false, false),
        State::JumpStart { standing, sprint } => (6, standing, sprint),
        State::Air { standing, sprint } => (7, standing, sprint),
        State::SecondJump => (8, false, false),
        State::Fall => (9, false, false),
        State::Land { standing } => (10, standing, false),
        State::SprintJumpLand => (11, false, false),
        State::LedgeGrab { braced, phase } => {
            let p = match phase {
                GrabPhase::Reach => 0,
                GrabPhase::Rise => 1,
                GrabPhase::Settle => 2,
            };
            w.u8(12).bool(braced).u8(p);
            return;
        }
        State::LedgeHang { braced } => (13, braced, false),
        State::LedgeClimb { braced } => (14, braced, false),
        State::Attack { attack } => {
            let a = match attack {
                Attack::SwordLight1 => 0,
                Attack::SwordLight2 => 1,
                Attack::SwordLight3 => 2,
                Attack::SwordHeavy => 3,
                Attack::StaffStrike1 => 4,
                Attack::StaffStrike2 => 5,
                Attack::SwordAirLight => 6,
                Attack::SwordAirHeavy => 7,
                Attack::SwordAirHeavyLand => 8,
                Attack::StaffAirStrike => 9,
            };
            w.u8(15).u8(a);
            return;
        }
        State::Shot { phase } => {
            let p = match phase {
                ShotPhase::Start => 0,
                ShotPhase::Loop => 1,
                ShotPhase::End => 2,
                ShotPhase::AirStart => 3,
                ShotPhase::AirLoop => 4,
                ShotPhase::AirEnd => 5,
            };
            w.u8(16).u8(p);
            return;
        }
    };
    w.u8(tag).bool(a).bool(b);
}

fn read_state(r: &mut Reader) -> Result<State> {
    let tag = r.u8()?;
    if tag == 15 {
        let attack = match r.u8()? {
            0 => Attack::SwordLight1,
            1 => Attack::SwordLight2,
            2 => Attack::SwordLight3,
            3 => Attack::SwordHeavy,
            4 => Attack::StaffStrike1,
            5 => Attack::StaffStrike2,
            6 => Attack::SwordAirLight,
            7 => Attack::SwordAirHeavy,
            8 => Attack::SwordAirHeavyLand,
            9 => Attack::StaffAirStrike,
            a => return Err(format!("attack {a}")),
        };
        return Ok(State::Attack { attack });
    }
    if tag == 16 {
        let phase = match r.u8()? {
            0 => ShotPhase::Start,
            1 => ShotPhase::Loop,
            2 => ShotPhase::End,
            3 => ShotPhase::AirStart,
            4 => ShotPhase::AirLoop,
            5 => ShotPhase::AirEnd,
            p => return Err(format!("shot phase {p}")),
        };
        return Ok(State::Shot { phase });
    }
    if tag == 12 {
        let braced = r.bool()?;
        let phase = match r.u8()? {
            0 => GrabPhase::Reach,
            1 => GrabPhase::Rise,
            2 => GrabPhase::Settle,
            p => return Err(format!("grab phase {p}")),
        };
        return Ok(State::LedgeGrab { braced, phase });
    }
    let (a, b) = (r.bool()?, r.bool()?);
    Ok(match tag {
        0 => State::Wait,
        1 => State::Walk { relaxed: a },
        2 => State::Run { relaxed: a },
        3 => State::SprintStart,
        4 => State::Sprint,
        5 => State::SprintStop,
        6 => State::JumpStart { standing: a, sprint: b },
        7 => State::Air { standing: a, sprint: b },
        8 => State::SecondJump,
        9 => State::Fall,
        10 => State::Land { standing: a },
        11 => State::SprintJumpLand,
        13 => State::LedgeHang { braced: a },
        14 => State::LedgeClimb { braced: a },
        t => return Err(format!("state tag {t}")),
    })
}

fn hold(w: &mut Writer, h: &Option<Hold>) {
    w.bool(h.is_some());
    if let Some(h) = h {
        v3(w, h.ledge.edge);
        v3(w, h.ledge.normal);
        v3(w, h.ledge.wall_normal);
        w.bool(h.hang.braced);
        v3(w, h.hang.hands);
        v3(w, h.hang.body);
        v3(w, h.hang.foot);
        v3(w, h.start);
        v3(w, h.wall);
        w.f32(h.from_y);
    }
}

fn read_hold(r: &mut Reader) -> Result<Option<Hold>> {
    if !r.bool()? {
        return Ok(None);
    }
    let ledge = Ledge { edge: read_v3(r)?, normal: read_v3(r)?, wall_normal: read_v3(r)? };
    let hang = Hang { braced: r.bool()?, hands: read_v3(r)?, body: read_v3(r)?, foot: read_v3(r)? };
    Ok(Some(Hold { ledge, hang, start: read_v3(r)?, wall: read_v3(r)?, from_y: r.f32()? }))
}

/// Writes `b` as a delta against `base` (the same id), or whole without
/// one.
pub fn write(w: &mut Writer, b: &Body, base: Option<&Body>) {
    let mut mask = ALL;
    if let Some(o) = base {
        mask = 0;
        let mut set = |bit: u16, same: bool| {
            if !same {
                mask |= bit;
            }
        };
        set(POSITION, o.position == b.position);
        set(YAW, o.yaw == b.yaw);
        set(STATE, o.state == b.state && o.frame == b.frame);
        set(VELOCITY, o.velocity == b.velocity);
        set(JUMP, o.jump == b.jump);
        set(
            FLAGS,
            (o.jumps_left, o.double_jump, o.relaxed, o.ran_out, o.body_class, o.load_class)
                == (b.jumps_left, b.double_jump, b.relaxed, b.ran_out, b.body_class, b.load_class),
        );
        set(MOTION, o.speed == b.speed && o.drop_from == b.drop_from);
        set(STAMINA, o.stamina == b.stamina && o.stamina_max == b.stamina_max);
        set(RATE, o.unit_rate == b.unit_rate);
        set(HOLD, o.hold == b.hold);
    }
    w.u8(b.id).u16(mask);
    if mask & POSITION != 0 {
        v3(w, b.position);
    }
    if mask & YAW != 0 {
        w.f32(b.yaw);
    }
    if mask & STATE != 0 {
        state(w, b.state);
        w.f32(b.frame);
    }
    if mask & VELOCITY != 0 {
        v3(w, b.velocity);
    }
    if mask & JUMP != 0 {
        let j = b.jump;
        w.f32(j.speed_y).f32(j.speed_z).f32(j.gravity).f32(j.add_move_xz).f32(j.damping_z);
    }
    if mask & FLAGS != 0 {
        w.u8(b.jumps_left).bool(b.double_jump).bool(b.relaxed).bool(b.ran_out).u8(b.body_class).u8(b.load_class);
    }
    if mask & MOTION != 0 {
        w.f32(b.speed).f32(b.drop_from);
    }
    if mask & STAMINA != 0 {
        w.f32(b.stamina).f32(b.stamina_max);
    }
    if mask & RATE != 0 {
        w.f32(b.unit_rate);
    }
    if mask & HOLD != 0 {
        hold(w, &b.hold);
    }
}

/// Reads one body; `base` finds the baseline body of an id (a delta
/// without its baseline is an error).
pub fn read(r: &mut Reader, base: &dyn Fn(PlayerId) -> Option<Body>) -> Result<Body> {
    let id = r.u8()?;
    let mask = r.u16()?;
    let mut b = match base(id) {
        Some(b) => b,
        None if mask == ALL => Body {
            id,
            position: [0.0; 3],
            yaw: 0.0,
            state: State::Wait,
            frame: 0.0,
            velocity: [0.0; 3],
            jump: ddsim::JUMP_NORMAL,
            jumps_left: 0,
            double_jump: false,
            relaxed: false,
            ran_out: false,
            body_class: 0,
            load_class: 0,
            speed: 0.0,
            drop_from: 0.0,
            stamina: 0.0,
            stamina_max: 0.0,
            unit_rate: 1.0,
            hold: None,
        },
        None => return Err(format!("delta for body {id} without a baseline")),
    };
    if mask & POSITION != 0 {
        b.position = read_v3(r)?;
    }
    if mask & YAW != 0 {
        b.yaw = r.f32()?;
    }
    if mask & STATE != 0 {
        b.state = read_state(r)?;
        b.frame = r.f32()?;
    }
    if mask & VELOCITY != 0 {
        b.velocity = read_v3(r)?;
    }
    if mask & JUMP != 0 {
        b.jump = JumpParam { speed_y: r.f32()?, speed_z: r.f32()?, gravity: r.f32()?, add_move_xz: r.f32()?, damping_z: r.f32()? };
    }
    if mask & FLAGS != 0 {
        b.jumps_left = r.u8()?;
        b.double_jump = r.bool()?;
        b.relaxed = r.bool()?;
        b.ran_out = r.bool()?;
        b.body_class = r.u8()?;
        b.load_class = r.u8()?;
    }
    if mask & MOTION != 0 {
        b.speed = r.f32()?;
        b.drop_from = r.f32()?;
    }
    if mask & STAMINA != 0 {
        b.stamina = r.f32()?;
        b.stamina_max = r.f32()?;
    }
    if mask & RATE != 0 {
        b.unit_rate = r.f32()?;
    }
    if mask & HOLD != 0 {
        b.hold = read_hold(r)?;
    }
    Ok(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn body() -> Body {
        let mut sim = ddsim::Sim::new(HashMap::new());
        sim.add_player(3, [1.0, 2.0, 3.0], 0.5);
        Body::of(sim.player(3).unwrap())
    }

    #[test]
    fn whole_and_delta() {
        let a = body();
        let mut b = a.clone();
        b.position[0] += 7.5;
        b.state = State::LedgeGrab { braced: true, phase: GrabPhase::Rise };
        b.hold = Some(Hold {
            ledge: Ledge { edge: [1.0; 3], normal: [0.0, 0.0, 1.0], wall_normal: [0.0, 0.0, -1.0] },
            hang: Hang { braced: true, hands: [2.0; 3], body: [3.0; 3], foot: [4.0; 3] },
            start: [5.0; 3],
            wall: [6.0; 3],
            from_y: -1.0,
        });
        let mut w = Writer::new();
        write(&mut w, &a, None);
        write(&mut w, &b, Some(&a));
        let bytes = w.finish();
        let mut r = Reader::new(&bytes);
        let a2 = read(&mut r, &|_| None).unwrap();
        assert_eq!(a2, a);
        let b2 = read(&mut r, &|_| Some(a.clone())).unwrap();
        assert_eq!(b2, b);
        r.end().unwrap();
        // An unchanged body costs three bytes.
        let mut w = Writer::new();
        write(&mut w, &a, Some(&a));
        assert_eq!(w.finish().len(), 3);
    }

    #[test]
    fn lerp_takes_the_short_way_round() {
        let mut a = body();
        let mut b = a.clone();
        a.yaw = 3.0;
        b.yaw = -3.0;
        b.position = [11.0, 2.0, 3.0];
        let m = Body::lerp(&a, &b, 0.5);
        assert!((m.yaw.abs() - std::f32::consts::PI).abs() < 0.01);
        assert_eq!(m.position, [6.0, 2.0, 3.0]);
    }
}
