//! Source-backed knowledge-bit tables read from the player's executable.
//! Uses the existing pawns::Knowledge storage; never writes saves or awards bits.
use ddexe::{Error, Exe};

pub type Flags = [u32; 322];
pub const BIT_COUNT: u32 = 0x2840;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Quest,
    Enemy,
    Area,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub category: Category,
    /// Quest number, enemy class id or area/stage id, depending on category.
    pub id: u32,
    /// Ordered bit list, including any duplicates present in the table.
    pub bits: Vec<u32>,
    /// Quest +0xc is its completion bit. Other categories have other metadata.
    pub completion_bit: Option<u32>,
    /// Last word: message index used by the corresponding knowledge menu.
    /// Negative enemy values disable that menu row. Resource is category-specific.
    pub name_message: i32,
    /// Remaining table metadata, deliberately not assigned guessed semantics.
    pub metadata: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub learned: u32,
    pub total: u32,
    /// None is the original -1 result for an empty denominator.
    pub stars: Option<u8>,
}

/// Translated from 0x005d05e0 (DDDA.exe 2017-12-13 Steam).
pub fn has_bit(flags: &Flags, bit: u32) -> bool {
    bit < BIT_COUNT && (flags[(bit >> 5) as usize] & (1 << (bit & 31))) != 0
}

/// Translated from 0x005d0830 (DDDA.exe 2017-12-13 Steam).
/// Preserve f32 conversion/division: integer cross multiplication can differ
/// for large counts, even though the installed lists are small.
pub fn stars(learned: u32, total: u32) -> Option<u8> {
    if total == 0 {
        return None;
    }
    let ratio = learned as f32 / total as f32;
    Some(if ratio <= 0.0 {
        0
    } else if ratio < 0.5 {
        1
    } else if ratio < 1.0 {
        2
    } else {
        3
    })
}

impl Entry {
    /// Translated from 0x005d05e0/0x005d0830 (DDDA.exe 2017-12-13 Steam).
    pub fn progress(&self, flags: &Flags) -> Progress {
        let learned = self.bits.iter().filter(|&&bit| has_bit(flags, bit)).count() as u32;
        let total = self.bits.len() as u32;
        Progress {
            learned,
            total,
            stars: stars(learned, total),
        }
    }

