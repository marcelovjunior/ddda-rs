//! d3d9shader::flow on hand-made ps_3_0 shaders (L113).
use d3d9shader::{flow::interpolants_only_address, op, parse, reg};

fn dst(kind: u8, index: u32, mask: u32) -> u32 {
    let k = kind as u32;
    0x8000_0000 | ((k & 7) << 28) | ((k >> 3) << 11) | index | (mask << 16)
}
fn src(kind: u8, index: u32, swizzle: u32) -> u32 {
    let k = kind as u32;
    0x8000_0000 | ((k & 7) << 28) | ((k >> 3) << 11) | index | (swizzle << 16)
}
fn ins(op: u16, operands: &[u32]) -> Vec<u32> {
    let mut v = vec![op as u32 | ((operands.len() as u32) << 24)];
    v.extend_from_slice(operands);
    v
}
/// An instruction with control bits (comparison, texld variant).
fn insc(op: u16, control: u32, operands: &[u32]) -> Vec<u32> {
    let mut v = ins(op, operands);
    v[0] |= control << 16;
    v
}
const XYZW: u32 = 0xE4;
fn r(i: u32) -> u32 {
    src(reg::TEMP, i, XYZW)
}
fn rd(i: u32) -> u32 {
    dst(reg::TEMP, i, 0xF)
}
fn v0() -> u32 {
    src(reg::INPUT, 0, XYZW)
}
fn c(i: u32) -> u32 {
    src(reg::CONST, i, XYZW)
}
fn s(i: u32) -> u32 {
    src(reg::SAMPLER, i, XYZW)
}
fn oc0() -> u32 {
    dst(reg::COLOROUT, 0, 0xF)
}
/// ps_3_0 with v0 as texcoord0 and samplers 0 and 1 declared.
fn ps(body: &[Vec<u32>]) -> d3d9shader::Shader {
    let mut v = vec![0xFFFF_0300];
    v.extend(ins(op::DCL, &[0x8000_0005, dst(reg::INPUT, 0, 0x3)]));
    v.extend(ins(op::DCL, &[0x9000_0000, dst(reg::SAMPLER, 0, 0xF)]));
    v.extend(ins(op::DCL, &[0x9000_0000, dst(reg::SAMPLER, 1, 0xF)]));
    for i in body {
        v.extend_from_slice(i);
    }
    v.push(0x0000_FFFF);
    parse(&v).unwrap()
}

#[test]
fn coordinates_from_interpolants_are_addressing_only() {
    // r0 = v0 * c0; r1 = texldl(r0, s0); r2 = texldl(r1, s1) (dependent); oC0 = r2 * c1
    let p = ps(&[
        ins(op::MUL, &[rd(0), c(0), v0()]),
        ins(op::TEXLDL, &[rd(1), r(0), s(0)]),
        ins(op::TEXLDL, &[rd(2), r(1), s(1)]),
        ins(op::MUL, &[oc0(), r(2), c(1)]),
    ]);
    let a = interpolants_only_address(&p).unwrap();
    assert_eq!(a.samplers, 0b01, "only s0's coordinates come from v0");
    assert_eq!(a.implicit_level, 0);
}

#[test]
fn an_output_or_branch_from_interpolants_is_rejected() {
    // oC0 = v0
    assert!(interpolants_only_address(&ps(&[ins(op::MOV, &[oc0(), v0()])])).is_err());
    // r0 = v0 + c0; oC0 = texldl(...) * r0
    let p = ps(&[
        ins(op::ADD, &[rd(0), v0(), c(0)]),
        ins(op::TEXLDL, &[rd(1), c(1), s(0)]),
        ins(op::MUL, &[oc0(), r(1), r(0)]),
    ]);
    assert!(interpolants_only_address(&p).is_err());
    // if_ge v0.x, c0.x (comparison control 3) ... endif
    let p = ps(&[
        insc(op::IFC, 3, &[v0(), c(0)]),
        ins(op::MOV, &[oc0(), c(1)]),
        ins(op::ENDIF, &[]),
    ]);
    assert!(interpolants_only_address(&p).is_err());
}

#[test]
fn a_write_in_a_branch_does_not_clear_taint() {
    // r0 = v0; if b0 { r0 = c0 } ; r1 = texldl(c1, s0); oC0 = r1 * r0
    let b0 = src(reg::CONSTBOOL, 0, XYZW);
    let p = ps(&[
        ins(op::MOV, &[rd(0), v0()]),
        ins(op::IF, &[b0]),
        ins(op::MOV, &[rd(0), c(0)]),
        ins(op::ENDIF, &[]),
        ins(op::TEXLDL, &[rd(1), c(1), s(0)]),
        ins(op::MUL, &[oc0(), r(1), r(0)]),
    ]);
    assert!(interpolants_only_address(&p).is_err());
    // without the branch the overwrite clears it
    let p = ps(&[
        ins(op::MOV, &[rd(0), v0()]),
        ins(op::MOV, &[rd(0), c(0)]),
        ins(op::TEXLDL, &[rd(1), c(1), s(0)]),
        ins(op::MUL, &[oc0(), r(1), r(0)]),
    ]);
    assert_eq!(interpolants_only_address(&p).unwrap().samplers, 0);
}

#[test]
fn a_branch_that_overwrites_an_interpolant_is_accepted() {
    // r0 = v0; r1 = texldl(r0, s0); if_ge r1.x, c0.x { r0 = texldl(r0, s1); oC0 = r0 } else { oC0 = r1 }
    let p = ps(&[
        ins(op::MOV, &[rd(0), v0()]),
        ins(op::TEXLDL, &[rd(1), r(0), s(0)]),
        insc(op::IFC, 3, &[src(reg::TEMP, 1, 0), c(0)]),
        ins(op::TEXLDL, &[rd(0), r(0), s(1)]),
        ins(op::MOV, &[oc0(), r(0)]),
        ins(op::ELSE, &[]),
        ins(op::MOV, &[oc0(), r(1)]),
        ins(op::ENDIF, &[]),
    ]);
    assert_eq!(interpolants_only_address(&p).unwrap().samplers, 0b11);
}
