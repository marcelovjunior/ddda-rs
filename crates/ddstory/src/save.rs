//! Running quest/task save state. This project's versioned encoding is not
//! the game's XML or its pointer-bearing cAIFSMData layout.
//!
//! Native sources: 0x00493AA0/0x00484750 save tasks/quests,
//! 0x00495150/0x00484910 recreate them; 0x00E09620/0x00E08560 save
//! graph states, pending transitions and from-all history; 0x00E07F70/
//! 0x00E06B40 restore them. Owner 0x005CC0A0/0x005CC140 saves only
//! FreeIns, FreeReal and the selected timer. Restore re-enters the saved
//! states, so transient process memory, camera, flags and state clocks reset.

use crate::data::Data;
use crate::qct::Ctrl;
use crate::script::{Level, ProcMem, Script};
use crate::story::{EventRequest, Quest, StageState};
use crate::{Story, World};
use ddid::{Reader, Writer};
use mtfsm::Cluster;
use std::collections::HashSet;

type Result<T> = std::result::Result<T, String>;

/// Native task-save exclusions at 0x00494420..0x00494460. `fix` alone
/// is not excluded: only `stage_005` and `nosave` substrings are.
// Translated from 0x00493AA0 (DDDA.exe 2017-12-13 Steam).
pub fn task_saved(path: &str) -> bool {
    !path.contains("stage_005") && !path.contains("nosave")
}

fn text(w: &mut Writer, s: &str) {
    w.bytes(s.as_bytes());
}
fn read_text(r: &mut Reader<'_>) -> Result<String> {
    let s = String::from_utf8(r.bytes()?).map_err(|_| "script path is not UTF-8")?;
    if s.contains('\0') {
        return Err("NUL in saved string".into());
    }
    Ok(s)
}
fn finite(r: &mut Reader<'_>) -> Result<f32> {
    let v = r.f32()?;
    if !v.is_finite() {
        return Err("non-finite script register".into());
    }
    Ok(v)
}

fn write_script(w: &mut Writer, s: &Script) -> Result<()> {
    text(w, &s.path);
    w.u32(s.attribute).len(s.levels.len());
    let mut cluster = s.graph().root.as_ref().ok_or("script has no root")?;
    for (k, level) in s.levels.iter().enumerate() {
        let state = cluster.state(level.current).ok_or("saved state absent")?;
        // Native persists unique node ids, not pointers or local state numbers.
        w.u32(cluster.id).u32(state.unique_id).bool(level.next.is_some());
        if let Some(next) = level.next {
            w.u32(cluster.state(next).ok_or("saved next state absent")?.unique_id);
        }
        w.bool(level.go).len(level.once.len());
        for id in &level.once {
            w.u32(*id);
        }
        if k + 1 < s.levels.len() {
            cluster = state
                .subcluster
                .as_deref()
                .ok_or("saved level has no parent machine")?;
        }
    }
    for v in s.owner.free_ins {
        w.i32(v);
    }
    for v in s.owner.free_real {
        if !v.is_finite() {
            return Err("non-finite script register".into());
        }
        w.f32(v);
    }
    w.i8(s.owner.timer_index);
    Ok(())
}

fn state_by_unique_id(cluster: &Cluster, id: u32) -> Result<u32> {
    let mut states = cluster.states.iter().filter(|s| s.unique_id == id);
    let state = states
        .next()
        .ok_or_else(|| format!("saved state unique id {id} absent"))?;
    if states.next().is_some() {
        return Err(format!("duplicate state unique id {id}"));
    }
    Ok(state.id)
}

