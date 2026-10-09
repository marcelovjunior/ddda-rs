//! Typed view of the processes ("orders") of a quest/stage script.
//!
//! Quest scripts are `rAIFSM` resources whose owner is `cFSMOrder`
//! (e.g. `quest\q0001_b00`). Each state runs processes named by
//! `mContainerName`; `cFSMOrder`'s constructor (0x005b7a60) registers 70
//! names, each with an enter handler, an update handler and a parameter
//! class (registrar 0x00e088b0). This module decodes the parameter records
//! of the processes the opening uses; every other process is kept as
//! [`Order::Other`] with its raw record.
//!
//! Field names are the reflected (MtDTI) names, offsets from the classes'
//! property registration (vtable slot 3).

use mtfsm::{Parameter, Process, Record};
use mtxfs::Value;

/// Where a process writes its completion ("result") flag.
///
/// `cFSMOrder::cFSMOrderParamFlag` (props 0x005bb780: mFlagType +4,
/// mFlagNo +8, mFlagBool +0xc) is the base of most parameter classes.
/// The writer is 0x005cc030: type 0 sets bit `mFlagNo` (< 32) of the
/// script's own FreeFlag bits (cFSMOrder +0x174); type 1 sets bit
/// `mFlagNo` (< 0x600) of the global story flags (GSF); any other type
/// writes nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResultFlag {
    pub kind: u32,
    pub no: u32,
    pub value: bool,
}

impl ResultFlag {
    /// The bank this flag lands in, per 0x005cc030.
    pub fn bank(&self) -> FlagBank {
        match self.kind {
            0 if self.no < 32 => FlagBank::Free(self.no as u8),
            1 if self.no < crate::story::GSF_COUNT as u32 => FlagBank::Gsf(self.no as u16),
            _ => FlagBank::None,
        }
    }
}

/// Destination of a [`ResultFlag`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagBank {
    /// The script's own 32 FreeFlag bits (reset when the script restarts:
    /// cFSMOrder vf05 0x005bacd0).
    Free(u8),
    /// A global story flag (saved).
    Gsf(u16),
    /// Out of range or an unknown type: the game writes nothing.
    None,
}

/// One decoded process.
#[derive(Debug, Clone, PartialEq)]
pub enum Order {
    /// "Flag" (enter 0x005bc280, update 0x005bc2b0): writes its flag once on
    /// entering the state when `set_once`, otherwise on every update.
    SetFlag { flag: ResultFlag, set_once: bool },
    /// "Event": request a cutscene (enter 0x005bcb50 -> update 0x005bcb70).
    Event {
        stage: i32,
        event: i32,
        /// When `Some`, the event sets the story scenario number (SceNo).
        sce_no: Option<u32>,
        set_pos_type: u8,
        start_pos_no: u32,
        set_pos: bool,
        set_angle: bool,
        /// 0x005bcb70 writes `!played.value` when the request is issued and
        /// `played.value` once the stage reports the event over.
        played: ResultFlag,
    },
    /// "SceNo": raise the scenario number to mS32 (enter 0x005bc960 calls
    /// the setter 0x0043d750 with raise-only set).
    SceNo(i32),
    /// "QuestFlag" (enter 0x005c5530, update 0x005c55d0).
    QuestFlag {
        act: u8,
        quest: u16,
        bit: u16,
        result: ResultFlag,
    },
    /// "SetLayout" (enter 0x005c5080): turn a stage layout flag on or off.
    SetLayout { act: u16, stage: u16, flag: u16 },
    /// "CheckLayout" (update 0x005c52a0).
    CheckLayout(Record),
    /// "BootQuest" (enter 0x005c5a00): start another quest script.
    BootQuest { act: u8, quest: u16, branch: u16 },
    /// "Message" (enter 0x005c5bc0, update 0x005c5d40).
    Message {
        kind: u8,
        quest: u16,
        mes_id: u16,
        disp_sec: f32,
        margin_sec: f32,
        cancel_in_cam: bool,
        done: ResultFlag,
    },
    /// "AreaHit" (enter 0x005c2ce0, update 0x005c2d70): watch a stage
    /// scenario area (`task_type`, `no`).
    AreaHit {
        act: u32,
        task_type: Option<u32>,
        no: Option<u32>,
        target_check_type: u32,
        npc_id: i32,
        result_num: u32,
        result: ResultFlag,
    },
    /// "JumpArea" (enter 0x005bc700): move the party to another stage
    /// (0x00517ca0) and, when `sce_no` is `Some`, raise SceNo to it.
    JumpArea {
        fade_type: u32,
        stage: u32,
        pos_no: u32,
        sce_no: Option<u32>,
    },
    /// "Fade" (update 0x005c9a40); may set SceNo, the start position and
    /// the clock while the screen is black.
    Fade {
        fade_type: u32,
        fade_time: f32,
        sce_no: Option<u32>,
        start_pos_no: u32,
        time: Option<(u8, u8)>,
        done: ResultFlag,
    },
    /// "TimeJump" (enter 0x005c2890): set the clock to `hour:minute`.
    TimeJump {
        hour: u8,
        minute: u8,
        start_pos_no: u32,
        set_pos_type: u8,
        done: ResultFlag,
    },
    /// "Camera" (enter 0x005c3750, update 0x005c37c0): a scripted camera
    /// or "simple event"; 0x005c4790 raises SceNo to `sce_no` when it ends
    /// and SceNo is lower.
    Camera { act: u8, cam_id: Option<u32>, sce_no: u32, simple_ev: bool, quest: i32 },
    /// "Finish": the quest script's end state.
    Finish,
    /// Any other process, undecoded.
    Other { container: String, parameter: Option<Record> },
}

