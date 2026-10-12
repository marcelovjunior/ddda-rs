//! steam_api.dll services: an offline Steam for one local user
//! (brief L26; census in docs/tasks/L26-steam-saves.md#results).
//!
//! The game is built against Steamworks SDK 1.33 (the installed
//! steam_api.dll exports SteamUser018, SteamFriends015, SteamUtils007,
//! STEAMAPPS_INTERFACE_VERSION007, STEAMUSERSTATS_INTERFACE_VERSION011,
//! STEAMREMOTESTORAGE_INTERFACE_VERSION012, STEAMSCREENSHOTS_INTERFACE_VERSION002
//! and STEAMUGC_INTERFACE_VERSION005, a set only 1.33 has). Each `Steam*()`
//! accessor returns its own interface object (crate::interfaces) whose
//! vtable is the SDK's declaration order, with MSVC's rule that overloads
//! of one name take adjacent slots in reverse order (the game's own
//! CCallbackBase vtables show it: `Run(void*, bool, SteamAPICall_t)` in
//! slot 0, `Run(void*)` in slot 1). Exports are cdecl; interface methods
//! are thiscall (MSVC C++ virtuals: `this` in ecx, the callee pops); a
//! `CSteamID` result comes back through a hidden pointer argument, a
//! 64-bit result in edx:eax.
//!
//! The runtime behaves like a Steam client that runs without a connection
//! to the Steam servers: the user is known (SteamID, persona name), the
//! game's Steam Cloud files, stats and achievements are kept locally (in
//! the sandbox, never the player's Steam folder), and every request that
//! needs the servers completes, at a later `SteamAPI_RunCallbacks`, as a
//! network failure. Choices the SDK documentation does not settle are
//! labelled "runtime choice".
use crate::dispatch::{Conflict, Registry};
use crate::interfaces::{self, Spec, Tokens};
use crate::services::{arg, call_guest, ret, with_state};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;
use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

/// Dragon's Dogma: Dark Arisen's Steam app id.
pub const APP_ID: u32 = 367_500;

/// steamclientpublic.h: a SteamID is universe (bits 56..64), account type
/// (52..56), instance (32..52) and account id (0..32). An individual user
/// on the public universe and the desktop instance:
/// k_EUniversePublic = 1, k_EAccountTypeIndividual = 1,
/// k_unSteamUserDesktopInstance = 1.
pub const INDIVIDUAL: u64 = (1 << 56) | (1 << 52) | (1 << 32);

/// The account id when nothing is configured (runtime choice: any
/// account id is a valid SteamID; an imported save sets its owner's).
pub const DEFAULT_ACCOUNT: u32 = 1;
/// The persona name when nothing is configured (runtime choice).
pub const DEFAULT_NAME: &str = "Arisen";
/// ISteamUtils::GetIPCountry when nothing is configured (runtime choice:
/// the client geolocates its IP address; the runtime's locale is en-US).
pub const DEFAULT_COUNTRY: &str = "US";

// steam_api.h / isteamclient.h callback numbering (k_i*Callbacks + n)
pub const GAME_OVERLAY_ACTIVATED: u32 = 300 + 31;
pub const GAMEPAD_TEXT_INPUT_DISMISSED: u32 = 700 + 14;
pub const ENCRYPTED_APP_TICKET_RESPONSE: u32 = 100 + 54;
pub const USER_STATS_STORED: u32 = 1100 + 2;
pub const USER_ACHIEVEMENT_STORED: u32 = 1100 + 3;
pub const LEADERBOARD_FIND_RESULT: u32 = 1100 + 4;
pub const LEADERBOARD_SCORES_DOWNLOADED: u32 = 1100 + 5;
pub const LEADERBOARD_SCORE_UPLOADED: u32 = 1100 + 6;
pub const REMOTE_STORAGE_FILE_SHARE_RESULT: u32 = 1300 + 7;
pub const REMOTE_STORAGE_DOWNLOAD_UGC_RESULT: u32 = 1300 + 17;

/// Sizes of the result structures (MSVC x86, the SDK's default 8-byte
/// packing). The game's CCallResult objects report the same sizes from
/// their GetCallbackSizeBytes (vtables 0x01613118: 4, 0x01408EF0: 0x10,
/// 0x01408EFC: 0x18, 0x01408F08: 0x20, 0x01428C70: 0x118, 0x01428C64: 0x128).
pub fn result_size(callback: u32) -> u32 {
    match callback {
        ENCRYPTED_APP_TICKET_RESPONSE => 4,       // EResult
        USER_STATS_STORED => 16,                  // uint64 game, EResult
        USER_ACHIEVEMENT_STORED => 152,           // uint64, bool, char[128], uint32, uint32
        LEADERBOARD_FIND_RESULT => 16,            // uint64, uint8
        LEADERBOARD_SCORES_DOWNLOADED => 24,      // uint64, uint64, int
        LEADERBOARD_SCORE_UPLOADED => 32,         // uint8, uint64, int32, uint8, int, int
        REMOTE_STORAGE_FILE_SHARE_RESULT => 280,  // EResult, uint64, char[260]
        REMOTE_STORAGE_DOWNLOAD_UGC_RESULT => 296, // EResult, uint64, AppId, int32, char[260], uint64
        _ => panic!("steam: no result structure known for callback {callback}"),
    }
}

/// steamclientpublic.h EResult
const RESULT_OK: u32 = 1;
const RESULT_NO_CONNECTION: u32 = 3;

/// The local user and where their Steam data lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    pub account: u32,
    pub name: String,
    pub country: String,
    pub language: String,
}

impl User {
    pub fn steam_id(&self) -> u64 {
        INDIVIDUAL | self.account as u64
    }
}

/// A callback or call result waiting for `SteamAPI_RunCallbacks`.
#[derive(Clone, Debug)]
enum Posted {
    Callback { id: u32, data: Vec<u8> },
    Result { call: u64, id: u32, data: Vec<u8>, io_failure: bool },
}

