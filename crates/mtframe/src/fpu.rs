//! The x87 operations the ports need, on the guest's x87 state.
//!
//! The original computes some values on the x87, whose results depend on
//! the guest's control word (precision and rounding control). These run
//! the same instructions on the guest state (`ddnative_base::x87`, the
//! host x87 with the guest's state swapped in), with the memory operand
//! passed by value.

use ddnative_base::x87::X87;

fn operand(bytes: &[u8]) -> [u8; 10] {
    let mut data = [0; 10];
    data[..bytes.len()].copy_from_slice(bytes);
    data
}

/// `fild dword`.
pub fn load_i32(fpu: &mut X87, v: i32) {
    fpu.exec::<0x00db>(&mut operand(&v.to_le_bytes()));
}

/// `fild qword`.
pub fn load_i64(fpu: &mut X87, v: i64) {
    fpu.exec::<0x28df>(&mut operand(&v.to_le_bytes()));
}

/// `fld dword`.
pub fn load_f32(fpu: &mut X87, bits: u32) {
    fpu.exec::<0x00d9>(&mut operand(&bits.to_le_bytes()));
}

/// `fadd dword`: st0 = st0 + v.
pub fn add_f32(fpu: &mut X87, bits: u32) {
    fpu.exec::<0x00d8>(&mut operand(&bits.to_le_bytes()));
}

/// `fdivr dword`: st0 = v / st0.
pub fn divr_f32(fpu: &mut X87, bits: u32) {
    fpu.exec::<0x38d8>(&mut operand(&bits.to_le_bytes()));
}

/// `fstp dword`: pops st0, rounded to f32.
pub fn store_f32(fpu: &mut X87) -> u32 {
    let mut data = [0; 10];
    fpu.exec::<0x18d9>(&mut data);
    u32::from_le_bytes(data[..4].try_into().unwrap())
}

/// `fstp qword`: pops st0, rounded to f64.
pub fn store_f64(fpu: &mut X87) -> u64 {
    let mut data = [0; 10];
    fpu.exec::<0x18dd>(&mut data);
    u64::from_le_bytes(data[..8].try_into().unwrap())
}

/// An unsigned 32-bit integer loaded the way MSVC converts one: `fild`
/// (signed), plus 2^32 (`[0x0161C220]` = 4294967296.0) when the sign bit
/// is set.
pub fn load_u32(fpu: &mut X87, v: u32) {
    load_i32(fpu, v as i32);
    if (v as i32) < 0 {
        add_f32(fpu, 0x4F80_0000);
    }
}
