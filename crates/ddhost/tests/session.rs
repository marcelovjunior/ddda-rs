//! A two-player session with the host-owned systems: a shop purchase under
//! the interaction lock, a Rift hire, a level-up; then a save and a load
//! into a fresh host, which must hold the same saved state (equal
//! `saved_hash`), also when the players come back on swapped seats.
//! Needs the install and DDDA.exe (DDDA_NATIVE_PC, DDDA_EXE); skips
//! without them.

use ddeconomy::inventory::Owner;
use ddhost::data::NoStoryData;
use ddhost::lock::LockError;
use ddhost::systems::SystemData;
use ddhost::{Control, FrameInputs, Host, HostError, Member, PlayerInput, ProfileId};
use ddpawn::cmc::Cmc;
use ddprogress::{Award, Role};
use ddsim::{Ground, Input, Sim};
use ddskill::vocation::Vocation;
use ddstory::scripted::ScriptedHost;
use ddstory::Story;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

struct Flat;
impl Ground for Flat {
    fn height(&self, _: f32, _: f32, below_y: f32) -> Option<f32> {
        (below_y >= 0.0).then_some(0.0)
    }
}

fn data() -> Option<SystemData> {
    static D: OnceLock<Option<SystemData>> = OnceLock::new();
    D.get_or_init(|| {
        let native = std::env::var_os("DDDA_NATIVE_PC")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC"));
        let exe = std::env::var_os("DDDA_EXE").map(PathBuf::from).or_else(|| ddexe::locate(&native).ok());
        let Some(exe) = exe.filter(|e| native.is_dir() && e.is_file()) else {
            eprintln!("SKIP: no install or DDDA.exe");
            return None;
        };
        let vfs = ddvfs::Vfs::open(&native).unwrap();
        Some(SystemData {
            catalog: Some(Arc::new(ddhost::install::catalog(&vfs).unwrap())),
            tables: Some(Arc::new(ddhost::install::tables(&vfs, &exe).unwrap())),
            rift: None,
            npc: None,
        })
    })
    .clone()
}

const HOST: ProfileId = ProfileId(0x1111);
const GUEST: ProfileId = ProfileId(0x2222);
/// Caxton (NPC 149) and his Trusty Sword (item 307): the shop
/// ddeconomy's install test opens at SceNo 8000.
const CAXTON: u32 = 149;
const SWORD: u32 = 307;

/// A fresh host with the two profiles seated in `order`.
fn host(data: &SystemData, order: [ProfileId; 2]) -> Host<NoStoryData, ScriptedHost> {
    let mut h = Host::new(Sim::new(HashMap::new()), Some(Story::new_game(NoStoryData)), ScriptedHost::default(), Box::new(Flat));
    h.data = data.clone();
    h.join(order[0], Control::Local, [0.0, 0.0, 0.0], 0.0).unwrap();
    h.join(order[1], Control::Remote, [200.0, 0.0, 0.0], 0.0).unwrap();
    h
}

fn inputs(frame: u64) -> FrameInputs {
    let t = frame as f32;
    let mut m = FrameInputs::new();
    m.insert(0, PlayerInput { sim: Input { stick: [0.0, 1.0], camera_yaw: t * 0.02, ..Default::default() }, interact: false });
    m.insert(1, PlayerInput { sim: Input { stick: [1.0, 0.0], walk: true, ..Default::default() }, interact: false });
    m
}

fn row(id: u32, level: u8) -> ddrift::Row {
    let mut pawn = Cmc::initial();
    pawn.param.level = level;
    pawn.rom_id = id;
    ddrift::Row { flags: 0, pawn, online: ddrift::PawnRef::rom(id) }
}

fn level(h: &Host<NoStoryData, ScriptedHost>, role: Role) -> u32 {
    let m = h.progression.party.iter().flatten().find(|m| m.role == role).unwrap();
    h.progression.characters[m.character].progress.level
}