#[derive(Default)]
pub struct Steam {
    user: Option<User>,
    /// The sandbox home the Steam data is kept under.
    home: PathBuf,
    /// Registered CCallbackBase objects: (callback id, object), in
    /// registration order.
    callbacks: Vec<(u32, u32)>,
    /// Registered call results: (call, object).
    results: Vec<(u64, u32)>,
    posted: VecDeque<Posted>,
    last_call: u64,
    running: bool,
    /// Achievements stored (persisted) and set since the last StoreStats.
    stored: BTreeSet<String>,
    unlocked: BTreeSet<String>,
}

fn with_steam<R>(f: impl FnOnce(&mut Steam) -> R) -> R {
    crate::runtime::with(|rt| rt.steam.with(f))
}

fn user() -> User {
    with_steam(|s| s.user.clone()).expect("steam: no user configured (steam::configure)")
}

// --- configuration, the sandbox layout and save import ---------------------

/// `<home>\steam\userdata\<account>\367500`: the app's folder in the
/// sandbox, laid out like the Steam client's own `userdata` (Steam Cloud
/// files in `remote\`).
pub fn app_dir(home: &Path, account: u32) -> PathBuf {
    home.join("steam").join("userdata").join(account.to_string()).join(APP_ID.to_string())
}

fn user_file(home: &Path) -> PathBuf {
    home.join("steam").join("user.txt")
}

/// The configured user: `<home>\steam\user.txt` (`key=value` lines:
/// account, name, country, language; written by [`import_save`]), then the
/// environment (`DDRUNTIME_STEAM_ACCOUNT`, `DDRUNTIME_STEAM_NAME`), then
/// the defaults.
pub fn load_user(home: &Path) -> User {
    let mut u = User {
        account: DEFAULT_ACCOUNT,
        name: DEFAULT_NAME.into(),
        country: DEFAULT_COUNTRY.into(),
        // the language the runtime runs the game in (its text and voice)
        language: "english".into(),
    };
    if let Ok(text) = std::fs::read_to_string(user_file(home)) {
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            match k.trim() {
                "account" => u.account = v.trim().parse().unwrap_or(u.account),
                "name" => u.name = v.trim().to_string(),
                "country" => u.country = v.trim().to_string(),
                "language" => u.language = v.trim().to_string(),
                _ => {}
            }
        }
    }
    if let Some(a) = std::env::var("DDRUNTIME_STEAM_ACCOUNT").ok().and_then(|v| v.parse().ok()) {
        u.account = a;
    }
    if let Ok(n) = std::env::var("DDRUNTIME_STEAM_NAME") {
        u.name = n;
    }
    u
}

fn save_user(home: &Path, u: &User) -> std::io::Result<()> {
    let p = user_file(home);
    std::fs::create_dir_all(p.parent().unwrap())?;
    std::fs::write(p, format!("account={}\nname={}\ncountry={}\nlanguage={}\n", u.account, u.name, u.country, u.language))
}

/// Use `home` (the sandbox root) for the Steam data, with the user it
/// configures; loads the stored achievements. Called by [`install`] with
/// the session's sandbox; tests call it with their own.
pub fn configure(home: &Path) {
    let u = load_user(home);
    let stored = read_achievements(&app_dir(home, u.account));
    with_steam(|s| {
        s.home = home.to_path_buf();
        s.user = Some(u);
        s.stored = stored;
        s.unlocked.clear();
    });
}

/// What [`import_save`] copied.
#[derive(Debug, PartialEq, Eq)]
pub struct Imported {
    pub account: u32,
    /// Files copied into the sandbox's `remote\` folder.
    pub files: Vec<String>,
    /// Files left alone because the sandbox already has them.
    pub kept: Vec<String>,
}

/// The account id in a path under Steam's `userdata\<account>\...`.
pub fn account_in_path(p: &Path) -> Option<u32> {
    let parts: Vec<_> = p.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
    let k = parts.iter().position(|c| c.eq_ignore_ascii_case("userdata"))?;
    parts.get(k + 1)?.parse().ok()
}

/// Import the player's existing Steam Cloud files (their `DDDA.sav`) into
/// the sandbox so the rebuilt game can continue the save. `source` is the
/// player's `...\userdata\<account>\367500\remote` folder (or a file in
/// it); it is only read. Its account becomes the sandbox user's: the game
/// refuses a save whose `mSteamID` is not GetSteamID (the load check after
/// 0x0040FA05 in 0x0040F880), and Steam names the folder after the low 32
/// bits of that SteamID. `account` overrides the folder's. Files the
/// sandbox already has are kept (the rebuilt game's progress is never
/// overwritten; delete them to import again).
pub fn import_save(source: &Path, account: Option<u32>, home: &Path) -> Result<Imported, String> {
    let dir = if source.is_dir() { source.to_path_buf() } else { source.parent().ok_or("no folder")?.to_path_buf() };
    let account = account.or_else(|| account_in_path(&dir))
        .ok_or_else(|| format!("{}: not under Steam's userdata\\<account>; give the account id", dir.display()))?;
    let files: Vec<PathBuf> = if source.is_dir() {
        let mut v: Vec<PathBuf> = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?
            .filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_file()).collect();
        v.sort();
        v
    } else {
        vec![source.to_path_buf()]
    };
    let remote = app_dir(home, account).join("remote");
    std::fs::create_dir_all(&remote).map_err(|e| e.to_string())?;
    let mut out = Imported { account, files: Vec::new(), kept: Vec::new() };
    for f in files {
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let to = remote.join(&name);
        if to.exists() {
            out.kept.push(name);
            continue;
        }
        let bytes = std::fs::read(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        std::fs::write(&to, bytes).map_err(|e| format!("{}: {e}", to.display()))?;
        out.files.push(name);
    }
    let mut u = load_user(home);
    u.account = account;
    save_user(home, &u).map_err(|e| e.to_string())?;
    Ok(out)
}

// --- the flat API ------------------------------------------------------------

/// SteamAPI_Init(): true (the offline client is always there).
pub fn init(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 1;
    ret(m, cpu, 0);
}
/// SteamAPI_Shutdown(): nothing to release.
pub fn shutdown(m: &mut dyn Mem, cpu: &mut Cpu) {
    ret(m, cpu, 0);
}

