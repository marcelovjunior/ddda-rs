//! The D3DX9_43 imports (brief L37) called as the game calls them
//! (0x00E10690): D3DXGetShaderConstantTableEx with flags 0x20000, then
//! GetDesc, GetConstant(NULL, i), GetConstantDesc and Release on the
//! returned ID3DXConstantTable, and D3DXGetShaderOutputSemantics, on the
//! Title package's shaders in guest memory. Every answer is compared with
//! d3dx9_43.dll on the same bytes. Skips without the game or the DLL.
use ddnative_base::mem::Mem;
use ddruntime::session::{Outcome, Session};
use std::path::PathBuf;

const SCRATCH: u32 = 0x7EFF_0000;
const SHADER: u32 = 0x7EF0_0000;

fn installed_exe() -> Option<ddexe::ExeFile> {
    let path = std::env::var("DDDA_EXE").unwrap_or_else(|_| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    ddexe::ExeFile::open(path).ok()
}

/// The Title package's distinct shaders.
fn title_shaders() -> Option<Vec<Vec<u32>>> {
    let native_pc = std::env::var_os("DDDA_NATIVEPC").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC"));
    let (archive, mut file) = mtarc::Archive::open(native_pc.join("sa").join("DX9").join("Title.arc")).ok()?;
    let entry = archive.entries.iter().find(|e| e.type_hash == mtarc::type_hash("rShaderPackage"))?;
    let tokens = d3d9shader::tokens(&mtarc::read_entry(&mut file, entry).ok()?);
    let mut out: Vec<Vec<u32>> = Vec::new();
    let mut k = 0;
    while k < tokens.len() {
        if matches!(tokens[k], 0xFFFE_0300 | 0xFFFF_0300) {
            if let Ok(s) = d3d9shader::parse(&tokens[k..]) {
                if s.instructions.iter().any(|i| i.opcode != d3d9shader::op::COMMENT) {
                    let t = tokens[k..k + s.tokens].to_vec();
                    if !out.contains(&t) {
                        out.push(t);
                    }
                    k += s.tokens;
                    continue;
                }
            }
        }
        k += 1;
    }
    Some(out)
}

fn call(s: &mut Session, f: u32, args: &[u32]) -> u32 {
    match s.call(f, 0, args) {
        Outcome::Returned(c) => c.eax,
        o => panic!("{o}"),
    }
}

fn string(s: &Session, mut p: u32) -> String {
    let mut v = Vec::new();
    loop {
        let b = s.mem.u8(p);
        if b == 0 {
            break;
        }
        v.push(b);
        p += 1;
    }
    String::from_utf8_lossy(&v).into_owned()
}

#[test]
fn the_imports_answer_like_d3dx9_43_on_the_title_shaders() {
    let (Some(exe), Some(dll), Some(shaders)) = (installed_exe(), d3dxref::D3dx::load(), title_shaders()) else {
        eprintln!("skipped: game install or d3dx9_43.dll not found");
        return;
    };
    let path = exe.path.to_string_lossy().to_string();
    let mut s = Session::new(&path).expect("session");
    let token = |s: &Session, n: &str| s.imports.iter().find(|i| i.full_name().eq_ignore_ascii_case(n)).unwrap_or_else(|| panic!("{n}")).token;
    let (get_table, get_semantics) = (token(&s, "d3dx9_43.dll!D3DXGetShaderConstantTableEx"), token(&s, "d3dx9_43.dll!D3DXGetShaderOutputSemantics"));
    let (out, desc, count) = (SCRATCH, SCRATCH + 0x100, SCRATCH + 0x200);
    let mut constants = 0;
    for tokens in &shaders {
        for (k, t) in tokens.iter().enumerate() {
            s.mem.set_u32(SHADER + 4 * k as u32, *t);
        }
        let theirs = dll.constant_table(tokens, 0x20000).expect("the DLL reads every game shader");
        assert_eq!(call(&mut s, get_table, &[SHADER, 0x20000, out]), 0);
        let table = s.mem.u32(out);
        let method = |s: &Session, slot: u32| s.mem.u32(s.mem.u32(table) + 4 * slot);
        // GetDesc: Creator, Version, Constants
        let f = method(&s, 5);
        call(&mut s, f, &[table, desc]);
        assert_eq!((string(&s, s.mem.u32(desc)), s.mem.u32(desc + 4), s.mem.u32(desc + 8)),
                   (theirs.creator.clone(), theirs.version, theirs.constants.len() as u32));
        for (i, c) in theirs.constants.iter().enumerate() {
            let f = method(&s, 8);
            let h = call(&mut s, f, &[table, 0, i as u32]);
            assert_ne!(h, 0);
            s.mem.set_u32(count, 0); // as the game's second loop calls it
            let f = method(&s, 6);
            assert_eq!(call(&mut s, f, &[table, h, desc, count]), 0);
            assert_eq!(s.mem.u32(count), 1);
            let w: Vec<u32> = (0..12).map(|k| s.mem.u32(desc + 4 * k)).collect();
            let d = &c.desc;
            assert_eq!(string(&s, w[0]), d.name);
            assert_eq!(w[1..11], [d.register_set, d.register_index, d.register_count, d.class, d.ty, d.rows, d.columns, d.elements, d.struct_members, d.bytes], "{}", d.name);
            let default = (w[11] != 0).then(|| (0..d.bytes).map(|k| s.mem.u8(w[11] + k)).collect::<Vec<u8>>());
            assert_eq!(default, d.default_value, "{} default value", d.name);
            constants += 1;
        }
        let f = method(&s, 8);
        assert_eq!(call(&mut s, f, &[table, 0, theirs.constants.len() as u32]), 0, "past the end");
        let f = method(&s, 2);
        assert_eq!(call(&mut s, f, &[table]), 0, "Release frees the table");
        if tokens[0] == 0xFFFE_0300 {
            let (sem, n) = dll.output_semantics(tokens).unwrap();
            assert_eq!(call(&mut s, get_semantics, &[SHADER, desc, count]), 0);
            let ours: Vec<(u32, u32)> = (0..s.mem.u32(count)).map(|k| (s.mem.u32(desc + 8 * k), s.mem.u32(desc + 8 * k + 4))).collect();
            assert_eq!(ours, sem);
            assert_eq!(n as usize, sem.len());
        }
    }
    eprintln!("{} Title shaders, {constants} constants: equal to d3dx9_43.dll", shaders.len());
}
