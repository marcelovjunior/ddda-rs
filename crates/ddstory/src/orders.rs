//! cFSMOrder orders: "update" (once, on entering the state) and "state"
//! (every frame) callbacks, registered by cFSMOrder's constructor
//! 0x005b7a60. Each handler cites the routine it translates. Orders the
//! opening does not use return [`Error::Unimplemented`].
//!
//! `R(v)` below is the result-flag write 0x005cc030 with the record's
//! (mFlagType, mFlagNo) and `v` = mFlagBool or its negation.

use crate::data::Data;
use crate::script::{Dispatch, OrderCtx};
use crate::story::EventRequest;
use crate::{Error, Request};
use ddquest::order::Fields;
use mtfsm::{Process, Record};

pub struct Orders;

fn rec(p: &Process) -> Result<&Record, Error> {
    p.parameter.as_ref().ok_or_else(|| Error::Data(format!("{} without parameters", name(p))))
}

fn name(p: &Process) -> String {
    String::from_utf8_lossy(&p.container).into_owned()
}

fn u(r: &Record, f: &str) -> Result<u64, Error> {
    r.u(f).ok_or_else(|| Error::Data(format!("{:?}.{f} missing", r.class_name)))
}
fn s(r: &Record, f: &str) -> Result<i64, Error> {
    r.s(f).ok_or_else(|| Error::Data(format!("{:?}.{f} missing", r.class_name)))
}
fn b(r: &Record, f: &str) -> Result<bool, Error> {
    r.b(f).ok_or_else(|| Error::Data(format!("{:?}.{f} missing", r.class_name)))
}
fn f(r: &Record, n: &str) -> Result<f32, Error> {
    r.f(n).ok_or_else(|| Error::Data(format!("{:?}.{n} missing", r.class_name)))
}

/// cFSMOrderParamFlag base: (mFlagType, mFlagNo, mFlagBool).
fn result_flag(r: &Record) -> Result<(u32, u32, bool), Error> {
    Ok((u(r, "mFlagType")? as u32, u(r, "mFlagNo")? as u32, b(r, "mFlagBool")?))
}

/// R(v ? mFlagBool : !mFlagBool).
fn write(ctx: &mut OrderCtx, r: &Record, positive: bool) -> Result<(), Error> {
    let (k, n, v) = result_flag(r)?;
    ctx.owner.write_result(ctx.env.world, k, n, if positive { v } else { !v });
    Ok(())
}

fn dt(ctx: &OrderCtx) -> f32 {
    ctx.env.time.delta.max(0.01) / ctx.env.time.fps
}

/// Hand a presentation order to the host unchanged.
fn pass(ctx: &mut OrderCtx, p: &Process) {
    ctx.env.out.push(Request::Order { script: ctx.script.to_string(), container: name(p), parameter: p.parameter.clone() });
}

fn unimplemented(ctx: &OrderCtx, p: &Process, what: &str) -> Error {
    Error::Unimplemented(format!(
        "order {} ({what}) in state {:?}",
        name(p),
        String::from_utf8_lossy(ctx.state_name)
    ))
}