// CCallbackBase (steam_api.h): +0 vtable, +4 uint8 m_nCallbackFlags
// (k_ECallbackFlagsRegistered 1, k_ECallbackFlagsGameServer 2),
// +8 int m_iCallback. Vtable: 0 Run(void*, bool, SteamAPICall_t),
// 1 Run(void*), 2 GetCallbackSizeBytes().
const FLAG_REGISTERED: u8 = 1;
const FLAG_GAME_SERVER: u8 = 2;

/// SteamAPI_RegisterCallback(callback, id) (cdecl): as the installed
/// steam_api.dll does (0x3B404D70): sets the registered flag and
/// m_iCallback = id, and files the object under id.
pub fn register_callback(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (obj, id) = (arg(m, cpu, 0), arg(m, cpu, 1));
    m.set_u8(obj + 4, m.u8(obj + 4) | FLAG_REGISTERED);
    m.set_u32(obj + 8, id);
    with_steam(|s| s.callbacks.push((id, obj)));
    ret(m, cpu, 0);
}

/// SteamAPI_UnregisterCallback(callback) (cdecl): if registered, clears
/// the flag and forgets the object (0x3B4056A0).
pub fn unregister_callback(m: &mut dyn Mem, cpu: &mut Cpu) {
    let obj = arg(m, cpu, 0);
    if m.u8(obj + 4) & FLAG_REGISTERED != 0 {
        m.set_u8(obj + 4, m.u8(obj + 4) & !FLAG_REGISTERED);
        let id = m.u32(obj + 8);
        with_steam(|s| s.callbacks.retain(|&(i, o)| (i, o) != (id, obj)));
    }
    ret(m, cpu, 0);
}

fn call_arg(m: &dyn Mem, cpu: &Cpu, k: u32) -> u64 {
    arg(m, cpu, k) as u64 | (arg(m, cpu, k + 1) as u64) << 32
}

/// SteamAPI_RegisterCallResult(callback, call) (cdecl, call as two words).
pub fn register_call_result(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (obj, call) = (arg(m, cpu, 0), call_arg(m, cpu, 1));
    with_steam(|s| s.results.push((call, obj)));
    ret(m, cpu, 0);
}

/// SteamAPI_UnregisterCallResult(callback, call) (cdecl).
pub fn unregister_call_result(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (obj, call) = (arg(m, cpu, 0), call_arg(m, cpu, 1));
    with_steam(|s| s.results.retain(|&r| r != (call, obj)));
    ret(m, cpu, 0);
}

/// Call guest virtual `slot` of `obj` (thiscall: ecx = obj, the callee
/// pops `args`).
fn call_virtual(m: &mut dyn Mem, cpu: &mut Cpu, obj: u32, slot: u32, args: &[u32]) -> u32 {
    let target = m.u32(m.u32(obj) + 4 * slot);
    cpu.ecx = obj;
    call_guest(m, cpu, target, args)
}

fn guest_block(m: &mut dyn Mem, data: &[u8]) -> u32 {
    let p = with_state(|s| s.heap.alloc(data.len() as u32)).expect("guest heap");
    for (k, &b) in data.iter().enumerate() {
        m.set_u8(p + k as u32, b);
    }
    p
}

/// SteamAPI_RunCallbacks() (cdecl): delivers what was posted before this
/// call, in order. A callback goes to every object registered for its id
/// (not game-server objects), through Run(void*) (steam_api.dll
/// 0x3B404EF0); a call result goes to the objects registered for its call
/// whose m_iCallback and GetCallbackSizeBytes match the result, through
/// Run(void*, bIOFailure, call), and is then forgotten (0x3B405510..). The
/// parameter block lives on the guest heap for the call. A nested call
/// (from inside a callback) returns at once, as steam_api.dll's guard
/// (0x3B42C585) makes it; results posted while delivering wait for the
/// next call (a request to the client never completes inside the frame
/// that made it).
pub fn run_callbacks(m: &mut dyn Mem, cpu: &mut Cpu) {
    let batch = with_steam(|s| {
        if s.running {
            return None;
        }
        s.running = true;
        Some(std::mem::take(&mut s.posted))
    });
    let Some(batch) = batch else {
        ret(m, cpu, 0);
        return;
    };
    for p in batch {
        match p {
            Posted::Callback { id, data } => {
                let targets: Vec<u32> = with_steam(|s| s.callbacks.iter().filter(|c| c.0 == id).map(|c| c.1).collect());
                let block = guest_block(m, &data);
                for obj in targets {
                    // still registered (an earlier one may have unregistered it)
                    let live = with_steam(|s| s.callbacks.contains(&(id, obj)));
                    if live && m.u8(obj + 4) & FLAG_GAME_SERVER == 0 {
                        call_virtual(m, cpu, obj, 1, &[block]);
                    }
                }
                with_state(|s| s.heap.free(block)).expect("steam callback block");
            }
            Posted::Result { call, id, data, io_failure } => {
                let targets: Vec<u32> = with_steam(|s| s.results.iter().filter(|r| r.0 == call).map(|r| r.1).collect());
                let block = guest_block(m, &data);
                for obj in targets {
                    if !with_steam(|s| s.results.contains(&(call, obj))) {
                        continue;
                    }
                    let size = call_virtual(m, cpu, obj, 2, &[]);
                    assert!(m.u32(obj + 8) == id && size == data.len() as u32,
                        "steam: call result {call} is callback {id} ({} bytes), its object {obj:#x} expects {} ({size} bytes)",
                        data.len(), m.u32(obj + 8));
                    call_virtual(m, cpu, obj, 0, &[block, io_failure as u32, call as u32, (call >> 32) as u32]);
                    with_steam(|s| {
                        if let Some(k) = s.results.iter().position(|&r| r == (call, obj)) {
                            s.results.remove(k);
                        }
                    });
                }
                with_state(|s| s.heap.free(block)).expect("steam result block");
            }
        }
    }
    with_steam(|s| s.running = false);
    ret(m, cpu, 0);
}

