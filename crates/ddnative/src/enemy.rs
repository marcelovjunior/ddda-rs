//! `uEnemy` members.

/// One blend source of `uEnemy::vf195` (`+0x5E84` array, 0x18 bytes each):
/// the pointed pair of 2D values and the blend factor at +0x14.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlendSource {
    /// `[source+4]`: a0, a1 (from) and b0, b1 (to) at +0x0, +0x4, +0x8, +0xC.
    pub values: [f32; 4],
    pub t: f32,
}

/// One target of the blend (`+0x22B8` array, 0x230 bytes each): it gets
/// state 4 at +0x8 and the blended pair at +0x114 / +0x118.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlendTarget {
    pub state_08: u32,
    pub value_114: f32,
    pub value_118: f32,
}

/// `uEnemy::vf195`: for each of `+0x22BC` targets, blends its source pair
/// as `(1 - t) * a + b * t` in single precision, in the original's order
/// (`b*t` first, then `(1-t)*a`, then the sum). `sources` is None when
/// `+0x5E84` is null; `targets` None when `+0x22B8` is null.
// Translated from 0x00AAEF50 (DDDA.exe 2017-12-13 Steam), uEnemy::vf195.
pub fn blend(sources: Option<&[BlendSource]>, targets: Option<&mut [BlendTarget]>) {
    let (Some(sources), Some(targets)) = (sources, targets) else { return };
    for (s, t) in sources.iter().zip(targets.iter_mut()) {
        t.state_08 = 4;
        let one_minus = 1.0f32 - s.t;
        t.value_114 = one_minus * s.values[0] + s.values[2] * s.t;
        t.value_118 = one_minus * s.values[1] + s.values[3] * s.t;
    }
}

/// The goblin (uEm0100) fields the goblin action entry resets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GoblinMotion {
    /// +0x2530: velocity (x, y, z, w); +0x2540: acceleration.
    pub velocity_2530: [f32; 4],
    pub accel_2540: [f32; 4],
    /// +0x2574 (u16), +0x2578 / +0x257C (f32), +0x5C7C (byte).
    pub word_2574: u16,
    pub value_2578: f32,
    pub value_257c: f32,
    pub byte_5c7c: u8,
}

/// `cEm0100Action::vf05`, the entry of every goblin action: the
/// enemy-family entry ([`crate::action::enemy_action_enter`]) against the
/// goblin class, then velocity and acceleration set to the globals vector
/// (0x018CFE80..88, w = 0), +0x2578 = +0x257C, +0x2574 = 0x100 and
/// +0x5C7C = 0. The original writes these through the is-a result without
/// a null check, so the owner must be a goblin (precondition).
// Translated from 0x008D4840 (DDDA.exe 2017-12-13 Steam), cEm0100Action::vf05.
pub fn goblin_action_enter(act: &mut crate::action::EnemyAction, owner_addr: u32,
    owner: &mut crate::action::Owner, owner_2ba0: &mut u32, owner_is_goblin: impl FnOnce() -> bool,
    g: &mut GoblinMotion, globals: [f32; 3]) {
    crate::action::enemy_action_enter(act, owner_addr, owner, owner_2ba0, owner_is_goblin);
    assert!(act.owner_74 != 0, "cEm0100Action::vf05 requires a goblin owner (the original faults)");
    g.velocity_2530 = [globals[0], globals[1], globals[2], 0.0];
    g.accel_2540 = [globals[0], globals[1], globals[2], 0.0];
    g.value_2578 = g.value_257c;
    g.word_2574 = 0x100;
    g.byte_5c7c = 0;
}