impl Dispatch for Orders {
    fn update(ctx: &mut OrderCtx, p: &Process) -> Result<(), Error> {
        match name(p).as_str() {
            // Translated from 0x005bc280: write once when mSetOnce.
            "Flag" => {
                let r = rec(p)?;
                if b(r, "mSetOnce")? {
                    write(ctx, r, true)?;
                }
            }
            // Translated from 0x005bc960 -> 0x0043d750(mS32, raise-only).
            // (The stage-601 special case at 0x005bc982 does not apply to
            // the opening and is not translated.)
            "SceNo" => {
                let v = s(rec(p)?, "mS32")? as u32;
                if ctx.env.stage.stage_no == 601 {
                    return Err(unimplemented(ctx, p, "stage 601 case 0x005bc982"));
                }
                ctx.env.world.set_sce_no(v, true);
            }
            // 0x005bcb50 runs the state handler once.
            "Event" => event(ctx, rec(p)?)?,
            // Translated from 0x005bbeb0.
            "FreeIns" => {
                let r = rec(p)?;
                let t = u(r, "mSetTarget")? as usize;
                let x = s(r, "X")? as i32;
                if t < 32 {
                    match u(r, "mSetType")? {
                        0 => ctx.owner.free_ins[t] = x,
                        1 => ctx.owner.free_ins[t] = ctx.owner.free_ins[t].wrapping_add(x),
                        _ => {}
                    }
                }
            }
            // Translated from 0x005bc410: (u8)mFreeRealNo - 0x10 picks the
            // register (10000 -> 0).
            "SetTimer" => {
                let r = rec(p)?;
                let i = (u(r, "mFreeRealNo")? as u8).wrapping_sub(0x10) as usize;
                if i < 32 {
                    match u(r, "mSetType")? {
                        0 => ctx.owner.timer_index = i as i8,
                        1 => ctx.owner.timer_index = -1,
                        2 => ctx.owner.free_real[i] = 0.0,
                        _ => {}
                    }
                }
            }
            // Translated from 0x005bc590: stamp the quest's GameHour origin.
            "SetGameHour" => {
                if ctx.owner.quest.is_some() {
                    let w = &*ctx.env.world;
                    ctx.owner.quest_stamp = match u(rec(p)?, "mSetType")? {
                        0 | 2 => Some((w.day, w.ms)),
                        _ => None,
                    };
                }
            }
            // Translated from 0x005c2890 -> 0x0043f070(hour, minute, advance).
            "TimeJump" => {
                let r = rec(p)?;
                let (h, m) = (u(r, "Hour")? as u32, u(r, "Minute")? as u32);
                ctx.env.world.set_clock(h, m);
                if u(r, "mSetPosType")? != 0 {
                    let pos = u(r, "mStartPosNo")? as u32;
                    ctx.env.world.start_pos = pos;
                    ctx.env.out.push(Request::PlaceParty { start_pos: Some(pos), pos: None });
                }
                ctx.env.out.push(Request::SetClock { hour: h, minute: m });
            }
            // Translated from 0x005c5080 (lot flag set 0x00443ca0 / clear
            // 0x00443f20).
            "SetLayout" => {
                let r = rec(p)?;
                let (act, stage, n) = (u(r, "mActType")?, u(r, "mStageNo")? as u32, u(r, "mFlagNo")? as usize);
                let w = &mut *ctx.env.world;
                // 0x005c50c8: (1, 380, 5) is ignored while sGameSys +0xb8844
                // & 0x40000 (not modelled: treated as clear).
                match act {
                    0 => {
                        w.set_lot(stage, n, true);
                        // 0x00443ca0: of stage-100 flags 0x9e, 0xa8, 0xaa at
                        // most one stays set.
                        if stage == 100 && [0x9e, 0xa8, 0xaa].contains(&n) {
                            let set = [0x9e, 0xa8, 0xaa].iter().filter(|&&f| w.lot(100, f)).count();
                            if set >= 2 {
                                for f in [0x9e, 0xa8, 0xaa] {
                                    w.set_lot(100, f, false);
                                }
                            }
                        }
                    }
                    1 => w.set_lot(stage, n, false),
                    _ => {}
                }
                ctx.env.out.push(Request::Layout { stage, flag: n as u32, on: act == 0 });
            }
            // Translated from 0x005c5530.
            "QuestFlag" => {
                let r = rec(p)?;
                let (act, q, bit) = (u(r, "mActType")?, u(r, "mQuestNo")? as usize, u(r, "mBitNo")? as usize);
                if q >= 0x80 || bit >= 0x60 {
                    return Err(Error::Data(format!("QuestFlag q{q} bit {bit} out of range (crashes the game)")));
                }
                if ctx.env.world.quest_bit(18, 55) && act == 2 && q == 18 && bit == 6 {
                    write(ctx, r, true)?;
                } else {
                    match act {
                        2 => {
                            ctx.env.world.set_quest_bit(q, bit, true);
                            write(ctx, r, true)?;
                        }
                        3 => {
                            ctx.env.world.set_quest_bit(q, bit, false);
                            write(ctx, r, true)?;
                        }
                        _ => {}
                    }
                }
            }
            // Translated from 0x005c5a00 -> 0x00484000.
            "BootQuest" => {
                let r = rec(p)?;
                if u(r, "mActType")? == 0 {
                    ctx.env.cmds.push(crate::story::Command::BootQuest {
                        quest: u(r, "mQuestNo")? as u32,
                        branch: u(r, "mBranchNo")? as u32,
                        start: false,
                    });
                }
            }
            // Translated from 0x005bc700.
            "JumpArea" => {
                let r = rec(p)?;
                if ctx.env.stage.event.active {
                    return Ok(());
                }
                let (stage, pos) = (u(r, "mJumpStageNo")? as u32, u(r, "mJumpStagePosNo")? as u32);
                ctx.env.world.stage_no = stage;
                ctx.env.world.start_pos = pos;
                ctx.env.out.push(Request::JumpArea { stage, pos, fade: u(r, "mFadeType")? as u32 });
                if b(r, "mDoSetSceNo")? {
                    ctx.env.world.set_sce_no(u(r, "mSceNo")? as u32, true);
                }
            }
            // Translated from 0x005c5bc0.
            "Message" => {
                let r = rec(p)?;
                write(ctx, r, false)?;
                let kind = u(r, "mType")? as u8;
                match kind {
                    2 | 3 => {
                        ctx.env.stage.message_showing = true;
                        ctx.env.out.push(Request::Message {
                            kind,
                            quest: u(r, "mQuestNo")? as u16,
                            mes_id: u(r, "mMesId")? as u16,
                            disp_sec: f(r, "mDispSec")?,
                        });
                    }
                    0 | 1 => {}
                    _ => return Err(unimplemented(ctx, p, "message type")),
                }
                ctx.mem.time = f(r, "mMarginSec")?;
            }
            // Translated from 0x005c6340.
            "TutorialMessage" => {
                let r = rec(p)?;
                write(ctx, r, false)?;
                if !ctx.env.host.tutorials_enabled() {
                    write(ctx, r, true)?;
                }
                ctx.mem.time = f(r, "mMarginSec")?;
                ctx.owner.event_step = 0;
            }
            // Translated from 0x005cb860.
            "PlaceName" => {
                let r = rec(p)?;
                ctx.env.out.push(Request::PlaceName { mes_id: u(r, "mMesId")? as u32 });
                let g = s(r, "mGsfNo")?;
                if (0..0x600).contains(&g) {
                    ctx.env.world.gsf.set(g as usize, true);
                }
            }
            // Translated from 0x005c7f90.
            "Weather" => {
                let act = u(rec(p)?, "mActType")?;
                if act <= 4 {
                    ctx.env.out.push(Request::Weather { kind: act as u32 });
                }
            }
            // Translated from 0x005c6130: a pawn line; host performs it.
            "CmcMessage" => pass(ctx, p),
            // Translated from 0x005c3750 (Camera update): mActType 1 ends
            // the camera, 2 restores the normal camera.
            "Camera" => {
                let r = rec(p)?;
                match u(r, "mActType")? {
                    0 => {}
                    1 => {
                        let c = &mut ctx.owner.camera;
                        c.sce_end = u(r, "mSceNo")? as u32;
                        c.set_pos_type = u(r, "mSetPosType")? as u32;
                        c.start_pos = u(r, "mStartPosNo")? as u32;
                        if c.state != 0 {
                            camera_end(ctx.owner, ctx.env);
                        }
                    }
                    2 => pass(ctx, p),
                    _ => return Err(unimplemented(ctx, p, "mActType")),
                }
            }
            // Translated from 0x005c2ce0 (AreaHit update).
            "AreaHit" => {
                let r = rec(p)?;
                let (task, no) = area_hit_key(r)?;
                let act = u(r, "mAct")?;
                let sc = &mut ctx.env.stage.scenarios;
                if let Some(status) = sc.entry(task, no).map(|e| e.status) {
                    match act {
                        0 => {
                            if status & 4 == 0 {
                                sc.end(task as usize, no as usize)?;
                            }
                            sc.entry_mut(task, no).unwrap().status |= 4;
                        }
                        1 => sc.entry_mut(task, no).unwrap().status &= !4,
                        _ => {}
                    }
                }
                write(ctx, r, false)?;
            }
            // State-only orders and Finish/Kill have no update.
            "Finish" | "Kill" | "ClockCheck" | "CheckLayout" | "CheckHp" | "PlCheck" => {}
            other => return actor_update(ctx, p, other),
        }
        Ok(())
    }

