//! Headless event (cutscene) player: steps an event's timelines frame by
//! frame and reports what the presentation shows and plays. It never
//! renders: the camera pose, actors, fade, subtitle text and sound cues
//! are data for the host.
//!
//! In the original, the stage's event mode (aStage mode 3) hands the row
//! to sEventManager (singleton 0x018fa4b8, update 0x00435840), whose setup
//! 0x004362f0 loads the row's timelines as `uSchedulerExt` units:
//! the main timeline `event\<path>`, a light timeline `_light<N>`, and for
//! some rows the subtitles `_msg_<m|f>` and sound `_sound_<m|f>`. The play
//! state 0x00435fb0 ends the event once the main timeline reaches its
//! frame count - 1. Then the stage's end step raises SceNo and places the
//! party: in this crate that is [`crate::Story`]'s handling of
//! [`crate::HostEvent::EventFinished`], which the host sends when
//! [`EventPlayer::finished`] is true. See docs/EVENTS.md, "Playback".

use crate::data::Data;
use crate::Error;
use ddquest::sdl::{Clock, Kind, Scheduler, Value};
use std::collections::HashMap;

/// Language codes of the message path rewrite, by language index (table
/// 0x016333e0, getter 0x0114fe40).
pub const LANGUAGES: [&str; 21] = [
    "jpn", "eng", "fre", "spa", "ger", "ita", "kor", "zht", "chS", "por", "rus", "pol", "dut", "fin", "swe", "dan",
    "nor", "cze", "hun", "slo", "oth",
];

