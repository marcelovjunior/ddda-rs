//! Buffered stream skip. Only bytes already inside the current buffer batch;
//! refill attempts (including repeated failures) retain the original order.
use ddnative_base::{emit, mem::Mem, x86::{self, Cpu}};

/// Translated from 0x00cfeec0 (DDDA.exe 2017-12-13 Steam).
/// `this+0c` is the cursor, `this+10` the buffer end; one stack argument is
/// the requested skip count. Preserve volatile registers, flags and spills.
pub fn skip(m: &mut dyn Mem, cpu: &mut Cpu) {
    skip_with(m, cpu, &mut |m, cpu| x86::call_ext(m, cpu, 0x00cfe9d0));
}

#[cfg(test)]
mod tests {
    use super::*;
    use ddnative_base::mem::Sparse;

    #[test]
    fn full_u32_span_is_one_batch_without_refill_or_wrap() {
        let mut m = Sparse::default();
        let mut c = Cpu { ecx: 0x1000, esp: 0x9000, ..Cpu::default() };
        m.set_u32(0x9004, u32::MAX);
        m.set_u32(0x100c, 0);
        m.set_u32(0x1010, u32::MAX);
        skip_with(&mut m, &mut c, &mut |_, _| panic!("unexpected refill"));
        assert_eq!(m.u32(0x100c), u32::MAX);
        assert_eq!(c.eax, u32::MAX - 1);
        assert_eq!(c.esp, 0x9008);
        assert!(c.fl.zf() && c.fl.cf());
    }

