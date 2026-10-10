//! `cActionBase` and AI action interfaces.

/// The owner fields `cActionBase::vf05` touches (the action reaches its
/// owner through `action+0x44`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Owner {
    /// +0x206C: category flags (bit 0x2: the Arisen/player category).
    pub category_206c: u32,
    /// +0x2370, +0x2374: per-action words cleared on entry.
    pub word_2370: u32,
    pub word_2374: u32,
    /// +0x273C: body state flags.
    pub flags_273c: u32,
    /// +0x2E34: current status bits; +0x2E38: the pending ones.
    pub status_2e34: u32,
    pub pending_2e38: u32,
}

/// `cActionBase::vf05`, the entry of every action: clears the owner's body
/// flags (player category), the action's byte `+0x50` (returned) and the
/// owner's per-action words, and takes over pending status bits, promoting
/// 0x40 to 0x100000 and 0x400 to 0x4000000.
// Translated from 0x007F1890 (DDDA.exe 2017-12-13 Steam), cActionBase::vf05.
pub fn base_enter(o: &mut Owner) -> u8 {
    if o.category_206c & 2 != 0 {
        o.flags_273c = 0;
    }
    o.word_2370 = 0;
    o.word_2374 = 0;
    if o.category_206c & 2 != 0 {
        let pending = o.pending_2e38;
        if pending != 0 {
            o.status_2e34 = pending;
            if pending & 0x40 != 0 {
                o.status_2e34 = (pending & !0x40) | 0x0010_0000;
            }
            // tests the pending value, edits the current one (0x007F18E7)
            if pending & 0x400 != 0 {
                o.status_2e34 = (o.status_2e34 & !0x400) | 0x0400_0000;
            }
        }
    }
    0 // action +0x50
}

/// The interface's owner as `cAIActionInterfaceDemo::vf06` sees it
/// (`interface+4`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DemoOwner {
    /// +0x0E: a byte that makes the query report 2 without clearing.
    pub byte_0e: u8,
    /// +0x40: optional work block; its bytes +0x10..+0x34 (nine words).
    pub work_40: Option<[u32; 9]>,
}

/// `cAIActionInterfaceDemo::vf06`: the interface state query. Returns 2
/// without an owner or with the owner's byte +0x0E set; otherwise clears
/// the owner's work block (if any) and returns 4.
// Translated from 0x00572860 (DDDA.exe 2017-12-13 Steam), cAIActionInterfaceDemo::vf06.
pub fn demo_state(owner: Option<&mut DemoOwner>) -> u32 {
    let Some(o) = owner else { return 2 };
    if o.byte_0e != 0 {
        return 2;
    }
    if let Some(w) = o.work_40.as_mut() {
        *w = [0; 9];
    }
    4
}

/// `cActionBase::vf08`: clears the owner's current status bits (player
/// category only).
// Translated from 0x0089EFE0 (DDDA.exe 2017-12-13 Steam), cActionBase::vf08.
pub fn base_exit_status(o: &mut Owner) {
    if o.category_206c & 2 != 0 {
        o.status_2e34 = 0;
    }
}

/// The initial state an action constructor gives the first 0x54 bytes of a
/// new action object: the class vtable at +0x00, zero in every word of
/// +0x04..+0x48 (the floats +0x24..+0x40 are +0.0), and a zero byte at
/// +0x50. Bytes +0x4C..+0x50 and +0x51.. are left as they were.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewAction {
    pub vtable: u32,
}

impl NewAction {
    /// Writes the constructor's fields into an object image.
    pub fn write(&self, obj: &mut [u8]) {
        obj[0..4].copy_from_slice(&self.vtable.to_le_bytes());
        obj[4..0x4C].fill(0);
        obj[0x50] = 0;
    }
}

/// The in-place constructor shared by 957 action classes (`vf16` of
/// cEm*Act*, cAI*, ...): with memory, initialises it for `vtable` and
/// returns it; without (a null pointer), returns null. Each class differs
/// only in its vtable constant (`research/native` finds the family).
// Translated from 0x008C7780 (DDDA.exe 2017-12-13 Steam), cEm0100Action::vf16,
// and every same-body constructor listed in the private family file.
pub fn construct(memory: Option<&mut [u8]>, vtable: u32) -> bool {
    let Some(obj) = memory else { return false };
    NewAction { vtable }.write(obj);
    true
}

