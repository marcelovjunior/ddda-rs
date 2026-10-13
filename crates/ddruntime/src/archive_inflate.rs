//! Exact archive zlib decoder ports. The outer entry and page access policy
//! are separately opt-in; allocation and the archive stream remain original.
use ddnative_base::{mem::Mem, x86::Cpu};

#[path = "zlib_access.rs"]
mod access;
#[path = "zlib_port.rs"]
pub mod port;
#[path = "zlib_table.rs"]
pub mod table;
#[path = "archive_skip.rs"]
pub mod stream;
use access::Access;

/// Install before diagnostic wrappers, preserving explicit ports/mods.
pub fn install(ports: &mut Vec<(u32, ddnative_base::x86::LiftFn)>) {
    if std::env::var("DDRUNTIME_ARCHIVE_SKIP").as_deref() == Ok("1")
        && !ports.iter().any(|&(address, _)| address == 0x00cf_eec0)
    {
        ports.push((0x00cf_eec0, stream::skip));
    }
    if std::env::var("DDRUNTIME_ZLIB_TABLE").as_deref() == Ok("1")
        && !ports.iter().any(|&(address, _)| address == 0x00cf_30c0)
    {
        ports.push((0x00cf_30c0, table::build));
    }
    if std::env::var("DDRUNTIME_ZLIB_PORT").as_deref() == Ok("1")
        && !ports.iter().any(|&(address, _)| address == 0x00cf_02e0)
    {
        ports.push((0x00cf_02e0, port::inflate));
    }
    if std::env::var("DDRUNTIME_ZLIB_ADLER").as_deref() == Ok("1")
        && !ports.iter().any(|&(address, _)| address == 0x00cf_34d0)
    {
        ports.push((0x00cf_34d0, adler32));
    }
    if std::env::var("DDRUNTIME_ARCHIVE_INFLATE").is_ok_and(|value| value == "0") {
        return;
    }
    if !ports.iter().any(|&(address, _)| address == 0x00cf_2cb0) {
        let decoder: ddnative_base::x86::LiftFn =
            if std::env::var("DDRUNTIME_ZLIB_FAST").as_deref() == Ok("2") {
                inflate_fast_wide
            } else if std::env::var("DDRUNTIME_ZLIB_FAST").as_deref() == Ok("1") {
                inflate_fast_pages
            } else {
                inflate_fast
            };
        ports.push((0x00cf_2cb0, decoder));
    }
}

/// Translated from 0x00cf34d0 (DDDA.exe 2017-12-13 Steam), zlib Adler-32.
/// Includes the original stack spills, modified length argument, volatile
/// registers and defined arithmetic flags. No host allocation or stream state.
pub fn adler32(m: &mut dyn Mem, cpu: &mut Cpu) {
    use ddnative_base::x86::{Flags, LOGIC};
    let stack = cpu.esp;
    let initial = m.u32(stack.wrapping_add(4));
    let mut source = m.u32(stack.wrapping_add(8));
    m.set_u32(stack.wrapping_sub(4), cpu.ebp);
    m.set_u32(stack.wrapping_sub(8), cpu.esi);
    m.set_u32(stack.wrapping_sub(12), cpu.edi);
    let mut low = initial & 0xffff;
    let mut high = initial >> 16;
    if source == 0 {
        cpu.eax = 1;
        cpu.ecx = low;
        cpu.fl = Flags::new(LOGIC, 0, 0, 0, 32);
        cpu.esp = stack.wrapping_add(4);
        return;
    }
    m.set_u32(stack.wrapping_sub(16), cpu.ebx);
    let mut remaining = m.u32(stack.wrapping_add(12));
    // 5552 and 65521 are embedded at 0x00cf3502 / 0x00cf35d5.
    let mut buffer = [0u8; 5552];
    while remaining != 0 {
        let count = remaining.min(5552);
        remaining -= count;
        m.set_u32(stack.wrapping_add(12), remaining);
        m.read_bytes(source, &mut buffer[..count as usize]);
        for &byte in &buffer[..count as usize] {
            low = low.wrapping_add(u32::from(byte));
            high = high.wrapping_add(low);
        }
        source = source.wrapping_add(count);
        low %= 65521;
        // The final multiply/subtract used to reduce high leaves this in EDX.
        cpu.edx = (high / 65521).wrapping_mul(65521).wrapping_neg();
        high %= 65521;
    }
    cpu.ecx = low;
    cpu.eax = (high << 16) | low;
    cpu.fl = Flags::new(LOGIC, high << 16, low, cpu.eax, 32);
    cpu.esp = stack.wrapping_add(4);
}

