//! Rift rules: hiring cost, the random number generator and the offline
//! pawn pool. Each function names the exe code it was translated from
//! (DDDA.exe 2017-12-13 Steam). See `docs/PAWNS.md`.

use crate::{NoraFile, RomPawnId, NORA_FILES, NORA_LEVEL_BANDS};

/// Upper bound of a hiring cost (`0x5f5e0ff` in `0x0043eec0`).
pub const MAX_HIRE_COST: i32 = 99_999_999;
/// Constant term of the cost formula (f32 at `0x0150a2fc`).
pub const HIRE_COST_BASE: f32 = 18.0;
/// Default cost multiplier (`sNetwork+0x5ab0`, set to 1.0 by the sNetwork
/// constructor `0x004676c0` and whenever the online setting fails to load,
/// `0x004704e0`). Online, it is a downloaded integer x 0.001.
pub const DEFAULT_COST_MULTIPLIER: f32 = 1.0;

/// The multiplier the cost function actually uses: values below 0.009 or
/// above 100.001 are reset to 1.0 (`0x0043eec0`).
pub fn effective_multiplier(m: f32) -> f32 {
    if m < 0.009 || m > 100.001 {
        1.0
    } else {
        m
    }
}

/// Rift Crystal cost of hiring a pawn.
///
/// Translated from `0x0043eec0` (DDDA.exe 2017-12-13 Steam). `player_level`
/// is the Arisen's level, `pawn_level` the pawn's (`cCmcInfo+0x700`).
/// Pawns whose owner is on the player's friend list (`0x0043ef90`, network
/// pawns only) and pawns at or below the player's level are free. Otherwise
/// `trunc(m * ((L + P) * (L - P)^2 + 18))`, in single precision, clamped to
/// `0..=99_999_999` (a float overflow converts to `i32::MIN` and ends as 0,
/// as `CVTTSS2SI` does).
pub fn hire_cost(player_level: i32, pawn_level: u32, multiplier: f32, owner_is_friend: bool) -> i32 {
    if owner_is_friend {
        return 0;
    }
    // FILD of a u32 with the sign fix-up, then stored as f32.
    let l = pawn_level as f64 as f32;
    let p = player_level as f32;
    let mut cost = 0i32;
    if l > p {
        let d = l - p;
        let x = (l + p) * (d * d) + HIRE_COST_BASE;
        let y = effective_multiplier(multiplier) * x;
        cost = if y.is_nan() || y >= 2_147_483_648.0 || y < -2_147_483_648.0 { i32::MIN } else { y as i32 };
    }
    if cost > MAX_HIRE_COST {
        MAX_HIRE_COST
    } else if cost < 0 {
        0
    } else {
        cost
    }
}

/// Source of random numbers for the pool rules.
pub trait PawnRng {
    fn next_u32(&mut self) -> u32;
}

/// The game's generator: a xorshift128 variant with shifts (15, 17, 4).
///
/// Translated from `0x00d00690` (DDDA.exe 2017-12-13 Steam). The Rift code
/// uses the global state at `0x018d017c`; how that state is seeded is not
/// traced, so a matching sequence cannot be reproduced yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XorShift128 {
    pub state: [u32; 4],
}

impl PawnRng for XorShift128 {
    fn next_u32(&mut self) -> u32 {
        let [x, y, z, w] = self.state;
        let t = (x << 15) ^ x;
        let n = (((w >> 17) ^ t) >> 4) ^ w ^ t;
        self.state = [y, z, w, n];
        n
    }
}

impl XorShift128 {
    /// A generator seeded the way `0x00d006c0` seeds one. Translated from
    /// `0x00d006c0` (DDDA.exe 2017-12-13 Steam); compared live (oracle,
    /// board #724: seeds 0x1105 and 0 give exactly the test values):
    /// fixed start words, then 20 rounds that mix the seed through a small
    /// scrambler and four xorshift steps.
    pub fn seeded(seed: u32) -> XorShift128 {
        fn scramble(u: u32) -> u32 {
            let u = u ^ (0x65ac_9365u32 >> (u & 3));
            u ^ (((u >> 1) ^ u) >> 3) ^ (((u << 1) ^ u) << 3)
        }
        fn mix(a: u32, prev: u32) -> u32 {
            let t = (a << 15) ^ a;
            (((prev >> 17) ^ t) >> 4) ^ prev ^ t
        }
        let mut s = [0x075b_cd15u32, 0x159a_55e5, 0x1f12_3bb5, 0x0549_1333];
        let mut u1 = seed ^ 0x00ac_9365;
        let mut u3 = s[3];
        let mut u2 = 0;
        let mut u4 = 0;
        for _ in 0..20 {
            u1 = scramble(u1);
            u2 = mix(u1, u3);
            u1 = scramble(u1);
            u3 = mix(u1, u2);
            u1 = scramble(u1);
            u2 = mix(u1, u3);
            u1 = scramble(u1);
            u4 = mix(u1, u2);
            s[0] = u3;
            u1 = scramble(u1);
            u3 = mix(u1, u4);
        }
        s[1] = u2;
        s[2] = u4;
        s[3] = u3;
        XorShift128 { state: s }
    }
}