/// A new SteamAPICall_t (nonzero; k_uAPICallInvalid is 0) whose result
/// `id` is delivered at the next RunCallbacks.
fn post_result(id: u32, data: Vec<u8>, io_failure: bool) -> u64 {
    assert_eq!(data.len() as u32, result_size(id));
    with_steam(|s| {
        s.last_call += 1;
        let call = s.last_call;
        s.posted.push_back(Posted::Result { call, id, data, io_failure });
        call
    })
}

/// A request the Steam servers would answer: without a connection its
/// result arrives with bIOFailure set (ESteamAPICallFailure
/// k_ESteamAPICallFailureNetworkFailure, "the network connection to the
/// Steam servers has been lost, or was already broken") and a zeroed
/// structure.
fn post_network_failure(id: u32) -> u64 {
    post_result(id, vec![0; result_size(id) as usize], true)
}

fn post_callback(id: u32, data: Vec<u8>) {
    with_steam(|s| s.posted.push_back(Posted::Callback { id, data }));
}

fn ret_u64(m: &dyn Mem, cpu: &mut Cpu, v: u64, n: u32) {
    cpu.eax = v as u32;
    cpu.edx = (v >> 32) as u32;
    ret(m, cpu, n);
}

fn write_u64(m: &mut dyn Mem, at: u32, v: u64) {
    m.set_u32(at, v as u32);
    m.set_u32(at + 4, (v >> 32) as u32);
}

/// A string returned to the guest: kept on the guest heap per key (the
/// SDK: "it's important that this pointer is not saved off").
fn string(m: &mut dyn Mem, key: &'static str, s: &str) -> u32 {
    let bytes: Vec<u8> = s.bytes().chain([0]).collect();
    crate::win32::persistent(m, key, &bytes)
}

fn guest_name(m: &dyn Mem, p: u32) -> Vec<u8> {
    (0..).map(|k| m.u8(p.wrapping_add(k))).take_while(|&b| b != 0).collect()
}

// --- ISteamUser (SteamUser018) ----------------------------------------------

/// BLoggedOn() (slot 1): false, there is no connection to the servers.
fn logged_on(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 0;
    ret(m, cpu, 0);
}

/// GetSteamID() (slot 2, CSteamID through the hidden result pointer).
fn steam_id(m: &mut dyn Mem, cpu: &mut Cpu) {
    let out = arg(m, cpu, 0);
    write_u64(m, out, user().steam_id());
    cpu.eax = out;
    ret(m, cpu, 4);
}

/// RequestEncryptedAppTicket(data, size) (slot 20): its
/// EncryptedAppTicketResponse_t carries k_EResultNoConnection ("a
/// connection to Steam could not be established", the documented result).
fn request_encrypted_app_ticket(m: &mut dyn Mem, cpu: &mut Cpu) {
    let call = post_result(ENCRYPTED_APP_TICKET_RESPONSE, RESULT_NO_CONNECTION.to_le_bytes().to_vec(), false);
    ret_u64(m, cpu, call, 8);
}

/// GetEncryptedAppTicket(ticket, max, size*) (slot 21): false, no ticket.
fn get_encrypted_app_ticket(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 0;
    ret(m, cpu, 12);
}

// --- ISteamFriends (SteamFriends015) ----------------------------------------

/// GetPersonaName() (slot 0).
fn persona_name(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = string(m, "steam persona", &user().name);
    ret(m, cpu, 0);
}

/// GetFriendCount(flags) (slot 3): -1, documented for a user who is not
/// logged on.
fn friend_count(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = u32::MAX;
    ret(m, cpu, 4);
}

/// GetFriendByIndex(index, flags) (slot 4, CSteamID through the hidden
/// pointer): k_steamIDNil, as for any invalid index.
fn friend_by_index(m: &mut dyn Mem, cpu: &mut Cpu) {
    let out = arg(m, cpu, 0);
    write_u64(m, out, 0);
    cpu.eax = out;
    ret(m, cpu, 12);
}

/// GetFriendRelationship(id) (slot 5): k_EFriendRelationshipNone (no
/// friends are known offline).
fn friend_relationship(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 0;
    ret(m, cpu, 8);
}

/// GetFriendPersonaName(id) (slot 7): the user's own name for their own
/// id; "" for anyone else (documented for unknown users; never NULL).
fn friend_persona_name(m: &mut dyn Mem, cpu: &mut Cpu) {
    let id = call_arg(m, cpu, 0);
    let u = user();
    cpu.eax = if id == u.steam_id() { string(m, "steam persona", &u.name) } else { string(m, "steam unknown persona", "") };
    ret(m, cpu, 8);
}

/// ActivateGameOverlayToUser(dialog, id) (slot 29): the runtime has no
/// overlay, so nothing opens and no GameOverlayActivated_t follows
/// (runtime choice).
fn overlay_to_user(m: &mut dyn Mem, cpu: &mut Cpu) {
    ret(m, cpu, 12);
}

/// RequestUserInformation(id, nameOnly) (slot 37): false, "we already
/// have all the details about that user" (runtime choice: offline nothing
/// more will arrive, and true would promise a PersonaStateChange_t).
fn request_user_information(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 0;
    ret(m, cpu, 12);
}

// --- ISteamUtils (SteamUtils007) --------------------------------------------

/// GetIPCountry() (slot 4).
fn ip_country(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = string(m, "steam country", &user().country);
    ret(m, cpu, 0);
}

/// GetAppID() (slot 9).
fn app_id(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = APP_ID;
    ret(m, cpu, 0);
}

/// SetWarningMessageHook(hook) (slot 16, called by WinMain at 0x00407A5F):
/// the runtime sends no warnings, so the hook is never called.
fn set_warning_message_hook(m: &mut dyn Mem, cpu: &mut Cpu) {
    ret(m, cpu, 4);
}

/// ShowGamepadTextInput(mode, lineMode, description, max, existing)
/// (slot 20): false, "the big picture overlay" is not running, so no
/// GamepadTextInputDismissed_t follows.
fn show_gamepad_text_input(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 0;
    ret(m, cpu, 20);
}

/// GetEnteredGamepadTextInput(text, size) (slot 22): false, no text.
fn entered_gamepad_text_input(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 0;
    ret(m, cpu, 8);
}

