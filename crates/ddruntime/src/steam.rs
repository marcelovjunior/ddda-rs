//! steam_api.dll services. The runtime is not a Steam client: it behaves
//! like a Steam API that initialised and has nothing to report (no
//! callbacks fire, nothing is pending). Each `Steam*()` accessor returns its
//! own interface object (crate::interfaces); methods are implemented as the
//! game reaches them, the rest stop with their name. Exports are cdecl;
//! interface methods are thiscall (MSVC C++ virtuals).
use crate::dispatch::{Conflict, Registry};
use crate::interfaces::{self, Spec, Tokens};
use crate::services::{arg, ret, with_state};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;

/// SteamAPI_Init(): true.
pub fn init(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 1;
    ret(m, cpu, 0);
}
/// SteamAPI_Shutdown(), SteamAPI_RunCallbacks(): nothing to do.
pub fn nothing(m: &mut dyn Mem, cpu: &mut Cpu) {
    ret(m, cpu, 0);
}
/// SteamAPI_RegisterCallback(callback, id) and the Unregister/CallResult
/// variants: accepted; no callback is ever delivered.
pub fn register(m: &mut dyn Mem, cpu: &mut Cpu) {
    ret(m, cpu, 0);
}

/// thiscall method with one argument and no meaningful result
/// (e.g. ISteamUtils::SetWarningMessageHook, slot 16).
fn method_1_void(m: &mut dyn Mem, cpu: &mut Cpu) {
    let _ = arg(m, cpu, 0);
    ret(m, cpu, 4);
}

/// ISteamApps::GetCurrentGameLanguage (slot 4, thiscall, no arguments):
/// "english". The game picks its text language from this.
fn current_game_language(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = crate::win32::persistent(m, "steam language", b"english\0");
    ret(m, cpu, 0);
}

/// ISteamUtils::GetAppID (slot 9, thiscall, no arguments): Dragon's
/// Dogma: Dark Arisen's Steam app id. (ISteamUtils' slot order is pinned
/// by WinMain calling SetWarningMessageHook at slot 16, offset 0x40.)
pub const APP_ID: u32 = 367_500;
fn app_id(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = APP_ID;
    ret(m, cpu, 0);
}

/// ISteamApps::GetInstalledDepots (slot 17, thiscall: appId, depots*,
/// max) -> count. The runtime knows of no installed depots: 0 (start-up
/// then takes its "depot 367505 not installed" path, 0x00DF3154).
fn installed_depots(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = 0;
    ret(m, cpu, 12);
}

const SLOTS: u32 = 64;
pub const INTERFACES: &[Spec] = &[
    Spec { name: "ISteamUtils", slots: SLOTS, methods: &[(9, app_id), (16, method_1_void)], names: &[] },
    Spec { name: "ISteamUser", slots: SLOTS, methods: &[], names: &[] },
    Spec { name: "ISteamUserStats", slots: SLOTS, methods: &[], names: &[] },
    Spec { name: "ISteamFriends", slots: SLOTS, methods: &[], names: &[] },
    Spec { name: "ISteamApps", slots: SLOTS, methods: &[(4, current_game_language), (17, installed_depots)], names: &[] },
    Spec { name: "ISteamRemoteStorage", slots: SLOTS, methods: &[], names: &[] },
    Spec { name: "ISteamScreenshots", slots: SLOTS, methods: &[], names: &[] },
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

pub const APIS: &[(&str, &str, fn(&mut dyn Mem, &mut Cpu))] = &[
    ("steam_api.dll", "SteamAPI_Init", init),
    ("steam_api.dll", "SteamAPI_Shutdown", nothing),
    ("steam_api.dll", "SteamAPI_RunCallbacks", nothing),
    ("steam_api.dll", "SteamAPI_RegisterCallback", register),
    ("steam_api.dll", "SteamAPI_UnregisterCallback", register),
    ("steam_api.dll", "SteamAPI_RegisterCallResult", register),
    ("steam_api.dll", "SteamAPI_UnregisterCallResult", register),
    ("steam_api.dll", "SteamUtils", steam_utils),
    ("steam_api.dll", "SteamUser", steam_user),
    ("steam_api.dll", "SteamUserStats", steam_user_stats),
    ("steam_api.dll", "SteamFriends", steam_friends),
    ("steam_api.dll", "SteamApps", steam_apps),
    ("steam_api.dll", "SteamRemoteStorage", steam_remote_storage),
    ("steam_api.dll", "SteamScreenshots", steam_screenshots),
];

/// Create every interface object (before the registry is frozen).
pub fn install(m: &mut dyn Mem, registry: &mut Registry, tokens: &mut Tokens) -> Result<(), Conflict> {
    for spec in INTERFACES {
        let o = interfaces::create(m, registry, tokens, spec)?;
        with_state(|s| s.objects.insert(spec.name, o));
    }
    Ok(())
}
