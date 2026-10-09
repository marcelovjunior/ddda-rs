//! `--play --opening`: the first playable story slice (m5-cassardis). The
//! session starts in Cassardis at q0001 "Newly Arisen", as after the
//! prologue: the host walks the prologue and q0000 headless first
//! (`ddhost::opening::fast_forward`, the scripted stand-ins of ddstory).
//! From there everything runs in `ddhost::Host` (story, NPC schedules, the
//! interaction lock) and the view performs what the story asks:
//! - **cutscenes** (st200 ev10 and ev15) with the event player, then
//!   reports them over (`OpeningWorld::event_done`);
//! - the **party placement** after an event (`PlaceParty`: a start position
//!   of `scr\st200\etc\st200`);
//! - the **vocation menu** at the weapon table (stage mode 0x19): a
//!   labelled stub, 1 Fighter or 2 Mage;
//! - **talks** (`Host::talk`): E next to an NPC, lines as subtitles, E for
//!   the next line, digits for choices;
//! - the **NPCs** on their daily schedules (`ddnpcrt`), drawn with their
//!   looks. The host completes their moves at once; the view walks them
//!   there (*viewer choice*, 150 cm/s).
//!
//! Walking into the 1300 volume runs the original's scripts: SceNo 1300,
//! `bootEv_st200ev15`, event st200 ev15 (SceNo 1400), the NPC script that
//! sets GSF 381, and the boot of q0002 "Upon a Pawn".
//!
//! `--demo` plays the slice by itself (weapon table, room exit, ev10, a
//! talk with Quina, the walk out, ev15, q0002) and logs the flag and SceNo
//! checks; `--shots <dir>` saves a screenshot at each step and exits at
//! the end.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use ddhost::ddnpcrt::{self, TalkEvent};
use ddhost::opening::{fast_forward, to_newly_arisen, OpeningWorld, CASSARDIS_ROWS};
use ddhost::{Control, FrameClock, FrameInputs, Host, PlayerInput, ProfileId};
use ddsim::Input;
use ddstory::data::InstallData;
use ddstory::event::{EventConfig, EventFrame, EventPlayer};
use ddstory::{Menu, Request, Story};

use crate::character::Playback;
use crate::event::{Actor, EventCam, FadePlane, InfoText, SubtitleText};
use crate::play::{self, FollowCam, PlayState, Shown};
use crate::stage::{Loader, UNIT};

/// The Cassardis stage.
pub const STAGE: u32 = 200;
/// NPCs within this distance (cm, horizontal) of the Arisen answer E.
pub const TALK_RANGE: f32 = 220.0;
/// The view's NPC walking speed (cm/s, *viewer choice*).
pub const NPC_SPEED: f32 = 150.0;

/// A cutscene the view plays for the story.
struct Cut {
    row: usize,
    player: EventPlayer,
    last: EventFrame,
    acc: f32,
}

/// A talk shown to the local seat.
struct TalkView {
    npc: u32,
    line: String,
    choices: Vec<String>,
}

/// The demo's script steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    WeaponTable,
    RoomExit,
    TalkQuina,
    WalkOut,
    Q0002Event,
    Done,
}

struct Demo {
    step: Step,
    since: f32,
    /// Where the Arisen was at the last progress check.
    check: (f32, Vec3),
    path: std::collections::VecDeque<Vec3>,
    press: bool,
    last_talk: f32,
    talked: bool,
}

type StoryHost = Host<InstallData, OpeningWorld>;

/// The same authoritative session, with an optional LAN transport.
/// The original has no co-op: this ownership arrangement is our design.
pub struct HostHandle {
    local: Option<Box<StoryHost>>,
    server: Option<Box<ddnet::Server<InstallData, OpeningWorld, ddnet::Udp>>>,
}
impl std::ops::Deref for HostHandle {
    type Target = StoryHost;
    fn deref(&self) -> &StoryHost {
        self.server
            .as_ref()
            .map(|s| &s.host)
            .unwrap_or_else(|| self.local.as_deref().unwrap())
    }
}
impl std::ops::DerefMut for HostHandle {
    fn deref_mut(&mut self) -> &mut StoryHost {
        match &mut self.server {
            Some(s) => &mut s.host,
            None => self.local.as_deref_mut().unwrap(),
        }
    }
}
impl HostHandle {
    fn tick_network(
        &mut self,
        inputs: &FrameInputs,
        now: u64,
    ) -> Result<ddhost::FrameReport, ddhost::HostError> {
        match &mut self.server {
            Some(s) => s.step(inputs, now),
            None => self.local.as_mut().unwrap().tick(inputs),
        }
    }
}

/// The session (host frame stepping, the cutscene, the talk, the demo).
pub struct Session {
    pub host: HostHandle,
    guest: Option<ddnet::Client<ddnet::Udp>>,
    network_ready: bool,
    pending_input: PlayerInput,
    failure: Option<String>,
    guest_control_shot: bool,
    guest_npcs: HashMap<u32, [f32; 3]>,
    acc: f32,
    cut: Option<Cut>,
    talk: Option<TalkView>,
    /// Start positions of the stage: position, angle, member offsets.
    starts: Vec<([f32; 3], f32, Vec<[f32; 3]>)>,
    /// NPC entities and where the view shows them.
    npcs: HashMap<u32, (Entity, Vec3, f32)>,
    npc_motions: (Option<mtlmt::Motion>, Option<mtlmt::Motion>),
    npc_moving: HashMap<u32, bool>,
    demo: Option<Demo>,
    /// `--shots <dir>`: a screenshot per step.
    shots: Option<PathBuf>,
    shot_queue: Vec<String>,
    shots_taken: usize,
    view_frames: u64,
    quit_at: Option<f32>,
    language: usize,
    sex: u8,
    last_sce: u32,
    /// The Arisen's look (seat 0), for the cutscenes' player actor.
    pub arisen_look: Option<ddlook::Look>,
    /// NPC entities to hide for good.
    gone: Vec<Entity>,
}

#[derive(Resource)]
pub struct OpeningRes(pub Mutex<Session>);