// Translated from 0x00E06B40 and 0x005CC140 (DDDA.exe 2017-12-13 Steam).
// File encoding and stronger validation before publishing are project choices.
fn read_script<D: Data>(r: &mut Reader<'_>, data: &D) -> Result<Script> {
    let path = read_text(r)?;
    if path.is_empty() {
        return Err("empty script path".into());
    }
    let graph = data.fsm(&path).map_err(|e| e.to_string())?;
    let mut script = Script::new(&path, graph.clone()).map_err(|e| e.to_string())?;
    script.attribute = r.u32()?;
    let count = r.len(14)?;
    if count == 0 {
        return Err("saved script has no levels".into());
    }
    let mut levels = Vec::new();
    let mut cluster = graph.root.as_ref().ok_or("script has no root")?;
    for k in 0..count {
        if r.u32()? != cluster.id {
            return Err("saved cluster does not match installed graph".into());
        }
        let current = state_by_unique_id(cluster, r.u32()?)?;
        let next = if r.bool()? {
            Some(state_by_unique_id(cluster, r.u32()?)?)
        } else {
            None
        };
        let go = r.bool()?;
        if go != next.is_some() {
            return Err("pending transition without next state".into());
        }
        let mut once = Vec::new();
        let mut seen = HashSet::new();
        for _ in 0..r.len(4)? {
            let id = r.u32()?;
            let state = cluster.state(state_by_unique_id(cluster, id)?).unwrap();
            if state.setting & 8 == 0 || state.from_all.is_none() || !seen.insert(id) {
                return Err("invalid or duplicate from-all history".into());
            }
            once.push(id);
        }
        levels.push(Level {
            current,
            next,
            go,
            came_by_entry: false,
            skip_check: false,
            pending_enter: false,
            once,
        });
        let sub = cluster.state(current).unwrap().subcluster.as_deref();
        if k + 1 < count {
            cluster = sub.ok_or("saved levels exceed graph hierarchy")?;
        } else if sub.is_some() {
            return Err("saved levels omit active submachine".into());
        }
    }
    script.levels = levels;
    // 0x00E06B40 sets core +0x0e: saved states run their entry callbacks.
    script.first = true;
    for v in &mut script.owner.free_ins {
        *v = r.i32()?;
    }
    for v in &mut script.owner.free_real {
        *v = finite(r)?;
    }
    script.owner.timer_index = r.i8()?;
    if !(-1..32).contains(&script.owner.timer_index) {
        return Err("script timer outside -1..31".into());
    }
    Ok(script)
}