/// Rewrites the language code inside a resource path: every 3-letter
/// token (separators `\ _ - .` and digits) that is one of [`LANGUAGES`]
/// becomes the code of `language`. `id\message\event\st200ev10_jpn`
/// becomes `..._eng` for English. The subtitle unit loads the rewritten
/// path and falls back to the original one (0x00726780).
// Translated from 0x00de13a0 (DDDA.exe 2017-12-13 Steam).
pub fn localize_path(path: &str, language: usize) -> String {
    let Some(code) = LANGUAGES.get(language) else {
        return path.to_string();
    };
    let lower = path.to_lowercase();
    let mut out: Vec<u8> = path.as_bytes().to_vec();
    let is_sep = |c: u8| b"\\_-.0123456789".contains(&c);
    let b = lower.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if is_sep(b[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && !is_sep(b[i]) {
            i += 1;
        }
        let tok = &lower[start..i];
        // The table compare is case-sensitive against the lowered token,
        // so "chS" never matches.
        if tok.len() == 3 && LANGUAGES.contains(&tok) {
            out[start..i].copy_from_slice(code.as_bytes());
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| path.to_string())
}

/// Which `_light<N>` timeline an event loads (0x004362f0): light 1 on
/// some stages, none on stage 800 or for rows past 0x61, else by the
/// hour (sGameSys +0xb876c).
// Translated from 0x004362f0 (DDDA.exe 2017-12-13 Steam), light part.
pub fn light_index(row: usize, stage: u32, hour: u32) -> Option<u32> {
    if row > 0x61 || stage == 800 {
        return None;
    }
    if [320, 380, 400, 410, 436, 440, 450, 500, 502, 610, 611].contains(&stage) {
        return Some(1);
    }
    Some(match hour {
        0..=3 => 4,
        4..=6 => 1,
        7..=16 => 2,
        17..=19 => 3,
        _ => 4,
    })
}

/// Rows that load the subtitle and sound timelines (0x004362f0).
pub const ROWS_WITH_MSG: [usize; 6] = [0x14, 0x16, 0x1a, 0x1b, 0x2f, 0x34];

/// One loaded timeline with its clock and the value each track last
/// applied.
#[derive(Debug, Clone)]
pub struct Timeline {
    pub path: String,
    pub sched: Scheduler,
    pub clock: Clock,
    /// Last value applied, by track.
    pub values: Vec<Option<Value>>,
    /// Tracks that applied a value in the last update.
    pub applied: Vec<usize>,
    /// The playhead frame of the last update that evaluated the tracks.
    pub last_frame: Option<f32>,
    paths: HashMap<String, Vec<usize>>,
}

impl Timeline {
    pub fn new(path: &str, sched: Scheduler) -> Timeline {
        let n = sched.tracks.len();
        let paths = sched.paths();
        Timeline {
            path: path.to_string(),
            sched,
            clock: Clock::default(),
            values: vec![None; n],
            applied: vec![],
            last_frame: None,
            paths,
        }
    }

    /// One update of the unit: evaluate (if the frame moved) and advance.
    pub fn step(&mut self, delta: f32) {
        self.applied.clear();
        if let Some(at) = self.clock.step(&self.sched, delta) {
            self.last_frame = Some(at.frame);
            for i in 0..self.sched.tracks.len() {
                if !self.sched.tracks[i].kind.has_keys() {
                    continue;
                }
                if let Some(v) = self.sched.eval(i, &at) {
                    self.values[i] = Some(v);
                    self.applied.push(i);
                }
            }
        }
    }

    /// Track indices with this path (`unit.member.property`).
    pub fn tracks(&self, path: &str) -> &[usize] {
        self.paths.get(path).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// The value of the first track with this path.
    pub fn get(&self, path: &str) -> Option<&Value> {
        self.tracks(path).first().and_then(|&i| self.values[i].as_ref())
    }

    pub fn int(&self, path: &str) -> Option<i32> {
        match self.get(path)? {
            Value::Int(v) => Some(*v),
            _ => None,
        }
    }
    pub fn float(&self, path: &str) -> Option<f32> {
        match self.get(path)? {
            Value::Float(v) => Some(*v),
            _ => None,
        }
    }
    pub fn boolean(&self, path: &str) -> Option<bool> {
        match self.get(path)? {
            Value::Bool(v) => Some(*v),
            _ => None,
        }
    }
    pub fn vec4(&self, path: &str) -> Option<[f32; 4]> {
        match self.get(path)? {
            Value::Vec4(v) => Some(*v),
            _ => None,
        }
    }
    pub fn string(&self, path: &str) -> Option<&str> {
        match self.get(path)? {
            Value::String(Some(v)) => Some(v),
            _ => None,
        }
    }
    pub fn resource(&self, path: &str) -> Option<(u32, &str)> {
        match self.get(path)? {
            Value::Resource(Some((h, p))) => Some((*h, p)),
            _ => None,
        }
    }

    /// Units (kind 2) of a class, as (track index, name).
    pub fn units(&self, class: &str) -> Vec<(usize, &str)> {
        let h = mtarc::type_hash(class);
        self.sched
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.kind == Kind::Unit && t.type_hash == h)
            .map(|(i, t)| (i, t.name.as_str()))
            .collect()
    }

    /// Name of the unit or bind a reference value points at.
    pub fn ref_name(&self, v: Option<&Value>) -> Option<&str> {
        match v? {
            Value::Ref(r) if *r != 0 => self.sched.tracks.get(*r as usize).map(|t| t.name.as_str()),
            _ => None,
        }
    }

    /// Did a bool property turn true in the last update (from unset or
    /// false)?
    fn rose(&self, path: &str, before: &[Option<Value>]) -> bool {
        self.tracks(path).first().is_some_and(|&i| {
            self.applied.contains(&i)
                && matches!(self.values[i], Some(Value::Bool(true)))
                && !matches!(before[i], Some(Value::Bool(true)))
        })
    }
}

/// The player's side of an event: language and sex select the subtitle
/// file and the `_m` / `_f` timelines, the hour the light timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventConfig {
    /// Index into [`LANGUAGES`] (1 = English).
    pub language: usize,
    /// sGameSys +0xa76dc bit 0: 0 loads `_m`, 1 `_f` (0x004362f0, strings
    /// 0x0155c6fc "m", 0x0155c700 "f"). *Assumed* to be the player's sex
    /// ([`crate::World::player_sex`]).
    pub sex: u8,
    pub hour: u32,
    pub stage: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CameraAdjust {
    /// `mAdjustTarget`: bit 0 moves the target, bit 1 the eye (0x007ecc30
    /// masks 1 and 2, applied in uOfsMotionCamera::vf09 0x007ebba0).
    pub mask: i32,
    /// The actor unit whose joint the camera follows.
    pub target: Option<String>,
    pub joint: i32,
    pub weight: f32,
}

pub use mtcamera::{vertical_fov_radians, DEFAULT_FAR_CM, DEFAULT_NEAR_CM};

/// The event camera (`uOfsMotionCamera`) this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct CameraView {
    pub list: String,
    /// Slot of the camera list (`mCut`).
    pub cut: i32,
    /// Local camera frame (`mFrame`, counted from the cut).
    pub frame: f32,
    /// The base motion-camera pose (mtcamera, 0x00fab460), in the
    /// resource's coordinates. *Not applied*: sWorldOffset (0x007eb9b0),
    /// `mOffsetPos`, and the actor-joint adjustments below (they need
    /// the actors' skeletons; 0x007ecc30 / 0x007ed220).
    pub pose: Option<mtcamera::Pose>,
    pub fov_type: u32,
    /// Authored projection fields; constructor defaults when not keyed.
    pub near_cm: f32,
    pub far_cm: f32,
    pub zoom_degrees: f32,
    /// `uMotionCamera.mOffsetPos` (+0x70). No reader of it was found in
    /// the per-frame camera path (uMotionCamera::vf08 0x00fab460,
    /// uOfsMotionCamera::vf08 0x007eb9b0 / vf09 0x007ebba0, uCamera::vf09
    /// 0x01128840, cSetInfoCoord::vf09 0x005f35c0), so it is reported and
    /// not applied.
    pub offset_pos: Option<[f32; 4]>,
    /// `mOption` (+0x490): bit 0 smooths the adjustment (0x007bc400).
    pub option: i32,
    pub adjust: Vec<CameraAdjust>,
}

impl CameraView {
    /// The pose with the actor-joint adjustments applied. `offsets[i]` is
    /// adjustment entry i's joint displacement: the joint's world position
    /// minus the position its local pose chain gives (0x007ed220), so it is
    /// non-zero only where IK or body morphs move the joint. The host,
    /// which owns the skeletons, supplies it.
    ///
    /// For each mask (1 target, 2 eye), the matching entries (target set,
    /// joint >= 0, weight > 0) are averaged with weight / total weight of
    /// `offset * weight` (0x007ecc30); the target gets mask 1, the eye
    /// mask 2 (0x007ebba0, without the smoothing of mOption bit 0).
    // Translated from 0x007ecc30, 0x007ed220 (last step) and 0x007ebba0
    // (DDDA.exe 2017-12-13 Steam).
    pub fn adjusted(&self, offsets: &[[f32; 3]]) -> Result<Option<mtcamera::Pose>, Error> {
        let Some(mut pose) = self.pose else { return Ok(None) };
        if self.option & 1 != 0 {
            return Err(Error::Unimplemented("camera adjustment smoothing (mOption bit 0, 0x007bc400)".into()));
        }
        let sum = |mask: i32| -> [f32; 3] {
            let live = |a: &CameraAdjust| a.mask & mask != 0 && a.target.is_some() && a.joint >= 0 && a.weight > 0.0;
            let total: f32 = self.adjust.iter().filter(|a| live(a)).map(|a| a.weight).sum();
            let mut out = [0.0; 3];
            if total > 0.0 {
                for (i, a) in self.adjust.iter().enumerate().filter(|(_, a)| live(a)) {
                    let o = offsets.get(i).copied().unwrap_or([0.0; 3]);
                    for k in 0..3 {
                        out[k] += o[k] * a.weight * (a.weight / total);
                    }
                }
            }
            out
        };
        let (t, e) = (sum(1), sum(2));
        for k in 0..3 {
            pose.target[k] += t[k];
            pose.position[k] += e[k];
        }
        Ok(Some(pose))
    }
}

/// An actor (`uActorModelPl`) this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct ActorView {
    pub name: String,
    /// 0 the player, 1 pawn `unit_no`, 2 NPC `unit_no`
    /// (`uActorModelPl::vf05` 0x0083e010).
    pub unit_type: i32,
    pub unit_no: i32,
    pub motion_list: Option<String>,
    pub motion_no: i32,
    pub motion_frame: f32,
    pub draw: bool,
    pub moving: bool,
    pub offset_pos: Option<[f32; 4]>,
}

