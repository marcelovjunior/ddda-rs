//! Lifted body against readable port, per call (docs/MATH.md, "Performance").
//!
//! Each wired function runs three ways on the same inputs:
//! - `lifted`: the lifted body (`ddlift_NN::replaced::f_X`) over guest memory;
//! - `adapter`: the port behind the lifted ABI (`mtmath::abi::f_X`), which
//!   is what the lifted program now calls;
//! - `readable`: the readable function on Rust values, no guest memory.
//!
//! Guest memory is a flat `Image`; the call frame is rebuilt every
//! iteration. Times are the median of several runs, in nanoseconds per call.
//! Ports that call the CRT (through `x86::call_ext`) are not timed here:
//! they need the runtime's dispatcher.
use ddnative_base::mem::{Image, Mem};
use ddnative_base::x86::{Cpu, Entry, LiftFn};
use mtmath::{abi, angle, matrix, random::Random, F32, Vec3};
use std::hint::black_box;
use std::time::Instant;

const BASE: u32 = 0x1000_0000;
const OBJ: u32 = BASE + 0x100; // output object / generator state
const VEC: u32 = BASE + 0x200; // input vectors, 16 bytes apart
const ESP: u32 = BASE + 0x8000;
const RET: u32 = 0x0DEA_D000;

fn memory() -> Image {
    let mut m = Image { base: BASE, bytes: vec![0; 0x10000] };
    let floats = [1.5f32, -2.0, 3.25, 0.0, 10.0, 4.0, -7.5, 0.0, 0.0, 1.0, 0.0, 0.0, 0.3, 0.4, 0.5, 2.0];
    for (k, v) in floats.iter().enumerate() {
        m.set_u32(VEC + 4 * k as u32, v.to_bits());
    }
    for (k, v) in [0x1234_5678u32, 0x9ABC_DEF0, 0x0F0F_0F0F, 0xDEAD_BEEF].iter().enumerate() {
        m.set_u32(OBJ + 4 * k as u32, *v);
    }
    m
}

/// One call: return address and stack arguments at ESP, `this` in ECX
/// (EAX, EDX, ESI and EDI point at the input vectors and the output, for
/// the functions with register arguments).
fn call(f: LiftFn, m: &mut Image, args: &[u32], xmm: [u32; 2]) -> Cpu {
    m.set_u32(ESP, RET);
    for (k, a) in args.iter().enumerate() {
        m.set_u32(ESP + 4 + 4 * k as u32, *a);
    }
    let mut cpu = Cpu::from(Entry { eax: VEC, ecx: OBJ, edx: VEC + 0x20, ebx: 0, esp: ESP, ebp: 0, esi: VEC + 0x30,
                                    edi: OBJ + 0x40, xmm0: xmm[0] });
    cpu.xmm[1][0] = xmm[1];
    f(m, &mut cpu);
    cpu
}

fn time(mut run: impl FnMut()) -> f64 {
    let n = 200_000;
    let mut runs: Vec<f64> = (0..7)
        .map(|_| {
            let t = Instant::now();
            for _ in 0..n {
                run();
            }
            t.elapsed().as_nanos() as f64 / n as f64
        })
        .collect();
    runs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    runs[runs.len() / 2]
}

struct Case {
    name: &'static str,
    lifted: LiftFn,
    adapter: LiftFn,
    args: Vec<u32>,
    xmm: [u32; 2],
    readable: Box<dyn FnMut()>,
}

