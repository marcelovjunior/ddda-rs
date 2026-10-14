//! Flat-buffer form of the exact 0x00cf2cb0 decoder (zlib `inflate_fast`).
//!
//! The decoder in `archive_inflate.rs` reads and writes guest memory byte by
//! byte through page checks. This one gathers what a stretch of the call can
//! touch into host buffers first: the input bytes (bounded by what the
//! output limit can consume), the output with the history a match may refer
//! to, and the two root decode tables. It decodes on plain slices and writes
//! the produced bytes back in bulk. The decisions, the bit buffer, the match
//! semantics (a forward copy reads bytes the match has just written), the
//! window fallbacks, every stack spill and the final registers are the
//! original's, as in the `WIDE` decoder; the same recorded vectors check it.
//!
//! A long call runs in segments: the original loop re-tests its limits after
//! every symbol, so ending a gathered segment at such a boundary (when fewer
//! than a few hundred bytes of room remain) and gathering the next one changes
//! nothing it does. Calls too small to repay the gather, or whose geometry the
//! gather cannot cover, return `false` before writing anything and run the
//! per-byte decoder.
use ddnative_base::{mem::Mem, x86::Cpu};

/// Below this many output bytes a call is not worth gathering for.
const MINIMUM_OUTPUT: u32 = 1024;
/// The largest deflate distance, and so the history a match can need.
const HISTORY: u32 = 32768;
/// Output bytes gathered per segment (plus the history before them).
const SEGMENT_OUTPUT: u32 = 1 << 18;
/// Input bytes gathered per segment: at most what [`SEGMENT_OUTPUT`] can
/// consume (two bytes per output byte) plus the bit buffer and refills.
const SEGMENT_INPUT: u32 = 2 * SEGMENT_OUTPUT + 64;
/// Slack at a segment's end: one decode step writes at most 258 bytes and
/// reads at most six, and the bit buffer reads two ahead.
const OUTPUT_MARGIN: u32 = 300;
const INPUT_MARGIN: u32 = 64;

fn mask(bits: u32) -> u32 {
    1u32.wrapping_shl(bits).wrapping_sub(1)
}

/// One decode table: the root entries copied from the guest (the table is
/// immutable during the call), longer codes read from the guest.
struct Table {
    base: u32,
    entries: u32,
    root: Vec<u32>,
}

impl Table {
    fn new(m: &dyn Mem, base: u32, entries: u32) -> Option<Table> {
        if entries > 4096 {
            return None;
        }
        let mut bytes = vec![0u8; entries as usize * 4];
        m.read_bytes(base, &mut bytes);
        let root = bytes.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        Some(Table { base, entries, root })
    }
    #[inline(always)]
    fn read(&self, m: &dyn Mem, index: u32) -> u32 {
        if index < self.entries {
            self.root[index as usize]
        } else {
            m.u32(self.base.wrapping_add(index.wrapping_mul(4)))
        }
    }
}

/// The bit buffer over a gathered span of the input. `input` is a guest
/// address one byte before the next input byte, as in the original; `base`
/// is the guest address of `bytes[0]`.
struct Bits {
    bytes: Vec<u8>,
    base: u32,
    input: u32,
    hold: u32,
    count: u32,
}

impl Bits {
    #[inline(always)]
    fn at(&self, address: u32) -> u32 {
        // A read past the gathered span cannot happen under the caller's
        // guarantees (at least six input bytes at each loop head)
        self.bytes[address.wrapping_sub(self.base) as usize] as u32
    }
    #[inline(always)]
    fn byte(&mut self) {
        self.input = self.input.wrapping_add(1);
        self.hold = self.hold.wrapping_add(self.at(self.input).wrapping_shl(self.count));
        self.count += 8;
    }
    #[inline(always)]
    fn pair_if_needed(&mut self) {
        if self.count < 15 {
            let index = self.input.wrapping_add(1).wrapping_sub(self.base) as usize;
            let pair = match self.bytes.get(index..index + 2) {
                Some(pair) => u32::from(pair[0]) | u32::from(pair[1]) << 8,
                None => 0,
            };
            self.input = self.input.wrapping_add(2);
            self.hold = self.hold.wrapping_add(pair.wrapping_shl(self.count));
            self.count += 16;
        }
    }
    #[inline(always)]
    fn take(&mut self, count: u32) -> u32 {
        let value = self.hold & mask(count);
        self.hold = self.hold.wrapping_shr(count);
        self.count = self.count.wrapping_sub(count);
        value
    }
    /// Gather the guest bytes from `address` (`remaining` are left in the
    /// stream) as the new span; returns how many stream bytes it holds.
    fn gather(&mut self, m: &dyn Mem, address: u32, remaining: u32) -> u32 {
        let wanted = remaining.min(SEGMENT_INPUT);
        if self.bytes.len() < SEGMENT_INPUT as usize + 8 {
            self.bytes.resize(SEGMENT_INPUT as usize + 8, 0);
        }
        let real = (wanted + 8).min(remaining) as usize;
        m.read_bytes(address, &mut self.bytes[..real]);
        self.bytes[real..wanted as usize + 8].fill(0);
        self.base = address;
        wanted
    }
}