/// The subtitle shown this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Subtitle {
    /// GMD label (`IndexName`).
    pub label: String,
    /// The line in the configured language, if the file has it.
    pub text: Option<String>,
    pub speaker: Option<i32>,
    pub listener: Option<i32>,
}

/// A sound the host should start (audio playback does not exist yet).
#[derive(Debug, Clone, PartialEq)]
pub enum Cue {
    /// `uSoundStream`: a stream starts when `mStreamCallFlag` turns true
    /// (*guessed* edge rule; the unit was not traced).
    Stream { unit: String, path: String, stream_id: i32, mode: i32, prepare_id: i32 },
    /// `uSoundSe`: a sound effect or voice starts when `mSeCallFlag`
    /// turns true (*guessed* edge rule). `conditional` is
    /// `mConditionalNo` when `mConditionalFlag` is set: which of several
    /// variants plays (e.g. the Arisen's voice type) is not traced, so
    /// every variant is cued and the host picks.
    Se { unit: String, path: String, se_id: i32, conditional: Option<i32> },
}

/// What one frame of the event shows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EventFrame {
    /// Main timeline frame evaluated this update (`None` when it did not
    /// move, e.g. after the end).
    pub frame: Option<f32>,
    /// Cut track value (`mCut`), when the timeline has a camera.
    pub camera: Option<CameraView>,
    pub actors: Vec<ActorView>,
    /// Full-screen colour of the image-plane filter, when drawn.
    pub fade: Option<[f32; 4]>,
    pub subtitle: Option<Subtitle>,
    pub cues: Vec<Cue>,
    pub finished: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Play,
    Done,
}

