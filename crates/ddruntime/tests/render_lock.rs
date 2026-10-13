//! L82: real COM dispatch with the headless recorder, no game execution/GPU.
use ddnative_base::mem::Mem;
use ddruntime::session::{Outcome, Session};

const SCRATCH: u32 = 0x7EFF_0000;
const PP: u32 = SCRATCH + 0x200;
const OUT: u32 = SCRATCH + 0x300;

fn call(s: &mut Session, target: u32, args: &[u32]) -> u32 {
    match s.call(target, 0, args) {
        Outcome::Returned(c) => c.eax,
        other => panic!("{other:?}"),
    }
}

fn com(s: &mut Session, object: u32, slot: u32, args: &[u32]) -> u32 {
    let target = s.mem.u32(s.mem.u32(object) + slot * 4);
    call(s, target, &[&[object], args].concat())
}

fn token(s: &Session, name: &str) -> u32 {
    s.imports.iter().find(|i| i.full_name() == name).unwrap().token
}

fn acquisitions(report: &str, service: &str) -> u64 {
    let mut lines = report.lines();
    while let Some(line) = lines.next() {
        if line.contains(service) && !line.contains("blocks tid=") {
            let count = lines.next().unwrap().trim().strip_prefix("acquisitions=").unwrap();
            return count.split_whitespace().next().unwrap().parse().unwrap();
        }
    }
    panic!("missing {service}: {report}");
}

#[test]
fn draw_and_clear_return_without_a_second_acquisition() {
    let path = std::env::var("DDDA_EXE")
        .unwrap_or_else(|_| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    if ddexe::ExeFile::open(&path).is_err() {
        eprintln!("skipped: no installed game image");
        return;
    }
    std::env::set_var("DDRUNTIME_LOCK_STATS", "1");
    let mut s = Session::new(&path).unwrap();
    ddruntime::threads::set_parallel(true);
    let name = SCRATCH + 0x100;
    s.mem.set_u16(name, b'W' as u16);
    s.mem.set_u32(SCRATCH + 8, token(&s, "USER32.dll!DefWindowProcW"));
    s.mem.set_u32(SCRATCH + 40, name);
    let f = token(&s, "USER32.dll!RegisterClassExW");
    call(&mut s, f, &[SCRATCH]);
    let f = token(&s, "USER32.dll!CreateWindowExW");
    let hwnd = call(&mut s, f, &[0, name, 0, 0x00CA_0000, 0, 0, 70, 93, 0, 0, 0x0040_0000, 0]);
    let f = token(&s, "d3d9.dll!Direct3DCreate9");
    let d3d = call(&mut s, f, &[32]);
    for (k, v) in [64, 64, 21, 1, 0, 0, 1, hwnd, 1, 0, 0, 0, 0, 0].into_iter().enumerate() {
        s.mem.set_u32(PP + 4 * k as u32, v);
    }
    assert_eq!(com(&mut s, d3d, 16, &[0, 1, hwnd, 0x46, PP, OUT]), 0);
    let dev = s.mem.u32(OUT);
    assert_eq!(com(&mut s, dev, 27, &[6, 0, 101, 1, OUT, 0]), 0); // index buffer
    let ib = s.mem.u32(OUT);
    assert_eq!(com(&mut s, dev, 104, &[ib]), 0);
    assert_eq!(com(&mut s, dev, 41, &[]), 0); // BeginScene
    assert_eq!(com(&mut s, dev, 81, &[4, 0, 1]), 0); // DrawPrimitive
    assert_eq!(com(&mut s, dev, 82, &[4, 0, 0, 3, 0, 1]), 0); // DrawIndexedPrimitive
    assert_eq!(com(&mut s, dev, 43, &[0, 0, 1, 0x12345678, 1f32.to_bits(), 0]), 0);
    assert_eq!(com(&mut s, dev, 83, &[4, 1, SCRATCH + 0x400, 12]), 0); // UP retains reacquisition
    assert_eq!(com(&mut s, dev, 42, &[]), 0); // EndScene
    assert_eq!(com(&mut s, dev, 17, &[0, 0, 0, 0]), 0); // Present/frame delimiter
    let report = ddruntime::threads::lock_stats().unwrap();
    for service in ["::DrawPrimitive (", "::DrawIndexedPrimitive (", "::Clear ("] {
        assert_eq!(acquisitions(&report, service), 1, "{service}");
    }
    assert_eq!(acquisitions(&report, "::DrawPrimitiveUP ("), 2);
    ddruntime::d3d9draw::recorded(|r| {
        assert_eq!(r.draws.len(), 3);
        assert!(matches!(r.draws[0].indices, d3d9cmd::Indices::None));
        assert!(matches!(r.draws[1].indices, d3d9cmd::Indices::Buffer(p, 101) if p == ib));
        assert_eq!(r.clears[0].colour, 0x12345678);
        assert_eq!(r.presents, 1);
    }).unwrap();
}
