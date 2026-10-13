//! Original cf30c0 instructions: complete, incomplete and oversubscribed
//! Huffman trees, symbol classes, root widths and table exhaustion.
use ddnative_base::{
    mem::Mem,
    x86::{Cpu, Flags, RAW},
};
use std::io::{Cursor, Read};

fn word(c: &mut Cursor<Vec<u8>>) -> u32 {
    let mut b = [0; 4];
    c.read_exact(&mut b).unwrap();
    u32::from_le_bytes(b)
}
fn machine(c: &mut Cursor<Vec<u8>>) -> ([u32; 41], Vec<(u32, Vec<u8>)>) {
    let r = std::array::from_fn(|_| word(c));
    let windows = (0..word(c))
        .map(|_| {
            let address = word(c);
            let mut bytes = vec![0; word(c) as usize];
            c.read_exact(&mut bytes).unwrap();
            (address, bytes)
        })
        .collect();
    (r, windows)
}

#[test]
// verifies 0x00cf30c0
fn original_table_builder_all_memory_and_registers() {
    let path = std::env::var("L156_TABLE_VECTORS")
        .unwrap_or_else(|_| "E:/ddda-re/notes/native-L156/table.bin".into());
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if std::env::var_os("DDNATIVE_REQUIRE_VECTORS").is_some() => panic!("{path}: {e}"),
        Err(_) => {
            eprintln!("private vectors absent: {path}");
            return;
        }
    };
    ddnative_base::mem::ATOMIC_ACCESSES.store(true, std::sync::atomic::Ordering::Relaxed);
    let mut c = Cursor::new(bytes);
    let mut magic = [0; 8];
    c.read_exact(&mut magic).unwrap();
    assert_eq!(&magic, b"L156TB01");
    let count = word(&mut c);
    assert!(count >= 81);
    for index in 0..count {
        let (r, input) = machine(&mut c);
        let (expected, output) = machine(&mut c);
        // Real guest pages exercise the new table memory policy. The
        // streaming oracle also exercises the trait-only adapter.
        let mut m = ddruntime::memory::GuestMemory::new();
        let mut pages = std::collections::BTreeSet::new();
        for (address, bytes) in &input {
            for page in (*address & !4095..(address + bytes.len() as u32 + 4095) & !4095).step_by(4096) {
                if pages.insert(page) {
                    m.map(page, 4096, ddruntime::memory::READ | ddruntime::memory::WRITE)
                        .unwrap();
                }
            }
        }
        for (a, bytes) in input {
            for (i, byte) in bytes.into_iter().enumerate() {
                m.set_u8(a + i as u32, byte);
            }
        }
        let mut cpu = Cpu {
            eax: r[0],
            ecx: r[1],
            edx: r[2],
            ebx: r[3],
            esp: r[4],
            ebp: r[5],
            esi: r[6],
            edi: r[7],
            fl: Flags::new(RAW, 0, 0, r[8], 32),
            ..Cpu::default()
        };
        for i in 0..8 {
            cpu.xmm[i].copy_from_slice(&r[9 + i * 4..13 + i * 4]);
        }
        ddruntime::archive_inflate::table::build(&mut m, &mut cpu);
        assert_eq!(
            [
                cpu.eax, cpu.ecx, cpu.edx, cpu.ebx, cpu.esp, cpu.ebp, cpu.esi, cpu.edi
            ],
            expected[..8],
            "case {index}"
        );
        let (flags, valid) = cpu.fl.snapshot();
        assert_eq!(
            flags & valid & 0x8d5,
            expected[8] & valid & 0x8d5,
            "case {index} flags"
        );
        for i in 0..8 {
            assert_eq!(cpu.xmm[i], expected[9 + i * 4..13 + i * 4], "case {index} XMM{i}");
        }
        for (a, bytes) in output {
            for (i, byte) in bytes.into_iter().enumerate() {
                assert_eq!(
                    m.u8(a + i as u32),
                    byte,
                    "case {index}, address {:#x}",
                    a + i as u32
                );
            }
        }
    }
    assert_eq!(c.position() as usize, c.get_ref().len());
    eprintln!("{count} original table builder cases PASS");
}