/// Plays one event row.
pub struct EventPlayer {
    pub row: usize,
    /// `stSSS\evEE\stSSSevEE`.
    pub path: String,
    pub main: Timeline,
    pub light: Option<Timeline>,
    pub msg: Option<Timeline>,
    pub sound: Option<Timeline>,
    /// Last frame of the event (frame count - 1; 0x004362f0 stores it at
    /// sEventManager +0x24).
    pub end_frame: u32,
    camera: HashMap<String, Vec<u8>>,
    messages: HashMap<String, Option<Vec<u8>>>,
    config: EventConfig,
    state: State,
}

fn data_err(path: &str) -> impl Fn(ddquest::sdl::Error) -> Error + '_ {
    move |e| Error::Data(format!("{path}: {e}"))
}

impl EventPlayer {
    /// Loads row `row`'s timelines (0x004362f0). The event table comes
    /// from the player's exe ([`Data::event_table`]).
    pub fn open<D: Data>(data: &D, row: usize, config: EventConfig) -> Result<EventPlayer, Error> {
        let table = data
            .event_table()
            .ok_or_else(|| Error::Unimplemented("event playback without the exe's event table".into()))?;
        let r = table.rows.get(row).ok_or_else(|| Error::Data(format!("event row {row}")))?;
        let load = |p: &str| -> Result<Option<Timeline>, Error> {
            let full = format!("event\\{p}");
            match data.resource(&full, "rScheduler")? {
                None => Ok(None),
                Some(b) => {
                    let s = Scheduler::parse(&b).map_err(data_err(&full))?;
                    let mut t = Timeline::new(&full, s);
                    // Every event timeline plays once and starts paused;
                    // sEventManager's prepare state 0x00435d90 unpauses
                    // them once loaded (here at once).
                    t.clock.looping = false;
                    t.clock.paused = false;
                    Ok(Some(t))
                }
            }
        };
        let main = load(&r.path)?.ok_or_else(|| Error::Data(format!("event\\{} missing", r.path)))?;
        let light = match light_index(row, config.stage, config.hour) {
            Some(n) => load(&format!("{}_light{n}", r.path))?,
            None => None,
        };
        let (msg, sound) = if ROWS_WITH_MSG.contains(&row) {
            let letter = if config.sex & 1 == 0 { "m" } else { "f" };
            (load(&format!("{}_msg_{letter}", r.path))?, load(&format!("{}_sound_{letter}", r.path))?)
        } else {
            (None, None)
        };
        let end_frame = main.sched.frames.saturating_sub(1);
        let mut p = EventPlayer {
            row,
            path: r.path.clone(),
            main,
            light,
            msg,
            sound,
            end_frame,
            camera: HashMap::new(),
            messages: HashMap::new(),
            config,
            state: State::Play,
        };
        // Camera lists and message files named by the timelines.
        let mut cams = vec![];
        for (i, t) in p.main.sched.tracks.iter().enumerate() {
            if let Some(Value::Resource(Some((h, path)))) = t.keys.first().map(|k| &k.value) {
                if *h == mtarc::type_hash("rCameraList") {
                    cams.push((i, path.clone()));
                }
            }
        }
        for (_, path) in cams {
            if let Some(b) = data.resource(&path, "rCameraList")? {
                mtcamera::CameraList::parse(&b).map_err(|e| Error::Data(format!("{path}: {e}")))?;
                p.camera.insert(path, b);
            }
        }
        if let Some(m) = &p.msg {
            let mut files = vec![];
            for t in &m.sched.tracks {
                if t.name == "MessageResourcePath" {
                    for k in &t.keys {
                        if let Value::String(Some(s)) = &k.value {
                            files.push(s.clone());
                        }
                    }
                }
            }
            for f in files {
                // 0x00726780: the localized path, else the path as written.
                let loc = localize_path(&f, p.config.language);
                let b = match data.resource(&loc, "rGUIMessage")? {
                    Some(b) => Some(b),
                    None => data.resource(&f, "rGUIMessage")?,
                };
                if let Some(b) = &b {
                    mtgmd::MessageFile::parse(b).map_err(|e| Error::Data(format!("{loc}: {e}")))?;
                }
                p.messages.insert(f, b);
            }
        }
        Ok(p)
    }

