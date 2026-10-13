//! Synthetic contracts supplement (not replace) the private original oracle.
use mtstate::{descriptor, layout, packed, SAMPLER};

#[test]
fn rejects_out_of_table_descriptors() {
    assert!(descriptor::raster([2, 0, 0, 0]).is_err());
    assert!(descriptor::blend([13 << 9, 0, 0, 0]).is_err());
    assert!(descriptor::depth([8 << 2, 0]).is_err());
    assert!(descriptor::sampler([5 << 8, 0, 0, 0, 0, 0, 0, 0]).is_err());
    assert!(layout::from_descriptors(&[[0, 31 << 6]]).is_err());
    assert!(layout::from_descriptors(&[[0, 1 << 6 | 127 << 11 | 255 << 22]]).is_err());
}

#[test]
fn delta_sampler_option_is_not_full_apply_option() {
    let p = [2 << 9 | 3 << 13 | 2 << 17 | 16 << 21, 0, 0];
    let full = packed(SAMPLER, &p, None, Some(0), 1);
    assert!(full.iter().any(|c| c.args == [0, 6, 3]));
    let delta = packed(SAMPLER, &p, Some(&p), Some(0), 1);
    assert_eq!(delta.len(), 1);
    assert_eq!(delta[0].args, [0, 6, 2]);
}

#[test]
fn source_semantics_do_not_define_generated_usage() {
    let a = layout::from_descriptors(&[[123, 1 << 6 | 4 << 11]]).unwrap();
    let b = layout::from_descriptors(&[[999, 1 << 6 | 4 << 11]]).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.elements, [[0, 3], [255, 17]]);
    assert_eq!(a.stride_20, 16);
}

#[test]
fn invalid_float_border_channels_follow_cvtt_indefinite() {
    let p = descriptor::sampler([
        0, 0, 0x7fc00000, 0, 0x7fc00000, 0x7f800000, 0xff800000, 0x7f7fffff,
    ])
    .unwrap();
    assert_eq!(p[1], 0);
    assert_eq!(p[0] >> 26 & 15, 0);
}
