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
//!   installed uGUIJobChange list, Fighter, Strider or Mage; the host then gives the
//!   Arisen that vocation's fight kit (`Host::apply_job_kit`);
//! - **talks** (`Host::talk`): E next to an NPC, lines as subtitles, E for
//!   the next line, digits for choices;
//! - **NPCs** admitted from loaded, gated Cassardis layouts, including
//!   owners whose daily graph is suspended by an authored action. Place
//!   requests update host positions; Goto awaits a headless mover and Guard
//!   awaits actor arbitration/cleanup observations. Neither is auto-completed.
//!
//! The authored route passes SceNo 1300, `bootEv_st200ev15`, event st200
//! ev15 (SceNo 1400), and the NPC script that sets GSF 381 before q0002.
//! Real NPC prerequisites may currently stop that route; this adapter does
//! not synthesize actor completion to satisfy the demo.
//!
//! `--demo` plays the slice by itself (weapon table, room exit, ev10, a
//! talk with Quina, the walk out, ev15, q0002) and logs the flag and SceNo
//! checks; `--shots <dir>` saves a screenshot at each step and exits at
//! the end.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

#[path = "npc_motion.rs"]
mod npc_motion;
#[path = "guest_npc.rs"]
mod guest_npc;
pub(crate) use guest_npc::GaugeActor;

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

#[path = "dragon.rs"]
mod dragon_view;

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
    player: ddsim::PlayerId,
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
    Gate,
    FieldWalk,
    FieldSettle,
    /// After the performance settle, actually enter the installed roadside
    /// scenario. Enemies may have run outside its Area before dying.
    RescueApproach,
    /// `DDVIEW_DEMO_FIGHT`: after the settle, fight the field's goblins
    /// with ordinary inputs (play::demo_attack) and log the quest state.
    Rescue,
    /// Ordinary talk only after the retained actor reaches its authored
    /// talking loop. QCT text completion gives q2:88; no demo flag write.
    ThankReynard,
    ReynardRepeat,
    /// After the rescue's outcome (q2:20 or q2:30): walk the road toward
    /// the authored `q0002_event2` volume, which sets q2:10 and boots
    /// q0003 (docs/tasks/q0002-roadside.md, sequence 5).
    RoadWalk,
    Done,
}

struct Demo {
    step: Step,
    since: f32,
    step_origin: Vec3,
    last_door_press: f32,
    /// Where the Arisen was at the last progress check.
    check: (f32, Vec3),
    path: std::collections::VecDeque<Vec3>,
    press: bool,
    last_talk: f32,
    talked: bool,
    perf_route: bool,
    field_cell: Option<(i32, i32)>,
    planning: Option<RoutePlan>,
    /// When the fight demo last used a curative.
    last_heal: f32,
    /// Damage dealt to NPC combat bodies, by (victim, attacker) owner:
    /// (hits, valued total). Diagnostic for the rescue log.
    npc_damage: std::collections::BTreeMap<(u32, u32), (u32, f32)>,
    /// Started from a route checkpoint (S46): no Gate entry cell recorded.
    resumed: bool,
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

/// Loaded NPC layouts and real actor requests, independent of graph-map membership.
struct NpcAdapter {
    exe: ddexe::ExeFile,
    combat_body: Option<std::sync::Arc<ddhost::ddenemyrt::npc_body::NpcBody>>,
    layouts: HashMap<u32, ddhost::npc_placements::LoadedLayouts>,
    field_groups: Option<ddhost::ddspawn::gpl::GroupList>,
    field_cells: Option<Vec<(u16, u16)>>,
    families: ddhost::npc_placements::NameFamilies,
    guards: HashMap<u32, ddnpcrt::schedule::Guard>,
    goto: HashMap<u32, [f32; 3]>,
    placed: HashMap<u32, ([f32; 3], Option<[f32; 3]>)>,
    owners: HashMap<u32, ddhost::story_npcs::NpcLifetime>,
    bindings: HashMap<u32, ddhost::story_npcs::NpcLifetime>,
    reported_stage: u32,
    /// MultiNpc placements already reported (once each).
    multi_reported: std::collections::HashSet<(u32, u32, u32)>,
}

impl NpcAdapter {
    fn cassardis(vfs: &ddvfs::Vfs, exe: &std::path::Path) -> Result<Self, String> {
        let file = ddexe::ExeFile::open(exe).map_err(|error| error.0)?;
        let families = ddhost::npc_placements::NameFamilies::from_exe(&file.image().map_err(|e| e.0)?)
            .map_err(|error| error.0)?;
        let list = ddhost::npc_placements::group_list(vfs, STAGE)?;
        // Cassardis is loaded as a whole stage by opening::setup. Field
        // layouts are parsed separately from the streamer's completed cell set.
        let mut cells: Vec<_> = list.groups.iter().flat_map(|g| g.cells.iter().map(|c| (c.m, c.n))).collect();
        cells.sort(); cells.dedup();
        let layouts = ddhost::npc_placements::LoadedLayouts::read(vfs, STAGE, &list, &cells)?;
        Ok(Self { exe: file, combat_body: None,
            layouts: HashMap::from([(STAGE, layouts)]), field_groups: None, field_cells: None,
            families, guards: HashMap::new(), goto: HashMap::new(),
            placed: HashMap::new(), owners: HashMap::new(), bindings: HashMap::new(), reported_stage: STAGE,
            multi_reported: Default::default() })
    }

    /// Parse NPC archives only after the streamer's layout set completes.
    /// LoadedLayouts applies the existing 0x0075faa0 gates; parsing a layout
    /// does not acknowledge any actor's Guard/Goto or model readiness.
    fn sync_field_layouts(
        &mut self, vfs: &ddvfs::Vfs, stage: u32, cells: &[(i32, i32)],
    ) -> Result<bool, String> {
        if stage != 100 {
            let changed = self.field_cells.take().is_some();
            self.layouts.remove(&100);
            return Ok(changed);
        }
        let mut loaded = cells.iter().map(|&(m, n)| {
            Ok((u16::try_from(m).map_err(|_| format!("invalid field layout M {m}"))?,
                u16::try_from(n).map_err(|_| format!("invalid field layout N {n}"))?))
        }).collect::<Result<Vec<_>, String>>()?;
        // Stage bootstrap 0x004a21b0, NPC loop 0x004a277d..0x004a27cb,
        // loads authored (0,0) groups through 0x004a37a0(type=3), outside
        // the split-grid window. Their intels remain until stage unload;
        // individual physical actor readiness is still a separate system.
        loaded.push((0, 0));
        loaded.sort_unstable();
        loaded.dedup();
        if self.field_cells.as_ref() == Some(&loaded) { return Ok(false); }
        if self.field_groups.is_none() {
            self.field_groups = Some(ddhost::npc_placements::group_list(vfs, 100)?);
        }
        let layouts = ddhost::npc_placements::LoadedLayouts::read(
            vfs, 100, self.field_groups.as_ref().unwrap(), &loaded,
        )?;
        self.layouts.insert(100, layouts);
        self.field_cells = Some(loaded);
        Ok(true)
    }
}

/// The session (host frame stepping, the cutscene, the talk, the demo).
pub struct Session {
    dragon_view: Option<dragon_view::DragonView>,
    /// Explicit S29 actor capture fixture, never a claimed opening route.
    dragon_capture: bool,
    /// Authored event movie currently awaited by the story (if intro enabled).
    pub intro_movie: Option<PathBuf>,
    pub host: HostHandle,
    vocation_menu: crate::vocation_menu::Selection,
    guest: Option<ddnet::Client<ddnet::Udp>>,
    network_ready: bool,
    pending_input: PlayerInput,
    pending_local: HashMap<ddsim::PlayerId, Input>,
    failure: Option<String>,
    guest_control_shot: bool,
    guest_npcs: HashMap<u32, ddnet::npc::NpcBody>,
    guest_npc_bodies: guest_npc::Bodies,
    guest_enemy_views: crate::net::enemy_views::Views,
    acc: f32,
    /// The party before the last host frame, to draw between frames.
    between: play::Between,
    cut: Option<Cut>,
    talk: Option<TalkView>,
    /// Start positions of the stage: position, angle, member offsets.
    starts: Vec<([f32; 3], f32, Vec<[f32; 3]>)>,
    /// NPC entities and where the view shows them.
    npcs: HashMap<u32, (Entity, Vec3, f32)>,
    npc_motions: (Option<mtlmt::Motion>, Option<mtlmt::Motion>),
    npc_moving: HashMap<u32, bool>,
    npc_authored_motion: npc_motion::View,
    npc_adapter: NpcAdapter,
    demo: Option<Demo>,
    /// `DDVIEW_OPENING_FROM`: a route checkpoint (Host::save) to load once
    /// the opening is up, and `DDVIEW_OPENING_STEP` the demo step to resume.
    resume: Option<PathBuf>,
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
    /// Each seat's look, by seat id (setup's party looks; weapons follow the
    /// vocation kit, [`kit_weapons`]).
    party_looks: Vec<Option<ddlook::Look>>,
    /// The vocation whose starting weapons the Arisen show (0: none yet).
    weapons_job: u32,
    /// The stage whose enemy groups [`field_enemies`] spawned, its group
    /// list and the cells done.
    enemy_stage: u32,
    enemy_groups: Option<ddhost::ddspawn::gpl::GroupList>,
    enemy_cells: std::collections::HashSet<(u16, u16)>,
    /// Placements spawned this stage visit (group, record id) and the story
    /// world the gates last saw.
    enemy_spawned: std::collections::HashSet<(u32, i32)>,
    enemy_world: Option<(u32, u32, Option<ddquest::story::MsbBits>)>,
    /// NPC entities to hide for good.
    gone: Vec<Entity>,
}

#[derive(Resource)]
pub struct OpeningRes(pub Mutex<Session>);

impl OpeningRes {
    pub(crate) fn item_menu_snapshot(&self, player: ddsim::PlayerId) -> Option<crate::item_menu::Snapshot> {
        let session = self.0.lock().unwrap();
        // Guest inventory requests are not in the LAN protocol. Events and
        // story-owned menus retain priority over local inventory UI.
        if session.guest.is_some() || session.cut.is_some() || session.talk.is_some() || session.world_menu().is_some()
            || ddhost::StoryWorld::player_actions_blocked(&session.host.world, player) { return None; }
        session.host.roster.seat(player)?;
        // Co-op policy: this Arisen and the session's active pawn bags, never
        // another Arisen. Native 0x00669990 requires a member's active actor;
        // its additional record+0x354 admission flag remains unported.
        let bags = session.host.item_recipients().into_iter().filter(|r| match r.owner {
            ddeconomy::inventory::Owner::Arisen(p) => p == player,
            ddeconomy::inventory::Owner::Pawn(_) => session.host.sim.player(r.seat).is_some()
                && session.host.fight.as_ref().is_some_and(|f| f.arisen.contains_key(&r.seat)),
            _ => false,
        }).filter_map(|r| Some(crate::item_menu::BagView {
            owner:r.owner, bag:session.host.economy.inventory.bag(r.owner)?.clone(),
        })).collect::<Vec<_>>();
        (!bags.is_empty()).then_some(crate::item_menu::Snapshot { bags })
    }

    pub(crate) fn item_menu_use(&self, player: ddsim::PlayerId, bag: ddeconomy::inventory::Owner, row: &crate::item_menu::Entry) -> Result<(), String> {
        let mut session = self.0.lock().unwrap();
        if session.guest.is_some() || session.cut.is_some() || session.talk.is_some() || session.world_menu().is_some() { return Err("item menu is unavailable".into()); }
        let item = session.host.item_request(player, ddhost::items::ItemRequest::Use {
            owner:bag,index:row.index,expected:row.slot,
        })?;
        info!("opening item menu: seat {player}, bag {bag:?}, used item {item}");
        Ok(())
    }

