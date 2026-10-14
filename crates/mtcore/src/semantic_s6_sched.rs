//! L186 s6: semantic ports of the job scheduler (sMain's job queue):
//!
//! - [`run_jobs`], 0x00DBD8D0: the loop with which the calling thread (and
//!   any thread that calls it) takes jobs from the batch until it is empty;
//! - [`worker_thread`], 0x00DBDC90: a worker's thread procedure: signal that
//!   it is up, then wait for its wake-up event, take jobs the same way and
//!   signal that it is done, until told to stop;
//! - [`schedule`], 0x00DBE560: `scheduler->run(mode)`, the fork-join of a
//!   batch: wake the participating workers, take jobs on the calling thread
//!   too, wait for the workers.
//!
//! The scheduler object is the global at 0x018D0B28. Its batch: `+0x8c` the
//! number of jobs, `+0x90` the index of the last job taken (advanced with
//! InterlockedIncrement), `+0x94` the batch mode (0..=3), `+0xc0` the job
//! array: 8-byte entries (object, function) in modes 0 and 1, 12-byte
//! entries (object, function, extra word) in modes 2 and 3. A job is called
//! thiscall with the object, and with the worker's index pushed in modes 1
//! and 3 and the extra word in modes 2 and 3 (both: index below extra). If
//! `+0x1c14c` holds a function, it is called first, thiscall on `+0x1c148`,
//! with (mode, object, index or 0, extra word or 0) pushed: the profiler's
//! job hook.
//!
//! What the original asks the operating system for stays a guest call at
//! the same place, with the same arguments and return address
//! (SetEvent 0x0139D174, ResetEvent 0x0139D094, WaitForSingleObject
//! 0x0139D1A4, WaitForMultipleObjects 0x0139D240 are imports read at the
//! call), as do the job functions, the hook, the thread creation 0x00DBDF30
//! and the nested `schedule`'s `run_jobs` call. InterlockedIncrement
//! (0x0139D0D0) is done in place on the guest word, as the runtime's
//! service does (an atomic add where the word is aligned in a backed page),
//! and so are the two flag bytes 0x00DC5C20 sets. Those saved the lifted
//! code its dispatch, thread-local lookups and a stack frame per job.
//!
//! Translated from 0x00DBD8D0, 0x00DBDC90 and 0x00DBE560 (DDDA.exe
//! 2017-12-13 Steam).
use ddnative_base::mem::{Mem, Pages};
use ddnative_base::x86::{call_ext, Cpu};

use crate::semantic::call_from;

const SCHEDULER: u32 = 0x018d_0b28;
const SET_EVENT: u32 = 0x0139_d174;
const RESET_EVENT: u32 = 0x0139_d094;
const WAIT_FOR_SINGLE_OBJECT: u32 = 0x0139_d1a4;
const WAIT_FOR_MULTIPLE_OBJECTS: u32 = 0x0139_d240;

/// The return addresses of the job hook and job calls in each batch mode
/// (the original has one copy of the loop per mode and per function).
struct LoopSites {
    hook: [u32; 4],
    job: [u32; 4],
}

/// In 0x00DBD8D0.
const MAIN_SITES: LoopSites = LoopSites { hook: [0xdbd951, 0xdbd9c3, 0xdbda41, 0xdbdab0], job: [0xdbd958, 0xdbd9cb, 0xdbda4c, 0xdbdabc] };
/// In 0x00DBDC90.
const WORKER_SITES: LoopSites = LoopSites { hook: [0xdbdd45, 0xdbddb7, 0xdbde35, 0xdbdea1], job: [0xdbdd4c, 0xdbddbf, 0xdbde40, 0xdbdead] };

/// A guest call of a function at the address stored in an import slot.
fn call_import(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, slot: u32, args: &[u32], back: u32) -> u32 {
    let target = pg.u32(m, slot);
    let frame = c.esp;
    call_from(m, c, pg, frame, c.ecx, args, back, |m, c| call_ext(m, c, target))
}