/// The produced bytes and the history before them, addressed by guest address.
struct Output {
    bytes: Vec<u8>,
    /// guest address of `bytes[0]`
    base: u32,
}

impl Output {
    #[inline(always)]
    fn index(&self, address: u32) -> usize {
        address.wrapping_sub(self.base) as usize
    }
}

/// The window, loaded on first use.
struct Window {
    start: u32,
    size: u32,
    bytes: Option<Vec<u8>>,
}

impl Window {
    fn contains(&self, address: u32) -> bool {
        self.size != 0 && address.wrapping_sub(self.start) < self.size
    }
    fn byte(&mut self, m: &dyn Mem, address: u32) -> u8 {
        let (start, size) = (self.start, self.size);
        let bytes = self.bytes.get_or_insert_with(|| {
            let mut bytes = vec![0u8; size as usize];
            m.read_bytes(start, &mut bytes);
            bytes
        });
        bytes[address.wrapping_sub(start) as usize]
    }
}

/// The original's forward copy of `count` bytes: `source` and `output` are
/// guest addresses one before the next byte, each byte is read after the
/// previous one was written. A source in the window is read from there.
fn copy(m: &dyn Mem, out: &mut Output, window: &mut Window, output: &mut u32, source: &mut u32, count: u32) {
    if !window.contains(source.wrapping_add(1)) {
        // inside the output buffer: the overlap-aware fast copy
        let to = out.index(output.wrapping_add(1));
        let from = out.index(source.wrapping_add(1));
        copy_within_forward(&mut out.bytes, from, to, count as usize);
        *source = source.wrapping_add(count);
        *output = output.wrapping_add(count);
        return;
    }
    // window bytes, one at a time (rare)
    for _ in 0..count {
        let s = source.wrapping_add(1);
        let byte = if window.contains(s) { window.byte(m, s) } else { out.bytes[out.index(s)] };
        let d = out.index(output.wrapping_add(1));
        out.bytes[d] = byte;
        *source = s;
        *output = output.wrapping_add(1);
    }
}

/// `bytes[to..to + count] = bytes[from..from + count]` as a forward byte copy
/// (`from < to`; they overlap when `to - from < count`).
#[inline(always)]
fn copy_within_forward(bytes: &mut [u8], from: usize, to: usize, count: usize) {
    if count == 0 {
        return;
    }
    let distance = to.wrapping_sub(from);
    if from + count <= to {
        bytes.copy_within(from..from + count, to);
    } else if distance == 1 {
        let byte = bytes[from];
        bytes[to..to + count].fill(byte);
    } else if count <= 16 {
        for i in 0..count {
            bytes[to + i] = bytes[from + i];
        }
    } else {
        // periodic: the seed, then the produced run doubled (always a whole
        // number of periods, so the pattern stays aligned)
        bytes.copy_within(from..from + distance, to);
        let mut done = distance;
        while done < count {
            let chunk = done.min(count - done);
            bytes.copy_within(to..to + chunk, to + done);
            done += chunk;
        }
    }
}