/// `(level offset, count)` pairs at `0x0150a358`: the offline pool is 25
/// pawns at the player's level, 16 at +2, 2 at -2, and so on (100 in all;
/// the table ends with `(0, 99)`).
pub const POOL_LEVEL_OFFSETS: [(i8, u8); 20] = [
    (0, 25),
    (2, 16),
    (-2, 2),
    (4, 16),
    (-4, 2),
    (6, 10),
    (-6, 2),
    (8, 6),
    (-8, 1),
    (10, 6),
    (-10, 1),
    (12, 2),
    (16, 2),
    (20, 2),
    (30, 2),
    (40, 1),
    (50, 1),
    (60, 1),
    (80, 1),
    (100, 1),
];

/// Number of Rift pool slots (`mCmcStock` slots 20..120 in sGameSys).
pub const POOL_SLOTS: usize = 100;
/// First `mCmcStock` slot of the Rift pool; slots 0..20 are story pawns.
pub const POOL_FIRST_STOCK_SLOT: usize = 20;

/// Nora unique ID for a level and template index (`0x00794f10`,
/// `0x007951c0`: `(level + 0x1000) * 0x10000 + index`).
pub fn nora_id(level: u32, index: u32) -> u32 {
    (level + 0x1000).wrapping_mul(0x10000).wrapping_add(index)
}

/// Level band file for any level, as the exe's loop does it (`0x00794f10`):
/// the first band whose bound is `>= level`, or file 0 if none.
pub fn band_of(level: u32) -> usize {
    NORA_LEVEL_BANDS.iter().position(|&max| level <= max as u32).unwrap_or(0)
}

/// Fills the Rift pool with `count` generated pawns, as the offline path
/// does (`count` = 100 offline, `100 - downloaded` online).
///
/// Translated from `0x007951c0` (DDDA.exe 2017-12-13 Steam).
/// - `player_level` is the Arisen's level (`sGameSys+0xa7dd0`), or `None`
///   while no player exists, which the exe treats as 5. It is clamped to
///   5..=200.
/// - Pawn `k` (0-based) gets the level `player_level + offset` of the
///   current [`POOL_LEVEL_OFFSETS`] entry (the player level if that leaves
///   1..=200) and goes to `pool[99 - k]` (stock slot `119 - k`).
/// - Its template is drawn from the band file at `rng() % len`, then the
///   following indices are tried while the ID is already anywhere in the
///   pool (including stale IDs from an earlier fill). Past the end of the
///   file the exe uses index 0 for every further try (`if idx >= len: 0`),
///   a quirk kept here. If all tries fail the same pawn is redrawn.
///
/// `pool` holds the `mRomPawnUniqueID` of stock slots 20..120 (`u32::MAX`
/// for an empty or network slot); network pawns compared by their network
/// IDs never collide with nora IDs, so they are not modelled.
pub fn fill_offline_pool(
    files: &[NoraFile],
    player_level: Option<u32>,
    count: usize,
    pool: &mut [u32; POOL_SLOTS],
    rng: &mut impl PawnRng,
) -> Result<(), String> {
    if files.len() != NORA_FILES {
        return Err(format!("need {NORA_FILES} nora files, have {}", files.len()));
    }
    let base = player_level.unwrap_or(5).clamp(5, 200) as i32;
    let count = count.min(POOL_SLOTS);
    let mut entry = 0usize;
    let mut left = POOL_LEVEL_OFFSETS[0].1 as i32;
    let mut k = 0usize;
    let mut redraws = 0usize;
    while k < count {
        // Past the 20 entries the table's sentinel (0, 99) applies.
        let offset = POOL_LEVEL_OFFSETS.get(entry).map_or(0, |e| e.0);
        let mut level = base + offset as i32;
        if !(1..=200).contains(&level) {
            level = base;
        }
        let file = &files[band_of(level as u32)];
        let len = file.pawns.len();
        if len == 0 {
            return Err(format!("band file {} is empty (the exe would loop forever)", band_of(level as u32)));
        }
        let start = rng.next_u32() as usize % len;
        let mut placed = false;
        for tries in 0..len {
            let mut index = tries + start;
            if index >= len {
                index = 0;
            }
            let id = nora_id(level as u32, index as u32);
            if !pool.contains(&id) {
                pool[POOL_SLOTS - 1 - k] = id;
                placed = true;
                break;
            }
        }
        if !placed {
            redraws += 1;
            if redraws > 10_000 {
                return Err("no free template after 10000 redraws".into());
            }
            continue;
        }
        k += 1;
        left -= 1;
        if left < 1 {
            entry += 1;
            left = POOL_LEVEL_OFFSETS.get(entry).map_or(99, |e| e.1 as i32);
        }
    }
    Ok(())
}

