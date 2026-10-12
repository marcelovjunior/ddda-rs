//! The engine's string hash: CRC-32 (reflected, polynomial 0xEDB88320)
//! over the bytes of a C string, starting from a caller-given value and
//! without the final inversion. DTI ids are `crc(name, 0xFFFFFFFF) &
//! 0x7FFFFFFF` (the MtDTI constructor 0x00CF5780).

use ddnative_base::mem::Mem;

/// The reflected CRC-32 polynomial.
pub const POLYNOMIAL: u32 = 0xEDB8_8320;

/// The CRC-32 table (computed; the original reads the identical table at
/// 0x014075C0, which the differential test checks against the exe).
pub const TABLE: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { POLYNOMIAL ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
};

/// One byte of the CRC.
pub const fn step(crc: u32, byte: u8) -> u32 {
    TABLE[((crc ^ byte as u32) & 0xFF) as usize] ^ (crc >> 8)
}

/// The CRC of the C string at `s` (its bytes up to, not including, the
/// terminating zero), starting from `crc`. Also returns the address of the
/// terminator (EDX in the original).
// Translated from 0x00D01B60 (DDDA.exe 2017-12-13 Steam), MtString hash (crc32 of a C string).
pub fn crc_str(m: &dyn Mem, s: u32, mut crc: u32) -> (u32, u32) {
    let mut p = s;
    loop {
        let c = m.u8(p);
        if c == 0 {
            return (crc, p);
        }
        crc = step(crc, c);
        p = p.wrapping_add(1);
    }
}

/// [`crc_str`] over Rust bytes (for tools: the id of a class name is
/// `crc_bytes(name, 0xFFFFFFFF) & 0x7FFFFFFF`).
pub fn crc_bytes(bytes: &[u8], crc: u32) -> u32 {
    bytes.iter().fold(crc, |c, &b| step(c, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_crc32_with_the_usual_inversions() {
        // CRC-32 of "123456789" is 0xCBF43926 with init !0 and final !
        assert_eq!(!crc_bytes(b"123456789", !0), 0xCBF4_3926);
        assert_eq!(TABLE[1], 0x7707_3096);
    }
}
