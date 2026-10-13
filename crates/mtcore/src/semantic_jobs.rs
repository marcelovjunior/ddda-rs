//! Semantic job-thread routines, with original guest storage and callback order.
use ddnative_base::{
    mem::{Mem, Pages},
    x86::{self, Cpu},
};

// Translated from 0x010A35D0 (DDDA.exe 2017-12-13 Steam).
/// Traverse four-way bounds nodes (+0x58), using the selected worker's
/// nesting-indexed guest work stack. Each 0x70-byte node stores child/leaf
/// flags at +0, four u16 indices at +4, and six four-lane bounds at +0x10.
/// Leaf callbacks receive (context, index, 0); their results are ORed.
pub fn query_bounds_tree(m: &mut dyn Mem, c: &mut Cpu) {
    let p = Pages::of(m);
    let entry = c.esp;
    let owner = c.ecx;
    let saved = [c.ebx, c.ebp, c.esi, c.edi];
    let frame = (entry.wrapping_sub(4) & !15).wrapping_sub(0x360);
    let query = p.u32(m, entry.wrapping_add(4));
    let receiver = p.u32(m, entry.wrapping_add(8));
    let callback = p.u32(m, entry.wrapping_add(12));
    let context = p.u32(m, entry.wrapping_add(16));
    let lane = p.u32(m, entry.wrapping_add(20));
    if lane >= 25 {
        let root = p.u32(m, owner.wrapping_add(0x58));
        c.esp = frame;
        for value in [context, callback, receiver, query, root, 0x010a3604] {
            c.esp = c.esp.wrapping_sub(4);
            p.set_u32(m, c.esp, value);
        }
        x86::call_ext(m, c, 0x010a2460);
    } else {
        let counter = 0x018d5ee8u32.wrapping_add(lane * 4);
        let depth = p.u32(m, counter);
        let slot = depth.wrapping_mul(25).wrapping_add(lane);
        let stack = p.u32(m, 0x018d5e20u32.wrapping_add(slot.wrapping_mul(4)));
        p.set_u32(m, counter, depth.wrapping_add(1));
        let root = p.u32(m, owner.wrapping_add(0x58));
        bounds_record(m, p, stack, root, query);
        let mut count = 1u32;
        let mut result = 0;
        while count != 0 {
            count = count.wrapping_sub(1);
            let record = stack.wrapping_add(count.wrapping_mul(32));
            // All four lanes are snapshotted before callbacks or child pushes.
            let hit: [u32; 4] = std::array::from_fn(|i| p.u32(m, record.wrapping_add(i as u32 * 4)));
            let children = p.u32(m, record.wrapping_add(16));
            let leaves = p.u32(m, record.wrapping_add(20));
            let node = p.u32(m, record.wrapping_add(24));
            for lane in 0..4u32 {
                if hit[lane as usize] == 0 {
                    continue;
                }
                let index = p.u16(m, node.wrapping_add(4 + lane * 2)) as u32;
                if (children >> (lane * 8)) as u8 != 0 {
                    let child = p.u32(m, owner.wrapping_add(0x58)).wrapping_add(index * 0x70);
                    bounds_record(m, p, stack.wrapping_add(count.wrapping_mul(32)), child, query);
                    count = count.wrapping_add(1);
                } else if (leaves >> (lane * 8)) as u8 != 0 {
                    c.esp = frame;
                    let return_pc = [0x010a3869, 0x010a398c, 0x010a3aaf, 0x010a3bd2][lane as usize];
                    for value in [0, index, context, return_pc] {
                        c.esp = c.esp.wrapping_sub(4);
                        p.set_u32(m, c.esp, value);
                    }
                    c.ecx = receiver;
                    x86::call_ext(m, c, callback);
                    result |= c.eax;
                }
            }
        }
        let depth = p.u32(m, counter).wrapping_sub(1);
        p.set_u32(m, counter, depth);
        c.eax = result;
    }
    [c.ebx, c.ebp, c.esi, c.edi] = saved;
    c.esp = entry.wrapping_add(24);
}

/// Emit the original observable work record, including both flag words.
fn bounds_record(m: &mut dyn Mem, p: Pages, out: u32, node: u32, query: u32) {
    let read = |base: u32, offset: u32| -> [f32; 4] {
        std::array::from_fn(|i| f32::from_bits(p.u32(m, base.wrapping_add(offset + i as u32 * 4))))
    };
    let qmin = [read(query, 16), read(query, 32), read(query, 48)];
    let nmax = [read(node, 64), read(node, 80), read(node, 96)];
    let nmin = [read(node, 16), read(node, 32), read(node, 48)];
    let qmax = [read(query, 64), read(query, 80), read(query, 96)];
    // SSE comparisons retain unordered/DAZ semantics of CMPLEPS, including
    // the current guest MXCSR. No arithmetic or rounded host constants.
    let masks = unsafe {
        use std::arch::x86_64::*;
        let mut mask = _mm_castsi128_ps(_mm_set1_epi32(-1));
        for axis in 0..3 {
            mask = _mm_and_ps(
                mask,
                _mm_cmple_ps(
                    _mm_loadu_ps(qmin[axis].as_ptr()),
                    _mm_loadu_ps(nmax[axis].as_ptr()),
                ),
            );
            mask = _mm_and_ps(
                mask,
                _mm_cmple_ps(
                    _mm_loadu_ps(nmin[axis].as_ptr()),
                    _mm_loadu_ps(qmax[axis].as_ptr()),
                ),
            );
        }
        let mut words = [0u32; 4];
        _mm_storeu_si128(words.as_mut_ptr().cast(), _mm_castps_si128(mask));
        words
    };
    for (i, value) in masks.into_iter().enumerate() {
        p.set_u32(m, out.wrapping_add(i as u32 * 4), value);
    }
    p.set_u32(m, out.wrapping_add(24), node);
    let leaves = p.u32(m, node) & 0x80402010;
    p.set_u32(m, out.wrapping_add(20), leaves);
    let children = p.u32(m, node) & 0x08040201;
    p.set_u32(m, out.wrapping_add(16), children);
}
