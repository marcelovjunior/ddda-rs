//! A scripted host for tests and dumps: it answers every request after a
//! fixed delay and treats every enemy as defeated a while after the story
//! first asks about it. It stands in for the game's presentation, AI and
//! combat; nothing here is the original's behaviour.

use crate::{Host, HostEvent, Menu, NpcIntel, Request, Target};
use std::collections::{HashMap, HashSet};

pub struct ScriptedHost {
    pub frame: u64,
    /// Frames a cutscene, camera or message takes.
    pub event_frames: u64,
    pub camera_frames: u64,
    pub message_frames: u64,
    /// Frames after which an enemy the story asks about counts as defeated.
    pub kill_frames: u64,
    pub tutorials: bool,
    /// Vocation picked at the weapon table.
    pub job: u32,
    /// Stage the host loads after the character editor (stage 801).
    pub after_edit: u32,
    present: HashSet<(u32, Target)>,
    first_asked: std::cell::RefCell<HashMap<(u32, Target), u64>>,
    applied: HashMap<Target, (String, u64)>,
    pending: Vec<(u64, HostEvent)>,
    /// Extra host events after a camera starts (stand-ins for host-run AI,
    /// e.g. the dragon of q0000).
    pub on_camera: HashMap<u32, Vec<(u64, HostEvent)>>,
    /// Every request seen, with its frame.
    pub log: Vec<(u64, Request)>,
}

impl Default for ScriptedHost {
    fn default() -> Self {
        ScriptedHost {
            frame: 0,
            event_frames: 90,
            camera_frames: 60,
            message_frames: 30,
            kill_frames: 60,
            tutorials: true,
            job: 1,
            after_edit: 200,
            present: HashSet::new(),
            first_asked: Default::default(),
            applied: HashMap::new(),
            pending: vec![],
            on_camera: HashMap::new(),
            log: vec![],
        }
    }
}

impl ScriptedHost {
    /// The host for the opening: stands in for the dragon's AI in q0000
    /// (its script dragon_op of stage 200 sets quest 0
    /// bits 20, 21 and 5) during the dragon camera (17).
    /// Enemies fall after 20 s, long enough for q0000 to reach its
    /// "battle" state (`mCntState > 17`) as a real fight does.
    pub fn opening() -> ScriptedHost {
        let mut h = ScriptedHost { kill_frames: 600, ..ScriptedHost::default() };
        h.on_camera.insert(
            17,
            vec![
                (30, HostEvent::SetQuestBit { quest: 0, bit: 20, value: true }),
                (45, HostEvent::SetQuestBit { quest: 0, bit: 21, value: true }),
                (120, HostEvent::SetQuestBit { quest: 0, bit: 5, value: true }),
            ],
        );
        h
    }

    fn later(&mut self, frames: u64, e: HostEvent) {
        self.pending.push((self.frame + frames, e));
    }

