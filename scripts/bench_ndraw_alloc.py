"""CPU-only allocation/read-order benchmark of the actual Shadow::prune method.

Extracts the method from two source revisions into a standalone rustc harness;
only its guest-memory dependency is replaced by a deterministic word reader.
Never builds lifted shards or starts the game. Run outside head's timing pause:
  python scripts/bench_ndraw_alloc.py --before-ref 1eea00dd
"""
import argparse
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
SOURCE = 'crates/ddruntime/src/ndraw_native.rs'


def method(source):
    start = source.index('    fn prune(&mut self, g: &G, mirror: &Mirror)')
    end = source.index('\n    }', start) + len('\n    }')
    return source[start:end].replace('    fn prune(', '    pub fn prune(', 1)


FIXTURE = r'''
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::Instant;
struct Counter;
static CALLS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
#[global_allocator] static ALLOC: Counter = Counter;
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        CALLS.fetch_add(1, Relaxed); BYTES.fetch_add(l.size() as u64, Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) { unsafe { System.dealloc(p, l) } }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        CALLS.fetch_add(1, Relaxed); BYTES.fetch_add(n as u64, Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}
struct G { trace: Option<std::cell::RefCell<Vec<u32>>> }
impl G {
    fn u32(&self, a: u32) -> u32 {
        if let Some(trace) = &self.trace { trace.borrow_mut().push(a); }
        a / 4
    }
}
struct Mirror { rows: [[u32;14];21], textures: [u32;21] }
fn slot_of(stage: u32) -> Option<usize> {
    match stage { 0..=15 => Some(stage as usize), 256..=260 => Some(stage as usize - 240), _ => None }
}
'''
STRUCT = r'''
#[derive(Default)]
pub struct Shadow { pub ctx: Vec<(u32,u32)>, pub sampler_states: Vec<(u32,u32,u32)>, pub textures: Vec<(u32,u32)> }
'''
MAIN = r'''
fn main() {
    let mirror = Mirror { rows: [[0;14];21], textures: [0;21] };
    let mut seed = 17u32;
    for len in 0..256 { for _ in 0..100 {
        let input: Vec<_> = (0..len).map(|_| {
            seed ^= seed<<13; seed ^= seed>>17; seed ^= seed<<5;
            ((seed%32)*4, (seed>>16)%32)
        }).collect();
        let mut a = before::Shadow { ctx: input.clone(), ..Default::default() };
        let mut b = after::Shadow { ctx: input, ..Default::default() };
        let ga = G { trace: Some(Default::default()) }; let gb = G { trace: Some(Default::default()) };
        a.prune(&ga, &mirror); b.prune(&gb, &mirror);
        assert_eq!(a.ctx, b.ctx); assert_eq!(ga.trace, gb.trace);
    }}
    println!("equivalence: 25600 cases, output and guest read order match");
    for len in [0,4,16,32,64] {
        let input: Vec<_> = (0..len).map(|i| ((i%21)*4, i)).collect();
        let mut a = before::Shadow { ctx: Vec::with_capacity(len as usize), ..Default::default() };
        let mut b = after::Shadow { ctx: Vec::with_capacity(len as usize), ..Default::default() };
        let g = G { trace: None };
        macro_rules! bench { ($label:expr, $round:expr, $shadow:expr) => {{
            let calls = CALLS.load(Relaxed); let bytes = BYTES.load(Relaxed); let at = Instant::now();
            for _ in 0..100000 {
                $shadow.ctx.clear(); $shadow.ctx.extend_from_slice(std::hint::black_box(&input));
                $shadow.prune(std::hint::black_box(&g), std::hint::black_box(&mirror));
                std::hint::black_box(&$shadow);
            }
            let ns = at.elapsed().as_nanos() as f64 / 100000.;
            let calls = CALLS.load(Relaxed)-calls; let bytes = BYTES.load(Relaxed)-bytes;
            println!("len={len} round={} {} ns={ns:.3} allocs={} bytes={}", $round, $label, calls as f64/100000., bytes as f64/100000.);
        }}; }
        for round in 0..7 {
            if round%2 == 0 { bench!("before", round, a); bench!("after", round, b); }
            else { bench!("after", round, b); bench!("before", round, a); }
        }
    }
}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--before-ref', default='1eea00dd')
    parser.add_argument('--after-file', type=Path, default=ROOT / SOURCE)
    parser.add_argument('--target', type=Path, default=Path('C:/ddda-targets/L154'))
    args = parser.parse_args()
    pause = subprocess.run(['python', 'E:/ddda-re/tools/pause.py', 'status'], capture_output=True, text=True, check=True)
    if pause.stdout.strip() != 'not paused':
        raise SystemExit('Head timing pause: no benchmark started')
    before = subprocess.check_output(['git', 'show', f'{args.before_ref}:{SOURCE}'], cwd=ROOT, text=True)
    after = args.after_file.read_text()
    source = FIXTURE
    for name, text in [('before', before), ('after', after)]:
        source += f'\nmod {name} {{ use super::*;\n' + STRUCT + '\nimpl Shadow {\n' + method(text) + '\n}\n}\n'
    source += MAIN
    args.target.mkdir(parents=True, exist_ok=True)
    rust = args.target / 'ndraw_alloc_bench.rs'
    exe = args.target / 'ndraw_alloc_bench.exe'
    rust.write_text(source)
    subprocess.run([os.environ.get('RUSTC', 'rustc'), '--edition=2024', '-O', str(rust), '-o', str(exe)], check=True, timeout=60)
    result = subprocess.run([str(exe)], capture_output=True, text=True, timeout=60)
    (args.target / 'ndraw_alloc_bench.log').write_text(result.stdout + result.stderr)
    print(result.stdout, end='')
    if result.returncode:
        raise SystemExit(result.stderr or result.returncode)


if __name__ == '__main__':
    main()
