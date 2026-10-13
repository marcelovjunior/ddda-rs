import glob, re
hdr = '''//! Float kernels of the uCnsEdit matrix providers (0x00864660, ...), generated
//! from the original's SSE instructions by research/native/cns_kernels/ (a
//! symbolic executor that follows the data flow of every float operation and
//! merges both arms of each float compare). Every operation is kept in the
//! original's order and grouping, including the multiplications by 0.0 whose
//! NaN/infinity propagation the original has, so results are bit-identical.
//! Do not edit by hand: regenerate.
#![allow(clippy::all, unused_parens, unused_variables, dead_code, non_upper_case_globals)]
use super::{read_f32 as rd, Bases, Matrix};
use ddnative_base::mem::{Mem, Pages};

/// The x87 load/store pair the original uses on some results: a signalling NaN comes back quiet.
fn quiet(x: f32) -> f32 {
    if x.is_nan() { f32::from_bits(x.to_bits() | 0x0040_0000) } else { x }
}

'''
body = ''
consts = {}
for f in sorted(glob.glob('k_*.rs')):
    t = open(f).read()
    for m in re.finditer(r'^const (K_\w+): f32 = (.*);$', t, re.M):
        consts[m.group(1)] = m.group(2)
    body += re.sub(r'^const K_.*\n', '', t, flags=re.M)
cs = ''.join('const %s: f32 = %s;\n' % (k, v) for k, v in sorted(consts.items()))
open('E:/ddda-rs-wt/T5/crates/mtcore/src/semantic_s5/kernels.rs', 'w').write(hdr + cs + '\n' + body)
