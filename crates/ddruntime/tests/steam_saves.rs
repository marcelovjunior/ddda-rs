//! Brief L26: the offline Steam services (crate::steam) the game calls,
//! and its save data in the sandbox.
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;
use ddruntime::dispatch::{Binding, Registry};
use ddruntime::interfaces::{Tokens, METHOD_TOKENS};
use ddruntime::memory::{GuestMemory, READ, WRITE};
use ddruntime::{services, steam};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const STACK: u32 = 0x0011_0000;
/// Guest scratch: strings, buffers and the test's callback objects.
const SCRATCH: u32 = 0x0030_0000;

/// Every interface method the census found the game calling (slot).
const CENSUS: &[(&str, &[u32])] = &[
    ("ISteamUtils", &[4, 9, 16, 20, 22]),
    ("ISteamUser", &[1, 2, 20, 21]),
    ("ISteamUserStats", &[7, 10, 23, 28, 29, 30, 31]),
    ("ISteamFriends", &[0, 3, 4, 5, 7, 29, 37]),
    ("ISteamApps", &[4, 17]),
    ("ISteamRemoteStorage", &[0, 1, 4, 10, 12, 18, 19, 21, 24]),
    ("ISteamScreenshots", &[2]),
];

thread_local! {
    /// (object, kind, parameter block contents, bIOFailure, call)
    static LOG: RefCell<Vec<(u32, &'static str, Vec<u8>, u32, u64)>> = const { RefCell::new(Vec::new()) };
}

// A guest CCallback/CCallResult: vtable at +0 (slots Run(void*, bool,
// SteamAPICall_t), Run(void*), GetCallbackSizeBytes), the result size the
// test wants reported at +0x20.
const RUN_RESULT: u32 = 0x0050_0000;
const RUN_CALLBACK: u32 = 0x0050_0010;
const SIZE: u32 = 0x0050_0020;
/// A callback that calls SteamAPI_RunCallbacks and FindLeaderboard from
/// inside its Run(void*).
const RUN_REENTER: u32 = 0x0050_0030;
const VTABLE: u32 = SCRATCH + 0x3000;
const VTABLE_REENTER: u32 = SCRATCH + 0x3010;

fn block(m: &dyn Mem, pv: u32, obj: u32) -> Vec<u8> {
    (0..m.u32(obj + 0x20)).map(|k| m.u8(pv + k)).collect()
}

fn run_result(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (pv, io, lo, hi) = (m.u32(cpu.esp + 4), m.u32(cpu.esp + 8), m.u32(cpu.esp + 12), m.u32(cpu.esp + 16));
    let b = block(m, pv, cpu.ecx);
    LOG.with(|l| l.borrow_mut().push((cpu.ecx, "result", b, io, lo as u64 | (hi as u64) << 32)));
    cpu.esp += 4 + 16;
}

fn run_callback(m: &mut dyn Mem, cpu: &mut Cpu) {
    let pv = m.u32(cpu.esp + 4);
    let b = block(m, pv, cpu.ecx);
    LOG.with(|l| l.borrow_mut().push((cpu.ecx, "callback", b, 0, 0)));
    cpu.esp += 4 + 4;
}

fn size(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = m.u32(cpu.ecx + 0x20);
    cpu.esp += 4;
}

fn run_reenter(m: &mut dyn Mem, cpu: &mut Cpu) {
    let this = cpu.ecx;
    run_callback(m, cpu);
    // as lifted code calls a service: arguments, return address, dispatch
    let saved = (cpu.esp, cpu.ebx, cpu.ebp, cpu.esi, cpu.edi);
    cpu.esp -= 4;
    m.set_u32(cpu.esp, 0x0040_9999);
    steam::run_callbacks(m, cpu);
    let stats = call_steam(m, cpu, steam::steam_user_stats, &[]);
    put_str(m, SCRATCH + 0x2F00, "leader_1");
    cpu.eax = method_on(m, cpu, stats, 23, &[SCRATCH + 0x2F00]);
    m.set_u32(this + 0x24, cpu.eax);
    (cpu.esp, cpu.ebx, cpu.ebp, cpu.esi, cpu.edi) = saved;
}

struct Guest {
    m: GuestMemory,
    _registry: Arc<Registry>,
    home: PathBuf,
}

fn home(tag: &str) -> PathBuf {
    let h = std::env::temp_dir().join(format!("ddruntime-steam-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&h);
    h
}

/// A guest with the Steam interfaces installed and the user configured
/// from `home`.
fn guest(home: PathBuf) -> Guest {
    let mut m = GuestMemory::new();
    m.map(0x0010_0000, 0x10000, READ | WRITE).unwrap();
    m.map(SCRATCH, 0x4000, READ | WRITE).unwrap();
    m.map_lazy(services::HEAP_BASE, services::HEAP_SIZE, READ | WRITE).unwrap();
    let mut r = Registry::new();
    let mut tokens = Tokens::default();
    steam::install(&mut m, &mut r, &mut tokens).unwrap();
    m.map(METHOD_TOKENS, tokens.span(), READ).unwrap();
    for (a, f) in [(RUN_RESULT, run_result as fn(&mut dyn Mem, &mut Cpu)), (RUN_CALLBACK, run_callback), (SIZE, size), (RUN_REENTER, run_reenter)] {
        r.insert(a, Binding::Lifted(f)).unwrap();
    }
    for (k, a) in [RUN_RESULT, RUN_CALLBACK, SIZE].iter().enumerate() {
        m.set_u32(VTABLE + 4 * k as u32, *a);
    }
    for (k, a) in [RUN_RESULT, RUN_REENTER, SIZE].iter().enumerate() {
        m.set_u32(VTABLE_REENTER + 4 * k as u32, *a);
    }
    let r = Arc::new(r);
    r.install();
    steam::configure(&home);
    LOG.with(|l| l.borrow_mut().clear());
    Guest { m, _registry: r, home }
}

fn put_str(m: &mut dyn Mem, at: u32, s: &str) {
    for (k, b) in s.bytes().chain([0]).enumerate() {
        m.set_u8(at + k as u32, b);
    }
}

fn get_str(m: &dyn Mem, at: u32) -> String {
    (0..).map(|k| m.u8(at + k)).take_while(|&b| b != 0).map(|b| b as char).collect()
}

/// A cdecl export, called with `args` (the caller pops them).
fn call_steam(m: &mut dyn Mem, cpu: &mut Cpu, f: fn(&mut dyn Mem, &mut Cpu), args: &[u32]) -> u32 {
    let esp = cpu.esp;
    for &a in args.iter().rev() {
        cpu.esp -= 4;
        m.set_u32(cpu.esp, a);
    }
    cpu.esp -= 4;
    m.set_u32(cpu.esp, 0x0040_5678);
    f(m, cpu);
    assert_eq!(cpu.esp, esp - 4 * args.len() as u32, "cdecl: the caller pops");
    cpu.esp = esp;
    cpu.eax
}

/// A thiscall method through the object's vtable; checks the callee pops
/// exactly `args`.
fn method_on(m: &mut dyn Mem, cpu: &mut Cpu, obj: u32, slot: u32, args: &[u32]) -> u32 {
    let esp = cpu.esp;
    for &a in args.iter().rev() {
        cpu.esp -= 4;
        m.set_u32(cpu.esp, a);
    }
    cpu.esp -= 4;
    m.set_u32(cpu.esp, 0x0040_5678);
    cpu.ecx = obj;
    let target = m.u32(m.u32(obj) + 4 * slot);
    ddnative_base::x86::call_ext(m, cpu, target);
    assert_eq!(cpu.esp, esp, "thiscall: the callee pops its {} argument word(s)", args.len());
    cpu.eax
}

impl Guest {
    fn cpu(&self) -> Cpu {
        Cpu { esp: STACK, ..Cpu::default() }
    }
    fn object(&mut self, f: fn(&mut dyn Mem, &mut Cpu)) -> u32 {
        let mut cpu = self.cpu();
        call_steam(&mut self.m, &mut cpu, f, &[])
    }
    /// (eax, edx) of a method call.
    fn call(&mut self, accessor: fn(&mut dyn Mem, &mut Cpu), slot: u32, args: &[u32]) -> (u32, u32) {
        let obj = self.object(accessor);
        let mut cpu = self.cpu();
        let eax = method_on(&mut self.m, &mut cpu, obj, slot, args);
        (eax, cpu.edx)
    }
    /// The string a method returns.
    fn string(&mut self, accessor: fn(&mut dyn Mem, &mut Cpu), slot: u32, args: &[u32]) -> String {
        let p = self.call(accessor, slot, args).0;
        get_str(&self.m, p)
    }
    fn run_callbacks(&mut self) {
        let mut cpu = self.cpu();
        call_steam(&mut self.m, &mut cpu, steam::run_callbacks, &[]);
        assert_eq!((cpu.ebx, cpu.ebp, cpu.esi, cpu.edi), (0, 0, 0, 0));
    }
    fn flat(&mut self, f: fn(&mut dyn Mem, &mut Cpu), args: &[u32]) -> u32 {
        let mut cpu = self.cpu();
        call_steam(&mut self.m, &mut cpu, f, args)
    }
    /// A guest CCallbackBase at `at` reporting `size` bytes.
    fn callback_object(&mut self, at: u32, vtable: u32, id: u32, size: u32) -> u32 {
        for k in 0..0x28 {
            self.m.set_u8(at + k, 0);
        }
        self.m.set_u32(at, vtable);
        self.m.set_u32(at + 8, id);
        self.m.set_u32(at + 0x20, size);
        at
    }
    fn remote(&self, account: u32) -> PathBuf {
        steam::app_dir(&self.home, account).join("remote")
    }
}

fn log() -> Vec<(u32, &'static str, Vec<u8>, u32, u64)> {
    LOG.with(|l| l.borrow_mut().drain(..).collect())
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

#[test]
fn every_method_the_game_calls_is_bound_and_the_rest_are_named() {
    let mut m = GuestMemory::new();
    m.map_lazy(services::HEAP_BASE, services::HEAP_SIZE, READ | WRITE).unwrap();
    let mut r = Registry::new();
    let mut tokens = Tokens::default();
    steam::install(&mut m, &mut r, &mut tokens).unwrap();
    let objects = [("ISteamUtils", steam::steam_utils as fn(&mut dyn Mem, &mut Cpu)), ("ISteamUser", steam::steam_user),
        ("ISteamUserStats", steam::steam_user_stats), ("ISteamFriends", steam::steam_friends), ("ISteamApps", steam::steam_apps),
        ("ISteamRemoteStorage", steam::steam_remote_storage), ("ISteamScreenshots", steam::steam_screenshots)];
    m.map(0x0010_0000, 0x10000, READ | WRITE).unwrap();
    for (name, slots) in CENSUS {
        let f = objects.iter().find(|o| o.0 == *name).unwrap().1;
        let mut cpu = Cpu { esp: STACK, ..Cpu::default() };
        let obj = call_steam(&mut m, &mut cpu, f, &[]);
        let spec = steam::INTERFACES.iter().find(|s| s.name == *name).unwrap();
        assert_eq!(spec.names.len() as u32, spec.slots);
        for k in 0..spec.slots {
            let token = m.u32(m.u32(obj) + 4 * k);
            assert_eq!(r.get(token).is_some(), slots.contains(&k), "{name} slot {k} ({})", spec.names[k as usize]);
        }
    }
    // SDK 1.33 slot names at the census's slots
    let name = |i: &str, k: usize| steam::INTERFACES.iter().find(|s| s.name == i).unwrap().names[k];
    assert_eq!(name("ISteamUserStats", 7), "SetAchievement");
    assert_eq!(name("ISteamUserStats", 31), "UploadLeaderboardScore");
    assert_eq!(name("ISteamFriends", 37), "RequestUserInformation");
    assert_eq!(name("ISteamRemoteStorage", 24), "UGCRead");
    assert_eq!(name("ISteamUtils", 22), "GetEnteredGamepadTextInput");
    assert_eq!(name("ISteamApps", 17), "GetInstalledDepots");
}

#[test]
fn the_local_user_is_one_offline_individual() {
    let h = home("user");
    std::fs::create_dir_all(h.join("steam")).unwrap();
    std::fs::write(h.join("steam/user.txt"), "account=4242\nname=Tester\n").unwrap();
    let mut g = guest(h.clone());
    let id = SCRATCH + 0x100;
    let (eax, _) = g.call(steam::steam_user, 2, &[id]);
    assert_eq!(eax, id, "GetSteamID returns its hidden result pointer");
    let sid = g.m.u32(id) as u64 | (g.m.u32(id + 4) as u64) << 32;
    assert_eq!(sid, 76_561_197_960_265_728 + 4242, "public individual desktop SteamID");
    assert_eq!(g.call(steam::steam_user, 1, &[]).0, 0, "BLoggedOn: no connection");
    assert_eq!(g.string(steam::steam_friends, 0, &[]), "Tester");
    assert_eq!(g.call(steam::steam_friends, 3, &[4]).0, u32::MAX, "GetFriendCount: -1 when not logged on");
    let out = SCRATCH + 0x110;
    g.m.set_u32(out, 0xAAAA_AAAA);
    assert_eq!(g.call(steam::steam_friends, 4, &[out, 0, 4]).0, out);
    assert_eq!((g.m.u32(out), g.m.u32(out + 4)), (0, 0), "GetFriendByIndex: k_steamIDNil");
    assert_eq!(g.call(steam::steam_friends, 5, &[1, 0x0110_0001]).0, 0, "k_EFriendRelationshipNone");
    assert_eq!(g.string(steam::steam_friends, 7, &[sid as u32, (sid >> 32) as u32]), "Tester");
    assert_eq!(g.string(steam::steam_friends, 7, &[7, 0x0110_0001]), "", "unknown users: \"\"");
    assert_eq!(g.call(steam::steam_friends, 37, &[7, 0x0110_0001, 1]).0, 0, "RequestUserInformation: nothing pending");
    put_str(&mut g.m, SCRATCH + 0x200, "steamid");
    g.call(steam::steam_friends, 29, &[SCRATCH + 0x200, 0, 0]);
    assert_eq!(g.string(steam::steam_utils, 4, &[]), steam::DEFAULT_COUNTRY);
    assert_eq!(g.call(steam::steam_utils, 9, &[]).0, 367_500);
    g.call(steam::steam_utils, 16, &[0x00DF_0000]);
    assert_eq!(g.call(steam::steam_utils, 20, &[0, 0, SCRATCH + 0x200, 0x100, SCRATCH + 0x200]).0 & 0xFF, 0, "no Big Picture overlay");
    assert_eq!(g.call(steam::steam_utils, 22, &[SCRATCH + 0x300, 0x100]).0 & 0xFF, 0);
    assert_eq!(g.string(steam::steam_apps, 4, &[]), "english");
    assert_eq!(g.call(steam::steam_apps, 17, &[367_500, SCRATCH + 0x300, 0x20]).0, 0);
    g.call(steam::steam_screenshots, 2, &[]);
    g.run_callbacks();
    assert!(log().is_empty(), "nothing is posted by these calls");
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn callbacks_arrive_at_run_callbacks_for_their_registered_objects() {
    let h = home("callbacks");
    let mut g = guest(h.clone());
    let stored = g.callback_object(SCRATCH + 0x1000, VTABLE, 0, 16);
    let achieved = g.callback_object(SCRATCH + 0x1040, VTABLE, 0, 152);
    let other = g.callback_object(SCRATCH + 0x1080, VTABLE, 0, 1);
    g.flat(steam::register_callback, &[stored, steam::USER_STATS_STORED]);
    g.flat(steam::register_callback, &[achieved, steam::USER_ACHIEVEMENT_STORED]);
    g.flat(steam::register_callback, &[other, steam::GAME_OVERLAY_ACTIVATED]);
    assert_eq!((g.m.u8(stored + 4), g.m.u32(stored + 8)), (1, steam::USER_STATS_STORED), "registered flag and m_iCallback");
    put_str(&mut g.m, SCRATCH + 0x200, "ACH_ACHIEVEMENT_07");
    assert_eq!(g.call(steam::steam_user_stats, 7, &[SCRATCH + 0x200]).0 & 0xFF, 1, "SetAchievement");
    assert_eq!(g.call(steam::steam_user_stats, 10, &[]).0 & 0xFF, 1, "StoreStats");
    assert!(log().is_empty(), "nothing before SteamAPI_RunCallbacks");
    g.run_callbacks();
    let l = log();
    assert_eq!(l.len(), 2, "{l:?}");
    assert_eq!((l[0].0, l[0].1), (stored, "callback"));
    assert_eq!((u64_at(&l[0].2, 0), u32_at(&l[0].2, 8)), (367_500, 1), "UserStatsStored_t: game, k_EResultOK");
    assert_eq!((l[1].0, l[1].1), (achieved, "callback"));
    assert_eq!(u64_at(&l[1].2, 0), 367_500);
    assert_eq!(get_str_bytes(&l[1].2[9..]), "ACH_ACHIEVEMENT_07");
    assert_eq!((u32_at(&l[1].2, 140), u32_at(&l[1].2, 144)), (0, 0), "an unlock, not progress");
    assert!(steam::achievements().contains("ACH_ACHIEVEMENT_07"));
    // storing again: no new unlock, only UserStatsStored_t
    g.call(steam::steam_user_stats, 10, &[]);
    g.run_callbacks();
    assert_eq!(log().iter().map(|e| e.0).collect::<Vec<_>>(), vec![stored]);
    // unregistered objects get nothing, and the flag is cleared
    g.flat(steam::unregister_callback, &[stored]);
    assert_eq!(g.m.u8(stored + 4), 0);
    g.call(steam::steam_user_stats, 10, &[]);
    g.run_callbacks();
    assert!(log().is_empty());
    // the achievement is kept in the sandbox for the next session
    steam::configure(&h);
    assert!(steam::achievements().contains("ACH_ACHIEVEMENT_07"));
    assert!(std::fs::read_to_string(steam::app_dir(&h, steam::DEFAULT_ACCOUNT).join("achievements.txt")).unwrap().contains("ACH_ACHIEVEMENT_07"));
    let _ = std::fs::remove_dir_all(&h);
}

fn get_str_bytes(b: &[u8]) -> String {
    b.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect()
}

#[test]
fn server_requests_complete_offline_as_call_results() {
    let h = home("results");
    let mut g = guest(h.clone());
    put_str(&mut g.m, SCRATCH + 0x200, "leader_3");
    // FindLeaderboard: a handle now, LeaderboardFindResult_t (16 bytes) later
    let (lo, hi) = g.call(steam::steam_user_stats, 23, &[SCRATCH + 0x200]);
    let call = lo as u64 | (hi as u64) << 32;
    assert_ne!(call, 0, "a valid SteamAPICall_t");
    let find = g.callback_object(SCRATCH + 0x1000, VTABLE, steam::LEADERBOARD_FIND_RESULT, 16);
    g.flat(steam::register_call_result, &[find, lo, hi]);
    assert_eq!(g.m.u8(find + 4), 0, "RegisterCallResult leaves the flags alone");
    // the ticket: EncryptedAppTicketResponse_t with k_EResultNoConnection
    let (tlo, thi) = g.call(steam::steam_user, 20, &[0, 0]);
    let ticket = g.callback_object(SCRATCH + 0x1040, VTABLE, steam::ENCRYPTED_APP_TICKET_RESPONSE, 4);
    g.flat(steam::register_call_result, &[ticket, tlo, thi]);
    // an upload nobody registers for is dropped
    g.call(steam::steam_user_stats, 31, &[1, 0, 2, 100, SCRATCH + 0x300, 2]);
    g.run_callbacks();
    let l = log();
    assert_eq!(l.len(), 2, "{l:?}");
    assert_eq!((l[0].0, l[0].1, l[0].3, l[0].4), (find, "result", 1, call), "bIOFailure: the servers are unreachable");
    assert!(l[0].2.iter().all(|&b| b == 0));
    assert_eq!((l[1].0, l[1].3, l[1].4), (ticket, 0, tlo as u64 | (thi as u64) << 32));
    assert_eq!(u32_at(&l[1].2, 0), 3, "k_EResultNoConnection");
    assert_eq!(g.call(steam::steam_user, 21, &[SCRATCH + 0x300, 0x400, SCRATCH + 0x3F0]).0 & 0xFF, 0, "no ticket");
    // a result is delivered once, then forgotten
    g.run_callbacks();
    assert!(log().is_empty());
    // the other requests, with their result sizes
    for (slot, args, id) in [
        (28u32, vec![1u32, 0, 0, 1, 10], steam::LEADERBOARD_SCORES_DOWNLOADED),
        (29, vec![1, 0, SCRATCH + 0x300, 4], steam::LEADERBOARD_SCORES_DOWNLOADED),
        (31, vec![1, 0, 2, 100, SCRATCH + 0x300, 2], steam::LEADERBOARD_SCORE_UPLOADED),
    ] {
        let (lo, hi) = g.call(steam::steam_user_stats, slot, &args);
        let o = g.callback_object(SCRATCH + 0x1080, VTABLE, id, steam::result_size(id));
        g.flat(steam::register_call_result, &[o, lo, hi]);
        g.run_callbacks();
        let l = log();
        assert_eq!((l.len(), l[0].2.len() as u32, l[0].3), (1, steam::result_size(id), 1), "slot {slot}");
    }
    assert_eq!(g.call(steam::steam_user_stats, 29, &[1, 0, SCRATCH + 0x300, 101]), (0, 0), "more than 100 users: k_uAPICallInvalid");
    assert_eq!(g.call(steam::steam_user_stats, 30, &[1, 0, 0, SCRATCH + 0x300, SCRATCH + 0x380, 2]).0 & 0xFF, 0);
    put_str(&mut g.m, SCRATCH + 0x200, "0");
    for (slot, args, id) in [
        (4u32, vec![SCRATCH + 0x200], steam::REMOTE_STORAGE_FILE_SHARE_RESULT),
        (21, vec![5, 0, 0], steam::REMOTE_STORAGE_DOWNLOAD_UGC_RESULT),
    ] {
        let (lo, hi) = g.call(steam::steam_remote_storage, slot, &args);
        let o = g.callback_object(SCRATCH + 0x1080, VTABLE, id, steam::result_size(id));
        g.flat(steam::register_call_result, &[o, lo, hi]);
        g.run_callbacks();
        let l = log();
        assert_eq!((l.len(), l[0].2.len() as u32, l[0].3), (1, steam::result_size(id), 1), "remote storage slot {slot}");
    }
    assert_eq!(g.call(steam::steam_remote_storage, 24, &[5, 0, SCRATCH + 0x300, 0x100, 0, 0]).0, 0, "UGCRead: nothing downloaded");
    // an unregistered call result is not delivered
    let (lo, hi) = g.call(steam::steam_user_stats, 23, &[SCRATCH + 0x200]);
    let o = g.callback_object(SCRATCH + 0x1080, VTABLE, steam::LEADERBOARD_FIND_RESULT, 16);
    g.flat(steam::register_call_result, &[o, lo, hi]);
    g.flat(steam::unregister_call_result, &[o, lo, hi]);
    g.run_callbacks();
    assert!(log().is_empty());
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn run_callbacks_does_not_nest_and_defers_what_a_callback_requests() {
    let h = home("nest");
    let mut g = guest(h.clone());
    let cb = g.callback_object(SCRATCH + 0x1000, VTABLE_REENTER, 0, 16);
    g.flat(steam::register_callback, &[cb, steam::USER_STATS_STORED]);
    g.call(steam::steam_user_stats, 10, &[]);
    g.run_callbacks();
    assert_eq!(log().len(), 1, "the nested SteamAPI_RunCallbacks delivered nothing");
    // the FindLeaderboard made inside the callback completes next time
    let call = g.m.u32(cb + 0x24);
    assert_ne!(call, 0);
    let o = g.callback_object(SCRATCH + 0x1080, VTABLE, steam::LEADERBOARD_FIND_RESULT, 16);
    g.flat(steam::register_call_result, &[o, call, 0]);
    g.run_callbacks();
    let l = log();
    assert_eq!((l.len(), l[0].0, l[0].4), (1, o, call as u64));
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn remote_storage_keeps_the_games_files_in_the_sandbox() {
    let h = home("remote");
    let mut g = guest(h.clone());
    let (name, data, buf) = (SCRATCH + 0x200, SCRATCH + 0x400, SCRATCH + 0x1000);
    put_str(&mut g.m, name, "DDDA.sav");
    assert_eq!(g.call(steam::steam_remote_storage, 10, &[name]).0 & 0xFF, 0, "FileExists: nothing yet");
    assert_eq!(g.call(steam::steam_remote_storage, 12, &[name]).0, 0, "GetFileSize: 0 for a missing file");
    assert_eq!(g.call(steam::steam_remote_storage, 1, &[name, buf, 0x100]).0, 0, "FileRead: 0 for a missing file");
    for k in 0..0x200 {
        g.m.set_u8(data + k, (k * 7) as u8);
    }
    assert_eq!(g.call(steam::steam_remote_storage, 0, &[name, data, 0x200]).0 & 0xFF, 1, "FileWrite");
    let path = g.remote(steam::DEFAULT_ACCOUNT).join("DDDA.sav");
    let want: Vec<u8> = (0..0x200u32).map(|k| (k * 7) as u8).collect();
    assert_eq!(std::fs::read(&path).unwrap(), want, "userdata\\<account>\\367500\\remote\\DDDA.sav in the sandbox");
    assert_eq!(g.call(steam::steam_remote_storage, 10, &[name]).0 & 0xFF, 1);
    assert_eq!(g.call(steam::steam_remote_storage, 12, &[name]).0, 0x200);
    assert_eq!(g.call(steam::steam_remote_storage, 1, &[name, buf, 0x1000]).0, 0x200);
    assert_eq!((0..0x200).map(|k| g.m.u8(buf + k)).collect::<Vec<_>>(), want);
    // a short buffer reads the start; a rewrite replaces the whole file
    assert_eq!(g.call(steam::steam_remote_storage, 1, &[name, buf, 0x10]).0, 0x10);
    assert_eq!(g.call(steam::steam_remote_storage, 0, &[name, data, 3]).0 & 0xFF, 1);
    assert_eq!(std::fs::read(&path).unwrap(), &want[..3]);
    // refused: no buffer, negative size, names outside the folder or invalid on Windows
    assert_eq!(g.call(steam::steam_remote_storage, 0, &[name, 0, 3]).0 & 0xFF, 0);
    assert_eq!(g.call(steam::steam_remote_storage, 0, &[name, data, u32::MAX]).0 & 0xFF, 0);
    for bad in [r"..\escape.sav", r"C:\x.sav", "a:b", "", "dir/"] {
        put_str(&mut g.m, name, bad);
        assert_eq!(g.call(steam::steam_remote_storage, 0, &[name, data, 3]).0 & 0xFF, 0, "{bad:?}");
    }
    assert!(!h.join("steam/userdata/1/escape.sav").exists());
    assert_eq!(g.call(steam::steam_remote_storage, 18, &[]).0 & 0xFF, 1, "IsCloudEnabledForAccount");
    assert_eq!(g.call(steam::steam_remote_storage, 19, &[]).0 & 0xFF, 1, "IsCloudEnabledForApp");
    let _ = std::fs::remove_dir_all(&h);
}

/// A copy of a Steam folder the import reads: `<root>\userdata\<account>\367500\remote`.
fn player_folder(root: &Path, account: u32, files: &[(&str, &[u8])]) -> PathBuf {
    let remote = root.join("userdata").join(account.to_string()).join("367500").join("remote");
    std::fs::create_dir_all(&remote).unwrap();
    for (n, b) in files {
        std::fs::write(remote.join(n), b).unwrap();
    }
    remote
}

#[test]
fn an_imported_save_is_a_copy_the_game_continues_as_its_owner() {
    let h = home("import");
    let player = player_folder(&h.join("player"), 76_543, &[("DDDA.sav", b"the player's save"), ("0", b"pawn")]);
    let before = std::fs::read(player.join("DDDA.sav")).unwrap();
    let r = steam::import_save(&player, None, &h.join("sandbox")).unwrap();
    assert_eq!((r.account, r.files.clone(), r.kept.clone()), (76_543, vec!["0".to_string(), "DDDA.sav".to_string()], vec![]));
    assert_eq!(std::fs::read(player.join("DDDA.sav")).unwrap(), before, "the player's file is only read");
    let mut g = guest(h.join("sandbox"));
    // the game checks the save's mSteamID against GetSteamID (0x0040F880)
    let id = SCRATCH + 0x100;
    g.call(steam::steam_user, 2, &[id]);
    assert_eq!(g.m.u32(id), 76_543);
    put_str(&mut g.m, SCRATCH + 0x200, "DDDA.sav");
    let buf = SCRATCH + 0x1000;
    assert_eq!(g.call(steam::steam_remote_storage, 1, &[SCRATCH + 0x200, buf, 0x100]).0, 17);
    assert_eq!(get_str(&g.m, buf), "the player's save");
    // the rebuilt game's own saves go to the sandbox copy only
    put_str(&mut g.m, SCRATCH + 0x400, "new progress");
    g.call(steam::steam_remote_storage, 0, &[SCRATCH + 0x200, SCRATCH + 0x400, 12]);
    assert_eq!(std::fs::read(player.join("DDDA.sav")).unwrap(), before);
    // importing again keeps the sandbox's progress
    let r = steam::import_save(&player, None, &h.join("sandbox")).unwrap();
    assert_eq!((r.files.len(), r.kept.len()), (0, 2));
    assert_eq!(std::fs::read(g.remote(76_543).join("DDDA.sav")).unwrap(), b"new progress");
    // a folder outside Steam's userdata needs the account
    assert!(steam::import_save(&h.join("player"), None, &h.join("other")).is_err());
    let _ = std::fs::remove_dir_all(&h);
}

// --- the save format: the game's container writer, Steam Cloud, ddsave ------

fn installed_exe() -> Option<String> {
    let path = std::env::var("DDDA_EXE").unwrap_or_else(|_| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    Path::new(&path).is_file().then_some(path)
}

/// A private copy of the player's save (`DDDA_SAVE`, else the newest
/// backup under E:/ddda-re/save-backups), as the ddsave tests find it.
fn private_save() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("DDDA_SAVE") {
        return Some(PathBuf::from(p));
    }
    let mut v: Vec<PathBuf> = std::fs::read_dir("E:/ddda-re/save-backups").into_iter().flatten().filter_map(Result::ok)
        .flat_map(|d| [d.path().join("367500/remote/DDDA.sav"), d.path().join("remote/DDDA.sav")])
        .filter(|p| p.is_file()).collect();
    v.sort();
    v.pop()
}

/// A zlib stream (RFC 1950) of stored blocks (RFC 1951 BTYPE 00).
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut z = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = data.chunks(0xFFFF).collect();
    for (k, c) in chunks.iter().enumerate() {
        z.push((k + 1 == chunks.len()) as u8);
        z.extend((c.len() as u16).to_le_bytes());
        z.extend((!(c.len() as u16)).to_le_bytes());
        z.extend_from_slice(c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend(((b << 16) | a).to_be_bytes());
    z
}

/// The game's save write, from its compressed stream on: the container
/// writer 0x0040F1A0 (lifted; eax = buffer, stack: version, inflated
/// length, compressed length; stream at the 16-aligned buffer + 0x20)
/// fills the header, then the whole 0x80000-byte buffer goes to
/// ISteamRemoteStorage::FileWrite("DDDA.sav") as 0x004B7390 sends it.
/// Returns the file the sandbox now holds.
fn game_writes_save(s: &mut ddruntime::session::Session, sandbox: &Path, version: u32, inflated: u32, stream: &[u8]) -> Vec<u8> {
    use ddruntime::session::{STACK_SIZE, STACK_TOP};
    let buf = STACK_TOP - STACK_SIZE; // the far end of the session stack: 1 MiB, 16-aligned
    for k in 0..0x8_0000 {
        s.mem.set_u8(buf + k, 0);
    }
    for (k, &b) in stream.iter().enumerate() {
        s.mem.set_u8(buf + 0x20 + k as u32, b);
    }
    let mut cpu = Cpu { esp: STACK_TOP - 0x100, eax: buf, ..Cpu::default() };
    for a in [stream.len() as u32, inflated, version] {
        cpu.esp -= 4;
        s.mem.set_u32(cpu.esp, a);
    }
    cpu.esp -= 4;
    s.mem.set_u32(cpu.esp, ddruntime::session::RETURN_SENTINEL);
    s.registry.call(&mut s.mem, &mut cpu, 0x0040_F1A0).expect("0x0040F1A0 is lifted");
    assert_eq!(cpu.eax, buf + 0x20, "it returns the stream's address");
    // FileWrite through the ISteamRemoteStorage vtable, as the game calls it
    let name = STACK_TOP - 0x1000;
    put_str(&mut s.mem, name, "DDDA.sav");
    let rs = s.imports.iter().find(|i| i.full_name() == "steam_api.dll!SteamRemoteStorage").unwrap().token;
    let ddruntime::session::Outcome::Returned(c) = s.call(rs, 0, &[]) else { panic!("SteamRemoteStorage") };
    let obj = c.eax;
    let write = s.mem.u32(s.mem.u32(obj));
    let ddruntime::session::Outcome::Returned(c) = s.call(write, obj, &[name, buf, 0x8_0000]) else { panic!("FileWrite") };
    assert_eq!(c.eax & 0xFF, 1, "FileWrite");
    let account = steam::load_user(sandbox).account;
    std::fs::read(steam::app_dir(sandbox, account).join("remote/DDDA.sav")).unwrap()
}

#[test]
fn a_save_the_game_writes_through_steam_cloud_is_what_ddsave_reads() {
    let Some(exe) = installed_exe() else { eprintln!("skipped: no installed DDDA.exe"); return };
    let h = home("format");
    let mut s = ddruntime::session::Session::new(&exe).expect("session");
    steam::configure(&h);
    let xml = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<class name=\"mSaveData\" type=\"sSave::saveDataAllDA\">\n<u32 name=\"mCreateNo\" value=\"21\"/>\n</class>\n";
    let bytes = game_writes_save(&mut s, &h, 21, xml.len() as u32, &zlib_stored(xml.as_bytes()));
    assert_eq!(bytes.len(), 0x8_0000);
    let save = ddsave::SaveFile::parse(&bytes).expect("ddsave reads the written save");
    assert_eq!((save.header.version, save.header.reserved), (21, 0));
    assert_eq!(save.xml, xml);
    assert!(save.trailing_zero);
    assert_eq!(save.document().unwrap().root().get::<u32>("mCreateNo"), Some(21));

    // the player's real save: its own stream through the same path gives
    // back the same file, and an import makes its owner the local user
    let Some(private) = private_save() else { eprintln!("skipped the private part: no save copy"); return };
    let original = std::fs::read(&private).unwrap();
    let parsed = ddsave::SaveFile::parse(&original).unwrap();
    let hd = parsed.header;
    let stream = &original[32..32 + hd.compressed_len as usize];
    let written = game_writes_save(&mut s, &h, hd.version, hd.inflated_len, stream);
    assert_eq!(&written[..32 + stream.len()], &original[..32 + stream.len()], "the same header and stream");
    let again = ddsave::SaveFile::parse(&written).unwrap();
    assert_eq!(again.xml, parsed.xml);
    let doc = parsed.document().unwrap();
    let owner: u64 = doc.iter().find(|n| n.name().as_deref() == Some("mSteamID")).and_then(|n| n.parse()).expect("mSteamID");
    let sandbox = h.join("imported");
    let r = steam::import_save(&private, Some(owner as u32), &sandbox).unwrap();
    assert_eq!(r.files, vec!["DDDA.sav".to_string()]);
    steam::configure(&sandbox);
    let user = s.imports.iter().find(|i| i.full_name() == "steam_api.dll!SteamUser").unwrap().token;
    let ddruntime::session::Outcome::Returned(c) = s.call(user, 0, &[]) else { panic!("SteamUser") };
    let obj = c.eax;
    let out = ddruntime::session::STACK_TOP - 0x1100;
    let get_id = s.mem.u32(s.mem.u32(obj) + 8);
    let ddruntime::session::Outcome::Returned(_) = s.call(get_id, obj, &[out]) else { panic!("GetSteamID") };
    let id = s.mem.u32(out) as u64 | (s.mem.u32(out + 4) as u64) << 32;
    assert!(id == owner, "GetSteamID is the imported save's mSteamID (the load check of 0x0040F880)");
    let _ = std::fs::remove_dir_all(&h);
}