/// A match whose source lies in the output buffer (the common case): the same
/// forward copy as [`copy`] without the window checks, in eight-byte steps
/// when the distance allows them. Bytes beyond the run may be overwritten
/// (they are produced later), never read before that.
#[inline(always)]
fn copy_local(out: &mut Output, output: &mut u32, source: &mut u32, count: u32) {
    let to = out.index(output.wrapping_add(1));
    let from = out.index(source.wrapping_add(1));
    let count_usize = count as usize;
    let bytes = &mut out.bytes[..];
    if to - from >= 8 && to + count_usize + 8 <= bytes.len() {
        let base = bytes.as_mut_ptr();
        let mut i = 0usize;
        while i < count_usize {
            // SAFETY: from + i + 8 <= to + i < len (distance >= 8, bounds checked above)
            unsafe {
                let word = std::ptr::read_unaligned(base.add(from + i) as *const u64);
                std::ptr::write_unaligned(base.add(to + i) as *mut u64, word);
            }
            i += 8;
        }
    } else {
        copy_within_forward(bytes, from, to, count_usize);
    }
    *source = source.wrapping_add(count);
    *output = output.wrapping_add(count);
}

thread_local! {
    /// Reused buffers (their contents are always gathered or written before use).
    static SCRATCH: std::cell::RefCell<(Vec<u8>, Vec<u8>)> = const { std::cell::RefCell::new((Vec::new(), Vec::new())) };
}

/// Run one 0x00cf2cb0 call on gathered buffers. `false`: not handled, nothing
/// was written to guest memory.
pub(super) fn run(m: &mut dyn Mem, cpu: &mut Cpu) -> bool {
    let (input_scratch, output_scratch) = SCRATCH.with(|scratch| std::mem::take(&mut *scratch.borrow_mut()));
    let (handled, input_scratch, output_scratch) = run_with(m, cpu, input_scratch, output_scratch);
    SCRATCH.with(|scratch| *scratch.borrow_mut() = (input_scratch, output_scratch));
    handled
}

