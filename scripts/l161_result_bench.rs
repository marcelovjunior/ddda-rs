// The two FEDD60 loop bodies are emitted from private cached instruction arms
// by l161_check.py. Inputs here are synthetic, not original-vector fixtures.
fn result_thread_cycles() -> u64 {
    #[link(name="kernel32")]
    extern "system" {
        fn GetCurrentThread() -> *mut std::ffi::c_void;
        fn QueryThreadCycleTime(thread: *mut std::ffi::c_void, cycles: *mut u64) -> i32;
    }
    let mut cycles=0;
    assert_ne!(unsafe { QueryThreadCycleTime(GetCurrentThread(),&mut cycles) },0);
    cycles
}

#[test]
#[ignore = "bounded CPU recorded-loop benchmark, no game"]
fn result_branch_cost() {
    use ddnative_base::mem::{PageTable, READ, WRITE, ATOMIC_ACCESSES};
    use std::{hint::black_box, time::Instant, sync::atomic::Ordering::Relaxed};
    ddnative_host::install();
    ATOMIC_ACCESSES.store(true, Relaxed);
    let mut m = PageMemory { table: Box::new(PageTable::default()) };
    for page in (16..25).chain(32..41) {
        m.table.backed_page(page); m.table.set_perm(page, READ | WRITE);
    }
    for a in 0x10000..0x19000 { m.set_u8(a,(a as u8).wrapping_mul(17)); }
    for (addr, baseline, candidate) in [
        (0xfedf26u32,crate::loop_baseline::f_00fedf26 as x86::LiftFn,crate::loop_candidate::f_00fedf26 as x86::LiftFn),
        (0xfedf44u32,crate::loop_baseline::f_00fedf44 as x86::LiftFn,crate::loop_candidate::f_00fedf44 as x86::LiftFn),
    ] {
        for count in [16u32,256,4096] {
            for offset in [0,1] {
                for round in 0..7 {
                    let mut states=Vec::new();
                    for step in 0..2 {
                        let fused=(round+step)%2==1;
                        let f=black_box(if fused {candidate} else {baseline});
                        let mut cpu=Cpu::default();
                        let iterations=(2_000_000/count).max(100);
                        let start=Instant::now();
                        let first_cycles=result_thread_cycles();
                        for _ in 0..iterations {
                            cpu.ecx=0x10000+offset;cpu.esi=0x20000+offset;
                            cpu.edx=count;cpu.edi=count;
                            f(black_box(&mut m),black_box(&mut cpu));
                        }
                        let cycles=(result_thread_cycles()-first_cycles) as f64/(iterations*count) as f64;
                        let ns=start.elapsed().as_nanos() as f64/(iterations*count) as f64;
                        let output:Vec<_>=(0..count*8).map(|k| m.u8(0x20000+offset+k)).collect();
                        for k in 0..count*8 { assert_eq!(output[k as usize],m.u8(0x10000+offset+k)); }
                        states.push((cpu.exit(),cpu.fl.snapshot(),output));
                        eprintln!("l161-result addr={addr:08x} count={count} offset={offset} round={round} fused={} ns={ns:.3} cycles={cycles:.3}",u8::from(fused));
                    }
                    assert_eq!(states[0],states[1]);
                }
            }
        }
    }
}