    fn state(ctx: &mut OrderCtx, p: &Process) -> Result<(), Error> {
        match name(p).as_str() {
            // Translated from 0x005bc2b0: write every frame unless mSetOnce.
            "Flag" => {
                let r = rec(p)?;
                if !b(r, "mSetOnce")? {
                    write(ctx, r, true)?;
                }
            }
            "Event" => event(ctx, rec(p)?)?,
            // Translated from 0x005c55d0.
            "QuestFlag" => {
                let r = rec(p)?;
                let (act, q, bit) = (u(r, "mActType")?, u(r, "mQuestNo")? as usize, u(r, "mBitNo")? as usize);
                let set = ctx.env.world.quest_bit(q, bit);
                match act {
                    0 => write(ctx, r, set)?,
                    1 => write(ctx, r, !set)?,
                    _ => {}
                }
            }
            // Translated from 0x005c52a0.
            "CheckLayout" => {
                let r = rec(p)?;
                let (stage, bit) = (u(r, "mStageNo")? as u32, u(r, "mBitNo")? as usize);
                if crate::world::stage_index(stage).is_some() {
                    let set = ctx.env.world.lot(stage, bit);
                    match u(r, "mActType")? {
                        0 => write(ctx, r, set)?,
                        1 => write(ctx, r, !set)?,
                        _ => {}
                    }
                }
            }
            // Translated from 0x005c2440 (ms of day; wrap past midnight).
            "ClockCheck" => {
                let r = rec(p)?;
                let ms = |h: u64, m: u64| ((m + 60 * h) * 60_000) as i64;
                let mut t = ctx.env.world.ms as i64;
                let t0 = ms(u(r, "Hour0")?, u(r, "Minute0")?);
                let mut t1 = ms(u(r, "Hour1")?, u(r, "Minute1")?);
                if t0 > t {
                    t += 86_400_000;
                }
                if t0 > t1 {
                    t1 += 86_400_000;
                }
                match u(r, "mActType")? {
                    0 => write(ctx, r, t0 <= t && t <= t1)?,
                    1 => write(ctx, r, t <= t0)?,
                    2 => write(ctx, r, true)?,
                    _ => {}
                }
            }
            // Translated from 0x005c5d40.
            "Message" => {
                let r = rec(p)?;
                let kind = u(r, "mType")?;
                let done = kind < 2 || !ctx.env.stage.message_showing;
                if ctx.owner.event_step == 0 {
                    ctx.owner.event_step = 1;
                    write(ctx, r, false)?;
                } else if done {
                    if ctx.mem.time <= 0.0 {
                        write(ctx, r, true)?;
                    } else {
                        ctx.mem.time -= dt(ctx);
                        write(ctx, r, false)?;
                    }
                } else {
                    write(ctx, r, false)?;
                }
            }
            "TutorialMessage" => tutorial_state(ctx, p)?,
            // Translated from 0x005c37c0 (Camera state): start once per state
            // entry (guarded by owner +0x6c); a simple event waits until the
            // stage is free (0x00516e80).
            "Camera" => {
                let r = rec(p)?;
                if u(r, "mActType")? == 0 && ctx.owner.event_step == 0 {
                    let simple = b(r, "mSimpleEv")?;
                    let st = &*ctx.env.stage;
                    if simple && (st.event.active || st.event.pending.is_some()) {
                        return Ok(());
                    }
                    let cam_id = r.u("CamId").unwrap_or(0) as u32;
                    ctx.owner.camera = crate::script::CameraSlot {
                        cam_id,
                        sce_end: u(r, "mSceNo")? as u32,
                        set_pos_type: u(r, "mSetPosType")? as u32,
                        start_pos: u(r, "mStartPosNo")? as u32,
                        state: 1,
                        simple,
                    };
                    ctx.env.stage.camera_running = true;
                    if simple {
                        // 0x00517050(1, ...): stage mode 4 sets +0x84 at
                        // once and +0x12a when loaded (0x0050dcd0).
                        ctx.env.stage.event.active = true;
                        ctx.env.stage.event.simple = true;
                    }
                    ctx.env.out.push(Request::Camera { cam_id, simple, quest: s(r, "mQuestNo")? as i32 });
                    ctx.owner.event_step = 1;
                }
            }
            // Translated from 0x005c2d70 (AreaHit state).
            "AreaHit" => {
                let r = rec(p)?;
                let (task, no) = area_hit_key(r)?;
                let Some(e) = ctx.env.stage.scenarios.entry(task, no) else {
                    return write(ctx, r, false);
                };
                let (status, hit) = (e.status, e.hit_now());
                let shape = e.shape.clone();
                match u(r, "mAct")? {
                    0 | 1 => {}
                    2 => write(ctx, r, status & 1 != 0)?,
                    3 => write(ctx, r, hit)?,
                    4 => write(ctx, r, status & 8 != 0)?,
                    6 => {
                        if hit {
                            let want = s(r, "mNpcID")?;
                            let shape = shape.ok_or_else(|| Error::Data("AreaHit 6 without shape".into()))?;
                            let mut n = 0;
                            for npc in &ctx.env.stage.npcs {
                                if (want < 0 || npc.id as i64 == want) && shape.hit(npc.unit.pos, npc.unit.yaw) {
                                    n += 1;
                                    if want >= 0 {
                                        break;
                                    }
                                }
                            }
                            let i = u(r, "mResultNum")? as usize;
                            if i < 32 {
                                ctx.owner.free_ins[i] = n;
                            }
                            write(ctx, r, n > 0)?;
                        }
                    }
                    _ => return Err(unimplemented(ctx, p, "mAct 5/7/8")),
                }
            }
            // Translated from 0x005c0ca0; the per-kind counts are host
            // queries (player 0 only, as in the game).
            "PlCheck" => {
                let r = rec(p)?;
                let kind = u(r, "mCheckFlag")? as u32;
                if !(2..=10).contains(&kind) {
                    return Err(unimplemented(ctx, p, "PlCheck kinds 0/1 (skill/target lists)"));
                }
                let n = ctx.env.host.pl_check(kind);
                let total = 1;
                match u(r, "mCheckType")? {
                    0 => write(ctx, r, total != 0 && n >= total)?,
                    1 => write(ctx, r, n != 0)?,
                    2 => write(ctx, r, total != 0)?,
                    _ => {}
                }
                let i = u(r, "mResultNum")? as usize;
                if i < 32 {
                    ctx.owner.free_ins[i] = n;
                }
            }
            // Translated from 0x007497e0: SysCondition |= 1 (the script stops
            // at the end of the frame, cFSMOrder vf06 0x005babb0).
            "Finish" => ctx.owner.sys_condition |= 1,
            // 0x00749800: SysCondition |= 0x10 (no reader found).
            "Kill" => ctx.owner.sys_condition |= 0x10,
            // Orders with no state callback.
            "SceNo" | "FreeIns" | "SetTimer" | "SetGameHour" | "TimeJump" | "SetLayout" | "BootQuest"
            | "JumpArea" | "PlaceName" | "Weather" | "CmcMessage" => {}
            other => return actor_state(ctx, p, other),
        }
        Ok(())
    }
}

