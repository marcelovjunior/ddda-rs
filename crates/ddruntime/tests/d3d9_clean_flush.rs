//! L98: headless COM sequences exercise dirty owners and surface aliases.
use ddnative_base::mem::Mem;
use ddruntime::session::{Outcome, Session};
use ddruntime::d3d9draw::{recorded, Upload};

const SCRATCH: u32 = 0x7EFF_0000;
const PP: u32 = SCRATCH + 0x200;
const OUT: u32 = SCRATCH + 0x300;
const DATA: u32 = SCRATCH + 0x400;

fn installed_exe() -> Option<ddexe::ExeFile> {
    let path = std::env::var("DDDA_EXE").unwrap_or_else(|_| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    ddexe::ExeFile::open(path).ok()
}

/// A session with a window and a D3D9 device (windowed 1280x720, auto
/// depth D24S8): (session, device, window).
fn device_session() -> Option<(Session, u32, u32)> {
    let exe = installed_exe()?;
    let path = exe.path.to_string_lossy().to_string();
    let mut s = Session::new(&path).expect("session");
    let token = |s: &Session, n: &str| s.imports.iter().find(|i| i.full_name() == n).unwrap().token;
    let (wc, name) = (SCRATCH, SCRATCH + 0x100);
    s.mem.set_u16(name, b'W' as u16);
    s.mem.set_u32(wc + 8, token(&s, "USER32.dll!DefWindowProcW"));
    s.mem.set_u32(wc + 40, name);
    let Outcome::Returned(_) = s.call(token(&s, "USER32.dll!RegisterClassExW"), 0, &[wc]) else { panic!() };
    let Outcome::Returned(cpu) = s.call(token(&s, "USER32.dll!CreateWindowExW"), 0, &[0, name, 0, 0x00CA_0000, 0, 0, 1286, 749, 0, 0, 0x0040_0000, 0]) else { panic!() };
    let hwnd = cpu.eax;
    let Outcome::Returned(cpu) = s.call(token(&s, "d3d9.dll!Direct3DCreate9"), 0, &[32]) else { panic!() };
    let d3d = cpu.eax;
    write_pp(&mut s, [0, 0, 0, 0, 0, 0, 1, hwnd, 1, 1, 75, 0, 0, 0]);
    let create = s.mem.u32(s.mem.u32(d3d) + 4 * 16);
    let Outcome::Returned(_) = s.call(create, 0, &[d3d, 0, 1, hwnd, 0x46, PP, OUT]) else { panic!() };
    let dev = s.mem.u32(OUT);
    Some((s, dev, hwnd))
}

fn write_pp(s: &mut Session, pp: [u32; 14]) {
    for (k, v) in pp.into_iter().enumerate() {
        s.mem.set_u32(PP + 4 * k as u32, v);
    }
}

/// Call COM method `slot` of `obj` with `args` (this first); eax.
fn com(s: &mut Session, obj: u32, slot: u32, args: &[u32]) -> u32 {
    let f = s.mem.u32(s.mem.u32(obj) + 4 * slot);
    let mut a = vec![obj];
    a.extend_from_slice(args);
    match s.call(f, 0, &a) {
        Outcome::Returned(c) => c.eax,
        o => panic!("slot {slot}: {o}"),
    }
}

/// An object the device hands out through OUT.
fn out(s: &mut Session, obj: u32, slot: u32, args: &[u32]) -> u32 {
    assert_eq!(com(s, obj, slot, args), 0, "slot {slot}");
    s.mem.u32(OUT)
}


fn uploads() -> Vec<Upload> { recorded(|r| r.uploads.clone()).unwrap() }
fn draw(s: &mut Session, dev: u32) { assert_eq!(com(s, dev, 81, &[4, 0, 1]), 0); }
fn write_level(s: &mut Session, tex: u32, level: u32, value: u32) {
    assert_eq!(com(s, tex, 19, &[level, DATA, 0, 0]), 0);
    s.mem.set_u32(s.mem.u32(DATA + 4), value);
    assert_eq!(com(s, tex, 20, &[level]), 0);
}

#[test]
fn dirty_mips_surface_aliases_copies_and_relocks_keep_upload_order() {
    let Some((mut s, dev, _)) = device_session() else { return };
    let tex = out(&mut s, dev, 23, &[4, 4, 2, 0, 21, 1, OUT, 0]);
    let a = out(&mut s, tex, 18, &[0, OUT]);
    let b = out(&mut s, tex, 18, &[1, OUT]);
    let dst = out(&mut s, dev, 36, &[4, 4, 21, 2, OUT, 0]);
    assert_eq!(com(&mut s, dev, 65, &[0, tex]), 0);
    assert_eq!(com(&mut s, dev, 41, &[]), 0);
    write_level(&mut s, tex, 1, 0x11223344);
    write_level(&mut s, tex, 0, 0x55667788);
    // Restore an already-dirty Image during a second LockRect.
    assert_eq!(com(&mut s, tex, 19, &[0, DATA, 0, 0]), 0);
    draw(&mut s, dev);
    let first = uploads();
    assert!(matches!(&first[0], Upload::Image { resource, index: 0, bytes, .. }
        if *resource == tex && bytes[..4] == 0x55667788u32.to_le_bytes()));
    assert!(matches!(&first[1], Upload::Image { resource, index: 1, bytes, .. }
        if *resource == tex && bytes[..4] == 0x11223344u32.to_le_bytes()));
    draw(&mut s, dev);
    assert_eq!(uploads(), first);
    assert_eq!(com(&mut s, tex, 20, &[0]), 0);
    // StretchRect flushes the surface itself, not its owning texture.
    assert_eq!(com(&mut s, dev, 34, &[a, 0, dst, 0, 0]), 0);
    assert!(matches!(uploads().last(), Some(Upload::Image { resource, index: 0, .. }) if *resource == a));
    let n = uploads().len();
    draw(&mut s, dev);
    assert_eq!(uploads().len(), n);
    // Updating a child surface marks its parent for a subsequent draw.
    assert_eq!(com(&mut s, dev, 30, &[dst, 0, a, 0]), 0);
    write_level(&mut s, tex, 1, 0x99aabbcc);
    draw(&mut s, dev);
    let u = uploads();
    assert!(matches!(&u[n], Upload::Image { resource, index: 0, .. } if *resource == tex));
    assert!(matches!(&u[n+1], Upload::Image { resource, index: 1, .. } if *resource == tex));
    // Direct surface UnlockRect also marks the parent.
    assert_eq!(com(&mut s, b, 13, &[DATA, 0, 0]), 0);
    assert_eq!(com(&mut s, b, 14, &[]), 0);
    draw(&mut s, dev);
    assert!(matches!(uploads().last(), Some(Upload::Image { resource, index: 1, .. }) if *resource == tex));
    let n = uploads().len();
    draw(&mut s, dev);
    assert_eq!(uploads().len(), n);
}

#[test]
fn all_42_reads_duplicate_bindings_buffers_cube_and_volume() {
    let Some((mut s, dev, _)) = device_session() else { return };
    let tex = out(&mut s, dev, 23, &[4, 4, 1, 0, 21, 1, OUT, 0]);
    write_level(&mut s, tex, 0, 7);
    let vb = out(&mut s, dev, 26, &[48, 8, 0, 1, OUT, 0]);
    let ib = out(&mut s, dev, 27, &[6, 8, 101, 1, OUT, 0]);
    for buf in [vb, ib] {
        assert_eq!(com(&mut s, buf, 11, &[0, 0, OUT, 0]), 0);
        assert_eq!(com(&mut s, buf, 12, &[]), 0);
        // Taking/restoring an already-dirty buffer must keep it indexed.
        assert_eq!(com(&mut s, buf, 11, &[0, 0, OUT, 0]), 0);
    }
    for sampler in (0..16).chain(256..261) {
        assert_eq!(com(&mut s, dev, 65, &[sampler, tex]), 0);
    }
    for stream in 0..16 { assert_eq!(com(&mut s, dev, 100, &[stream, vb, 0, 16]), 0); }
    assert_eq!(com(&mut s, dev, 104, &[ib]), 0);
    let rt = out(&mut s, dev, 28, &[4, 4, 21, 0, 0, 1, OUT, 0]);
    assert_eq!(com(&mut s, rt, 13, &[DATA, 0, 0]), 0);
    assert_eq!(com(&mut s, rt, 14, &[]), 0);
    for slot in 0..4 { assert_eq!(com(&mut s, dev, 37, &[slot, rt]), 0); }
    assert_eq!(com(&mut s, dev, 41, &[]), 0);
    assert_eq!(com(&mut s, dev, 82, &[4, 0, 0, 3, 0, 1]), 0);
    assert_eq!(uploads().len(), 4, "duplicates upload only once");
    assert_eq!(com(&mut s, dev, 82, &[4, 0, 0, 3, 0, 1]), 0);
    assert_eq!(uploads().len(), 4);
    for buf in [vb, ib] { assert_eq!(com(&mut s, buf, 12, &[]), 0); }
    assert_eq!(com(&mut s, dev, 82, &[4, 0, 0, 3, 0, 1]), 0);
    assert_eq!(uploads().len(), 6);

    let cube = out(&mut s, dev, 25, &[4, 2, 0, 21, 1, OUT, 0]);
    assert_eq!(com(&mut s, cube, 19, &[5, 1, DATA, 0, 0]), 0);
    assert_eq!(com(&mut s, cube, 20, &[5, 1]), 0);
    assert_eq!(com(&mut s, dev, 65, &[0, cube]), 0);
    draw(&mut s, dev);
    assert!(matches!(uploads().last(), Some(Upload::Image { resource, index: 11, .. }) if *resource == cube));
    let vol = out(&mut s, dev, 24, &[4, 4, 4, 2, 0, 21, 1, OUT, 0]);
    assert_eq!(com(&mut s, vol, 19, &[1, DATA, 0, 0]), 0);
    assert_eq!(com(&mut s, vol, 20, &[1]), 0);
    assert_eq!(com(&mut s, vol, 19, &[1, DATA, 0, 0]), 0);
    assert_eq!(com(&mut s, dev, 65, &[0, vol]), 0);
    draw(&mut s, dev);
    assert!(matches!(uploads().last(), Some(Upload::Image { resource, index: 1, depth: 2, .. }) if *resource == vol));
    let n = uploads().len();
    draw(&mut s, dev);
    assert_eq!(uploads().len(), n);
    assert_eq!(com(&mut s, vol, 20, &[1]), 0);
    draw(&mut s, dev);
    assert_eq!(uploads().len(), n + 1);
}