/// Field lookup on a reflected record.
pub trait Fields {
    fn value(&self, name: &str) -> Option<&Value>;
    fn object(&self, name: &str) -> Option<&Record>;
    fn u(&self, name: &str) -> Option<u64> {
        match self.value(name)? {
            Value::U(v) => Some(*v),
            Value::S(v) => u64::try_from(*v).ok(),
            _ => None,
        }
    }
    fn s(&self, name: &str) -> Option<i64> {
        match self.value(name)? {
            Value::S(v) => Some(*v),
            Value::U(v) => i64::try_from(*v).ok(),
            _ => None,
        }
    }
    fn b(&self, name: &str) -> Option<bool> {
        match self.value(name)? {
            Value::Bool(v) => Some(*v),
            _ => None,
        }
    }
    fn f(&self, name: &str) -> Option<f32> {
        match self.value(name)? {
            Value::F(v) => v.first().map(|&x| x as f32),
            _ => None,
        }
    }
}

impl Fields for Record {
    fn value(&self, name: &str) -> Option<&Value> {
        let (_, vs) = self.fields.iter().find(|(n, _)| n == name)?;
        match vs.first()? {
            Parameter::Scalar(v) => Some(v),
            Parameter::Object(_) => None,
        }
    }
    fn object(&self, name: &str) -> Option<&Record> {
        let (_, vs) = self.fields.iter().find(|(n, _)| n == name)?;
        match vs.first()? {
            Parameter::Object(Some(r)) => Some(r),
            _ => None,
        }
    }
}

fn result_flag(r: &Record) -> ResultFlag {
    ResultFlag {
        kind: r.u("mFlagType").unwrap_or(0) as u32,
        no: r.u("mFlagNo").unwrap_or(0) as u32,
        value: r.b("mFlagBool").unwrap_or(false),
    }
}

impl Order {
    /// Decode one process. Unknown containers, or known ones whose record
    /// lacks a field, come back as [`Order::Other`].
    pub fn decode(p: &Process) -> Order {
        let name = String::from_utf8_lossy(&p.container).into_owned();
        Self::try_decode(&name, p.parameter.as_ref()).unwrap_or(Order::Other {
            container: name,
            parameter: p.parameter.clone(),
        })
    }