/// The guest's InterlockedIncrement service: an atomic add on an aligned
/// word of a backed page, else a plain read-modify-write; returns the new
/// value.
fn interlocked_increment(m: &mut dyn Mem, pg: Pages, address: u32) -> u32 {
    match m.atomic_u32(address) {
        Some(word) => word.fetch_add(1, std::sync::atomic::Ordering::SeqCst).wrapping_add(1),
        None => {
            let v = pg.u32(m, address).wrapping_add(1);
            pg.set_u32(m, address, v);
            v
        }
    }
}

/// Take jobs of the current batch until the counter reaches the job count:
/// the loop shared by [`run_jobs`] and [`worker_thread`]. `index` is the
/// worker's index (passed to jobs in modes 1 and 3). The stack pointer is
/// kept as the guest left it after each call.
fn take_jobs(m: &mut dyn Mem, c: &mut Cpu, pg: Pages, scheduler: u32, mode: u32, index: u32, sites: &LoopSites) {
    if mode > 3 {
        return;
    }
    let total = pg.u32(m, scheduler.wrapping_add(0x8c));
    let base = scheduler.wrapping_add(0xc0);
    let counter = scheduler.wrapping_add(0x90);
    if (pg.u32(m, counter) as i32) >= total as i32 {
        return;
    }
    // modes 0, 1: 8-byte job entries; modes 2, 3: 12-byte entries
    let stride: u32 = if mode < 2 { 8 } else { 12 };
    let mut counter = counter;
    loop {
        let taken = interlocked_increment(m, pg, counter);
        if taken as i32 > total as i32 {
            return;
        }
        let job = base.wrapping_add(taken.wrapping_mul(stride)).wrapping_sub(stride);
        if job == 0 {
            return;
        }
        let scheduler = pg.u32(m, SCHEDULER);
        let hook = pg.u32(m, scheduler.wrapping_add(0x1c14c));
        if hook != 0 {
            // the hook sees the job's words as they are before it runs
            let object = pg.u32(m, job);
            let extra = if mode >= 2 { pg.u32(m, job.wrapping_add(8)) } else { 0 };
            let index_arg = if mode & 1 != 0 { index } else { 0 };
            let current_mode = pg.u32(m, scheduler.wrapping_add(0x94));
            let this = pg.u32(m, scheduler.wrapping_add(0x1c148));
            let frame = c.esp;
            call_from(m, c, pg, frame, this, &[current_mode, object, index_arg, extra], sites.hook[mode as usize], |m, c| call_ext(m, c, hook));
        }
        // the job's words are read after the hook, as the original does
        let object = pg.u32(m, job);
        let function = pg.u32(m, job.wrapping_add(4));
        let extra = if mode >= 2 { pg.u32(m, job.wrapping_add(8)) } else { 0 };
        let frame = c.esp;
        let args: &[u32] = match mode {
            0 => &[],
            1 => &[index],
            2 => &[extra],
            _ => &[index, extra],
        };
        call_from(m, c, pg, frame, object, args, sites.job[mode as usize], |m, c| call_ext(m, c, function));
        counter = pg.u32(m, SCHEDULER).wrapping_add(0x90);
        if (pg.u32(m, counter) as i32) >= total as i32 {
            return;
        }
    }
}

/// 0x00DBD8D0, `run_jobs(worker)` (cdecl): take jobs of the current batch
/// until none is left, as worker `worker[+0x10]`. Returns nothing.
pub fn run_jobs(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let index = pg.u32(m, pg.u32(m, entry.wrapping_add(4)).wrapping_add(0x10));
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    let scheduler = pg.u32(m, SCHEDULER);
    let mode = pg.u32(m, scheduler.wrapping_add(0x94));
    // EBX, EBP, ESI and EDI pushed
    c.esp = entry.wrapping_sub(16);
    take_jobs(m, c, pg, scheduler, mode, index, &MAIN_SITES);
    (c.ebx, c.ebp, c.esi, c.edi) = saved;
    c.esp = entry.wrapping_add(4);
}

