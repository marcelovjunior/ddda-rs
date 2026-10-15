//! Sound-engine leaves. Same ABI-level conventions as `semantic_l190.rs`; oracle in
//! `crates/mtcore/tests/semantic_sound.rs` (vectors from `research/native/record_filter_bank.py`).
use ddnative_base::{
    mem::{Mem, Pages},
    x86::Cpu,
};

/// The channel count of the engine object.
const CHANNELS: u32 = 0x270;
/// Coefficient arrays of the object, four lanes each (4 bytes a lane): the input gain `b0`, then
/// the taps on the last input `b1`, the one before `b2`, the last output `a1` and the one before `a2`.
const GAIN: u32 = 0x70;
const TAPS: [u32; 4] = [0x80, 0x90, 0xA0, 0xB0];
/// The first channel record of lane 0 plus 0xC: records are 0x40 apart, lanes 16 bytes apart, and
/// a record holds `x[n-1]` at -0xC, `x[n-2]` at -8, `y[n-1]` at -4 and `y[n-2]` at 0.
const RECORDS: u32 = 0xCC;
const RECORD_SIZE: u32 = 0x40;
const LANE_SIZE: u32 = 16;
/// Constants in the image: the flush-to-zero threshold (f32) and the sign-clearing mask (low dword).
const FLUSH_THRESHOLD: u32 = 0x0142_abe8;
const ABS_MASK: u32 = 0x0161_c330;

// Translated from 0x00dcdde0 (DDDA.exe 2017-12-13 Steam).
/// 0x00DCDDE0, `engine->filter_bank(buffer, rounds, lane)` (thiscall, `ret 0xC`, no result): runs
/// `rounds` rounds of a direct-form-I biquad over the engine's channels, one lane of coefficients,
/// in place. Each round takes one sample per channel from `buffer`, which advances through the
/// buffer (round after round, channel after channel), and stores the filtered sample back:
///
/// `y = x*b0 + ((x[n-1]*b1 + x[n-2]*b2) + y[n-1]*a1) + a2*y[n-2]`
///
/// summed in that order (float addition is not associative), and flushed to +0 when `|y|` is
/// below the threshold (a comparison that is false for NaN). Float arithmetic is plain `f32`;
/// NaN payload order of two NaN operands is not reproduced.
///
/// Registers as the original leaves them: EAX the last record visited (the rounds when no record
/// was), EDX the buffer pointer after the last sample (the caller's when there are no rounds), ECX,
/// EBX, ESI, EDI unchanged. The flags are
/// not reproduced (the only caller is a `call; ret 0x14` thunk).
pub fn filter_bank(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let mut buffer = pg.u32(m, entry.wrapping_add(4));
    let rounds = pg.u32(m, entry.wrapping_add(8));
    let lane = pg.u32(m, entry.wrapping_add(12));
    let float = |m: &dyn Mem, at: u32| f32::from_bits(pg.u32(m, at));
    // the tap coefficients are read before the rounds are looked at, as the original does
    let [b1, b2, a1, a2] = TAPS.map(|base| float(m, this.wrapping_add(base).wrapping_add(lane.wrapping_mul(4))));

    let mut last_record = rounds;
    if rounds != 0 {
        for _ in 0..rounds {
            let channels = |m: &dyn Mem| pg.u32(m, this.wrapping_add(CHANNELS));
            if channels(m) == 0 {
                continue;
            }
            let mut record = this.wrapping_add(lane.wrapping_mul(LANE_SIZE)).wrapping_add(RECORDS);
            let mut channel = 0u32;
            loop {
                let x1 = pg.u32(m, record.wrapping_sub(0xC));
                let x2 = f32::from_bits(pg.u32(m, record.wrapping_sub(8)));
                let y1 = pg.u32(m, record.wrapping_sub(4));
                let mut sum = f32::from_bits(x1) * b1;
                sum += x2 * b2;
                sum += f32::from_bits(y1) * a1;
                pg.set_u32(m, record.wrapping_sub(8), x1);
                let tail = a2 * float(m, record);
                pg.set_u32(m, record, y1);
                let x = float(m, buffer);
                pg.set_u32(m, record.wrapping_sub(0xC), x.to_bits());
                let scaled = x * float(m, this.wrapping_add(GAIN).wrapping_add(lane.wrapping_mul(4)));
                sum += tail;
                let mut y = scaled + sum;
                let threshold = float(m, FLUSH_THRESHOLD);
                let magnitude = f32::from_bits(y.to_bits() & pg.u32(m, ABS_MASK));
                if threshold > magnitude {
                    y = 0.0;
                }
                pg.set_u32(m, record.wrapping_sub(4), y.to_bits());
                pg.set_u32(m, buffer, y.to_bits());
                channel += 1;
                buffer = buffer.wrapping_add(4);
                record = record.wrapping_add(RECORD_SIZE);
                if channel >= channels(m) {
                    break;
                }
            }
            last_record = record;
        }
    }
    c.eax = last_record;
    if rounds != 0 {
        // the original loads EDX only once it has rounds to run
        c.edx = buffer;
    }
    c.esp = entry.wrapping_add(4 + 12);
}