/// The Event order's state handler 0x005bcb70 (its update 0x005bcb50 calls
/// it once). Step counter: owner +0x6c.
// Translated from 0x005bcb70 (DDDA.exe 2017-12-13 Steam).
fn event(ctx: &mut OrderCtx, r: &Record) -> Result<(), Error> {
    match ctx.owner.event_step {
        0 => {
            if ctx.env.stage.event.active {
                return Ok(());
            }
            let table = ctx
                .env
                .events
                .ok_or_else(|| Error::Data("Event order needs the exe's event table (DDDA.exe)".into()))?;
            let (stage, ev) = (s(r, "mStageNo")? as u16, s(r, "mEventNo")? as u16);
            // 0x00435a10: the row; a missing row does nothing.
            let Some(row) = table.find(stage, ev) else { return Ok(()) };
            if (7..=18).contains(&row) || (0x1a..=0x1f).contains(&row) {
                return Err(Error::Unimplemented(format!("event variant selection for row {row} (0x00435a10)")));
            }
            let set_pos = b(r, "mSetPos")?;
            let set_pos_type = u(r, "mSetPosType")? as u8;
            if set_pos && set_pos_type == 1 {
                return Err(Error::Unimplemented("event with an explicit position".into()));
            }
            let req = EventRequest {
                row,
                stage: stage as u32,
                event: ev as u32,
                sce_no: if b(r, "mDoSetSceNo")? { Some(u(r, "mSceNo")? as u32) } else { None },
                set_pos_type: if set_pos { set_pos_type } else { 0 },
                start_pos: u(r, "mStartPosNo")? as u32,
            };
            // 0x00516ec0: accepted unless an event is already active (the
            // player-busy test 0x00516e00 is the host's: not modelled).
            let st = &mut *ctx.env.stage;
            if !st.event.active {
                st.event.active = true;
                st.event.pending = Some(req);
            }
            if st.event.active {
                ctx.owner.event_step += 1;
            }
            write(ctx, r, false)?;
        }
        1 if !ctx.env.stage.event.active => {
            write(ctx, r, true)?;
            ctx.owner.event_step += 1;
        }
        _ => {}
    }
    Ok(())
}