    fn try_decode(name: &str, r: Option<&Record>) -> Option<Order> {
        if name == "Finish" {
            return Some(Order::Finish);
        }
        let r = r?;
        Some(match name {
            "Flag" => Order::SetFlag {
                flag: result_flag(r),
                set_once: r.b("mSetOnce")?,
            },
            "Event" => Order::Event {
                stage: r.s("mStageNo")? as i32,
                event: r.s("mEventNo")? as i32,
                sce_no: if r.b("mDoSetSceNo")? {
                    Some(r.u("mSceNo")? as u32)
                } else {
                    None
                },
                set_pos_type: r.u("mSetPosType")? as u8,
                start_pos_no: r.u("mStartPosNo")? as u32,
                set_pos: r.b("mSetPos")?,
                set_angle: r.b("mSetAngle")?,
                played: result_flag(r),
            },
            "SceNo" => Order::SceNo(r.s("mS32")? as i32),
            "QuestFlag" => Order::QuestFlag {
                act: r.u("mActType")? as u8,
                quest: r.u("mQuestNo")? as u16,
                bit: r.u("mBitNo")? as u16,
                result: result_flag(r),
            },
            "SetLayout" => Order::SetLayout {
                act: r.u("mActType")? as u16,
                stage: r.u("mStageNo")? as u16,
                flag: r.u("mFlagNo")? as u16,
            },
            "CheckLayout" => Order::CheckLayout(r.clone()),
            "JumpArea" => Order::JumpArea {
                fade_type: r.u("mFadeType")? as u32,
                stage: r.u("mJumpStageNo")? as u32,
                pos_no: r.u("mJumpStagePosNo")? as u32,
                sce_no: if r.b("mDoSetSceNo")? { Some(r.u("mSceNo")? as u32) } else { None },
            },
            "Fade" => Order::Fade {
                fade_type: r.u("mFadeType")? as u32,
                fade_time: r.f("mFadeTime")?,
                sce_no: if r.b("mDoSetSceNo")? { Some(r.u("mSceNo")? as u32) } else { None },
                start_pos_no: r.u("mStartPosNo")? as u32,
                time: if r.b("mDoSetTime")? {
                    Some((r.u("mHour")? as u8, r.u("mMinute")? as u8))
                } else {
                    None
                },
                done: result_flag(r),
            },
            "TimeJump" => Order::TimeJump {
                hour: r.u("Hour")? as u8,
                minute: r.u("Minute")? as u8,
                start_pos_no: r.u("mStartPosNo")? as u32,
                set_pos_type: r.u("mSetPosType")? as u8,
                done: result_flag(r),
            },
            "Camera" => Order::Camera {
                act: r.u("mActType")? as u8,
                cam_id: r.u("CamId").map(|v| v as u32),
                sce_no: r.u("mSceNo")? as u32,
                simple_ev: r.b("mSimpleEv")?,
                quest: r.s("mQuestNo")? as i32,
            },
            "BootQuest" => Order::BootQuest {
                act: r.u("mActType")? as u8,
                quest: r.u("mQuestNo")? as u16,
                branch: r.u("mBranchNo")? as u16,
            },
            "Message" => Order::Message {
                kind: r.u("mType")? as u8,
                quest: r.u("mQuestNo")? as u16,
                mes_id: r.u("mMesId")? as u16,
                disp_sec: r.f("mDispSec")?,
                margin_sec: r.f("mMarginSec")?,
                cancel_in_cam: r.b("CancelInCam")?,
                done: result_flag(r),
            },
            "AreaHit" => {
                let hit = r.object("AreaHit");
                Order::AreaHit {
                    act: r.u("mAct")? as u32,
                    task_type: hit.and_then(|h| h.u("mTaskType")).map(|v| v as u32),
                    no: hit.and_then(|h| h.u("mNo")).map(|v| v as u32),
                    target_check_type: r.u("mTargetCheckType")? as u32,
                    npc_id: r.s("mNpcID")? as i32,
                    result_num: r.u("mResultNum")? as u32,
                    result: result_flag(r),
                }
            }
            _ => return None,
        })
    }
}

/// A variable a script condition reads (`rAIConditionTree::VariableNode`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct VariableRef {
    pub owner: String,
    pub property: String,
    /// The constant array index (`mIndex`) when `mIsArray` is set and the
    /// index is not dynamic.
    pub index: Option<u32>,
}

/// Every variable read in an expression tree, in tree order.
pub fn variables(e: &mtfsm::Expression, out: &mut Vec<VariableRef>) {
    if e.record.class_name == Some("rAIConditionTree::VariableNode") {
        if let Some(info) = e.record.object("mVariable") {
            let text = |n: &str| match info.value(n) {
                Some(Value::Str(s)) => String::from_utf8_lossy(s).into_owned(),
                _ => String::new(),
            };
            let is_array = e.record.b("mIsArray").unwrap_or(false);
            let dynamic = e.record.b("mIsDynamicIndex").unwrap_or(false);
            out.push(VariableRef {
                owner: text("mOwnerName"),
                property: text("mPropertyName"),
                index: if is_array && !dynamic {
                    e.record.u("mIndex").map(|v| v as u32)
                } else {
                    None
                },
            });
        }
    }
    for c in e.children.iter().flatten() {
        variables(c, out);
    }
}