fn main() {
    let v = |k: u32| Vec3::new(F32(1.5 + k as f32), F32(-2.0 * k as f32), F32(3.25));
    let mut rng = Random { x: 1, y: 2, z: 3, w: 4 };
    let cases = vec![
        Case { name: "MtRandom::next 0x00D00690", lifted: ddlift_13::replaced::f_00d00690,
               adapter: abi::f_00d00690, args: vec![], xmm: [0; 2],
               readable: Box::new(move || { black_box(rng.next()); }) },
        Case { name: "MtRandom::seed 0x00D006C0", lifted: ddlift_13::replaced::f_00d006c0,
               adapter: abi::f_00d006c0, args: vec![12345], xmm: [0; 2],
               readable: Box::new(|| { black_box(Random::default().seed(black_box(12345))); }) },
        Case { name: "angle_difference 0x007AD870", lifted: ddlift_07::replaced::f_007ad870,
               adapter: abi::f_007ad870, args: vec![], xmm: [(-2.5f32).to_bits(), 2.75f32.to_bits()],
               readable: Box::new(|| { black_box(angle::angle_difference(black_box(F32(-2.5)), F32(2.75))); }) },
        Case { name: "unit_normal 0x007AEED0", lifted: ddlift_07::replaced::f_007aeed0,
               adapter: abi::f_007aeed0, args: vec![VEC + 0x10], xmm: [0; 2],
               readable: Box::new(move || {
                   black_box(mtmath::vector::unit_normal(black_box(v(0)), v(2), || (v(1), v(2))));
               }) },
        Case { name: "basis_from_forward 0x007AF5B0", lifted: ddlift_07::replaced::f_007af5b0,
               adapter: abi::f_007af5b0, args: vec![OBJ + 0x40, VEC + 0x30], xmm: [0; 2],
               readable: Box::new(move || {
                   black_box(mtmath::vector::basis_from_forward(black_box(v(0)), v(2), v(1), v(3)));
               }) },
        Case { name: "orthoRH 0x00CFC8B0", lifted: ddlift_12::replaced::f_00cfc8b0,
               adapter: abi::f_00cfc8b0, args: [1280.0f32, 720.0, 0.1, 1000.0].map(f32::to_bits).to_vec(), xmm: [0; 2],
               readable: Box::new(|| { black_box(matrix::ortho_rh(black_box(F32(1280.0)), F32(720.0), F32(0.1), F32(1000.0))); }) },
        Case { name: "lookAtRH 0x00CFD5E0", lifted: ddlift_12::replaced::f_00cfd5e0,
               adapter: abi::f_00cfd5e0, args: vec![VEC, VEC + 0x10, VEC + 0x20], xmm: [0; 2],
               readable: Box::new(move || { black_box(matrix::look_at_rh(black_box(v(0)), v(1), v(2))); }) },
        Case { name: "lookAtLH 0x00CFD890", lifted: ddlift_12::replaced::f_00cfd890,
               adapter: abi::f_00cfd890, args: vec![VEC, VEC + 0x10, VEC + 0x20], xmm: [0; 2],
               readable: Box::new(move || { black_box(matrix::look_at_lh(black_box(v(0)), v(1), v(2))); }) },
        Case { name: "reflect 0x00CFDB10", lifted: ddlift_12::replaced::f_00cfdb10,
               adapter: abi::f_00cfdb10, args: vec![VEC + 0x30], xmm: [0; 2],
               readable: Box::new(|| {
                   let p = mtmath::types::Plane { a: F32(0.3), b: F32(0.4), c: F32(0.5), d: F32(2.0) };
                   black_box(matrix::reflect(black_box(p)));
               }) },
    ];
    println!("{:<30} {:>10} {:>10} {:>10} {:>8}", "function", "lifted", "adapter", "readable", "speedup");
    for mut c in cases {
        // the two machine-level forms must agree before they are timed
        let (mut m1, mut m2) = (memory(), memory());
        let (a, b) = (call(c.lifted, &mut m1, &c.args, c.xmm), call(c.adapter, &mut m2, &c.args, c.xmm));
        assert_eq!((a.eax, a.ecx, a.edx, a.esp, a.xmm[0]), (b.eax, b.ecx, b.edx, b.esp, b.xmm[0]), "{}", c.name);
        assert!(m1.bytes == m2.bytes, "{}: memory differs", c.name);
        let lifted = time(|| { black_box(call(c.lifted, &mut m1, &c.args, c.xmm)); });
        let adapter = time(|| { black_box(call(c.adapter, &mut m2, &c.args, c.xmm)); });
        let readable = time(&mut c.readable);
        println!("{:<30} {:>8.1}ns {:>8.1}ns {:>8.1}ns {:>7.1}x", c.name, lifted, adapter, readable, lifted / adapter);
    }
}