/// TutorialMessage state handler 0x005c6430 (window type 0 and panel
/// types). The game's suppression rules beyond "tutorials disabled" and the
/// pause conditions are not modelled.
// Translated from 0x005c6430 (DDDA.exe 2017-12-13 Steam), main path.
fn tutorial_state(ctx: &mut OrderCtx, p: &Process) -> Result<(), Error> {
    let r = rec(p)?;
    let id = u(r, "mMesId")? as u32;
    if !ctx.env.host.tutorials_enabled() || (0x85..=0x89).contains(&id) {
        return write(ctx, r, true);
    }
    match ctx.owner.event_step {
        0 => {
            ctx.env.stage.message_showing = true;
            ctx.env.out.push(Request::Tutorial {
                window: u(r, "mActType")? == 0,
                ids: [id, s(r, "mMesId01")? as u32, s(r, "mMesId02")? as u32, s(r, "mMesId03")? as u32],
                disp_sec: f(r, "mDispSec")?,
            });
            ctx.owner.event_step = 2;
            write(ctx, r, false)
        }
        _ => {
            if ctx.env.stage.message_showing {
                write(ctx, r, false)
            } else if ctx.mem.time > 0.0 {
                ctx.mem.time -= dt(ctx);
                write(ctx, r, false)
            } else {
                write(ctx, r, true)
            }
        }
    }
}

/// Every cLinkUnit::cTarget under a record, in order.
fn targets(r: &Record) -> Vec<crate::Target> {
    fn walk(r: &Record, out: &mut Vec<crate::Target>) {
        if r.class_name == Some("cLinkUnit::cTarget") {
            out.push(crate::Target {
                kind: r.u("mType").unwrap_or(0) as u32,
                group: r.u("mNo0").unwrap_or(0) as u32,
                index: r.u("mNo1").unwrap_or(0) as u32,
            });
            return;
        }
        for (_, vs) in &r.fields {
            for v in vs {
                if let mtfsm::Parameter::Object(Some(o)) = v {
                    walk(o, out);
                }
            }
        }
    }
    let mut out = vec![];
    walk(r, &mut out);
    out
}