    /// React to this frame's requests.
    pub fn handle(&mut self, stage: u32, reqs: &[Request]) {
        for r in reqs {
            self.log.push((self.frame, r.clone()));
            match r {
                // Events fade out before they load (stage event mode 0x0050be20)
                // and their timelines end in black (st200ev10 frames
                // 1643-1702).
                Request::PlayEvent { .. } => {
                    self.later(3, HostEvent::FadeBlack(true));
                    self.later(10, HostEvent::FadeBlack(false));
                    self.later(self.event_frames - 10, HostEvent::FadeBlack(true));
                    self.later(self.event_frames, HostEvent::EventFinished);
                    self.later(self.event_frames + 10, HostEvent::FadeBlack(false));
                }
                Request::JumpArea { stage, .. } => {
                    self.later(2, HostEvent::StageLoaded { stage: *stage });
                    // The character editor (stage 801) hands over to the
                    // village when done.
                    if *stage == 801 {
                        let s = self.after_edit;
                        self.later(30, HostEvent::StageLoaded { stage: s });
                    }
                }
                Request::Camera { simple, cam_id, .. } => {
                    for (d, e) in self.on_camera.get(cam_id).cloned().unwrap_or_default() {
                        self.later(d, e);
                    }
                    // A simple event fades out first (stage mode 4, 0x0050dcd0).
                    if *simple {
                        self.later(3, HostEvent::FadeBlack(true));
                        self.later(10, HostEvent::FadeBlack(false));
                    }
                    self.later(self.camera_frames, HostEvent::CameraFinished)
                }
                Request::Message { .. } | Request::Tutorial { .. } => {
                    self.later(self.message_frames, HostEvent::MessageFinished)
                }
                Request::OpenMenu { menu: Menu::Vocation, .. } => {
                    let j = self.job;
                    self.later(20, HostEvent::PlayerJob(j));
                    self.later(20, HostEvent::MenuClosed(Menu::Vocation));
                }
                Request::Placement { target, present, .. } => {
                    if *present {
                        self.present.insert((stage, *target));
                    } else {
                        self.present.remove(&(stage, *target));
                    }
                }
                Request::ApplyScript { target, path } => {
                    self.applied.insert(*target, (path.clone(), self.frame));
                    // The NPC script of `bootEv_st200ev15` (q0002_n001_ai)
                    // talks to the Arisen and then sets GSF 381.
                    if path.to_lowercase().ends_with("q0002_n001_ai") {
                        self.later(90, HostEvent::SetGsf { no: 381, value: true });
                    }
                }
                _ => {}
            }
        }
    }

    /// Host events due this frame; advances the frame counter.
    pub fn events(&mut self) -> Vec<HostEvent> {
        self.frame += 1;
        let now = self.frame;
        let (due, rest): (Vec<_>, Vec<_>) = self.pending.drain(..).partition(|(t, _)| *t <= now);
        self.pending = rest;
        due.into_iter().map(|(_, e)| e).collect()
    }

    fn defeated(&self, stage: u32, t: &Target) -> bool {
        let mut m = self.first_asked.borrow_mut();
        let first = *m.entry((stage, *t)).or_insert(self.frame);
        self.frame >= first + self.kill_frames
    }
}

impl Host for ScriptedHost {
    fn layout_loaded(&self, _: u32, _: &Target) -> bool {
        true
    }
    fn unit_exists(&self, stage: u32, t: &Target) -> bool {
        self.present.contains(&(stage, *t))
    }
    fn group_loaded(&self, _: u32, _: u32, _: u32) -> bool {
        true
    }
    fn enemy_group_done(&self, _: u32, _: u32) -> bool {
        false
    }
    fn enemy_group_active(&self, _: u32, _: u32) -> bool {
        true
    }
    fn enemy_check(&self, stage: u32, t: &Target, flag: u32) -> bool {
        let dead = self.defeated(stage, t);
        match flag {
            0 => self.present.contains(&(stage, *t)),
            1 | 3 => dead,
            2 => !dead,
            _ => false,
        }
    }
    fn npc_intel(&self, _: u32, t: &Target) -> Option<NpcIntel> {
        Some(NpcIntel { id: t.group % 900, hp_positive: true })
    }
    fn npc_unit(&self, _: u32) -> bool {
        true
    }
    fn applied_script_finished(&self, _: u32, t: &Target) -> bool {
        self.applied.get(t).is_some_and(|(_, f)| self.frame >= f + 120)
    }
    fn unit_hp(&self, _: u32, _: &Target, _: bool) -> Option<f32> {
        Some(100.0)
    }
    fn tutorials_enabled(&self) -> bool {
        self.tutorials
    }
    fn pl_check(&self, _: u32) -> i32 {
        1
    }
    fn player_damaged(&self) -> bool {
        false
    }
    fn pawn_lost(&self) -> bool {
        false
    }
    fn has_item(&self, _: u32, _: u32) -> bool {
        false
    }
}