/// The session up to its save.
fn play(data: &SystemData) -> Host<NoStoryData, ScriptedHost> {
    let mut h = host(data, [HOST, GUEST]);
    h.create_arisen(0, Vocation::Mage).unwrap();
    h.create_arisen(1, Vocation::Fighter).unwrap();
    h.create_main_pawn(Vocation::Strider).unwrap();
    h.story.as_mut().unwrap().world.set_sce_no(8000, false);
    h.economy.wallet(0).gold = 10_000;
    h.economy.wallet(1).rift_crystals = 5000;
    for f in 0..30 {
        h.tick(&inputs(f)).unwrap();
    }

    // A purchase under the interaction lock.
    let s = h.open_shop(0, CAXTON).unwrap();
    assert!(matches!(h.open_shop(1, CAXTON), Err(HostError::Lock(LockError::Held(0)))));
    assert!(matches!(h.save(), Err(HostError::Busy)), "no save inside a shop");
    let paid = h.buy(&s, SWORD, 1, Owner::Arisen(0)).unwrap();
    assert_eq!(paid, 2700);
    h.close_shop(s);
    assert_eq!(h.economy.wallets[&0].gold, 10_000 - 2700);
    // The guest may use the shop now.
    let s = h.open_shop(1, CAXTON).unwrap();
    h.close_shop(s);

    // A hire at the Riftstone, paid from the guest's crystals.
    h.rift_enter(1).unwrap();
    let mut r = row(0x1000_0007, 10);
    let room = h.rift_summon(1, &mut r).unwrap();
    let slot = h.rift_hire(1, room).unwrap();
    assert_eq!(slot, 1);
    assert!(h.economy.wallets[&1].rift_crystals < 5000);
    assert_eq!(h.party.slots[2], Some(Member::Hired { owner: 1, slot: 1 }));
    h.rift_leave(1);
    assert!(!h.interacting());

    // A level-up: the award, then one level per frame in the tick.
    h.award_exp(Award::new(3000)).unwrap();
    for f in 30..90 {
        h.tick(&inputs(f)).unwrap();
    }
    assert!(level(&h, Role::Arisen(0)) > 1);
    assert!(level(&h, Role::Arisen(1)) > 1);
    h
}

#[test]
fn save_and_load_keep_the_saved_state() {
    let Some(data) = data() else { return };
    let a = play(&data);
    let bytes = a.save().unwrap().to_bytes();
    let file = ddhost::save::SaveFile::from_bytes(&bytes).unwrap();

    let mut b = host(&data, [HOST, GUEST]);
    b.load(&file, NoStoryData).unwrap();
    assert_eq!(b.economy, a.economy);
    assert_eq!(b.progression, a.progression);
    assert_eq!(b.rift.party, a.rift.party);
    assert_eq!(b.party, a.party);
    assert_eq!(b.saved_hash(), a.saved_hash());
    // And the reloaded host saves the same bytes again.
    assert_eq!(b.save().unwrap().to_bytes(), bytes);
}

#[test]
fn load_on_swapped_seats_follows_the_profiles() {
    let Some(data) = data() else { return };
    let a = play(&data);
    let file = a.save().unwrap();
    // The guest's machine hosts next time: GUEST takes seat 0.
    let mut c = host(&data, [GUEST, HOST]);
    c.load(&file, NoStoryData).unwrap();
    assert_eq!(c.economy.wallets[&0], a.economy.wallets[&1]);
    assert_eq!(c.economy.wallets[&1], a.economy.wallets[&0]);
    assert_eq!(c.economy.inventory.bag(Owner::Arisen(1)), a.economy.inventory.bag(Owner::Arisen(0)));
    assert_eq!(level(&c, Role::Arisen(0)), level(&a, Role::Arisen(1)));
    // The hired pawn stays with the profile that hired it.
    assert_eq!(c.party.slots[2], Some(Member::Hired { owner: 0, slot: 1 }));
    // The guest's place follows its profile too.
    assert_eq!(c.sim.player(0).unwrap().position, a.sim.player(1).unwrap().position);
}