/// One random nora pawn for a "Pawns by Level" search entry.
///
/// Translated from `0x00794f10` (DDDA.exe 2017-12-13 Steam). Levels below 1
/// become 1; levels of 200 and more become 200 (the exe also draws one or
/// two extra random numbers there, depending on the player level; that
/// detail is reproduced). Returns the unique ID; the list builder
/// `0x0047a550` redraws up to 100 times while the ID is already listed.
pub fn random_nora_id(
    files: &[NoraFile],
    requested_level: i32,
    player_level: u32,
    rng: &mut impl PawnRng,
) -> Option<u32> {
    let level: u32 = if requested_level < 1 {
        1
    } else if requested_level > 199 {
        if player_level < 200 || rng.next_u32() & 1 == 0 {
            rng.next_u32();
        }
        200
    } else {
        requested_level as u32
    };
    let file = files.get(band_of(level))?;
    if file.pawns.is_empty() {
        return None;
    }
    let index = rng.next_u32() % file.pawns.len() as u32;
    Some(nora_id(level, index))
}

/// Decodes an ID back to its band file and template (the inverse used by
/// `0x00795010`: level over 200 becomes 0, index past the file becomes 0).
pub fn nora_template(files: &[NoraFile], id: u32) -> Option<(u8, usize, usize)> {
    let RomPawnId::Nora { level, index } = RomPawnId::decode(id) else { return None };
    let level = if level > 200 { 0 } else { level };
    let file = band_of(level as u32);
    let len = files.get(file)?.pawns.len();
    let index = if index as usize >= len { 0 } else { index as usize };
    Some((level, file, index))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hire_cost_rules() {
        // At or below the player's level: free.
        assert_eq!(hire_cost(10, 10, 1.0, false), 0);
        assert_eq!(hire_cost(10, 3, 1.0, false), 0);
        // (L+P)(L-P)^2 + 18 with L=11, P=10: 21*1 + 18 = 39.
        assert_eq!(hire_cost(10, 11, 1.0, false), 39);
        // L=20, P=10: 30*100 + 18 = 3018.
        assert_eq!(hire_cost(10, 20, 1.0, false), 3018);
        // Multiplier, and its sanitising.
        assert_eq!(hire_cost(10, 20, 0.5, false), 1509);
        assert_eq!(hire_cost(10, 20, 0.0, false), 3018);
        assert_eq!(hire_cost(10, 20, 200.0, false), 3018);
        // Friends' pawns are free.
        assert_eq!(hire_cost(1, 200, 1.0, true), 0);
        // Level 200 hired at level 1: 201 * 199^2 + 18 = 7_959_819.
        assert_eq!(hire_cost(1, 200, 1.0, false), 7_959_819);
        // The clamp.
        assert_eq!(hire_cost(1, 200, 100.0, false), MAX_HIRE_COST);
    }

    #[test]
    fn xorshift_matches_the_exe_shifts() {
        let mut r = XorShift128 { state: [1, 2, 3, 4] };
        // t = 1 ^ (1 << 15) = 0x8001; w = ((4 >> 17) ^ t) >> 4 ^ 4 ^ t
        let t = 0x8001u32;
        let expected = ((0 ^ t) >> 4) ^ 4 ^ t;
        assert_eq!(r.next_u32(), expected);
        assert_eq!(r.state, [2, 3, 4, expected]);
    }

    #[test]
    fn pool_offsets_sum_to_100() {
        assert_eq!(POOL_LEVEL_OFFSETS.iter().map(|e| e.1 as usize).sum::<usize>(), POOL_SLOTS);
    }

    #[test]
    fn nora_id_layout() {
        assert_eq!(nora_id(4, 634), 0x1004_027A);
        assert_eq!(band_of(0), 0);
        assert_eq!(band_of(171), 32);
        assert_eq!(band_of(250), 0);
    }
}

#[cfg(test)]
mod seed_tests {
    use super::*;

    /// The original's output: 0x00d006c0 called live on a scratch buffer
    /// (oracle, claude-4, board #724).
    #[test]
    fn seeded_regression() {
        assert_eq!(XorShift128::seeded(0x1105).state, [0xcab55256, 0x08501947, 0x5c500e44, 0xd9a8fda2]);
        assert_eq!(XorShift128::seeded(0).state, [0x0194fd72, 0x79e6c985, 0x08dd9701, 0x41cfce91]);
    }
}
