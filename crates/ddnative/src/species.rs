//! Species base entries: the enemy-family entry
//! ([`crate::action::enemy_action_enter`]) against a species class, then a
//! species tail. Tails are memory-model ports (`crate::mem::Mem`): `act` is
//! the action's address, `owner` the address the entry kept at +0x74 (0 when
//! the owner is not of the species). Tails that write through +0x74 without
//! a null check require a species owner (the original faults otherwise).
use crate::mem::Mem;

/// Velocity (+0x2530) and acceleration (+0x2540) set to the globals vector
/// 0x018CFE80..88 with w = 0.
fn motion_reset(m: &mut (impl Mem + ?Sized), owner: u32, g: [u32; 3]) {
    for base in [0x2530, 0x2540] {
        for (i, v) in g.into_iter().enumerate() {
            m.set_u32(owner + base + 4 * i as u32, v);
        }
        m.set_u32(owner + base + 12, 0);
    }
}

/// +0x2578 = +0x257C (both rewritten with that value), +0x2574 = 0x100.
fn speed_copy(m: &mut (impl Mem + ?Sized), owner: u32) {
    let v = m.u32(owner + 0x257C);
    m.set_u32(owner + 0x2578, v);
    m.set_u32(owner + 0x257C, v);
    m.set_u16(owner + 0x2574, 0x100);
}

// Translated from 0x00A4DB70 (DDDA.exe 2017-12-13 Steam), cEm5800Action::vf05.
/// cEm5800: (species owner) action +0x78 (u16) = 0, owner +0x603B = 0.
pub fn em5800_tail(m: &mut (impl Mem + ?Sized), act: u32, owner: u32) {
    assert!(owner != 0, "cEm5800Action::vf05 requires a species owner");
    m.set_u16(act + 0x78, 0);
    m.set_u8(owner + 0x603B, 0);
}

// Translated from 0x008FBDD0 (DDDA.exe 2017-12-13 Steam), cEm0200Action::vf05.
/// cEm0200 (wolf): (species owner) body flag 0x1, motion reset, +0x5C7C = 0.
pub fn em0200_tail(m: &mut (impl Mem + ?Sized), owner: u32, g: [u32; 3]) {
    assert!(owner != 0, "cEm0200Action::vf05 requires a species owner");
    let f = m.u32(owner + 0x273C);
    m.set_u32(owner + 0x273C, f | 1);
    motion_reset(m, owner, g);
    m.set_u8(owner + 0x5C7C, 0);
}

// Translated from 0x00919DB0 (DDDA.exe 2017-12-13 Steam), cEm0400Action::vf05.
/// cEm0400: (species owner) motion reset and speed copy.
pub fn em0400_tail(m: &mut (impl Mem + ?Sized), owner: u32, g: [u32; 3]) {
    assert!(owner != 0, "cEm0400Action::vf05 requires a species owner");
    motion_reset(m, owner, g);
    speed_copy(m, owner);
}

// Translated from 0x0095A810 (DDDA.exe 2017-12-13 Steam), cEm0900Action::vf05.
/// cEm0900: if a species owner: motion reset, speed copy, +0x6088 = 0.
pub fn em0900_tail(m: &mut (impl Mem + ?Sized), owner: u32, g: [u32; 3]) {
    if owner != 0 {
        motion_reset(m, owner, g);
        speed_copy(m, owner);
        m.set_u32(owner + 0x6088, 0);
    }
}

// Translated from 0x00A2AE80 (DDDA.exe 2017-12-13 Steam), cEm5500TentacleBaseAction::vf05.
/// cEm5500 tentacle: action +0x78 = 0 (the prefix clears it, with +0x74,
/// before the is-a check), then 1 when the species owner's +0x7324 is null
/// or its byte +0x2D is 0x41 or 0xAF.
pub fn em5500_tentacle_tail(m: &mut (impl Mem + ?Sized), act: u32, owner: u32) {
    m.set_u8(act + 0x78, 0);
    if owner != 0 {
        let p = m.u32(owner + 0x7324);
        if p == 0 || matches!(m.u8(p + 0x2D), 0x41 | 0xAF) {
            m.set_u8(act + 0x78, 1);
        }
    }
}

// Translated from 0x00A202E0 (DDDA.exe 2017-12-13 Steam), cEm5500_00Action::vf05.
/// cEm5500_00: action +0x78 = 0 (cleared in the prefix, as for the
/// tentacle), then 1 when the species owner's +0x7324 is non-null and its
/// byte +0x2D is 0x41.
pub fn em5500_00_tail(m: &mut (impl Mem + ?Sized), act: u32, owner: u32) {
    m.set_u8(act + 0x78, 0);
    if owner != 0 {
        let p = m.u32(owner + 0x7324);
        if p != 0 && m.u8(p + 0x2D) == 0x41 {
            m.set_u8(act + 0x78, 1);
        }
    }
}