fn mask(bits: u32) -> u32 {
    1u32.wrapping_shl(bits).wrapping_sub(1)
}

struct Bits {
    input: u32, // one byte before next input, as in the original
    hold: u32,
    count: u32,
}
impl Bits {
    fn byte<const FAST: bool, const WIDE: bool>(&mut self, m: &Access<FAST, WIDE>) {
        self.input = self.input.wrapping_add(1);
        self.hold = self
            .hold
            .wrapping_add((m.u8(self.input) as u32).wrapping_shl(self.count));
        self.count += 8;
    }
    fn pair_if_needed<const FAST: bool, const WIDE: bool>(&mut self, m: &Access<FAST, WIDE>) {
        if self.count < 15 {
            if WIDE {
                let value = m.pair(self.input.wrapping_add(1));
                self.input = self.input.wrapping_add(2);
                self.hold = self.hold.wrapping_add(value.wrapping_shl(self.count));
                self.count += 16;
            } else {
                self.byte(m);
                self.byte(m);
            }
        }
    }
    fn take(&mut self, count: u32) -> u32 {
        let value = self.hold & mask(count);
        self.hold = self.hold.wrapping_shr(count);
        self.count = self.count.wrapping_sub(count);
        value
    }
    fn code<const FAST: bool, const WIDE: bool>(
        &mut self,
        m: &Access<FAST, WIDE>,
        table: &DecodeTable,
        index: u32,
    ) -> u32 {
        let address = table.base.wrapping_add(index.wrapping_mul(4));
        let code = if WIDE {
            table.read(m, index)
        } else {
            m.u32(address)
        };
        self.take((code >> 8) & 255);
        code
    }
}

// The root tables remain immutable throughout this callee-free decoder.
// Retain the complete root span when it fits one page. Subtables and page
// crossings retain checked accesses; no persistent table or stream cache.
struct DecodeTable {
    base: u32,
    entries: u32,
    pointer: Option<*mut u8>,
}
impl DecodeTable {
    fn new<const FAST: bool, const WIDE: bool>(m: &Access<FAST, WIDE>, base: u32, entries: u32) -> Self {
        Self {
            base,
            entries,
            pointer: if WIDE {
                m.read_span(base, entries.saturating_mul(4))
            } else {
                None
            },
        }
    }
    #[inline]
    fn read<const FAST: bool, const WIDE: bool>(&self, m: &Access<FAST, WIDE>, index: u32) -> u32 {
        if index < self.entries {
            if let Some(pointer) = self.pointer {
                // SAFETY: the index stays in the checked root-table span.
                return unsafe { ddnative_base::mem::load_at(pointer.add(index as usize * 4), 4) };
            }
        }
        m.u32(self.base.wrapping_add(index.wrapping_mul(4)))
    }
}

/// Translated from 0x00cf2cb0 (DDDA.exe 2017-12-13 Steam), zlib inflate_fast.
/// cdecl (z_stream*, starting avail_out). Preconditions are enforced by the
/// original caller at 0x00cf02e0: >=6 input bytes, >=258 output bytes, valid
/// decode tables and a non-aliasing stream/state/window/stack. Preserves the
/// original's argument spill, stack locals and final general registers.
pub fn inflate_fast(m: &mut dyn Mem, cpu: &mut Cpu) {
    inflate_decoder(&mut Access::<false>::new(m), cpu);
}

/// Same original decoder and ABI, with page-wise forward copies and local
/// checked scalar accesses. Opt-in DDRUNTIME_ZLIB_FAST=1 (head L145).
pub fn inflate_fast_pages(m: &mut dyn Mem, cpu: &mut Cpu) {
    inflate_decoder(&mut Access::<true>::new(m), cpu);
}