/// The extended action constructor (object in EAX, no null check): the
/// base fields of [`NewAction`], then `+0x54 = -1`, zero words at +0x5C,
/// +0x64, +0x6C, +0x70 and +0.0 at +0x60, +0x68. Bytes +0x4C..+0x50,
/// +0x51..+0x54 and +0x58..+0x5C are left as they were.
// Translated from 0x004CBBE0 (DDDA.exe 2017-12-13 Steam), and every
// same-body constructor in the private family file (272 classes).
pub fn construct_extended(obj: &mut [u8], vtable: u32) {
    NewAction { vtable }.write(obj);
    obj[0x54..0x58].copy_from_slice(&u32::MAX.to_le_bytes());
    for off in [0x5C, 0x60, 0x64, 0x68, 0x6C, 0x70] {
        obj[off..off + 4].fill(0);
    }
}

/// The constructor of [`construct`] plus `+0x8C = +0.0` (cEm5400, cEm5500
/// and other classes with a float at +0x8C).
// Translated from 0x004DFA00 (DDDA.exe 2017-12-13 Steam), and every
// same-body constructor in the private family file.
pub fn construct_with_8c(memory: Option<&mut [u8]>, vtable: u32) -> bool {
    let Some(obj) = memory else { return false };
    NewAction { vtable }.write(obj);
    obj[0x8C..0x90].fill(0);
    true
}

/// `cPlActTired::vf09`, the action's message handler: message 0x11 sets
/// the byte at +0x84, 0x12 the byte at +0x85; it returns 0 for every
/// message (four stack arguments, only the first read).
// Translated from 0x00ACC4A0 (DDDA.exe 2017-12-13 Steam), cPlActTired::vf09.
pub fn tired_message(msg: u32, byte_84: &mut u8, byte_85: &mut u8) -> u32 {
    match msg {
        0x11 => *byte_84 = 1,
        0x12 => *byte_85 = 1,
        _ => {}
    }
    0
}

/// `cPlActWpnWandBase::vf10`: drops the dead unit references at +0x74 and
/// +0x78 (see `unit::drop_dead_refs`), then the one at +0x1E4 unless its
/// target's count at +0x50 is at most 0 (signed) and its byte at +0x4C is
/// nonzero. `target` reads (count_50, byte_4c) of the +0x1E4 target.
// Translated from 0x00B44FB0 (DDDA.exe 2017-12-13 Steam), cPlActWpnWandBase::vf10.
pub fn wand_base_refs(refs_74_78: &mut [u32; 2], ref_1e4: &mut u32,
    state_word: impl Fn(u32) -> u32, target: impl Fn(u32) -> (i32, u8)) {
    crate::unit::drop_dead_refs(refs_74_78, state_word);
    if *ref_1e4 != 0 {
        let (count, flag) = target(*ref_1e4);
        if count > 0 || flag == 0 {
            *ref_1e4 = 0;
        }
    }
}

/// The action fields the enemy-family action entry writes (+0x50..+0x78).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnemyAction {
    pub byte_50: u8,
    /// +0x54: the owner, when its category has bit 0x80; else 0.
    pub owner_54: u32,
    pub word_58: u32,
    pub word_60: u32,
    pub float_64: f32,
    pub byte_6c: u8,
    pub word_70: u32,
    /// +0x74: the owner, when it is-a the family's class; else 0.
    pub owner_74: u32,
}

/// The base entry of the enemy-family actions (cEm0600Action ...
/// cEm9807Action, cEmDieBoneAction: 16 classes, `vf05`): runs
/// `cActionBase::vf05` ([`base_enter`]), keeps the owner at +0x54 when its
/// category has bit 0x80 (and then sets the owner's +0x2BA0 to -1), resets
/// the action's fields, and keeps the owner at +0x74 when it is-a the
/// family's class. `owner_is_class` is that DTI walk (the owner's
/// `vf4`/+0x10 DTI, then parents at +0x10, comparing the id at +4 with
/// the family's class-id global); it is called on every entry.
// Translated from 0x004E0EB0 (DDDA.exe 2017-12-13 Steam), cEm9100Action::vf05,
// and the 15 same-body enemy-family action entries.
pub fn enemy_action_enter(act: &mut EnemyAction, owner_addr: u32, owner: &mut Owner,
    owner_2ba0: &mut u32, owner_is_class: impl FnOnce() -> bool) {
    act.byte_50 = base_enter(owner);
    act.owner_54 = if owner_addr != 0 && owner.category_206c & 0x80 != 0 { owner_addr } else { 0 };
    act.word_58 = 0;
    act.word_60 = 0;
    act.word_70 = 0;
    act.float_64 = 0.0;
    act.byte_6c = 1;
    if act.owner_54 != 0 {
        *owner_2ba0 = u32::MAX;
    }
    act.owner_74 = if owner_is_class() { owner_addr } else { 0 };
}