// --- ISteamApps (STEAMAPPS_INTERFACE_VERSION007) ------------------------------

/// GetCurrentGameLanguage() (slot 4): the game picks its text language
/// from this (0x00DF2F30 compares it with french, italian, german,
/// spanish, japanese, tchinese).
fn current_game_language(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = string(m, "steam language", &user().language);
    ret(m, cpu, 0);
}

/// GetInstalledDepots(appId, depots*, max) (slot 17) -> count. The
/// runtime knows of no installed depots: 0 (start-up then takes its
/// "depot 367505 not installed" path, 0x00DF3154).
fn installed_depots(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 0;
    ret(m, cpu, 12);
}

// --- ISteamRemoteStorage (STEAMREMOTESTORAGE_INTERFACE_VERSION012) -------------
// The game's Steam Cloud files: DDDA.sav (0x01564F1C; 0x80000 bytes,
// 0x004B7390 writes it, 0x004B72B0 and 0x004B7CA0 read it) and the online
// pawn files of MtNetObject (0x00DA3950). The client keeps them in
// userdata\<account>\367500\remote; so does the sandbox.

/// The host path of a remote file, or None for a name Steam refuses
/// (empty, k_cchFilenameMax 260 or longer, or not a valid relative
/// Windows path: "valid on all supported operating systems").
fn remote_path(name: &[u8]) -> Option<PathBuf> {
    let name = std::str::from_utf8(name).ok()?;
    if name.is_empty() || name.len() >= 260 {
        return None;
    }
    let mut p = app_dir(&with_steam(|s| s.home.clone()), user().account).join("remote");
    for part in name.split(['/', '\\']) {
        let bad = part.is_empty() || part == "." || part == ".."
            || part.chars().any(|c| (c as u32) < 32 || "<>:\"|?*".contains(c))
            || part.ends_with(' ') || part.ends_with('.');
        if bad {
            return None;
        }
        p.push(part);
    }
    Some(p)
}

/// FileWrite(name, data, size) (slot 0): replaces the file; false for a
/// NULL buffer, a negative size, more than k_unMaxCloudFileChunkSize
/// (100 MiB) or a refused name.
fn file_write(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (name, data, size) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2) as i32);
    let ok = (|| {
        if data == 0 || !(0..=100 * 1024 * 1024).contains(&size) {
            return false;
        }
        let Some(path) = remote_path(&guest_name(m, name)) else { return false };
        let bytes: Vec<u8> = (0..size as u32).map(|k| m.u8(data + k)).collect();
        std::fs::create_dir_all(path.parent().unwrap()).is_ok() && std::fs::write(&path, bytes).is_ok()
    })();
    cpu.eax = ok as u32;
    ret(m, cpu, 12);
}

/// FileRead(name, buffer, size) (slot 1) -> bytes read (at most `size`);
/// 0 if the file does not exist or cannot be read.
fn file_read(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (name, buf, size) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2) as i32);
    let bytes = remote_path(&guest_name(m, name)).and_then(|p| std::fs::read(p).ok()).unwrap_or_default();
    let n = bytes.len().min(size.max(0) as usize);
    for (k, &b) in bytes[..n].iter().enumerate() {
        m.set_u8(buf + k as u32, b);
    }
    cpu.eax = n as u32;
    ret(m, cpu, 12);
}

/// FileShare(name) (slot 4): sharing needs the servers.
fn file_share(m: &mut dyn Mem, cpu: &mut Cpu) {
    ret_u64(m, cpu, post_network_failure(REMOTE_STORAGE_FILE_SHARE_RESULT), 4);
}

/// FileExists(name) (slot 10).
fn file_exists(m: &mut dyn Mem, cpu: &mut Cpu) {
    let name = arg(m, cpu, 0);
    cpu.eax = remote_path(&guest_name(m, name)).is_some_and(|p| p.is_file()) as u32;
    ret(m, cpu, 4);
}

/// GetFileSize(name) (slot 12): 0 if the file does not exist.
fn file_size(m: &mut dyn Mem, cpu: &mut Cpu) {
    let name = arg(m, cpu, 0);
    let len = remote_path(&guest_name(m, name)).and_then(|p| std::fs::metadata(p).ok()).filter(|md| md.is_file()).map_or(0, |md| md.len());
    cpu.eax = len.min(i32::MAX as u64) as u32;
    ret(m, cpu, 4);
}

/// IsCloudEnabledForAccount() / IsCloudEnabledForApp() (slots 18, 19):
/// true, Steam's default for both settings (runtime choice: the player's
/// own settings are not read).
fn cloud_enabled(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 1;
    ret(m, cpu, 0);
}

/// UGCDownload(handle, priority) (slot 21): downloads need the servers.
fn ugc_download(m: &mut dyn Mem, cpu: &mut Cpu) {
    ret_u64(m, cpu, post_network_failure(REMOTE_STORAGE_DOWNLOAD_UGC_RESULT), 12);
}

/// UGCRead(handle, data, size, offset, action) (slot 24): 0, nothing was
/// downloaded.
fn ugc_read(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 0;
    ret(m, cpu, 24);
}

// --- ISteamUserStats (STEAMUSERSTATS_INTERFACE_VERSION011) ---------------------
// Stats are available from the start: the client requests them itself
// (the SDK documentation: RequestCurrentStats "is no longer necessary"),
// and the game never calls RequestCurrentStats. The game unlocks
// "ACH_ACHIEVEMENT_%02d" (0x00D96F50) and stores; achievements are kept in
// userdata\<account>\367500\achievements.txt (the runtime's own format,
// one API name per line). The achievement schema is Steam's, not the
// game's: names are not checked against it (unresolved).

fn achievements_file(dir: &Path) -> PathBuf {
    dir.join("achievements.txt")
}

fn read_achievements(dir: &Path) -> BTreeSet<String> {
    std::fs::read_to_string(achievements_file(dir)).map(|t| t.lines().filter(|l| !l.is_empty()).map(String::from).collect()).unwrap_or_default()
}

/// The achievements stored for the configured user.
pub fn achievements() -> BTreeSet<String> {
    with_steam(|s| s.stored.clone())
}

