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
