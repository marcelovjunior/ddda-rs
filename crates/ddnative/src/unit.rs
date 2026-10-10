//! Unit references held by actions and actors.

/// A referenced unit stays referenced while the low 3 bits of its word at
/// +0x04 (its unit state) are 1 or 2.
pub fn unit_alive(state_word_04: u32) -> bool {
    matches!(state_word_04 & 7, 1 | 2)
}

/// Clears each held unit reference (0 = none) whose unit is no longer in
/// state 1 or 2, in slot order. `state_word` reads the referenced unit's
/// word at +0x04.
///
/// The `vf10` of 23 actions is this function over one field offset each
/// (cPlActWait +0x8C, cPlActBallistaCtrl +0x84, cPlActHagaijime +0x90,
/// cNpcActReaction, cEm0100ActHagaijimeKick, cEm0200ContinueBite, ...);
/// `research/native/patterns.py drop_dead_refs` finds them and lists each
/// one's offset.
// Translated from 0x004F57E0 (DDDA.exe 2017-12-13 Steam), and every
// function of the drop_dead_refs pattern family.
pub fn drop_dead_refs(refs: &mut [u32], state_word: impl Fn(u32) -> u32) {
    for r in refs {
        if *r != 0 && !unit_alive(state_word(*r)) {
            *r = 0;
        }
    }
}