    pub fn finished(&self) -> bool {
        self.state == State::Done
    }

    /// Skip pressed: sEventManager's skip state 0x00436070 fades over 10
    /// and ends. *Simplified*: ends at once (the fade is the host's).
    pub fn skip(&mut self) {
        self.pause_all();
        self.state = State::Done;
    }

    fn pause_all(&mut self) {
        for t in [Some(&mut self.main), self.light.as_mut(), self.msg.as_mut(), self.sound.as_mut()].into_iter().flatten() {
            t.clock.paused = true;
        }
    }

    /// One frame: the manager's end test (0x00435fb0, before the units
    /// move; the order of the system and unit updates is *assumed*), then
    /// every timeline's update, then what the frame shows.
    // Translated from 0x00435fb0 (DDDA.exe 2017-12-13 Steam), end test.
    pub fn step(&mut self, delta: f32) -> Result<EventFrame, Error> {
        if self.state == State::Play {
            let at = self.main.clock.frame.round() as i64;
            if at >= self.end_frame as i64 {
                self.pause_all();
                self.state = State::Done;
            }
        }
        let before_sound = self.sound.as_ref().map(|t| t.values.clone());
        for t in [Some(&mut self.main), self.light.as_mut(), self.msg.as_mut(), self.sound.as_mut()].into_iter().flatten() {
            t.step(delta);
        }
        let mut f = EventFrame::default();
        if !self.main.applied.is_empty() {
            f.frame = self.main.last_frame;
        }
        f.camera = self.camera_view()?;
        f.actors = self.actors();
        f.fade = self.fade();
        f.subtitle = self.subtitle()?;
        if let (Some(t), Some(before)) = (&self.sound, before_sound) {
            f.cues = Self::cues(t, &before);
        }
        f.finished = self.finished();
        Ok(f)
    }