fn run_with(m: &mut dyn Mem, cpu: &mut Cpu, input_scratch: Vec<u8>, output_scratch: Vec<u8>) -> (bool, Vec<u8>, Vec<u8>) {
    let frame = cpu.esp.wrapping_sub(4);
    let stream = m.u32(cpu.esp.wrapping_add(4));
    let start = m.u32(cpu.esp.wrapping_add(8));
    let state = m.u32(stream.wrapping_add(0x1c));
    let available = m.u32(stream.wrapping_add(0x10));
    let input_length = m.u32(stream.wrapping_add(4));
    // geometry the gather covers
    if available < MINIMUM_OUTPUT || start < available || input_length < 6 {
        return (false, input_scratch, output_scratch);
    }
    let previous = (start - available).min(HISTORY);
    let next_out = m.u32(stream.wrapping_add(0xc));
    let next_in = m.u32(stream);
    let window_size = m.u32(state.wrapping_add(0x20));
    if window_size > 65536 {
        return (false, input_scratch, output_scratch);
    }
    let literal_mask = mask(m.u32(state.wrapping_add(0x4c)));
    let distance_mask = mask(m.u32(state.wrapping_add(0x50)));
    let literals_44 = m.u32(state.wrapping_add(0x44));
    let distances_48 = m.u32(state.wrapping_add(0x48));
    let (Some(literal_table), Some(distance_table)) =
        (Table::new(m, literals_44, literal_mask.wrapping_add(1)), Table::new(m, distances_48, distance_mask.wrapping_add(1)))
    else {
        return (false, input_scratch, output_scratch);
    };

    // input segment: from the next byte, as much as the output segment can consume
    let mut bits = Bits {
        bytes: input_scratch,
        base: next_in,
        input: next_in.wrapping_sub(1),
        hold: m.u32(state.wrapping_add(0x30)),
        count: m.u32(state.wrapping_add(0x34)),
    };
    let gathered = bits.gather(m, next_in, input_length);
    // output segment: the history, then room for SEGMENT_OUTPUT bytes (a call
    // that fits in one segment needs only what it can write)
    let mut out = Output { bytes: output_scratch, base: next_out.wrapping_sub(previous) };
    let length_needed = if available > SEGMENT_OUTPUT { HISTORY + SEGMENT_OUTPUT + OUTPUT_MARGIN } else { previous + available + OUTPUT_MARGIN };
    if out.bytes.len() < (HISTORY + SEGMENT_OUTPUT + OUTPUT_MARGIN + 16) as usize {
        out.bytes.resize((HISTORY + SEGMENT_OUTPUT + OUTPUT_MARGIN + 16) as usize, 0);
    }
    m.read_bytes(out.base, &mut out.bytes[..previous as usize]);
    let window_2c = m.u32(state.wrapping_add(0x2c));
    let mut window = Window { start: window_2c, size: window_size, bytes: None };

    let window_have_24 = m.u32(state.wrapping_add(0x24));
    let window_write_28 = m.u32(state.wrapping_add(0x28));
    let window_size_20 = window_size;
    let input_end = bits.input.wrapping_add(input_length).wrapping_sub(5);
    let mut output = next_out.wrapping_sub(1);
    let beginning = output.wrapping_add(available.wrapping_sub(start));
    let output_end = output.wrapping_add(available).wrapping_sub(257);
    // Segment limits, as offsets from the segment base: the input (None: the
    // whole remaining input is gathered) and the output.
    let mut input_limit = (gathered < input_length).then(|| gathered - INPUT_MARGIN);
    let output_room = length_needed;
    let output_limit = |_: &Output| output_room - OUTPUT_MARGIN;
    let mut segment_output_limit = output_limit(&out);
    // everything below `flushed` is already written back to the guest
    let mut flushed = next_out;

    // the stack spills the original makes at entry
    m.set_u32(frame, cpu.ebp);
    m.set_u32(frame.wrapping_sub(0x40), cpu.ebx);
    m.set_u32(frame.wrapping_sub(0x44), cpu.esi);
    m.set_u32(frame.wrapping_sub(0x48), cpu.edi);
    for (offset, value) in [
        (0x0c, input_end),
        (0x10, state),
        (0x14, window_write_28),
        (0x1c, literals_44),
        (0x20, distances_48),
        (0x24, window_size_20),
        (0x28, window_2c),
        (0x2c, output_end),
        (0x30, distance_mask),
        (0x34, beginning),
        (0x38, window_have_24),
        (0x3c, literal_mask),
    ] {
        m.set_u32(frame.wrapping_sub(offset), value);
    }

    let mut last_distance = None;
    let mut last_auxiliary = None;
    let mut length = 1u32;
    'decode: loop {
        bits.pair_if_needed();
        let mut code = literal_table.read(m, bits.hold & literal_mask);
        bits.take((code >> 8) & 255);
        loop {
            let operation = code & 255;
            if operation == 0 {
                output = output.wrapping_add(1);
                let at = out.index(output);
                out.bytes[at] = (code >> 16) as u8;
                break;
            }
            if operation & 16 != 0 {
                length = code >> 16;
                let extra = operation & 15;
                if extra != 0 {
                    if bits.count < extra {
                        bits.byte();
                    }
                    length += bits.take(extra);
                }
                bits.pair_if_needed();
                code = distance_table.read(m, bits.hold & distance_mask);
                bits.take((code >> 8) & 255);
                last_auxiliary = Some(code);
                while code & 16 == 0 {
                    if code & 64 != 0 {
                        // Error text remains in the player's mapped executable.
                        m.set_u32(stream.wrapping_add(0x18), 0x0140_3e78); // 0x00cf3039
                        m.set_u32(state, 27);
                        break 'decode;
                    }
                    let index = (code >> 16).wrapping_add(bits.hold & mask(code & 255));
                    code = distance_table.read(m, index);
                    bits.take((code >> 8) & 255);
                    last_auxiliary = Some(code);
                }
                let extra = code & 15;
                let base = code >> 16;
                if bits.count < extra {
                    bits.byte();
                    if bits.count < extra {
                        bits.byte();
                    }
                }
                let low = bits.take(extra);
                let distance = base.wrapping_add(low);
                last_auxiliary = Some(low);
                last_distance = Some(distance);
                let produced = output.wrapping_sub(beginning);
                let mut source;
                if distance > produced {
                    let back = distance - produced;
                    last_auxiliary = Some(back);
                    if back > window_have_24 {
                        m.set_u32(stream.wrapping_add(0x18), 0x0140_3e58); // 0x00cf3027
                        m.set_u32(state, 27);
                        break 'decode;
                    }
                    source = window_2c.wrapping_sub(1);
                    if window_write_28 == 0 {
                        source = source.wrapping_add(window_size_20.wrapping_sub(back));
                        if back < length {
                            length -= back;
                            copy(m, &mut out, &mut window, &mut output, &mut source, back);
                            source = output.wrapping_sub(distance);
                        }
                    } else if window_write_28 < back {
                        source = source.wrapping_add(window_write_28.wrapping_sub(back)).wrapping_add(window_size_20);
                        let tail = back - window_write_28;
                        if tail < length {
                            length -= tail;
                            copy(m, &mut out, &mut window, &mut output, &mut source, tail);
                            source = window_2c.wrapping_sub(1);
                            if window_write_28 < length {
                                length -= window_write_28;
                                last_auxiliary = Some(window_write_28);
                                copy(m, &mut out, &mut window, &mut output, &mut source, window_write_28);
                                source = output.wrapping_sub(distance);
                            }
                        }
                    } else {
                        source = source.wrapping_add(window_write_28 - back);
                        if back < length {
                            length -= back;
                            copy(m, &mut out, &mut window, &mut output, &mut source, back);
                            source = output.wrapping_sub(distance);
                        }
                    }
                } else {
                    source = output.wrapping_sub(distance);
                }
                if window.contains(source.wrapping_add(1)) {
                    copy(m, &mut out, &mut window, &mut output, &mut source, length);
                } else {
                    copy_local(&mut out, &mut output, &mut source, length);
                }
                length %= 3;
                break;
            }
            if operation & 64 != 0 {
                if operation & 32 != 0 {
                    m.set_u32(state, 11);
                } else {
                    m.set_u32(stream.wrapping_add(0x18), 0x0140_3e90); // 0x00cf3055
                    m.set_u32(state, 27);
                }
                break 'decode;
            }
            let index = (code >> 16).wrapping_add(bits.hold & mask(operation));
            code = literal_table.read(m, index);
            bits.take((code >> 8) & 255);
        }
        if bits.input >= input_end || output >= output_end {
            break;
        }
        // The original keeps going; only the gathered segment ends here.
        let input_done = input_limit.is_some_and(|limit| bits.input.wrapping_add(1).wrapping_sub(bits.base) >= limit);
        if input_done || output.wrapping_sub(out.base) >= segment_output_limit {
            // write back what is produced, keep the history, gather the next segment
            let count = output.wrapping_add(1).wrapping_sub(flushed) as usize;
            let from = out.index(flushed);
            crate::fastmem::write_bulk(m, flushed, &out.bytes[from..from + count]);
            flushed = output.wrapping_add(1);
            let keep = flushed.wrapping_sub(next_out).min(HISTORY);
            let keep_from = out.index(flushed.wrapping_sub(keep));
            out.bytes.copy_within(keep_from..keep_from + keep as usize, 0);
            out.base = flushed.wrapping_sub(keep);
            segment_output_limit = output_limit(&out);
            if input_done {
                let next = bits.input.wrapping_add(1);
                let remaining = next_in.wrapping_add(input_length).wrapping_sub(next);
                let wanted = bits.gather(m, next, remaining);
                input_limit = (wanted < remaining).then(|| wanted - INPUT_MARGIN);
            }
        }
    }
    // write the rest of the produced bytes back, in order
    let count = output.wrapping_add(1).wrapping_sub(flushed) as usize;
    if count != 0 {
        let from = out.index(flushed);
        crate::fastmem::write_bulk(m, flushed, &out.bytes[from..from + count]);
    }
    // the final spills, bit buffer and stream fields (see the WIDE decoder)
    if let Some(value) = last_auxiliary {
        m.set_u32(frame.wrapping_sub(0x18), value);
    }
    if let Some(value) = last_distance {
        m.set_u32(frame.wrapping_sub(8), value);
    }
    let input = bits.input.wrapping_sub(bits.count >> 3);
    let count = bits.count & 7;
    let hold = bits.hold & mask(count);
    let input_left = input_end.wrapping_sub(input).wrapping_add(5);
    m.set_u32(frame.wrapping_sub(4), input);
    m.set_u32(frame.wrapping_add(12), length);
    m.set_u32(stream, input.wrapping_add(1));
    m.set_u32(stream.wrapping_add(0xc), output.wrapping_add(1));
    m.set_u32(stream.wrapping_add(0x10), output_end.wrapping_sub(output).wrapping_add(257));
    m.set_u32(stream.wrapping_add(4), input_left);
    m.set_u32(state.wrapping_add(0x34), count);
    m.set_u32(state.wrapping_add(0x30), hold);
    cpu.eax = state;
    cpu.ecx = stream;
    cpu.edx = input_left;
    let before_add = output_end.wrapping_sub(output);
    cpu.fl = ddnative_base::x86::Flags::new(ddnative_base::x86::ADD, before_add, 257, before_add.wrapping_add(257), 32);
    cpu.esp = cpu.esp.wrapping_add(4);
    (true, bits.bytes, out.bytes)
}