/// Readable text of a condition expression, for documentation and dumps.
///
/// Operator codes follow docs/AI.md (switch 0x0117bd30): 0 always, 1 is
/// set, 2 not, 3 ==, 4 !=, 5 <, 6 <=, 7 >, 8 >=, 9 all bits, 10 any bit,
/// 16 and, 17 or. Unknown codes print as `op<N>(..)`.
pub fn expr_text(e: &mtfsm::Expression) -> String {
    let rec = &e.record;
    match rec.class_name {
        Some("rAIConditionTree::VariableNode") => {
            let info = rec.object("mVariable");
            let name = info
                .and_then(|i| match i.value("mPropertyName") {
                    Some(Value::Str(s)) => Some(String::from_utf8_lossy(s).into_owned()),
                    _ => None,
                })
                .unwrap_or_else(|| "?".into());
            if rec.b("mIsArray").unwrap_or(false) {
                if rec.b("mIsDynamicIndex").unwrap_or(false) {
                    format!("{name}[dyn]")
                } else {
                    format!("{name}[{}]", rec.u("mIndex").unwrap_or(0))
                }
            } else {
                name
            }
        }
        Some("rAIConditionTree::ConstS32Node") => rec.s("mValue").map(|v| v.to_string()).unwrap_or("?".into()),
        Some("rAIConditionTree::ConstF32Node") => rec.f("mValue").map(|v| format!("{v:?}")).unwrap_or("?".into()),
        _ => {
            let kids: Vec<String> = e
                .children
                .iter()
                .map(|c| c.as_ref().map(expr_text).unwrap_or_else(|| "null".into()))
                .collect();
            let op = e.operator.unwrap_or(u32::MAX);
            let bin = |s: &str| {
                if kids.len() == 2 {
                    format!("({} {s} {})", kids[0], kids[1])
                } else {
                    format!("{s}({})", kids.join(", "))
                }
            };
            match op {
                // No operands: true whatever the operator (0x0117c040,
                // docs/AI.md). With operands, 0 and 11..15 are false.
                _ if kids.is_empty() => "always".into(),
                1 if kids.len() == 1 => kids[0].clone(),
                2 if kids.len() == 1 => format!("!{}", kids[0]),
                3 => bin("=="),
                4 => bin("!="),
                5 => bin("<"),
                6 => bin("<="),
                7 => bin(">"),
                8 => bin(">="),
                9 => bin("all-bits"),
                10 => bin("any-bit"),
                16 => bin("&&"),
                17 => bin("||"),
                _ => format!("op{op}({})", kids.join(", ")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(class: &'static str, fields: &[(&str, Value)]) -> Record {
        Record {
            class_hash: 0,
            class_name: Some(class),
            fields: fields
                .iter()
                .map(|(n, v)| (n.to_string(), vec![Parameter::Scalar(v.clone())]))
                .collect(),
        }
    }

    #[test]
    fn result_flag_banks_follow_0x005cc030() {
        let f = |kind, no| ResultFlag { kind, no, value: true }.bank();
        assert_eq!(f(0, 31), FlagBank::Free(31));
        assert_eq!(f(0, 32), FlagBank::None);
        assert_eq!(f(1, 0x5ff), FlagBank::Gsf(0x5ff));
        assert_eq!(f(1, 0x600), FlagBank::None);
        assert_eq!(f(100, 0), FlagBank::None);
    }

    #[test]
    fn decodes_event_order() {
        let r = rec(
            "cFSMOrder::cFSMOrderParamEvent",
            &[
                ("mFlagType", Value::U(0)),
                ("mFlagNo", Value::U(2)),
                ("mFlagBool", Value::Bool(true)),
                ("mStageNo", Value::S(200)),
                ("mEventNo", Value::S(10)),
                ("mDoSetSceNo", Value::Bool(true)),
                ("mSceNo", Value::U(1200)),
                ("mSetPosType", Value::U(2)),
                ("mStartPosNo", Value::U(12)),
                ("mSetPos", Value::Bool(true)),
                ("mSetAngle", Value::Bool(true)),
            ],
        );
        let p = Process {
            container: b"Event".to_vec(),
            category: b"cAIFSMProcessContainer".to_vec(),
            parameter: Some(r),
        };
        match Order::decode(&p) {
            Order::Event { stage, event, sce_no, played, .. } => {
                assert_eq!((stage, event, sce_no), (200, 10, Some(1200)));
                assert_eq!(played.bank(), FlagBank::Free(2));
            }
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn unknown_process_is_kept() {
        let p = Process {
            container: b"Weather".to_vec(),
            category: b"cAIFSMProcessContainer".to_vec(),
            parameter: None,
        };
        assert!(matches!(Order::decode(&p), Order::Other { container, .. } if container == "Weather"));
    }
}