/// 0x00DBDC90, `worker_thread(worker)` (stdcall, `ret 4`): a worker's
/// thread procedure. `worker`: `+8` its wake-up event, `+0xc` its done
/// event, `+0x10` its index, `+0x1c` the stop flag. Signals the done event,
/// then until the stop flag is set: waits for the wake-up event (forever),
/// resets it, takes the batch's jobs and signals the done event. Returns 0.
pub fn worker_thread(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    let worker = pg.u32(m, entry.wrapping_add(4));
    // PUSH ESI
    c.esp = entry.wrapping_sub(4);
    c.esi = worker;
    let done = pg.u32(m, worker.wrapping_add(0xc));
    c.ecx = done;
    call_import(m, c, pg, SET_EVENT, &[done], 0xdbdc9f);
    let finish = |c: &mut Cpu| {
        (c.ebx, c.ebp, c.esi, c.edi) = saved;
        c.eax = 0;
        c.esp = entry.wrapping_add(8);
    };
    if pg.u8(m, worker.wrapping_add(0x1c)) != 0 {
        return finish(c);
    }
    // EBX, EBP and EDI pushed
    c.esp = c.esp.wrapping_sub(12);
    loop {
        let wake = pg.u32(m, worker.wrapping_add(8));
        let waited = call_import(m, c, pg, WAIT_FOR_SINGLE_OBJECT, &[wake, u32::MAX], 0xdbdcbc);
        if waited == 0 {
            let wake = pg.u32(m, worker.wrapping_add(8));
            call_import(m, c, pg, RESET_EVENT, &[wake], 0xdbdcce);
            let scheduler = pg.u32(m, SCHEDULER);
            let mode = pg.u32(m, scheduler.wrapping_add(0x94));
            let index = pg.u32(m, worker.wrapping_add(0x10));
            take_jobs(m, c, pg, scheduler, mode, index, &WORKER_SITES);
            // the worker pointer is read back from the stack
            let worker_again = pg.u32(m, c.esp.wrapping_add(0x14));
            let done = pg.u32(m, worker_again.wrapping_add(0xc));
            call_import(m, c, pg, SET_EVENT, &[done], 0xdbdec9);
            if pg.u8(m, worker_again.wrapping_add(0x1c)) == 0 {
                continue;
            }
            return finish(c);
        }
        if pg.u8(m, worker.wrapping_add(0x1c)) != 0 {
            return finish(c);
        }
    }
}

