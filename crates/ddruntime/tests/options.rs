//! Brief L34: the game's options and config.ini, through the game's own
//! (lifted) code on the runtime's profile services (docs/OPTIONS.md).
//!
//! One session: the CRT start-up up to the static initializers (MtRegistry
//! 0x018CFF48 is built by one of them, with the sandbox's
//! `<Local AppData>\CAPCOM\DRAGONS DOGMA DARK ARISEN\config.ini`). Then:
//! - the PC options menu's writer 0x00EDDB20, on a menu object holding the
//!   choices a real config.ini written by the original game records;
//! - the option items' writer 0x00DF2CA0 (what start-up runs after reading
//!   them, 0x00DF3AC0), on an sApp holding items made like the game's
//!   registrars make them (names, types, ranges and choice lists are the
//!   exe's own, from research/runtime/options_census.py), with direct
//!   fields instead of the original objects' accessors;
//! - the window's position writes (0x00DF1550, WM_DESTROY).
//! The file must come out as the original's does (the expected text is a
//! config.ini the original game wrote on the maintainer's machine, without
//! its [KEYBOARDMOUSE] section, which the input object writes). The items'
//! reader 0x00DF39E0 and the typed readers then read it back, and an
//! imported copy of a player's config.ini is honoured.
//!
//! Needs the lifted program (ddruntime's `game` feature, the default since L36).
#![cfg(feature = "game")]
use ddnative_base::mem::Mem;
use ddruntime::memory::{READ, WRITE};
use ddruntime::session::{Outcome, Session};
use std::path::Path;

const REGISTRY: u32 = 0x018C_FF48;
/// DAT_018D08F4: the sRender instance the menu reads its resolution list from.
const RENDER_GLOBAL: u32 = 0x018D_08F4;
/// Guest scratch the test lays its objects out in.
const SCRATCH: u32 = 0x6000_0000;
const SCRATCH_SIZE: u32 = 0x0080_0000;

/// The option items' section masks (0x0142C068: GAME 1, GRAPHICS 2,
/// SOUNDS 4, DISPLAY 8, ETC 0x10, DEVELOP 0x20, CPU 0x40).
const GRAPHICS: u16 = 2;
const SOUNDS: u16 = 4;
const DISPLAY: u16 = 8;
const CPU: u16 = 0x40;

/// What the original wrote (GRAPHICS and DISPLAY from the menu first, the
/// items it does not write appended, SOUNDS and CPU from the items, Window
/// at exit).
const EXPECTED: &str = "[GRAPHICS]\r\nAltAntiAlias=NONE\r\nHDR=DEFAULT\r\nTextureDetail=MEDIUM\r\nShadowQuality=LOW\r\n\
MaxFPS=150.000000\r\nGrassQuality=LOW\r\nCameraFov=15.000000\r\nTextureFiltering=ANISO_X4\r\nEffectVolume=LOW\r\n\
Brightness=10.000000\r\nViewRange=NORMAL\r\nDofFilter=ON\r\nAntiAlias=NONE\r\nSLI=OFF\r\nStereo=OFF\r\nTextureMipLimite=2\r\n\
[DISPLAY]\r\nResolution=1920x1080\r\nRefreshRate=144.00Hz\r\nVSYNC=OFF\r\nFullScreen=OFF\r\nFlush=OFF\r\n\
[SOUNDS]\r\nSeVoiceNum=128\r\n[CPU]\r\nJobThread=7\r\nRenderingThread=ON\r\n[Window]\r\nMainX=0\r\nMainY=0\r\n";

