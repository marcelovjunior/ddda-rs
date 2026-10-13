//! L184 semantic ports s10: native rewrites (ABI level, see `semantic.rs`)
//! of three functions on the shadow job's critical path. Object offsets and
//! control flow come from the 2017-12-13 Steam executable; the heavy
//! callees stay guest calls.
use crate::semantic::call_from;
use ddnative_base::{
    mem::{Mem, Pages},
    x86::{self, Cpu},
};

/// Runs the guest function at `target` (the lifted body behind the runtime's
/// call handler).
fn guest(target: u32) -> impl FnOnce(&mut dyn Mem, &mut Cpu) {
    move |m, c| x86::call_ext(m, c, target)
}

/// The descriptor a trivial `mov eax, imm32; ret` getter returns, read
/// straight from its code, or `None` for any other method.
#[inline]
fn constant_getter(m: &dyn Mem, pg: Pages, method: u32) -> Option<u32> {
    if pg.u8(m, method) == 0xb8 && pg.u8(m, method.wrapping_add(5)) == 0xc3 {
        Some(pg.u32(m, method.wrapping_add(1)))
    } else {
        None
    }
}

/// Where a class-descriptor chain (`dti`, each link's parent at +0x10, its
/// id at +4) contains `id`.
#[inline]
fn chain_has(m: &dyn Mem, pg: Pages, mut dti: u32, id: u32) -> bool {
    while dti != 0 {
        if pg.u32(m, dti.wrapping_add(4)) == id {
            return true;
        }
        dti = pg.u32(m, dti.wrapping_add(0x10));
    }
    false
}

/// Class-id globals tested by [`apply_param_block`].
const CLASS_SKIPPED: u32 = 0x019a_6cc0;
const CLASS_TARGETED: u32 = 0x019a_6ce0;

// Translated from 0x00c85a00 (DDDA.exe 2017-12-13 Steam).
/// 0x00C85A00, `apply_param_block(block, target, extra)` (stdcall, `ret
/// 0xc`; void): apply a parameter block to a scene object. Objects of the
/// class at 0x019A6CC0 (or derived from it) are left alone; objects not
/// derived from the class at 0x019A6CE0 are left alone too. For the
/// others, the block's three base helpers run (0x00C86390 (EAX target, ECX
/// block), 0x00C86420 (EAX extra, ECX block, stack target), 0x00C864D0 (EAX
/// target, ECX block)), then one helper per section selected by the block's
/// flag word (+4): bit 3 with mode +0x18 = 1 / 2 (0x00C879D0 / 0x00C87A70),
/// bit 4 with mode +0x1c = 1 (0x00C87B70), bit 5 with mode +0x20 = 0 / 1 /
/// 2 (0x00C87C30 / 0x00C87CA0). The block's fields are read again after
/// every helper, as the original, since a helper may change them.
///
/// The object's class descriptor comes from its virtual method +0x10,
/// called twice by the original; a plain `mov eax, imm; ret` method is
/// read from its code instead (it has no effects), any other is called.
pub fn apply_param_block(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let block = pg.u32(m, entry.wrapping_add(4));
    let target = pg.u32(m, entry.wrapping_add(8));
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    // The original's frame: EBP = entry - 4, ESP aligned to 8, then four pushes.
    let ebp = entry.wrapping_sub(4);
    let frame = (ebp & !7).wrapping_sub(16);
    let entry_edi = c.edi;
    (c.ebx, c.ebp, c.esi, c.edi) = (block, ebp, target, entry_edi);

    // The class descriptor of `target`: one virtual call (resume 0x00C85A19
    // for the first, 0x00C85A49 for the second).
    let vtable = pg.u32(m, target);
    let method = pg.u32(m, vtable.wrapping_add(0x10));
    let constant = constant_getter(m, pg, method);
    let descriptor = |m: &mut dyn Mem, c: &mut Cpu, resume: u32| -> u32 {
        if let Some(dti) = constant {
            return dti;
        }
        let vtable = pg.u32(m, target);
        let method = pg.u32(m, vtable.wrapping_add(0x10));
        c.eax = vtable;
        c.edx = method;
        call_from(m, c, pg, frame, target, &[], resume, guest(method))
    };
    let dti = descriptor(m, c, 0x00c8_5a19);
    let skipped = pg.u32(m, CLASS_SKIPPED);
    let leave = |c: &mut Cpu| {
        (c.ebx, c.ebp, c.esi, c.edi) = saved;
        c.esp = entry.wrapping_add(4 + 0xc);
    };
    if chain_has(m, pg, dti, skipped) {
        return leave(c);
    }
    let dti = descriptor(m, c, 0x00c8_5a49);
    let targeted = pg.u32(m, CLASS_TARGETED);
    if !chain_has(m, pg, dti, targeted) {
        return leave(c);
    }

    // helper(target in EAX, block in ECX), ESI = target
    let run = |m: &mut dyn Mem, c: &mut Cpu, helper: u32, resume: u32, eax: u32, ecx: u32, args: &[u32], esi: u32, edi: u32| {
        (c.ebx, c.ebp, c.esi, c.edi) = (block, ebp, esi, edi);
        c.eax = eax;
        call_from(m, c, pg, frame, ecx, args, resume, guest(helper));
    };
    let mut edi = entry_edi;
    run(m, c, 0x00c8_6390, 0x00c8_5a7e, target, block, &[], target, edi);
    let extra = pg.u32(m, entry.wrapping_add(0xc));
    run(m, c, 0x00c8_6420, 0x00c8_5a89, extra, block, &[target], target, edi);
    run(m, c, 0x00c8_64d0, 0x00c8_5a92, target, block, &[], target, edi);
    if pg.u8(m, block.wrapping_add(4)) & 8 != 0 {
        match pg.u32(m, block.wrapping_add(0x18)) {
            1 => run(m, c, 0x00c8_79d0, 0x00c8_5abc, block, target, &[], target, edi),
            2 => {
                // ESI = block, EDI = target for this helper; EDI stays target
                edi = target;
                run(m, c, 0x00c8_7a70, 0x00c8_5aaf, 0, 0, &[], block, target);
            }
            _ => {}
        }
    }
    if pg.u8(m, block.wrapping_add(4)) & 0x10 != 0 && pg.u32(m, block.wrapping_add(0x1c)) == 1 {
        edi = block;
        run(m, c, 0x00c8_7b70, 0x00c8_5ad4, 0, 0, &[], target, block);
    }
    if pg.u8(m, block.wrapping_add(4)) & 0x20 != 0 {
        match pg.u32(m, block.wrapping_add(0x20)) {
            0 => run(m, c, 0x00c8_7c30, 0x00c8_5b01, 0, 0, &[], target, edi),
            1 | 2 => run(m, c, 0x00c8_7ca0, 0x00c8_5af3, target, block, &[], target, edi),
            _ => {}
        }
    }
    leave(c);
}