/// 0x00DBE560, `scheduler->run(mode)` (thiscall, `ret 4`): run the queued
/// batch of `+0x8c` jobs in batch mode `mode`.
///
/// With no jobs only the profiler hook fields are cleared. The first batch
/// creates the worker threads (0x00DBDF30). One job, or one participant
/// (`+0x98`), runs on the calling thread alone. Otherwise the participating
/// workers (flag bit 0 of their record, `+0x1bdc0 + 0x20 k + 0x2c`) are
/// reset and woken, the caller takes jobs too ([`run_jobs`], as worker 0),
/// and waits for all participants' events at once. Afterwards the job
/// count and the hook fields are cleared.
pub fn schedule(m: &mut dyn Mem, c: &mut Cpu) {
    let pg = Pages::of(m);
    let entry = c.esp;
    let this = c.ecx;
    let saved = (c.ebx, c.ebp, c.esi, c.edi);
    let mode = pg.u32(m, entry.wrapping_add(4));
    let finish = |m: &mut dyn Mem, c: &mut Cpu| {
        (c.ebx, c.ebp, c.esi, c.edi) = saved;
        let _ = m;
        c.esp = entry.wrapping_add(8);
    };
    let clear_hook = |m: &mut dyn Mem| {
        pg.set_u32(m, this.wrapping_add(0x1c148), 0);
        pg.set_u32(m, this.wrapping_add(0x1c14c), 0);
    };
    if pg.u32(m, this.wrapping_add(0x8c)) == 0 {
        clear_hook(m);
        return finish(m, c);
    }
    // SUB ESP, 0x28; EBP and ESI pushed, then EBX
    if pg.u8(m, this.wrapping_add(0x6d)) == 0 {
        c.ecx = this;
        call_from(m, c, pg, entry.wrapping_sub(0x34), this, &[], 0xdbe596, |m, c| call_ext(m, c, 0x00db_df30));
        pg.set_u8(m, this.wrapping_add(0x6d), 1);
    }
    // EDI pushed
    let frame = entry.wrapping_sub(0x38);
    pg.set_u32(m, this.wrapping_add(0x90), 0);
    pg.set_u32(m, this.wrapping_add(0x94), mode);
    let workers = this.wrapping_add(0x1bdc0);
    if pg.u32(m, this.wrapping_add(0x8c)) == 1 || pg.u32(m, this.wrapping_add(0x98)) == 1 {
        // alone on the calling thread, as if there were one participant
        let participants = pg.u32(m, this.wrapping_add(0x98));
        pg.set_u32(m, this.wrapping_add(0x98), 1);
        c.esp = frame;
        call_from(m, c, pg, frame, this, &[workers], 0xdbe6b9, |m, c| call_ext(m, c, 0x00db_d8d0));
        pg.set_u32(m, this.wrapping_add(0x98), participants);
        pg.set_u32(m, this.wrapping_add(0x8c), 0);
        clear_hook(m);
        return finish(m, c);
    }
    let participants = pg.u32(m, this.wrapping_add(0x98));
    c.esp = frame;
    if participants > 1 {
        set_job_flags(m, pg, true);
    }
    pg.set_u8(m, this.wrapping_add(0xb8), 1);
    // wake the participating workers: their wake event reset, their go event set
    let mut flags = [false; 8];
    let mut record = this.wrapping_add(0x1bdec);
    let mut k = 1u32;
    while k < pg.u32(m, this.wrapping_add(0x98)) {
        let flag = pg.u8(m, record.wrapping_add(0xc)) & 1;
        flags[(k & 7) as usize] = flag != 0;
        if flag != 0 {
            let event = pg.u32(m, record);
            call_import(m, c, pg, RESET_EVENT, &[event], 0xdbe5f9);
            let event = pg.u32(m, record.wrapping_sub(4));
            call_import(m, c, pg, SET_EVENT, &[event], 0xdbe603);
        }
        k += 1;
        record = record.wrapping_add(0x20);
    }
    // the caller takes jobs too (cdecl: its argument stays pushed)
    call_from(m, c, pg, frame, this, &[workers], 0xdbe61c, |m, c| call_ext(m, c, 0x00db_d8d0));
    c.esp = c.esp.wrapping_add(4);
    // collect the participants' done events and wait for all of them
    let mut count = 0u32;
    let handles = frame.wrapping_add(0x18);
    let limit = pg.u32(m, this.wrapping_add(0x98));
    let mut record = this.wrapping_add(0x1bdec);
    let mut k = 1u32;
    while k < limit {
        if flags[(k & 7) as usize] {
            let handle = pg.u32(m, record);
            pg.set_u32(m, handles.wrapping_add(4 * count), handle);
            count += 1;
        }
        k += 1;
        record = record.wrapping_add(0x20);
    }
    if count != 0 {
        call_import(m, c, pg, WAIT_FOR_MULTIPLE_OBJECTS, &[count, handles, 1, u32::MAX], 0xdbe66d);
    }
    if pg.u32(m, this.wrapping_add(0x98)) > 1 {
        set_job_flags(m, pg, false);
    }
    pg.set_u32(m, this.wrapping_add(0x8c), 0);
    clear_hook(m);
    pg.set_u8(m, this.wrapping_add(0xb8), 0);
    finish(m, c);
}

/// 0x00DC5C20, `set_job_flags(on)` (cdecl): the low byte of the argument
/// to the two bytes at 0x018D0BB0 and 0x018D0404. Done in place.
fn set_job_flags(m: &mut dyn Mem, pg: Pages, on: bool) {
    pg.set_u8(m, 0x018d_0bb0, on as u8);
    pg.set_u8(m, 0x018d_0404, on as u8);
}
