//! Private original-instruction oracle, including read-only installed ARC data.
use ddnative_base::{mem::{Mem, Sparse}, x86::Cpu};
use std::io::{Cursor, Read};

fn word(c: &mut Cursor<Vec<u8>>) -> u32 {
    let mut b = [0; 4]; c.read_exact(&mut b).unwrap(); u32::from_le_bytes(b)
}
fn machine(c: &mut Cursor<Vec<u8>>) -> ([u32; 8], Vec<(u32, Vec<u8>)>) {
    let regs = std::array::from_fn(|_| word(c));
    let windows = (0..word(c)).map(|_| {
        let address = word(c);
        let mut bytes = vec![0; word(c) as usize]; c.read_exact(&mut bytes).unwrap();
        (address, bytes)
    }).collect();
    (regs, windows)
}

#[test]
fn original_adler_all_registers_memory_and_flags() {
    let path = std::env::var("L140_ADLER_VECTORS").unwrap_or_else(|_| "E:/ddda-re/notes/native-L140/adler.bin".into());
    let bytes = match std::fs::read(&path) {
        Ok(v) => v,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => { eprintln!("private vectors absent: {path}"); return; }
    };
    let mut c = Cursor::new(bytes);
    let mut magic = [0; 8]; c.read_exact(&mut magic).unwrap(); assert_eq!(&magic, b"L140AD01");
    let count = word(&mut c);
    for index in 0..count {
        let (r, input) = machine(&mut c);
        let (expected, output) = machine(&mut c);
        let flags = word(&mut c);
        let mut m = Sparse::default();
        for (address, bytes) in input { for (i, b) in bytes.into_iter().enumerate() { m.set_u8(address + i as u32, b); } }
        let mut cpu = Cpu { eax:r[0], ecx:r[1], edx:r[2], ebx:r[3], esp:r[4], ebp:r[5], esi:r[6], edi:r[7], ..Cpu::default() };
        ddruntime::archive_inflate::adler32(&mut m, &mut cpu);
        assert_eq!([cpu.eax,cpu.ecx,cpu.edx,cpu.ebx,cpu.esp,cpu.ebp,cpu.esi,cpu.edi], expected, "case {index}");
        assert_eq!(cpu.fl.snapshot().0 & 0x8c5, flags & 0x8c5, "case {index} defined flags");
        for (address, bytes) in output { for (i, b) in bytes.into_iter().enumerate() { assert_eq!(m.u8(address + i as u32), b, "case {index} memory {:#x}", address + i as u32); } }
    }
    assert_eq!(c.position() as usize, c.get_ref().len());
    eprintln!("{count} original Adler cases matched (all GPRs, memory, defined flags)");
}