    fn camera_view(&self) -> Result<Option<CameraView>, Error> {
        let m = &self.main;
        let Some(&(unit, name)) = m.units("uOfsMotionCamera").first() else {
            return Ok(None);
        };
        let _ = unit;
        let Some((_, list)) = m.resource(&format!("{name}.mpCameraList")) else {
            return Ok(None);
        };
        let cut = m.int(&format!("{name}.mCut")).unwrap_or(0);
        let frame = m.float(&format!("{name}.mFrame")).unwrap_or(0.0);
        let (pose, fov_type) = match self.camera.get(list) {
            Some(b) => {
                let cl = mtcamera::CameraList::parse(b).map_err(|e| Error::Data(format!("{list}: {e}")))?;
                match cl.cameras.get(cut.max(0) as usize).and_then(|c| c.as_ref()) {
                    // uMotionCamera +0xb9 (repeat) is not keyed by event
                    // timelines: the frame clamps.
                    Some(c) => (c.sample(frame, false).map(|s| s.pose()), c.fov_type),
                    None => (None, 0),
                }
            }
            None => (None, 0),
        };
        let mut adjust = vec![];
        let p = format!("{name}.mAdjustParamList");
        let masks = m.tracks(&format!("{p}.mAdjustTarget"));
        let targets = m.tracks(&format!("{p}.mpTarget"));
        let joints = m.tracks(&format!("{p}.mJointNo"));
        let weights = m.tracks(&format!("{p}.mAdjWeight"));
        for k in 0..masks.len() {
            let int = |v: Option<&usize>| match v.and_then(|&i| m.values[i].as_ref()) {
                Some(Value::Int(x)) => *x,
                _ => 0,
            };
            adjust.push(CameraAdjust {
                mask: int(masks.get(k)),
                target: m.ref_name(targets.get(k).and_then(|&i| m.values[i].as_ref())).map(str::to_string),
                joint: int(joints.get(k)),
                weight: match weights.get(k).and_then(|&i| m.values[i].as_ref()) {
                    Some(Value::Float(x)) => *x,
                    _ => 0.0,
                },
            });
        }
        Ok(Some(CameraView {
            list: list.to_string(),
            cut,
            frame,
            pose,
            fov_type,
            // uOfsMotionCamera -> uMotionCamera -> uCamera constructors:
            // 0x007eaf20 -> 0x00fa9830 -> 0x01128740 (2017 Steam).
            near_cm: m.float(&format!("{name}.mNearPlane")).unwrap_or(mtcamera::DEFAULT_NEAR_CM),
            far_cm: m.float(&format!("{name}.mFarPlane")).unwrap_or(mtcamera::DEFAULT_FAR_CM),
            zoom_degrees: m.float(&format!("{name}.mZoom")).unwrap_or(0.0),
            offset_pos: m.vec4(&format!("{name}.mOffsetPos")),
            option: m.int(&format!("{name}.mOption")).unwrap_or(0),
            adjust,
        }))
    }

    fn actors(&self) -> Vec<ActorView> {
        let m = &self.main;
        m.units("uActorModelPl")
            .into_iter()
            .map(|(_, n)| ActorView {
                name: n.to_string(),
                unit_type: m.int(&format!("{n}.mUnitType")).unwrap_or(0),
                unit_no: m.int(&format!("{n}.mUnitNo")).unwrap_or(0),
                motion_list: m.resource(&format!("{n}.mpMotionList")).map(|(_, p)| p.to_string()),
                motion_no: m.int(&format!("{n}.mMotion.mMotionNo")).unwrap_or(0),
                motion_frame: m.float(&format!("{n}.mMotion.mFrame")).unwrap_or(0.0),
                draw: m.boolean(&format!("{n}.Draw")).unwrap_or(true),
                moving: m.boolean(&format!("{n}.Move")).unwrap_or(true),
                offset_pos: m.vec4(&format!("{n}.mOffsetPos")),
            })
            .collect()
    }

    fn fade(&self) -> Option<[f32; 4]> {
        let m = &self.main;
        let (_, n) = *m.units("uImagePlaneFilter").first()?;
        if !m.boolean(&format!("{n}.Draw")).unwrap_or(false) {
            return None;
        }
        m.vec4(&format!("{n}.mColor"))
    }

    fn subtitle(&self) -> Result<Option<Subtitle>, Error> {
        let Some(t) = &self.msg else { return Ok(None) };
        let Some(&(_, n)) = t.units("uGUISubtitlesExt2").first() else {
            return Ok(None);
        };
        if !t.boolean(&format!("{n}.Subtitles.Visible")).unwrap_or(false) {
            return Ok(None);
        }
        let Some(label) = t.string(&format!("{n}.Subtitles.IndexName")) else {
            return Ok(None);
        };
        let text = match t.string(&format!("{n}.MessageResourcePath")).and_then(|p| self.messages.get(p)) {
            Some(Some(b)) => {
                let g = mtgmd::MessageFile::parse(b).map_err(|e| Error::Data(format!("message file: {e}")))?;
                g.by_label(label).map(str::to_string)
            }
            _ => None,
        };
        Ok(Some(Subtitle {
            label: label.to_string(),
            text,
            speaker: t.int(&format!("{n}.SpeakerType")),
            listener: t.int(&format!("{n}.ListenerType")),
        }))
    }