/// SetAchievement(name) (slot 7): true; the unlock is kept until
/// StoreStats.
fn set_achievement(m: &mut dyn Mem, cpu: &mut Cpu) {
    let name = String::from_utf8_lossy(&guest_name(m, arg(m, cpu, 0))).to_string();
    with_steam(|s| s.unlocked.insert(name));
    cpu.eax = 1;
    ret(m, cpu, 4);
}

/// StoreStats() (slot 10): stores locally and returns true, then posts
/// UserStatsStored_t (k_EResultOK) and a UserAchievementStored_t for each
/// achievement unlocked since the last store (progress 0/0: an unlock).
fn store_stats(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (home, account) = (with_steam(|s| s.home.clone()), user().account);
    let new: Vec<String> = with_steam(|s| {
        let new: Vec<String> = s.unlocked.difference(&s.stored).cloned().collect();
        s.stored.extend(new.iter().cloned());
        s.unlocked.clear();
        new
    });
    let dir = app_dir(&home, account);
    let text: String = achievements().iter().map(|a| format!("{a}\n")).collect();
    let ok = std::fs::create_dir_all(&dir).is_ok() && std::fs::write(achievements_file(&dir), text).is_ok();
    let mut stored = (APP_ID as u64).to_le_bytes().to_vec();
    stored.extend((if ok { RESULT_OK } else { 2 /* k_EResultFail */ }).to_le_bytes());
    stored.resize(16, 0);
    post_callback(USER_STATS_STORED, stored);
    for a in new {
        let mut d = vec![0u8; 152];
        d[..8].copy_from_slice(&(APP_ID as u64).to_le_bytes());
        // +8 m_bGroupAchievement false; +9 m_rgchAchievementName[128];
        // +140 m_nCurProgress, +144 m_nMaxProgress: 0 for an unlock
        let n = a.len().min(127);
        d[9..9 + n].copy_from_slice(&a.as_bytes()[..n]);
        post_callback(USER_ACHIEVEMENT_STORED, d);
    }
    cpu.eax = ok as u32;
    ret(m, cpu, 0);
}

/// FindLeaderboard(name) (slot 23).
fn find_leaderboard(m: &mut dyn Mem, cpu: &mut Cpu) {
    ret_u64(m, cpu, post_network_failure(LEADERBOARD_FIND_RESULT), 4);
}

/// DownloadLeaderboardEntries(board, request, start, end) (slot 28).
fn download_leaderboard_entries(m: &mut dyn Mem, cpu: &mut Cpu) {
    ret_u64(m, cpu, post_network_failure(LEADERBOARD_SCORES_DOWNLOADED), 20);
}

/// DownloadLeaderboardEntriesForUsers(board, users*, count) (slot 29):
/// k_uAPICallInvalid for more than 100 users (documented), else a request.
fn download_leaderboard_entries_for_users(m: &mut dyn Mem, cpu: &mut Cpu) {
    let count = arg(m, cpu, 3) as i32;
    let call = if count > 100 { 0 } else { post_network_failure(LEADERBOARD_SCORES_DOWNLOADED) };
    ret_u64(m, cpu, call, 16);
}

/// GetDownloadedLeaderboardEntry(entries, index, entry*, details*, max)
/// (slot 30): false, no entries were downloaded.
fn downloaded_leaderboard_entry(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 0;
    ret(m, cpu, 24);
}

/// UploadLeaderboardScore(board, method, score, details*, count) (slot 31).
fn upload_leaderboard_score(m: &mut dyn Mem, cpu: &mut Cpu) {
    ret_u64(m, cpu, post_network_failure(LEADERBOARD_SCORE_UPLOADED), 24);
}

// --- ISteamScreenshots (STEAMSCREENSHOTS_INTERFACE_VERSION002) -----------------

/// TriggerScreenshot() (slot 2): the runtime has no overlay to take one
/// (runtime choice).
fn trigger_screenshot(m: &mut dyn Mem, cpu: &mut Cpu) {
    ret(m, cpu, 0);
}

// --- interfaces ----------------------------------------------------------------