    pub(crate) fn vocation_snapshot(&self) -> Option<(usize, usize, bool, bool)> {
        let session = self.0.lock().unwrap();
        let (_, owner) = session.world_menu().filter(|(menu, _)| *menu == Menu::Vocation)?;
        // The host may request the menu during this frame's tick, after
        // input was sampled. Draw its native initial row immediately so a
        // same-frame screenshot doesn't miss the newly opened UI.
        Some(session.vocation_menu.snapshot().unwrap_or((owner,0,false,true)))
    }
}

/// Walk the stand-in prologue to an awaited movie, or q0001 when intro is bypassed.
pub fn prepare(
    native_pc: &std::path::Path,
    exe: &std::path::Path,
    players: u8,
    demo: bool,
    shots: Option<PathBuf>,
    intro: bool,
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
    // The fight (ddenemyrt): the Arisen gets its kit when the vocation is
    // picked at the weapon table (Host::apply_job_kit); enemies join it as
    // the story spawns them.
    host.fight = Some(ddhost::ddenemyrt::fight::Fight::new(play::fight_data(&vfs, native_pc)?));
    // progression (experience, levels): ddprogress tables from the install
    // and the exe beside it
    host.data.tables = Some(std::sync::Arc::new(ddhost::install::tables(
        &vfs,
        &ddexe::locate(native_pc).map_err(|e| e.to_string())?,
    )?));
    host.data.npc = Some(std::sync::Arc::new(
        ddnpcrt::InstallData::open(native_pc, "eng").map_err(|e| e.to_string())?,
    ));
    for k in 0..players.max(1) {
        // The network prepare path seats only its local host. Additional
        // seats here are explicitly local; LAN peers join through Server.
        host.join(ProfileId(u64::from(k) + 1), Control::Local, ddhost::opening::AWAY, 0.0)
            .ok_or("cannot seat")?;
    }
    let movies = if intro {
        crate::movie::event_paths(&host.story.as_ref().unwrap().data)?
    } else { HashMap::new() };
    host.world.inner.play_rows.extend(movies.keys().copied());
    let route = to_newly_arisen();
    // Split the existing stand-in route, retaining its sourced scenario volumes.
    // The stop itself is the quest's PlayEvent request, never elapsed time.
    let route_end = if intro { route.iter().position(|w| w.stage == STAGE).unwrap_or(route.len()) } else { route.len() };
    let f = fast_forward(&mut host, &route[..route_end], 240, 30_000, |h| {
        if intro { h.world.inner.awaiting.is_some_and(|row| movies.contains_key(&row)) }
        else { h.story.as_ref().is_some_and(|s| s.quest(1).is_some()) }
    }).map_err(|e| e.to_string())?;
    let intro_movie = host.world.inner.awaiting.and_then(|row| movies.get(&row).cloned());
    if !intro {
        host.world.live(CASSARDIS_ROWS);
        host.world.manual_stages = true;
    }
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
    let npc_adapter = NpcAdapter::cassardis(&vfs, exe)?;
    // The preparation route can stop before the dragon challenge or after
    // the actual heart-taking event. Prologue consumables were erased.
    let bag_phase = if sce >= 1050 { ddeconomy::new_game::Phase::NewlyArisen }
        else if matches!(w.stage_no, 800 | 801) { ddeconomy::new_game::Phase::BeforeCreation }
        else { ddeconomy::new_game::Phase::DragonChallenge };
    host.new_game_items(std::sync::Arc::new(ddhost::items::ItemData::load(&vfs)?), bag_phase);
    let mut s = Session {
        intro_movie,
        host: HostHandle {
            local: Some(Box::new(host)),
            server: None,
        },
        vocation_menu: crate::vocation_menu::Selection::default(),
        guest: None,
        network_ready: false,
        pending_input: PlayerInput::default(),
        pending_local: HashMap::new(),
        failure: None,
        guest_control_shot: false,
        guest_npcs: HashMap::new(),
        guest_npc_bodies: guest_npc::Bodies::default(),
        guest_enemy_views: crate::net::enemy_views::Views::default(),
        acc: 0.0,
        between: play::Between::default(),
        cut: None,
        talk: None,
        starts,
        npcs: HashMap::new(),
        npc_motions: (None, None),
        npc_moving: HashMap::new(),
        npc_authored_motion: npc_motion::View::load(&vfs, exe)?,
        npc_adapter,
        resume: std::env::var_os("DDVIEW_OPENING_FROM").map(PathBuf::from),
        demo: demo.then_some(Demo {
            step: Step::WeaponTable,
            since: 0.0,
            step_origin: Vec3::ZERO,
            last_door_press: f32::NEG_INFINITY,
            check: (0.0, Vec3::ZERO),
            path: Default::default(),
            press: false,
            last_talk: 0.0,
            talked: false,
            perf_route: false,
            field_cell: None,
            planning: None,
            last_heal: f32::NEG_INFINITY,
            npc_damage: Default::default(),
            resumed: false,
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
        party_looks: vec![],
        weapons_job: 0,
        enemy_stage: 0,
        enemy_groups: None,
        enemy_cells: Default::default(),
        enemy_spawned: Default::default(),
        enemy_world: None,
        gone: vec![],
        dragon_view: None,
        dragon_capture: std::env::var_os("DDVIEW_DRAGON_ENTRANCE_CAPTURE").is_some(),
    };
    let start = f
        .arrival
        .filter(|(stage, _)| *stage == STAGE)
        .map_or(s.world().start_pos, |(_, n)| n);
    if !intro { s.place_party(start); }
    // S29 owns admission/body independently. S31 will enter the live
    // authored scene instead of the stand-in fast-forward above.
    let exe_file=ddexe::ExeFile::next_to(native_pc).map_err(|e|e.to_string())?;
    s.host.set_opening_dragon_data(ddhost::dragon::OpeningDragonData::load(&vfs,
        &exe_file.image().map_err(|e|e.to_string())?)?);
    if s.dragon_capture {
        s.demo=None;
        s.place_party(3);
        let story=s.host.story.as_mut().unwrap();
        // Presentation fixture starts at the documented post-ev00 gate;
        // actual story sequencing is S31. Stop its unrelated q1 scripts.
        story.quests.clear();
        story.tasks.clear();
        story.stage.scenarios=Default::default();
        story.stage.event=Default::default();
        s.host.world.enable_live_encounters();
        let story=s.host.story.as_mut().unwrap();
        story.world.set_sce_no(1025,false);
        story.world.set_lot(200,0,true);
        info!("S29 isolated dragon capture: installed placement/FSM, real host phases; story route bypassed explicitly");
        if std::env::var_os("DDVIEW_LAN_ENEMY_CAPTURE").is_some() {
            // S33 visual fixture only: synthetic goblin position six metres
            // in front of this runtime Arisen, not a natural beach encounter.
            let player=s.host.sim.player(0).unwrap();
            let pos=[player.position[0]+player.yaw.sin()*600.0,player.position[1],
                player.position[2]+player.yaw.cos()*600.0];
            s.host.fight.as_mut().unwrap().add_goblin(pos,0.0);
        }
    }
    Ok(s)
}

impl Session {
    /// S34 ordinary NPC304 adapter. Admission alone cannot create a
    /// combat actor: this receives only successfully materialized bodies
    /// and waits for the real canonical motion binding.
    fn bind_npc_combat(&mut self, vfs: &ddvfs::Vfs, bodies: &[u32]) -> Result<(), String> {
        use ddhost::ddenemyrt::{body::MotionId, npc_body::NpcBody, npc_combat};
        if !bodies.contains(&304) { return Ok(()); }
        let Some(owner) = self.host.story_npc_lifetime(304) else { return Ok(()) };
        if self.host.story_npc_combat_owner(owner).is_some() { return Ok(()); }
        let Some(actor) = self.host.story_npc_motion(304).filter(|a| a.token.owner == owner) else { return Ok(()) };
        let motion = MotionId::new((actor.motion >> 8) as u8, actor.motion & 255);
        if self.npc_adapter.combat_body.is_none() {
            // Same installed Look id/unit/row as npc_look above. Specialized
            // NPC types/common99 and other NPC combat bodies are not bound.
            self.npc_adapter.combat_body = Some(std::sync::Arc::new(NpcBody::load(vfs,
                &self.npc_adapter.exe.image().map_err(|e| e.0)?, 304, 304, 0, 0, 0)?));
        }
        let body = self.npc_adapter.combat_body.as_ref().unwrap().clone();
        let fight = self.host.fight.as_ref().ok_or("NPC combat needs the host fight")?;
        let region = npc_combat::ordinary_region(vfs, &fight.data.tables.game, npc_combat::ORDINARY_NPC_HP)?;
        let bound = self.host.bind_story_npc_combat(owner, body, region, motion).map_err(|e| e.to_string())?;
        if let Some(combat_owner) = bound {
            info!("opening: NPC304 lifetime {} materialized combat owner {combat_owner}", owner.generation);
        }
        Ok(())
    }

    /// Finish the actual awaited movie, then wait for the quest's editor jump.
    pub fn intro_movie_done(&mut self) -> Result<(), String> {
        if self.intro_movie.is_none() { return Err("no intro movie awaiting completion".into()); }
        self.host.world.inner.awaiting.take().ok_or("no movie event awaited")?;
        self.host.world.event_done();
        fast_forward(&mut self.host, &[], 240, 30_000, |h| {
            h.story.as_ref().is_some_and(|s| s.world.stage_no == 801)
        }).map_err(|e| e.to_string())?;
        self.intro_movie = None;
        info!("opening: movie EventFinished -> authored editor stage 801");
        Ok(())
    }

    /// Editor confirmation resumes the remaining stand-in dragon route.
    pub fn intro_edit_done(&mut self) -> Result<(), String> {
        if self.world().stage_no != 801 { return Err("editor confirmation outside stage 801".into()); }
        self.host.character_created_items();
        let route: Vec<_> = to_newly_arisen().into_iter().filter(|w| w.stage == STAGE).collect();
        let f = fast_forward(&mut self.host, &route, 240, 30_000, |h| {
            h.story.as_ref().is_some_and(|s| s.quest(1).is_some())
        }).map_err(|e| e.to_string())?;
        // The existing preparation route can bypass ev01 headless. Apply
        // only its completed boundary; a live challenge leaves this pending.
        if self.world().sce_no >= 1050 { self.host.heart_taken_items(); }
        self.host.world.live(CASSARDIS_ROWS);
        self.host.world.manual_stages = true;
        self.last_sce = self.world().sce_no;
        let start = f.arrival.filter(|(stage, _)| *stage == STAGE)
            .map_or(self.world().start_pos, |(_, n)| n);
        self.place_party(start);
        Ok(())
    }

    fn sync_npc_intels(&mut self) -> Result<(), String> {
        let stage = self.world().stage_no;
        // Retire old-stage owners before admitting the new stage, including
        // any NPC id reused across the handoff. Do not infer death on unload.
        let mut old_stages: Vec<_> = self.host.story_npc_intels()
            .filter(|p| p.stage != stage).map(|p| p.stage).collect();
        old_stages.sort_unstable(); old_stages.dedup();
        for old in old_stages {
            self.host.reconcile_story_npc_intels(old, vec![]).map_err(|e| e.to_string())?;
        }
        let Some(layouts) = self.npc_adapter.layouts.get(&stage) else {
            if self.npc_adapter.reported_stage != stage {
                warn!("opening: stage {stage} NPC materialization waits for loaded-layout/archive readiness adapter");
                self.npc_adapter.reported_stage = stage;
            }
            // Stage unload is real; field materialization waits for its own
            // loaded-cell/archive adapter rather than keeping Cassardis actors.
            self.host.reconcile_story_npc_intels(STAGE, vec![]).map_err(|e| e.to_string())?;
            self.npc_adapter.guards.clear();
            self.npc_adapter.goto.clear();
            self.npc_adapter.placed.clear();
            self.npc_adapter.owners.clear();
            self.npc_adapter.bindings.clear();
            return Ok(());
        };
        let candidates = {
            let (scene, hour, flag) = ddhost::spawns::world_inputs(self.world(), stage);
            let world = ddhost::ddspawn::World { scene, hour, flag: &flag };
            let mut candidates = layouts.candidates(&world);
            for owner in self.host.story_npc_intels() {
                if layouts.retains_existing(&world, owner)
                    && !candidates.placements.iter().any(|placement| placement.target == owner.target)
                {
                    candidates.placements.push(owner.clone());
                }
            }
            candidates
        };
        let mut admitted = Vec::new();
        for placement in candidates.placements {
            match self.npc_adapter.families.select(&placement.transform.name) {
                None => continue,
                // A MultiNpc is an ordinary NPC intel (cLayoutSetNpc::vf15
                // 0x00762000 -> 0x007624D0 only sets intel+0x3D0) whose
                // body also takes record fields (cSetInfoNpc vf08
                // 0x007A3595..0x007A35F3 -> intel +0x444/+0x448/+0x44A/
                // +0x44C -> body +0x35A4/+0x6F40/+0x307C/+0x3070 in
                // 0x00BBE070) and which skips the +0x430 x 1000 HP setup
                // in uNpcIntel::vf05. Admitted as an NPC; those body
                // fields are not ported yet (reported once per placement).
                Some(true) => {
                    let t = placement.target;
                    if self.npc_adapter.multi_reported.insert((t.kind, t.group, t.index)) {
                        warn!(
                            "opening: MultiNpc placement admitted as an ordinary NPC (kind-specific body fields unported): stage {stage} group {} record {} npc {} mMultiNpcKind {:?}",
                            t.group, t.index, placement.npc, placement.record.get("mMultiNpcKind")
                        );
                    }
                    admitted.push(placement)
                }
                Some(false) => admitted.push(placement),
            }
        }
        // 0x004a76a0 has 200 slots. This stage's port owns only NPC intels;
        // the bounded fresh-slot path is supported, priority eviction is not.
        if admitted.len() > 200 { return Err("NPC intel pool priority eviction not implemented".into()); }
        for (target, diagnostic) in self.host.reconcile_story_npc_intels(stage, admitted).map_err(|e| e.to_string())? {
            warn!("opening: NPC placement {target:?}: {diagnostic}");
        }
        let owned: HashMap<_, _> = self.host.story_npc_intels()
            .filter_map(|p| self.host.story_npc_lifetime(p.npc as u32).map(|owner| (owner.npc, owner))).collect();
        if stage == 100 && owned != self.npc_adapter.owners {
            let admitted: Vec<_> = self.host.story_npc_intels()
                .map(|p| (p.npc, p.target.group, p.target.index)).collect();
            info!("opening: field NPC intels admitted {admitted:?}");
        }
        // Request ownership is independent of successful model creation.
        for (&npc, old) in &self.npc_adapter.owners {
            if owned.get(&npc) != Some(old) {
                self.npc_adapter.guards.remove(&npc);
                self.npc_adapter.goto.remove(&npc);
                self.npc_adapter.placed.remove(&npc);
            }
        }
        self.npc_adapter.owners = owned;
        Ok(())
    }

    fn npc_requests(&mut self, requests: &[ddnpcrt::schedule::Request]) {
        use ddnpcrt::schedule::Request as NpcRequest;
        for request in requests {
            match request {
                NpcRequest::Place { npc, pos, dir } => {
                    self.npc_adapter.placed.insert(*npc, (*pos, *dir));
                    self.npc_adapter.goto.remove(npc);
                }
                NpcRequest::Goto { npc, pos, .. } => {
                    self.npc_adapter.goto.insert(*npc, *pos);
                    // Keep the actual request pending. The existing visual-only
                    // 150cm/s interpolation cannot acknowledge a host arrival.
                    warn!("opening: NPC {npc} awaits headless movement to {pos:?}");
                }
                NpcRequest::Guard { npc, config } => { self.npc_adapter.guards.insert(*npc, config.clone()); }
                NpcRequest::Wait { npc, dir: Some(dir), .. } => {
                    if let Some((_, _, yaw)) = self.npcs.get_mut(npc) { *yaw = dir[0].atan2(dir[2]); }
                }
                _ => {}
            }
        }
    }

    /// Viewer benchmark driver only: uses normal player inputs and actual
    /// stage callbacks, not scripted placement or quest completion.
    pub fn enable_perf_route(&mut self) {
        if let Some(demo) = &mut self.demo {
            demo.perf_route = true;
        }
        // explicit --shots are kept: a route with screenshots is a labelled
        // visual capture (main.rs), not a timing benchmark
    }
    /// Both endpoints supply the same installed gauge pose renderer.
    pub(crate) fn npc_gauge_actors(&self) -> Vec<GaugeActor> {
        if self.guest.is_some() { return self.guest_npc_bodies.gauges(&self.guest_npcs); }
        let Some(fight) = self.host.fight.as_ref() else { return vec![] };
        fight.npcs.values().filter_map(|npc| {
            if fight.npc_errors.contains_key(&npc.fighter.owner)
                || self.host.story_npc_gauge_visible(npc.npc) != Some(true) { return None; }
            let ddhost::ddenemyrt::combat::Victim::Npc { region, .. } = &npc.fighter.victim else { return None };
            if region.hp <= 0.0 { return None; }
            Some(GaugeActor { owner: npc.fighter.owner, npc: npc.npc, body: npc.body.clone(),
                motion: npc.motion, frame: npc.frame, position: npc.position, yaw: npc.yaw,
                scale: npc.scale, hp: region.hp, hp_max: region.hp_max })
        }).collect()
    }
    /// The rendered body entity, for gauge anchor telemetry.
    pub(crate) fn npc_body_entity(&self, npc: u32) -> Option<Entity> {
        self.npcs.get(&npc).map(|n| n.0)
    }
    pub(crate) fn door_npc_coverage(&self) -> bool {
        let stage=self.world().stage_no;
        let Some(layouts)=self.npc_adapter.layouts.get(&stage) else {return false;};
        let (scene,hour,flag)=ddhost::spawns::world_inputs(self.world(),stage);
        let candidates=layouts.candidates(&ddhost::ddspawn::World{scene,hour,flag:&flag});
        !candidates.gates.iter().any(|(_,gate)|matches!(gate,ddhost::ddspawn::Gate::Unported(_)))
            && candidates.placements.iter().all(|p|
                self.npc_adapter.families.select(&p.transform.name)==Some(false)
                    && self.host.story_npc_position(p.npc as u32).is_some())
    }
    /// Presentation query using the same rendered positions and range as E.
    pub fn can_talk(&self, seat: ddsim::PlayerId) -> bool {
        if self.cut.is_some() || self.talk.is_some() {
            return false;
        }
        let Some(player) = self.host.sim.player(seat) else { return false; };
        self.npcs.values().any(|(_, at, _)| {
            (at.x - player.position[0]).hypot(at.z - player.position[2]) <= TALK_RANGE
        })
    }

    fn talk_npc(&mut self, seat: ddsim::PlayerId, npc: u32) -> Result<ddhost::npcs::TalkOutcome, ddhost::HostError> {
        let owner = self.host.story_npc_lifetime(npc).ok_or(ddhost::HostError::Talk(ddnpcrt::TalkError::Silent))?;
        let body = self.npcs.get(&npc).copied();
        // Viewer adapter assumption: a successfully constructed idle body
        // with the current canonical lifetime is eligible in this supported
        // opening state. Full original cUnit status and character flags are
        // not yet simulated. This is explicit admission, never TalkEnd.
        let ready = body.is_some() && self.npc_adapter.bindings.get(&npc) == Some(&owner)
            && self.cut.is_none() && self.world_menu().is_none() && self.talk.is_none();
        // uNpc ctor0x00bab3a0 initializes mode3. Its readiness remains
        // unknown here, selecting the original manual Guard candidate path.
        let motion = ddnpcrt::guard::TalkMotionObservation { mode: Some(3), ..Default::default() };
        if self.host.story_npc_guard(npc).is_none() {
            return self.host.talk_story_npc(seat, owner, ready, motion)?
                .ok_or(ddhost::HostError::Talk(ddnpcrt::TalkError::Silent));
        }
        self.host.talk_story_guard(seat, owner, ready, body.map_or(0.0, |(_, _, yaw)| yaw), motion)?
            .ok_or(ddhost::HostError::Talk(ddnpcrt::TalkError::Silent))
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
        // the hired pawns travel with the party (else their bodies stay at the
        // old stage's coordinates and the field streams cells around them)
        self.host.place_hired_pawns();
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

    /// Viewer input policy: the chosen local seat supplies the position.
    fn npc_near_for(&self, seat: ddsim::PlayerId) -> Option<u32> {
        let p = self.host.sim.player(seat)?.position;
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
                    player,
                    npc,
                    text,
                    conversation,
                    index,
                    ..
                } => {
                    let line = clean(text);
                    info!("opening: NPC {npc} conversation {conversation} line {index}: {line}");
                    self.talk = Some(TalkView {
                        player: *player,
                        npc: *npc,
                        line,
                        choices: vec![],
                    });
                }
                TalkEvent::Choices { player, npc, options, .. } => {
                    let line = self.talk.as_ref().map(|t| t.line.clone()).unwrap_or_default();
                    self.talk = Some(TalkView {
                        player: *player,
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
                TalkEvent::QuestEffects { player, npc, effects } => {
                    for effect in effects {
                        if let ddnpcrt::quest_talk::Effect::Item { item, count, .. } = *effect {
                            info!("opening: NPC {npc} quest reward: seat {player} item {item} count {count}");
                        }
                    }
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

/// Keep rendered actor lifetimes aligned with actual materialized intels,
/// including actors whose normal graph is suspended by an authored action.
#[allow(clippy::too_many_arguments)]
fn sync_npc_models(
    session: &mut Session,
    commands: &mut Commands,
    loader: &mut Loader,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    bindposes: &mut Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>,
) {
    let placements: Vec<_> = session.host.story_npc_intels()
        .filter(|placement| !session.host.story_npc_body_retired(placement.npc as u32)).cloned().collect();
    let wanted: HashMap<_, _> = placements.iter().filter_map(|p|
        session.host.story_npc_lifetime(p.npc as u32).map(|owner| (owner.npc, owner))).collect();
    let retired: Vec<_> = session.npcs.keys().copied().filter(|id|
        !wanted.contains_key(id) || wanted.get(id) != session.npc_adapter.bindings.get(id)).collect();
    for id in retired {
        if let Some((entity, _, _)) = session.npcs.remove(&id) { commands.entity(entity).despawn(); }
        session.npc_adapter.bindings.remove(&id);
        session.npc_moving.remove(&id);
    }
    for placement in placements {
        let id = placement.npc as u32;
        if session.npcs.contains_key(&id) { continue; }
        let pos = Vec3::from(session.host.story_npc_position(id).unwrap_or(placement.transform.position));
        let yaw = placement.transform.angle[1]; // Same documented Euler-order limit as other layout adapters.
        let Some(look) = npc_look(loader.vfs, id, 0) else { continue; };
        match crate::character::spawn_look(commands, loader, meshes, materials, images,
            bindposes, &look, play::MOTIONS, 0, pos * UNIT, Some(yaw)) {
            Ok(entity) => {
                session.npcs.insert(id, (entity, pos, yaw));
                if let Some(&owner) = wanted.get(&id) { session.npc_adapter.bindings.insert(id, owner); }
            }
            Err(error) => warn!("opening: NPC {id} body unavailable: {error}"),
        }
    }
    let bodies: Vec<_> = session.npcs.keys().copied().collect();
    if let Err(error) = session.npc_authored_motion.bind(&mut *session.host, &bodies) {
        session.failure = Some(format!("NPC authored motion body: {error}"));
    }
    if let Err(error) = session.bind_npc_combat(loader.vfs, &bodies) {
        session.failure = Some(format!("NPC combat body: {error}"));
    }
}

/// Initial Arisen view bodies, with explicit LAN snapshot reservations.
fn initial_player_placements(
    roster: &ddhost::player::Roster,
    sim: &ddsim::Sim,
    network: bool,
) -> Vec<(u8, [f32; 3], f32)> {
    // Viewer policy: LAN reserves bodies for later snapshots. Local play
    // draws only occupied Arisen seats; capacity is not session membership.
    let ids: Vec<_> = if network {
        (0..ddhost::player::MAX_ARISEN as u8).collect()
    } else {
        roster.ids().collect()
    };
    ids.into_iter().filter_map(|id| {
        let p = sim.player(id).or_else(|| network.then(|| sim.player(0)).flatten())?;
        Some((id, p.position, p.yaw))
    }).collect()
}

fn complete_player_looks(
    vfs: &ddvfs::Vfs,
    seats: &[(u8, [f32; 3], f32)],
    party_looks: &mut Vec<Option<ddlook::Look>>,
) {
    for &(id, _, _) in seats {
        party_looks.resize_with(party_looks.len().max(usize::from(id) + 1), || None);
        if party_looks[usize::from(id)].is_none() {
            // The existing built-in pawn stand-in is a viewer choice, not
            // an NPC outfit. It also dresses a second local Arisen.
            party_looks[usize::from(id)] = ddlook::save::template_look(vfs, 0).ok();
        }
    }
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
    mut party_looks: Vec<Option<ddlook::Look>>,
    demo: bool,
    network: bool,
) -> Option<PlayState> {
    let mut s = res.0.lock().unwrap();
    crate::event::spawn_stage_models(commands, loader, meshes, materials, images, STAGE);
    let vfs = loader.vfs;
    // The Arisen: play.rs's characters at the seats.
    let seats = initial_player_placements(&s.host.roster, &s.host.sim, network);
    complete_player_looks(vfs, &seats, &mut party_looks);
    info!("opening: Arisen bodies for seats {:?}", seats.iter().map(|s| s.0).collect::<Vec<_>>());
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
    s.party_looks = party_looks.clone();
    s.sex = s.arisen_look.as_ref().map_or(0, |l| l.sex);
    let party = play::Party {
        arisen,
        pawn: None,
        goblins: 0,
        kits: HashMap::new(),
    };
    // The view's own copy of the stage collision: the follow camera pulls in
    // against it (play::follow_camera). The host keeps its own for the
    // simulation.
    let terrain = ddhost::install::stage_collision(vfs, STAGE).unwrap_or_else(|e| {
        warn!("opening: no collision for the camera: {e}");
        ddworld::Terrain::empty()
    });
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
    // Whole-stage data is now loaded; actor observations replace scripted
    // actor completion from this point onward.
    s.host.world.enable_live_encounters();
    if let Err(error) = crate::quest_camera::install(&mut s.host.world, vfs, STAGE) {
        s.failure = Some(format!("stage event camera load: {error}"));
    }
    if let Err(error) = s.sync_npc_intels() { s.failure = Some(error); }
    // NPCs with their looks, idle until their schedule moves them.
    let list = mtlmt::parse(&vfs.read(play::MOTIONS, crate::character::RMOTIONLIST).ok()?).ok()?;
    s.npc_motions = (
        list.motions.first().cloned().flatten(),
        list.motions.get(0x54).cloned().flatten(),
    );
    sync_npc_models(&mut s, commands, loader, meshes, materials, images, bindposes);
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

/// Loads `stage` into the view and the host (streamer, collision, quest
/// cameras, start positions), clears the previous stage's NPCs and reports
/// StageLoaded to the story: what a JumpArea does. `keep_places` keeps each
/// seat where it is (a loaded checkpoint) instead of placing the party at
/// start position `pos`.
fn enter_stage(
    stage_view: &mut OpeningStage,
    st: &mut PlayState,
    s: &mut Session,
    stage: u32,
    pos: u32,
    keep_places: bool,
) -> Result<(), String> {
    let kept: Vec<(ddsim::PlayerId, [f32; 3], f32)> = s.host.sim.players.iter()
        .map(|p| (p.id, p.position, p.yaw)).collect();
    let streamer = stage_view.streamer.as_mut().ok_or("opening stage streamer absent")?;
    let starts = ddhost::install::start_positions(streamer.loader.vfs, stage)?;
    let start = starts.entries.get(pos as usize).ok_or("stage start position absent")?;
    let focus = if keep_places {
        s.host.sim.player(0).map(|p| p.position).unwrap_or([start.position[0], start.position[1], start.position[2]])
    } else {
        [start.position[0], start.position[1], start.position[2]]
    };
    let terrain = streamer.enter(&mut stage_view.commands, stage_view.entities.iter(),
        &mut stage_view.meshes, &mut stage_view.materials, &mut stage_view.images,
        stage, focus)?;
    crate::quest_camera::install(&mut s.host.world, streamer.loader.vfs, stage)?;
    s.host.doors = None;
    st.door_collision = Default::default();
    st.terrain = terrain.clone();
    s.host.set_ground_with_doors(Box::new(terrain));
    s.starts = starts.entries.iter().map(|e| (
        [e.position[0], e.position[1], e.position[2]], e.angle,
        e.offset_positions.iter().map(|o| [o[0], o[1], o[2]]).collect(),
    )).collect();
    s.place_party(pos);
    if keep_places {
        for (id, position, yaw) in kept {
            if let Some(p) = s.host.sim.player_mut(id) {
                p.position = position;
                p.yaw = yaw;
                p.drop_from = position[1];
            }
        }
    }
    if let Some(server) = &mut s.host.server {
        if let Some(spot) = &mut server.config.spot {
            spot.stage = stage;
        }
    }
    s.between.snapshot(&s.host.sim);
    for (_, (entity, _, _)) in s.npcs.drain() {
        stage_view.commands.entity(entity).despawn();
    }
    s.npc_moving.clear();
    s.host.npcs.npcs.clear();
    s.host.npc_parked.clear();
    s.host.world.npcs.clear();
    s.host.world.report(ddstory::HostEvent::StageLoaded { stage });
    Ok(())
}

/// S46: saves the host as a route checkpoint `<dir>/<name>.ddhs` (the
/// original's save content: world, places, progression, economy, Rift,
/// NPC quest masks, running scripts). Refused while an interaction is open,
/// as in the original; logged, never fatal.
fn write_checkpoint(s: &Session, dir: &std::path::Path, name: &str) {
    let result = (|| -> Result<PathBuf, String> {
        let file = s.host.save().map_err(|e| e.to_string())?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!("{name}.ddhs"));
        std::fs::write(&path, file.to_bytes()).map_err(|e| e.to_string())?;
        Ok(path)
    })();
    match result {
        Ok(path) => info!("opening: checkpoint written {}", path.display()),
        Err(e) => warn!("opening: checkpoint {name} not written: {e}"),
    }
}

/// S46: loads a route checkpoint the way the original loads a save:
/// restores the host (flags, quests, running scripts, places, bags,
/// progression), enters the saved stage keeping each seat at its saved
/// place, and resumes the demo at `DDVIEW_OPENING_STEP` if given. Layouts,
/// NPCs and enemies admit from their gates as usual; nothing is forced.
fn resume_checkpoint(
    stage_view: &mut OpeningStage,
    st: &mut PlayState,
    s: &mut Session,
    path: &std::path::Path,
    native_pc: &std::path::Path,
) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let file = ddhost::save::SaveFile::from_bytes(&bytes)?;
    let exe = ddexe::locate(native_pc).map_err(|e| e.to_string())?;
    let data = InstallData::open(native_pc, Some(&exe)).map_err(|e| e.to_string())?;
    s.host.load(&file, data).map_err(|e| e.to_string())?;
    let (stage, start) = (s.world().stage_no, s.world().start_pos);
    if stage == STAGE {
        s.host.world.report(ddstory::HostEvent::StageLoaded { stage });
    } else {
        enter_stage(stage_view, st, s, stage, start, true)?;
    }
    if let (Some(d), Some(name)) = (s.demo.as_mut(), std::env::var("DDVIEW_OPENING_STEP").ok()) {
        d.step = match name.as_str() {
            "FieldWalk" => Step::FieldWalk,
            "FieldSettle" => Step::FieldSettle,
            "RescueApproach" => Step::RescueApproach,
            "Rescue" => Step::Rescue,
            "ThankReynard" => Step::ThankReynard,
            "RoadWalk" => Step::RoadWalk,
            other => return Err(format!("DDVIEW_OPENING_STEP {other} not resumable")),
        };
        d.path.clear();
        d.planning = None;
    }
    if let Some(d) = s.demo.as_mut() {
        d.resumed = true;
    }
    info!("opening: resumed checkpoint {} at stage {stage} (SceNo {})", path.display(), s.world().sce_no);
    Ok(())
}

/// The step part: host frames, inputs, the party, NPCs, talks, the menu.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
/// Assets and world entities needed by an actual stage load.
#[derive(bevy::ecs::system::SystemParam)]
pub struct OpeningStage<'w, 's> {
    commands: Commands<'w, 's>,
    streamer: Option<ResMut<'w, crate::stream::Streamer>>,
    entities: crate::stream::WorldEntities<'w, 's>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    images: ResMut<'w, Assets<Image>>,
    bindposes: ResMut<'w, Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>,
}

/// Spawns the enemy groups of the loaded layout cells of `stage` into the
/// host's fight: `ddspawn::spawns` per cell (the group list's gates on SceNo,
/// hour and layout flags; dead units of the kill records left out), each
/// placement once. Cells are checked again whenever new cells load or the
/// story world changes (a gate opening later, e.g. a quest's layout flag).
/// Goblins run and get a body and their encounter binding; other enemies
/// have no runtime here.
/// *Not yet*: despawning on cell unload or a closing gate, kill records (a
/// new game has none), the record's think/equipment/stat options
/// (`Fight::add_goblin` uses the goblin defaults), other enemy families.
fn field_enemies(view: &mut OpeningStage, st: &mut PlayState, s: &mut Session, stage: u32) {
    let Some(streamer) = view.streamer.as_mut() else { return };
    if s.enemy_stage != stage {
        s.enemy_stage = stage;
        s.enemy_cells.clear();
        s.enemy_spawned.clear();
        s.enemy_world = None;
        s.enemy_groups = match ddhost::spawns::group_list(streamer.loader.vfs, stage) {
            Ok(list) => Some(list),
            Err(e) => {
                warn!("opening: stage {stage} enemy groups: {e}");
                None
            }
        };
    }
    let Some(list) = s.enemy_groups.as_ref() else { return };
    let cells: Vec<(u16, u16)> = streamer
        .loaded_layout_cells()
        .iter()
        .filter_map(|&(m, n)| Some((u16::try_from(m).ok()?, u16::try_from(n).ok()?)))
        .collect();
    let new_cells = cells.iter().any(|c| !s.enemy_cells.contains(c));
    // the gates read SceNo, the hour and the stage's layout flags
    let gate_key = {
        let w = s.world();
        (w.sce_no, w.hour, w.lot_bits(stage).cloned())
    };
    let world_changed = s.enemy_world.as_ref() != Some(&gate_key);
    if !new_cells && !world_changed {
        return;
    }
    s.enemy_cells = cells.iter().copied().collect();
    let world = s.world().clone();
    let (scene, hour, flag) = ddhost::spawns::world_inputs(&world, stage);
    let gate_world = ddhost::ddspawn::World { scene, hour, flag: &flag };
    let vfs = streamer.loader.vfs;
    let read = |p: &str| vfs.read(p, mtarc::type_hash("rLayout")).ok();
    let mut goblins = vec![];
    let mut others = 0;
    for &cell in &cells {
        let spawns = match ddhost::ddspawn::spawns(stage, list, cell, &read, &gate_world, &[]) {
            Ok((_, spawns)) => spawns,
            Err(e) => {
                warn!("opening: stage {stage} cell {cell:?} enemies: {e}");
                continue;
            }
        };
        for spawn in spawns {
            if !s.enemy_spawned.insert((spawn.group, spawn.set_id)) {
                continue;
            }
            if spawn.class == "cSetInfoEnemy0100" {
                goblins.push(spawn);
            } else {
                others += 1;
            }
        }
    }
    drop(gate_world);
    s.enemy_world = Some(gate_key);
    if goblins.is_empty() && others == 0 {
        return;
    }
    let Some(fight) = s.host.fight.as_mut() else { return };
    if st.fight_data.is_none() {
        st.fight_data = Some(fight.data.clone());
    }
    let mut owners = vec![];
    for spawn in &goblins {
        // the placement's stat rates and experience override
        let owner = ddhost::spawns::add_placed_goblin(fight, spawn);
        let hp = fight.goblins.last().map_or(1.0, |g| g.fighter.victim.hp());
        owners.push(owner);
        let at = (Vec3::from(spawn.position) - st.origin) * UNIT;
        match crate::character::spawn(
            &mut view.commands,
            &mut streamer.loader,
            &mut view.meshes,
            &mut view.materials,
            &mut view.images,
            &mut view.bindposes,
            play::GOBLIN_MODEL,
            ddhost::ddenemyrt::goblin::BANKS[0].1,
            0,
            at,
            Some(0.0),
        ) {
            Ok(e) => st.goblins.push(play::GoblinView::new(e, hp)),
            Err(e) => {
                // keep the fight's and the view's goblin orders paired
                warn!("opening: goblin body: {e}");
                st.goblins.push(play::GoblinView::new(Entity::PLACEHOLDER, hp));
            }
        }
    }
    // Each goblin's layout identity binds it to the story's encounter
    // targets (EnemyCheck reads its HP and death through observe_fight):
    // 0x007613d0 / 0x00cc1f60 / 0x007643d0, OpeningWorld::bind_spawned_goblin.
    for (spawn, owner) in goblins.iter().zip(owners) {
        s.host.world.bind_spawned_goblin(spawn, owner);
    }
    info!(
        "opening: stage {stage}: {} goblins spawned (groups {:?}), {others} other enemies without a runtime",
        goblins.len(),
        goblins.iter().map(|g| (g.group, g.set_id)).collect::<Vec<_>>()
    );
}

/// When the story's vocation changes (the weapon table's menu, then
/// `Host::apply_job_kit` gives the fight kit), each local Arisen is respawned
/// with that vocation's starting weapons sheathed: the first-choice items of
/// `ddprogress::change::starting_items` before the two shared ones (primary,
/// then secondary; 0x00673740). *Viewer choice*: the original puts the items
/// in the inventory and equips them; here only the look changes. Seats
/// without a look (the base body) keep theirs.
/// Bodies for the hired pawns the host put into the fight
/// (`Host::sync_hired_pawn_bodies`): each with its record's look
/// (`ddlook::save::cmc_look`), drawn and posed like the party.
fn hired_pawn_bodies(view: &mut OpeningStage, st: &mut PlayState, s: &mut Session) {
    let Some(streamer) = view.streamer.as_mut() else { return };
    for index in 0..2 {
        let seat = ddhost::Host::<ddstory::data::InstallData, ddhost::opening::OpeningWorld>::hired_pawn_seat(index);
        if st.entities.contains_key(&seat) || !s.host.fight.as_ref().is_some_and(|f| f.pawns.contains_key(&seat)) {
            continue;
        }
        let Some(hired) = s.host.rift.party.hired[index].as_ref() else { continue };
        let look = match ddlook::save::cmc_look(streamer.loader.vfs, &hired.pawn) {
            Ok(l) => l,
            Err(e) => {
                warn!("opening: hired pawn {index} look: {e}");
                continue;
            }
        };
        match crate::character::spawn_look(
            &mut view.commands,
            &mut streamer.loader,
            &mut view.meshes,
            &mut view.materials,
            &mut view.images,
            &mut view.bindposes,
            &look,
            play::MOTIONS,
            0,
            Vec3::ZERO,
            Some(0.0),
        ) {
            Ok(e) => {
                st.entities.insert(seat, e);
                st.visible.insert(e, true);
                info!("opening: hired pawn {index} (seat {seat}) joins with its body");
            }
            Err(e) => warn!("opening: hired pawn {index} body: {e}"),
        }
    }
}

fn kit_weapons(view: &mut OpeningStage, st: &mut PlayState, s: &mut Session) {
    let job = s.world().player_job;
    if job == s.weapons_job {
        return;
    }
    let Some(vocation) = ddhost::ddskill::vocation::Vocation::from_id(job) else { return };
    let Some(streamer) = view.streamer.as_mut() else { return };
    s.weapons_job = job;
    let items = ddhost::ddprogress::change::starting_items(vocation);
    let own = &items[..items.len().saturating_sub(2)];
    let primary = own.first().map_or(-1, |&i| i32::from(i));
    let secondary = own.get(1).map_or(-1, |&i| i32::from(i));
    let weapons = match ddlook::save::weapons_for_items(streamer.loader.vfs, primary, secondary) {
        Ok(w) => w,
        Err(e) => {
            warn!("opening: starting weapons of {vocation:?}: {e}");
            return;
        }
    };
    for id in st.locals.clone() {
        let (Some(&old), Some(Some(look))) = (st.entities.get(&id), s.party_looks.get(usize::from(id))) else { continue };
        let mut look = look.clone();
        look.weapons = weapons.clone();
        match crate::character::spawn_look(
            &mut view.commands,
            &mut streamer.loader,
            &mut view.meshes,
            &mut view.materials,
            &mut view.images,
            &mut view.bindposes,
            &look,
            play::MOTIONS,
            0,
            Vec3::ZERO,
            Some(0.0),
        ) {
            Ok(e) => {
                view.commands.entity(old).despawn();
                st.entities.insert(id, e);
                if let Some(v) = st.visible.remove(&old) {
                    st.visible.insert(e, v);
                }
                st.forget_shown(id);
                if id == 0 {
                    s.arisen_look = Some(look.clone());
                }
                s.party_looks[usize::from(id)] = Some(look);
                info!("opening: seat {id} takes the {vocation:?} starting weapons {primary:#x} / {secondary:#x} (body {old:?} -> {e:?})");
            }
            Err(e) => warn!("opening: seat {id} with {vocation:?} weapons: {e}"),
        }
    }
}

pub fn step(
    mut stage_view: OpeningStage,
    time: Res<Time>,
    order_devices: play::PawnOrderDevices,
    buttons: Res<ButtonInput<MouseButton>>,
    mouse: Res<AccumulatedMouseMotion>,
    state: Option<ResMut<PlayState>>,
    res: Option<Res<OpeningRes>>,
    args: Res<crate::Args>,
    pause: Res<crate::pause::PauseState>,
    mut roots: Query<(&mut Transform, &mut Playback), Without<FollowCam>>,
    mut cams: Query<&mut Transform, With<FollowCam>>,
    mut vis: Query<&mut Visibility, Without<Actor>>,
    mut hud: ResMut<play::Hud>,
    vocation_view: Option<Res<crate::vocation_menu::View>>,
    item_menus: Res<crate::item_menu::Menus>,
) {
    let _timing = crate::perf::Section::start("opening::step");
    let keys = &order_devices.keys;
    let gamepads = &order_devices.gamepads;
    let (Some(mut st), Some(res)) = (state, res) else {
        return;
    };
    let st = &mut *st;
    let mut guard = res.0.lock().unwrap();
    let s = &mut *guard;
    let now = time.elapsed_secs();
    // the fight demo adds the rescue (up to 240 s) and the road walk (300 s)
    let limit = if std::env::var_os("DDVIEW_DEMO_FIGHT").is_some() { 1200.0 } else { 600.0 };
    if args.perf_route && now > limit {
        s.failure = Some(format!("performance route exceeded {limit} seconds without completing the walk"));
        return;
    }
    if let Some(path) = s.resume.take() {
        if let Err(e) = resume_checkpoint(&mut stage_view, st, s, &path, &args.native_pc) {
            s.failure = Some(format!("checkpoint {}: {e}", path.display()));
            return;
        }
    }
    if s.guest.is_none() {
    kit_weapons(&mut stage_view, st, s);
    hired_pawn_bodies(&mut stage_view, st, s);
    if let Some(streamer) = stage_view.streamer.as_mut() {
        streamer.focus = s.host.sim.player(st.follow).map(|p| p.position);
        streamer.actors = s.host.sim.players.iter().map(|p| p.position).collect();
        if let Some(terrain) = streamer.take_terrain() {
            // the camera's copy too (play::follow_camera)
            st.terrain = terrain.clone();
            s.host.set_ground_with_doors(Box::new(terrain));
        }
        let stage = s.world().stage_no;
        // A target stage's cells cannot admit actors before its streamer
        // enters that stage. The next completed set is observed next frame.
        let loaded_stage = if streamer.stage == stage { stage } else { 0 };
        match s.npc_adapter.sync_field_layouts(streamer.loader.vfs, loaded_stage, streamer.loaded_layout_cells()) {
            Ok(true) => info!("opening: stage {stage} NPC layout cells {:?}", s.npc_adapter.field_cells),
            Ok(false) => {}
            Err(error) => { s.failure = Some(format!("NPC layouts: {error}")); return; }
        }
        if loaded_stage != 0 {
            field_enemies(&mut stage_view, st, s, loaded_stage);
        }
    }
    }
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
            &mut stage_view,
            &keys,
            &gamepads,
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
    // Like --play, quick use is local host input. Opening menus/events
    // suppress it; LAN clients cannot consume an unsynchronised local bag.
    if !pause.paused && s.cut.is_none() && s.talk.is_none() && s.world_menu().is_none() {
        for &seat in &st.locals {
            let key = match seat { 0 => KeyCode::KeyU, 1 => KeyCode::Numpad0, _ => continue };
            if !item_menus.blocks(seat) && keys.just_pressed(key) {
                match s.host.item_request(seat, ddhost::items::ItemRequest::QuickUse) {
                    Ok(item) => info!("opening quick-use: seat {seat} used item {item}"),
                    Err(error) => info!("opening quick-use: seat {seat}: {error}"),
                }
            }
        }
    }
    if !pause.paused && !item_menus.blocks(0) && pause.mouse_look(buttons.pressed(MouseButton::Right)) {
        play::orbit(st, &buttons, &mouse);
    }
    // Local input: talk and menu keys first, the rest to the host.
    // Local keyboard mapping is viewer policy: E for seat0, numpad Enter
    // for seat1. Host dialogue ownership is the same for either seat.
    let e_press = !pause.paused && !item_menus.blocks(0) && keys.just_pressed(KeyCode::KeyE);
    let talk_press = if e_press { Some(0) }
        else if !pause.paused && !item_menus.blocks(1) && st.locals.contains(&1) && keys.just_pressed(KeyCode::NumpadEnter) { Some(1) }
        else { None };
    let near = s.npc_near_for(talk_press.unwrap_or(0));
    let mut input = PlayerInput {
        sim: play::gamepad_order_input(play::p1_input(&keys, &buttons, st.cam_yaw), &gamepads, 0),
        interact: false,
    };
    if s.world_menu().is_some() {
        input.sim = Input {
            camera_yaw: st.cam_yaw,
            ..Default::default()
        };
        let owner = s.world_menu().unwrap().1;
        s.vocation_menu.open(owner);
        let pick = if !pause.paused {
            vocation_view.as_ref().and_then(|view| {
                let input = s.vocation_menu.controls(&keys, &gamepads);
                view.catalog.input(&mut s.vocation_menu, input)
            })
        } else { None };
        if let Some(j) = pick {
            close_menu(s, j);
        }
    } else if s.talk.is_some() {
        input.sim = Input {
            camera_yaw: st.cam_yaw,
            ..Default::default()
        };
        let talk_player = s.talk.as_ref().unwrap().player;
        let choices = if talk_player == 0 {
            [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4]
        } else {
            [KeyCode::Numpad1, KeyCode::Numpad2, KeyCode::Numpad3, KeyCode::Numpad4]
        };
        let digit = choices.iter().position(|&k| keys.just_pressed(k));
        let has_choices = s.talk.as_ref().is_some_and(|t| !t.choices.is_empty());
        if has_choices {
            if let Some(i) = digit {
                match s.host.talk_choose(talk_player, i as u8) {
                    Ok(o) => s.show_talk(&o.events),
                    Err(e) => warn!("talk: {e}"),
                }
            }
        } else if talk_press == Some(talk_player) {
            match s.host.talk_next(talk_player) {
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
    } else if talk_press.is_some() && near.is_some() {
        let npc = near.unwrap();
        match s.talk_npc(talk_press.unwrap(), npc) {
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
    if !pause.paused && !item_menus.blocks(0) && s.demo.is_some() && s.cut.is_none() {
        input = if s.shots.is_some() && s.shots_taken == 0 {
            PlayerInput::default()
        } else {
            demo_input(s, now, &st.terrain, vocation_view.as_ref().map(|view| &view.catalog))
        };
    }
    if pause.paused {
        input = PlayerInput::default();
    }
    if item_menus.blocks(0) {
        input = PlayerInput::default();
        s.pending_input = PlayerInput::default();
    }
    if pause.paused || s.cut.is_some() || s.talk.is_some() || s.world_menu().is_some() {
        s.pending_input = PlayerInput::default();
        s.pending_local.clear();
    }
    s.pending_input.sim = play::latch_input(s.pending_input.sim, input.sim);
    s.pending_input.interact |= input.interact;
    input = s.pending_input;
    if s.host.server.is_none() && !pause.paused && s.cut.is_none()
        && s.talk.is_none() && s.world_menu().is_none()
    {
        let queued = s.pending_local.entry(1).or_default();
        *queued = if item_menus.blocks(1) { Input::default() } else { play::latch_input(*queued,
            play::gamepad_order_input(play::p2_input(&keys, st.cam_yaw), &gamepads, 1))
        };
    }
    let mut local_inputs = s.pending_local.clone();
    // Host frames at 30 Hz.
    let frame = FrameClock::default().seconds();
    // the catch-up cap follows the virtual clock's own (DDVIEW_TIME_SCALE)
    s.acc = (s.acc + time.delta_secs()).min(0.25 * crate::time_scale().max(1.0));
    while s.acc >= frame {
        if s.dragon_capture && s.host.fight.as_ref().and_then(|f|f.dragon.as_ref())
            .is_some_and(|d|d.thinker.current_states()==[1] && d.actor.frame>=15.0)
        {
            // Hold a frame reached by real simulation while GPU pipelines
            // warm up. This never reports completion to the actor/FSM.
            s.acc=0.0;
            break;
        }
        let _timing = crate::perf::Section::start("opening host frame");
        s.acc -= frame;
        s.between.snapshot(&s.host.sim);
        s.pending_input = PlayerInput::default();
        s.pending_local.clear();
        if let Err(error) = s.sync_npc_intels() {
            s.failure = Some(format!("NPC materialization: {error}"));
            break;
        }
        // Scenario observations use authoritative actor positions, not the
        // view's interpolated transforms or graph-map membership.
        s.host.world.npcs = s.host.story_npc_intels().filter_map(|placement| {
            let id = placement.npc as u32;
            // Heading is still the authored placement heading: canonical
            // Wait/Place-direction updates are unresolved, never read from rendering.
            s.host.story_npc_position(id).map(|pos| ddstory::NpcUnit {
                id, unit: ddstory::Player { pos, yaw: placement.transform.angle[1] },
            })
        }).collect();
        let mut inputs = FrameInputs::new();
        let seats: Vec<_> = s.host.roster.ids().collect();
        for id in seats {
            inputs.insert(id, if id == 0 { input } else {
                PlayerInput { sim: consume_local_input(&mut local_inputs, id), interact: false }
            });
        }
        input.interact = false;
        s.host.scenario_field_cells = stage_view.streamer.as_ref().and_then(|streamer| streamer.scenario_field_cells());
        let report = match s.host.tick_network(&inputs, network_now) {
            Ok(r) => r,
            Err(e) => {
                s.failure = Some(format!("host frame failed: {e}"));
                break;
            }
        };
        if let (Some(f), Some(d)) = (s.host.fight.as_ref(), s.demo.as_mut()) {
            for e in f.events.iter().filter(|e| f.npcs.contains_key(&e.victim)) {
                let acc = d.npc_damage.entry((e.victim, e.attacker)).or_default();
                acc.0 += 1;
                acc.1 += e.total;
            }
        }
        if let Some(error) = s.host.world.cameras.as_ref().and_then(|c|c.error.as_ref()) {
            s.failure = Some(format!("authored camera request: {error}"));
            break;
        }
        s.npc_requests(&report.npc_requests);
        for r in &report.requests {
            match r {
                Request::PlaceParty {
                    start_pos: Some(n), ..
                } => s.place_party(*n),
                Request::PlaceParty { pos: Some(p), .. } => info!("opening: party placed at {p:?}"),
                Request::JumpArea { stage, pos, .. } => {
                    if let Err(e) = enter_stage(&mut stage_view, st, s, *stage, *pos, false) {
                        s.failure = Some(format!("stage {stage} handoff failed: {e}"));
                    }
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
    if let Some(streamer) = stage_view.streamer.as_mut() {
        sync_npc_models(s, &mut stage_view.commands, &mut streamer.loader,
            &mut stage_view.meshes, &mut stage_view.materials, &mut stage_view.images,
            &mut stage_view.bindposes);
    }
    // The NPCs walk where the schedule put them.
    let dt = time.delta_secs();
    let ids: Vec<u32> = s.npcs.keys().copied().collect();
    for id in ids {
        let explicit = s.npc_adapter.placed.remove(&id);
        let target = explicit.map(|(pos, _)| Vec3::from(pos))
            .or_else(|| s.host.story_npc_position(id).map(Vec3::from))
            .unwrap_or(s.npcs[&id].1);
        let (e, mut at, mut yaw) = s.npcs[&id];
        if let Some((_, dir)) = explicit {
            at = target;
            if let Some(dir) = dir { yaw = dir[0].atan2(dir[2]); }
        }
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
            if s.npc_authored_motion.show(&*s.host, id, &mut pb) {
                // A later locomotion interface must restart its presentation
                // even when its moving/idle value matches the earlier one.
                s.npc_moving.remove(&id);
            } else {
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
    }
    // The party; during a cutscene the camera belongs to the event.
    let bodies: Vec<Shown> = s.between.shown(&s.host.sim, (s.acc / frame).clamp(0.0, 1.0));
    let follow = st.follow;
    if s.cut.is_some() {
        st.follow = u8::MAX;
    }
    play::show(st, &bodies, &mut roots, &mut cams);
    play::show_goblins(st, s.host.fight.as_ref(), &mut roots, 1.0);
    dragon_view::sync(&mut stage_view,st,s);
    dragon_view::show(st,s,&mut roots);
    if s.dragon_capture {
        if let Some(dragon)=s.host.fight.as_ref().and_then(|f|f.dragon.as_ref()) {
            let target=(Vec3::from(dragon.actor.pos)-st.origin)*UNIT;
            // Capture camera choice, not a claim about the original camera.
            for mut camera in &mut cams {
                *camera=Transform::from_translation(target+Vec3::new(40.0,20.0,60.0))
                    .looking_at(target,Vec3::Y);
            }
        }
    }
    // The party's bars once the fight has its Arisen (the vocation's kit):
    // each seat's maximum from its kit, as play::setup records it.
    if let Some(f) = s.host.fight.as_ref() {
        for (&seat, a) in &f.arisen {
            if let ddhost::ddenemyrt::combat::Victim::Player { hp_max, .. } = a.fighter.victim {
                st.hp_max.insert(seat, hp_max);
            }
        }
    }
    hud.rows = play::hud_rows(st, &s.host.sim, s.host.fight.as_ref(), None);
    hud.enemy = play::enemy_gauge(st, s.host.fight.as_ref());
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
/// The view closed the vocation menu with `job` (Fighter 1, Strider 2, Mage 3:
/// `ddskill::vocation::Vocation`).
fn close_menu(s: &mut Session, job: u32) {
    info!("opening: vocation menu closed with job {job}");
    s.vocation_menu.close();
    s.host.world.close_menu(Some(job));
    // Host::apply_job_kit delivers the first-choice items once on its next tick.
}

fn cut_config(world: &ddstory::World, guest_stage: Option<u32>, language: usize, sex: u8) -> EventConfig {
    EventConfig { language, sex, hour: world.hour, stage: guest_stage.unwrap_or(world.stage_no) }
}

fn start_cut(s: &mut Session, row: usize) {
    // Translated from 0x0050c290 case 0x15: the real ev01 request
    // replaces the challenge gear before the movie, once per session.
    if row == 21 && s.guest.is_none() { s.host.heart_taken_items(); }
    let w = s.world();
    let cfg = cut_config(w, s.guest.as_ref().and_then(|c| c.stage().or_else(|| c.spot.map(|spot| spot.stage))), s.language, s.sex);
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
    mut voices: Local<Option<mtsound::voice::EventVoices>>,
) {
    let _timing = crate::perf::Section::start("opening::cutscene");
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
    let voices = *voices.get_or_insert_with(|| {
        args.save_xml.as_deref().and_then(|xml| {
            crate::audio::voices_from_save_xml(xml, "mPlayerDataManual")
                .map_err(|e| warn!("opening voice context unavailable: {e}")).ok()
        }).unwrap_or_default()
    });
    let aspect = windows
        .single()
        .map(|w| w.width() / w.height().max(1.0))
        .unwrap_or(16.0 / 9.0);
    let mut finished = None;
    if let Some(c) = &mut s.cut {
        // mDeltaTime: frames at 30 per second.
        c.acc += time.delta_secs().min(0.1 * crate::time_scale().max(1.0)) * 30.0;
        while c.acc >= 1.0 && !c.player.finished() {
            c.acc -= 1.0;
            match c.player.step(1.0) {
                Ok(f) => {
                    crate::audio::cues_with_voices(&mut commands, &f, voices);
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
            t.0 = if s.world_menu().is_some() {
                String::new()
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
                "\nq0002 active.",
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
/// The demo's fight on the field: the Arisen attacks the nearest live
/// goblin with ordinary inputs; every 5 seconds it logs the goblins left,
/// the party's HP and q0002's state. Ends when no goblin is left alive or
/// after 240 seconds. No enemy death, flag or quest state is forced.
fn demo_rescue(s: &mut Session, now: f32) -> PlayerInput {
    let alive = s.host.fight.as_ref().map_or(0, |f| f.goblins.iter().filter(|g| !g.dead()).count());
    let (since, last_log) = {
        let d = s.demo.as_ref().unwrap();
        (d.since, d.check.0)
    };
    // q2:20 (rescued) or q2:30 (Reynard died): event1's outcomes. Ordinary
    // field goblins left alive don't hold the road walk up.
    let outcome = s.world().quest_bits(2).is_some_and(|b| b.get(20) || b.get(30));
    if now - last_log >= 5.0 || alive == 0 || outcome || now - since > 240.0 {
        let w = s.world();
        let hp = s.host.fight.as_ref().and_then(|f| f.arisen.get(&0)).map(|a| a.fighter.victim.hp());
        info!(
            "opening demo: rescue: {alive} goblins alive, Arisen HP {hp:?}, Reynard (NPC 304) HP {:?}, level/exp {:?}, SceNo {}, q0002 bits {:08x?}, quests {:?}",
            s.host.story_npc_lifetime(304).and_then(|l| s.host.story_npc_combat_facts(l)).map(|f| (f.hp, f.max_hp)),
            s.host.progression.characters.first().map(|c| (c.progress.level, c.progress.exp)),
            w.sce_no,
            w.quest_bits(2).map(|b| b.words()),
            s.host.story.as_ref().map(|st| st.quests.iter().map(|q| q.number()).collect::<Vec<_>>())
        );
        if let Some(d) = s.demo.as_ref().filter(|d| !d.npc_damage.is_empty()) {
            info!("opening demo: rescue: damage on NPC bodies by (victim, attacker): (hits, total) {:?}", d.npc_damage);
        }
        // each live goblin: owner, action, target, distance to the Arisen
        // (m) and its last actions, to see why one never reaches or hurts it
        if let (Some(f), Some(p)) = (s.host.fight.as_ref(), s.host.sim.player(0)) {
            for g in f.goblins.iter().filter(|g| !g.dead()) {
                let d = ((g.pos[0] - p.position[0]).powi(2) + (g.pos[2] - p.position[2]).powi(2)).sqrt() / 100.0;
                let recent: Vec<i32> = g.log.iter().rev().take(6).map(|x| x.1).collect();
                info!(
                    "opening demo: rescue: goblin {} action {} target {:?} {d:.1} m away, height {:.0} vs {:.0}, recent {recent:?}",
                    g.fighter.owner, g.action, g.target, g.pos[1], p.position[1]
                );
            }
        }
        let d = s.demo.as_mut().unwrap();
        d.check.0 = now;
        if outcome {
            let next = if w_bit(s, 20) { Step::ThankReynard } else { Step::RoadWalk };
            info!("opening demo: Rescue done (q2:20 {}, q2:30 {}) -> {next:?}", w_bit(s, 20), w_bit(s, 30));
            let d = s.demo.as_mut().unwrap();
            d.step = next;
            d.since = now;
            d.talked = false;
            d.path.clear();
            d.planning = None;
        } else if now - since > 240.0 {
            info!("opening demo: Rescue done without an outcome -> Done");
            d.step = Step::Done;
            d.since = now;
        }
    }
    demo_heal(s, now);
    let d = s.demo.as_mut().unwrap();
    let sim = play::demo_attack(&s.host.sim, s.host.fight.as_ref(), 0, 0.0, &mut d.press);
    PlayerInput { sim, interact: false }
}

/// The fight demo uses the next curative of the Arisen's bag (S28's quick
/// use, the same path as the U key) below 40% HP, at most every 5 seconds.
fn demo_heal(s: &mut Session, now: f32) {
    let hurt = s.host.fight.as_ref().and_then(|f| f.arisen.get(&0)).is_some_and(|a| {
        let max = match &a.fighter.victim {
            ddhost::ddenemyrt::combat::Victim::Player { hp_max, .. } => *hp_max,
            _ => return false,
        };
        let v = &a.fighter.victim;
        !v.dead() && v.hp() < 0.4 * max
    });
    let d = s.demo.as_mut().unwrap();
    if !hurt || now - d.last_heal < 5.0 {
        return;
    }
    d.last_heal = now;
    match s.host.item_request(0, ddhost::items::ItemRequest::QuickUse) {
        Ok(item) => info!("opening demo: used item {item}"),
        Err(e) => info!("opening demo: no curative: {e}"),
    }
}

/// Whether a live goblin is within `range` cm (horizontal) of the Arisen.
fn demo_goblin_near(s: &Session, range: f32) -> bool {
    let (Some(f), Some(p)) = (s.host.fight.as_ref(), s.host.sim.player(0)) else { return false };
    f.goblins
        .iter()
        .any(|g| !g.dead() && (g.pos[0] - p.position[0]).hypot(g.pos[2] - p.position[2]) < range)
}

fn w_bit(s: &Session, bit: usize) -> bool {
    s.world().quest_bits(2).is_some_and(|b| b.get(bit))
}

fn demo_input(s: &mut Session, now: f32, stage_ground: &dyn ddsim::Ground, vocation_catalog: Option<&crate::vocation_menu::Catalog>) -> PlayerInput {
    let Some(p) = s.host.sim.player(0).map(|p| Vec3::from(p.position)) else {
        return PlayerInput::default();
    };
    let Some(d) = s.demo.as_mut() else {
        return PlayerInput::default();
    };
    if d.since == 0.0 {
        d.since = now;
        d.step_origin = p;
        d.check = (now, p);
    }
    let step = d.step;
    let elapsed = now - d.since;
    let road_terminal = step == Step::RoadWalk && (s.world().quest_bit(2, 10)
        || s.host.story.as_ref().is_some_and(|story| story.quest(3).is_some())
        || elapsed > 300.0);
    if step == Step::Rescue {
        return demo_rescue(s, now);
    }
    // With DDVIEW_DEMO_FIGHT the Arisen defends itself in the field: once
    // a live goblin is within 10 m it fights with ordinary inputs (the
    // goblins run in and attack; an idle Arisen would die before Rescue).
    if matches!(step, Step::FieldWalk | Step::FieldSettle | Step::RescueApproach | Step::ThankReynard | Step::ReynardRepeat | Step::RoadWalk)
        && !road_terminal
        && std::env::var_os("DDVIEW_DEMO_FIGHT").is_some()
        && demo_goblin_near(s, 1000.0)
    {
        demo_heal(s, now);
        let d = s.demo.as_mut().unwrap();
        let sim = play::demo_attack(&s.host.sim, s.host.fight.as_ref(), 0, 0.0, &mut d.press);
        return PlayerInput { sim, interact: false };
    }
    let route = ddstory::route::opening();
    let place = |name: &str| -> Option<Vec3> {
        let w = route.iter().find(|w| w.name == name)?;
        let st = &s.host.story.as_ref()?.stage;
        w.place(&st.scenarios).map(|(p, _)| Vec3::from(p))
    };
    let mut target = match step {
        Step::WeaponTable => place("cassardis: weapon table"),
        Step::RoomExit => place("cassardis: room exit"),
        Step::TalkQuina => s.npcs.get(&2).map(|n| n.1),
        // The SceNo1300 OBB is in the doorway, not outside the house.
        // Continue through its far boundary from the approach side. This
        // is demo navigation geometry, not an original quest condition.
        Step::WalkOut => s.host.story.as_ref().and_then(|story| {
            let shape = story.stage.scenarios.entry(4, 7)?.shape.as_ref()?;
            demo_through_box(&shape.zone, s.demo.as_ref()?.step_origin)
        }),
        Step::Q0002Event if s.world().sce_no >= 1400 => s.npcs.get(&1).map(|(_, at, _)| *at),
        Step::Q0002Event => place("cassardis: q0002 event"),
        // Installed st200_door entry 0 is the stage100/start2 gate.
        // Coordinates and angle are read from that scenario at runtime.
        Step::Gate if s.world().stage_no == 200 => benchmark_place(s, 2, 0).map(|v| v.0),
        // Walk toward the authored q2 field event volume; stop once the
        // actual simulated position crosses a cell, before claiming any
        // encounter outcome. No fixture NPC or quest state is changed.
        Step::FieldWalk | Step::RescueApproach => s.host.story.as_ref().and_then(|story| {
            let n = story.stage.scenarios.tasks.get(4)?.iter().position(|e| e.name == "q0002_event1")?;
            benchmark_place(s, 4, n as u32).map(|v| v.0)
        }),
        Step::RoadWalk => s.host.story.as_ref().and_then(|story| {
            let n = story.stage.scenarios.tasks.get(4)?.iter().position(|e| e.name == "q0002_event2")?;
            benchmark_place(s, 4, n as u32).map(|v| v.0)
        }),
        Step::ThankReynard | Step::ReynardRepeat => s.host.story_npc_position(304).map(Vec3::from),
        Step::Gate | Step::FieldSettle | Step::Rescue | Step::Done => None,
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
    // A pending collision search must not hide a real quest outcome or
    // the demo's existing road deadline. These checks do not write flags.
    if road_terminal {
        d.planning = None;
        d.path.clear();
        target = None;
    }
    // A closed hinged door changes the collision topology. A human would
    // press interact before trying to plan around it. Use the host's
    // explicitly labelled nearest-door adapter through ordinary input;
    // never remove its collision or declare its motion finished here.
    let closed_door_nearby = target.is_some() && !talking && menu.is_none()
        && s.host.doors.as_ref().is_some_and(|doors| doors.states.values().any(|door| {
            let entry = &doors.entries[&door.id];
            door.phase == dddoor::runtime::Phase::Closed
                && entry.policy.resolve(entry.observed) == Some(dddoor::policy::Availability::Unlocked)
                && Vec2::new(p.x - door.position[0], p.z - door.position[2]).length() <= 400.0
                && (p.y - door.position[1]).abs() <= 250.0
        }));
    if closed_door_nearby && now - d.check.0 > 1.0 && (p - d.check.1).length() < 60.0
        && now - d.last_door_press > 1.0
    {
        d.last_door_press = now;
        d.planning = None;
        // The plan already allows this unlocked-door transition. Preserve
        // its exit direction while the real leaf opens; dropping the path
        // here would send the direct goal heading back into the building.
        info!("opening demo: interacting with closed door at {p:?}, retaining route");
        return PlayerInput { interact: true, ..Default::default() };
    }
    if let Some(plan) = &mut d.planning {
        let _timing = crate::perf::Section::start("demo route planning");
        let ground = DemoPlanningGround::new(stage_ground, s.host.doors.as_ref());
        match plan.advance(
            &ground,
            &s.host.sim.sweep_param,
            std::time::Duration::from_millis(1),
        ) {
            Some(path) => {
                info!(
                    "opening demo: collision planner finished with {} waypoints",
                    path.len()
                );
                d.path = path.into();
                d.planning = None;
                d.check = (now, p);
            }
            None => return PlayerInput::default(),
        }
    }
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
                        let shape = (step == Step::RoadWalk).then(|| {
                            s.host.story.as_ref()?.stage.scenarios.tasks.get(4)?
                                .iter().find(|entry| entry.name == "q0002_event2")?
                                .shape.clone()
                        }).flatten();
                        d.planning = Some(RoutePlan::to_shape(p, goal, shape));
                        warn!(
                            "opening demo: blocked walking to {step:?}; planning incrementally (no placement fallback)"
                        );
                        input = PlayerInput::default();
                    }
                }
                d.check = (now, p);
            }
        } else {
            input.sim = Input {
                camera_yaw: flat.x.atan2(flat.z),
                ..Default::default()
            };
            if step == Step::WeaponTable || step == Step::Gate {
                d.press = !d.press;
                input.interact = d.press;
                face = if step == Step::Gate {
                    // Reproduce a player turning toward the authored door
                    // interaction angle while standing in its volume.
                    benchmark_place_from_scenarios(&s.host.story.as_ref().unwrap().stage.scenarios, 2, 0).map(|v| v.1)
                } else { route
                    .iter()
                    .find(|w| w.name == "cassardis: weapon table")
                    .and_then(|w| {
                        w.place(&s.host.story.as_ref()?.stage.scenarios)
                            .map(|(_, yaw)| yaw)
                    }) };
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
        Step::WalkOut if w.sce_no >= 1300 && target.is_some_and(|goal| {
            Vec2::new(goal.x - p.x, goal.z - p.z).length() <= 60.0
        }) && s.host.story.as_ref().and_then(|story| story.stage.scenarios.entry(4, 7))
            .and_then(|entry| entry.shape.as_ref()).is_some_and(|shape| !shape.hit(p.to_array(), 0.0))
            => next = Some(Step::Q0002Event),
        Step::Q0002Event if quests.contains(&2) => {
            next = Some(if d.perf_route { Step::Gate } else { Step::Done });
        }
        Step::Gate if w.stage_no == 100 => {
            d.field_cell = Some(benchmark_cell(p));
            info!("perf route: entered field at {p:?}, cell {:?}", d.field_cell);
            next = Some(Step::FieldWalk);
        }
        Step::FieldWalk if d.field_cell.is_some_and(|cell| cell != benchmark_cell(p)) => {
            info!("perf route: walked cell boundary {:?} -> {:?} at {p:?}; settling 30 seconds without screenshots", d.field_cell, benchmark_cell(p));
            next = Some(Step::FieldSettle);
        }
        Step::FieldSettle if now - d.since >= 30.0 => {
            next = Some(if std::env::var_os("DDVIEW_DEMO_FIGHT").is_some() { Step::RescueApproach } else { Step::Done })
        }
        Step::RescueApproach if w.quest_bit(2, 25) || w.quest_bit(2, 20) || w.quest_bit(2, 30) => {
            // q0002_event1 sets25 after actual Area entry/camera request.
            // Existing main-script outcomes also permit the next real check.
            // No flag is written by this input driver.
            next = Some(Step::Rescue);
        }
        Step::ThankReynard if w.quest_bit(2, 88) && s.talk.is_none() => {
            next = Some(Step::ReynardRepeat);
        }
        Step::ReynardRepeat if d.talked && s.talk.is_none() => {
            next = Some(Step::RoadWalk);
        }
        Step::RoadWalk if w.quest_bits(2).is_some_and(|b| b.get(10)) || quests.contains(&3) => {
            info!("opening demo: road: q2:10 {}, q0003 booted {}", w.quest_bits(2).is_some_and(|b| b.get(10)), quests.contains(&3));
            next = Some(Step::Done);
        }
        Step::RoadWalk if now - d.since > 300.0 => {
            warn!("opening demo: road: q0002_event2 not reached in 300 s");
            next = Some(Step::Done);
        }
        _ => {}
    }
    // Authored q0002_n304's ready loop has both Loop and Talk true;
    // tremble3226 has Talk false. Wait for the real actor, not a timer.
    let reynard_ready = s.host.story_npc_motion(304)
        .is_some_and(|actor| actor.authored.looping && actor.authored.talk);
    let talk_due = (step == Step::TalkQuina || (step == Step::Q0002Event && w.sce_no >= 1400)
        || (step == Step::ThankReynard && reynard_ready) || step == Step::ReynardRepeat)
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
        d.planning = None;
        d.since = now;
        d.step_origin = p;
        d.check = (now, p);
        d.last_talk = 0.0;
        if matches!(n, Step::ThankReynard | Step::ReynardRepeat) { d.talked = false; }
    }
    if let (Some(n), Some(dir)) = (next, std::env::var_os("DDVIEW_CHECKPOINT_DIR")) {
        write_checkpoint(s, std::path::Path::new(&dir), &format!("{n:?}"));
    }
    if let (Some(yaw), Some(pl)) = (face, s.host.sim.player_mut(0)) {
        // the demo turns the Arisen to the table (the scenario's angle
        // filter), as a player would with the stick
        pl.yaw = yaw;
    }
    if menu_due {
        // DDVIEW_DEMO_JOB picks another vocation (2 Strider, 3 Mage) for
        // unattended checks of the other kits
        let job = std::env::var("DDVIEW_DEMO_JOB").ok().and_then(|v| v.parse().ok()).unwrap_or(ddskill_fighter());
        if let Some(catalog) = vocation_catalog {
            s.vocation_menu.open(menu.unwrap().1);
            match catalog.demo_select(&mut s.vocation_menu,job) {
                Ok(()) => {
                    let confirm = crate::vocation_menu::Input { confirm:true, ..default() };
                    let confirmed = catalog.input(&mut s.vocation_menu,confirm)
                        .or_else(|| catalog.input(&mut s.vocation_menu,confirm));
                    if let Some(job) = confirmed { close_menu(s,job); }
                    else { s.failure=Some("unattended vocation confirmation did not accept".into()); }
                }
                Err(error) => s.failure=Some(error),
            }
        } else { s.failure=Some("vocation GUI catalog is unavailable".into()); }
    }
    if talk_due {
        if s.talk.is_some() {
            if let Ok(o) = s.host.talk_next(0) {
                s.show_talk(&o.events);
            }
        } else if let Some(d) = s.demo.as_mut() {
            if !d.talked || step == Step::Q0002Event {
                let npc = match step {
                    Step::Q0002Event => 1,
                    Step::ThankReynard | Step::ReynardRepeat => 304,
                    _ => 2,
                };
                match s.talk_npc(0, npc) {
                    Ok(o) => {
                        if let Some(d) = s.demo.as_mut() { d.talked = true; }
                        s.show_talk(&o.events);
                        if npc == 2 { s.shot("5-talk-quina"); }
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
            let stage_no = w.stage_no;
            info!(
                "opening demo: done: SceNo {}, GSF 372 {}, GSF 381 {}, q0001 bits {:08x?}, q0002 booted {}",
                w.sce_no,
                w.gsf.get(372),
                w.gsf.get(381),
                w.quest_bits(1).map(|b| b.words()),
                quests.contains(&2)
            );
            let valid = if s.demo.as_ref().is_some_and(|d| d.perf_route) {
                w.stage_no == 100 && s.demo.as_ref().is_some_and(|d| d.resumed
                    || d.field_cell.is_some_and(|cell| cell != benchmark_cell(p)))
            } else { w.sce_no == 1400
                && w.gsf.get(372)
                && w.gsf.get(381)
                && quests.contains(&2)
                && w.quest_bits(1)
                    .is_some_and(|bits| bits.words() == [0x18000000, 0, 3]) };
            if !valid {
                s.failure = Some("opening endpoint flag/SceNo check failed".into());
            }
            s.shot("7-q0002");
            s.quit_at = if s.demo.as_ref().is_some_and(|d| d.perf_route) {
                info!("perf route: complete without screenshots, stage {stage_no}, position {p:?}");
                Some(now + 1.0)
            } else { s.shots.as_ref().map(|_| now + 3.0) };
        }
        _ => {}
    }
    input
}

fn benchmark_cell(p: Vec3) -> (i32, i32) {
    (50 + (p.z / 10_000.0).floor() as i32, 50 + (p.x / 10_000.0).floor() as i32)
}

/// A render frame may contain several host ticks: held movement persists,
/// while each locally latched button edge is delivered only once.
fn consume_local_input(inputs: &mut HashMap<ddsim::PlayerId, Input>, seat: ddsim::PlayerId) -> Input {
    let Some(input) = inputs.get_mut(&seat) else { return Input::default() };
    let current = *input;
    input.jump = false;
    input.light = false;
    input.heavy = false;
    current
}

/// Demo-only goal past the far support plane of an authored doorway box.
/// The approach chooses the exit side; dimensions come from the install.
fn demo_through_box(zone: &ddstory::scenario::Zone, from: Vec3) -> Option<Vec3> {
    let ddstory::scenario::Zone::Obb { m, ext } = zone else { return None };
    let center = Vec3::new(m[3][0], from.y, m[3][2]);
    let direction = (center - from).try_normalize()?;
    let support: f32 = (0..3).map(|i| {
        direction.dot(Vec3::new(m[i][0], m[i][1], m[i][2])).abs() * ext[i]
    }).sum();
    Some(center + direction * (support + 2.0 * ddsim::BODY_RADIUS))
}

fn benchmark_place(s: &Session, task: u32, no: u32) -> Option<(Vec3, f32)> {
    benchmark_place_from_scenarios(&s.host.story.as_ref()?.stage.scenarios, task, no)
}

fn benchmark_place_from_scenarios(sc: &ddstory::scenario::Scenarios, task: u32, no: u32) -> Option<(Vec3, f32)> {
    ddstory::route::Waypoint {
        name: "performance route", stage: 0, entry: (task, no), until: None,
        interact: true, npcs: vec![],
    }.place(sc).map(|(p, yaw)| (Vec3::from(p), yaw))
}

impl Session {
    fn network(&mut self, args: &crate::Args) -> Result<(), String> {
        if args.join.is_some() {
            let guest = crate::net::take_prejoined().ok_or("opening guest did not complete prejoin")?;
            // Collision is confirmed/loaded from the received stage in guest_step,
            // before any prediction. Prejoined field snapshots never get Cassardis ground.
            self.guest = Some(guest);
            self.demo = None;
        } else if let Some(port) = args.host {
            let udp = ddnet::Udp::bind(&format!("0.0.0.0:{port}")).map_err(|e| e.to_string())?;
            let p = self.host.sim.player(0).ok_or("missing host seat")?;
            let spot = ddnet::Spot {
                stage: self.world().stage_no,
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
    view: &mut OpeningStage,
    keys: &ButtonInput<KeyCode>,
    gamepads: &Query<(Entity, &Gamepad)>,
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
            play::gamepad_order_input(play::p1_input(keys, buttons, st.cam_yaw), gamepads, 0)
        },
        interact: !paused && keys.just_pressed(KeyCode::KeyE),
    };
    let c = s.guest.as_mut().unwrap();
    c.pump(now);
    // The host's loaded stage and positions select presentation/collision.
    // Reuse Streamer::enter as the host handoff does, without StageLoaded,
    // place_party, local quest advancement or NPC ownership admission.
    if let Some(streamer) = view.streamer.as_mut() {
        let received_stage = c.stage().or_else(|| c.spot.map(|spot| spot.stage));
        if received_stage == Some(streamer.stage) && !c.ground_ready() {
            c.set_ground(Box::new(st.terrain.clone()));
        }
        if let Some(stage) = received_stage.filter(|&stage| stage != streamer.stage) {
            let focus = c.latest().and_then(|snap| c.seat.and_then(|seat|
                snap.bodies.iter().find(|body| body.id == seat))).map(|body| body.position);
            if let Some(focus) = focus {
                match streamer.enter(&mut view.commands, view.entities.iter(), &mut view.meshes,
                    &mut view.materials, &mut view.images, stage, focus) {
                    Ok(terrain) => {
                        st.origin = Vec3::ZERO; // Streamer::enter uses stage origin cell(50,50).
                        st.terrain = terrain.clone();
                        st.door_collision = Default::default();
                        c.set_ground(Box::new(terrain));
                        for (id, (entity, _, _)) in s.npcs.drain() {
                            view.commands.entity(entity).despawn();
                            s.npc_authored_motion.forget_remote(id);
                        }
                        s.guest_npcs.clear(); s.npc_moving.clear();
                        info!("opening guest: presented authoritative stage {stage} at {focus:?}");
                    }
                    Err(error) => { s.failure = Some(format!("guest stage {stage}: {error}")); }
                }
            }
        }
        streamer.focus = c.own().map(|body| body.position);
        streamer.actors = c.latest().map_or_else(Vec::new, |snap| snap.bodies.iter().map(|body| body.position).collect());
        if let Some(terrain) = streamer.take_terrain() {
            st.terrain = terrain.clone(); c.set_ground(Box::new(terrain));
        }
    }
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
    let enemies = c.enemies(0.0);
    let npc_snapshots = c.npcs();
    let camera_snapshot = c.quest_camera();
    let retired: Vec<_> = s.npcs.keys().copied().filter(|id|
        npc_snapshots.iter().find(|n| n.id == *id).is_none_or(|next|
            next.combat.is_some_and(|combat| combat.retired)
                || s.guest_npcs.get(id).is_some_and(|old| old.generation != next.generation))).collect();
    for id in retired {
        s.npc_authored_motion.forget_remote(id);
        if let Some((entity, _, _)) = s.npcs.remove(&id) { view.commands.entity(entity).despawn(); }
    }
    s.guest_npcs = npc_snapshots.iter().map(|n| (n.id, *n)).collect();
    if let Some(streamer) = view.streamer.as_mut() {
        if let Err(error) = crate::quest_camera::receive(s.host.world.cameras.get_or_insert_with(Default::default),
            camera_snapshot, streamer.loader.vfs) { s.failure = Some(format!("guest quest camera: {error}")); }
        if let Err(error) = s.guest_npc_bodies.load(&npc_snapshots, streamer.loader.vfs,
            &s.npc_adapter.exe.image().expect("supported installed exe")) { s.failure = Some(format!("guest NPC body: {error}")); }
        // Admission follows the authoritative list; own install supplies look.
        for npc in &npc_snapshots {
            if s.npcs.contains_key(&npc.id) || npc.combat.is_some_and(|c| c.retired) { continue; }
            let Some(look) = npc_look(streamer.loader.vfs, npc.id, 0) else { continue };
            match crate::character::spawn_look(&mut view.commands, &mut streamer.loader,
                &mut view.meshes, &mut view.materials, &mut view.images, &mut view.bindposes,
                &look, play::MOTIONS, 0, (Vec3::from(npc.pos)-st.origin)*UNIT, Some(npc.yaw)) {
                Ok(entity) => { s.npcs.insert(npc.id, (entity, Vec3::from(npc.pos), npc.yaw)); }
                Err(error) => { s.failure = Some(format!("guest NPC {} model: {error}", npc.id)); }
            }
        }
    }
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
    if let Some(streamer) = view.streamer.as_mut() {
        s.guest_enemy_views.sync(&enemies, st.origin, &mut view.commands,
            &mut streamer.loader, &mut view.meshes, &mut view.materials,
            &mut view.images, &mut view.bindposes);
    }
    s.guest_enemy_views.show(&enemies, st.origin, s.cut.is_some(), roots, &mut st.visible);
    if s.dragon_capture {
        if let Some(dragon)=enemies.iter().find(|e|e.identity.family==ddnet::mirror::EnemyFamily::Dragon) {
            let target=(Vec3::from(dragon.pos)-st.origin)*UNIT;
            // Same explicit diagnostic camera as the host; not an original camera.
            for mut camera in cams.iter_mut() {
                *camera=Transform::from_translation(target+Vec3::new(40.0,20.0,60.0))
                    .looking_at(target,Vec3::Y);
            }
        }
    }
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
        if let (Some(at), Ok((mut t, mut playback))) = (at, roots.get_mut(entity)) {
            *t = npc_motion::remote_transform(at, st.origin);
            if let Err(error) = s.npc_authored_motion.show_remote(at, &mut playback) {
                s.failure = Some(format!("guest NPC {id} motion: {error}"));
            }
        }
        if let Ok(mut v) = vis.get_mut(entity) {
            *v = if at.is_some_and(|n| n.combat.is_none_or(|c| !c.retired)) && s.cut.is_none() {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
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

type RouteCell = (i32, i32);
type RouteNode = (Vec3, f32, RouteCell);

/// Demo navigation treats an unlocked door as an interaction transition.
/// The real host ground retains its closed leaf and requires an actual E
/// press before movement can pass. Unknown/locked doors remain obstacles
/// even for planning. No shared collision enable bit is changed here.
struct DemoPlanningGround<'a> { parts: Vec<&'a dyn ddsim::Ground> }
impl<'a> DemoPlanningGround<'a> {
    fn new(base: &'a dyn ddsim::Ground, doors: Option<&'a ddhost::doors::Doors>) -> Self {
        let mut parts = vec![base];
        if let Some(doors) = doors {
            for (id, leaf) in doors.collision.0.iter() {
                if leaf.enabled.load(std::sync::atomic::Ordering::Relaxed)
                    && doors.entries.get(id).is_none_or(|entry|
                        entry.policy.resolve(entry.observed) != Some(dddoor::policy::Availability::Unlocked))
                {
                    parts.push(&leaf.terrain);
                }
            }
        }
        Self { parts }
    }
}
impl ddsim::Ground for DemoPlanningGround<'_> {
    fn height(&self, x: f32, z: f32, below: f32) -> Option<f32> {
        self.parts.iter().filter_map(|ground| ground.height(x, z, below)).max_by(f32::total_cmp)
    }
    fn triangles_in(&self, lo: [f32; 3], hi: [f32; 3], out: &mut Vec<ddsim::sweep::Face>) -> bool {
        let mut found = false;
        for ground in &self.parts { found |= ground.triangles_in(lo, hi, out); }
        found
    }
}

/// Demo navigation only, not a translated NPC/pathfinding rule. An A* search
/// samples capsule sweeps against the installed collision mesh. Resolution
/// is the existing player radius. The budget bounds demo work; a failed
/// search leaves the player where collision put it, never warps it.
struct RoutePlan {
    origin: Vec3,
    target: Vec3,
    /// Demo completion uses the installed Area for RoadWalk. Its centre
    /// can lie in blocked scenery; a player only needs to enter the Area.
    shape: Option<ddstory::scenario::Shape>,
    open: std::collections::BinaryHeap<(std::cmp::Reverse<i32>, (i32, i32))>,
    nodes: HashMap<RouteCell, RouteNode>,
    visited: std::collections::HashSet<(i32, i32)>,
    /// The grid step: the body radius, coarser for far goals so the node
    /// cap still covers a detour (the field road: 125 m uphill).
    cell: f32,
}

impl RoutePlan {
    #[cfg(test)]
    fn new(from: Vec3, target: Vec3) -> Self {
        Self::to_shape(from, target, None)
    }

    fn to_shape(from: Vec3, target: Vec3, shape: Option<ddstory::scenario::Shape>) -> Self {
        Self {
            origin: from,
            target,
            shape,
            open: [(
                std::cmp::Reverse((from.x - target.x).hypot(from.z - target.z) as i32),
                (0, 0),
            )]
            .into(),
            nodes: [((0, 0), (from, 0.0, (0, 0)))].into(),
            visited: Default::default(),
            cell: ddsim::BODY_RADIUS
                * ((from.x - target.x).hypot(from.z - target.z) / 3000.0).clamp(1.0, 3.0),
        }
    }

    /// Benchmark-only work budget. Yield between node expansions so the
    /// driver itself does not add a hundreds-of-ms hitch to the sample.
    fn advance(
        &mut self,
        ground: &dyn ddsim::Ground,
        param: &ddsim::sweep::SweepParam,
        budget: std::time::Duration,
    ) -> Option<Vec<Vec3>> {
        use std::cmp::Reverse;
        let Self {
            origin,
            target,
            shape,
            open,
            nodes,
            visited,
            cell,
        } = self;
        let cell = *cell;
        let distance = |p: Vec3| (p.x - target.x).hypot(p.z - target.z);
        let start = std::time::Instant::now();
        let mut expanded = false;
        loop {
            if expanded && start.elapsed() >= budget {
                return None;
            }
            let Some((_, key)) = open.pop() else {
                return Some(partial_path(nodes, visited, *target));
            };
            expanded = true;
            if !visited.insert(key) {
                continue;
            }
            let (at, cost, _) = nodes[&key];
            if shape.as_ref().map_or_else(
                || distance(at) < TALK_RANGE - 2.0 * ddsim::BODY_RADIUS,
                |shape| shape.hit(at.to_array(), 0.0),
            ) {
                let mut path = vec![];
                let mut k = key;
                while k != (0, 0) {
                    let (p, _, parent) = nodes[&k];
                    path.push(p);
                    k = parent;
                }
                path.reverse();
                return Some(path);
            }
            if visited.len() > 60000 {
                return Some(partial_path(nodes, visited, *target));
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
                    // Anchor every key to the same grid. Advancing from
                    // the last slid position accumulates lateral drift:
                    // different keys then revisit the same physical wall
                    // and produce a route that loops around it repeatedly.
                    let mx = origin.x + next.0 as f32 * cell - at.x;
                    let mz = origin.z + next.1 as f32 * cell - at.z;
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
    }
}

/// When the planner is exhausted (the goal lies beyond the loaded
/// collision, or the node cap): the path to the explored point nearest the
/// goal, if it is at least 10 m closer than the start, else none. Walking
/// there streams in the next cells and the demo plans again.
fn partial_path(
    nodes: &HashMap<RouteCell, RouteNode>,
    visited: &std::collections::HashSet<(i32, i32)>,
    target: Vec3,
) -> Vec<Vec3> {
    let distance = |p: Vec3| (p.x - target.x).hypot(p.z - target.z);
    let from = distance(nodes[&(0, 0)].0);
    let Some((best, _)) = visited
        .iter()
        .map(|&k| (k, distance(nodes[&k].0)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .filter(|&(_, d)| d < from - 1000.0)
    else {
        return vec![];
    };
    let mut path = vec![];
    let mut k = best;
    while k != (0, 0) {
        let (p, _, parent) = nodes[&k];
        path.push(p);
        k = parent;
    }
    path.reverse();
    path
}

#[cfg(test)]
fn plan_path(
    ground: &dyn ddsim::Ground,
    param: &ddsim::sweep::SweepParam,
    from: Vec3,
    target: Vec3,
) -> Vec<Vec3> {
    RoutePlan::new(from, target)
        .advance(ground, param, std::time::Duration::MAX)
        .unwrap()
}

#[cfg(test)]
mod tests {
    #[test]
    fn installed_item_menu_uses_selected_slot_through_host_and_rejects_other_bags() {
        use super::*;
        let native=std::path::Path::new("E:/SteamLibrary/steamapps/common/DDDA/nativePC");
        if !native.is_dir() {return;}
        let exe=ddexe::locate(native).unwrap();
        let mut session=prepare(native,&exe,2,false,None,false).unwrap();
        // Explicit gameplay fixture: no cutscene/talk/story-owned menu. Bag
        // contents still come from the installed authored q1 reward.
        session.cut=None; session.talk=None; session.host.world.menu=None;
        let reward=ddstory::orders::reward(&session.host.story.as_ref().unwrap().data,1).unwrap();
        for request in reward {
            if let ddstory::Request::Reward {quest,items,..}=request {
                let host=&mut *session.host;
                let data=host.item_data.clone().unwrap();
                for seat in host.roster.ids() {
                    host.economy.opening_grants.reward(&mut host.economy.inventory,&data.items,seat,quest,&items);
                }
            }
        }
        let original_other=session.host.economy.inventory.bag(ddeconomy::inventory::Owner::Arisen(1)).unwrap().clone();
        let fight=session.host.fight.as_mut().unwrap();
        // Preparation stops before the vocation creates fight actors. This
        // recovery fixture explicitly creates them from installed FightData;
        // it is not a claim that the route selected a vocation or gave a reward.
        for seat in [0,1] { fight.add_arisen(seat); }
        if let ddhost::ddenemyrt::combat::Victim::Player {hp,hp_max,..}=&mut fight.arisen.get_mut(&0).unwrap().fighter.victim { *hp=20.0; *hp_max=1000.0; }
        let res=OpeningRes(Mutex::new(session));
        let data=res.item_menu_snapshot(0).unwrap();
        let index=data.bags[0].bag.slots.iter().position(|s|s.item==43).unwrap();
        let row=crate::item_menu::Entry {index,slot:data.bags[0].bag.slots[index]};
        assert!(res.item_menu_use(1,ddeconomy::inventory::Owner::Arisen(0),&row).is_err());
        res.item_menu_use(0,ddeconomy::inventory::Owner::Arisen(0),&row).unwrap();
        // The stale stack count no longer matches: never consume a different
        // live slot using an old presentation row.
        assert!(res.item_menu_use(0,ddeconomy::inventory::Owner::Arisen(0),&row).is_err());
        let mut session=res.0.lock().unwrap();
        assert!(session.host.fight.as_ref().unwrap().arisen[&0].fighter.victim.hp()>20.0);
        assert_eq!(session.host.economy.inventory.bag(ddeconomy::inventory::Owner::Arisen(0)).unwrap().count(43),4);
        assert_eq!(session.host.economy.inventory.bag(ddeconomy::inventory::Owner::Arisen(1)).unwrap(),&original_other);
        let frame=session.host.frame;
        session.host.tick(&Default::default()).unwrap();
        assert_eq!(session.host.frame,frame+1);
    }
    #[test]
    fn installed_second_arisen_has_an_outfit_and_keeps_the_first_seats_look() {
        let native = std::path::Path::new(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC");
        if !native.is_dir() { return; }
        let vfs = ddvfs::Vfs::open(native).unwrap();
        let first = ddlook::save::template_look(&vfs, 1).unwrap();
        let mut looks = vec![Some(first.clone())];
        complete_player_looks(&vfs, &[(0, [0.0; 3], 0.0), (1, [100.0; 3], 1.0)], &mut looks);
        assert_eq!(looks[0], Some(first), "keep the supplied editor/save appearance");
        let second = looks[1].as_ref().expect("installed second-seat appearance");
        assert!(second.parts.iter().any(|p| p.hair));
        assert!(second.parts.iter().any(|p| !p.hair && !p.beard), "second seat has installed outfit parts");
    }

    #[test]
    fn opening_player_bodies_follow_occupied_seats_and_keep_lan_reservations() {
        use ddhost::player::{Control, ProfileId, Roster};
        let mut roster = Roster::default();
        let mut sim = ddsim::Sim::new(HashMap::new());
        assert_eq!(roster.join(ProfileId(1), Control::Local), Some(0));
        sim.add_player(0, [10.0, 20.0, 30.0], 0.25);
        sim.add_player(4, [90.0; 3], 0.0); // A pawn is not another Arisen.
        assert_eq!(initial_player_placements(&roster, &sim, false),
            vec![(0, [10.0, 20.0, 30.0], 0.25)]);
        assert_eq!(initial_player_placements(&roster, &sim, true),
            vec![(0, [10.0, 20.0, 30.0], 0.25), (1, [10.0, 20.0, 30.0], 0.25)]);
        assert_eq!(roster.join(ProfileId(2), Control::Local), Some(1));
        sim.add_player(1, [40.0, 50.0, 60.0], 0.75);
        assert_eq!(initial_player_placements(&roster, &sim, false),
            vec![(0, [10.0, 20.0, 30.0], 0.25), (1, [40.0, 50.0, 60.0], 0.75)]);
        roster.leave(0).unwrap();
        assert_eq!(initial_player_placements(&roster, &sim, false),
            vec![(1, [40.0, 50.0, 60.0], 0.75)], "seat zero is not assumed occupied");
    }

    #[test]
    fn installed_field_npc_cells_admit_idempotently_and_retire_on_unload() {
        let native = std::path::Path::new(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC");
        if !native.is_dir() { return; }
        let exe = native.parent().unwrap().join("DDDA.exe");
        let vfs = ddvfs::Vfs::open(native).unwrap();
        let list = ddhost::npc_placements::group_list(&vfs, 100).unwrap();
        let group = list.groups.iter().find(|g| g.slot == 4).unwrap();
        assert!(group.cells.iter().any(|c| c.m == 0 && c.n == 0));
        let starts = ddhost::install::start_positions(&vfs, 100).unwrap();
        let start = starts.entries.get(2).unwrap();
        let grid = crate::stream::Streamer::field_grid(native).unwrap();
        let cells = [grid.name(grid.index([start.position[0], start.position[1], start.position[2]]))];
        assert_ne!(cells[0], (0, 0), "field arrival uses split-grid coordinates");
        let mut session = prepare(native, &exe, 2, false, None, false).unwrap();
        assert_eq!(session.host.economy.inventory.bag(ddeconomy::inventory::Owner::Arisen(0)).unwrap(),
            &ddeconomy::new_game::bag(ddeconomy::new_game::Phase::NewlyArisen));
        // Fixture observations satisfy this installed group's authored gates;
        // this is not a claim that the full opening route has reached them.
        let story = session.host.story.as_mut().unwrap();
        story.stage.stage_no = 100;
        story.world.stage_no = 100;
        story.world.sce_no = group.scene_window.map_or(1500, |(begin, _)| begin as u32);
        story.world.hour = group.hours.map_or(12, |(begin, _)| u32::from(begin));
        for flag in [group.layout_flag, group.layout_flag2].into_iter().flatten() {
            story.world.set_lot(100, usize::from(flag), true);
        }
        assert!(session.npc_adapter.sync_field_layouts(&vfs, 100, &cells).unwrap());
        session.sync_npc_intels().unwrap();
        let owner = session.host.story_npc_lifetime(304).expect("installed group 4 NPC admitted");
        let admitted: Vec<_> = session.host.story_npc_intels()
            .map(|p| (p.npc, p.target.group, p.target.index)).collect();
        println!("field loaded cells {cells:?}: admitted {admitted:?}");
        assert!(admitted.contains(&(304, 4, 0)));
        session.bind_npc_combat(&vfs, &[]).unwrap();
        assert!(session.host.story_npc_combat_owner(owner).is_none(),
            "admission alone is not a physical combat body");
        // Explicit installed graph/model-readiness fixture; no view completion
        // or quest outcome is fabricated. The ordinary route calls this only
        // after successful model creation and canonical motion binding.
        let target = session.host.story_npc_intels().find(|p| p.npc == 304).unwrap().target;
        session.host.apply_story_npc(100, target, r"ai\fsm\npc\st100\quest\q0002_n304");
        session.npc_authored_motion.bind(&mut *session.host, &[304]).unwrap();
        for _ in 0..20 { session.host.tick(&Default::default()).unwrap(); }
        session.bind_npc_combat(&vfs, &[304]).unwrap();
        let combat_owner = session.host.story_npc_combat_owner(owner).expect("real motion/body binds combat");
        session.host.tick(&Default::default()).unwrap();
        // 1000 x his n04 placement HP rate 2.5 (uNpcIntel::vf05 0x00BBC4E0)
        assert_eq!(session.host.story_npc_combat_facts(owner).unwrap().hp, 2500.0);
        let combat = &session.host.fight.as_ref().unwrap().npcs[&combat_owner];
        assert!(!combat.fighter.hurts.is_empty());
        assert!(session.host.fight.as_ref().unwrap().npc_errors.is_empty());
        session.bind_npc_combat(&vfs, &[304]).unwrap();
        assert_eq!(session.host.story_npc_combat_owner(owner), Some(combat_owner));
        assert!(session.host.story_npc_intels().all(|p| p.stage == 100));
        assert!(!session.npc_adapter.sync_field_layouts(&vfs, 100, &[cells[0], cells[0]]).unwrap());
        session.sync_npc_intels().unwrap();
        assert_eq!(session.host.story_npc_lifetime(304), Some(owner));
        assert!(session.npc_adapter.sync_field_layouts(&vfs, 100, &[]).unwrap());
        session.sync_npc_intels().unwrap();
        assert_eq!(session.host.story_npc_lifetime(304), Some(owner),
            "global NPC intel survives neighboring split-cell unload");
        let retired: Vec<_> = admitted.iter().filter_map(|&(npc, _, _)|
            session.host.story_npc_lifetime(npc as u32).is_none().then_some(npc)).collect();
        println!("split-cell unload retires {retired:?}; global NPC 304 retained");
        assert!(!retired.is_empty(), "arrival fixture also contains a split-cell NPC");
        assert_eq!(session.npc_adapter.field_cells.as_deref(), Some(&[(0, 0)][..]));
        assert!(session.npc_adapter.sync_field_layouts(&vfs, STAGE, &[]).unwrap());
        let story = session.host.story.as_mut().unwrap();
        story.stage.stage_no = STAGE;
        story.world.stage_no = STAGE;
        session.sync_npc_intels().unwrap();
        assert!(session.host.story_npc_lifetime(304).is_none());
        session.host.tick(&Default::default()).unwrap();
        assert!(!session.host.fight.as_ref().unwrap().npcs.contains_key(&combat_owner));
        let story = session.host.story.as_mut().unwrap();
        story.stage.stage_no = 100;
        story.world.stage_no = 100;
        assert!(session.npc_adapter.sync_field_layouts(&vfs, 100, &cells).unwrap());
        session.sync_npc_intels().unwrap();
        assert_ne!(session.host.story_npc_lifetime(304), Some(owner));
        assert_eq!(session.host.roster.ids().count(), 2);
        assert!(session.npc_adapter.sync_field_layouts(&vfs, STAGE, &[]).unwrap());
        assert!(!session.npc_adapter.layouts.contains_key(&100));
    }

    #[test]
    fn installed_rescue_driver_walks_to_real_area_and_empty_fight_does_not_fake_outcome() {
        use ddstory::data::Data;
        struct DriverGround;
        impl ddsim::Ground for DriverGround {
            fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> { None }
        }
        let native = std::path::Path::new(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC");
        if !native.is_dir() { return; }
        let exe = native.parent().unwrap().join("DDDA.exe");
        let mut session = prepare(native, &exe, 2, true, None, false).unwrap();
        let data = &session.host.story.as_ref().unwrap().data;
        let scenarios = ddstory::scenario::Scenarios::load(&data.scenarios(100).unwrap()).unwrap();
        let index = scenarios.tasks[4].iter().position(|entry| entry.name == "q0002_event1").unwrap();
        let target = benchmark_place_from_scenarios(&scenarios, 4, index as u32).unwrap().0;
        let story = session.host.story.as_mut().unwrap();
        story.stage.stage_no = 100; story.world.stage_no = 100;
        story.stage.scenarios = scenarios;
        let position = (target + Vec3::X * 1000.0).to_array(); // Explicit isolated position fixture.
        session.host.sim.player_mut(0).unwrap().position = position;
        session.demo.as_mut().unwrap().step = Step::RescueApproach;
        let input = demo_input(&mut session, 1.0, &DriverGround, None);
        assert_eq!(input.sim.stick, [0.0, 1.0], "ordinary movement heads toward installed Area");
        assert_eq!(session.host.sim.player(0).unwrap().position, position, "driver never places the body");
        assert_eq!(session.demo.as_ref().unwrap().step, Step::RescueApproach);
        assert!(!session.world().quest_bit(2, 25));
        assert!(!session.world().quest_bit(2, 20));
        // Explicit observation fixture: actual event1 entry supplies this in production.
        session.host.story.as_mut().unwrap().world.set_quest_bit(2, 25, true);
        demo_input(&mut session, 2.0, &DriverGround, None);
        assert_eq!(session.demo.as_ref().unwrap().step, Step::Rescue);
        assert!(session.host.fight.as_ref().unwrap().goblins.is_empty());
        demo_rescue(&mut session, 3.0);
        assert_eq!(session.demo.as_ref().unwrap().step, Step::Rescue, "zero live enemies is not an outcome");
        assert!(!session.world().quest_bit(2, 20));
        // Explicit independently observed success fixture, not manufactured by the driver.
        session.host.story.as_mut().unwrap().world.set_quest_bit(2, 20, true);
        demo_rescue(&mut session, 4.0);
        assert_eq!(session.demo.as_ref().unwrap().step, Step::ThankReynard);
    }

    #[test]
    fn installed_intro_waits_on_quest_movie_then_editor_before_q0001() {
        use ddstory::data::Data;
        let native = std::path::Path::new(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC");
        if !native.exists() { return; }
        let exe = native.parent().unwrap().join("DDDA.exe");
        let mut session = prepare(native, &exe, 2, false, None, true).unwrap();
        assert!(session.host.economy.inventory.bag(ddeconomy::inventory::Owner::Arisen(0)).unwrap().slots.is_empty());
        let row = session.host.world.inner.awaiting.unwrap();
        let event = &session.host.story.as_ref().unwrap().data.event_table().unwrap().rows[row];
        assert_eq!((event.stage, event.event), (100, 0));
        assert_eq!(session.world().stage_no, 800);
        assert_eq!(session.world().sce_no, 1000);
        assert!(session.host.story.as_ref().unwrap().quest(0).is_some());
        assert!(session.host.story.as_ref().unwrap().quest(1).is_none());
        assert!(session.intro_movie.as_ref().unwrap().ends_with("movie/event/st100ev00.wmv"));
        assert!(session.intro_edit_done().is_err(), "editor cannot confirm during the movie");
        for _ in 0..120 { session.host.tick(&FrameInputs::new()).unwrap(); }
        assert_eq!(session.host.world.inner.awaiting, Some(row), "movie never auto-completes");
        assert_eq!(session.world().stage_no, 800);
        session.intro_movie_done().unwrap();
        assert_eq!(session.world().stage_no, 801);
        assert!(session.intro_movie_done().is_err(), "movie completion is consumed once");
        assert!(session.host.story.as_ref().unwrap().quest(1).is_none());
        session.intro_edit_done().unwrap();
        for seat in [0, 1] {
            assert_eq!(session.host.economy.inventory.bag(ddeconomy::inventory::Owner::Arisen(seat)).unwrap(),
                &ddeconomy::new_game::bag(ddeconomy::new_game::Phase::NewlyArisen));
        }
        assert_eq!(session.world().stage_no, STAGE);
        assert!(session.host.story.as_ref().unwrap().quest(1).is_some());
        assert_eq!(session.host.roster.ids().count(), 2);
        assert!(session.intro_edit_done().is_err(), "editor completion is consumed once");
    }

    use super::*;
    #[test]
    fn cutscene_lighting_follows_loaded_stage_and_guest_welcome() {
        let mut world = ddstory::World::default();
        world.stage_no = 380;
        world.hour = 12;
        let cfg = cut_config(&world, None, 1, 0);
        assert_eq!(ddstory::event::light_index(20, cfg.stage, cfg.hour), Some(1));
        let guest = cut_config(&world, Some(800), 1, 0);
        assert_eq!(ddstory::event::light_index(20, guest.stage, guest.hour), None);
        world.stage_no = 100;
        let field = cut_config(&world, None, 1, 0);
        assert_eq!(field.stage, 100);
        assert_eq!(ddstory::event::light_index(20, field.stage, field.hour), Some(2));
    }

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
    /// A goal beyond the loaded collision (a 40 m plate here) gives the
    /// path to the plate's edge nearest it, not an empty plan.
    #[test]
    fn a_goal_beyond_the_loaded_ground_gives_a_partial_path_toward_it() {
        struct Plate;
        impl ddsim::Ground for Plate {
            fn height(&self, x: f32, z: f32, below_y: f32) -> Option<f32> {
                (x.abs() < 2000.0 && z.abs() < 2000.0 && below_y >= 0.0).then_some(0.0)
            }
        }
        let goal = Vec3::new(10_000.0, 0.0, 0.0);
        let path = plan_path(&Plate, &ddsim::sweep::SweepParam::default(), Vec3::ZERO, goal);
        let end = *path.last().expect("a partial path");
        assert!(end.x > 1500.0 && end.x < 2000.0, "{end:?}");
        assert!((end - goal).length() < goal.length() - 1000.0);
    }

    #[test]
    fn demo_planner_enters_trigger_without_requiring_its_blocked_center() {
        struct Plate;
        impl ddsim::Ground for Plate {
            fn height(&self, x: f32, z: f32, below: f32) -> Option<f32> {
                (x.abs() < 500.0 && z.abs() < 200.0 && below >= 0.0).then_some(0.0)
            }
        }
        let shape = ddstory::scenario::Shape {
            zone: ddstory::scenario::Zone::Aabb {
                min: [280.0, 0.0, -100.0], max: [1120.0, 200.0, 100.0],
            },
            angle: None, toward: None,
        };
        let mut plan = RoutePlan::to_shape(Vec3::ZERO, Vec3::new(700.0, 0.0, 0.0), Some(shape.clone()));
        let path = plan.advance(&Plate, &ddsim::sweep::SweepParam::default(),
            std::time::Duration::MAX).unwrap();
        assert!(shape.hit(path.last().expect("walk into the trigger").to_array(), 0.0));
        assert!(path.last().unwrap().x < 500.0, "centre has no walkable ground");
    }

    #[test]
    fn installed_road_planner_enters_area_without_repeated_wall_loops() {
        let native = std::path::Path::new(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC");
        if !native.is_dir() { return; }
        let vfs = ddvfs::Vfs::open(native).unwrap();
        let bytes = vfs.read(r"scr\st100\etc\st100_fsm", mtarc::type_hash("rScenario")).unwrap();
        let scenarios = ddstory::scenario::Scenarios::load(&[(4, std::sync::Arc::new(mtxfs::parse(&bytes).unwrap()))]).unwrap();
        let index = scenarios.tasks[4].iter().position(|entry| entry.name == "q0002_event2").unwrap();
        let shape = scenarios.tasks[4][index].shape.clone().unwrap();
        assert!(shape.angle.is_none() && shape.toward.is_none());
        let target = benchmark_place_from_scenarios(&scenarios, 4, index as u32).unwrap().0;
        // Recorded S46 RoadWalk checkpoint position, replay fixture only.
        let from = Vec3::new(35764.04, 31966.314, 141997.23);
        let grid = crate::stream::Streamer::field_grid(native).unwrap();
        let mut ground = ddworld::Terrain::load_collision_cells(&vfs, 100,
            grid.wanted(ddworld::stream::Kind::Collision, from.to_array(), 1.0));
        ground.add_scenery_cells(&vfs, &ddworld::Terrain::scenery_index(&vfs), 100,
            grid.wanted(ddworld::stream::Kind::Layout, from.to_array(), 1.0));
        let mut plan = RoutePlan::to_shape(from, target, Some(shape.clone()));
        let path = plan.advance(&ground, &ddsim::sweep::SweepParam::default(),
            std::time::Duration::MAX).unwrap();
        println!("road collision search: {} nodes, {} visited, {} waypoints; end {:?}; target {:?}; shape {:?}",
            plan.nodes.len(), plan.visited.len(), path.len(), path.last(), target, shape);
        assert!(shape.hit(path.last().expect("real road route").to_array(), 0.0));
        for (index, point) in path.iter().enumerate() {
            assert!(!path[..index].iter().any(|previous| (*point - *previous).length() < 1.0),
                "route must not revisit the same wall position: {point:?}");
        }
    }

    #[test]
    fn road_deadline_and_authored_outcome_survive_a_pending_search() {
        struct Flat;
        impl ddsim::Ground for Flat {
            fn height(&self, _: f32, _: f32, _: f32) -> Option<f32> { Some(0.0) }
        }
        let native = std::path::Path::new(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC");
        if !native.is_dir() { return; }
        let exe = native.parent().unwrap().join("DDDA.exe");
        let mut session = prepare(native, &exe, 2, true, None, false).unwrap();
        session.host.story.as_mut().unwrap().world.stage_no = 100;
        session.host.sim.player_mut(0).unwrap().position = [0.0; 3];
        let d = session.demo.as_mut().unwrap();
        d.step = Step::RoadWalk; d.since = 1.0;
        d.planning = Some(RoutePlan::new(Vec3::ZERO, Vec3::new(10_000.0, 0.0, 0.0)));
        demo_input(&mut session, 302.0, &Flat, None);
        assert_eq!(session.demo.as_ref().unwrap().step, Step::Done);
        assert!(!session.world().quest_bit(2, 10), "timeout cannot invent success");
        assert!(session.host.story.as_ref().unwrap().quest(3).is_none());
        let d = session.demo.as_mut().unwrap();
        d.step = Step::RoadWalk; d.since = 1.0;
        d.planning = Some(RoutePlan::new(Vec3::ZERO, Vec3::new(10_000.0, 0.0, 0.0)));
        // Explicit observation fixture: production event2 writes the bit.
        session.host.story.as_mut().unwrap().world.set_quest_bit(2, 10, true);
        demo_input(&mut session, 2.0, &Flat, None);
        assert_eq!(session.demo.as_ref().unwrap().step, Step::Done);
        assert!(session.world().quest_bit(2, 10));
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
    fn doorway_demo_goal_is_beyond_the_opposite_face() {
        let zone = ddstory::scenario::Zone::Obb {
            m: Mat4::from_rotation_y(0.7).to_cols_array_2d(), ext: [100.0, 200.0, 80.0],
        };
        for from in [Vec3::new(0.0, 0.0, -500.0), Vec3::new(0.0, 0.0, 500.0)] {
            let goal = demo_through_box(&zone, from).unwrap();
            assert!(!zone.contains(goal.to_array()));
            assert!(goal.dot(from) < 0.0, "the route must cross to the far side");
            // Walking can stop up to60cm before the target; that point
            // must still have crossed the shape, not merely entered it.
            let stop = goal - goal.normalize() * 60.0;
            assert!(!zone.contains(stop.to_array()));
        }
    }
    #[test]
    fn local_input_survives_zero_ticks_and_consumes_edges_once() {
        let pressed = Input { jump: true, light: true, heavy: true, stick: [0.0, 1.0], ..Default::default() };
        let mut pending = HashMap::from([(1, pressed), (2, Input::default())]);
        // No host tick in this render frame; the next render has no edge.
        *pending.get_mut(&1).unwrap() = play::latch_input(pending[&1], Input::default());
        let mut frame = pending.clone();
        let first = consume_local_input(&mut frame, 1);
        pending.clear();
        assert!(first.jump && first.light && first.heavy);
        let second = consume_local_input(&mut frame, 1);
        assert!(!second.jump && !second.light && !second.heavy);
        assert_eq!(second.stick, first.stick);
        assert!(!consume_local_input(&mut frame, 2).jump);
        assert!(pending.is_empty(), "consumed edges cannot leak into the next render");
    }
    #[test]
    fn demo_door_planning_preserves_real_collision_and_rejects_unknown_locks() {
        use ddsim::Ground;
        let native = std::path::Path::new(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC");
        if !native.exists() { return; }
        let vfs = ddvfs::Vfs::open(native).unwrap();
        let exe = ddexe::ExeFile::next_to(native).unwrap();
        let families = dddoor::placement::Families::read(&exe.image().unwrap()).unwrap();
        let groups = dddoor::placement::groups(&vfs, STAGE).unwrap();
        let world = ddhost::ddspawn::World { scene: 1400, hour: 12, flag: &|_| false };
        let placements = dddoor::placement::candidates(&vfs, &families, STAGE, &groups, &[(0, 0)], &world).unwrap();
        let door = placements.doors.into_iter().find(|door| door.id.group == 18 && door.id.record == 2).unwrap();
        let id = door.id;
        let mut doors = ddhost::doors::Doors::load(&vfs, STAGE, vec![door], |_| dddoor::policy::Observed::fresh(12)).unwrap();
        let base = ddworld::Terrain::empty();
        let lo = [-100_000.0; 3]; let hi = [100_000.0; 3];
        let mut faces = Vec::new();
        DemoPlanningGround::new(&base, Some(&doors)).triangles_in(lo, hi, &mut faces);
        assert!(faces.is_empty(), "an unlocked door may be planned as an interaction transition");
        doors.collision.over(&base).triangles_in(lo, hi, &mut faces);
        assert!(!faces.is_empty(), "the actual closed leaf still blocks movement");
        faces.clear();
        doors.entries.get_mut(&id).unwrap().observed.permanently_open = None;
        DemoPlanningGround::new(&base, Some(&doors)).triangles_in(lo, hi, &mut faces);
        assert!(!faces.is_empty(), "unknown policy cannot authorize a planned crossing");
        faces.clear();
        let entry = doors.entries.get_mut(&id).unwrap();
        entry.observed.permanently_open = Some(false);
        entry.observed.key_present = Some(false);
        entry.policy.mode = 1;
        DemoPlanningGround::new(&base, Some(&doors)).triangles_in(lo, hi, &mut faces);
        assert!(!faces.is_empty(), "a locked leaf remains a planning obstacle");
        assert!(doors.states[&id].collision);
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
        // Yield after every expansion, then verify resumed search retains
        // the same collision-safe route as one uninterrupted search.
        let mut plan = RoutePlan::new(Vec3::ZERO, Vec3::new(400.0, 0.0, 0.0));
        let param = ddsim::sweep::SweepParam::default();
        assert!(plan.advance(&ground, &param, std::time::Duration::ZERO).is_none());
        let resumed = (0..100_000)
            .find_map(|_| plan.advance(&ground, &param, std::time::Duration::ZERO))
            .expect("incremental search must finish");
        assert_eq!(resumed, path);
    }
}
