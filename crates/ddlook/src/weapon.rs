//! Weapon models and where they hang (actor +0x1330 primary, +0x1334
//! secondary; spawned as uWeaponPl by 0x00840180 / 0x008404b0).

/// The model path of a weapon id (category << 24 | number): number % 100,
/// with 20 read as 999, and a `_a`, `_b`... suffix per hundred above 99.
/// Categories 15 and 16 are a quiver whose model sits under `qui` with an
/// `arr` file name.
// Translated from 0x00bc1a00 (DDDA.exe 2017-12-13 Steam).
pub fn weapon_path(id: i32) -> Option<String> {
    if id == -1 {
        return None;
    }
    let id = id as u32;
    let n = id & 0xff_ffff;
    let mut num = n % 100;
    let suffix = if n < 100 { String::new() } else { format!("_{}", char::from(b'a' + ((n - 100) / 100) as u8)) };
    if num == 0x14 {
        num = 999;
    }
    let (dir, file) = match id >> 24 {
        1 => ("one", "one"),
        2 => ("mac", "mac"),
        3 => ("two", "two"),
        4 => ("dag", "dag"),
        5 => ("stk", "stk"),
        6 => ("wan", "wan"),
        7 => ("war", "war"),
        8 => ("sld", "sld"),
        9 => ("bld", "bld"),
        10 => ("bow", "bow"),
        11 => ("bbw", "bbw"),
        12 => ("mbw", "mbw"),
        13 | 14 => ("qui", "qui"),
        15 | 16 => ("qui", "arr"),
        _ => return None,
    };
    Some(format!("model\\wp\\{dir}\\{dir}{num:03}{suffix}\\{file}{num:03}{suffix}"))
}

/// The attach row of a weapon: the slot (0 primary, 2 secondary) and the
/// weapon's category.
// Translated from 0x00c1bad0 (DDDA.exe 2017-12-13 Steam).
pub fn attach_row(id: i32, slot: u32) -> usize {
    let cat = if id == -1 { 0 } else { (id as u32 >> 24) & 0xff };
    let cat = if cat > 0x10 { 0 } else { cat };
    if slot == 1 && cat == 4 {
        return 2;
    }
    match cat {
        1 | 2 => 0,
        3 => 0xb,
        4 => 1,
        5 => 5,
        6 => 6,
        7 => 0xc,
        8 => 3,
        9 => 4,
        10 | 12 => 7,
        11 => 8,
        13 => 9,
        14 => 10,
        _ => 0xd,
    }
}

/// Sheathed attachment by row (table 0x01872bc0, 23 rows of 0x30 bytes:
/// joint id, rotation in degrees at +0x10, offset in cm at +0x20, read by
/// 0x00840180 (joint) and 0x0080bcb0 (rotation, offset)). Row 13 is "no
/// weapon". Rows 14-22 are not selected by 0x00c1bad0.
pub const ATTACH: [(u8, [f32; 3], [f32; 3]); 23] = [
    (13, [0.0, 122.0, -77.0], [22.0, -4.3, 5.4]),
    (13, [0.0, -122.0, 77.0], [-22.0, -4.3, 5.4]),
    (13, [0.0, -122.0, 103.0], [22.0, -4.3, 5.4]),
    (11, [0.0, -30.0, -10.0], [13.0, 0.0, 0.0]),
    (2, [-84.0, -5.0, 19.0], [4.0, -1.0, -20.0]),
    (2, [85.0, 9.8, 168.0], [-5.0, 20.0, -22.0]),
    (2, [85.0, 9.8, 168.0], [-5.0, -4.0, -22.0]),
    (2, [-265.0, -4.0, 24.0], [-1.5, -13.0, -27.0]),
    (2, [-265.0, -4.0, 24.0], [-1.5, -13.0, -27.0]),
    (2, [5.0, 2.5, 25.0], [-0.8, 12.0, -20.0]),
    (2, [5.0, 2.5, 25.0], [4.2, -2.0, -20.0]),
    (2, [72.0, 180.0, 22.0], [-18.0, 52.0, -11.0]),
    (2, [72.0, 180.0, 22.0], [-26.0, 72.0, -4.0]),
    (0, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
    (11, [-29.0, 21.0, -20.0], [20.0, 0.0, 0.5]),
    (8, [0.0, -34.0, 0.0], [-7.0, -1.8, 1.0]),
    (8, [0.0, 146.0, 0.0], [-7.0, -1.8, 1.0]),
    (12, [180.0, 34.0, 0.0], [7.0, -1.8, 1.0]),
    (8, [0.0, 1.0, 0.0], [-8.5, -1.8, 14.0]),
    (8, [0.0, 1.0, 0.0], [-8.5, -1.8, 14.0]),
    (8, [0.0, -4.0, 0.0], [-6.5, -0.3, -2.8]),
    (8, [0.0, -4.0, 0.0], [-6.5, -0.3, -2.8]),
    (12, [-0.0, 0.0, -180.0], [-8.1, -3.2, 4.7]),
];

/// A weapon on the character.
#[derive(Debug, Clone, PartialEq)]
pub struct WeaponPart {
    pub id: i32,
    pub path: String,
    pub joint: u8,
    /// Degrees, applied X then Y then Z (uCnsAttach 0x00859bb0, mode 1).
    pub rotation: [f32; 3],
    /// Centimetres, in the joint's frame.
    pub offset: [f32; 3],
}

/// Weapons in the primary (slot 0) and secondary (slot 1) hands, sheathed.
pub fn weapons(primary: i32, secondary: i32) -> Vec<WeaponPart> {
    let mut out = vec![];
    for (id, slot) in [(primary, 0), (secondary, 1)] {
        let Some(path) = weapon_path(id) else { continue };
        let row = attach_row(id, if slot == 0 { 0 } else { 2 });
        let (joint, rotation, offset) = ATTACH[row];
        if row == 0xd {
            continue;
        }
        out.push(WeaponPart { id, path, joint, rotation, offset });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        assert_eq!(weapon_path(0x0500_0014).unwrap(), r"model\wp\stk\stk999\stk999");
        assert_eq!(weapon_path(0x0100_0000).unwrap(), r"model\wp\one\one000\one000");
        assert_eq!(weapon_path(0x0800_0002).unwrap(), r"model\wp\sld\sld002\sld002");
        assert_eq!(weapon_path(0x0100_00c8).unwrap(), r"model\wp\one\one000_b\one000_b");
        assert_eq!(weapon_path(0x1000_0001).unwrap(), r"model\wp\qui\qui001\arr001");
        assert_eq!(weapon_path(-1), None);
        assert_eq!(attach_row(0x0400_0001, 0), 1);
        assert_eq!(attach_row(0x0400_0001, 1), 2);
        assert_eq!(attach_row(0x0a00_0000, 0), 7);
    }
}