    /// Quest completion/knows-quest predicate, not its fractional star count.
    /// Translated from 0x005d0760 (DDDA.exe 2017-12-13 Steam).
    pub fn quest_known(&self, flags: &Flags) -> bool {
        self.completion_bit
            .is_some_and(|bit| bit != 0 && has_bit(flags, bit))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Threshold {
    /// Authored table value before multiplication by 30 frame units.
    pub encounter_units: f32,
    pub kills: u32,
}

impl Threshold {
    /// Translated from 0x004172c0 (DDDA.exe 2017-12-13 Steam).
    /// Assembly 0x00417402 uses JA: equality at both thresholds passes.
    /// Reject nonfinite inputs explicitly; this is not the hardware NaN path.
    pub fn met(&self, encounter_frames: f32, kills: u32) -> bool {
        self.encounter_units.is_finite()
            && encounter_frames.is_finite()
            && encounter_frames >= self.encounter_units * 30.0
            && kills >= self.kills
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnemyCounter {
    pub enemy_id: u32,
    /// Index in both EncountFrame[72] and KillCnt[72].
    pub counter_index: usize,
    /// In order, gates for the first five bits of the enemy's knowledge list.
    pub thresholds: [Threshold; 5],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UniqueRule {
    pub bit: u32,
    /// Index in UniqueCnt[116]. The table has 113 rules, not 116.
    pub counter_index: usize,
    pub required: u8,
}

impl UniqueRule {
    /// Translated from 0x00417470 (DDDA.exe 2017-12-13 Steam).
    pub fn met(&self, counts: &[u8; 116]) -> bool {
        counts
            .get(self.counter_index)
            .is_some_and(|&count| count >= self.required)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tables {
    pub quests: Vec<Entry>,
    pub enemies: Vec<Entry>,
    pub areas: Vec<Entry>,
    pub enemy_counters: Vec<EnemyCounter>,
    pub unique_rules: Vec<UniqueRule>,
}

impl Tables {
    /// Table users: 0x005d06d0/0x0070ebc0 (quests), 0x005d0680/0x0070e910
    /// (enemies), 0x005d07e0/0x0070eea0 (areas), 0x004172c0/0x00417470
    /// (counters), DDDA.exe 2017-12-13 Steam. Bulk records stay in the install.
    pub fn from_image(exe: &Exe<'_>) -> Result<Self, Error> {
        fn entries(
            exe: &Exe<'_>,
            category: Category,
            va: u32,
            stride: usize,
            count: usize,
        ) -> Result<Vec<Entry>, Error> {
            exe.records(va, stride, count)?
                .iter()
                .map(|r| {
                    let n = r.u32(8)?;
                    // Bound before byte-count multiplication/allocation. This is a
                    // reader safety limit, not an original duplicate-bit rule.
                    if n > BIT_COUNT {
                        return Err(Error("knowledge bit-list too large".into()));
                    }
                    let bits = if n == 0 {
                        Vec::new()
                    } else {
                        exe.u32s(r.u32(4)?, n as usize)?
                    };
                    Ok(Entry {
                        category,
                        id: r.u32(0)?,
                        bits,
                        completion_bit: if category == Category::Quest {
                            Some(r.u32(12)?)
                        } else {
                            None
                        },
                        name_message: r.u32(stride - 4)? as i32,
                        metadata: (12..stride - 4)
                            .step_by(4)
                            .map(|o| r.u32(o))
                            .collect::<Result<_, _>>()?,
                    })
                })
                .collect()
        }
        let mut enemy_counters = Vec::new();
        for r in exe.records(0x0153_b6e8, 8, 92)? {
            let index = r.u32(4)? as usize;
            if index >= 72 {
                return Err(Error("enemy knowledge counter index outside save array".into()));
            }
            let frames = exe.f32s(0x0153_b9c8 + (index * 20) as u32, 5)?;
            let kills = exe.u32s(0x0153_bf68 + (index * 20) as u32, 5)?;
            enemy_counters.push(EnemyCounter {
                enemy_id: r.u32(0)?,
                counter_index: index,
                thresholds: std::array::from_fn(|i| Threshold {
                    encounter_units: frames[i],
                    kills: kills[i],
                }),
            });
        }
        let unique_rules = exe
            .records(0x0153_c508, 12, 113)?
            .iter()
            .map(|r| {
                let counter_index = r.u32(4)? as usize;
                if counter_index >= 116 {
                    return Err(Error("unique knowledge counter index outside save array".into()));
                }
                Ok(UniqueRule {
                    bit: r.u32(0)?,
                    counter_index,
                    required: r.u32(8)? as u8,
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            quests: entries(exe, Category::Quest, 0x0152_1e90, 24, 52)?,
            enemies: entries(exe, Category::Enemy, 0x0152_31e0, 20, 97)?,
            areas: entries(exe, Category::Area, 0x0152_2670, 24, 18)?,
            enemy_counters,
            unique_rules,
        })
    }

    /// Reverse area mapping. Source scans from the last row and only accepts
    /// bits above 0x203f and below0x2840; preserve precedence for shared bits.
    /// Translated from 0x005d08a0 (DDDA.exe 2017-12-13 Steam).
    pub fn area_for_bit(&self, bit: u32) -> Option<u32> {
        if !(0x2040..BIT_COUNT).contains(&bit) {
            return None;
        }
        self.areas
            .iter()
            .rev()
            .find(|a| a.bits.first().is_some_and(|&first| first <= bit) && a.bits.contains(&bit))
            .map(|a| a.id)
    }
}
