//! `uCharacterBase`, `uPlayerBase` and `uNpc` members.

/// The damage-edge bytes of `uCharacterBase::vf111` and their inputs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DamageEdges {
    /// +0x206D: category (bit 0x8: NPC).
    pub category_206d: u8,
    /// +0x2360: action flags (bit 0x4000: damage).
    pub action_2360: u32,
    /// +0x273C: body flags (bit 0x10: damage).
    pub flags_273c: u32,
    /// +0x2EA0: previous update's damage state; +0x2EA1 start edge;
    /// +0x2EA2 end edge; +0x2EA3 cleared each update.
    pub prev_2ea0: u8,
    pub start_2ea1: u8,
    pub end_2ea2: u8,
    pub byte_2ea3: u8,
}

/// `uCharacterBase::vf111`: one-update damage edges. A character is damaged
/// when body flag 0x10 is set, or (an NPC) its `vf180` flag byte has 0x80,
/// or action flag 0x4000 is set. `npc_flags` is `vf180`'s flag byte, read
/// only for NPCs and before the other two tests, as the original does.
// Translated from 0x00850100 (DDDA.exe 2017-12-13 Steam), uCharacterBase::vf111.
pub fn damage_edges(c: &mut DamageEdges, npc_flags: impl FnOnce() -> u8) {
    let body = c.flags_273c & 0x10 == 0x10;
    c.start_2ea1 = 0;
    c.end_2ea2 = 0;
    c.byte_2ea3 = 0;
    let damaged = (c.category_206d & 8 != 0 && npc_flags() & 0x80 != 0)
        || body
        || c.action_2360 & 0x4000 != 0;
    if damaged {
        if c.prev_2ea0 == 0 {
            c.start_2ea1 = 1;
        }
        c.prev_2ea0 = 1;
    } else {
        if c.prev_2ea0 == 1 {
            c.end_2ea2 = 1;
        }
        c.prev_2ea0 = 0;
    }
}

/// Where `uNpc::vf180` finds the NPC flag word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NpcFlags {
    /// The linked intel's word at `intel+0x5C0`.
    Intel(u32),
    /// The NPC's own word at `this+0x2738` (no intel linked).
    Own,
}

/// `uNpc::vf180`: the NPC flag word's location (`+0x5854` is the linked
/// intel pointer).
// Translated from 0x00BB4730 (DDDA.exe 2017-12-13 Steam), uNpc::vf180.
pub fn npc_flags(intel_5854: u32) -> NpcFlags {
    if intel_5854 != 0 {
        NpcFlags::Intel(intel_5854.wrapping_add(0x5C0))
    } else {
        NpcFlags::Own
    }
}

/// `uPlayerBase::vf18`: false while the state word `+0x5824` is 1 or 2.
// Translated from 0x00B88AB0 (DDDA.exe 2017-12-13 Steam), uPlayerBase::vf18.
pub fn player_vf18(state_5824: u32) -> bool {
    !(state_5824 == 1 || state_5824 == 2)
}

/// `uPlayerBase::vf207`: `+0x2104` is nonzero and action flag 0x1000000 set.
// Translated from 0x00B53190 (DDDA.exe 2017-12-13 Steam), uPlayerBase::vf207.
pub fn player_vf207(word_2104: u32, action_2360: u32) -> bool {
    word_2104 != 0 && action_2360 & 0x0100_0000 != 0
}

/// `uPlayerBase::vf187`: zeroes `+0x207C` while the global scene object
/// (0x018FA4BC) has state 0x259 at `+0x34`; the stack argument is unused.
// Translated from 0x00B6AE40 (DDDA.exe 2017-12-13 Steam), uPlayerBase::vf187.
pub fn player_vf187(scene_state: u32, value_207c: &mut f32) {
    if scene_state == 0x259 {
        *value_207c = 0.0;
    }
}

/// `uPlayerBase::vf100`: a byte from the exe's 256-wide table 0x014F2AF0,
/// row `+0x5720`, column `arg` (`table` reads the installed exe).
// Translated from 0x00B531B0 (DDDA.exe 2017-12-13 Steam), uPlayerBase::vf100.
pub fn player_vf100(row_5720: u32, arg: u32, table: impl Fn(u32) -> u8) -> u8 {
    table((row_5720 << 8).wrapping_add(arg))
}

/// `uPlayerBase::vf85`: the world position of the joint the model's bone
/// map names at index 3 (`+0x368` map, `+0x364` joint array, 0xA0 bytes
/// per joint, its world row at joint+0x40); without that joint (map byte
/// 0xFF, or a joint address that wraps to zero) the model's own world
/// origin (`+0xE0`, as `uObjModel::vf85`).
// Translated from 0x00B84300 (DDDA.exe 2017-12-13 Steam), uPlayerBase::vf85.
pub fn player_joint3_position(map_byte3: u8, joints_364: u32,
    joint_row: impl Fn(u32) -> [f32; 4], origin: [f32; 4]) -> [f32; 4] {
    let idx = u32::from(map_byte3);
    let joint = joints_364.wrapping_add(idx * 0xA0);
    let row = if idx == 0xFF || joint == 0 { origin } else { joint_row(joint) };
    row.map(crate::x87_copy)
}