/// Builds the host and walks it to q0001 (before the window opens).
pub fn prepare(
    native_pc: &std::path::Path,
    exe: &std::path::Path,
    players: u8,
    demo: bool,
    shots: Option<PathBuf>,
) -> Result<Session, String> {
    let t = std::time::Instant::now();
    let data = InstallData::open(native_pc, Some(exe)).map_err(|e| e.to_string())?;
    let vfs = ddvfs::Vfs::open(native_pc)?;
    let sim = ddhost::install::sim(&vfs)?;
    let ground = ddhost::install::stage_collision(&vfs, STAGE)?;
    let mut host = Host::new(
        sim,
        Some(Story::new_game(data)),
        OpeningWorld::new(),
        Box::new(ground),
    );
    host.data.npc = Some(std::sync::Arc::new(
        ddnpcrt::InstallData::open(native_pc, "eng").map_err(|e| e.to_string())?,
    ));
    for k in 0..players.max(1) {
        let control = if k == 0 { Control::Local } else { Control::Remote };
        host.join(ProfileId(u64::from(k) + 1), control, ddhost::opening::AWAY, 0.0)
            .ok_or("cannot seat")?;
    }
    let f = fast_forward(&mut host, &to_newly_arisen(), 240, 30_000, |h| {
        h.story.as_ref().is_some_and(|s| s.quest(1).is_some())
    })
    .map_err(|e| e.to_string())?;
    host.world.live(CASSARDIS_ROWS);
    let w = &host.story.as_ref().unwrap().world;
    info!(
        "opening: fast-forwarded {} frames in {:.1} s: SceNo ladder {:?}; now stage {}, SceNo {}, start position {}, {:02}:{:02}",
        f.frames,
        t.elapsed().as_secs_f32(),
        f.ladder,
        w.stage_no,
        w.sce_no,
        w.start_pos,
        w.hour,
        w.minute
    );
    let starts = ddhost::install::start_positions(&vfs, STAGE)?
        .entries
        .iter()
        .map(|e| {
            (
                [e.position[0], e.position[1], e.position[2]],
                e.angle,
                e.offset_positions.iter().map(|o| [o[0], o[1], o[2]]).collect(),
            )
        })
        .collect();
    let sce = w.sce_no;
    let ids = ddhost::install::stage_npcs(&vfs, STAGE, sce);
    let mut spawned = 0;
    for id in &ids {
        if host.spawn_npc(*id, 1).unwrap_or(false) {
            spawned += 1;
        }
    }
    info!(
        "opening: {spawned} NPCs on their stage-{STAGE} schedules ({} controllers)",
        ids.len()
    );
    let mut s = Session {
        host: HostHandle {
            local: Some(Box::new(host)),
            server: None,
        },
        guest: None,
        network_ready: false,
        pending_input: PlayerInput::default(),
        failure: None,
        guest_control_shot: false,
        guest_npcs: HashMap::new(),
        acc: 0.0,
        cut: None,
        talk: None,
        starts,
        npcs: HashMap::new(),
        npc_motions: (None, None),
        npc_moving: HashMap::new(),
        demo: demo.then_some(Demo {
            step: Step::WeaponTable,
            since: 0.0,
            check: (0.0, Vec3::ZERO),
            path: Default::default(),
            press: false,
            last_talk: 0.0,
            talked: false,
        }),
        shots,
        shot_queue: vec![],
        shots_taken: 0,
        view_frames: 0,
        quit_at: None,
        language: 1,
        sex: 0,
        last_sce: sce,
        arisen_look: None,
        gone: vec![],
    };
    let start = f
        .arrival
        .filter(|(stage, _)| *stage == STAGE)
        .map_or(s.world().start_pos, |(_, n)| n);
    s.place_party(start);
    Ok(s)
}

impl Session {
    /// Presentation query using the same rendered positions and range as E.
    pub fn can_talk(&self, seat: ddsim::PlayerId) -> bool {
        // Opening interaction currently routes E only to host seat 0.
        if seat != 0 || self.cut.is_some() || self.talk.is_some() {
            return false;
        }
        let Some(player) = self.host.sim.player(seat) else { return false; };
        self.npcs.values().any(|(_, at, _)| {
            (at.x - player.position[0]).hypot(at.z - player.position[2]) <= TALK_RANGE
        })
    }

    pub fn event_active(&self) -> bool {
        self.cut.is_some()
    }

    /// Seats at start position `n` (seat k > 0 at member offset k - 1).
    fn place_party(&mut self, n: u32) {
        let Some((pos, yaw, offs)) = self.starts.get(n as usize).cloned() else {
            warn!("opening: no start position {n}");
            return;
        };
        let seats: Vec<_> = self.host.roster.ids().collect();
        for (k, id) in seats.into_iter().enumerate() {
            let off = if k == 0 {
                [0.0; 3]
            } else {
                offs.get(k - 1).copied().unwrap_or([150.0 * k as f32, 0.0, 0.0])
            };
            if let Some(p) = self.host.sim.player_mut(id) {
                p.position = [pos[0] + off[0], pos[1] + off[1], pos[2] + off[2]];
                p.yaw = yaw;
                p.drop_from = p.position[1];
            }
        }
        if let Some(server) = &mut self.host.server {
            server.config.spawn = pos;
            if let Some(spot) = &mut server.config.spot {
                spot.pos = pos;
                spot.yaw = yaw;
            }
        }
        info!(
            "opening: party placed at start position {n} ({:.0}, {:.0}, {:.0})",
            pos[0], pos[1], pos[2]
        );
    }

    fn world(&self) -> &ddstory::World {
        &self.host.story.as_ref().unwrap().world
    }

