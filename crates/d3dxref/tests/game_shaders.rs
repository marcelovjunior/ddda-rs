//! Every distinct shader of the installed game (all packages in sa\DX9):
//! d3d9shader's constant-table reflection and output semantics against
//! d3dx9_43.dll. Skips without the game or the DLL.
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

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

/// The shaders in a package (as in d3d9shader's disassembly test).
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

/// The game's flags (0x00E10690): D3DXCONSTTABLE_LARGEADDRESSAWARE.
const FLAGS: u32 = 0x20000;

#[test]
fn every_game_shader_reflects_like_d3dx9_43() {
    let (Some(native_pc), Some(dll)) = (native_pc(), d3dxref::D3dx::load()) else {
        eprintln!("skipped: game install or d3dx9_43.dll not found");
        return;
    };
    let dir = native_pc.join("sa").join("DX9");
    let mut names: Vec<String> = std::fs::read_dir(&dir).map(|d| d.filter_map(|e| e.ok())
        .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned())).collect()).unwrap_or_default();
    names.sort();
    let mut seen = HashSet::new();
    let (mut compared, mut tables, mut differ) = (0usize, 0usize, 0usize);
    let mut examples = Vec::new();
    let mut count0 = BTreeMap::new();
    for name in &names {
        let Some(bytes) = package(&native_pc, name) else { continue };
        for tokens in shaders(&bytes) {
            if !seen.insert(tokens.clone()) {
                continue;
            }
            compared += 1;
            let shader = d3d9shader::parse(&tokens).unwrap();
            let ours = d3d9shader::ctab::table(&shader);
            let theirs = dll.constant_table(&tokens, FLAGS);
            let mut problems = Vec::new();
            match (&ours, &theirs) {
                (Some(o), Ok(t)) => {
                    tables += 1;
                    if o.creator != t.creator || o.version != t.version || o.constants.len() != t.constants.len() {
                        problems.push(format!("desc: ours ({:?}, {:#x}, {}) / DLL ({:?}, {:#x}, {})", o.creator, o.version, o.constants.len(), t.creator, t.version, t.constants.len()));
                    }
                    if !t.past_end_is_null {
                        problems.push("the DLL answers GetConstant past the end".into());
                    }
                    for (c, d) in o.constants.iter().zip(&t.constants) {
                        *count0.entry(d.count0).or_insert(0usize) += 1;
                        let x = &d.desc;
                        let default = (c.default_offset != 0).then(|| o.data[c.default_offset as usize..][..c.bytes as usize].to_vec());
                        let mine = d3dxref::ConstantDesc {
                            name: c.name.clone(), register_set: c.register_set, register_index: c.register_index, register_count: c.register_count,
                            class: c.class, ty: c.ty, rows: c.rows, columns: c.columns, elements: c.elements, struct_members: c.struct_members,
                            bytes: c.bytes, default_value: default,
                        };
                        if d.hr != 0 || d.count != 1 {
                            problems.push(format!("{}: GetConstantDesc hr {:#x} count {}", c.name, d.hr, d.count));
                        }
                        if &mine != x {
                            problems.push(format!("{}: ours {mine:?} / DLL {x:?}", c.name));
                        }
                    }
                }
                (None, Err(_)) => {}
                (o, t) => problems.push(format!("table: ours {:?} / DLL {:?}", o.as_ref().map(|o| o.constants.len()), t.as_ref().map(|t| t.constants.len()))),
            }
            if shader.stage == d3d9shader::Stage::Vertex {
                let o = d3d9shader::ctab::output_semantics(&shader);
                match dll.output_semantics(&tokens) {
                    Ok((t, n)) if t == o && n as usize == o.len() => {}
                    r => problems.push(format!("output semantics: ours {o:?} / DLL {r:?}")),
                }
            }
            if !problems.is_empty() {
                differ += 1;
                if examples.len() < 15 {
                    examples.push(format!("{name}: {}", problems.join("; ")));
                }
            }
        }
    }
    eprintln!("{compared} distinct shaders, {tables} with constant tables, {differ} differ");
    eprintln!("GetConstantDesc with *pCount = 0 (hr, count, desc written): {count0:?}");
    for e in &examples {
        eprintln!("{e}");
    }
    assert!(compared > 0 && differ == 0);
}

/// The DLL's answers outside the game's use (prints): a shader without a
/// constant table, and flags 0.
#[test]
fn edge_cases() {
    let Some(dll) = d3dxref::D3dx::load() else { return };
    // vs_3_0: dcl_position v0; dcl_position o0; mov o0, v0
    let bare = [0xFFFE_0300u32, 0x0200_001F, 0x8000_0000, 0x900F_0000, 0x0200_001F, 0x8000_0000, 0xE00F_0000, 0x0200_0001, 0xE00F_0000, 0x90E4_0000, 0x0000_FFFF];
    eprintln!("no constant table, flags 0x20000: {:?}", dll.constant_table(&bare, FLAGS).map(|t| t.constants.len()).map_err(|e| format!("{:#x}", e as u32)));
    eprintln!("no constant table, output semantics: {:?}", dll.output_semantics(&bare));
}