// Project continuation extension, not a translation of native save behavior.
fn write_continuation_script(w: &mut Writer, s: &Script) -> Result<()> {
    write_script(w, s)?;
    w.bool(s.running)
        .bool(s.first)
        .u32(s.owner.sys_condition)
        .u32(s.owner.free_flags)
        .f32(s.owner.cnt_state)
        .f32(s.owner.cnt_fsm)
        .u32(s.owner.event_step);
    let c = &s.owner.camera;
    w.u32(c.cam_id)
        .u32(c.sce_end)
        .u32(c.set_pos_type)
        .u32(c.start_pos)
        .u8(c.state)
        .bool(c.simple);
    for l in &s.levels {
        w.bool(l.came_by_entry).bool(l.skip_check).bool(l.pending_enter);
    }
    // HashMap traversal must not make equivalent checkpoints encode differently.
    let mut scratch: Vec<_> = s.mem.iter().collect();
    scratch.sort_by_key(|(key, _)| **key);
    w.len(scratch.len());
    for ((node, process), mem) in scratch {
        w.u32(*node)
            .len(*process)
            .u32(mem.step)
            .u64(mem.value as u64)
            .f32(mem.time);
    }
    Ok(())
}
fn read_continuation_script<D: Data>(r: &mut Reader<'_>, data: &D) -> Result<Script> {
    let mut s = read_script(r, data)?;
    s.running = r.bool()?;
    s.first = r.bool()?;
    s.owner.sys_condition = r.u32()?;
    s.owner.free_flags = r.u32()?;
    s.owner.cnt_state = finite(r)?;
    s.owner.cnt_fsm = finite(r)?;
    s.owner.event_step = r.u32()?;
    let c = &mut s.owner.camera;
    c.cam_id = r.u32()?;
    c.sce_end = r.u32()?;
    c.set_pos_type = r.u32()?;
    c.start_pos = r.u32()?;
    c.state = r.u8()?;
    c.simple = r.bool()?;
    if c.state > 3 {
        return Err("invalid script camera state".into());
    }
    for l in &mut s.levels {
        l.came_by_entry = r.bool()?;
        l.skip_check = r.bool()?;
        l.pending_enter = r.bool()?;
    }
    fn process_exists(c: &Cluster, node: u32, process: usize) -> bool {
        c.states.iter().any(|s| {
            s.unique_id == node && process < s.processes.len()
                || s.subcluster
                    .as_deref()
                    .is_some_and(|c| process_exists(c, node, process))
        })
    }
    for _ in 0..r.len(24)? {
        let (node, process) = (r.u32()?, r.u32()? as usize);
        let mem = ProcMem {
            step: r.u32()?,
            value: r.u64()? as i64,
            time: finite(r)?,
        };
        if !process_exists(s.graph().root.as_ref().unwrap(), node, process)
            || s.mem.insert((node, process), mem).is_some()
        {
            return Err("invalid or duplicate script process scratch".into());
        }
    }
    Ok(s)
}
fn write_event(w: &mut Writer, request: &Option<EventRequest>) {
    w.bool(request.is_some());
    if let Some(e) = request {
        w.len(e.row).u32(e.stage).u32(e.event).bool(e.sce_no.is_some());
        if let Some(s) = e.sce_no {
            w.u32(s);
        }
        w.u8(e.set_pos_type).u32(e.start_pos);
    }
}
fn read_event(r: &mut Reader<'_>) -> Result<Option<EventRequest>> {
    if !r.bool()? {
        return Ok(None);
    }
    Ok(Some(EventRequest {
        row: r.u32()? as usize,
        stage: r.u32()?,
        event: r.u32()?,
        sce_no: if r.bool()? { Some(r.u32()?) } else { None },
        set_pos_type: r.u8()?,
        start_pos: r.u32()?,
    }))
}
fn write_stage(w: &mut Writer, s: &StageState) {
    let e = &s.event;
    w.bool(e.active).bool(e.started).bool(e.simple);
    write_event(w, &e.pending);
    write_event(w, &e.playing);
    w.bool(s.dragon_challenge.baseline_hp.is_some());
    if let Some(hp) = s.dragon_challenge.baseline_hp {
        w.f32(hp);
    }
    w.bool(s.fade_black)
        .bool(s.camera_running)
        .bool(s.message_showing)
        .bool(s.menu_open)
        .bool(s.pawn_message_slots.is_some());
    if let Some(n) = s.pawn_message_slots {
        w.u32(n);
    }
    w.u32(s.game_flags.b883c)
        .u32(s.game_flags.b8844)
        .u32(s.game_flags.b8848)
        .u32(s.game_flags.s54);
}
fn read_stage(r: &mut Reader<'_>, s: &mut StageState) -> Result<()> {
    let e = &mut s.event;
    e.active = r.bool()?;
    e.started = r.bool()?;
    e.simple = r.bool()?;
    e.pending = read_event(r)?;
    e.playing = read_event(r)?;
    s.dragon_challenge.baseline_hp = if r.bool()? { Some(finite(r)?) } else { None };
    s.fade_black = r.bool()?;
    s.camera_running = r.bool()?;
    s.message_showing = r.bool()?;
    s.menu_open = r.bool()?;
    s.pawn_message_slots = if r.bool()? { Some(r.u32()?) } else { None };
    s.game_flags.b883c = r.u32()?;
    s.game_flags.b8844 = r.u32()?;
    s.game_flags.b8848 = r.u32()?;
    s.game_flags.s54 = r.u32()?;
    Ok(())
}

impl<D: Data> Story<D> {
    /// Native-derived running-script payload in a project-owned versioned
    /// chunk. Stage reload rebuilds scenarios and fix_nosave tasks; saved
    /// task paths are kept on that first stage load instead of discarded.
    pub fn save_scripts(&self) -> Result<Vec<u8>> {
        self.encode_scripts(false)
    }

    /// Optional project-owned deterministic continuation, including transient
    /// clocks/scratch and nosave tasks. It is not native save fidelity.
    /// Actor, combat and external presentation state belongs to their owners.
    pub fn save_continuation(&self) -> Result<Vec<u8>> {
        self.encode_scripts(true)
    }

