//! `cParticleManager` (effect generators).

/// The generator counter words (`+0x3C` flags, `+0x44`/`+0x48` counters,
/// `+0x4C` seed).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Generator {
    pub flags_3c: u32,
    pub counter_44: u32,
    pub counter_48: u32,
    pub seed_4c: u32,
}

/// Restarts both counters from the seed's low 16 bits.
// Translated from 0x010FA570 (DDDA.exe 2017-12-13 Steam), cParticleManager::vf08.
pub fn reset_counters(g: &mut Generator) {
    let v = g.seed_4c & 0xFFFF;
    g.counter_48 = v;
    g.counter_44 = v;
}

/// Clears flag 0x10000000 when `arg` (a byte) is nonzero.
// Translated from 0x010FA5B0 (DDDA.exe 2017-12-13 Steam), cParticleManager::vf10.
pub fn clear_flag(g: &mut Generator, arg: u8) {
    if arg != 0 {
        g.flags_3c &= !0x1000_0000;
    }
}
