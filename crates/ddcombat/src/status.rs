//! Status build-up on a region: accumulation per hit, the chance roll,
//! thresholds, decay, and which status fires.
//!
//! Translated from 0x007772B0 (build-up), the status part of 0x007781A0
//! (decay) and 0x0076E340 (firing) of DDDA.exe (2017-12-13 Steam). The
//! fired status reaches the owner's vf74 with its index; what it does then
//! (duration, `rStatusParam`) is not covered here.

/// The 20 build-up slots of a region, in the order of
/// `cObjCollision::RegionStatus` (+0x70 accumulators, +0xC0 thresholds,
/// +0x164 chances, flag bits from +0x110 bit 3). Names follow the
/// `cCharParamEnemy` resistance labels that fill the thresholds
/// (0x00770FD0).
pub const SLOT_NAMES: [&str; 20] = [
    "耐毒 poison",
    "耐のろま slow",
    "耐暗闇 blind",
    "耐睡眠 sleep",
    "耐油まみれ oil",
    "耐ずぶ濡れ drenched",
    "耐敵化 enemy",
    "耐沈黙 silence",
    "耐スキル封印 skill seal",
    "耐呪い curse",
    "耐延焼 burning",
    "耐氷漬け frozen",
    "耐落雷 lightning",
    "耐久聖 holy",
    "耐久魔 dark",
    "耐石化 petrify",
    "耐攻撃力ダウン attack down",
    "耐防御力ダウン defence down",
    "耐魔法攻撃力ダウン magick attack down",
    "耐魔法防御力ダウン magick defence down",
];

/// Which of the attack's 15 status values (`mPoison`..`mMgcDefDown`, in
/// `rObjCollision::Attack` order) feeds each slot; None for the element
/// slots 10..14, which take the element build-up instead (0x007772B0).
pub const SLOT_SOURCE: [Option<usize>; 20] = [
    Some(0),  // mPoison
    Some(1),  // mSlow
    Some(2),  // mBlind
    Some(4),  // mSleep
    Some(5),  // mOil
    Some(6),  // mWet
    Some(8),  // mEnemy
    Some(3),  // mSilence
    Some(9),  // mSeal
    Some(7),  // mCurse
    None,     // fire
    None,     // ice
    None,     // thunder
    None,     // holy
    None,     // dark
    Some(10), // mStone
    Some(11), // mAtkDown
    Some(12), // mDefDown
    Some(13), // mMgcAtkDown
    Some(14), // mMgcDefDown
];

/// `0x015098C0`: the status kind `uEnemy::vf74` handles for each slot
/// (-1: handled by `uCharacterBase::vf74` itself: lightning chains and the
/// holy / dark effects). Kinds 14..17 are the `cStatus` types the damage
/// rules multiply by (0x0E attack, 0x0F defence, 0x10 magick attack, 0x11
/// magick defence), which ties those types to the "down" slots.
pub const SLOT_KIND: [i32; 20] = [0, 1, 2, 4, 5, 6, 7, 3, 8, 9, 10, 11, -1, -1, -1, 12, 14, 15, 16, 17];

/// One region's build-up state (`cObjCollision::RegionStatus`).
#[derive(Debug, Clone, PartialEq)]
pub struct StatusPools {
    pub acc: [f32; 20],
    /// Thresholds; `>= 10000` or 0 means the slot never builds up.
    pub threshold: [f32; 20],
    /// Chance (0..1) the roll must not exceed.
    pub chance: [f32; 20],
    /// Frames since the last build-up (decay timers +0x114..).
    pub timer: [f32; 20],
    /// Fired slots (bit i = slot i).
    pub fired: u32,
    /// +0x1DC bits: bit 0 raises the burning threshold by 1000 and lowers
    /// frozen and lightning by 800; bit 1 lowers burning by 800.
    pub region_bits: u32,
    /// +0x1CC / +0x1D0 frames and +0x1D4 / +0x1D8 decay per frame.
    pub decay_start: f32,
    pub decay_full: f32,
    pub decay_slow: f32,
    pub decay_fast: f32,
}

impl Default for StatusPools {
    /// The constructor values of 0x00776BA0: thresholds 999 (999.9 for
    /// slots 17 and 19), chances 1, decay after 150 frames at 0.5 per
    /// frame and after 450 frames at 1 per frame.
    fn default() -> Self {
        let mut threshold = [999.0f32; 20];
        threshold[17] = f32::from_bits(0x4479_f99a);
        threshold[19] = f32::from_bits(0x4479_f99a);
        StatusPools {
            acc: [0.0; 20],
            threshold,
            chance: [1.0; 20],
            timer: [0.0; 20],
            fired: 0,
            region_bits: 0,
            decay_start: 150.0,
            decay_full: 450.0,
            decay_slow: 0.5,
            decay_fast: 1.0,
        }
    }
}

