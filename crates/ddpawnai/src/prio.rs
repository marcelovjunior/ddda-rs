//! `rAIPriorityThink`: the pawn's attention table (`AI\PrioThink\cmc`).
//!
//! The root holds `タイプ` and one property per group and level,
//! `"<group> - NN"` with NN = 00..07, each an array of [`PrioParam`].

use crate::{class, float, int, list, number};

/// `cIsPersonal`: an inclination test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsPersonal {
    /// `チェックする性格`: the inclination index (`mInfo[k]`).
    pub personality: i64,
    /// `チェックする状態`: the inclination state to match.
    pub state: i64,
}

/// `cPersonalValue`: an adjustment applied when the pawn's inclinations
/// match.
#[derive(Debug, Clone, PartialEq)]
pub struct PersonalValue {
    /// `加算値S32`.
    pub add_s32: i64,
    /// `加算値F32`.
    pub add_f32: f32,
    /// `適用したら抜ける`: stop checking further values once applied.
    pub stop_after_apply: bool,
    /// `性格リスト`.
    pub checks: Vec<IsPersonal>,
}

/// `rAIPriorityThink::cOrderValue`: a party-command condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderValue {
    /// `設定値`.
    pub value: i64,
    /// `命令タイプ`.
    pub order_type: i64,
}

/// `rAIPriorityThink::cPrioParam`.
#[derive(Debug, Clone, PartialEq)]
pub struct PrioParam {
    /// `センサー`.
    pub sensor: i64,
    /// `コード`.
    pub code: i64,
    /// `カテゴリ`.
    pub category: i64,
    pub object_id: i64,
    /// `付加情報`.
    pub extra: i64,
    /// `性格`.
    pub personal: Vec<PersonalValue>,
    /// `命令`.
    pub orders: Vec<OrderValue>,
}

/// `rAIPriorityThink::cCodeParam`: the entry of type-2 tables (NPCs and
/// human enemies), kept for completeness; pawns use [`PrioParam`].
#[derive(Debug, Clone, PartialEq)]
pub struct CodeParam {
    /// `コード`.
    pub code: i64,
    /// `ランク小`, `ランク大`.
    pub rank: (i64, i64),
    /// `確率` (stored as integer or float depending on the file).
    pub probability: f64,
    /// `目的`.
    pub purpose: i64,
    /// `設定`.
    pub setting: i64,
    pub flag_type: i64,
    pub quest_no: i64,
    pub flag_no: i64,
    pub mess_tbl: i64,
    pub mess_id: i64,
    pub code_free_s32: [i64; 4],
    pub code_free_f32: [f32; 4],
    pub goal_free_s32: [i64; 4],
    pub goal_free_f32: [f32; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// The property name, e.g. `Enemy - 02`.
    pub name: String,
    /// The part before ` - `, e.g. `Enemy`.
    pub kind: String,
    /// The level NN.
    pub level: u32,
    pub params: Vec<PrioParam>,
    pub codes: Vec<CodeParam>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PriorityThink {
    /// `タイプ`.
    pub kind: i64,
    /// In file order.
    pub groups: Vec<Group>,
}

impl PriorityThink {
    pub fn parse(bytes: &[u8]) -> Result<PriorityThink, String> {
        let x = mtxfs::parse(bytes).map_err(|e| format!("{e:?}"))?;
        let r = &x.root;
        if class(&x, r) != "rAIPriorityThink" {
            return Err(format!("root is {}", class(&x, r)));
        }
        let def = x.class_of(r);
        let mut groups = Vec::new();
        for p in &def.properties {
            let Some((kind, level)) = p.name.split_once(" - ") else { continue };
            let level: u32 = level.parse().map_err(|_| format!("bad group name {}", p.name))?;
            let mut params = Vec::new();
            let mut codes = Vec::new();
            for o in list(&x, r, &p.name)? {
                if class(&x, o) == "rAIPriorityThink::cCodeParam" {
                    let a4 = |n: &str| -> Result<[i64; 4], String> {
                        Ok([
                            int(&x, o, &format!("{n}[ 0 ]"))?,
                            int(&x, o, &format!("{n}[ 1 ]"))?,
                            int(&x, o, &format!("{n}[ 2 ]"))?,
                            int(&x, o, &format!("{n}[ 3 ]"))?,
                        ])
                    };
                    let f4 = |n: &str| -> Result<[f32; 4], String> {
                        Ok([
                            float(&x, o, &format!("{n}[ 0 ]"))?,
                            float(&x, o, &format!("{n}[ 1 ]"))?,
                            float(&x, o, &format!("{n}[ 2 ]"))?,
                            float(&x, o, &format!("{n}[ 3 ]"))?,
                        ])
                    };
                    codes.push(CodeParam {
                        code: int(&x, o, "コード")?,
                        rank: (int(&x, o, "ランク小")?, int(&x, o, "ランク大")?),
                        probability: number(&x, o, "確率")?,
                        purpose: int(&x, o, "目的")?,
                        setting: int(&x, o, "設定")?,
                        flag_type: int(&x, o, "FLAG_TYPE")?,
                        quest_no: int(&x, o, "QUEST_NO")?,
                        flag_no: int(&x, o, "FLAG_NO")?,
                        mess_tbl: int(&x, o, "MessTBL")?,
                        mess_id: int(&x, o, "MessID")?,
                        code_free_s32: a4("mCodeFreeS32")?,
                        code_free_f32: f4("mCodeFreeF32")?,
                        goal_free_s32: a4("mGoalFreeS32")?,
                        goal_free_f32: f4("mGoalFreeF32")?,
                    });
                    continue;
                }
                params.push(PrioParam {
                    sensor: int(&x, o, "センサー")?,
                    code: int(&x, o, "コード")?,
                    category: int(&x, o, "カテゴリ")?,
                    object_id: int(&x, o, "OBJECT_ID")?,
                    extra: int(&x, o, "付加情報")?,
                    personal: list(&x, o, "性格")?
                        .into_iter()
                        .map(|v| {
                            Ok(PersonalValue {
                                add_s32: int(&x, v, "加算値S32")?,
                                add_f32: float(&x, v, "加算値F32")?,
                                stop_after_apply: int(&x, v, "適用したら抜ける")? != 0,
                                checks: list(&x, v, "性格リスト")?
                                    .into_iter()
                                    .map(|c| {
                                        Ok(IsPersonal {
                                            personality: int(&x, c, "チェックする性格")?,
                                            state: int(&x, c, "チェックする状態")?,
                                        })
                                    })
                                    .collect::<Result<_, String>>()?,
                            })
                        })
                        .collect::<Result<_, String>>()?,
                    orders: list(&x, o, "命令")?
                        .into_iter()
                        .map(|v| Ok(OrderValue { value: int(&x, v, "設定値")?, order_type: int(&x, v, "命令タイプ")? }))
                        .collect::<Result<_, String>>()?,
                });
            }
            groups.push(Group { name: p.name.clone(), kind: kind.to_string(), level, params, codes });
        }
        Ok(PriorityThink { kind: int(&x, r, "タイプ")?, groups })
    }

    /// The group for a kind and level.
    pub fn group(&self, kind: &str, level: u32) -> Option<&Group> {
        self.groups.iter().find(|g| g.kind == kind && g.level == level)
    }
}