    #[test]
    fn original_instructions_private_vectors() {
        let path = std::env::var("L163_SKIP_VECTORS")
            .unwrap_or_else(|_| "E:/ddda-re/notes/native-L163/skip.bin".into());
        let data = match std::fs::read(&path) {
            Ok(data) => data,
            Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
            Err(_) => { eprintln!("private skip vectors absent: {path}"); return; }
        };
        assert_eq!(&data[..8], b"L163SK01");
        let mut words = data[8..].chunks_exact(4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
        let mut next = || words.next().unwrap();
        let cases = next();
        assert_eq!(cases, 192);
        fn regs(c: &Cpu) -> [u32; 9] {
            [c.eax, c.ecx, c.edx, c.ebx, c.esp, c.ebp, c.esi, c.edi, c.fl.snapshot().0]
        }
        fn compare(actual: [u32; 9], expected: [u32; 9], mask: u32) {
            assert_eq!(&actual[..8], &expected[..8]);
            assert_eq!(actual[8] & mask, expected[8] & mask);
        }
        for _ in 0..cases {
            let [cursor, end, count, mode] = std::array::from_fn(|_| next());
            let expected = std::array::from_fn(|_| next());
            let object: [u32; 2] = std::array::from_fn(|_| next());
            let stack: [u32; 5] = std::array::from_fn(|_| next());
            let calls: Vec<[u32; 9]> = (0..next()).map(|_| std::array::from_fn(|_| next())).collect();
            let mut m = Sparse::default();
            let mut c = Cpu { eax: 0x1234, ecx: 0x1000, edx: 0x5678,
                ebx: 0x9876, esp: 0x9000, ebp: 0x7654, esi: 0x3210,
                edi: 0xabc, ..Cpu::default() };
            for address in (0x8ff4..0x9008).step_by(4) { m.set_u32(address, 0); }
            m.set_u32(0x9000, 0x9100);
            m.set_u32(0x9004, count);
            m.set_u32(0x100c, cursor);
            m.set_u32(0x1010, end);
            let mut index = 0;
            skip_with(&mut m, &mut c, &mut |m, c| {
                compare(regs(c), calls[index], 0x8d5);
                index += 1;
                assert_eq!(m.u32(c.esp), 0xcfeedf);
                let success = mode == 0 || mode == 2 && index % 2 == 0 || mode == 3;
                c.eax = 0x1234_0000 | u32::from(success);
                c.edx = index as u32;
                if success {
                    m.set_u32(0x100c, if mode == 3 { u32::MAX } else { 0 });
                    m.set_u32(0x1010, 11);
                }
                c.esp += 4;
            });
            assert_eq!(index, calls.len());
            compare(regs(&c), expected, if count == 0 { 0x8c5 } else { 0x8d5 });
            assert_eq!([m.u32(0x100c), m.u32(0x1010)], object);
            assert_eq!(std::array::from_fn::<_, 5, _>(|i| m.u32(0x8ff4 + i as u32 * 4)), stack);
        }
        assert!(words.next().is_none());
    }

    // Scalar instruction reference translated from 00CFEEC0..00CFEEED
    // (DDDA.exe 2017-12-13 Steam). Deliberately retains every iteration.
    fn scalar(m: &mut dyn Mem, c: &mut Cpu, refill: &mut impl FnMut(&mut dyn Mem, &mut Cpu)) {
        let pt = m.page_table();
        c.eax = m.u32(c.esp.wrapping_add(4));
        emit::push(c.esi, m, pt, c);
        c.esi = c.ecx;
        c.fl.alu_and(c.eax, c.eax, 32);
        if c.eax != 0 {
            emit::push(c.edi, m, pt, c);
            c.edi = c.eax;
            loop {
                c.eax = m.u32(c.esi.wrapping_add(12));
                c.fl.alu_sub(c.eax, m.u32(c.esi.wrapping_add(16)), 32);
                let mut advance = c.fl.a < c.fl.b;
                if !advance {
                    c.ecx = c.esi;
                    emit::push(0x00cfeedf, m, pt, c);
                    refill(m, c);
                    c.fl.alu_and(c.eax & 255, c.eax & 255, 8);
                    advance = c.eax & 255 != 0;
                }
                if advance {
                    let a = m.u32(c.esi.wrapping_add(12));
                    let r = a.wrapping_add(1);
                    c.fl.inc_dec(x86::INC, a, r, 32);
                    m.set_u32(c.esi.wrapping_add(12), r);
                }
                let a = c.edi;
                c.edi = a.wrapping_sub(1);
                c.fl.inc_dec(x86::DEC, a, c.edi, 32);
                if c.fl.zf() { break; }
            }
            c.edi = emit::pop(m, pt, c);
        }
        c.esi = emit::pop(m, pt, c);
        c.esp = c.esp.wrapping_add(8);
    }

    #[test]
    fn batched_skip_matches_scalar_registers_flags_spills_and_refill_boundaries() {
        let mut cases = 0;
        for (cursor, end) in [(0, 0), (0, 1), (3, 9), (10, 9),
            (0xffff_fffa, u32::MAX), (u32::MAX, u32::MAX)] {
            for count in [0, 1, 2, 5, 6, 7, 16, 257] {
                for mode in 0..4 {
                    let run = |batch: bool| {
                        let mut m = Sparse::default();
                        let mut c = Cpu { eax: 0x1234, ecx: 0x1000, edx: 0x5678,
                            ebx: 0x9876, esp: 0x9000, ebp: 0x7654, esi: 0x3210,
                            edi: 0xabc, ..Cpu::default() };
                        m.set_u32(0x9004, count);
                        m.set_u32(0x100c, cursor);
                        m.set_u32(0x1010, end);
                        let mut calls = Vec::new();
                        let mut refill = |m: &mut dyn Mem, c: &mut Cpu| {
                            calls.push(format!("{c:?}"));
                            assert_eq!(m.u32(c.esp), 0x00cfeedf);
                            // Synthetic success, failure, interleaving and
                            // cursor wrap. High EAX bits are observable too.
                            let success = mode == 0 || mode == 2 && calls.len() % 2 == 0 || mode == 3;
                            c.eax = 0x1234_0000 | u32::from(success);
                            c.edx = calls.len() as u32;
                            if success {
                                m.set_u32(0x100c, if mode == 3 { u32::MAX } else { 0 });
                                m.set_u32(0x1010, 11);
                            }
                            c.esp += 4;
                        };
                        if batch { skip_with(&mut m, &mut c, &mut refill); }
                        else { scalar(&mut m, &mut c, &mut refill); }
                        (format!("{c:?}"), m.written, calls)
                    };
                    assert_eq!(run(true), run(false), "cursor={cursor:x} end={end:x} count={count} mode={mode}");
                    cases += 1;
                }
            }
        }
        assert_eq!(cases, 192);
    }
}

fn skip_with(m: &mut dyn Mem, cpu: &mut Cpu, refill: &mut impl FnMut(&mut dyn Mem, &mut Cpu)) {
    let pt = m.page_table();
    cpu.eax = m.fast_u32(pt, cpu.esp.wrapping_add(4));
    emit::push(cpu.esi, m, pt, cpu);
    cpu.esi = cpu.ecx;
    cpu.fl.alu_and(cpu.eax, cpu.eax, 32);
    if cpu.eax != 0 {
        emit::push(cpu.edi, m, pt, cpu);
        cpu.edi = cpu.eax;
        loop {
            cpu.eax = m.fast_u32(pt, cpu.esi.wrapping_add(0xc));
            let end = m.fast_u32(pt, cpu.esi.wrapping_add(0x10));
            cpu.fl.alu_sub(cpu.eax, end, 32);
            if cpu.eax < end {
                let count = cpu.edi.min(end - cpu.eax);
                // Reconstruct the final CMP/INC/DEC of this span. EAX holds
                // its pre-increment cursor, and INC/DEC preserve CMP's CF.
                cpu.eax += count - 1;
                cpu.fl.alu_sub(cpu.eax, end, 32);
                let next = cpu.eax + 1;
                cpu.fl.inc_dec(x86::INC, cpu.eax, next, 32);
                m.fast_set_u32(pt, cpu.esi.wrapping_add(0xc), next);
                let remaining = cpu.edi - count;
                cpu.fl.inc_dec(x86::DEC, remaining + 1, remaining, 32);
                cpu.edi = remaining;
            } else {
                cpu.ecx = cpu.esi;
                emit::push(0x00cfeedf, m, pt, cpu);
                refill(m, cpu);
                cpu.fl.alu_and(cpu.eax & 0xff, cpu.eax & 0xff, 8);
                if cpu.eax & 0xff != 0 {
                    let cursor = m.fast_u32(pt, cpu.esi.wrapping_add(0xc));
                    let next = cursor.wrapping_add(1);
                    cpu.fl.inc_dec(x86::INC, cursor, next, 32);
                    m.fast_set_u32(pt, cpu.esi.wrapping_add(0xc), next);
                }
                let remaining = cpu.edi.wrapping_sub(1);
                cpu.fl.inc_dec(x86::DEC, cpu.edi, remaining, 32);
                cpu.edi = remaining;
            }
            if cpu.edi == 0 { break; }
        }
        cpu.edi = emit::pop(m, pt, cpu);
    }
    cpu.esi = emit::pop(m, pt, cpu);
    cpu.esp = cpu.esp.wrapping_add(8);
}
