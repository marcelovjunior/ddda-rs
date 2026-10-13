//! L184 (s4): semantic port of 0x01054860, uModel::vf28, the model refresh
//! that the player pass (0x00B66B30) and other update paths call.
use crate::semantic::call_from;
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::Cpu;

// Translated from 0x01054860 (DDDA.exe 2017-12-13 Steam).
/// 0x01054860, `model->refresh(flags)` (thiscall, `ret 4`, void; uModel
/// vf28): run the model's own vf21 update (0x00EA4110), then, unless
/// `flags & 4`, update all its joints (0x01054700 over the index list at
/// +0x36C and the count at +0x360 with `flags`), mark the skin pending
/// (+0xD80 = 2) when the model is not in the 0x4800 rendering state (+4),
/// raise bit 0 of +0x370 if +0xF0 is set, and, unless `flags & 1`, finish
/// with 0x00FFA620(0).
pub fn model_refresh(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let model = c.ecx;
    let entry = c.esp;
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    // the original's frame after `push ebx; push esi`: its calls are made here
    let frame = entry.wrapping_sub(8);
    let guest = |target: u32| move |m: &mut dyn Mem, c: &mut Cpu| crate::semantic::call_guest(m, c, target);
    c.esi = model;
    call_from(m, c, pg, frame, model, &[], 0x0105_4869, guest(0x00ea_4110));
    // the original reads the flags argument after that call
    let flags = pg.u32(m, entry.wrapping_add(4));
    c.ebx = flags;
    if flags & 4 == 0 {
        let count = pg.u32(m, model.wrapping_add(0x360));
        let indices = pg.u32(m, model.wrapping_add(0x36c));
        call_from(m, c, pg, frame, model, &[indices, count, flags], 0x0105_4888, guest(0x0105_4700));
        let state = pg.u32(m, model.wrapping_add(4));
        if state & 7 != 2 || state & 0x4800 != 0x4800 {
            pg.set_u32(m, model.wrapping_add(0xd80), 2);
        }
        if pg.u32(m, model.wrapping_add(0xf0)) != 0 {
            let v = pg.u32(m, model.wrapping_add(0x370)) | 1;
            pg.set_u32(m, model.wrapping_add(0x370), v);
        }
        if flags & 1 == 0 {
            call_from(m, c, pg, frame, model, &[0], 0x0105_48c9, guest(0x00ff_a620));
        }
    }
    (c.ebx, c.ebp, c.esi, c.edi) = saved;
    c.esp = entry.wrapping_add(8);
}
