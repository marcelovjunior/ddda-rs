//! Everything the Rift reads from the install and the player's exe.

use ddpawn::builder::BuilderData;
use ddpawn::tables::{CharaInfo, ItemKeyTable, ItemSlots, SkillList, SkillMasks, Study};
use ddpawn::{nora_path, NoraFile, PawnNames, RomPawnInfo, NORA_FILES};

/// Quest knowledge bits: the 52 records of 24 bytes at `.rdata 0x01521e90`
/// (`(quest, _, _, bit, _, _)`, sorted by quest), searched by `0x005d06d0`.
pub const QUEST_KNOWLEDGE_VA: u32 = 0x0152_1e90;
pub const QUEST_KNOWLEDGE_LEN: usize = 52;

/// The offline pawn data and every table the pawn builder needs, loaded
/// once per session by the host.
pub struct RiftData {
    pub nora: Vec<NoraFile>,
    pub info: RomPawnInfo,
    pub names: PawnNames,
    pub skills: SkillList,
    pub levels: Vec<ddparam::Level>,
    pub stamina: ddparam::Stamina,
    pub chara: CharaInfo,
    pub study: Study,
    pub items: ItemSlots,
    pub masks: SkillMasks,
    pub keys: ItemKeyTable,
    /// `(quest number, knowledge bit)` pairs.
    pub quest_knowledge: Vec<(u32, u32)>,
    /// NPC ledger `mNameJ` by NPC number, for the story pawns' names.
    pub npc_names: Vec<(u32, Vec<u8>)>,
    /// `sGameSys.mLanguage`: 0 Japanese keeps the data's names.
    pub language: u32,
    /// `sItemManager+0x5b00` (read live as 0, board #724).
    pub item_key_seed: u32,
}

impl RiftData {
    /// Loads from an install and the bytes of its `DDDA.exe`.
    pub fn load(
        vfs: &ddvfs::Vfs,
        exe: &[u8],
        npc_names: Vec<(u32, Vec<u8>)>,
        language: u32,
    ) -> Result<RiftData, String> {
        let read = |path: &str, class: &str| vfs.read(path, mtarc::type_hash(class));
        let nora = (0..NORA_FILES)
            .map(|i| NoraFile::parse(&read(&nora_path(i), "rRomNoraPawn")?))
            .collect::<Result<_, _>>()?;
        let levels = ["LvFighter", "LvStrider", "LvMagicUser", "LvBattler", "LvHunter", "LvWizard"]
            .iter()
            .map(|n| {
                ddparam::Level::from_bytes(&read(&format!(r"param\pl\level\{n}"), "rPlLevel")?).map_err(|e| e.0)
            })
            .collect::<Result<_, String>>()?;
        let image = ddexe::Exe::parse(exe).map_err(|e| e.0)?;
        let quest_knowledge = image
            .records(QUEST_KNOWLEDGE_VA, 24, QUEST_KNOWLEDGE_LEN)
            .map_err(|e| e.0)?
            .iter()
            .map(|r| Ok((r.u32(0).map_err(|e| e.0)?, r.u32(0xc).map_err(|e| e.0)?)))
            .collect::<Result<_, String>>()?;
        Ok(RiftData {
            nora,
            info: RomPawnInfo::parse(&read(r"etc\editPattern\romPawnInfo", "rRomPawnInfo")?)?,
            names: PawnNames::parse(&read(r"etc\editPattern\romPawnNameAlphabet", "rRomPawnName")?)?,
            skills: SkillList::parse(&read(r"etc\skill\skill", "rSkillList")?)?,
            levels,
            stamina: ddparam::Stamina::from_bytes(&read(r"param\pl\stamina\PlStaminaMax", "rPlStamina")?)
                .map_err(|e| e.0)?,
            chara: CharaInfo::parse(&read(r"AI\preset_charainfo", "rAIPresetCharaInfo")?)?,
            study: Study::parse(&read(r"AI\preset_study", "rAIPresetStudy")?)?,
            items: ItemSlots::parse(&read(r"etc\item\itemList", "rItemList")?)?,
            masks: SkillMasks::from_image(&image)?,
            keys: ItemKeyTable::from_image(&image)?,
            quest_knowledge,
            npc_names,
            language,
            item_key_seed: 0,
        })
    }

    pub fn builder(&self) -> BuilderData<'_> {
        BuilderData {
            skills: &self.skills,
            levels: [&self.levels[0], &self.levels[1], &self.levels[2], &self.levels[3], &self.levels[4], &self.levels[5]],
            stamina: &self.stamina,
            chara: &self.chara,
            study: &self.study,
            items: &self.items,
            masks: &self.masks,
            names: &self.names,
            item_keys: &self.keys,
            item_key_seed: self.item_key_seed,
            language: self.language,
        }
    }

    /// Whether a pawn knows the quest: `0x005d0760` finds the quest's
    /// record (binary search `0x005d06d0`) and tests its bit (when non-zero
    /// and below 0x2840) in the pawn's `mStudyFlag`.
    pub fn knows_quest(&self, study_flag: &[u32], quest: u32) -> bool {
        let Ok(i) = self.quest_knowledge.binary_search_by_key(&quest, |r| r.0) else { return false };
        let bit = self.quest_knowledge[i].1;
        bit != 0 && bit < 0x2840 && study_flag.get((bit >> 5) as usize).is_some_and(|w| w & (1 << (bit & 31)) != 0)
    }
}