fn field_object<'a>(r: &'a Record, n: &str) -> Result<&'a Record, Error> {
    r.object(n).ok_or_else(|| Error::Data(format!("{:?}.{n} missing", r.class_name)))
}

fn actor_update(ctx: &mut OrderCtx, p: &Process, other: &str) -> Result<(), Error> {
    match other {
        // SetObj / SetGroup update 0x005bd6a0: remember the entry stage
        // (param +0x10).
        "EnemySet" | "NpcSet" | "ItemSet" | "OmSet" | "EnemyGroup" | "NpcGroup" | "ItemGroup" | "OmGroup" => {
            ctx.mem.value = ctx.env.stage.stage_no as i64;
            ctx.mem.step = 0;
        }
        // Translated from 0x005c0310: revive NPC mNpcId (0x00446540).
        "NpcAlive" => {
            let id = s(rec(p)?, "mNpcId")?;
            if (0..900).contains(&id) {
                ctx.env.world.revive_npc(id as usize);
                ctx.env.out.push(Request::NpcLife { npc: id as u32, alive: true });
            }
        }
        // Translated from 0x005c8140.
        "Collision" => {
            let r = rec(p)?;
            ctx.env.out.push(Request::Collision {
                kind: u(r, "mType")? as u32,
                split: (u(r, "mSplitX")? as u32, u(r, "mSplitZ")? as u32),
                part: u(r, "mPartsNo")? as u32,
                on: b(r, "mSetValue")?,
            });
        }
        // Presentation orders the host performs (sound 0x005c6bf0, music
        // 0x005c7a10, camera shake 0x005ca210, shell 0x005cad60).
        "SE Request" | "BattleBGM" | "StartQuake" | "SetShell" => pass(ctx, p),
        // State-only orders.
        "EnemyCheck" | "NpcCheck" | "NpcDieCheck" | "ApplyFSM" => {}
        _ => return Err(unimplemented(ctx, p, "no translation")),
    }
    Ok(())
}

