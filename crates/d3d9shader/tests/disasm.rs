//! Every vs_3_0/ps_3_0 shader in the installed game's shader packages,
//! decoded and printed by this crate, against Microsoft's D3DDisassemble
//! (D3DCompiler_47.dll, part of Windows). Instruction lines must match
//! exactly; Microsoft's comment header (constant table) is not compared.
//! Skips when the game or the DLL is not present.
use std::collections::HashSet;
use std::ffi::c_void;
use std::path::PathBuf;

/// The shader packages compared: the title screen, the game's root and
/// main packages, and the first stage (Cassardis), which together hold
/// several thousand distinct shaders.
const PACKAGES: &[&str] = &["Title", "GameMain", "root", "Stage100"];

fn native_pc() -> Option<PathBuf> {
    let p = std::env::var_os("DDDA_NATIVEPC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC"));
    p.is_dir().then_some(p)
}

fn package(native_pc: &std::path::Path, name: &str) -> Option<Vec<u8>> {
    let path = native_pc.join("sa").join("DX9").join(format!("{name}.arc"));
    let (archive, mut file) = mtarc::Archive::open(&path).ok()?;
    let wanted = mtarc::type_hash("rShaderPackage");
    let entry = archive.entries.iter().find(|e| e.type_hash == wanted)?;
    mtarc::read_entry(&mut file, entry).ok()
}

/// Shaders inside a package: positions of version tokens that start a
/// stream this crate decodes to END (the constant table also stores the
/// version token; those positions do not decode).
fn shaders(bytes: &[u8]) -> Vec<Vec<u32>> {
    let tokens = d3d9shader::tokens(bytes);
    let mut out = Vec::new();
    let mut k = 0;
    while k < tokens.len() {
        if matches!(tokens[k], 0xFFFE_0300 | 0xFFFF_0300) {
            if let Ok(s) = d3d9shader::parse(&tokens[k..]) {
                if s.instructions.iter().any(|i| i.opcode != d3d9shader::op::COMMENT) {
                    out.push(tokens[k..k + s.tokens].to_vec());
                    k += s.tokens;
                    continue;
                }
            }
        }
        k += 1;
    }
    out
}

type Disassemble = unsafe extern "system" fn(*const c_void, usize, u32, *const u8, *mut *mut c_void) -> i32;

extern "system" {
    fn LoadLibraryA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
}

fn microsoft() -> Option<Disassemble> {
    unsafe {
        let module = LoadLibraryA(c"D3DCompiler_47.dll".as_ptr() as *const u8);
        if module.is_null() {
            return None;
        }
        let f = GetProcAddress(module, c"D3DDisassemble".as_ptr() as *const u8);
        (!f.is_null()).then(|| std::mem::transmute::<*mut c_void, Disassemble>(f))
    }
}

/// D3DDisassemble's text (an ID3DBlob: GetBufferPointer is vtable slot 3,
/// GetBufferSize slot 4, Release slot 2).
fn reference(f: Disassemble, tokens: &[u32]) -> String {
    let bytes: Vec<u8> = tokens.iter().flat_map(|t| t.to_le_bytes()).collect();
    unsafe {
        let mut blob: *mut c_void = std::ptr::null_mut();
        let hr = f(bytes.as_ptr() as *const c_void, bytes.len(), 0, std::ptr::null(), &mut blob);
        assert!(hr >= 0 && !blob.is_null(), "D3DDisassemble failed: {hr:#x}");
        let vtable = *(blob as *const *const usize);
        let pointer: unsafe extern "system" fn(*mut c_void) -> *const u8 = std::mem::transmute(*vtable.add(3));
        let size: unsafe extern "system" fn(*mut c_void) -> usize = std::mem::transmute(*vtable.add(4));
        let release: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(*vtable.add(2));
        let text = std::slice::from_raw_parts(pointer(blob), size(blob));
        let text = String::from_utf8_lossy(text).trim_end_matches('\0').to_string();
        release(blob);
        text
    }
}

fn instruction_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.trim_end().to_string())
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with("//"))
        .collect()
}

#[test]
fn every_game_shader_disassembles_like_d3ddisassemble() {
    let (Some(native_pc), Some(ms)) = (native_pc(), microsoft()) else {
        eprintln!("skipped: game install or D3DCompiler_47.dll not found");
        return;
    };
    let mut seen = HashSet::new();
    let (mut compared, mut differing) = (0usize, 0usize);
    let mut examples = Vec::new();
    // DDDA_ALL_SHADERS=1: every package in sa\DX9 (all stages), not only the
    // representative ones
    let names: Vec<String> = if std::env::var_os("DDDA_ALL_SHADERS").is_some() {
        let dir = native_pc.join("sa").join("DX9");
        let mut v: Vec<String> = std::fs::read_dir(&dir).map(|d| d.filter_map(|e| e.ok())
            .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned()))
            .collect()).unwrap_or_default();
        v.sort();
        v
    } else {
        PACKAGES.iter().map(|s| s.to_string()).collect()
    };
    for name in &names {
        let Some(bytes) = package(&native_pc, name) else { continue };
        for tokens in shaders(&bytes) {
            if !seen.insert(tokens.clone()) {
                continue;
            }
            compared += 1;
            let shader = d3d9shader::parse(&tokens).expect("decoded once already");
            let ours = d3d9shader::disasm::disassemble(&shader);
            let theirs = instruction_lines(&reference(ms, &tokens));
            if ours != theirs {
                differing += 1;
                if examples.len() < 12 {
                    let k = ours.iter().zip(&theirs).position(|(a, b)| a != b).unwrap_or(ours.len().min(theirs.len()));
                    examples.push(format!(
                        "{name}: line {k}: ours {:?} / Microsoft {:?}",
                        ours.get(k),
                        theirs.get(k)
                    ));
                }
            }
        }
    }
    eprintln!("{compared} distinct shaders compared, {differing} differ");
    assert!(compared > 1000, "expected thousands of shaders, found {compared}");
    assert!(differing == 0, "{differing} of {compared} differ, e.g.:\n{}", examples.join("\n"));
}
