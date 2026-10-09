//! cFSMOrder variables read by script conditions (reflected properties of
//! cFSMOrder, vf03 0x005b8ff0). Each getter cites its address.

use crate::cond::{Value, Var};
use crate::script::Owner;
use crate::{Env, Error};

fn idx(v: &Var) -> Result<usize, Error> {
    v.index
        .map(|i| i as usize)
        .ok_or_else(|| Error::Data(format!("{} read without an index", v.property)))
}

pub fn var(owner: &Owner, env: &Env, v: &Var) -> Result<Value, Error> {
    // Scripts are run by cFSMOrder whatever owner class the file names
    // (0x005081a0 starts every fix script as cFSMOrder); stage scripts
    // also name cFSMOrderOM and cFSMEvent and read properties cFSMOrder
    // has. Binding by property name on the running owner is assumed (the
    // binder was not traced); a property cFSMOrder lacks is an error below.
    let _ = v.owner;
    let w = &*env.world;
    Ok(match v.property {
        // 0x005bad40: bit of mGsfFlag, false past 0x600.
        "GSF" => Value::Bool(w.gsf.get(idx(v)?)),
        // 0x005bada0: FreeFlag bit (the game does not bound it; the data
        // never reads past 31).
        "FreeFlag" => {
            let i = idx(v)?;
            if i >= 32 {
                return Err(Error::Data(format!("FreeFlag[{i}]")));
            }
            Value::Bool(owner.free_flag(i as u32))
        }
        // 0x005bae60: bit of the script's own quest; false without one.
        "MyQuestFlag" => {
            let i = idx(v)?;
            Value::Bool(match owner.quest {
                Some(q) if i < 0x60 && (q >> 16) < 0x80 => w.quest_bit((q >> 16) as usize, i),
                _ => false,
            })
        }
        // 0x007abc70 -> 0x00523110: current stage's AreaHitFlag.
        "AreaHitFlag" => Value::Bool(w.area_hit(env.stage.stage_no, idx(v)?)),
        // 0x005bae00.
        "AreaStayFlag" => Value::Bool(w.area_stay(idx(v)?)),
        "FreeIns" => Value::Int(*owner.free_ins.get(idx(v)?).ok_or_else(|| Error::Data("FreeIns index".into()))? as i64),
        "FreeReal" => Value::Float(*owner.free_real.get(idx(v)?).ok_or_else(|| Error::Data("FreeReal index".into()))?),
        "mCntState" => Value::Float(owner.cnt_state),
        "mCntFSM" => Value::Float(owner.cnt_fsm),
        // 0x005baf00: sGameSys +0x30.
        "SceNo" => Value::Int(w.sce_no as i64),
        // 0x005baf40: aStage +0x724.
        "StageNo" => Value::Int(env.stage.stage_no as i64),
        // 0x005bb080: aStage +0x84 == 1.
        "IsEvent" => Value::Bool(env.stage.event.active),
        // 0x005bb380: sGameSys +0x16dd (fade fully black).
        "IsFadeBlack" => Value::Bool(env.stage.fade_black),
        // 0x005bb0a0: event accepted and actually playing (+0x85).
        // (+0x12a stands for a playing simple event; the extra request-bit
        // condition sGameSys +0xb8844 & 0x4000000 is assumed to hold then.)
        "IsEventStart" => Value::Bool(env.stage.event.active && (env.stage.event.started || env.stage.event.simple)),
        // 0x005c4970: the script's camera slot reached state 3.
        "EndCamera" => Value::Bool(owner.camera.state == 3),
        // 0x005bb100: sGameSys +0x24 (trial build); 0 in the retail game.
        "IsTrial" => Value::Bool(false),
        // 0x005baf70: like StageNo but 0 during some mode requests (not
        // modelled).
        "CurrentStageNo" => Value::Int(env.stage.stage_no as i64),
        // 0x005bb110: whole game hours since the quest's stamp; 0 without
        // a quest.
        "GameHour" => Value::Int(match owner.quest {
            None => 0,
            Some(_) => {
                let (d, ms) = owner.quest_stamp.unwrap_or((0, 0));
                let now = w.day as i64 * 86_400_000 + w.ms as i64;
                let then = d as i64 * 86_400_000 + ms as i64;
                (now - then).max(0) / 3_600_000
            }
        }),
        "NpcFriend" => Value::Int(*w.npc_friend.get(idx(v)?).unwrap_or(&500) as i64),
        "IsNpcDie" => Value::Bool(w.npc_die.get(idx(v)?)),
        "IsNpcUnit" => Value::Bool(env.host.npc_unit(idx(v)? as u32)),
        "IsOmFlag" => Value::Bool(w.om_flag(env.stage.stage_no, idx(v)?)),
        "JobPl" => Value::Int(w.player_job as i64),
        "IsDamagePl" => Value::Bool(env.host.player_damaged()),
        "IsMyCmcLost" => Value::Bool(env.host.pawn_lost()),
        "ValidQuestNum" => Value::Int(env.stage.valid_quests as i64),
        "CaughtNpcId" => Value::Int(w.caught_npc as i64),
        other => return Err(Error::Unimplemented(format!("variable cFSMOrder.{other}"))),
    })
}