    /// The nearest NPC to seat 0 within [`TALK_RANGE`].
    fn npc_near(&self) -> Option<u32> {
        let p = self.host.sim.player(0)?.position;
        self.npcs
            .iter()
            .map(|(&id, &(_, at, _))| (id, (at.x - p[0]).hypot(at.z - p[2])))
            .filter(|&(_, d)| d <= TALK_RANGE)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    fn shot(&mut self, name: &str) {
        if self.shots.is_some() {
            self.shot_queue.push(name.to_string());
        }
    }

    /// Talk events for the local seat.
    fn show_talk(&mut self, events: &[TalkEvent]) {
        for e in events {
            match e {
                TalkEvent::Line {
                    npc,
                    text,
                    conversation,
                    index,
                    ..
                } => {
                    let line = clean(text);
                    info!("opening: NPC {npc} conversation {conversation} line {index}: {line}");
                    self.talk = Some(TalkView {
                        npc: *npc,
                        line,
                        choices: vec![],
                    });
                }
                TalkEvent::Choices { npc, options, .. } => {
                    let line = self.talk.as_ref().map(|t| t.line.clone()).unwrap_or_default();
                    self.talk = Some(TalkView {
                        npc: *npc,
                        line,
                        choices: options.iter().map(|o| clean(o)).collect(),
                    });
                }
                TalkEvent::Action { npc, action, .. } => {
                    info!("opening: NPC {npc} talk command {action:?} (screens are not shown in this slice)");
                    self.talk = None;
                }
                TalkEvent::End { npc, .. } => {
                    info!("opening: talk with NPC {npc} ended");
                    self.talk = None;
                }
            }
        }
    }
}

/// GMD markup removed for the subtitle line.
fn clean(t: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for c in t.chars() {
        match c {
            '<' => tag = true,
            '>' if tag => tag = false,
            _ if !tag => out.push(c),
            _ => {}
        }
    }
    out.replace('\r', "").trim().to_string()
}

/// The player look of a cutscene actor or NPC.
fn npc_look(vfs: &ddvfs::Vfs, id: u32, row: u32) -> Option<ddlook::Look> {
    ddlook::npc_look(vfs, id, id, row)
        .map(|l| l.look)
        .map_err(|e| warn!("NPC {id}: look: {e}"))
        .ok()
}

/// Startup part: the stage, the Arisen (via play.rs), the NPCs, the
/// cutscenes' actors (hidden) and the subtitle UI.
#[allow(clippy::too_many_arguments)]
pub fn setup(
    commands: &mut Commands,
    loader: &mut Loader,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    bindposes: &mut Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>,
    res: &OpeningRes,
    party_looks: Vec<Option<ddlook::Look>>,
    demo: bool,
) -> Option<PlayState> {
    let mut s = res.0.lock().unwrap();
    crate::event::spawn_stage_models(commands, loader, meshes, materials, images, STAGE);
    let vfs = loader.vfs;
    // The Arisen: play.rs's characters at the seats.
    let seats: Vec<(u8, [f32; 3], f32)> = (0..ddhost::player::MAX_ARISEN as u8)
        .map(|id| {
            let p = s.host.sim.player(id).or_else(|| s.host.sim.player(0)).unwrap();
            (id, p.position, p.yaw)
        })
        .collect();
    let arisen = seats
        .iter()
        .map(|&(id, pos, yaw)| {
            (
                id,
                party_looks.get(usize::from(id)).cloned().flatten(),
                Vec3::from(pos),
                yaw,
            )
        })
        .collect();
    s.arisen_look = party_looks.first().cloned().flatten();
    s.sex = s.arisen_look.as_ref().map_or(0, |l| l.sex);
    let party = play::Party {
        arisen,
        pawn: None,
        goblins: 0,
        kits: HashMap::new(),
    };
    let terrain = ddworld::Terrain::empty();
    let mut st = match play::setup(
        commands,
        loader,
        meshes,
        materials,
        images,
        bindposes,
        terrain,
        Vec3::ZERO,
        party,
        None,
    ) {
        Ok(st) => st,
        Err(e) => {
            error!("opening: {e}");
            return None;
        }
    };
    st.demo = demo;
    // NPCs with their looks, idle until their schedule moves them.
    let list = mtlmt::parse(&vfs.read(play::MOTIONS, crate::character::RMOTIONLIST).ok()?).ok()?;
    s.npc_motions = (
        list.motions.first().cloned().flatten(),
        list.motions.get(0x54).cloned().flatten(),
    );
    let ids: Vec<u32> = s.host.npcs.npcs.keys().copied().collect();
    for id in ids {
        let pos = Vec3::from(s.host.npcs.position(id).unwrap_or([0.0, -100_000.0, 0.0]));
        let r = match npc_look(vfs, id, 0) {
            Some(l) => crate::character::spawn_look(
                commands,
                loader,
                meshes,
                materials,
                images,
                bindposes,
                &l,
                play::MOTIONS,
                0,
                pos * UNIT,
                Some(0.0),
            ),
            None => continue,
        };
        match r {
            Ok(e) => {
                s.npcs.insert(id, (e, pos, 0.0));
            }
            Err(e) => warn!("NPC {id}: {e}"),
        }
    }
    info!("opening: {} NPCs drawn", s.npcs.len());
    // The cutscenes' actors, hidden until their event plays.
    let data = &s.host.story.as_ref().unwrap().data;
    for row in CASSARDIS_ROWS {
        let cfg = EventConfig {
            language: s.language,
            sex: s.sex,
            hour: s.world().hour,
            stage: STAGE,
        };
        let Ok(mut probe) = EventPlayer::open(data, row, cfg) else {
            warn!("opening: event row {row} does not open");
            continue;
        };
        let Ok(first) = probe.step(1.0) else { continue };
        let mut looks = HashMap::new();
        for a in &first.actors {
            let l = match a.unit_type {
                2 => npc_look(vfs, a.unit_no as u32, row as u32),
                0 => s.arisen_look.clone(),
                _ => None,
            };
            if let Some(l) = l {
                looks.insert(a.name.clone(), l);
            }
        }
        let actors = crate::event::spawn_event_actors(
            commands, loader, meshes, materials, images, bindposes, &first, &looks, row,
        );
        for e in actors {
            commands.entity(e).insert(Visibility::Hidden);
        }
    }
    crate::event::spawn_event_ui(commands);
    s.shot("1-newly-arisen");
    Some(st)
}

/// The step part: host frames, inputs, the party, NPCs, talks, the menu.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn step(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mouse: Res<AccumulatedMouseMotion>,
    state: Option<ResMut<PlayState>>,
    res: Option<Res<OpeningRes>>,
    args: Res<crate::Args>,
    pause: Res<crate::pause::PauseState>,
    mut roots: Query<(&mut Transform, &mut Playback), Without<FollowCam>>,
    mut cams: Query<&mut Transform, With<FollowCam>>,
    mut vis: Query<&mut Visibility, Without<Actor>>,
) {
    let (Some(mut st), Some(res)) = (state, res) else {
        return;
    };
    let st = &mut *st;
    let mut guard = res.0.lock().unwrap();
    let s = &mut *guard;
    let now = time.elapsed_secs();
    if !s.network_ready {
        s.network_ready = true;
        if let Err(e) = s.network(&args) {
            panic!("opening network: {e}");
        }
    }
    if pause.paused && args.host.is_none() && args.join.is_none() {
        return;
    }
    let network_now = (now * 1000.0) as u64;
    if let Some(server) = &mut s.host.server {
        server.pump(network_now);
        if server
            .host
            .story
            .as_ref()
            .is_some_and(|story| story.quest(2).is_some())
        {
            server.heartbeat(network_now);
        }
    }
    if s.guest.is_some() {
        guest_step(
            s,
            st,
            &keys,
            &buttons,
            &mouse,
            time.delta_secs(),
            pause.paused,
            pause.mouse_look(buttons.pressed(MouseButton::Right)),
            network_now,
            &mut roots,
            &mut cams,
            &mut vis,
        );
        return;
    }
    if !pause.paused && pause.mouse_look(buttons.pressed(MouseButton::Right)) {
        play::orbit(st, &buttons, &mouse);
    }
    // Local input: talk and menu keys first, the rest to the host.
    let near = s.npc_near();
    let mut input = PlayerInput {
        sim: play::p1_input(&keys, &buttons, st.cam_yaw),
        interact: false,
    };
    let e_press = !pause.paused && keys.just_pressed(KeyCode::KeyE);
    if s.world_menu().is_some() {
        input.sim = Input {
            camera_yaw: st.cam_yaw,
            ..Default::default()
        };
        let pick = if !pause.paused && keys.just_pressed(KeyCode::Digit1) {
            Some(ddskill_fighter())
        } else if !pause.paused && keys.just_pressed(KeyCode::Digit2) {
            Some(ddskill_mage())
        } else {
            None
        };
        if let Some(j) = pick {
            close_menu(s, j);
        }
    } else if s.talk.is_some() {
        input.sim = Input {
            camera_yaw: st.cam_yaw,
            ..Default::default()
        };
        let digit = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4]
            .iter()
            .position(|&k| keys.just_pressed(k));
        let has_choices = s.talk.as_ref().is_some_and(|t| !t.choices.is_empty());
        if has_choices {
            if let Some(i) = digit {
                match s.host.talk_choose(0, i as u8) {
                    Ok(o) => s.show_talk(&o.events),
                    Err(e) => warn!("talk: {e}"),
                }
            }
        } else if e_press {
            match s.host.talk_next(0) {
                Ok(o) => s.show_talk(&o.events),
                Err(e) => {
                    warn!("talk: {e}");
                    s.talk = None;
                }
            }
        }
    } else if s.cut.is_some() {
        input.sim = Input {
            camera_yaw: st.cam_yaw,
            ..Default::default()
        };
    } else if e_press && near.is_some() {
        let npc = near.unwrap();
        match s.host.talk(0, npc) {
            Ok(o) => s.show_talk(&o.events),
            Err(e) => info!("opening: NPC {npc}: {e}"),
        }
    } else {
        input.interact = keys.pressed(KeyCode::KeyE);
    }
    if play::active_input(&input.sim) && s.demo.is_some() {
        s.demo = None;
    }
    // The demo stands in for the keys.
    if !pause.paused && s.demo.is_some() && s.cut.is_none() {
        input = if s.shots.is_some() && s.shots_taken == 0 {
            PlayerInput::default()
        } else {
            demo_input(s, now)
        };
    }
    if pause.paused {
        input = PlayerInput::default();
    }
    if pause.paused || s.cut.is_some() || s.talk.is_some() || s.world_menu().is_some() {
        s.pending_input = PlayerInput::default();
    }
    s.pending_input.sim = play::latch_input(s.pending_input.sim, input.sim);
    s.pending_input.interact |= input.interact;
    input = s.pending_input;
    // Host frames at 30 Hz.
    let frame = FrameClock::default().seconds();
    s.acc = (s.acc + time.delta_secs()).min(0.25);
    while s.acc >= frame
        && !s
            .host
            .story
            .as_ref()
            .is_some_and(|story| story.quest(2).is_some())
    {
        s.acc -= frame;
        s.pending_input = PlayerInput::default();
        // the NPCs where the view shows them, for volumes that detect NPCs
        s.host.world.npcs = s
            .npcs
            .iter()
            .map(|(&id, &(_, p, yaw))| ddstory::NpcUnit {
                id,
                unit: ddstory::Player {
                    pos: p.to_array(),
                    yaw,
                },
            })
            .collect();
        let mut inputs = FrameInputs::new();
        let seats: Vec<_> = s.host.roster.ids().collect();
        for id in seats {
            inputs.insert(id, if id == 0 { input } else { PlayerInput::default() });
        }
        input.interact = false;
        let report = match s.host.tick_network(&inputs, network_now) {
            Ok(r) => r,
            Err(e) => {
                s.failure = Some(format!("host frame failed: {e}"));
                break;
            }
        };
        for r in &report.requests {
            match r {
                Request::PlaceParty {
                    start_pos: Some(n), ..
                } => s.place_party(*n),
                Request::PlaceParty { pos: Some(p), .. } => info!("opening: party placed at {p:?}"),
                Request::JumpArea { stage, .. } if *stage != STAGE => {
                    warn!("opening: JumpArea to stage {stage} is outside this slice")
                }
                Request::QuestBanner { kind, quest, log } => {
                    info!("opening: quest banner kind {kind} q{quest:04} log {log}")
                }
                Request::Message { quest, mes_id, .. } => info!("opening: message q{quest:04} #{mes_id}"),
                Request::OpenMenu { .. } | Request::Order { .. } | Request::Scenario { .. } => {}
                other => debug!("opening: {other:?}"),
            }
        }
        for (id, e) in s.host.npc_errors.drain(..) {
            warn!("opening: NPC {id} stops where it is (its schedule needs what ddnpcrt does not translate: {e})");
        }
        if let Some(row) = s.host.world.inner.awaiting.take() {
            start_cut(s, row);
        }
        let sce = s.world().sce_no;
        if sce != s.last_sce {
            let w = s.world();
            info!(
                "opening: SceNo {} -> {sce} (GSF 372 {}, 381 {}; quests {:?})",
                s.last_sce,
                w.gsf.get(372),
                w.gsf.get(381),
                s.host
                    .story
                    .as_ref()
                    .unwrap()
                    .quests
                    .iter()
                    .map(|q| q.number())
                    .collect::<Vec<_>>()
            );
            s.last_sce = sce;
        }
    }
    if s.world_menu().is_some() && s.shots_taken_for("2-vocation-menu") {
        s.shot("2-vocation-menu");
    }
    // The NPCs walk where the schedule put them.
    let dt = time.delta_secs();
    let ids: Vec<u32> = s.npcs.keys().copied().collect();
    for id in ids {
        let Some(target) = s.host.npcs.position(id).map(Vec3::from) else {
            continue;
        };
        let (e, mut at, mut yaw) = s.npcs[&id];
        let d = target - at;
        let flat = Vec3::new(d.x, 0.0, d.z);
        let moving = flat.length() > 5.0 && flat.length() < 800.0;
        if flat.length() >= 800.0 {
            at = target;
        } else if moving {
            at += flat.normalize() * (NPC_SPEED * dt).min(flat.length());
            at.y = target.y;
            yaw = flat.x.atan2(flat.z);
        } else {
            at = target;
        }
        s.npcs.insert(id, (e, at, yaw));
        if let Ok((mut tr, mut pb)) = roots.get_mut(e) {
            tr.translation = at * UNIT;
            tr.rotation = Quat::from_rotation_y(yaw);
            if s.npc_moving.get(&id) != Some(&moving) {
                let m = if moving {
                    s.npc_motions.1.clone()
                } else {
                    s.npc_motions.0.clone()
                };
                if let Some(m) = m {
                    pb.change_motion(m, 6.0);
                }
                s.npc_moving.insert(id, moving);
            }
            pb.hold_frame = None;
        }
    }
    // The party; during a cutscene the camera belongs to the event.
    let bodies: Vec<Shown> = s.host.sim.players.iter().map(Shown::of).collect();
    let follow = st.follow;
    if s.cut.is_some() {
        st.follow = u8::MAX;
    }
    play::show(st, &bodies, &mut roots, &mut cams);
    st.follow = follow;
    let hide = s.cut.is_some();
    for (&e, &on) in &st.visible {
        if let Ok(mut v) = vis.get_mut(e) {
            *v = if on && !hide {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
    for e in s.gone.drain(..) {
        if let Ok(mut v) = vis.get_mut(e) {
            *v = Visibility::Hidden;
        }
    }
    for &(e, ..) in s.npcs.values() {
        if let Ok(mut v) = vis.get_mut(e) {
            *v = if hide {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
        }
    }
}

impl Session {
    fn world_menu(&self) -> Option<(Menu, usize)> {
        self.host.world.menu
    }
    /// True until a screenshot of that name was queued once.
    fn shots_taken_for(&mut self, name: &str) -> bool {
        let key = format!("taken:{name}");
        if self.shot_queue.iter().any(|s| s == &key) {
            return false;
        }
        self.shot_queue.push(key);
        true
    }
}

fn ddskill_fighter() -> u32 {
    1
}
fn ddskill_mage() -> u32 {
    3
}

/// The view closed the vocation menu with `job` (Fighter 1, Mage 3:
/// `ddskill::vocation::Vocation`).
fn close_menu(s: &mut Session, job: u32) {
    info!("opening: vocation menu (stub) closed with job {job}");
    s.host.world.close_menu(Some(job));
}

fn start_cut(s: &mut Session, row: usize) {
    let w = s.world();
    let cfg = EventConfig {
        language: s.language,
        sex: s.sex,
        hour: w.hour,
        stage: STAGE,
    };
    match EventPlayer::open(&s.host.story.as_ref().unwrap().data, row, cfg) {
        Ok(p) => {
            info!("opening: cutscene row {row} ({}), {} frames", p.path, p.end_frame);
            s.cut = Some(Cut {
                row,
                player: p,
                last: EventFrame::default(),
                acc: 0.0,
            });
        }
        Err(e) => {
            s.failure = Some(format!("event {row} failed to open: {e}"));
        }
    }
}

/// The cutscene part: the event's frame on its actors, the camera, the fade
/// and the subtitle; the talk line, the menu stub and the info line
/// otherwise; the step screenshots.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn cutscene(
    mut commands: Commands,
    time: Res<Time>,
    res: Option<Res<OpeningRes>>,
    args: Res<crate::Args>,
    pause: Res<crate::pause::PauseState>,
    mut cams: Query<(&mut Transform, &mut Projection), (With<FollowCam>, Without<Actor>)>,
    mut actors: Query<(&mut Actor, &mut Playback, &mut Visibility, &mut Transform), Without<EventCam>>,
    mut fade: Query<&mut BackgroundColor, With<FadePlane>>,
    mut subs: Query<&mut Text, (With<SubtitleText>, Without<InfoText>)>,
    mut info_text: Query<&mut Text, (With<InfoText>, Without<SubtitleText>)>,
    windows: Query<&Window>,
    mut exit: MessageWriter<AppExit>,
    mut gameplay_projection: Local<GameplayProjection>,
) {
    let Some(res) = res else { return };
    let mut guard = res.0.lock().unwrap();
    let s = &mut *guard;
    if let Some(message) = s.failure.take() {
        error!("opening failed: {message}");
        exit.write(AppExit::error());
        return;
    }
    if pause.paused && args.host.is_none() && args.join.is_none() {
        return;
    }
    let aspect = windows
        .single()
        .map(|w| w.width() / w.height().max(1.0))
        .unwrap_or(16.0 / 9.0);
    let mut finished = None;
    if let Some(c) = &mut s.cut {
        // mDeltaTime: frames at 30 per second.
        c.acc += time.delta_secs().min(0.1) * 30.0;
        while c.acc >= 1.0 && !c.player.finished() {
            c.acc -= 1.0;
            match c.player.step(1.0) {
                Ok(f) => {
                    crate::audio::cues(&mut commands, &f);
                    c.last = f;
                },
                Err(e) => {
                    s.failure = Some(format!("event playback failed: {e}"));
                    break;
                }
            }
        }
        let f = &c.last;
        if let Ok((mut t, mut proj)) = cams.single_mut() {
            gameplay_projection.capture(&proj);
            crate::event::apply_camera(f, aspect, &mut t, &mut proj);
        }
        crate::event::apply_actors(f, c.row, &mut actors);
        crate::event::apply_ui(f, &mut fade, &mut subs);
        if s.guest.is_some() && f.frame.is_some_and(|x| ((x as u32) % 300) < 2) {
            info!(
                "opening guest event row {} frame {:?}, camera {:?}, {} actors",
                c.row,
                f.frame,
                f.camera,
                f.actors.len()
            );
            s.shot_queue.push(format!(
                "want:guest-row{}-frame{}",
                c.row,
                f.frame.unwrap_or(0.0) as u32 / 300 * 300
            ));
        }

        if f.frame.is_some_and(|x| (300.0..302.0).contains(&x)) {
            let name = if c.row == 22 {
                "3-cutscene-ev10"
            } else {
                "6-cutscene-ev15"
            };
            s.shot_queue.push(format!("want:{name}"));
        }
        if c.player.finished() || f.finished {
            finished = Some(Some(c.row));
        }
    }
    if let Some(Some(row)) = finished {
        info!("opening: cutscene row {row} over; control returns");
        s.cut = None;
        if s.guest.is_some() {
            s.guest_control_shot = true;
        }
        if s.guest.is_none() {
            s.host.world.event_done();
            if let Some(server) = &mut s.host.server {
                server.event_done();
            }
        }
        for (a, _, mut v, _) in &mut actors {
            if a.row == row {
                *v = Visibility::Hidden;
            }
        }
        if let Ok(mut bg) = fade.single_mut() {
            bg.0 = Color::NONE;
        }
        // the follow camera's projection again
        if let Ok((_, mut proj)) = cams.single_mut() {
            if gameplay_projection.restore(&mut proj) {
                if let Projection::Perspective(pp) = proj.as_ref() {
                    info!(
                        "opening: restored gameplay projection fov={} near={} far={}",
                        pp.fov, pp.near, pp.far
                    );
                }
            }
        }
    }
    let wants: Vec<String> = s
        .shot_queue
        .iter()
        .filter_map(|q| q.strip_prefix("want:").map(str::to_string))
        .collect();
    s.shot_queue.retain(|q| !q.starts_with("want:"));
    for w in wants {
        if s.shots_taken_for(&w) {
            s.shot(&w);
        }
    }
    if s.cut.is_none() {
        if let Ok(mut t) = subs.single_mut() {
            t.0 = if let Some((menu, _)) = s.world_menu() {
                format!("{menu:?} menu (viewer stub, not uGUIJobChange): 1 Fighter   2 Mage")
            } else if let Some(talk) = &s.talk {
                let mut l = format!("[NPC {}] {}", talk.npc, talk.line);
                for (k, c) in talk.choices.iter().enumerate() {
                    l += &format!("\n{}: {c}", k + 1);
                }
                l
            } else {
                String::new()
            };
        }
    }
    if let Ok(mut i) = info_text.single_mut() {
        let w = s.world();
        let quests: Vec<String> = s
            .host
            .story
            .as_ref()
            .unwrap()
            .quests
            .iter()
            .map(|q| format!("q{:04}", q.number()))
            .collect();
        i.0 = format!(
            "opening (m5 slice): stage {} SceNo {}  {:02}:{:02}  quests {}  GSF372 {} GSF381 {}{}\nE: interact / talk   WASD: move   mouse: camera   Esc: pause",
            w.stage_no,
            w.sce_no,
            w.hour,
            w.minute,
            quests.join(" "),
            w.gsf.get(372) as u8,
            w.gsf.get(381) as u8,
            s.cut.as_ref().map(|c| format!("  cutscene row {} frame {:?}", c.row, c.last.frame.map(|f| f as i64))).unwrap_or_default()
        );
    }
    if s.host
        .story
        .as_ref()
        .is_some_and(|story| story.quest(2).is_some())
    {
        if let Ok(mut i) = info_text.single_mut() {
            i.0.push_str(
                "\nOpening slice complete: q0002 booted. Further quest execution is outside this slice.",
            );
        }
    }
    if s.guest.is_some() {
        if let Ok(mut i) = info_text.single_mut() {
            i.0 = "Opening guest: story and NPCs controlled by host | WASD move | mouse look | Esc pause"
                .into();
        }
    }
    s.view_frames += 1;
    // Let pipelines and the first visible frame settle before capturing.
    if s.view_frames < 60 || time.elapsed_secs() < 5.0 {
        return;
    }
    // Screenshots (`--shots`).
    // Only one primary-window request per frame: Bevy drops duplicates.
    let next = s
        .shot_queue
        .iter()
        .position(|q| !q.contains(':'))
        .map(|i| s.shot_queue.remove(i));
    if let (Some(dir), Some(name)) = (s.shots.clone(), next) {
        let path = dir.join(format!("{name}.png"));
        info!("opening: screenshot {}", path.display());
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
        s.shots_taken += 1;
    }
    let now = time.elapsed_secs();
    if let Some(t) = s.quit_at {
        if now >= t {
            exit.write(AppExit::Success);
        }
    }
}

/// The demo's input for this frame: walk to the step's target, press E,
/// answer the menu and the talk.
fn demo_input(s: &mut Session, now: f32) -> PlayerInput {
    let Some(p) = s.host.sim.player(0).map(|p| Vec3::from(p.position)) else {
        return PlayerInput::default();
    };
    let Some(d) = s.demo.as_mut() else {
        return PlayerInput::default();
    };
    if d.since == 0.0 {
        d.since = now;
        d.check = (now, p);
    }
    let step = d.step;
    let route = ddstory::route::opening();
    let place = |name: &str| -> Option<Vec3> {
        let w = route.iter().find(|w| w.name == name)?;
        let st = &s.host.story.as_ref()?.stage;
        w.place(&st.scenarios).map(|(p, _)| Vec3::from(p))
    };
    let target = match step {
        Step::WeaponTable => place("cassardis: weapon table"),
        Step::RoomExit => place("cassardis: room exit"),
        Step::TalkQuina => s.npcs.get(&2).map(|n| n.1),
        Step::WalkOut => place("cassardis: 1300"),
        Step::Q0002Event => place("cassardis: q0002 event"),
        Step::Done => None,
    };
    if (now % 2.0) < 0.04 {
        info!(
            "opening demo: {step:?} at {p:?}, target {target:?}, SceNo {}",
            s.world().sce_no
        );
    }
    let w = s.world().clone();
    let menu = s.world_menu();
    let talking = s.talk.is_some();
    let quests: Vec<u32> = s
        .host
        .story
        .as_ref()
        .unwrap()
        .quests
        .iter()
        .map(|q| q.number())
        .collect();
    let d = s.demo.as_mut().unwrap();
    let mut next = None;
    let mut input = PlayerInput::default();
    let mut face = None;
    while d.path.front().is_some_and(|waypoint| {
        Vec3::new(waypoint.x - p.x, 0.0, waypoint.z - p.z).length() < ddsim::BODY_RADIUS * 0.5
    }) {
        d.path.pop_front();
    }
    if let Some(t) = d.path.front().copied().or(target) {
        let flat = Vec3::new(t.x - p.x, 0.0, t.z - p.z);
        let reach = if !d.path.is_empty() {
            ddsim::BODY_RADIUS * 0.5
        } else if step == Step::TalkQuina {
            150.0
        } else {
            60.0
        };
        if flat.length() > reach {
            input.sim = Input {
                stick: [0.0, 1.0],
                walk: !d.path.is_empty() || (step == Step::TalkQuina && flat.length() < 300.0),
                camera_yaw: flat.x.atan2(flat.z),
                ..Default::default()
            };
            // Report a blocked route, never silently place the player
            // through geometry and call that a walked acceptance run.
            if now - d.check.0 > 3.0 {
                if (p - d.check.1).length() < 60.0 {
                    if let Some(goal) = target {
                        d.path = demo_path(&s.host, p, goal).into();
                        warn!("opening demo: blocked walking to {step:?}; collision planner found {} waypoints (no placement fallback)", d.path.len());
                    }
                }
                d.check = (now, p);
            }
        } else {
            input.sim = Input {
                camera_yaw: flat.x.atan2(flat.z),
                ..Default::default()
            };
            if step == Step::WeaponTable {
                d.press = !d.press;
                input.interact = d.press;
                face = route
                    .iter()
                    .find(|w| w.name == "cassardis: weapon table")
                    .and_then(|w| {
                        w.place(&s.host.story.as_ref()?.stage.scenarios)
                            .map(|(_, yaw)| yaw)
                    });
            }
        }
    }
    match step {
        Step::WeaponTable if menu.is_some() => {
            if now - d.last_talk > 1.5 {
                if d.last_talk == 0.0 {
                    d.last_talk = now;
                } else {
                    next = Some(Step::RoomExit);
                }
            }
        }
        Step::WeaponTable => {}
        Step::RoomExit if w.sce_no >= 1200 => next = Some(Step::TalkQuina),
        Step::TalkQuina => {
            if d.talked && !talking {
                next = Some(Step::WalkOut);
            }
        }
        Step::WalkOut if w.sce_no >= 1400 || quests.contains(&2) => next = Some(Step::Q0002Event),
        Step::WalkOut if w.sce_no >= 1300 && now - d.since > 6.0 => next = Some(Step::Q0002Event),
        Step::Q0002Event if quests.contains(&2) => next = Some(Step::Done),
        _ => {}
    }
    let talk_due = step == Step::TalkQuina
        && target.is_some_and(|t| (Vec3::new(t.x - p.x, 0.0, t.z - p.z)).length() <= 150.0)
        && now - d.last_talk > 1.5;
    if talk_due {
        d.last_talk = now;
    }
    let menu_due = step == Step::WeaponTable && menu.is_some() && next == Some(Step::RoomExit);
    if let Some(n) = next {
        info!(
            "opening demo: {step:?} done (SceNo {}, GSF 372 {}, GSF 381 {}, quests {quests:?}) -> {n:?}",
            w.sce_no,
            w.gsf.get(372),
            w.gsf.get(381)
        );
        d.step = n;
        d.path.clear();
        d.since = now;
        d.check = (now, p);
        d.last_talk = 0.0;
    }
    if let (Some(yaw), Some(pl)) = (face, s.host.sim.player_mut(0)) {
        // the demo turns the Arisen to the table (the scenario's angle
        // filter), as a player would with the stick
        pl.yaw = yaw;
    }
    if menu_due {
        close_menu(s, ddskill_fighter());
    }
    if talk_due {
        if s.talk.is_some() {
            if let Ok(o) = s.host.talk_next(0) {
                s.show_talk(&o.events);
            }
        } else if let Some(d) = s.demo.as_mut() {
            if !d.talked {
                d.talked = true;
                match s.host.talk(0, 2) {
                    Ok(o) => {
                        s.show_talk(&o.events);
                        s.shot("5-talk-quina");
                    }
                    Err(e) => warn!("demo: talk: {e}"),
                }
            }
        }
    }
    match next {
        Some(Step::TalkQuina) => s.shot("4-control"),
        Some(Step::Done) => {
            let w = s.world();
            info!(
                "opening demo: done: SceNo {}, GSF 372 {}, GSF 381 {}, q0001 bits {:08x?}, q0002 booted {}",
                w.sce_no,
                w.gsf.get(372),
                w.gsf.get(381),
                w.quest_bits(1).map(|b| b.words()),
                quests.contains(&2)
            );
            let valid = w.sce_no == 1400
                && w.gsf.get(372)
                && w.gsf.get(381)
                && quests.contains(&2)
                && w.quest_bits(1)
                    .is_some_and(|bits| bits.words() == [0x18000000, 0, 3]);
            if !valid {
                s.failure = Some("opening endpoint flag/SceNo check failed".into());
            }
            s.shot("7-q0002");
            s.quit_at = s.shots.as_ref().map(|_| now + 3.0);
        }
        _ => {}
    }
    input
}

impl Session {
    fn network(&mut self, args: &crate::Args) -> Result<(), String> {
        if args.join.is_some() {
            let mut guest = crate::net::take_prejoined().ok_or("opening guest did not complete prejoin")?;
            let vfs = ddvfs::Vfs::open(&args.native_pc)?;
            guest.set_ground(Box::new(ddhost::install::stage_collision(&vfs, STAGE)?));
            self.guest = Some(guest);
            self.demo = None;
        } else if let Some(port) = args.host {
            let udp = ddnet::Udp::bind(&format!("0.0.0.0:{port}")).map_err(|e| e.to_string())?;
            let p = self.host.sim.player(0).ok_or("missing host seat")?;
            let spot = ddnet::Spot {
                stage: STAGE,
                cell: (50, 50),
                pos: p.position,
                yaw: p.yaw,
            };
            let config = ddnet::ServerConfig {
                build: 1_513_180_209,
                data: ddnet::data_fingerprint(&args.native_pc).map_err(|e| e.to_string())?,
                spawn: spot.pos,
                spot: Some(spot),
            };
            let host = *self.host.local.take().ok_or("already hosting")?;
            let mut server = ddnet::Server::new(host, udp, config);
            server.wait_for_events = true;
            self.host.server = Some(Box::new(server));
            info!("opening: hosting on {port}");
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn guest_step(
    s: &mut Session,
    st: &mut PlayState,
    keys: &ButtonInput<KeyCode>,
    buttons: &ButtonInput<MouseButton>,
    mouse: &AccumulatedMouseMotion,
    dt: f32,
    paused: bool,
    mouse_look: bool,
    now: u64,
    roots: &mut Query<(&mut Transform, &mut Playback), Without<FollowCam>>,
    cams: &mut Query<&mut Transform, With<FollowCam>>,
    vis: &mut Query<&mut Visibility, Without<Actor>>,
) {
    if !paused && mouse_look {
        play::orbit(st, buttons, mouse);
    }
    let input = PlayerInput {
        sim: if paused || s.cut.is_some() {
            Input::default()
        } else {
            play::p1_input(keys, buttons, st.cam_yaw)
        },
        interact: !paused && keys.just_pressed(KeyCode::KeyE),
    };
    let c = s.guest.as_mut().unwrap();
    c.pump(now);
    if paused || s.cut.is_some() {
        s.pending_input = PlayerInput::default();
    }
    s.pending_input.sim = play::latch_input(s.pending_input.sim, input.sim);
    s.pending_input.interact |= input.interact;
    let input = s.pending_input;
    s.acc = (s.acc + dt).min(0.25);
    while s.acc >= FrameClock::default().seconds() {
        s.acc -= FrameClock::default().seconds();
        c.frame(input, now);
        s.pending_input = PlayerInput::default();
    }
    if let Some(seat) = c.seat {
        st.follow = seat;
    }
    let latest = c.latest().map_or(0, |snap| snap.frame);
    let events: Vec<_> = c.events.drain(..).collect();
    let shown = |b: &ddnet::Body| Shown {
        id: b.id,
        position: b.position,
        yaw: b.yaw,
        state: b.state,
        frame: b.frame,
        weapon: ddsim::Weapon::None,
    };
    let mut bodies: Vec<_> = c.others(0.0).iter().map(shown).collect();
    bodies.extend(c.own().as_ref().map(shown));
    s.guest_npcs = c.npcs().into_iter().map(|n| (n.id, n.pos)).collect();
    for event in events {
        if let ddnet::proto::ToClient::PlayEvent { row, frame } = event {
            start_cut(s, row as usize);
            if let Some(cut) = &mut s.cut {
                // Catch up a delayed start or late join to the last received host frame.
                for _ in 0..latest.saturating_sub(frame).min(cut.player.end_frame as u64) {
                    if let Ok(f) = cut.player.step(1.0) {
                        cut.last = f;
                    }
                }
            }
        }
    }
    let follow = st.follow;
    if s.cut.is_some() {
        st.follow = u8::MAX;
    }
    play::show(st, &bodies, roots, cams);
    st.follow = follow;
    for (&e, &present) in &st.visible {
        if let Ok(mut v) = vis.get_mut(e) {
            *v = if present && s.cut.is_none() {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
    if s.guest_control_shot {
        info!(
            "opening guest control: {} player bodies, {} NPC snapshots",
            bodies.len(),
            s.guest_npcs.len()
        );
        s.shot("guest-control");
        s.guest_control_shot = false;
    }
    for (&id, &(entity, _, _)) in &s.npcs {
        let at = s.guest_npcs.get(&id);
        if let (Some(at), Ok((mut t, _))) = (at, roots.get_mut(entity)) {
            t.translation = Vec3::from(*at) * UNIT;
        }
        if let Ok(mut v) = vis.get_mut(entity) {
            *v = if at.is_some() && s.cut.is_none() {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
}

/// Demo navigation only, not a translated NPC/pathfinding rule. An A* search
/// samples capsule sweeps against the installed collision mesh. Resolution
/// is the existing player radius. The budget bounds demo work; a failed
/// search leaves the player where collision put it, never warps it.
fn demo_path(host: &StoryHost, from: Vec3, target: Vec3) -> Vec<Vec3> {
    plan_path(host.ground.as_ref(), &host.sim.sweep_param, from, target)
}
#[derive(Default)]
pub(crate) struct GameplayProjection(Option<Projection>);
impl GameplayProjection {
    fn capture(&mut self, projection: &Projection) {
        self.0.get_or_insert_with(|| projection.clone());
    }
    fn restore(&mut self, projection: &mut Projection) -> bool {
        if let Some(saved) = self.0.take() {
            *projection = saved;
            true
        } else {
            false
        }
    }
}

fn plan_path(
    ground: &dyn ddsim::Ground,
    param: &ddsim::sweep::SweepParam,
    from: Vec3,
    target: Vec3,
) -> Vec<Vec3> {
    use std::cmp::Reverse;
    use std::collections::{BinaryHeap, HashMap, HashSet};
    let distance = |p: Vec3| (p.x - target.x).hypot(p.z - target.z);
    let cell = ddsim::BODY_RADIUS;
    let mut open = BinaryHeap::from([(Reverse(distance(from) as i32), (0i32, 0i32))]);
    let mut nodes = HashMap::from([((0, 0), (from, 0.0f32, (0, 0)))]);
    let mut visited = HashSet::new();
    while let Some((_, key)) = open.pop() {
        if !visited.insert(key) {
            continue;
        }
        let (at, cost, _) = nodes[&key];
        if distance(at) < TALK_RANGE - 2.0 * ddsim::BODY_RADIUS {
            let mut path = vec![];
            let mut k = key;
            while k != (0, 0) {
                let (p, _, parent) = nodes[&k];
                path.push(p);
                k = parent;
            }
            path.reverse();
            return path;
        }
        if visited.len() > 20000 {
            break;
        }
        for dx in -1..=1i32 {
            for dz in -1..=1i32 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let next = (key.0 + dx, key.1 + dz);
                if visited.contains(&next) {
                    continue;
                }
                let mx = dx as f32 * cell;
                let mz = dz as f32 * cell;
                let hit = ddsim::sweep::sweep(
                    param,
                    &ddsim::sweep::PLAYER_CAPSULE,
                    at.to_array(),
                    [mx, -ddsim::GROUND_CHECK_DROP, mz],
                    true,
                    &|lo, hi, out| {
                        ground.triangles_in(lo, hi, out);
                    },
                );
                let mut p = Vec3::from(hit.position);
                if !hit.grounded() {
                    let Some(floor) = ground.height(p.x, p.z, p.y + ddsim::FALL_CHECK_UP) else {
                        continue;
                    };
                    if p.y - floor > ddsim::STEP_DOWN {
                        continue;
                    }
                    p.y = floor;
                }
                let moved = Vec3::new(p.x - at.x, 0.0, p.z - at.z);
                if (moved - Vec3::new(mx, 0.0, mz)).length() > cell * 0.5 {
                    continue;
                }
                let g = cost + moved.length();
                if nodes.get(&next).is_some_and(|(_, old, _)| *old <= g) {
                    continue;
                }
                nodes.insert(next, (p, g, key));
                open.push((Reverse((g + distance(p)) as i32), next));
            }
        }
    }
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cutscene_restores_gameplay_projection_after_multiple_frames() {
        let original = PerspectiveProjection {
            fov: 1.2,
            near: 0.05,
            far: 12000.0,
            aspect_ratio: 1.6,
            ..Default::default()
        };
        let mut projection = Projection::Perspective(original.clone());
        let mut saved = GameplayProjection::default();
        saved.capture(&projection);
        if let Projection::Perspective(event) = &mut projection {
            event.fov = 0.7;
            event.near = 16.0;
            event.far = 4_000_000.0;
            event.aspect_ratio = 1.8;
        }
        saved.capture(&projection);
        assert!(saved.restore(&mut projection));
        let Projection::Perspective(actual) = &projection else {
            panic!("perspective lost")
        };
        assert_eq!(actual.fov, original.fov);
        assert_eq!(actual.near, original.near);
        assert_eq!(actual.far, original.far);
        assert_eq!(actual.aspect_ratio, original.aspect_ratio);
        assert!(!saved.restore(&mut projection));
    }
    #[test]
    fn demo_navigation_cassardis_door() {
        let native = std::path::Path::new(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC");
        if !native.exists() {
            return;
        }
        let vfs = ddvfs::Vfs::open(native).unwrap();
        let ground = ddhost::install::stage_collision(&vfs, STAGE).unwrap();
        let path = plan_path(
            &ground,
            &ddsim::sweep::SweepParam::default(),
            Vec3::new(8024.744, 1731.1794, -7044.8994),
            Vec3::new(8231.768, 1708.3849, -6733.37),
        );
        assert!(!path.is_empty(), "walk around recorded demo blockage");
    }
    #[test]
    fn demo_navigation_cassardis_village() {
        let native = std::path::Path::new(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC");
        if !native.exists() {
            return;
        }
        let vfs = ddvfs::Vfs::open(native).unwrap();
        let ground = ddhost::install::stage_collision(&vfs, STAGE).unwrap();
        let path = plan_path(
            &ground,
            &ddsim::sweep::SweepParam::default(),
            Vec3::new(2154.6106, -74.36087, -1563.7985),
            Vec3::new(-879.3919, -8.177856, 250.40385),
        );
        assert!(!path.is_empty(), "walk around recorded village blockage");
    }
    #[test]
    fn demo_navigation_walks_around_a_wall_without_placement() {
        let mut ground = ddworld::Terrain::empty();
        ground.add([
            [-1000.0, 0.0, -1000.0],
            [-1000.0, 0.0, 1000.0],
            [1000.0, 0.0, 1000.0],
        ]);
        ground.add([
            [-1000.0, 0.0, -1000.0],
            [1000.0, 0.0, 1000.0],
            [1000.0, 0.0, -1000.0],
        ]);
        ground.add([
            [100.0, 0.0, -150.0],
            [100.0, 1000.0, -150.0],
            [100.0, 1000.0, 150.0],
        ]);
        ground.add([[100.0, 0.0, -150.0], [100.0, 1000.0, 150.0], [100.0, 0.0, 150.0]]);
        let path = plan_path(
            &ground,
            &ddsim::sweep::SweepParam::default(),
            Vec3::ZERO,
            Vec3::new(400.0, 0.0, 0.0),
        );
        assert!(!path.is_empty());
        assert!(path.iter().any(|p| p.z.abs() > 150.0));
        assert!((path.last().unwrap().x - 400.0).hypot(path.last().unwrap().z) < 150.0);
    }
}