// Translated from 0x009C5890 (DDDA.exe 2017-12-13 Steam), cEm5300Action::vf05.
/// cEm5300: for a species owner, byte +0x80 = 1 on each of the +0x60E8
/// parts listed at +0x60F4, then +0x60E2 = 1 and +0x618D = 0.
pub fn em5300_tail(m: &mut (impl Mem + ?Sized), owner: u32) {
    if owner != 0 {
        let mut i = 0u32;
        while i < m.u32(owner + 0x60E8) {
            let part = m.u32(m.u32(owner + 0x60F4).wrapping_add(i * 4));
            i += 1;
            m.set_u8(part + 0x80, 1);
        }
        m.set_u8(owner + 0x60E2, 1);
        m.set_u8(owner + 0x618D, 0);
    }
}

// Translated from 0x009B4920 (DDDA.exe 2017-12-13 Steam), cEm5200_00Action::vf05.
// Translated from 0x009BD970 (DDDA.exe 2017-12-13 Steam), cEm5200_01Action::vf05.
/// cEm5200_00 / cEm5200_01: no tail (the enemy-family entry alone).
pub fn em5200_tail() {}

/// Class-id globals of the bases that have a memory-model form.
pub const BASES: &[(u32, &str)] = &[
    (0x008D4840, "goblin"), (0x00A4DB70, "em5800"), (0x008FBDD0, "em0200"), (0x00919DB0, "em0400"),
    (0x0095A810, "em0900"), (0x00A2AE80, "em5500t"), (0x00A202E0, "em5500_00"), (0x009C5890, "em5300"),
    (0x009B4920, "em5200"), (0x009BD970, "em5200"),
];

/// A verified base entry over memory: the typed enemy-family prefix on the
/// action at `this` and its owner (`[this+0x44]`), written back, then the
/// base's tail. `owner_is_class` is the result of the DTI walk; `globals`
/// the vector at 0x018CFE80..88. Returns false for an unknown base.
pub fn base_entry_mem(m: &mut (impl Mem + ?Sized), base: u32, this: u32, owner_is_class: bool, globals: [u32; 3]) -> bool {
    use crate::action::{enemy_action_enter, EnemyAction, Owner};
    let Some(&(_, kind)) = BASES.iter().find(|(a, _)| *a == base) else { return false };
    let own = m.u32(this + 0x44);
    let mut owner = Owner { category_206c: m.u32(own + 0x206C), word_2370: m.u32(own + 0x2370),
        word_2374: m.u32(own + 0x2374), flags_273c: m.u32(own + 0x273C),
        status_2e34: m.u32(own + 0x2E34), pending_2e38: m.u32(own + 0x2E38) };
    let mut o2ba0 = m.u32(own + 0x2BA0);
    let mut act = EnemyAction { byte_50: m.u8(this + 0x50), owner_54: m.u32(this + 0x54), word_58: m.u32(this + 0x58),
        word_60: m.u32(this + 0x60), float_64: f32::from_bits(m.u32(this + 0x64)), byte_6c: m.u8(this + 0x6C),
        word_70: m.u32(this + 0x70), owner_74: m.u32(this + 0x74) };
    enemy_action_enter(&mut act, own, &mut owner, &mut o2ba0, || owner_is_class);
    m.set_u8(this + 0x50, act.byte_50); m.set_u32(this + 0x54, act.owner_54); m.set_u32(this + 0x58, act.word_58);
    m.set_u32(this + 0x60, act.word_60); m.set_u32(this + 0x64, act.float_64.to_bits()); m.set_u8(this + 0x6C, act.byte_6c);
    m.set_u32(this + 0x70, act.word_70); m.set_u32(this + 0x74, act.owner_74);
    m.set_u32(own + 0x2370, owner.word_2370); m.set_u32(own + 0x2374, owner.word_2374);
    m.set_u32(own + 0x273C, owner.flags_273c); m.set_u32(own + 0x2E34, owner.status_2e34);
    m.set_u32(own + 0x2BA0, o2ba0);
    let o = act.owner_74;
    match kind {
        "goblin" => {
            assert!(o != 0, "cEm0100Action::vf05 requires a goblin owner");
            motion_reset(m, o, globals);
            speed_copy(m, o);
            m.set_u8(o + 0x5C7C, 0);
        }
        "em5800" => em5800_tail(m, this, o),
        "em0200" => em0200_tail(m, o, globals),
        "em0400" => em0400_tail(m, o, globals),
        "em0900" => em0900_tail(m, o, globals),
        "em5500t" => em5500_tentacle_tail(m, this, o),
        "em5500_00" => em5500_00_tail(m, this, o),
        "em5300" => em5300_tail(m, o),
        _ => em5200_tail(),
    }
    true
}