fn installed_exe() -> Option<String> {
    let path = std::env::var("DDDA_EXE").unwrap_or_else(|_| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    Path::new(&path).is_file().then_some(path)
}

fn returned(o: Outcome, what: &str) -> ddnative_base::x86::Cpu {
    match o {
        Outcome::Returned(c) => c,
        o => panic!("{what}: {o}"),
    }
}

/// The CRT start-up as Session::start runs it, up to and including the
/// static initializers (they build MtRegistry and the default allocator
/// MtString uses, PTR_DAT_016296C0 -> 0x01876840).
fn start_up(s: &mut Session) {
    for (name, addr, args) in [("__heap_init", 0x012F_E354u32, &[][..]), ("__mtinit", 0x012F_982C, &[]), ("__ioinit", 0x012F_7C13, &[])] {
        returned(s.call(addr, 0, args), name);
    }
    let token = |s: &Session, n: &str| s.imports.iter().find(|i| i.full_name() == n).unwrap().token;
    let t = token(s, "KERNEL32.dll!GetCommandLineA");
    let line = returned(s.call(t, 0, &[]), "GetCommandLineA").eax;
    s.mem.set_u32(0x019A_B1C4, line);
    let env = returned(s.call(0x012F_E2BD, 0, &[]), "__crtGetEnvironmentStringsA").eax;
    s.mem.set_u32(0x018D_7A18, env);
    for (name, addr, args) in [("__setargv", 0x012F_E202u32, &[][..]), ("_setenvp", 0x012F_DF8C, &[]), ("__cinit", 0x012E_DA5E, &[1])] {
        returned(s.call(addr, 0, args), name);
    }
}

/// A bump allocator over the scratch region.
struct Scratch(u32);

impl Scratch {
    fn take(&mut self, n: u32) -> u32 {
        let at = self.0;
        self.0 = (self.0 + n + 15) & !15;
        assert!(self.0 < SCRATCH + SCRATCH_SIZE);
        at
    }
    fn str(&mut self, s: &mut Session, text: &str) -> u32 {
        let at = self.take(text.len() as u32 + 1);
        for (k, b) in text.bytes().chain([0]).enumerate() {
            s.mem.set_u8(at + k as u32, b);
        }
        at
    }
}

fn cstr(m: &dyn Mem, at: u32) -> String {
    (0..).map(|k| m.u8(at + k)).take_while(|&b| b != 0).map(|b| b as char).collect()
}

/// An option item as the registrars make them (0x00DF1090, 0x44 bytes):
/// +4 section mask, +6 type (1 choice, 3 f32, 4 s32, 5 bool), +8 the
/// property (name, type word, object, value; here a direct field: type
/// word without bit 23, +0x14 the field's address, +0x24 index 0),
/// +0x38/+0x3c the range, +0x40 the choice list.
struct Item {
    mask: u16,
    kind: u8,
    name: u32,
    /// MtDTI property type: 3 u8/bool, 6 u32, 0xC f32
    prop_type: u32,
    range: (u32, u32),
    choices: u32,
    value: u32,
}

fn item(s: &mut Session, a: &mut Scratch, i: &Item) -> (u32, u32) {
    let at = a.take(0x44);
    let field = a.take(4);
    for k in 0..0x44 {
        s.mem.set_u8(at + k, 0);
    }
    s.mem.set_u32(field, i.value);
    s.mem.set_u16(at + 4, i.mask);
    s.mem.set_u8(at + 6, i.kind);
    s.mem.set_u32(at + 8, i.name);
    s.mem.set_u32(at + 0xC, i.prop_type);
    s.mem.set_u32(at + 0x14, field);
    s.mem.set_u32(at + 0x38, i.range.0);
    s.mem.set_u32(at + 0x3C, i.range.1);
    s.mem.set_u32(at + 0x40, i.choices);
    (at, field)
}

fn read_config(home: &Path) -> String {
    String::from_utf8(std::fs::read(ddruntime::profile::config_path(home)).expect("config.ini written")).unwrap()
}

/// FUN_00D00120(out, section, key, default): the string value.
fn read_string(s: &mut Session, a: &mut Scratch, section: &str, key: &str, default: &str) -> String {
    let out = a.take(0x24);
    let (sec, k, d) = (a.str(s, section), a.str(s, key), a.str(s, default));
    returned(s.call(0x00D0_0120, REGISTRY, &[out, sec, k, d]), "0x00D00120");
    let p = s.mem.u32(out);
    if p != 0 { cstr(&s.mem, p + 8) } else { cstr(&s.mem, out + 4) }
}

#[test]

fn the_games_option_writers_and_readers_round_trip_through_config_ini() {
    let Some(exe) = installed_exe() else { eprintln!("skipped: no installed DDDA.exe"); return };
    let home = std::env::temp_dir().join(format!("ddruntime-options-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    // the session's known folders (and MtRegistry's path) are this sandbox
    std::env::set_var("DDRUNTIME_HOME", &home);
    let mut s = Session::new(&exe).expect("session");
    start_up(&mut s);
    s.mem.map_lazy(SCRATCH, SCRATCH_SIZE, READ | WRITE).unwrap();
    let mut a = Scratch(SCRATCH);
    let path = ddruntime::profile::config_path(&home);
    assert_eq!(cstr16(&s.mem, REGISTRY + 4), path.to_string_lossy(), "MtRegistry's file is the sandbox's");
    let _ = std::fs::remove_file(&path);

    // --- the PC options menu's writer 0x00EDDB20 ---------------------------
    // its resolution list (sRender +0x43CC24): 0x18-byte entries, +0 the
    // name, +8 the refresh-rate entries (8 bytes, +0 the name)
    let render = a.take(0x0044_0000);
    let rates = a.take(8);
    let r144 = a.str(&mut s, "144.00Hz");
    s.mem.set_u32(rates, r144);
    let list = a.take(0x18);
    let r1080 = a.str(&mut s, "1920x1080");
    s.mem.set_u32(list, r1080);
    s.mem.set_u32(list + 8, rates);
    s.mem.set_u32(render + 0x0043_CC24, list);
    s.mem.set_u32(RENDER_GLOBAL, render);
    let menu = a.take(0xC0);
    let words: [(u32, u32); 14] = [
        (4, 0),     // resolution 0: 1920x1080
        (8, 0),     // refresh rate 0: 144.00Hz
        (0xC, 1),   // TextureDetail: MEDIUM
        (0x10, 0),  // ShadowQuality: LOW
        (0x14, 2),  // frame rate 2: MaxFPS 150
        (0x18, 0),  // AltAntiAlias: NONE
        (0x1C, 0),  // HDR: DEFAULT
        (0x24, 15f32.to_bits()), // CameraFov
        (0x28, 10f32.to_bits()), // Brightness
        (0x2C, 1),  // GrassQuality: LOW
        (0x30, 1),  // EffectVolume: LOW
        (0x34, 3),  // TextureFiltering: ANISO_X4
        (0x38, 1),  // ViewRange: NORMAL
        (0x3C, 2),  // DofFilter: ON
    ];
    for (o, v) in words {
        s.mem.set_u32(menu + o, v);
    }
    s.mem.set_u8(menu + 0x20, 0); // VSYNC off
    s.mem.set_u8(menu + 0x21, 0); // FullScreen off
    returned(s.call(0x00ED_DB20, menu, &[]), "the menu's writer 0x00EDDB20");

    // --- the option items' writer 0x00DF2CA0 ---------------------------------
    // registration order: sMain, sRender, sound, effect, texture objects
    // (names and choice lists are the exe's: options_census.py)
    let antialias = a.take(16);
    let none = a.str(&mut s, "NONE");
    s.mem.set_u32(antialias, none); // the device's list: NONE = 0, then the end
    let items = [
        Item { mask: CPU, kind: 4, name: 0x0142_A8E4, prop_type: 6, range: (1, 8), choices: 0, value: 7 }, // JobThread
        Item { mask: GRAPHICS, kind: 3, name: 0x0142_A8D0, prop_type: 0xC, range: (30f32.to_bits(), 999f32.to_bits()), choices: 0, value: 150f32.to_bits() }, // MaxFPS
        Item { mask: CPU, kind: 5, name: 0x0142_9C78, prop_type: 3, range: (0, 0), choices: 0, value: 1 }, // RenderingThread
        Item { mask: GRAPHICS, kind: 1, name: 0x0142_9C54, prop_type: 6, range: (0, 0), choices: 0x0142_9C58, value: 1 }, // HDR: DEFAULT
        Item { mask: GRAPHICS, kind: 1, name: 0x0142_9C48, prop_type: 6, range: (0, 0), choices: antialias, value: 0 }, // AntiAlias
        Item { mask: GRAPHICS, kind: 1, name: 0x0142_9C10, prop_type: 6, range: (0, 0), choices: 0x0142_9C20, value: 0 }, // AltAntiAlias
        Item { mask: GRAPHICS, kind: 5, name: 0x0142_9BC0, prop_type: 3, range: (0, 0), choices: 0, value: 0 }, // SLI
        Item { mask: GRAPHICS, kind: 5, name: 0x0142_9BB8, prop_type: 3, range: (0, 0), choices: 0, value: 0 }, // Stereo
        Item { mask: GRAPHICS, kind: 1, name: 0x0142_9B94, prop_type: 6, range: (0, 0), choices: 0x0142_9BF0, value: 0 }, // ShadowQuality: LOW
        Item { mask: DISPLAY, kind: 5, name: 0x0142_9BAC, prop_type: 3, range: (0, 0), choices: 0, value: 0 }, // FullScreen
        Item { mask: DISPLAY, kind: 5, name: 0x0142_9B8C, prop_type: 3, range: (0, 0), choices: 0, value: 0 }, // VSYNC
        Item { mask: DISPLAY, kind: 5, name: 0x0142_9BE8, prop_type: 3, range: (0, 0), choices: 0, value: 0 }, // Flush
        Item { mask: SOUNDS, kind: 4, name: 0x0142_B1AC, prop_type: 6, range: (16, 128), choices: 0, value: 128 }, // SeVoiceNum
        Item { mask: GRAPHICS, kind: 1, name: 0x0143_3ADC, prop_type: 6, range: (0, 0), choices: 0x0143_8868, value: 0 }, // EffectVolume: LOW
        Item { mask: GRAPHICS, kind: 1, name: 0x0156_1CDC, prop_type: 6, range: (0, 0), choices: 0x0143_A6A0, value: 2 }, // TextureDetail: MEDIUM
        Item { mask: GRAPHICS, kind: 1, name: 0x0143_A63C, prop_type: 6, range: (0, 0), choices: 0x0143_A650, value: 1 }, // TextureMipLimite: 2
    ];
    let names: Vec<String> = items.iter().map(|i| cstr(&s.mem, i.name)).collect();
    assert_eq!(names, ["JobThread", "MaxFPS", "RenderingThread", "HDR", "AntiAlias", "AltAntiAlias", "SLI", "Stereo",
        "ShadowQuality", "FullScreen", "VSYNC", "Flush", "SeVoiceNum", "EffectVolume", "TextureDetail", "TextureMipLimite"]);
    let app = a.take(0x0000_2700);
    let mut fields = Vec::new();
    for (k, i) in items.iter().enumerate() {
        let (at, field) = item(&mut s, &mut a, i);
        s.mem.set_u32(app + 0x154 + 4 * k as u32, at);
        fields.push(field);
    }
    s.mem.set_u32(app + 0x554, items.len() as u32);
    returned(s.call(0x00DF_2CA0, app, &[]), "the items' writer 0x00DF2CA0");

    // --- the window's position at exit (0x00DF1550, WM_DESTROY) --------------
    for key in ["MainX", "MainY"] {
        let (sec, k) = (a.str(&mut s, "Window"), a.str(&mut s, key));
        returned(s.call(0x00CF_FE30, REGISTRY, &[sec, k, 0]), "0x00CFFE30");
    }
    assert_eq!(read_config(&home), EXPECTED, "config.ini as the original writes it");

    // --- reading it back -----------------------------------------------------
    for (f, i) in fields.iter().zip(&items) {
        s.mem.set_u32(*f, if i.kind == 3 { 30f32.to_bits() } else { 0 });
    }
    returned(s.call(0x00DF_39E0, app, &[0xFFFF]), "the items' reader 0x00DF39E0");
    for ((f, i), n) in fields.iter().zip(&items).zip(&names) {
        let got = if i.prop_type == 3 { s.mem.u8(*f) as u32 } else { s.mem.u32(*f) };
        assert_eq!(got, i.value, "{n} read back");
    }
    let fps = returned(s.call(0x00DB_E0F0, 0, &[]), "the MaxFPS reader 0x00DBE0F0").st.top_f32();
    assert_eq!(fps, Some(150f32.to_bits()));
    assert_eq!(read_string(&mut s, &mut a, "DISPLAY", "Resolution", "1280x720"), "1920x1080");
    assert_eq!(read_string(&mut s, &mut a, "DISPLAY", "RefreshRate", ""), "144.00Hz");
    let (sec, k) = (a.str(&mut s, "Window"), a.str(&mut s, "MainX"));
    assert_eq!(returned(s.call(0x00D0_01E0, REGISTRY, &[sec, k, 0x20]), "0x00D001E0").eax, 0);
    let (sec, k) = (a.str(&mut s, "DISPLAY"), a.str(&mut s, "VSYNC"));
    assert_eq!(returned(s.call(0x00D0_02A0, REGISTRY, &[sec, k, 1]), "0x00D002A0").eax & 0xFF, 0, "VSYNC=OFF");

    // --- an imported config.ini is honoured, and its source only read --------
    let player = home.join("player-config.ini");
    let theirs = EXPECTED.replace("MaxFPS=150.000000", "MaxFPS=60.000000").replace("JobThread=7", "JobThread=3").replace("Resolution=1920x1080", "Resolution=2560x1440");
    std::fs::write(&player, &theirs).unwrap();
    assert!(!ddruntime::profile::import_config(&player, &home, false).unwrap(), "the sandbox keeps its own config.ini");
    assert!(ddruntime::profile::import_config(&player, &home, true).unwrap());
    assert_eq!(std::fs::read_to_string(&player).unwrap(), theirs, "the source is untouched");
    returned(s.call(0x00DF_39E0, app, &[0xFFFF]), "the items' reader 0x00DF39E0");
    assert_eq!(s.mem.u32(fields[1]), 60f32.to_bits(), "MaxFPS from the imported file");
    assert_eq!(s.mem.u32(fields[0]), 3, "JobThread from the imported file");
    assert_eq!(read_string(&mut s, &mut a, "DISPLAY", "Resolution", "1280x720"), "2560x1440");

    // the player's own config.ini, when this machine has one: every key of
    // it reads back through the game's string reader as written
    if let Some(own) = ddruntime::profile::player_config() {
        let text = std::fs::read(&own).unwrap();
        assert!(ddruntime::profile::import_config(&own, &home, true).unwrap());
        assert_eq!(std::fs::read(&own).unwrap(), text, "the player's file is only read");
        let text = String::from_utf8_lossy(&text).to_string();
        let mut section = String::new();
        let mut checked = 0;
        for line in text.lines() {
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = name.to_string();
            } else if let Some((k, v)) = line.split_once('=') {
                assert_eq!(read_string(&mut s, &mut a, &section, k, "?"), v.trim(), "[{section}] {k}");
                checked += 1;
            }
        }
        assert!(checked > 0);
    }
    let _ = std::fs::remove_dir_all(&home);
}

fn cstr16(m: &dyn Mem, at: u32) -> String {
    let units: Vec<u16> = (0..).map(|k| m.u16(at + 2 * k)).take_while(|&u| u != 0).collect();
    String::from_utf16_lossy(&units)
}