impl StatusPools {
    /// The status part of 0x00770FD0: thresholds from the 20 `cCharParamEnemy`
    /// values `耐毒`..`耐魔法防御力ダウン` in memory order (+0x54..+0xA0, where
    /// `耐氷漬け` +0x7C precedes `耐延焼` +0x80); accumulators, timers and
    /// flags reset, chances 1.
    pub fn from_char_param(values: [f32; 20]) -> Self {
        let mut p = StatusPools::default();
        for (i, v) in values.iter().enumerate() {
            // memory index 10 is 耐氷漬け (slot 11), 11 is 耐延焼 (slot 10)
            let slot = match i {
                10 => 11,
                11 => 10,
                n => n,
            };
            p.threshold[slot] = *v;
        }
        p
    }
}

/// What one merged hit brings to the build-up.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StatusHit {
    /// The merged element (`HitInfo`+0xF8); only its slot is used.
    pub element: u32,
    /// Element build-up per element (`HitInfo`+0xB8 + 4*element).
    pub element_buildup: [f32; 6],
    /// The attack's 15 status values, summed over the merged hits.
    pub status: [f32; 15],
    /// Magick part after mitigation (> 0 selects rate +0xD2C).
    pub mag: f32,
}

/// Translated from 0x007772B0 (DDDA.exe 2017-12-13 Steam). `rate_phys` /
/// `rate_mag` are `cObjCollision`+0xD28 / +0xD2C (default 1); `resist` the
/// region's five element multipliers; `roll` yields the game's uniform
/// [0, 1) numbers in call order. Returns the slots that fired.
pub fn build_up(p: &mut StatusPools, hit: &StatusHit, resist: &[f32; 5], rate_phys: f32, rate_mag: f32, mut roll: impl FnMut() -> f32) -> u32 {
    let rate = if hit.mag <= 0.0 { rate_phys } else { rate_mag };
    let before = p.fired;
    if (1..=5).contains(&hit.element) {
        let e = hit.element as usize;
        let slot = 9 + e;
        let mut thr = p.threshold[slot];
        let mut ok = thr < 10000.0;
        if ok {
            match e {
                1 => {
                    if p.region_bits & 2 != 0 {
                        thr -= 800.0; // 0x017F42E4
                    }
                    if p.region_bits & 1 != 0 {
                        thr += 1000.0;
                    }
                }
                2 | 3 if p.region_bits & 1 != 0 => thr -= 800.0,
                _ => {}
            }
            ok = thr < 10000.0 && thr != 0.0;
        }
        if ok {
            let inc = rate * hit.element_buildup[e] * resist[e - 1];
            p.acc[slot] += inc;
            if roll() <= p.chance[slot] {
                if 0.0 < inc {
                    p.timer[slot] = 0.0;
                }
                if thr <= p.acc[slot] {
                    p.fired |= 1 << slot;
                    p.acc[slot] = thr;
                }
            }
        }
        // the latest element status replaces the others (second switch)
        match e {
            1 if p.fired & (1 << 10) != 0 => p.fired &= !((1 << 11) | (1 << 12)),
            2 if p.fired & (1 << 11) != 0 => p.fired &= !((1 << 10) | (1 << 12)),
            3 if p.fired & (1 << 12) != 0 => p.fired &= !((1 << 10) | (1 << 11)),
            _ => {}
        }
    }
    for (slot, source) in SLOT_SOURCE.iter().enumerate() {
        let Some(src) = *source else { continue };
        let thr = p.threshold[slot];
        if !(thr < 10000.0 && thr != 0.0) {
            continue;
        }
        let inc = hit.status[src] * rate;
        p.acc[slot] += inc;
        if roll() <= p.chance[slot] {
            if 0.0 < inc {
                p.timer[slot] = 0.0;
            }
            if thr <= p.acc[slot] {
                p.fired |= 1 << slot;
                p.acc[slot] = thr;
            }
        }
    }
    p.fired & !before
}

/// Translated from 0x0076E340's status part: each fired slot is reported
/// (in slot order; the game calls the owner's vf74 with it) and its
/// accumulator reset. Returns the slots, and whether slot 11 (frozen) was
/// among them (it cancels the stagger / knockdown reaction).
pub fn fire(p: &mut StatusPools) -> (Vec<usize>, bool) {
    let mut out = Vec::new();
    for slot in 0..20 {
        if p.fired & (1 << slot) != 0 {
            out.push(slot);
            p.acc[slot] = 0.0;
        }
    }
    p.fired = 0;
    let frozen = out.contains(&11);
    (out, frozen)
}

/// Translated from the decay part of 0x007781A0: per slot, frames since
/// the last build-up grow by `dt`; past `decay_start` the accumulator
/// drops by `decay_slow` per frame, past `decay_full` by `decay_fast`
/// (the timer stops there).
pub fn decay(p: &mut StatusPools, dt: f32) {
    for i in 0..20 {
        let t = dt + p.timer[i];
        p.timer[i] = t;
        let mut d = 0.0;
        if t <= p.decay_full {
            if p.decay_start <= t && t != p.decay_start {
                d = p.decay_slow;
            }
        } else {
            d = p.decay_fast;
            p.timer[i] = p.decay_full;
        }
        p.acc[i] = (p.acc[i] - d * dt).max(0.0);
    }
}