/// Method names in vtable order (SDK 1.33 headers, `_PS3` sections
/// excluded, overloads reversed), for diagnostics.
const USER_NAMES: &[&str] = &[
    "GetHSteamUser", "BLoggedOn", "GetSteamID", "InitiateGameConnection", "TerminateGameConnection",
    "TrackAppUsageEvent", "GetUserDataFolder", "StartVoiceRecording", "StopVoiceRecording",
    "GetAvailableVoice", "GetVoice", "DecompressVoice", "GetVoiceOptimalSampleRate",
    "GetAuthSessionTicket", "BeginAuthSession", "EndAuthSession", "CancelAuthTicket",
    "UserHasLicenseForApp", "BIsBehindNAT", "AdvertiseGame", "RequestEncryptedAppTicket",
    "GetEncryptedAppTicket", "GetGameBadgeLevel", "GetPlayerSteamLevel", "RequestStoreAuthURL",
];
const FRIENDS_NAMES: &[&str] = &[
    "GetPersonaName", "SetPersonaName", "GetPersonaState", "GetFriendCount", "GetFriendByIndex",
    "GetFriendRelationship", "GetFriendPersonaState", "GetFriendPersonaName", "GetFriendGamePlayed",
    "GetFriendPersonaNameHistory", "GetFriendSteamLevel", "GetPlayerNickname",
    "GetFriendsGroupCount", "GetFriendsGroupIDByIndex", "GetFriendsGroupName",
    "GetFriendsGroupMembersCount", "GetFriendsGroupMembersList", "HasFriend", "GetClanCount",
    "GetClanByIndex", "GetClanName", "GetClanTag", "GetClanActivityCounts",
    "DownloadClanActivityCounts", "GetFriendCountFromSource", "GetFriendFromSourceByIndex",
    "IsUserInSource", "SetInGameVoiceSpeaking", "ActivateGameOverlay", "ActivateGameOverlayToUser",
    "ActivateGameOverlayToWebPage", "ActivateGameOverlayToStore", "SetPlayedWith",
    "ActivateGameOverlayInviteDialog", "GetSmallFriendAvatar", "GetMediumFriendAvatar",
    "GetLargeFriendAvatar", "RequestUserInformation", "RequestClanOfficerList", "GetClanOwner",
    "GetClanOfficerCount", "GetClanOfficerByIndex", "GetUserRestrictions", "SetRichPresence",
    "ClearRichPresence", "GetFriendRichPresence", "GetFriendRichPresenceKeyCount",
    "GetFriendRichPresenceKeyByIndex", "RequestFriendRichPresence", "InviteUserToGame",
    "GetCoplayFriendCount", "GetCoplayFriend", "GetFriendCoplayTime", "GetFriendCoplayGame",
    "JoinClanChatRoom", "LeaveClanChatRoom", "GetClanChatMemberCount", "GetChatMemberByIndex",
    "SendClanChatMessage", "GetClanChatMessage", "IsClanChatAdmin", "IsClanChatWindowOpenInSteam",
    "OpenClanChatWindowInSteam", "CloseClanChatWindowInSteam", "SetListenForFriendsMessages",
    "ReplyToFriendMessage", "GetFriendMessage", "GetFollowerCount", "IsFollowing",
    "EnumerateFollowingList",
];
const UTILS_NAMES: &[&str] = &[
    "GetSecondsSinceAppActive", "GetSecondsSinceComputerActive", "GetConnectedUniverse",
    "GetServerRealTime", "GetIPCountry", "GetImageSize", "GetImageRGBA", "GetCSERIPPort",
    "GetCurrentBatteryPower", "GetAppID", "SetOverlayNotificationPosition", "IsAPICallCompleted",
    "GetAPICallFailureReason", "GetAPICallResult", "RunFrame", "GetIPCCallCount",
    "SetWarningMessageHook", "IsOverlayEnabled", "BOverlayNeedsPresent", "CheckFileSignature",
    "ShowGamepadTextInput", "GetEnteredGamepadTextLength", "GetEnteredGamepadTextInput",
    "GetSteamUILanguage", "IsSteamRunningInVR",
];
const APPS_NAMES: &[&str] = &[
    "BIsSubscribed", "BIsLowViolence", "BIsCybercafe", "BIsVACBanned", "GetCurrentGameLanguage",
    "GetAvailableGameLanguages", "BIsSubscribedApp", "BIsDlcInstalled",
    "GetEarliestPurchaseUnixTime", "BIsSubscribedFromFreeWeekend", "GetDLCCount",
    "BGetDLCDataByIndex", "InstallDLC", "UninstallDLC", "RequestAppProofOfPurchaseKey",
    "GetCurrentBetaName", "MarkContentCorrupt", "GetInstalledDepots", "GetAppInstallDir",
    "BIsAppInstalled", "GetAppOwner", "GetLaunchQueryParam", "GetDlcDownloadProgress",
    "GetAppBuildId",
];
const REMOTE_STORAGE_NAMES: &[&str] = &[
    "FileWrite", "FileRead", "FileForget", "FileDelete", "FileShare", "SetSyncPlatforms",
    "FileWriteStreamOpen", "FileWriteStreamWriteChunk", "FileWriteStreamClose",
    "FileWriteStreamCancel", "FileExists", "FilePersisted", "GetFileSize", "GetFileTimestamp",
    "GetSyncPlatforms", "GetFileCount", "GetFileNameAndSize", "GetQuota",
    "IsCloudEnabledForAccount", "IsCloudEnabledForApp", "SetCloudEnabledForApp", "UGCDownload",
    "GetUGCDownloadProgress", "GetUGCDetails", "UGCRead", "GetCachedUGCCount", "GetCachedUGCHandle",
    "GetFileListFromServer", "FileFetch", "FilePersist", "SynchronizeToClient",
    "SynchronizeToServer", "ResetFileRequestState", "PublishWorkshopFile",
    "CreatePublishedFileUpdateRequest", "UpdatePublishedFileFile", "UpdatePublishedFilePreviewFile",
    "UpdatePublishedFileTitle", "UpdatePublishedFileDescription", "UpdatePublishedFileVisibility",
    "UpdatePublishedFileTags", "CommitPublishedFileUpdate", "GetPublishedFileDetails",
    "DeletePublishedFile", "EnumerateUserPublishedFiles", "SubscribePublishedFile",
    "EnumerateUserSubscribedFiles", "UnsubscribePublishedFile",
    "UpdatePublishedFileSetChangeDescription", "GetPublishedItemVoteDetails",
    "UpdateUserPublishedItemVote", "GetUserPublishedItemVoteDetails",
    "EnumerateUserSharedWorkshopFiles", "PublishVideo", "SetUserPublishedFileAction",
    "EnumeratePublishedFilesByUserAction", "EnumeratePublishedWorkshopFiles",
    "UGCDownloadToLocation",
];
const USER_STATS_NAMES: &[&str] = &[
    "RequestCurrentStats", "GetStat", "GetStat", "SetStat", "SetStat", "UpdateAvgRateStat",
    "GetAchievement", "SetAchievement", "ClearAchievement", "GetAchievementAndUnlockTime",
    "StoreStats", "GetAchievementIcon", "GetAchievementDisplayAttribute",
    "IndicateAchievementProgress", "GetNumAchievements", "GetAchievementName", "RequestUserStats",
    "GetUserStat", "GetUserStat", "GetUserAchievement", "GetUserAchievementAndUnlockTime",
    "ResetAllStats", "FindOrCreateLeaderboard", "FindLeaderboard", "GetLeaderboardName",
    "GetLeaderboardEntryCount", "GetLeaderboardSortMethod", "GetLeaderboardDisplayType",
    "DownloadLeaderboardEntries", "DownloadLeaderboardEntriesForUsers",
    "GetDownloadedLeaderboardEntry", "UploadLeaderboardScore", "AttachLeaderboardUGC",
    "GetNumberOfCurrentPlayers", "RequestGlobalAchievementPercentages",
    "GetMostAchievedAchievementInfo", "GetNextMostAchievedAchievementInfo",
    "GetAchievementAchievedPercent", "RequestGlobalStats", "GetGlobalStat", "GetGlobalStat",
    "GetGlobalStatHistory", "GetGlobalStatHistory",
];
const SCREENSHOTS_NAMES: &[&str] = &[
    "WriteScreenshot", "AddScreenshotToLibrary", "TriggerScreenshot", "HookScreenshots",
    "SetLocation", "TagUser", "TagPublishedFile",
];