/// Exact cf2cb0 return contract with packed match copies. Opt-in FAST=2.
/// Translated from 0x00cf2cb0 (DDDA.exe 2017-12-13 Steam).
pub fn inflate_fast_wide(m: &mut dyn Mem, cpu: &mut Cpu) {
    inflate_decoder(&mut Access::<true, true>::new(m), cpu);
}

fn inflate_decoder<const FAST: bool, const WIDE: bool>(m: &mut Access<FAST, WIDE>, cpu: &mut Cpu) {
    let frame = cpu.esp.wrapping_sub(4);
    let stream = m.u32(cpu.esp.wrapping_add(4));
    let start = m.u32(cpu.esp.wrapping_add(8));
    m.set_u32(frame, cpu.ebp);
    m.set_u32(frame.wrapping_sub(0x40), cpu.ebx);
    m.set_u32(frame.wrapping_sub(0x44), cpu.esi);
    m.set_u32(frame.wrapping_sub(0x48), cpu.edi);
    let state = m.u32(stream + 0x1c);
    let mut bits = Bits {
        input: m.u32(stream).wrapping_sub(1),
        hold: m.u32(state + 0x30),
        count: m.u32(state + 0x34),
    };
    let input_end = bits.input.wrapping_add(m.u32(stream + 4)).wrapping_sub(5);
    let mut output = m.u32(stream + 0xc).wrapping_sub(1);
    let available = m.u32(stream + 0x10);
    let beginning = output.wrapping_add(available.wrapping_sub(start));
    let output_end = output.wrapping_add(available).wrapping_sub(257);
    let window_size_20 = m.u32(state + 0x20);
    let window_have_24 = m.u32(state + 0x24);
    let window_write_28 = m.u32(state + 0x28);
    let window_2c = m.u32(state + 0x2c);
    let literals_44 = m.u32(state + 0x44);
    let distances_48 = m.u32(state + 0x48);
    let literal_mask = mask(m.u32(state + 0x4c));
    let distance_mask = mask(m.u32(state + 0x50));
    let literal_table = DecodeTable::new(m, literals_44, literal_mask.wrapping_add(1));
    let distance_table = DecodeTable::new(m, distances_48, distance_mask.wrapping_add(1));
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
    let mut length = 1;
    'decode: loop {
        bits.pair_if_needed(m);
        let mut code = bits.code(m, &literal_table, bits.hold & literal_mask);
        loop {
            let operation = code & 255;
            if operation == 0 {
                output = output.wrapping_add(1);
                m.set_u8(output, (code >> 16) as u8);
                break;
            }
            if operation & 16 != 0 {
                length = code >> 16;
                let extra = operation & 15;
                if extra != 0 {
                    if bits.count < extra {
                        bits.byte(m);
                    }
                    length += bits.take(extra);
                }
                bits.pair_if_needed(m);
                code = bits.code(m, &distance_table, bits.hold & distance_mask);
                if WIDE {
                    last_auxiliary = Some(code);
                } else {
                    m.set_u32(frame.wrapping_sub(0x18), code);
                }
                while code & 16 == 0 {
                    if code & 64 != 0 {
                        // Error text remains in the player's mapped executable.
                        m.set_u32(stream + 0x18, 0x0140_3e78); // 0x00cf3039
                        m.set_u32(state, 27);
                        break 'decode;
                    }
                    let index = (code >> 16).wrapping_add(bits.hold & mask(code & 255));
                    code = bits.code(m, &distance_table, index);
                    if WIDE {
                        last_auxiliary = Some(code);
                    } else {
                        m.set_u32(frame.wrapping_sub(0x18), code);
                    }
                }
                let extra = code & 15;
                let base = code >> 16;
                if bits.count < extra {
                    bits.byte(m);
                    if bits.count < extra {
                        bits.byte(m);
                    }
                }
                let low = bits.take(extra);
                let distance = base.wrapping_add(low);
                if WIDE {
                    last_auxiliary = Some(low);
                } else {
                    m.set_u32(frame.wrapping_sub(0x18), low);
                }
                if WIDE {
                    last_distance = Some(distance);
                } else {
                    m.set_u32(frame.wrapping_sub(8), distance);
                }
                let produced = output.wrapping_sub(beginning);
                let mut source;
                if distance > produced {
                    let back = distance - produced;
                    if WIDE {
                        last_auxiliary = Some(back);
                    } else {
                        m.set_u32(frame.wrapping_sub(0x18), back);
                    }
                    if back > window_have_24 {
                        m.set_u32(stream + 0x18, 0x0140_3e58); // 0x00cf3027
                        m.set_u32(state, 27);
                        break 'decode;
                    }
                    source = window_2c.wrapping_sub(1);
                    if window_write_28 == 0 {
                        source = source.wrapping_add(window_size_20.wrapping_sub(back));
                        if back < length {
                            length -= back;
                            m.copy(&mut output, &mut source, back);
                            source = output.wrapping_sub(distance);
                        }
                    } else if window_write_28 < back {
                        source = source
                            .wrapping_add(window_write_28.wrapping_sub(back))
                            .wrapping_add(window_size_20);
                        let tail = back - window_write_28;
                        if tail < length {
                            length -= tail;
                            m.copy(&mut output, &mut source, tail);
                            source = window_2c.wrapping_sub(1);
                            if window_write_28 < length {
                                length -= window_write_28;
                                if WIDE {
                                    last_auxiliary = Some(window_write_28);
                                } else {
                                    m.set_u32(frame.wrapping_sub(0x18), window_write_28);
                                }
                                m.copy(&mut output, &mut source, window_write_28);
                                source = output.wrapping_sub(distance);
                            }
                        }
                    } else {
                        source = source.wrapping_add(window_write_28 - back);
                        if back < length {
                            length -= back;
                            m.copy(&mut output, &mut source, back);
                            source = output.wrapping_sub(distance);
                        }
                    }
                } else {
                    source = output.wrapping_sub(distance);
                }
                // The original unrolls the remaining copy in triples and
                // leaves only its remainder in the caller's second argument.
                if WIDE {
                    m.copy(&mut output, &mut source, length);
                    length %= 3;
                } else {
                    let triples = length / 3 * 3;
                    m.copy(&mut output, &mut source, triples);
                    length -= triples;
                    m.copy(&mut output, &mut source, length);
                }
                break;
            }
            if operation & 64 != 0 {
                if operation & 32 != 0 {
                    m.set_u32(state, 11);
                } else {
                    m.set_u32(stream + 0x18, 0x0140_3e90); // 0x00cf3055
                    m.set_u32(state, 27);
                }
                break 'decode;
            }
            let index = (code >> 16).wrapping_add(bits.hold & mask(operation));
            code = bits.code(m, &literal_table, index);
        }
        if bits.input >= input_end || output >= output_end {
            break;
        }
    }
    // cf2cb0 never reads these two spills again. Commit their final values
    // at the original return, including invalid-distance exits; untouched
    // slots stay untouched when no distance was decoded.
    if WIDE {
        if let Some(value) = last_auxiliary {
            m.set_u32(frame.wrapping_sub(0x18), value);
        }
        if let Some(value) = last_distance {
            m.set_u32(frame.wrapping_sub(8), value);
        }
    }
    bits.input = bits.input.wrapping_sub(bits.count >> 3);
    bits.count &= 7;
    bits.hold &= mask(bits.count);
    let input_left = input_end.wrapping_sub(bits.input).wrapping_add(5);
    m.set_u32(frame.wrapping_sub(4), bits.input);
    m.set_u32(frame + 12, length);
    m.set_u32(stream, bits.input.wrapping_add(1));
    m.set_u32(stream + 0xc, output.wrapping_add(1));
    m.set_u32(stream + 0x10, output_end.wrapping_sub(output).wrapping_add(257));
    m.set_u32(stream + 4, input_left);
    m.set_u32(state + 0x34, bits.count);
    m.set_u32(state + 0x30, bits.hold);
    cpu.eax = state;
    cpu.ecx = stream;
    cpu.edx = input_left;
    let before_add = output_end.wrapping_sub(output);
    cpu.fl = ddnative_base::x86::Flags::new(
        ddnative_base::x86::ADD,
        before_add,
        257,
        before_add.wrapping_add(257),
        32,
    );
    cpu.esp = cpu.esp.wrapping_add(4);
}
