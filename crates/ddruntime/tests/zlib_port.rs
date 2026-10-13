//! Original instruction fixtures for cf02e0 guard/terminal/empty-input paths.
//! Whole-stream state/window/output coverage is research/native/check_l145_streams.py.
use ddnative_base::{mem::{Mem, Sparse}, x86::{Cpu, Flags, RAW}};
use std::io::{Cursor, Read};

fn word(c: &mut Cursor<Vec<u8>>) -> u32 {
    let mut b = [0; 4]; c.read_exact(&mut b).unwrap(); u32::from_le_bytes(b)
}
fn machine(c: &mut Cursor<Vec<u8>>) -> ([u32; 41], Vec<(u32, Vec<u8>)>) {
    let r = std::array::from_fn(|_| word(c));
    let windows = (0..word(c)).map(|_| {
        let address = word(c);
        let mut bytes = vec![0; word(c) as usize];
        c.read_exact(&mut bytes).unwrap(); (address, bytes)
    }).collect();
    (r, windows)
}

#[test]
fn original_guards_terminals_and_empty_input() {
    let path = std::env::var("L145_VECTORS")
        .unwrap_or_else(|_| "E:/ddda-re/notes/native-L145/inflate-guards.bin".into());
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => { eprintln!("private vectors absent: {path}"); return; }
    };
    let mut c = Cursor::new(bytes);
    let mut magic = [0; 8]; c.read_exact(&mut magic).unwrap();
    assert_eq!(&magic, b"L145IF01");
    let count = word(&mut c);
    assert!(count >= 12);
    for index in 0..count {
        let (r, input) = machine(&mut c);
        let (expected, output) = machine(&mut c);
        let mut m = Sparse::default();
        for (a, bytes) in input { for (i, byte) in bytes.into_iter().enumerate() { m.set_u8(a+i as u32, byte); } }
        let mut cpu = Cpu { eax:r[0],ecx:r[1],edx:r[2],ebx:r[3],esp:r[4],ebp:r[5],esi:r[6],edi:r[7],
            fl:Flags::new(RAW,0,0,r[8],32),..Cpu::default() };
        for i in 0..8 { cpu.xmm[i].copy_from_slice(&r[9+i*4..13+i*4]); }
        ddruntime::archive_inflate::port::inflate_with(&mut m, &mut cpu, &mut |_,_,target,_| panic!("unexpected callee {target:#x}"));
        assert_eq!([cpu.eax,cpu.ecx,cpu.edx,cpu.ebx,cpu.esp,cpu.ebp,cpu.esi,cpu.edi], expected[..8], "case {index}");
        let (flags, valid) = cpu.fl.snapshot();
        assert_eq!(flags & valid & 0x8d5, expected[8] & valid & 0x8d5, "case {index} flags");
        for i in 0..8 { assert_eq!(cpu.xmm[i],expected[9+i*4..13+i*4], "case {index} XMM{i}"); }
        for (a, bytes) in output { for (i, byte) in bytes.into_iter().enumerate() { assert_eq!(m.u8(a+i as u32),byte,"case {index}, address {:#x}",a+i as u32); } }
    }
    assert_eq!(c.position() as usize,c.get_ref().len());
    eprintln!("{count} original outer inflater cases PASS");
}