fn actor_state(ctx: &mut OrderCtx, p: &Process, other: &str) -> Result<(), Error> {
    let stage = ctx.env.stage.stage_no;
    match other {
        // Translated from 0x005bd6c0 (SetObj state). The game re-issues the
        // set/remove call every frame; here the request goes out once per
        // state entry and the host keeps it.
        "EnemySet" | "NpcSet" | "ItemSet" | "OmSet" => {
            let r = rec(p)?;
            let act = s(r, "mActType")?;
            let list = targets(r);
            let host = ctx.env.host;
            match act {
                1 | 2 => {
                    if ctx.mem.value != stage as i64 {
                        return Ok(());
                    }
                    for t in &list {
                        if matches!(t.kind, 2 | 4 | 5) && !host.layout_loaded(stage, t) {
                            return write(ctx, r, false);
                        }
                    }
                    let mut all = true;
                    for t in &list {
                        if t.kind == 2 && host.enemy_group_done(stage, t.group) {
                            continue;
                        }
                        if t.kind == 2 && !host.enemy_group_active(stage, t.group) {
                            all = false;
                            continue;
                        }
                        if ctx.mem.step == 0 {
                            ctx.env.out.push(Request::Placement { target: *t, present: true, act: act as u32 });
                        }
                        if act == 2 && t.kind == 5 {
                            if let Some(i) = host.npc_intel(stage, t) {
                                ctx.env.world.revive_npc(i.id as usize);
                            }
                        }
                        all &= host.unit_exists(stage, t);
                    }
                    ctx.mem.step = 1;
                    write(ctx, r, all)
                }
                3 | 4 => {
                    if ctx.mem.value != stage as i64 {
                        return write(ctx, r, true);
                    }
                    let mut all = true;
                    for t in &list {
                        if ctx.mem.step == 0 {
                            ctx.env.out.push(Request::Placement { target: *t, present: false, act: act as u32 });
                        }
                        if act == 3 && t.kind == 5 {
                            if let Some(i) = host.npc_intel(stage, t) {
                                ctx.env.world.kill_npc(i.id as usize);
                            }
                        }
                        all &= !host.unit_exists(stage, t);
                    }
                    ctx.mem.step = 1;
                    write(ctx, r, all)
                }
                _ => Err(unimplemented(ctx, p, "mActType")),
            }
        }
        // Translated from 0x005bdec0 (SetGroup state).
        "EnemyGroup" | "NpcGroup" | "ItemGroup" | "OmGroup" => {
            let r = rec(p)?;
            let kind = match other {
                "EnemyGroup" => 2,
                "NpcGroup" => 5,
                _ => 4,
            };
            let group = u(r, "mGroupNo")? as u32;
            let host = ctx.env.host;
            match u(r, "mActType")? {
                0 => {
                    if ctx.mem.value != stage as i64 {
                        return Ok(());
                    }
                    if !host.group_loaded(stage, kind, group) {
                        return write(ctx, r, false);
                    }
                    if kind == 2 {
                        if host.enemy_group_done(stage, group) {
                            return write(ctx, r, true);
                        }
                        if !host.enemy_group_active(stage, group) {
                            return Ok(());
                        }
                    }
                    if ctx.mem.step == 0 {
                        ctx.env.out.push(Request::Group { kind, group, on: true });
                        ctx.mem.step = 1;
                    }
                    write(ctx, r, true)
                }
                1 | 2 => {
                    if ctx.mem.value == stage as i64 && ctx.mem.step == 0 {
                        ctx.env.out.push(Request::Group { kind, group, on: false });
                        ctx.mem.step = 1;
                    }
                    write(ctx, r, true)
                }
                _ => Err(unimplemented(ctx, p, "mActType")),
            }
        }
        // Translated from 0x005bef10 (count via 0x005bf4d0 / 0x005bf3d0).
        "EnemyCheck" => {
            let r = rec(p)?;
            let flag = u(r, "mCheckFlag")? as u32;
            if s(r, "mSubPurposeNo")? >= 0 && ctx.owner.quest.is_some() {
                return Err(unimplemented(ctx, p, "quest sub-objective 0x00487140"));
            }
            let all = targets(r);
            let host = ctx.env.host;
            let n = all
                .iter()
                .filter(|t| t.kind == 2 && (flag == 0x1000 || host.enemy_check(stage, t, flag)))
                .count();
            match u(r, "mCheckType")? {
                0 => write(ctx, r, n >= all.len())?,
                1 => write(ctx, r, n != 0)?,
                _ => {}
            }
            let i = u(r, "mResultNum")? as usize;
            if i < 32 {
                ctx.owner.free_ins[i] = n as i32;
            }
            Ok(())
        }
        // Translated from 0x005c0420 with the per-target test 0x005c0820.
        "NpcCheck" => {
            let r = rec(p)?;
            let flag = u(r, "mCheckFlag")? as u32;
            let host = ctx.env.host;
            let w = &*ctx.env.world;
            let test = |t: &crate::Target, f: u32| -> bool {
                if t.kind != 5 {
                    return false;
                }
                let intel = host.npc_intel(stage, t);
                let dead = |id: u32| w.npc_app_die.get(id as usize) || w.npc_die.get(id as usize);
                match f {
                    0 => intel.is_some_and(|i| !dead(i.id) && i.hp_positive),
                    1 => intel.is_some_and(|i| dead(i.id) || !i.hp_positive),
                    2 => host.unit_exists(stage, t),
                    0x1000 => intel.is_some(),
                    _ => false,
                }
            };
            let list = targets(r);
            let total = list.iter().filter(|t| test(t, 0x1000)).count();
            if total == 0 {
                return Ok(());
            }
            let n = list.iter().filter(|t| test(t, flag)).count();
            match u(r, "mCheckType")? {
                0 => write(ctx, r, n >= total)?,
                1 => write(ctx, r, n != 0)?,
                _ => {}
            }
            let i = u(r, "mResultNum")? as usize;
            if i < 32 {
                ctx.owner.free_ins[i] = n as i32;
            }
            Ok(())
        }
        // Translated from 0x005c09f0.
        "NpcDieCheck" => {
            let r = rec(p)?;
            let id = u(r, "mNpcId")? as usize;
            if id >= 900 {
                return Ok(());
            }
            let bit = match u(r, "mActType")? {
                0 => ctx.env.world.npc_app_die.get(id),
                1 => ctx.env.world.npc_die.get(id),
                _ => return Err(unimplemented(ctx, p, "mActType")),
            };
            write(ctx, r, bit)
        }
        // Translated from 0x005c8460: give each cLinkUnitFSM's MainFSM to
        // its targets; the result holds once all have finished.
        "ApplyFSM" => {
            let r = rec(p)?;
            let host = ctx.env.host;
            let (mut total, mut done) = (0, 0);
            let mut reqs = vec![];
            for v in r.fields.iter().filter(|(n, _)| n == "mTargetList").flat_map(|(_, vs)| vs) {
                let mtfsm::Parameter::Object(Some(list)) = v else { continue };
                for lv in list.fields.iter().filter(|(n, _)| n == "mpArray").flat_map(|(_, vs)| vs) {
                    let mtfsm::Parameter::Object(Some(lu)) = lv else { continue };
                    let path = match field_object(lu, "MainFSM")?.value("mFSMName") {
                        Some(mtxfs::Value::Str(s)) => String::from_utf8_lossy(s).into_owned(),
                        _ => return Err(Error::Data("ApplyFSM without MainFSM".into())),
                    };
                    for t in targets(lu) {
                        total += 1;
                        reqs.push(Request::ApplyScript { target: t, path: path.clone() });
                        if host.applied_script_finished(stage, &t) {
                            done += 1;
                        }
                    }
                }
            }
            if ctx.mem.step == 0 {
                ctx.env.out.extend(reqs);
                ctx.mem.step = 1;
            }
            write(ctx, r, done >= total)
        }
        // Translated from 0x005c9250.
        "CheckHp" => {
            let r = rec(p)?;
            write(ctx, r, false)?;
            let t = *targets(r).first().ok_or_else(|| Error::Data("CheckHp without target".into()))?;
            let percent = match u(r, "mActType")? {
                0 => false,
                1 => true,
                _ => return Err(unimplemented(ctx, p, "mActType")),
            };
            let hp = ctx.env.host.unit_hp(stage, &t, percent);
            if hp.is_some() {
                write(ctx, r, true)?;
            }
            let i = (u(r, "mResultNum")? as i64 - 10000) as usize;
            if i < 32 {
                ctx.owner.free_real[i] = hp.unwrap_or(0.0);
            }
            Ok(())
        }
        "Collision" | "NpcAlive" | "SE Request" | "BattleBGM" | "StartQuake" | "SetShell" => Ok(()),
        _ => Err(unimplemented(ctx, p, "no translation")),
    }
}

