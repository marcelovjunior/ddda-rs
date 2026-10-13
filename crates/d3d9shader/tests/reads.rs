//! L172: `Shader::float_reads`, the float constant components a shader may
//! read, on hand-built vs_3_0 token streams.
use d3d9shader::parse;

const VS_3_0: u32 = 0xFFFE_0300;
const END: u32 = 0x0000_FFFF;
const RELATIVE: u32 = 1 << 13;

fn ins(opcode: u32, params: u32) -> u32 {
    opcode | params << 24
}
fn param(kind: u32, index: u32, bits: u32) -> u32 {
    0x8000_0000 | (kind & 7) << 28 | (kind >> 3) << 11 | index | bits << 16
}
const TEMP: u32 = 0;
const INPUT: u32 = 1;
const CONST: u32 = 2;
const ADDR: u32 = 3;
/// swizzles (x in bits 0-1)
const XYZW: u32 = 0xE4;
const XXXX: u32 = 0x00;
const YZZZ: u32 = 1 | 2 << 2 | 2 << 4 | 2 << 6;
const WWWW: u32 = 0xFF;

#[test]
fn per_component_sources_read_the_swizzle_under_the_write_mask() {
    // mad r0.xy, v0, c5.x, c5.yzzz: c5 x, y, z (the CBSkinning pattern)
    let t = [
        VS_3_0,
        ins(4, 4),
        param(TEMP, 0, 0b0011),
        param(INPUT, 0, XYZW),
        param(CONST, 5, XXXX),
        param(CONST, 5, YZZZ),
        END,
    ];
    let r = parse(&t).unwrap().float_reads();
    assert_eq!(r.direct[5], 0b0111);
    assert!(!r.reads(5, 3) && r.reads(5, 0) && r.reads(5, 2));
    assert_eq!(r.any, 0);
    assert_eq!(r.direct.iter().filter(|&&m| m != 0).count(), 1);
    // the default swizzle with the full mask reads w
    let t = [VS_3_0, ins(1, 2), param(TEMP, 0, 0b1111), param(CONST, 5, XYZW), END];
    assert!(parse(&t).unwrap().float_reads().reads(5, 3));
    // ... and with a .xy mask only x y
    let t = [VS_3_0, ins(1, 2), param(TEMP, 0, 0b0011), param(CONST, 5, XYZW), END];
    assert_eq!(parse(&t).unwrap().float_reads().direct[5], 0b0011);
}

#[test]
fn dot_products_and_matrices_read_their_lanes_and_rows() {
    // dp3 r0.x, c6, v0: x y z of c6 whatever the mask
    let t = [VS_3_0, ins(8, 3), param(TEMP, 0, 0b0001), param(CONST, 6, XYZW), param(INPUT, 0, XYZW), END];
    assert_eq!(parse(&t).unwrap().float_reads().direct[6], 0b0111);
    // dp4 r0.x, c6, v0: all four
    let t = [VS_3_0, ins(9, 3), param(TEMP, 0, 0b0001), param(CONST, 6, XYZW), param(INPUT, 0, XYZW), END];
    assert_eq!(parse(&t).unwrap().float_reads().direct[6], 0b1111);
    // m4x3 r0.xyz, v0, c10: rows c10..c12, all four lanes
    let t = [VS_3_0, ins(21, 3), param(TEMP, 0, 0b0111), param(INPUT, 0, XYZW), param(CONST, 10, XYZW), END];
    let r = parse(&t).unwrap().float_reads();
    assert_eq!(&r.direct[9..14], &[0, 0b1111, 0b1111, 0b1111, 0]);
    // m3x2 r0.xy, v0, c20: two rows, x y z
    let t = [VS_3_0, ins(24, 3), param(TEMP, 0, 0b0011), param(INPUT, 0, XYZW), param(CONST, 20, XYZW), END];
    let r = parse(&t).unwrap().float_reads();
    assert_eq!(&r.direct[20..23], &[0b0111, 0b0111, 0]);
}

#[test]
fn relative_reads_may_reach_any_register() {
    // mov r0.x, c[a0.x + 3].w: w of any register
    let t = [
        VS_3_0,
        ins(1, 3),
        param(TEMP, 0, 0b0001),
        param(CONST, 3, WWWW) | RELATIVE,
        param(ADDR, 0, XXXX),
        END,
    ];
    let r = parse(&t).unwrap().float_reads();
    assert_eq!(r.any, 0b1000);
    assert!(r.reads(200, 3) && !r.reads(200, 0));
    assert_eq!(r.direct.iter().filter(|&&m| m != 0).count(), 0);
}

#[test]
fn unknown_operations_read_every_selected_component() {
    // rcp r0.x, c7.w (scalar: the replicated component)
    let t = [VS_3_0, ins(6, 2), param(TEMP, 0, 0b0001), param(CONST, 7, WWWW), END];
    assert_eq!(parse(&t).unwrap().float_reads().direct[7], 0b1000);
    // sincos r0.xy, c7.yzzz: lanes of the default (all four) selectors: y z
    let t = [VS_3_0, ins(37, 2), param(TEMP, 0, 0b0011), param(CONST, 7, YZZZ), END];
    assert_eq!(parse(&t).unwrap().float_reads().direct[7], 0b0110);
}
