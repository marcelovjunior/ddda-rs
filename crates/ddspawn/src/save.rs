//! The spawn inputs a game save holds (`DDDA.sav` XML, `ddsave`):
//! - `mScenarioNo` (SceNo), `mGameHour`, `mGameDay`;
//! - `mStgFlag[65]` (`cSAVE_DATA_STAGE::cSTAGE_FLAG`): per stage index,
//!   `Flag000` / `Flag032` / `Flag064` / `Flag096`, 128 layout flags MSB
//!   first; `mLotFlagField[4]`: stage 100's flags 128..255 (sGameSys
//!   +0xB3330);
//! - `sSetManager::cSaveWork` `mGroupDataEnemy[445]`: the enemy kill
//!   records ([`crate::respawn::Record`]) of every stage, each carrying its
//!   stage index (bits 1..7) and group (bits 8..16).

use crate::respawn::Record;
use ddsave::xml::Document;

#[derive(Debug, Clone, PartialEq)]
pub struct SaveState {
    pub scene: i32,
    pub hour: u32,
    pub day: u64,
    /// Per stage index: four words of 32 flags, MSB first.
    pub stage_flags: Vec<[u32; 4]>,
    /// Stage 100's flags 128..255.
    pub st100_extra: [u32; 4],
    /// `mGroupDataEnemy`.
    pub enemy_records: Vec<u64>,
}

fn bit(words: &[u32; 4], n: usize) -> bool {
    words
        .get(n / 32)
        .is_some_and(|w| w & (0x8000_0000 >> (n % 32)) != 0)
}

impl SaveState {
    pub fn from_xml(xml: &str) -> Result<SaveState, String> {
        let doc = Document::parse(xml).map_err(|e| format!("save xml: {e:?}"))?;
        let mut st = SaveState {
            scene: 0,
            hour: 0,
            day: 0,
            stage_flags: vec![],
            st100_extra: [0; 4],
            enemy_records: vec![],
        };
        let mut seen = (false, false, false, false, false, false);
        for n in doc.iter() {
            match n.name().as_deref() {
                Some("mScenarioNo") if !seen.0 => {
                    st.scene = n.parse().unwrap_or(0);
                    seen.0 = true;
                }
                Some("mGameHour") if !seen.1 => {
                    st.hour = n.parse().unwrap_or(0);
                    seen.1 = true;
                }
                Some("mGameDay") if !seen.2 => {
                    st.day = n.parse().unwrap_or(0);
                    seen.2 = true;
                }
                Some("mLotFlagField") if !seen.3 => {
                    let v: Vec<u32> = n.children().filter_map(|c| c.parse()).collect();
                    for (i, w) in v.iter().take(4).enumerate() {
                        st.st100_extra[i] = *w;
                    }
                    seen.3 = true;
                }
                Some("mStgFlag") if !seen.4 => {
                    for c in n.children() {
                        let w = |k: &str| c.get::<u32>(k).unwrap_or(0);
                        st.stage_flags
                            .push([w("Flag000"), w("Flag032"), w("Flag064"), w("Flag096")]);
                    }
                    seen.4 = true;
                }
                Some("mGroupDataEnemy") if !seen.5 => {
                    st.enemy_records = n.children().filter_map(|c| c.parse()).collect();
                    seen.5 = true;
                }
                _ => {}
            }
        }
        if !seen.0 || !seen.4 {
            return Err("save xml: no mScenarioNo / mStgFlag".into());
        }
        Ok(st)
    }

    /// Layout flag `n` of the stage at `stage_index` (index 0 is stage 100,
    /// whose flags 128..255 come from `mLotFlagField`).
    pub fn flag(&self, stage_index: usize, n: u16) -> bool {
        let n = usize::from(n);
        if n < 128 {
            self.stage_flags.get(stage_index).is_some_and(|w| bit(w, n))
        } else {
            stage_index == 0 && bit(&self.st100_extra, n - 128)
        }
    }

    /// The kill records of one stage, by group slot (295).
    pub fn records(&self, stage_index: u32) -> Vec<Record> {
        let mut out = vec![Record::default(); 295];
        for &b in &self.enemy_records {
            let r = Record { bits: b };
            if !r.in_use() || ((b >> 1) & 0x7f) as u32 != stage_index {
                continue;
            }
            let g = ((b >> 8) & 0x1ff) as usize;
            if let Some(slot) = out.get_mut(g) {
                *slot = r;
            }
        }
        out
    }
}