/// Every method the game calls (the census), bound; the other slots stop
/// with their name.
pub const INTERFACES: &[Spec] = &[
    Spec { name: "ISteamUtils", slots: UTILS_NAMES.len() as u32, names: UTILS_NAMES, methods: &[
        (4, ip_country), (9, app_id), (16, set_warning_message_hook), (20, show_gamepad_text_input),
        (22, entered_gamepad_text_input)] },
    Spec { name: "ISteamUser", slots: USER_NAMES.len() as u32, names: USER_NAMES, methods: &[
        (1, logged_on), (2, steam_id), (20, request_encrypted_app_ticket), (21, get_encrypted_app_ticket)] },
    Spec { name: "ISteamUserStats", slots: USER_STATS_NAMES.len() as u32, names: USER_STATS_NAMES, methods: &[
        (7, set_achievement), (10, store_stats), (23, find_leaderboard), (28, download_leaderboard_entries),
        (29, download_leaderboard_entries_for_users), (30, downloaded_leaderboard_entry),
        (31, upload_leaderboard_score)] },
    Spec { name: "ISteamFriends", slots: FRIENDS_NAMES.len() as u32, names: FRIENDS_NAMES, methods: &[
        (0, persona_name), (3, friend_count), (4, friend_by_index), (5, friend_relationship),
        (7, friend_persona_name), (29, overlay_to_user), (37, request_user_information)] },
    Spec { name: "ISteamApps", slots: APPS_NAMES.len() as u32, names: APPS_NAMES, methods: &[
        (4, current_game_language), (17, installed_depots)] },
    Spec { name: "ISteamRemoteStorage", slots: REMOTE_STORAGE_NAMES.len() as u32, names: REMOTE_STORAGE_NAMES, methods: &[
        (0, file_write), (1, file_read), (4, file_share), (10, file_exists), (12, file_size),
        (18, cloud_enabled), (19, cloud_enabled), (21, ugc_download), (24, ugc_read)] },
    Spec { name: "ISteamScreenshots", slots: SCREENSHOTS_NAMES.len() as u32, names: SCREENSHOTS_NAMES, methods: &[
        (2, trigger_screenshot)] },
];

fn accessor(m: &mut dyn Mem, cpu: &mut Cpu, name: &str) {
    cpu.eax = with_state(|s| s.objects.get(name).copied()).unwrap_or_else(|| panic!("{name} not created"));
    ret(m, cpu, 0);
}
pub fn steam_utils(m: &mut dyn Mem, cpu: &mut Cpu) { accessor(m, cpu, "ISteamUtils") }
pub fn steam_user(m: &mut dyn Mem, cpu: &mut Cpu) { accessor(m, cpu, "ISteamUser") }
pub fn steam_user_stats(m: &mut dyn Mem, cpu: &mut Cpu) { accessor(m, cpu, "ISteamUserStats") }
pub fn steam_friends(m: &mut dyn Mem, cpu: &mut Cpu) { accessor(m, cpu, "ISteamFriends") }
pub fn steam_apps(m: &mut dyn Mem, cpu: &mut Cpu) { accessor(m, cpu, "ISteamApps") }
pub fn steam_remote_storage(m: &mut dyn Mem, cpu: &mut Cpu) { accessor(m, cpu, "ISteamRemoteStorage") }
pub fn steam_screenshots(m: &mut dyn Mem, cpu: &mut Cpu) { accessor(m, cpu, "ISteamScreenshots") }

/// All 14 steam_api.dll imports of the exe.
pub const APIS: &[(&str, &str, fn(&mut dyn Mem, &mut Cpu))] = &[
    ("steam_api.dll", "SteamAPI_Init", init),
    ("steam_api.dll", "SteamAPI_Shutdown", shutdown),
    ("steam_api.dll", "SteamAPI_RunCallbacks", run_callbacks),
    ("steam_api.dll", "SteamAPI_RegisterCallback", register_callback),
    ("steam_api.dll", "SteamAPI_UnregisterCallback", unregister_callback),
    ("steam_api.dll", "SteamAPI_RegisterCallResult", register_call_result),
    ("steam_api.dll", "SteamAPI_UnregisterCallResult", unregister_call_result),
    ("steam_api.dll", "SteamUtils", steam_utils),
    ("steam_api.dll", "SteamUser", steam_user),
    ("steam_api.dll", "SteamUserStats", steam_user_stats),
    ("steam_api.dll", "SteamFriends", steam_friends),
    ("steam_api.dll", "SteamApps", steam_apps),
    ("steam_api.dll", "SteamRemoteStorage", steam_remote_storage),
    ("steam_api.dll", "SteamScreenshots", steam_screenshots),
];

/// Create every interface object (before the registry is frozen) and
/// configure the user from the sandbox. With `DDRUNTIME_IMPORT_SAVE` set
/// to the player's Steam `remote` folder (or their DDDA.sav), that save
/// is first imported into the sandbox ([`import_save`]).
pub fn install(m: &mut dyn Mem, registry: &mut Registry, tokens: &mut Tokens) -> Result<(), Conflict> {
    for spec in INTERFACES {
        let o = interfaces::create(m, registry, tokens, spec)?;
        with_state(|s| s.objects.insert(spec.name, o));
    }
    let home = crate::services::sandbox_home();
    if let Some(src) = std::env::var_os("DDRUNTIME_IMPORT_SAVE") {
        let account = std::env::var("DDRUNTIME_STEAM_ACCOUNT").ok().and_then(|v| v.parse().ok());
        match import_save(Path::new(&src), account, &home) {
            Ok(r) => eprintln!("steam: imported {:?} for account {} (kept {:?})", r.files, r.account, r.kept),
            Err(e) => eprintln!("steam: save import failed: {e}"),
        }
    }
    configure(&home);
    Ok(())
}