fn area_hit_key(r: &Record) -> Result<(u32, u32), Error> {
    let h = field_object(r, "AreaHit")?;
    Ok((u(h, "mTaskType")? as u32, u(h, "mNo")? as u32))
}

/// The camera slot tick 0x005c4340, run after each script step.
// Translated from 0x005c4340 (DDDA.exe 2017-12-13 Steam). The skip-held
// force timer and the 5 s motion fallback are not modelled: the host
// reports the end.
pub fn camera_tick(owner: &mut crate::script::Owner, env: &mut crate::Env) {
    let c = &mut owner.camera;
    if c.state == 0 {
        return;
    }
    if env.stage.event.started {
        c.state = 3;
    }
    match c.state {
        1 => c.state = 2,
        2 if !env.stage.camera_running => c.state = 3,
        3 if c.simple => camera_end(owner, env),
        _ => {}
    }
}

/// 0x005c4790: the camera ends; SceNo is raised to the slot's value; a
/// simple event places the party.
// Translated from 0x005c4790 (DDDA.exe 2017-12-13 Steam).
fn camera_end(owner: &mut crate::script::Owner, env: &mut crate::Env) {
    let c = &mut owner.camera;
    c.state = 0;
    if (env.world.sce_no as i32) < c.sce_end as i32 {
        env.world.set_sce_no(c.sce_end, true);
    }
    if c.simple {
        // 0x00517050(0, ...) ends the simple event; stage mode 4 substate 8
        // clears +0x84 and +0x12a (0x0050dcd0).
        env.stage.event.active = false;
        env.stage.event.simple = false;
        env.out.push(Request::CameraEnd { set_pos_type: c.set_pos_type, start_pos: c.start_pos });
    }
}

/// Rewards of a cleared quest: the first rQuestReward::cParam with
/// mQuestNo == quest and mQuestFlg == -1 or that quest bit set
/// (0x00790539).
pub fn reward<D: Data + ?Sized>(data: &D, quest: u32) -> Result<Vec<Request>, Error> {
    let x = data.rewards()?;
    for o in x.walk() {
        if x.class_of(o).name() != Some("rQuestReward::cParam") {
            continue;
        }
        let int = |n: &str| match x.get(o, n).and_then(|v| v.first()) {
            Some(mtxfs::Value::S(v)) => *v as i32,
            Some(mtxfs::Value::U(v)) => *v as i32,
            _ => 0,
        };
        if int("mQuestNo") as u32 != quest {
            continue;
        }
        if int("mQuestFlg") != -1 {
            return Err(Error::Unimplemented(format!("reward condition mQuestFlg {} for q{quest}", int("mQuestFlg"))));
        }
        let items = (0..3)
            .map(|i| (int(&format!("mItemNo{i}")), int(&format!("mItemNum{i}"))))
            .filter(|&(no, n)| no > 0 && n > 0)
            .collect();
        return Ok(vec![Request::Reward {
            quest,
            gold: int("mGold"),
            exp: int("mExp"),
            rift: int("mRimPoint"),
            items,
        }]);
    }
    Ok(vec![])
}

