//! uOmObj7010's regular hinged-door phases, with explicit actor requests.
use std::collections::BTreeMap;
use crate::placement::Id;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase { Closed, Opening, Open, Closing, Reset }

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    pub slot: usize,
    pub blend_frames: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Change {
    pub motion: Option<Motion>,
    pub collision: Option<bool>,
    /// rSoundRequest requests 0x28 / 0x29, when the authored bank exists.
    pub sound_request: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Door {
    pub id: Id,
    pub object: u32,
    pub position: [f32; 3],
    /// Evaluated model forward basis (vf22(-1)+0x20), not a camera vector.
    pub forward: [f32; 3],
    pub phase: Phase,
    pub motion: usize,
    pub collision: bool,
    pub pair: Option<Id>,
    clear: bool,
    close_requested: bool,
}

/// Constructor's pair-search radius: 0x00c3e700. These are code switch
/// constants, not a copied object catalogue. Other uOmObj7010 variants
/// need separate sliding/special-door handling.
pub fn hinged_pair_half_width(object: u32) -> Option<f32> {
    match object {
        0x1b62 | 0x1b85 | 0x1b8f | 0x1b9e | 0x1bc1 | 0x1bc2 |
        0x1bd0 | 0x1bd1 | 0x1be4 | 0x1bea | 0x1bee | 0x1bfd |
        0x1bfe | 0x1c1b | 0x1c21 | 0x1c22 => Some(162.0),
        0x1b94 => Some(128.0),
        0x1b95 => Some(256.0),
        _ => None,
    }
}

impl Door {
    /// Regular closed initial state (0x00c3e700). The caller must resolve
    /// the authored lock/save state before instantiating this state.
    pub fn closed(id: Id, object: u32, position: [f32; 3], forward: [f32; 3]) -> Result<Self, String> {
        if hinged_pair_half_width(object).is_none() { return Err(format!("unsupported door variant om{object:04}")); }
        Ok(Self { id, object, position, forward, phase: Phase::Closed, motion: 0,
            collision: true, pair: None, clear: false, close_requested: false })
    }

    /// Actor message 0x1a, issued by cPlActOpenDoor 0x00afc730 or NPC/AI.
    /// 0x00b6eac0's player path requires the interaction input. Supplying
    /// proximity alone would manufacture an opening request.
    /// `can_open` is the resolved lock/event/night state, not inferred here.
    // Translated from 0x00c3f130 and 0x00c1f220 (DDDA.exe 2017-12-13 Steam).
    pub fn request_open(&mut self, actor: Option<[f32; 3]>, can_open: bool) -> Change {
        if !can_open || self.phase != Phase::Closed { return Change::default(); }
        self.motion = opening_motion(self.position, self.forward, actor);
        self.phase = Phase::Opening;
        self.collision = false;
        self.clear = false;
        self.close_requested = false;
        Change { motion: Some(Motion { slot: self.motion, blend_frames: 5.0 }),
            collision: Some(false), sound_request: Some(0x28) }
    }

    /// Action update followed by the door phase update. `motion_end` is
    /// the model's motion completion observation (0x01057100), not a
    /// guessed duration. Occupancy is evaluated independently of requests.
    // Translated from 0x00c1f420, 0x00c1f4c0, 0x00c1f5a0 and
    // 0x00c3f130 (DDDA.exe 2017-12-13 Steam).
    pub fn step(&mut self, motion_end: bool, occupied: bool) -> Change {
        let mut change = Change::default();
        match self.phase {
            Phase::Opening if motion_end => {
                self.motion = match self.motion { 3 => 1, 4 => 2, _ => 0 };
                self.phase = Phase::Open;
                change.motion = Some(Motion { slot: self.motion, blend_frames: 5.0 });
            }
            Phase::Open if self.close_requested => {
                self.motion = match self.motion { 1 => 5, 2 => 6, _ => 0 };
                self.phase = Phase::Closing;
                self.clear = false;
                self.close_requested = false;
                self.pair = None;
                change.motion = Some(Motion { slot: self.motion, blend_frames: 5.0 });
                change.sound_request = Some(0x29);
            }
            Phase::Open => self.clear = !occupied,
            Phase::Closing if motion_end => {
                self.motion = 0;
                self.collision = true;
                self.phase = Phase::Reset;
                change.motion = Some(Motion { slot: 0, blend_frames: 5.0 });
                change.collision = Some(true);
            }
            Phase::Reset => self.phase = Phase::Closed,
            _ => {}
        }
        change
    }
}

// Translated from 0x00c1f220 (DDDA.exe 2017-12-13 Steam).
pub fn opening_motion(position: [f32; 3], forward: [f32; 3], actor: Option<[f32; 3]>) -> usize {
    let Some(actor) = actor else { return 3 };
    let dx = actor[0] - position[0];
    let dz = actor[2] - position[2];
    let length_squared = dz * dz + dx * dx;
    if length_squared <= f32::EPSILON { return 4; }
    let inverse_length = 1.0 / length_squared.sqrt();
    let dot = forward[1] * inverse_length * 0.0 + forward[0] * inverse_length * dx
        + forward[2] * dz * inverse_length;
    // The exe passes the f32 pi/2 value promoted to double to its cosine
    // routine, then converts the result to f32. The threshold is slightly
    // negative; replacing it with zero changes the boundary.
    let threshold = (std::f32::consts::FRAC_PI_2 as f64).cos() as f32;
    if threshold <= dot { 3 } else { 4 }
}

/// Eligible actors for the original's searches. The caller omits units
/// that are not active/searchable; N players, pawns, NPCs and enemies may
/// be supplied without giving any one player a global role.
#[derive(Debug, Clone, Copy)]
pub struct Occupant {
    pub position: [f32; 3],
    pub kind: OccupantKind,
}
#[derive(Debug, Clone, Copy)]
pub enum OccupantKind { Player, Pawn, Npc, Enemy(u8), HumanEnemy }

/// 0x00c3f870 searches four managers with a 400cm horizontal cylinder,
/// 250cm vertical half-height. 0x00422650 implements the inclusive test.
// Translated from 0x00c3f870 and 0x00422650 (DDDA.exe 2017-12-13 Steam).
pub fn occupied(position: [f32; 3], actors: &[Occupant]) -> bool {
    actors.iter().any(|actor| {
        if let OccupantKind::Enemy(kind) = actor.kind {
            // Original sEnemyManager search excludes this code-embedded
            // list. sNpcManager and sHumanEnemyManager have no such filter.
            if [0x61, 0x62, 0x69, 0x6a, 0x6b, 0x6c, 0x6d, 0x73, 0x74, 0x7b].contains(&kind) { return false; }
        }
        let dx = position[0] - actor.position[0];
        let dz = position[2] - actor.position[2];
        (dz * dz + dx * dx).sqrt() <= 400.0 && (position[1] - actor.position[1]).abs() <= 250.0
    })
}

/// Phase-10 latch runs after every door has produced its phase-91 clear
/// observation. This avoids iteration-order dependence between paired
/// leaves; a close request takes effect on the next phase-91 update.
// Translated from 0x00c3fba0 (DDDA.exe 2017-12-13 Steam).
pub fn finish_frame(doors: &mut BTreeMap<Id, Door>) {
    let clear: BTreeMap<_, _> = doors.iter().map(|(&id, door)| (id, door.clear)).collect();
    for door in doors.values_mut() {
        if door.phase == Phase::Open {
            door.close_requested = door.clear && door.pair.is_none_or(|id| clear.get(&id) == Some(&true));
        }
    }
}

/// Pair search 0x0049e920 / 0x00c3fb00: nearest same object number,
/// horizontal distance <= twice the class radius, height <=250cm, and
/// original interaction state 4 (closed). Source enumeration wins ties;
/// callers pass stable original placement order.
// Translated from 0x0049e920 and 0x00c3fb00 (DDDA.exe 2017-12-13 Steam).
pub fn find_pair<'a>(door: &Door, candidates: impl IntoIterator<Item = &'a Door>) -> Option<Id> {
    let half_width = hinged_pair_half_width(door.object)?;
    let mut closest = None;
    let mut distance = f32::INFINITY;
    for other in candidates {
        if other.id == door.id || other.object != door.object { continue; }
        let dx = door.position[0] - other.position[0];
        let dz = door.position[2] - other.position[2];
        let d = (dz * dz + dx * dx).sqrt();
        if d <= half_width * 2.0 && (door.position[1] - other.position[1]).abs() <= 250.0 && d < distance {
            distance = d;
            closest = Some(other);
        }
    }
    closest.filter(|other| other.phase == Phase::Closed).map(|other| other.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn door(record: i32) -> Door { Door::closed(Id { stage: 200, group: 18, record }, 7121,
        [record as f32 * 320.0, 0.0, 0.0], [0.0, 0.0, 1.0]).unwrap() }

    #[test]
    fn requested_open_uses_side_and_authored_completion() {
        let mut d = door(0);
        assert_eq!(d.request_open(Some([0.0, 0.0, -10.0]), false), Change::default());
        assert_eq!(d.request_open(Some([0.0, 0.0, -10.0]), true).motion.unwrap().slot, 4);
        assert!(!d.collision);
        assert_eq!(d.step(false, false), Change::default());
        assert_eq!(d.step(true, false).motion.unwrap().slot, 2);
        assert_eq!(d.phase, Phase::Open);
        assert_eq!(opening_motion([0.0; 3], [0.0,0.0,1.0], None), 3);
        assert_eq!(opening_motion([0.0; 3], [0.0,0.0,1.0], Some([0.0; 3])), 4);
        assert_eq!(opening_motion([0.0; 3], [0.0,0.0,1.0], Some([1.0,0.0,0.0])), 3);
    }

    #[test]
    fn pair_waits_for_both_leaves_and_closing_keeps_collision_disabled() {
        let mut doors = BTreeMap::from([(door(0).id, door(0)), (door(1).id, door(1))]);
        let ids: Vec<_> = doors.keys().copied().collect();
        assert_eq!(find_pair(&doors[&ids[0]], doors.values()), Some(ids[1]));
        for (i, id) in ids.iter().enumerate() {
            let d = doors.get_mut(id).unwrap();
            d.request_open(None, true);
            d.pair = Some(ids[1-i]);
            d.step(true, false);
        }
        doors.get_mut(&ids[0]).unwrap().step(false, false);
        doors.get_mut(&ids[1]).unwrap().step(false, true);
        finish_frame(&mut doors);
        assert!(!doors[&ids[0]].close_requested);
        doors.get_mut(&ids[1]).unwrap().step(false, false);
        finish_frame(&mut doors);
        for d in doors.values_mut() {
            assert_eq!(d.step(false, false).motion.unwrap().slot, 5);
            assert!(!d.collision);
            assert_eq!(d.step(true, false).collision, Some(true));
            assert_eq!(d.phase, Phase::Reset);
            d.step(false, false);
            assert_eq!(d.phase, Phase::Closed);
        }
    }

    #[test]
    fn occupancy_any_seat_and_inclusive_cylinder() {
        let mut actors = vec![Occupant { position: [400.0,250.0,0.0], kind: OccupantKind::Player }];
        assert!(occupied([0.0;3], &actors));
        actors[0].position[0] = 400.01;
        assert!(!occupied([0.0;3], &actors));
        actors.push(Occupant { position: [0.0;3], kind: OccupantKind::Pawn });
        assert!(occupied([0.0;3], &actors));
        actors[1].kind = OccupantKind::Enemy(0x61);
        assert!(!occupied([0.0;3], &actors));
    }

    #[test]
    fn nearest_open_leaf_does_not_pair_with_farther_closed_leaf() {
        let a = door(0);
        let mut b = door(1);
        b.position[0] = 10.0;
        b.request_open(None, true);
        let mut c = door(2);
        c.position[0] = 20.0;
        assert_eq!(find_pair(&a, [&b, &c]), None);
    }
}