    fn cues(t: &Timeline, before: &[Option<Value>]) -> Vec<Cue> {
        let mut out = vec![];
        for (_, n) in t.units("uSoundStream") {
            if t.rose(&format!("{n}.mStreamCallFlag"), before) {
                out.push(Cue::Stream {
                    unit: n.to_string(),
                    path: t.resource(&format!("{n}.mpStream")).map(|(_, p)| p.to_string()).unwrap_or_default(),
                    stream_id: t.int(&format!("{n}.mStreamId")).unwrap_or(0),
                    mode: t.int(&format!("{n}.mStreamMode")).unwrap_or(0),
                    prepare_id: t.int(&format!("{n}.mPrepareId")).unwrap_or(0),
                });
            }
        }
        for (_, n) in t.units("uSoundSe") {
            if t.rose(&format!("{n}.mSeCallFlag"), before) {
                out.push(Cue::Se {
                    unit: n.to_string(),
                    path: t.resource(&format!("{n}.mpSe")).map(|(_, p)| p.to_string()).unwrap_or_default(),
                    se_id: t.int(&format!("{n}.mSeId")).unwrap_or(0),
                    conditional: t
                        .boolean(&format!("{n}.mConditionalFlag"))
                        .unwrap_or(false)
                        .then(|| t.int(&format!("{n}.mConditionalNo")).unwrap_or(0)),
                });
            }
        }
        out
    }

    /// Resources the timelines name (unit, property path, class hash,
    /// path), e.g. motion lists and effect lists for the presentation.
    pub fn resources(&self) -> Vec<(String, u32, String)> {
        let mut out = vec![];
        for t in [Some(&self.main), self.light.as_ref(), self.msg.as_ref(), self.sound.as_ref()].into_iter().flatten() {
            for (i, tr) in t.sched.tracks.iter().enumerate() {
                for k in &tr.keys {
                    if let Value::Resource(Some((h, p))) = &k.value {
                        out.push((t.sched.path(i), *h, p.clone()));
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_adjustment() {
        let pose = mtcamera::Pose { position: [0.0; 3], target: [0.0, 0.0, -100.0], up: [0.0, 1.0, 0.0], fov_degrees: 30.0 };
        let adj = |mask, weight| CameraAdjust { mask, target: Some("pl_00".into()), joint: 173, weight };
        let v = CameraView {
            list: String::new(),
            cut: 0,
            frame: 0.0,
            pose: Some(pose),
            fov_type: 0,
            near_cm: mtcamera::DEFAULT_NEAR_CM,
            far_cm: mtcamera::DEFAULT_FAR_CM,
            zoom_degrees: 0.0,
            offset_pos: None,
            option: 0,
            adjust: vec![adj(2, 0.0), adj(1, 0.8)],
        };
        // Mask 1 moves the target by offset * weight (one live entry).
        let p = v.adjusted(&[[5.0, 0.0, 0.0], [10.0, 0.0, 0.0]]).unwrap().unwrap();
        assert_eq!(p.target, [8.0, 0.0, -100.0]);
        assert_eq!(p.position, [0.0; 3]);
        // No displacement, no change.
        assert_eq!(v.adjusted(&[]).unwrap().unwrap(), pose);
        let mut s = v.clone();
        s.option = 1;
        assert!(s.adjusted(&[]).is_err());
    }

    #[test]
    fn language_rewrite() {
        assert_eq!(localize_path(r"id\message\event\st200ev10_jpn", 1), r"id\message\event\st200ev10_eng");
        assert_eq!(localize_path(r"id\message\event\st200ev10_JPN", 4), r"id\message\event\st200ev10_ger");
        // Tokens that are not language codes stay.
        assert_eq!(localize_path(r"event\st200\ev10\x", 1), r"event\st200\ev10\x");
        assert_eq!(light_index(22, 200, 8), Some(2));
        assert_eq!(light_index(22, 380, 8), Some(1));
        assert_eq!(light_index(22, 800, 8), None);
        assert_eq!(light_index(22, 200, 23), Some(4));
    }
}