    fn encode_scripts(&self, continuation: bool) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        w.u8(1).u32(self.world.stage_no);
        if continuation {
            w.u64(self.frame);
            write_stage(&mut w, &self.stage);
        }
        let quests: Vec<_> = self
            .quests
            .iter()
            .filter(|q| continuation || q.flags & 1 != 0)
            .collect();
        w.len(quests.len());
        for q in quests {
            w.u32(q.id).u32(q.flags).len(q.log.len());
            for v in &q.log {
                w.u32(*v);
            }
            w.bool(q.ctrl.is_some());
            if let Some(ctrl) = &q.ctrl {
                for v in ctrl.disabled {
                    w.u32(v);
                }
            }
            w.bool(q.script.is_some());
            if let Some(script) = &q.script {
                w.bool(script.owner.quest_stamp.is_some());
                if let Some((day, ms)) = script.owner.quest_stamp {
                    w.u64(day).u32(ms);
                }
                if continuation {
                    write_continuation_script(&mut w, script)?;
                } else {
                    write_script(&mut w, script)?;
                }
            }
        }
        let tasks: Vec<_> = self
            .tasks
            .iter()
            .filter(|s| continuation || s.running && task_saved(&s.path))
            .collect();
        w.len(tasks.len());
        for task in tasks {
            if continuation {
                write_continuation_script(&mut w, task)?;
            } else {
                write_script(&mut w, task)?;
            }
        }
        // Native 0x00493AA0 saves disabled/fired bits of scenario families
        // 0..10. Masks and per-frame detection are rebuilt by the stage.
        let scenarios: Vec<_> = self
            .stage
            .scenarios
            .tasks
            .iter()
            .take(if continuation { usize::MAX } else { 11 })
            .enumerate()
            .flat_map(|(task, rows)| rows.iter().enumerate().map(move |(row, e)| (task, row, e)))
            .collect();
        w.len(scenarios.len());
        for (task, row, e) in scenarios {
            w.u32(task as u32).u32(row as u32);
            text(&mut w, &e.name);
            if continuation {
                w.u32(e.status).u32(e.mask).u32(e.prev).bool(e.ended);
            } else {
                w.u32(e.status & 12);
            }
        }
        Ok(w.finish())
    }

    /// Recreate resources, validate all saved identities, then publish a
    /// story. An absent chunk keeps the legacy world-only load behavior.
    pub fn from_save(data: D, world: World, bytes: Option<&[u8]>) -> Result<Story<D>> {
        let Some(bytes) = bytes else {
            return Ok(Story::from_world(data, world));
        };
        Self::decode_scripts(data, world, bytes, false)
    }

    /// Validate the native payload first, then apply the explicitly optional
    /// continuation extension to a fresh candidate; no partial story escapes.
    pub fn from_save_with_continuation(
        data: D,
        world: World,
        native: Option<&[u8]>,
        continuation: Option<&[u8]>,
    ) -> Result<Story<D>> {
        let baseline = Self::from_save(data, world, native)?;
        match continuation {
            Some(bytes) if native.is_some() => {
                let extended = Self::decode_scripts(baseline.data, baseline.world, bytes, true)?;
                if extended.save_scripts()?.as_slice() != native.unwrap() {
                    return Err("continuation does not match native script payload".into());
                }
                Ok(extended)
            }
            Some(_) => Err("continuation chunk without native script chunk".into()),
            None => Ok(baseline),
        }
    }

    fn decode_scripts(data: D, world: World, bytes: &[u8], continuation: bool) -> Result<Story<D>> {
        let mut r = Reader::new(bytes);
        r.version("STORY scripts", 1)?;
        let stage = r.u32()?;
        if stage != world.stage_no {
            return Err("script/world stage mismatch".into());
        }
        let mut story = Story::from_world(data, world);
        if continuation {
            story.frame = r.u64()?;
            read_stage(&mut r, &mut story.stage)?;
        }
        let mut ids = HashSet::new();
        for _ in 0..r.len(15)? {
            let (id, mut flags) = (r.u32()?, r.u32()?);
            if !ids.insert(id) || !continuation && flags & 1 == 0 {
                return Err("duplicate or unloaded saved quest".into());
            }
            // 0x00484910 excludes ordinary finished quests from restoration.
            let mut log = Vec::new();
            for _ in 0..r.len(4)? {
                log.push(r.u32()?);
            }
            let ctrl_path = format!("etc\\questCtrl\\q{:04}_b{:02}", id >> 16, id & 0xffff);
            let mut ctrl = story
                .data
                .qct(&ctrl_path)
                .map_err(|e| e.to_string())?
                .map(Ctrl::new);
            if r.bool()? {
                let c = ctrl
                    .as_mut()
                    .ok_or("saved quest control table absent from install")?;
                for v in &mut c.disabled {
                    *v = r.u32()?;
                }
            } else if ctrl.is_some() {
                return Err("saved quest omitted its control table".into());
            }
            let script = if r.bool()? {
                let stamp = if r.bool()? {
                    Some((r.u64()?, r.u32()?))
                } else {
                    None
                };
                let mut s = if continuation {
                    read_continuation_script(&mut r, &story.data)?
                } else {
                    read_script(&mut r, &story.data)?
                };
                let expected = format!("quest\\q{:04}_b{:02}", id >> 16, id & 0xffff);
                if !s.path.eq_ignore_ascii_case(&expected) {
                    return Err("quest script path/owner mismatch".into());
                }
                s.owner.quest = Some(id);
                s.owner.quest_stamp = stamp;
                Some(s)
            } else {
                None
            };
            // Native clears started on quests 31 and 39 when loading.
            if !continuation && [31, 39].contains(&(id >> 16)) {
                flags &= !2;
            }
            if continuation || flags & 0x20 == 0 || (98..123).contains(&(id >> 16)) || script.is_some() {
                story.quests.push(Quest {
                    id,
                    flags,
                    log,
                    ctrl,
                    script,
                });
            }
        }
        let mut paths = HashSet::new();
        for _ in 0..r.len(4)? {
            let s = if continuation {
                read_continuation_script(&mut r, &story.data)?
            } else {
                read_script(&mut r, &story.data)?
            };
            if !continuation && !task_saved(&s.path) || !paths.insert(s.path.to_ascii_lowercase()) {
                return Err("excluded or duplicate saved task".into());
            }
            story.tasks.push(s);
        }
        story.stage.scenarios =
            crate::scenario::Scenarios::load(&story.data.scenarios(stage).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let mut entries = HashSet::new();
        for _ in 0..r.len(16)? {
            let (task, row, name, status) = (r.u32()?, r.u32()?, read_text(&mut r)?, r.u32()?);
            if (!continuation && (task >= 11 || status & !12 != 0)) || !entries.insert((task, row)) {
                return Err("invalid saved scenario status".into());
            }
            let e = story
                .stage
                .scenarios
                .entry_mut(task, row)
                .ok_or("saved scenario missing from install")?;
            if e.name != name {
                return Err("saved scenario identity mismatch".into());
            }
            if continuation {
                e.status = status;
                e.mask = r.u32()?;
                e.prev = r.u32()?;
                e.ended = r.bool()?;
            } else {
                e.status = (e.status & !12) | status;
            }
        }
        r.end()?;
        story.restoring_stage = Some(stage);
        Ok(story)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::{Dispatch, OrderCtx, ProcMem};
    use mtfsm::{Graph, Process, State};
    use std::sync::Arc;

    struct Fixture(Arc<Graph>);
    impl Data for Fixture {
        fn fsm(&self, _: &str) -> std::result::Result<Arc<Graph>, crate::Error> {
            Ok(self.0.clone())
        }
        fn qct(&self, _: &str) -> std::result::Result<Option<Arc<ddquest::qct::QuestCtrl>>, crate::Error> {
            Ok(None)
        }
        fn stage_scripts(&self, _: u32) -> std::result::Result<Vec<String>, crate::Error> {
            Ok(vec![])
        }
        fn scenarios(&self, _: u32) -> std::result::Result<Vec<(u32, Arc<mtxfs::Xfs>)>, crate::Error> {
            Ok(vec![])
        }
        fn event_table(&self) -> Option<&ddquest::events::EventTable> {
            None
        }
        fn rewards(&self) -> std::result::Result<Arc<mtxfs::Xfs>, crate::Error> {
            unreachable!()
        }
        fn prologue_end(&self) -> Option<&crate::exe::PrologueEnd> {
            None
        }
    }
    fn fixture() -> Fixture {
        let state = |id, unique_id, setting, from_all| State {
            id,
            unique_id,
            owner_id: 0,
            name: format!("state{id}").into_bytes(),
            setting,
            user_attribute: 0,
            from_all,
            links: vec![],
            processes: vec![Process {
                container: b"entry".to_vec(),
                category: vec![],
                parameter: None,
            }],
            subcluster: None,
        };
        let mut root = state(1, 101, 0, None);
        root.subcluster = Some(Box::new(Cluster {
            id: 7,
            owner_node_unique_id: 101,
            initial_state: 2,
            states: vec![state(2, 202, 0, None)],
        }));
        Fixture(Arc::new(Graph {
            owner: b"cFSMOrder".to_vec(),
            attribute: 0,
            root: Some(Cluster {
                id: 5,
                owner_node_unique_id: 0,
                initial_state: 1,
                states: vec![root, state(9, 909, 8, Some(33))],
            }),
            conditions: vec![],
        }))
    }

    #[test]
    fn native_exclusions_do_not_drop_fix_scripts() {
        assert!(task_saved(r"scr\st100\fsm\fix\sunrise"));
        assert!(!task_saved(r"scr\st100\fsm\fix_nosave\sunrise"));
        assert!(!task_saved(r"event\st100\stage_FSM\stage_005"));
        assert!(task_saved(r"scr\st100\fsm\q0002_event1"));
    }

    #[test]
    fn native_restore_keeps_graph_registers_and_reenters_without_transient_scratch() {
        let data = fixture();
        let mut original = Script::new(r"quest\q0002_b00", data.0.clone()).unwrap();
        original.first = false;
        original.levels[0].once.push(909);
        original.owner.free_ins[17] = -991;
        original.owner.free_real[3] = 14.25;
        original.owner.timer_index = 3;
        original.owner.cnt_state = 22.0;
        original.owner.free_flags = 7;
        original.mem.insert(
            (101, 0),
            ProcMem {
                step: 8,
                value: 9,
                time: 4.0,
            },
        );
        let mut w = Writer::new();
        write_script(&mut w, &original).unwrap();
        let bytes = w.finish();
        let restored = read_script(&mut Reader::new(&bytes), &data).unwrap();
        assert_eq!(restored.state_path(), vec![1, 2]);
        assert_eq!(restored.levels[0].once, vec![909]);
        assert_eq!(restored.owner.free_ins, original.owner.free_ins);
        assert_eq!(restored.owner.free_real, original.owner.free_real);
        assert_eq!(restored.owner.timer_index, 3);
        assert!(restored.first);
        assert_eq!(restored.owner.cnt_state, 0.0);
        assert_eq!(restored.owner.free_flags, 0);
        assert!(restored.mem.is_empty());

        struct Entry;
        impl Dispatch for Entry {
            fn update(ctx: &mut OrderCtx, _: &Process) -> std::result::Result<(), crate::Error> {
                ctx.owner.free_ins[0] += 1;
                Ok(())
            }
            fn state(_: &mut OrderCtx, _: &Process) -> std::result::Result<(), crate::Error> {
                Ok(())
            }
        }
        let mut restored = restored;
        let mut world = World::default();
        let mut stage = Default::default();
        let host = crate::scripted::ScriptedHost::default();
        let (mut out, mut cmds) = (vec![], vec![]);
        let mut env = crate::Env {
            world: &mut world,
            stage: &mut stage,
            host: &host,
            time: Default::default(),
            out: &mut out,
            cmds: &mut cmds,
            events: None,
        };
        restored.step::<Entry>(&mut env).unwrap();
        assert_eq!(restored.owner.free_ins[0], 2, "both restored levels re-enter");
        restored.step::<Entry>(&mut env).unwrap();
        assert_eq!(
            restored.owner.free_ins[0], 2,
            "entry runs only on first restore step"
        );
    }

    #[test]
    fn pending_transition_and_corrupt_node_are_checked_against_installed_graph() {
        let data = fixture();
        let mut script = Script::new("test", data.0.clone()).unwrap();
        script.levels[0].next = Some(9);
        script.levels[0].go = true;
        let mut w = Writer::new();
        write_script(&mut w, &script).unwrap();
        let bytes = w.finish();
        let restored = read_script(&mut Reader::new(&bytes), &data).unwrap();
        assert_eq!(restored.levels[0].next, Some(9));
        assert!(restored.levels[0].go);
        let mut bad_graph = (*data.0).clone();
        bad_graph.root.as_mut().unwrap().states[0].unique_id = 1001;
        assert!(read_script(&mut Reader::new(&bytes), &Fixture(Arc::new(bad_graph))).is_err());
    }

    #[test]
    fn legacy_load_and_all_truncations_versions_and_timer_rejected() {
        let mut story = Story::from_world(fixture(), World::default());
        let mut s = Script::new("test", story.data.0.clone()).unwrap();
        s.owner.timer_index = 31;
        story.tasks.push(s);
        let bytes = story.save_scripts().unwrap();
        assert!(Story::from_save(fixture(), story.world.clone(), Some(&bytes)).is_ok());
        for end in 0..bytes.len() {
            assert!(
                Story::from_save(fixture(), story.world.clone(), Some(&bytes[..end])).is_err(),
                "prefix {end}"
            );
        }
        let mut bad = bytes.clone();
        bad[0] = 2;
        assert!(Story::from_save(fixture(), story.world.clone(), Some(&bad)).is_err());
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(Story::from_save(fixture(), story.world.clone(), Some(&trailing)).is_err());
        story.tasks[0].owner.timer_index = 32;
        let bad = story.save_scripts().unwrap();
        assert!(Story::from_save(fixture(), story.world.clone(), Some(&bad)).is_err());
        assert!(Story::from_save(fixture(), story.world.clone(), None)
            .unwrap()
            .tasks
            .is_empty());
    }

    #[test]
    fn optional_continuation_preserves_clocks_scratch_nosave_tasks_and_is_deterministic() {
        let mut story = Story::from_world(fixture(), World::default());
        story.frame = 930;
        story.stage.camera_running = true;
        story.stage.dragon_challenge.baseline_hp = Some(1400.0);
        let mut s = Script::new(r"scr\st100\fsm\fix_nosave\test", story.data.0.clone()).unwrap();
        s.first = false;
        for l in &mut s.levels {
            l.skip_check = false;
            l.pending_enter = false;
        }
        s.owner.cnt_state = 12.0;
        s.owner.cnt_fsm = 48.0;
        s.owner.free_flags = 7;
        s.owner.camera.cam_id = 12;
        s.owner.camera.state = 2;
        s.mem.insert(
            (202, 0),
            ProcMem {
                step: 3,
                value: -991,
                time: 7.0,
            },
        );
        s.mem.insert(
            (101, 0),
            ProcMem {
                step: 2,
                value: 81,
                time: 4.0,
            },
        );
        story.tasks.push(s);
        let native = story.save_scripts().unwrap();
        let continuation = story.save_continuation().unwrap();
        let restored = Story::from_save_with_continuation(
            fixture(),
            story.world.clone(),
            Some(&native),
            Some(&continuation),
        )
        .unwrap();
        assert_eq!(restored.script_states(), story.script_states());
        assert_eq!(restored.frame, 930);
        assert!(restored.stage.camera_running);
        assert_eq!(restored.stage.dragon_challenge, story.stage.dragon_challenge);
        assert_eq!(restored.tasks[0].mem, story.tasks[0].mem);
        assert_eq!(restored.tasks[0].owner.camera, story.tasks[0].owner.camera);
        assert!(!restored.tasks[0].first && !restored.tasks[0].levels[0].pending_enter);
        assert_eq!(restored.save_continuation().unwrap(), continuation);
        assert!(
            Story::from_save(fixture(), story.world.clone(), Some(&native))
                .unwrap()
                .tasks
                .is_empty(),
            "absent extension retains native exclusions"
        );
    }

    #[test]
    fn continuation_rejects_malformed_or_mismatched_payload_and_cannot_stand_alone() {
        let mut story = Story::from_world(fixture(), World::default());
        story
            .tasks
            .push(Script::new("test", story.data.0.clone()).unwrap());
        let native = story.save_scripts().unwrap();
        let continuation = story.save_continuation().unwrap();
        for end in 0..continuation.len() {
            assert!(
                Story::from_save_with_continuation(
                    fixture(),
                    story.world.clone(),
                    Some(&native),
                    Some(&continuation[..end])
                )
                .is_err(),
                "prefix {end}"
            );
        }
        assert!(Story::from_save_with_continuation(
            fixture(),
            story.world.clone(),
            None,
            Some(&continuation)
        )
        .is_err());
        story.tasks[0].owner.free_ins[0] = 42;
        let mismatched = story.save_continuation().unwrap();
        assert!(Story::from_save_with_continuation(
            fixture(),
            story.world.clone(),
            Some(&native),
            Some(&mismatched)
        )
        .is_err());
        story.tasks[0].owner.free_ins[0] = 0;
        story.tasks[0].mem.insert((123456, 0), ProcMem::default());
        let bad_scratch = story.save_continuation().unwrap();
        assert!(Story::from_save_with_continuation(
            fixture(),
            story.world.clone(),
            Some(&native),
            Some(&bad_scratch)
        )
        .is_err());
    }
}
