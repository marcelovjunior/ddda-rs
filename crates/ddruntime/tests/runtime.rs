//! Stage R03 tests (docs/RUNTIME.md): guest memory semantics, image
//! loading, and the dispatcher (reentrancy, conflicts, unbound targets).
use ddnative_base::mem::Mem;
use ddnative_base::x86::{self, Cpu};
use ddruntime::dispatch::{Binding, Registry, Unbound};
use ddruntime::memory::{GuestMemory, PAGE, READ, WRITE};
use std::sync::Arc as Rc;

fn fault_message(f: impl FnOnce() + std::panic::UnwindSafe) -> String {
    let e = std::panic::catch_unwind(f).expect_err("expected a guest fault");
    e.downcast_ref::<String>().cloned().unwrap_or_default()
}

#[test]
fn unmapped_and_readonly_accesses_fault() {
    let mut m = GuestMemory::new();
    m.map(0x10000, 0x1000, READ).unwrap();
    assert_eq!(m.u32(0x10000), 0, "fresh pages are zero");
    let msg = fault_message(std::panic::AssertUnwindSafe(|| { m.u32(0x20000); }));
    assert!(msg.contains("read of 4 byte(s) at 0x00020000: unmapped page"), "{msg}");
    let msg = fault_message(std::panic::AssertUnwindSafe(|| m.set_u8(0x10010, 1)));
    assert!(msg.contains("not writable"), "{msg}");
    // the null page is never mapped
    let msg = fault_message(std::panic::AssertUnwindSafe(|| { m.u8(0); }));
    assert!(msg.contains("0x00000000"), "{msg}");
}

#[test]
fn unaligned_page_crossing_and_wrapping_accesses_are_byte_exact() {
    let mut m = GuestMemory::new();
    m.map(0x20000, 2 * PAGE, READ | WRITE).unwrap();
    m.set_u32(0x20FFE, 0xA1B2_C3D4); // crosses into the second page
    assert_eq!(m.u8(0x20FFE), 0xD4);
    assert_eq!(m.u8(0x21001), 0xA1);
    assert_eq!(m.u32(0x20FFE), 0xA1B2_C3D4);
    assert_eq!(m.u16(0x20FFF), 0xB2C3);
    // a crossing access into an unmapped page faults as a whole, writing nothing
    let msg = fault_message(std::panic::AssertUnwindSafe(|| m.set_u32(0x21FFE, 0xFFFF_FFFF)));
    assert!(msg.contains("unmapped"), "{msg}");
    assert_eq!(m.u16(0x21FFE), 0, "a faulting store writes nothing");
    // addresses wrap at 0xFFFFFFFF like the CPU's
    m.map(0xFFFF_F000, PAGE, READ | WRITE).unwrap();
    m.map(0, PAGE, READ | WRITE).unwrap();
    m.set_u32(0xFFFF_FFFE, 0x1122_3344);
    assert_eq!(m.u16(0xFFFF_FFFE), 0x3344);
    assert_eq!(m.u16(0), 0x1122);
}

#[test]
fn mappings_cannot_overlap() {
    let mut m = GuestMemory::new();
    m.map(0x30000, 0x2000, READ).unwrap();
    assert_eq!(m.map(0x31800, 0x10, READ).unwrap_err().page, 0x31000);
}

// --- dispatcher ---------------------------------------------------------

fn ret(m: &mut dyn Mem, cpu: &mut Cpu) {
    let _ = m.u32(cpu.esp);
    cpu.esp = cpu.esp.wrapping_add(4);
}
fn push_and_call_ext(m: &mut dyn Mem, cpu: &mut Cpu, target: u32) {
    cpu.esp = cpu.esp.wrapping_sub(4);
    m.set_u32(cpu.esp, 0x0040_1234);
    x86::call_ext(m, cpu, target);
}
// lifted-style functions: 0x1000 calls 0x2000 through call_ext, which calls
// 0x3000 the same way (nested), which counts down eax recursively via 0x3000
fn f1000(m: &mut dyn Mem, cpu: &mut Cpu) {
    push_and_call_ext(m, cpu, 0x2000);
    cpu.ebx += 1;
    ret(m, cpu);
}
fn f2000(m: &mut dyn Mem, cpu: &mut Cpu) {
    push_and_call_ext(m, cpu, 0x3000);
    cpu.ebx += 10;
    ret(m, cpu);
}
fn f3000(m: &mut dyn Mem, cpu: &mut Cpu) {
    if cpu.eax > 0 {
        cpu.eax -= 1;
        push_and_call_ext(m, cpu, 0x3000);
        cpu.ebx += 100;
    }
    ret(m, cpu);
}
fn f4000(m: &mut dyn Mem, cpu: &mut Cpu) {
    push_and_call_ext(m, cpu, 0xDEAD_0000);
    ret(m, cpu);
}

fn stack() -> (GuestMemory, Cpu) {
    let mut m = GuestMemory::new();
    m.map(0x0010_0000, 0x10000, READ | WRITE).unwrap();
    let cpu = Cpu { esp: 0x0011_0000, ..Cpu::default() };
    (m, cpu)
}

#[test]
fn nested_and_recursive_ext_calls_reenter_the_dispatcher() {
    let mut r = Registry::new();
    for (a, f) in [(0x1000u32, f1000 as fn(&mut dyn Mem, &mut Cpu)), (0x2000, f2000), (0x3000, f3000)] {
        r.insert(a, Binding::Lifted(f)).unwrap();
    }
    let r = Rc::new(r);
    r.install();
    let (mut m, mut cpu) = stack();
    cpu.eax = 3;
    cpu.esp -= 4;
    m.set_u32(cpu.esp, 0);
    r.call(&mut m, &mut cpu, 0x1000).unwrap();
    assert_eq!(cpu.ebx, 1 + 10 + 3 * 100);
    assert_eq!(cpu.esp, 0x0011_0000, "every level popped its return address");
    x86::set_ext_calls(None);
}

#[test]
fn an_unbound_target_stops_with_its_address_and_caller() {
    let mut r = Registry::new();
    r.insert(0x4000, Binding::Lifted(f4000)).unwrap();
    r.name(0xDEAD_0000, "sNowhere::vf03");
    let r = Rc::new(r);
    r.install();
    let (mut m, mut cpu) = stack();
    cpu.esp -= 4;
    m.set_u32(cpu.esp, 0);
    let e = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| r.call(&mut m, &mut cpu, 0x4000)))
        .expect_err("the call must stop");
    let u = e.downcast_ref::<Unbound>().expect("an Unbound report");
    assert_eq!(u.target, 0xDEAD_0000);
    assert_eq!(u.return_to, Some(0x0040_1234));
    assert_eq!(u.name.as_deref(), Some("sNowhere::vf03"));
    assert!(u.to_string().contains("untranslated code at 0xdead0000 (sNowhere::vf03)"));
    // the handler stays installed after the panic (nothing was taken out)
    assert!(r.call(&mut m, &mut cpu, 0x5000).is_err());
    x86::set_ext_calls(None);
}

#[test]
fn one_binding_per_address() {
    let mut r = Registry::new();
    r.insert(0x1000, Binding::Lifted(f1000)).unwrap();
    let c = r.insert(0x1000, Binding::Service(f2000)).unwrap_err();
    assert_eq!((c.addr, c.existing, c.new), (0x1000, "lifted", "service"));
}

// --- the real image -----------------------------------------------------

fn installed_exe() -> Option<ddexe::ExeFile> {
    let path = std::env::var("DDDA_EXE")
        .unwrap_or_else(|_| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    ddexe::ExeFile::open(path).ok()
}

#[test]
fn the_installed_exe_maps_at_its_virtual_addresses() {
    let Some(exe) = installed_exe() else { return };
    let mut m = GuestMemory::new();
    let mapped = ddruntime::image::load(&mut m, &exe).expect("load");
    let image = exe.image().unwrap();
    // every file-backed byte of every section reads back through the guest
    for s in image.sections() {
        let len = s.virtual_size.min(s.raw_size).min(0x100);
        assert_eq!(m.read(s.va, len).unwrap(), image.bytes(s.va, len as usize).unwrap(), "{}", s.name);
    }
    let data = mapped.iter().find(|s| s.name == ".data").expect(".data");
    assert_eq!(data.perm, READ | WRITE);
    // .data's virtual tail beyond the file (BSS) is zero
    let s = image.sections().iter().find(|s| s.name == ".data").unwrap();
    assert!(s.virtual_size > s.raw_size, ".data has a zero-filled tail in this build");
    assert_eq!(m.u32(s.va + s.raw_size + 0x100), 0);
    // code is readable, not writable
    let text = image.sections().iter().find(|s| s.name == ".text").unwrap();
    assert_eq!(m.perm(text.va), READ);
}

// --- services -------------------------------------------------------------

use ddruntime::services;

fn call_service(f: fn(&mut dyn Mem, &mut Cpu), m: &mut GuestMemory, args: &[u32]) -> Cpu {
    let mut cpu = Cpu { esp: 0x0011_0000, ..Cpu::default() };
    for &a in args.iter().rev() {
        cpu.esp -= 4;
        m.set_u32(cpu.esp, a);
    }
    cpu.esp -= 4;
    m.set_u32(cpu.esp, 0x0040_5678);
    f(m, &mut cpu);
    cpu
}

#[test]
fn heap_blocks_are_aligned_distinct_zeroed_and_reused_by_size() {
    let mut h = services::Heap::default();
    let a = h.alloc(10).unwrap();
    let b = h.alloc(10).unwrap();
    assert_eq!(a % 16, 0);
    assert_ne!(a, b);
    assert_eq!(h.size_of(a), Some(16));
    h.free(a).unwrap();
    assert_eq!(h.free(a), Err(a), "double free is reported");
    assert_eq!(h.alloc(12).unwrap(), a, "a freed block of the same size is reused");
}

#[test]
fn critical_sections_count_recursion_and_release() {
    let (mut m, _) = stack();
    let cs = 0x0010_8000;
    call_service(services::initialize_critical_section, &mut m, &[cs]);
    assert_eq!(m.u32(cs + 4), u32::MAX, "LockCount starts at -1");
    call_service(services::enter_critical_section, &mut m, &[cs]);
    call_service(services::enter_critical_section, &mut m, &[cs]);
    assert_eq!((m.u32(cs + 8), m.u32(cs + 12)), (2, services::MAIN_THREAD_ID));
    call_service(services::leave_critical_section, &mut m, &[cs]);
    let cpu = call_service(services::leave_critical_section, &mut m, &[cs]);
    assert_eq!((m.u32(cs + 4), m.u32(cs + 8), m.u32(cs + 12)), (u32::MAX, 0, 0));
    assert_eq!(cpu.esp, 0x0011_0000, "stdcall: the callee pops its argument");
}

#[test]
fn every_import_gets_a_distinct_named_token() {
    let Some(exe) = installed_exe() else { return };
    let mut m = GuestMemory::new();
    ddruntime::image::load(&mut m, &exe).unwrap();
    let imports = ddruntime::image::bind_imports(&mut m, &exe).unwrap();
    assert_eq!(imports.len(), 273, "R01 counted 273 normal imports");
    let tokens: std::collections::HashSet<u32> = imports.iter().map(|i| i.token).collect();
    assert_eq!(tokens.len(), imports.len());
    for i in &imports {
        assert_eq!(m.u32(i.slot), i.token, "{}", i.full_name());
    }
    assert!(imports.iter().any(|i| i.full_name() == "KERNEL32.dll!EncodePointer"));
}

// --- files --------------------------------------------------------------

fn put_str(m: &mut GuestMemory, at: u32, s: &str) {
    for (k, b) in s.bytes().chain([0]).enumerate() {
        m.set_u8(at + k as u32, b);
    }
}

#[test]
fn files_read_from_the_install_and_write_only_to_the_overlay() {
    let root = std::env::temp_dir().join(format!("ddruntime-files-{}", std::process::id()));
    let (install, sandbox) = (root.join("install"), root.join("sandbox"));
    std::fs::create_dir_all(install.join("nativePC")).unwrap();
    std::fs::write(install.join("nativePC/a.txt"), b"original").unwrap();
    ddruntime::files::set_roots(&install, &sandbox);
    let (mut m, _) = stack();
    let (name, buf, out) = (0x0010_9000, 0x0010_A000, 0x0010_B000);
    put_str(&mut m, name, r"nativePC\a.txt");
    // read: GENERIC_READ (0x80000000), OPEN_EXISTING (3)
    let h = call_service(ddruntime::files::create_file_a, &mut m, &[name, 0x8000_0000, 1, 0, 3, 0x80, 0]).eax;
    assert_ne!(h, u32::MAX);
    let ok = call_service(ddruntime::files::read_file, &mut m, &[h, buf, 100, out, 0]).eax;
    assert_eq!((ok, m.u32(out)), (1, 8));
    assert_eq!(m.read(buf, 8).unwrap(), b"original");
    call_service(ddruntime::files::close_handle, &mut m, &[h]);
    // write to the same path: CREATE_ALWAYS (2) lands in the overlay
    let h = call_service(ddruntime::files::create_file_a, &mut m, &[name, 0x4000_0000, 0, 0, 2, 0x80, 0]).eax;
    put_str(&mut m, buf, "changed!");
    assert_eq!(call_service(ddruntime::files::write_file, &mut m, &[h, buf, 8, out, 0]).eax, 1);
    call_service(ddruntime::files::close_handle, &mut m, &[h]);
    assert_eq!(std::fs::read(install.join("nativePC/a.txt")).unwrap(), b"original", "the install is untouched");
    assert_eq!(std::fs::read(sandbox.join("overlay/nativePC/a.txt")).unwrap(), b"changed!");
    // and the guest now reads its own write
    let h = call_service(ddruntime::files::create_file_a, &mut m, &[name, 0x8000_0000, 1, 0, 3, 0x80, 0]).eax;
    call_service(ddruntime::files::read_file, &mut m, &[h, buf, 100, out, 0]);
    assert_eq!(m.read(buf, 8).unwrap(), b"changed!");
    // paths outside the roots are refused
    put_str(&mut m, name, r"C:\Windows\win.ini");
    assert_eq!(call_service(ddruntime::files::create_file_a, &mut m, &[name, 0x8000_0000, 1, 0, 3, 0x80, 0]).eax, u32::MAX);
    put_str(&mut m, name, r"..\..\escape.txt");
    assert_eq!(call_service(ddruntime::files::create_file_a, &mut m, &[name, 0x8000_0000, 1, 0, 3, 0x80, 0]).eax, u32::MAX);
    let _ = std::fs::remove_dir_all(&root);
}

// --- Direct3D 9 through the import table and a COM vtable ---------------------

#[test]
fn direct3d_create9_and_a_com_call_through_its_vtable() {
    let Some(exe) = installed_exe() else { return };
    let path = exe.path.to_string_lossy().to_string();
    let mut s = ddruntime::session::Session::new(&path).expect("session");
    let create = s.imports.iter().find(|i| i.full_name() == "d3d9.dll!Direct3DCreate9").unwrap().token;
    let ddruntime::session::Outcome::Returned(cpu) = s.call(create, 0, &[32]) else { panic!("Direct3DCreate9") };
    let d3d = cpu.eax;
    let vtable = s.mem.u32(d3d);
    let get_mode = s.mem.u32(vtable + 0x20); // slot 8, as 0x00DB1480 calls it
    let mode = 0x7EFF_0000; // a scratch spot on the session stack
    let ddruntime::session::Outcome::Returned(cpu) = s.call(get_mode, 0, &[d3d, 0, mode]) else { panic!("GetAdapterDisplayMode") };
    assert_eq!(cpu.eax, 0, "D3D_OK");
    assert_eq!([s.mem.u32(mode), s.mem.u32(mode + 4), s.mem.u32(mode + 8), s.mem.u32(mode + 12)], [1920, 1080, 60, 22]);
    assert_eq!(cpu.esp, ddruntime::session::STACK_TOP - 0x100, "stdcall: this + 2 arguments popped");
}

// --- USER32: windows, messages and guest window procedures -------------------

use ddruntime::user32;

thread_local! {
    static WNDPROC_LOG: std::cell::RefCell<Vec<(u32, u32, u32)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Push `args` and a return address and run a service, as lifted code's
/// `call` would from inside a window procedure.
fn call_from_guest(m: &mut dyn Mem, cpu: &mut Cpu, f: fn(&mut dyn Mem, &mut Cpu), args: &[u32]) -> u32 {
    for &a in args.iter().rev() {
        cpu.esp -= 4;
        m.set_u32(cpu.esp, a);
    }
    cpu.esp -= 4;
    m.set_u32(cpu.esp, 0x0040_9999);
    f(m, cpu);
    cpu.eax
}

/// A guest window procedure: logs (hwnd, msg, wparam), posts WM_QUIT on
/// WM_DESTROY like the game's (0x00DF1550 case 2), refuses nothing, and
/// leaves everything else to DefWindowProcW (a tail call: same frame).
fn test_wndproc(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (hwnd, msg, wparam) = (m.u32(cpu.esp + 4), m.u32(cpu.esp + 8), m.u32(cpu.esp + 12));
    WNDPROC_LOG.with(|l| l.borrow_mut().push((hwnd, msg, wparam)));
    if msg == user32::WM_DESTROY {
        call_from_guest(m, cpu, user32::post_quit_message, &[7]);
    }
    user32::def_window_proc(m, cpu);
}

fn desktop() -> (GuestMemory, Rc<Registry>) {
    let (mut m, _) = stack();
    m.map_lazy(services::HEAP_BASE, services::HEAP_SIZE, READ | WRITE).unwrap();
    m.map(0x0030_0000, 0x1000, READ | WRITE).unwrap(); // structures the guest passes
    let mut r = Registry::new();
    r.insert(0x0050_0000, Binding::Lifted(test_wndproc)).unwrap();
    let r = Rc::new(r);
    r.install();
    user32::reset();
    WNDPROC_LOG.with(|l| l.borrow_mut().clear());
    (m, r)
}

fn messages() -> Vec<u32> {
    WNDPROC_LOG.with(|l| l.borrow_mut().drain(..).map(|(_, msg, _)| msg).collect())
}

#[test]
fn a_window_gets_windows_creation_show_and_paint_messages_in_order() {
    let (mut m, _r) = desktop();
    // WNDCLASSEXW at 0x300000, class name L"MT" at 0x300100
    let (wc, name) = (0x0030_0000, 0x0030_0100);
    for (k, u) in "MT\0".encode_utf16().enumerate() {
        m.set_u16(name + 2 * k as u32, u);
    }
    m.set_u32(wc, 0x30);
    m.set_u32(wc + 8, 0x0050_0000);
    m.set_u32(wc + 40, name);
    let cpu = call_service(user32::register_class_ex_w, &mut m, &[wc]);
    assert_eq!((cpu.eax, cpu.esp), (0xC000, 0x0011_0000), "an atom; stdcall pops 1 argument");
    assert_eq!(call_service(user32::register_class_ex_w, &mut m, &[wc]).eax, 0, "the class exists");

    // CreateWindowExW as 0x00DF1200 calls it (style 0xCA0000, 1280x720 client)
    let cpu = call_service(user32::create_window_ex_w, &mut m, &[0, name, 0, 0x00CA_0000, 32, 32, 1286, 749, 0, 0, 0x0040_0000, 0]);
    let hwnd = cpu.eax;
    assert_ne!(hwnd, 0);
    assert_eq!(cpu.esp, 0x0011_0000, "12 arguments popped");
    assert_eq!(messages(), [0x81, 0x01], "WM_NCCREATE, WM_CREATE");

    assert_eq!(call_service(user32::show_window, &mut m, &[hwnd, 1]).eax, 0, "was hidden");
    assert_eq!(messages(), [0x18, 0x1C, 0x86, 0x06, 0x07, 0x47, 0x05, 0x03],
               "SHOWWINDOW, ACTIVATEAPP, NCACTIVATE, ACTIVATE, SETFOCUS, WINDOWPOSCHANGED -> SIZE, MOVE");
    call_service(user32::update_window, &mut m, &[hwnd]);
    assert_eq!(messages(), [0x0F], "WM_PAINT");
    call_service(user32::update_window, &mut m, &[hwnd]);
    assert_eq!(messages(), Vec::<u32>::new(), "DefWindowProc validated it");

    // client rectangle: 1286x749 minus the fixed frame (3+3) and caption (23+3)
    let rect = 0x0030_0200;
    call_service(user32::get_client_rect, &mut m, &[hwnd, rect]);
    assert_eq!([m.u32(rect), m.u32(rect + 4), m.u32(rect + 8), m.u32(rect + 12)], [0, 0, 1280, 720]);

    // the message loop: a posted WM_CLOSE goes through DefWindowProc to
    // DestroyWindow, whose WM_DESTROY posts WM_QUIT
    user32::post(user32::Msg { hwnd, message: user32::WM_CLOSE, wparam: 0, lparam: 0 });
    let msg = 0x0030_0300;
    assert_eq!(call_service(user32::peek_message, &mut m, &[msg, 0, 0, 0, 1]).eax, 1);
    assert_eq!(m.u32(msg + 4), user32::WM_CLOSE);
    call_service(user32::dispatch_message, &mut m, &[msg]);
    assert_eq!(messages(), [0x10, 0x02, 0x82], "WM_CLOSE, WM_DESTROY, WM_NCDESTROY");
    assert!(user32::window(hwnd).is_none());
    assert_eq!(call_service(user32::peek_message, &mut m, &[msg, 0, 0, 0, 1]).eax, 1);
    assert_eq!((m.u32(msg + 4), m.u32(msg + 8)), (user32::WM_QUIT, 7));
    assert_eq!(call_service(user32::peek_message, &mut m, &[msg, 0, 0, 0, 1]).eax, 0, "queue empty");
    x86::set_ext_calls(None);
}

#[test]
fn window_properties_and_class_atoms() {
    let (mut m, _r) = desktop();
    let (wc, name) = (0x0030_0000, 0x0030_0100);
    m.set_u8(name, b'A');
    m.set_u32(wc + 8, 0x0050_0000);
    m.set_u32(wc + 40, name);
    let atom = call_service(user32::register_class_ex_a, &mut m, &[wc]).eax;
    // created by atom (MAKEINTATOM)
    let hwnd = call_service(user32::create_window_ex_a, &mut m, &[0, atom, 0, 0, 0, 0, 100, 100, 0, 0, 0, 0]).eax;
    assert_ne!(hwnd, 0);
    let key = 0x0030_0200;
    for (k, b) in b"Prop\0".iter().enumerate() {
        m.set_u8(key + k as u32, *b);
    }
    assert_eq!(call_service(user32::get_prop, &mut m, &[hwnd, key]).eax, 0);
    assert_eq!(call_service(user32::set_prop, &mut m, &[hwnd, key, 0x1234]).eax, 1);
    assert_eq!(call_service(user32::get_prop, &mut m, &[hwnd, key]).eax, 0x1234);
    assert_eq!(call_service(user32::remove_prop, &mut m, &[hwnd, key]).eax, 0x1234);
    assert_eq!(call_service(user32::get_prop, &mut m, &[hwnd, key]).eax, 0);
    x86::set_ext_calls(None);
}

#[test]
fn the_adapter_answers_from_the_recorded_profile() {
    let p = ddruntime::d3d9::profile();
    // D3DCAPS9: shader model 3 (VertexShaderVersion at 196, PixelShaderVersion at 204),
    // the minimum 0x00DB1480 accepts
    let word = |o: usize| u32::from_le_bytes(p.caps[o..o + 4].try_into().unwrap());
    assert!(word(196) & 0xFFFF >= 0x300 && word(204) & 0xFFFF >= 0x300, "SM3");
    // checks the game makes in 0x00DA9EC0: A16B16G16R16F vertex texture,
    // render-target X8R8G8B8 with post-pixel-shader blending
    assert_eq!(p.format(0x10_0000, 3, 0x71), 0);
    assert_eq!(p.format(1, 1, 0x15), 0);
    assert_eq!(p.format(0x8_0000, 3, 0x15), 0);
    let (hr, quality) = p.multisample(0x15, 1, 4);
    assert_eq!(hr, 0);
    assert!(quality >= 1);
}

#[test]
fn create_device_completes_windowed_parameters_and_counts_references() {
    use ddruntime::session::Outcome;
    let Some(exe) = installed_exe() else { return };
    let path = exe.path.to_string_lossy().to_string();
    let mut s = ddruntime::session::Session::new(&path).expect("session");
    let token = |s: &ddruntime::session::Session, n: &str| s.imports.iter().find(|i| i.full_name() == n).unwrap().token;
    let scratch = 0x7EFF_0000;
    // a window class whose procedure is DefWindowProcW itself
    let (wc, name) = (scratch, scratch + 0x100);
    for (k, u) in "W\0".encode_utf16().enumerate() {
        s.mem.set_u16(name + 2 * k as u32, u);
    }
    s.mem.set_u32(wc + 8, token(&s, "USER32.dll!DefWindowProcW"));
    s.mem.set_u32(wc + 40, name);
    let Outcome::Returned(_) = s.call(token(&s, "USER32.dll!RegisterClassExW"), 0, &[wc]) else { panic!("RegisterClassExW") };
    let Outcome::Returned(cpu) = s.call(token(&s, "USER32.dll!CreateWindowExW"), 0,
        &[0, name, 0, 0x00CA_0000, 0, 0, 1286, 749, 0, 0, 0x0040_0000, 0]) else { panic!("CreateWindowExW") };
    let hwnd = cpu.eax;

    let Outcome::Returned(cpu) = s.call(token(&s, "d3d9.dll!Direct3DCreate9"), 0, &[32]) else { panic!() };
    let d3d = cpu.eax;
    let method = |s: &ddruntime::session::Session, obj: u32, slot: u32| s.mem.u32(s.mem.u32(obj) + 4 * slot);
    // windowed, size 0x0, format unknown, count 0, discard, auto depth D24S8
    let (pp, out) = (scratch + 0x200, scratch + 0x300);
    for (k, v) in [0, 0, 0, 0, 0, 0, 1, hwnd, 1, 1, 75, 0, 0, 0].into_iter().enumerate() {
        s.mem.set_u32(pp + 4 * k as u32, v);
    }
    let create = method(&s, d3d, 16);
    let Outcome::Returned(cpu) = s.call(create, 0, &[d3d, 0, 1, hwnd, 0x46, pp, out]) else { panic!("CreateDevice") };
    assert_eq!(cpu.eax, 0, "D3D_OK");
    let pp_now: Vec<u32> = (0..4).map(|k| s.mem.u32(pp + 4 * k)).collect();
    assert_eq!(pp_now, [1280, 720, 22, 1], "client size, display format, one back buffer written back");
    let device = s.mem.u32(out);
    assert_eq!(ddruntime::d3d9dev::refs(device), Some(("IDirect3DDevice9", 1)));

    let Outcome::Returned(cpu) = s.call(method(&s, device, 4), 0, &[device]) else { panic!() };
    assert_eq!(cpu.eax, ddruntime::d3d9::profile().texture_memory);
    // GetBackBuffer(0, 0, MONO) -> a surface with a reference for the caller
    let Outcome::Returned(cpu) = s.call(method(&s, device, 18), 0, &[device, 0, 0, 0, out]) else { panic!() };
    assert_eq!(cpu.eax, 0);
    let back = s.mem.u32(out);
    assert_eq!(ddruntime::d3d9dev::refs(back), Some(("IDirect3DSurface9", 3)), "swap chain + render target 0 + caller");
    let desc = scratch + 0x400;
    let Outcome::Returned(_) = s.call(method(&s, back, 12), 0, &[back, desc]) else { panic!() };
    let d: Vec<u32> = (0..8).map(|k| s.mem.u32(desc + 4 * k)).collect();
    assert_eq!(d, [22, 1, 1, 0, 0, 0, 1280, 720]);
    let Outcome::Returned(cpu) = s.call(method(&s, back, 2), 0, &[back]) else { panic!() };
    assert_eq!(cpu.eax, 2);
    // the last Release of the device frees it and everything it holds
    let Outcome::Returned(cpu) = s.call(method(&s, device, 2), 0, &[device]) else { panic!() };
    assert_eq!(cpu.eax, 0);
    assert_eq!(ddruntime::d3d9dev::refs(device), None);
    assert_eq!(ddruntime::d3d9dev::refs(back), None);
}

// --- guest threads -------------------------------------------------------------

use ddruntime::threads;

const CS: u32 = 0x0030_0000;
const COUNTER: u32 = 0x0030_0100;
const EVENT_SLOT: u32 = 0x0030_0104;

/// Add 1 to the counter `n` times, each under the critical section, with
/// a read-modify-write that spans the Leave call (where a slice may end).
fn count_under_lock(m: &mut dyn Mem, cpu: &mut Cpu, n: u32) {
    for _ in 0..n {
        call_from_guest(m, cpu, services::enter_critical_section, &[CS]);
        let v = m.u32(COUNTER);
        std::thread::sleep(std::time::Duration::from_micros(20));
        m.set_u32(COUNTER, v + 1);
        call_from_guest(m, cpu, services::leave_critical_section, &[CS]);
    }
}

/// A guest thread's start routine (stdcall, one parameter): counts, sets
/// the event whose handle is stored at EVENT_SLOT, returns its parameter.
fn worker(m: &mut dyn Mem, cpu: &mut Cpu) {
    let param = m.u32(cpu.esp + 4);
    count_under_lock(m, cpu, 300);
    let ev = m.u32(EVENT_SLOT);
    call_from_guest(m, cpu, threads::set_event, &[ev]);
    cpu.eax = param;
    let _ = m.u32(cpu.esp);
    cpu.esp += 8;
}

#[test]
fn guest_threads_share_memory_and_synchronise() {
    let mut m = GuestMemory::new();
    m.map(0x0010_0000, 0x10000, READ | WRITE).unwrap();
    m.map(0x0030_0000, 0x1000, READ | WRITE).unwrap();
    m.map_lazy(threads::STACKS_BASE, threads::STACKS_SIZE, READ | WRITE).unwrap();
    let mut r = Registry::new();
    r.insert(0x0060_0000, Binding::Lifted(worker)).unwrap();
    let r = Rc::new(r);
    r.install();
    threads::set_machine(m.share(), r.clone());
    let mut cpu = Cpu { esp: 0x0011_0000, ..Cpu::default() };
    let c = &mut cpu;

    call_from_guest(&mut m, c, services::initialize_critical_section, &[CS]);
    let ev = call_from_guest(&mut m, c, threads::create_event, &[0, 0, 0, 0]); // auto reset, not set
    m.set_u32(EVENT_SLOT, ev);
    let tid_out = 0x0030_0200;
    let h = call_from_guest(&mut m, c, threads::create_thread, &[0, 0, 0x0060_0000, 42, 4, tid_out]); // CREATE_SUSPENDED
    assert_ne!(h, 0);
    assert_ne!(m.u32(tid_out), services::MAIN_THREAD_ID);
    assert_eq!(call_from_guest(&mut m, c, threads::wait_for_single_object, &[h, 50]), threads::WAIT_TIMEOUT, "suspended");
    assert_eq!(m.u32(COUNTER), 0, "a suspended thread does not run");
    assert_eq!(call_from_guest(&mut m, c, threads::resume_thread, &[h]), 1);
    count_under_lock(&mut m, c, 300);
    assert_eq!(call_from_guest(&mut m, c, threads::wait_for_single_object, &[h, threads::INFINITE]), threads::WAIT_OBJECT_0);
    assert_eq!(m.u32(COUNTER), 600, "no lost update: the critical section excluded the other thread");
    let code = 0x0030_0204;
    assert_eq!(call_from_guest(&mut m, c, threads::get_exit_code_thread, &[h, code]), 1);
    assert_eq!(m.u32(code), 42);
    // the worker set the auto-reset event: one wait consumes it
    assert_eq!(call_from_guest(&mut m, c, threads::wait_for_single_object, &[ev, 0]), threads::WAIT_OBJECT_0);
    assert_eq!(call_from_guest(&mut m, c, threads::wait_for_single_object, &[ev, 0]), threads::WAIT_TIMEOUT);
    assert!(threads::faults().is_empty(), "{:?}", threads::faults());
    assert_eq!(cpu.esp, 0x0011_0000);
    x86::set_ext_calls(None);
}

#[test]
fn semaphores_and_mutexes_follow_their_contracts() {
    let (mut m, mut cpu) = stack();
    let c = &mut cpu;
    m.map(0x0030_0000, 0x1000, READ | WRITE).unwrap();
    let sem = call_from_guest(&mut m, c, threads::create_semaphore, &[0, 1, 2, 0]);
    assert_eq!(call_from_guest(&mut m, c, threads::wait_for_single_object, &[sem, 0]), threads::WAIT_OBJECT_0);
    assert_eq!(call_from_guest(&mut m, c, threads::wait_for_single_object, &[sem, 0]), threads::WAIT_TIMEOUT);
    let prev = 0x0030_0000;
    assert_eq!(call_from_guest(&mut m, c, threads::release_semaphore, &[sem, 2, prev]), 1);
    assert_eq!(m.u32(prev), 0);
    assert_eq!(call_from_guest(&mut m, c, threads::release_semaphore, &[sem, 1, prev]), 0, "past the maximum");
    // a mutex is recursive for its owner
    let mx = call_from_guest(&mut m, c, threads::create_mutex, &[0, 1, 0]);
    assert_eq!(call_from_guest(&mut m, c, threads::wait_for_single_object, &[mx, 0]), threads::WAIT_OBJECT_0);
    assert_eq!(call_from_guest(&mut m, c, threads::release_mutex, &[mx]), 1);
    assert_eq!(call_from_guest(&mut m, c, threads::release_mutex, &[mx]), 1);
    assert_eq!(call_from_guest(&mut m, c, threads::release_mutex, &[mx]), 0, "not owned any more");
    // wait-all on a set mix
    let a = call_from_guest(&mut m, c, threads::create_event, &[0, 1, 1, 0]);
    let handles = 0x0030_0010;
    m.set_u32(handles, a);
    m.set_u32(handles + 4, sem);
    assert_eq!(call_from_guest(&mut m, c, threads::wait_for_multiple_objects, &[2, handles, 1, 0]), threads::WAIT_OBJECT_0);
    assert_eq!(call_from_guest(&mut m, c, ddruntime::files::close_handle, &[a]), 1);
    assert_eq!(call_from_guest(&mut m, c, threads::wait_for_single_object, &[a, 0]), threads::WAIT_FAILED);
}

/// The game's completion routine 0x00D0D430: clears the OVERLAPPED's
/// hEvent field (its pending flag), stdcall with three arguments.
fn completion(m: &mut dyn Mem, cpu: &mut Cpu) {
    let ov = m.u32(cpu.esp + 12);
    m.set_u32(ov + 0x10, 0);
    let _ = m.u32(cpu.esp);
    cpu.esp += 16;
}

#[test]
fn read_file_ex_completes_at_the_next_alertable_sleep() {
    let root = std::env::temp_dir().join(format!("ddruntime-async-{}", std::process::id()));
    let (install, sandbox) = (root.join("install"), root.join("sandbox"));
    std::fs::create_dir_all(&install).unwrap();
    let content: Vec<u8> = (0..3000u32).map(|k| (k * 7) as u8).collect();
    std::fs::write(install.join("arc.bin"), &content).unwrap();
    ddruntime::files::set_roots(&install, &sandbox);
    let (mut m, mut cpu) = stack();
    let mut r = Registry::new();
    r.insert(0x0061_0000, Binding::Lifted(completion)).unwrap();
    let r = Rc::new(r);
    r.install();
    let c = &mut cpu;
    let (name, buf, ov) = (0x0010_9000, 0x0010_A000, 0x0010_C000);
    put_str(&mut m, name, "arc.bin");
    // FILE_FLAG_OVERLAPPED | FILE_FLAG_NO_BUFFERING, as an MtFile opens archives
    let h = call_from_guest(&mut m, c, ddruntime::files::create_file_a, &[name, 0x8000_0000, 1, 0, 3, 0x6000_0000, 0]);
    assert_ne!(h, u32::MAX);
    m.set_u32(ov + 8, 0x800); // Offset: the second sector
    m.set_u32(ov + 0x10, 1); // pending
    assert_eq!(call_from_guest(&mut m, c, ddruntime::files::read_file_ex, &[h, buf, 0x800, ov, 0x0061_0000]), 1);
    assert_eq!(m.u32(ov + 0x10), 1, "the completion has not run yet");
    assert_eq!(call_from_guest(&mut m, c, threads::sleep_ex, &[threads::INFINITE, 1]), threads::WAIT_IO_COMPLETION);
    assert_eq!(m.u32(ov + 0x10), 0, "delivered by the alertable sleep");
    assert_eq!(m.u32(ov + 4), 3000 - 0x800, "a read reaching the end transfers what is there");
    assert_eq!(m.read(buf, 3000 - 0x800).unwrap(), &content[0x800..]);
    // a read starting past the end fails at once
    m.set_u32(ov + 8, 0x1000);
    assert_eq!(call_from_guest(&mut m, c, ddruntime::files::read_file_ex, &[h, buf, 0x800, ov, 0x0061_0000]), 0);
    assert_eq!(call_from_guest(&mut m, c, ddruntime::win32::get_last_error, &[]), 38);
    assert_eq!(call_from_guest(&mut m, c, threads::sleep_ex, &[0, 1]), 0, "nothing queued");
    x86::set_ext_calls(None);
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(feature = "game")]
#[test]
fn the_crt_start_up_bridge_runs_the_heap_initialisation() {
    // Session::start translates 0x012EF8A7; each step reports where it stops.
    // The heap is the first step and must succeed on the installed exe.
    let Some(exe) = installed_exe() else { return };
    let path = exe.path.to_string_lossy().to_string();
    let mut s = ddruntime::session::Session::new(&path).expect("session");
    ddruntime::session::install_fault_hook();
    let started = s.start();
    let _ = std::panic::take_hook();
    assert_ne!(started.step, "__heap_init", "{}", started.outcome);
}

#[test]
fn the_device_state_machine_answers_d3d9s_defaults_and_counts_bound_references() {
    use ddruntime::session::Outcome;
    let Some(exe) = installed_exe() else { return };
    let path = exe.path.to_string_lossy().to_string();
    let mut s = ddruntime::session::Session::new(&path).expect("session");
    let token = |s: &ddruntime::session::Session, n: &str| s.imports.iter().find(|i| i.full_name() == n).unwrap().token;
    let scratch = 0x7EFF_0000;
    let (wc, name) = (scratch, scratch + 0x100);
    s.mem.set_u16(name, b'W' as u16);
    s.mem.set_u32(wc + 8, token(&s, "USER32.dll!DefWindowProcW"));
    s.mem.set_u32(wc + 40, name);
    let Outcome::Returned(_) = s.call(token(&s, "USER32.dll!RegisterClassExW"), 0, &[wc]) else { panic!() };
    let Outcome::Returned(cpu) = s.call(token(&s, "USER32.dll!CreateWindowExW"), 0, &[0, name, 0, 0x00CA_0000, 0, 0, 1286, 749, 0, 0, 0x0040_0000, 0]) else { panic!() };
    let hwnd = cpu.eax;
    let Outcome::Returned(cpu) = s.call(token(&s, "d3d9.dll!Direct3DCreate9"), 0, &[32]) else { panic!() };
    let d3d = cpu.eax;
    let method = |s: &ddruntime::session::Session, obj: u32, slot: u32| s.mem.u32(s.mem.u32(obj) + 4 * slot);
    let (pp, out) = (scratch + 0x200, scratch + 0x300);
    for (k, v) in [0, 0, 0, 0, 0, 0, 1, hwnd, 1, 1, 75, 0, 0, 0].into_iter().enumerate() {
        s.mem.set_u32(pp + 4 * k as u32, v);
    }
    let Outcome::Returned(_) = s.call(method(&s, d3d, 16), 0, &[d3d, 0, 1, hwnd, 0x46, pp, out]) else { panic!() };
    let dev = s.mem.u32(out);
    let mut call = |s: &mut ddruntime::session::Session, slot: u32, args: &[u32]| -> u32 {
        let mut a = vec![dev];
        a.extend_from_slice(args);
        let f = method(s, dev, slot);
        match s.call(f, 0, &a) { Outcome::Returned(c) => c.eax, o => panic!("slot {slot}: {o}") }
    };
    let v = scratch + 0x400;
    // defaults: CULLMODE (22) is CCW (3); ZENABLE (7) is TRUE with an auto depth buffer
    assert_eq!(call(&mut s, 58, &[22, v]), 0);
    assert_eq!(s.mem.u32(v), 3);
    call(&mut s, 58, &[7, v]);
    assert_eq!(s.mem.u32(v), 1);
    // set/get round trip; sampler 0's MINFILTER (6) default is POINT (1)
    call(&mut s, 57, &[27, 1]);
    call(&mut s, 58, &[27, v]);
    assert_eq!(s.mem.u32(v), 1);
    call(&mut s, 68, &[0, 6, v]);
    assert_eq!(s.mem.u32(v), 1);
    // a vertex shader from guest memory: ps_3_0 rejected, vs_3_0 kept verbatim
    let code = scratch + 0x500;
    let tokens = [0xFFFE_0300u32, 0x0200_001F, 0x8000_0000, 0x900F_0000, 0x0200_001F, 0x8000_0000, 0xE00F_0000,
                  0x0200_0001, 0xE00F_0000, 0x90E4_0000, 0x0000_FFFF];
    for (k, t) in tokens.iter().enumerate() {
        s.mem.set_u32(code + 4 * k as u32, *t);
    }
    assert_eq!(call(&mut s, 91, &[code, out]), 0);
    let vs = s.mem.u32(out);
    assert_eq!(call(&mut s, 106, &[code, out]), ddruntime::d3d9::D3DERR_INVALIDCALL, "not a pixel shader");
    assert_eq!(call(&mut s, 92, &[vs]), 0);
    assert_eq!(ddruntime::d3d9dev::refs(vs), Some(("IDirect3DVertexShader9", 2)), "the device holds a reference");
    let size = scratch + 0x600;
    let Outcome::Returned(_) = s.call(method(&s, vs, 4), 0, &[vs, 0, size]) else { panic!() };
    assert_eq!(s.mem.u32(size), 44);
    let copy = scratch + 0x700;
    let Outcome::Returned(_) = s.call(method(&s, vs, 4), 0, &[vs, copy, size]) else { panic!() };
    assert_eq!((0..11).map(|k| s.mem.u32(copy + 4 * k)).collect::<Vec<_>>(), tokens);
    assert_eq!(call(&mut s, 92, &[0]), 0);
    assert_eq!(ddruntime::d3d9dev::refs(vs), Some(("IDirect3DVertexShader9", 1)), "unbinding releases it");
    // constants: c255 is the last vertex register, c224 is past the pixel ones
    assert_eq!(call(&mut s, 94, &[255, v, 1]), 0);
    assert_eq!(call(&mut s, 109, &[224, v, 1]), ddruntime::d3d9::D3DERR_INVALIDCALL);
    // scenes do not nest
    assert_eq!(call(&mut s, 41, &[]), 0);
    assert_eq!(call(&mut s, 41, &[]), ddruntime::d3d9::D3DERR_INVALIDCALL);
    assert_eq!(call(&mut s, 42, &[]), 0);
}

/// A session with a window and a D3D9 device (windowed 1280x720, auto
/// depth D24S8); None without the game.
fn device_session() -> Option<(ddruntime::session::Session, u32)> {
    use ddruntime::session::Outcome;
    let exe = installed_exe()?;
    let path = exe.path.to_string_lossy().to_string();
    let mut s = ddruntime::session::Session::new(&path).expect("session");
    let token = |s: &ddruntime::session::Session, n: &str| s.imports.iter().find(|i| i.full_name() == n).unwrap().token;
    let scratch = 0x7EFF_0000;
    let (wc, name) = (scratch, scratch + 0x100);
    s.mem.set_u16(name, b'W' as u16);
    s.mem.set_u32(wc + 8, token(&s, "USER32.dll!DefWindowProcW"));
    s.mem.set_u32(wc + 40, name);
    let Outcome::Returned(_) = s.call(token(&s, "USER32.dll!RegisterClassExW"), 0, &[wc]) else { panic!() };
    let Outcome::Returned(cpu) = s.call(token(&s, "USER32.dll!CreateWindowExW"), 0, &[0, name, 0, 0x00CA_0000, 0, 0, 1286, 749, 0, 0, 0x0040_0000, 0]) else { panic!() };
    let hwnd = cpu.eax;
    let Outcome::Returned(cpu) = s.call(token(&s, "d3d9.dll!Direct3DCreate9"), 0, &[32]) else { panic!() };
    let d3d = cpu.eax;
    let (pp, out) = (scratch + 0x200, scratch + 0x300);
    for (k, v) in [0, 0, 0, 0, 0, 0, 1, hwnd, 1, 1, 75, 0, 0, 0].into_iter().enumerate() {
        s.mem.set_u32(pp + 4 * k as u32, v);
    }
    let create = s.mem.u32(s.mem.u32(d3d) + 4 * 16);
    let Outcome::Returned(_) = s.call(create, 0, &[d3d, 0, 1, hwnd, 0x46, pp, out]) else { panic!() };
    let dev = s.mem.u32(out);
    Some((s, dev))
}

/// Call COM method `slot` of `obj` with `args` (this first); eax.
fn com(s: &mut ddruntime::session::Session, obj: u32, slot: u32, args: &[u32]) -> u32 {
    let f = s.mem.u32(s.mem.u32(obj) + 4 * slot);
    let mut a = vec![obj];
    a.extend_from_slice(args);
    match s.call(f, 0, &a) {
        ddruntime::session::Outcome::Returned(c) => c.eax,
        o => panic!("slot {slot}: {o}"),
    }
}

#[test]
fn textures_and_buffers_lock_into_guest_memory_with_d3d9s_layouts() {
    let Some((mut s, dev)) = device_session() else { return };
    const INVALID: u32 = ddruntime::d3d9::D3DERR_INVALIDCALL;
    let scratch = 0x7EFF_0800;
    let (out, locked, desc) = (scratch, scratch + 0x10, scratch + 0x20);
    let dxt1 = u32::from_le_bytes(*b"DXT1");
    // CreateTexture(256, 256, levels 0 = full chain, usage 0, DXT1, MANAGED)
    assert_eq!(com(&mut s, dev, 23, &[256, 256, 0, 0, dxt1, 1, out, 0]), 0);
    let tex = s.mem.u32(out);
    assert_eq!(com(&mut s, tex, 13, &[]), 9, "256 -> 1: nine levels");
    assert_eq!(com(&mut s, tex, 17, &[3, desc]), 0);
    assert_eq!((s.mem.u32(desc), s.mem.u32(desc + 24), s.mem.u32(desc + 28)), (dxt1, 32, 32));
    // level 0: 64 blocks of 8 bytes per row; a rect at (8, 4) starts one
    // block row and two blocks in
    let rect = scratch + 0x40;
    for (k, v) in [8u32, 4, 16, 8].into_iter().enumerate() {
        s.mem.set_u32(rect + 4 * k as u32, v);
    }
    assert_eq!(com(&mut s, tex, 19, &[0, locked, 0, 0]), 0);
    let (pitch, bits) = (s.mem.u32(locked), s.mem.u32(locked + 4));
    assert_eq!(pitch, 512);
    s.mem.set_u32(bits, 0xDEAD_BEEF);
    assert_eq!(com(&mut s, tex, 19, &[0, locked, 0, 0]), INVALID, "already locked");
    assert_eq!(com(&mut s, tex, 20, &[0]), 0);
    assert_eq!(com(&mut s, tex, 19, &[0, locked, rect, 0]), 0);
    assert_eq!(s.mem.u32(locked + 4), bits + 512 + 16, "the rectangle's first block");
    assert_eq!(s.mem.u32(bits), 0xDEAD_BEEF, "the shadow keeps what the guest wrote");
    com(&mut s, tex, 20, &[0]);
    // level surfaces count on the texture
    assert_eq!(com(&mut s, tex, 18, &[1, out]), 0);
    let surf = s.mem.u32(out);
    assert_eq!(ddruntime::d3d9dev::refs(tex), Some(("IDirect3DTexture9", 2)));
    assert_eq!(com(&mut s, surf, 2, &[]), 1, "Release on the surface releases the texture");
    // a DEFAULT-pool texture without DYNAMIC cannot be locked; with it, it can
    assert_eq!(com(&mut s, dev, 23, &[64, 64, 1, 0, 21, 0, out, 0]), 0);
    let gpu_only = s.mem.u32(out);
    assert_eq!(com(&mut s, gpu_only, 19, &[0, locked, 0, 0]), INVALID);
    assert_eq!(com(&mut s, dev, 23, &[64, 64, 1, 0x200, 21, 0, out, 0]), 0);
    let dynamic = s.mem.u32(out);
    assert_eq!(com(&mut s, dynamic, 19, &[0, locked, 0, 0]), 0);
    assert_eq!(s.mem.u32(locked), 256, "64 A8R8G8B8 texels");
    // a non-lockable render target refuses LockRect
    assert_eq!(com(&mut s, dev, 28, &[128, 64, 21, 0, 0, 0, out, 0]), 0);
    let rt = s.mem.u32(out);
    assert_eq!(com(&mut s, rt, 13, &[locked, 0, 0]), INVALID);
    // vertex buffer: lock a range, unlock, describe
    assert_eq!(com(&mut s, dev, 26, &[1024, 0x208, 0, 0, out, 0]), 0);
    let vb = s.mem.u32(out);
    assert_eq!(com(&mut s, vb, 11, &[64, 32, out, 0]), 0);
    let p = s.mem.u32(out);
    assert_eq!(com(&mut s, vb, 11, &[0, 0, out, 0]), 0, "buffers may be locked again");
    assert_eq!(s.mem.u32(out) + 64, p);
    assert_eq!(com(&mut s, vb, 12, &[]), 0);
    assert_eq!(com(&mut s, vb, 12, &[]), 0);
    assert_eq!(com(&mut s, vb, 12, &[]), INVALID, "more unlocks than locks");
    assert_eq!(com(&mut s, vb, 11, &[1000, 100, out, 0]), INVALID, "past the end");
    assert_eq!(com(&mut s, vb, 13, &[desc]), 0);
    assert_eq!([s.mem.u32(desc), s.mem.u32(desc + 4), s.mem.u32(desc + 16)], [100, 6, 1024]);
    // the last Release frees the texture and its level surfaces
    assert_eq!(com(&mut s, tex, 2, &[]), 0);
    assert_eq!(ddruntime::d3d9dev::refs(surf), None);
}

#[test]
fn a_draw_reaches_the_backend_with_its_uploads_and_complete_state() {
    use ddruntime::d3d9draw::{recorded, Indices, Upload, Vertices};
    let Some((mut s, dev)) = device_session() else { return };
    const INVALID: u32 = ddruntime::d3d9::D3DERR_INVALIDCALL;
    let scratch = 0x7EFF_0900;
    let out = scratch;
    // a vertex buffer with three float4 positions, written through Lock
    assert_eq!(com(&mut s, dev, 26, &[48, 0x8, 0, 1, out, 0]), 0);
    let vb = s.mem.u32(out);
    com(&mut s, vb, 11, &[0, 0, out, 0]);
    let p = s.mem.u32(out);
    for k in 0..12u32 {
        s.mem.set_u32(p + 4 * k, (k as f32).to_bits());
    }
    com(&mut s, vb, 12, &[]);
    // a 4x4 A8R8G8B8 texture with one texel set
    assert_eq!(com(&mut s, dev, 23, &[4, 4, 1, 0, 21, 1, out, 0]), 0);
    let tex = s.mem.u32(out);
    let locked = scratch + 0x10;
    com(&mut s, tex, 19, &[0, locked, 0, 0]);
    s.mem.set_u32(s.mem.u32(locked + 4), 0xFF00_FF00);
    com(&mut s, tex, 20, &[0]);
    // declaration: float4 position; shaders: the pass-through pair
    let decl = scratch + 0x40;
    for (k, w) in [0u16, 0, 3, 0, 0x00FF, 0, 17, 0].iter().enumerate() {
        s.mem.set_u16(decl + 2 * k as u32, *w);
    }
    com(&mut s, dev, 86, &[decl, out]);
    let vd = s.mem.u32(out);
    let code = scratch + 0x80;
    let vs = [0xFFFE_0300u32, 0x0200_001F, 0x8000_0000, 0x900F_0000, 0x0200_001F, 0x8000_0000, 0xE00F_0000,
              0x0200_0001, 0xE00F_0000, 0x90E4_0000, 0x0000_FFFF];
    for (k, t) in vs.iter().enumerate() {
        s.mem.set_u32(code + 4 * k as u32, *t);
    }
    com(&mut s, dev, 91, &[code, out]);
    let vso = s.mem.u32(out);
    com(&mut s, dev, 87, &[vd]);
    com(&mut s, dev, 92, &[vso]);
    com(&mut s, dev, 100, &[0, vb, 0, 16]);
    com(&mut s, dev, 65, &[0, tex]);
    assert_eq!(com(&mut s, dev, 81, &[4, 0, 1]), INVALID, "outside a scene");
    com(&mut s, dev, 41, &[]);
    assert_eq!(com(&mut s, dev, 81, &[4, 0, 1]), 0);
    let (uploads, draw) = recorded(|r| (r.uploads.clone(), r.draws.last().cloned())).expect("recording backend");
    let draw = draw.expect("a draw");
    assert!(uploads.iter().any(|u| matches!(u, Upload::Buffer { resource, bytes } if *resource == vb && bytes[4..8] == 1f32.to_le_bytes())));
    assert!(uploads.iter().any(|u| matches!(u, Upload::Image { resource, bytes, pitch: 16, .. } if *resource == tex && bytes[0..4] == 0xFF00_FF00u32.to_le_bytes())));
    assert_eq!((draw.primitive, draw.primitives, draw.vertex_count), (4, 1, 3));
    assert_eq!(draw.vertices, Vertices::Streams(vec![(0, vb, 0, 16, 1)]));
    assert_eq!(draw.indices, Indices::None);
    assert_eq!(draw.vertex_shader.as_deref(), Some(&vs[..]));
    assert_eq!(draw.declaration.len(), 1);
    assert_eq!(draw.textures.len(), 1);
    assert_eq!((draw.textures[0].sampler, draw.textures[0].resource, draw.textures[0].format), (0, tex, 21));
    assert_eq!(draw.targets[0].as_ref().map(|t| (t.width, t.height)), Some((1280, 720)));
    // a second draw uploads nothing new (nothing is dirty)
    let before = recorded(|r| r.uploads.len()).unwrap();
    com(&mut s, dev, 81, &[4, 0, 1]);
    assert_eq!(recorded(|r| r.uploads.len()).unwrap(), before);
    // DrawPrimitiveUP carries its data and unsets stream 0
    let up = scratch + 0x100;
    assert_eq!(com(&mut s, dev, 83, &[4, 1, up, 16]), 0);
    let d = recorded(|r| r.draws.last().cloned()).unwrap().unwrap();
    assert!(matches!(d.vertices, Vertices::Inline { stride: 16, ref bytes } if bytes.len() == 48));
    com(&mut s, dev, 101, &[0, out, scratch + 0x20, scratch + 0x24]);
    assert_eq!(s.mem.u32(out), 0, "stream 0 is unset after a UP draw");
    com(&mut s, dev, 42, &[]);
    assert_eq!(com(&mut s, dev, 17, &[0, 0, 0, 0]), 0);
    assert_eq!(recorded(|r| r.presents).unwrap(), 1);
}

#[test]
fn surface_copies_and_queries_follow_d3d9() {
    use ddruntime::d3d9draw::recorded;
    let Some((mut s, dev)) = device_session() else { return };
    let scratch = 0x7EFF_0A00;
    let (out, locked) = (scratch, scratch + 0x10);
    // a system-memory surface written by the CPU, copied to a default-pool one
    assert_eq!(com(&mut s, dev, 36, &[4, 4, 21, 2, out, 0]), 0);
    let sys = s.mem.u32(out);
    assert_eq!(com(&mut s, sys, 13, &[locked, 0, 0]), 0);
    let bits = s.mem.u32(locked + 4);
    s.mem.set_u32(bits + 16 + 4, 0x1122_3344); // row 1, column 1
    com(&mut s, sys, 14, &[]);
    assert_eq!(com(&mut s, dev, 23, &[4, 4, 1, 0x200, 21, 0, out, 0]), 0);
    let tex = s.mem.u32(out);
    com(&mut s, tex, 18, &[0, out]);
    let level = s.mem.u32(out);
    let point = scratch + 0x40;
    s.mem.set_u32(point, 1);
    s.mem.set_u32(point + 4, 1);
    assert_eq!(com(&mut s, dev, 30, &[sys, 0, level, point]), 0, "UpdateSurface to (1, 1)");
    com(&mut s, tex, 19, &[0, locked, 0, 0x10]);
    let dst = s.mem.u32(locked + 4);
    assert_eq!(s.mem.u32(dst + 2 * 16 + 2 * 4), 0x1122_3344, "the texel moved by (1, 1)");
    com(&mut s, tex, 20, &[0]);
    // StretchRect and ColorFill reach the backend
    com(&mut s, dev, 38, &[0, out]);
    let back = s.mem.u32(out);
    assert_eq!(com(&mut s, dev, 34, &[level, 0, back, 0, 2]), 0);
    assert_eq!(com(&mut s, dev, 35, &[back, 0, 0xFF00_FF00]), 0);
    assert_eq!(recorded(|r| (r.copies.len(), r.fills.len())).unwrap(), (1, 1));
    // an event query completes once issued
    assert_eq!(com(&mut s, dev, 118, &[8, 0]), 0, "event queries are supported");
    assert_eq!(com(&mut s, dev, 118, &[8, out]), 0);
    let q = s.mem.u32(out);
    let data = scratch + 0x50;
    assert_eq!(com(&mut s, q, 7, &[data, 4, 1]), 1, "S_FALSE before Issue");
    assert_eq!(com(&mut s, q, 6, &[1]), 0);
    assert_eq!(com(&mut s, q, 7, &[data, 4, 1]), 0);
    assert_eq!(s.mem.u32(data), 1);
    assert_eq!(com(&mut s, dev, 118, &[99, 0]), ddruntime::d3d9::D3DERR_NOTAVAILABLE);
}

// --- L17: strings, code pages, system, registry, profile and file services ---
// Expected values are Windows 11 (build 26200) answers measured with the
// probes in research/runtime/windows.

use ddruntime::{files, profile, registry, system, text, win32};

/// A guest with the heap mapped and scratch space at 0x300000.
fn guest() -> GuestMemory {
    let (mut m, _) = stack();
    m.map_lazy(services::HEAP_BASE, services::HEAP_SIZE, READ | WRITE).unwrap();
    m.map(0x0030_0000, 0x4000, READ | WRITE).unwrap();
    m
}

fn put_w(m: &mut GuestMemory, at: u32, s: &str) {
    for (k, u) in s.encode_utf16().chain([0]).enumerate() {
        m.set_u16(at + 2 * k as u32, u);
    }
}

fn units(m: &GuestMemory, at: u32, n: u32) -> Vec<u16> {
    (0..n).map(|k| m.u16(at + 2 * k)).collect()
}

fn fill(m: &mut GuestMemory, at: u32, n: u32, b: u8) {
    for k in 0..n {
        m.set_u8(at + k, b);
    }
}

fn last_error(m: &mut GuestMemory) -> u32 {
    call_service(win32::get_last_error, m, &[]).eax
}

fn set_last_error(m: &mut GuestMemory, e: u32) {
    call_service(win32::set_last_error, m, &[e]);
}

/// The panic message of a service call that must stop the session.
fn stops(f: fn(&mut dyn Mem, &mut Cpu), m: &mut GuestMemory, args: &[u32]) -> String {
    let e = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| { call_service(f, m, args); }))
        .expect_err("the service must stop");
    e.downcast_ref::<String>().cloned().unwrap_or_default()
}

#[test]
fn multi_byte_to_wide_char_follows_windows() {
    let mut m = guest();
    let (src, dst) = (0x0030_0000, 0x0030_0100);
    m.poke(src, b"abc\0").unwrap();
    // size queries count the NUL with -1
    assert_eq!(call_service(text::multi_byte_to_wide_char, &mut m, &[1252, 0, src, 3, 0, 0]).eax, 3);
    let cpu = call_service(text::multi_byte_to_wide_char, &mut m, &[0, 1, src, u32::MAX, dst, 4]);
    assert_eq!((cpu.eax, cpu.esp), (4, 0x0011_0000), "4 units; stdcall pops 24 bytes");
    assert_eq!(units(&m, dst, 4), [0x61, 0x62, 0x63, 0]);
    // a short buffer gets the first units and ERROR_INSUFFICIENT_BUFFER
    fill(&mut m, dst, 16, 0xEE);
    assert_eq!(call_service(text::multi_byte_to_wide_char, &mut m, &[1252, 0, src, u32::MAX, dst, 2]).eax, 0);
    assert_eq!(last_error(&mut m), 122);
    assert_eq!(units(&m, dst, 3), [0x61, 0x62, 0xEEEE]);
    // code page 1252's 0x80..0x9F, including the undefined 0x81 (no error)
    m.poke(src, &[0x80, 0x81, 0x9F, 0xFF]).unwrap();
    assert_eq!(call_service(text::multi_byte_to_wide_char, &mut m, &[1252, 9, src, 4, dst, 8]).eax, 4);
    assert_eq!(units(&m, dst, 4), [0x20AC, 0x0081, 0x0178, 0x00FF]);
    // parameter and flag errors
    for (args, err) in [
        ([1252, 0, src, 0, dst, 4], 87),           // cbMultiByte 0
        ([1252, 0, 0, 3, dst, 4], 87),             // no input
        ([1252, 0, src, 3, dst, u32::MAX], 87),    // cchWideChar < 0
        ([1252, 0, src, 3, src, 4], 87),           // the same buffer
        ([1252, 3, src, 3, dst, 4], 1004),         // MB_PRECOMPOSED | MB_COMPOSITE
        ([1252, 0x10, src, 3, dst, 4], 1004),      // unknown flag
        ([65001, 0x80, src, 3, dst, 4], 1004),
    ] {
        assert_eq!(call_service(text::multi_byte_to_wide_char, &mut m, &args).eax, 0, "{args:x?}");
        assert_eq!(last_error(&mut m), err, "{args:x?}");
    }
    // UTF-8: invalid parts become U+FFFD as Windows groups them
    let utf8 = |m: &mut GuestMemory, bytes: &[u8], flags: u32| {
        m.poke(src, bytes).unwrap();
        fill(m, dst, 32, 0xEE);
        let r = call_service(text::multi_byte_to_wide_char, m, &[65001, flags, src, bytes.len() as u32, dst, 8]).eax;
        (r, units(m, dst, r.max(3)))
    };
    assert_eq!(utf8(&mut m, &[0x61, 0x80, 0x62], 0), (3, vec![0x61, 0xFFFD, 0x62]));
    assert_eq!(utf8(&mut m, &[0xF0, 0x9F, 0x98, 0x80], 0).1[..2], [0xD83D, 0xDE00]);
    assert_eq!(utf8(&mut m, &[0xC0, 0x80], 0).1[..2], [0xFFFD, 0xFFFD]);
    assert_eq!(utf8(&mut m, &[0xE2, 0x82], 0), (1, vec![0xFFFD, 0xEEEE, 0xEEEE]));
    assert_eq!(utf8(&mut m, &[0xE0, 0x80, 0x9F], 0).0, 2, "lead + out-of-range second byte is one U+FFFD");
    assert_eq!(utf8(&mut m, &[0xF4, 0x90, 0x80, 0x80], 0).0, 3);
    assert_eq!(utf8(&mut m, &[0x61, 0x62, 0x63], 3).0, 3, "UTF-8 ignores MB_PRECOMPOSED | MB_COMPOSITE");
    // MB_ERR_INVALID_CHARS: fails, but the text is written
    assert_eq!(utf8(&mut m, &[0x61, 0x80, 0x62], 8), (0, vec![0x61, 0xFFFD, 0x62]));
    assert_eq!(last_error(&mut m), 1113);
    // a short buffer splits a surrogate pair and wins over the invalid-char error
    m.poke(src, &[0x61, 0x80, 0x62]).unwrap();
    assert_eq!(call_service(text::multi_byte_to_wide_char, &mut m, &[65001, 8, src, 3, dst, 1]).eax, 0);
    assert_eq!(last_error(&mut m), 122);
    // code pages beyond 1252/UTF-8 stop
    assert!(stops(text::multi_byte_to_wide_char, &mut m, &[437, 0, src, 3, dst, 4]).contains("code page 437"));
}

#[test]
fn wide_char_to_multi_byte_follows_windows() {
    let mut m = guest();
    let (src, dst, used, def) = (0x0030_0000, 0x0030_0100, 0x0030_0200, 0x0030_0204);
    let conv = |m: &mut GuestMemory, cp: u32, flags: u32, s: &[u16], cb: u32, def_p: u32, used_p: u32| {
        for (k, &u) in s.iter().enumerate() {
            m.set_u16(src + 2 * k as u32, u);
        }
        fill(m, dst, 16, 0xEE);
        m.set_u32(used, 0x55);
        let cpu = call_service(text::wide_char_to_multi_byte, m, &[cp, flags, src, s.len() as u32, dst, cb, def_p, used_p]);
        assert_eq!(cpu.esp, 0x0011_0000, "stdcall pops 32 bytes");
        (cpu.eax, m.read(dst, 6).unwrap(), m.u32(used))
    };
    // 1252: own bytes, best fit, default character
    assert_eq!(conv(&mut m, 1252, 0, &[0x61, 0x100, 0x20AC], 8, 0, used), (3, vec![0x61, 0x41, 0x80, 0xEE, 0xEE, 0xEE], 0));
    assert_eq!(conv(&mut m, 1252, 0, &[0x61, 0x4E00], 8, 0, used).1[..2], [0x61, b'?']);
    assert_eq!(m.u32(used), 1);
    m.poke(def, b"*\0").unwrap();
    assert_eq!(conv(&mut m, 1252, 0, &[0x61, 0x4E00], 8, def, used).1[..2], [0x61, b'*']);
    assert_eq!(conv(&mut m, 1252, 0x400, &[0x100], 8, 0, used), (1, vec![b'?', 0xEE, 0xEE, 0xEE, 0xEE, 0xEE], 1), "WC_NO_BEST_FIT_CHARS");
    assert_eq!(conv(&mut m, 1252, 0, &[0xD83D, 0xDE00], 8, 0, used).1[..2], [b'?', b'?'], "a pair is two units");
    // size query: the count, and lpUsedDefaultChar is written
    assert_eq!(conv(&mut m, 1252, 0, &[0x61, 0x4E00], 0, 0, used).0, 2);
    assert_eq!(m.u32(used), 1);
    // a short buffer gets the first bytes
    assert_eq!(conv(&mut m, 1252, 0, &[0x61, 0x62, 0x63], 2, 0, used).1[..3], [0x61, 0x62, 0xEE]);
    assert_eq!(last_error(&mut m), 122);
    // -1 counts the NUL
    put_w(&mut m, src, "abc");
    assert_eq!(call_service(text::wide_char_to_multi_byte, &mut m, &[1252, 0, src, u32::MAX, 0, 0, 0, 0]).eax, 4);
    // errors
    for (cp, flags, cch, cb, err) in [(1252, 0x80, 3, 8, 1004), (1252, 0x40, 3, 8, 1004), (1252, 0x1000, 3, 8, 1004),
                                     (65001, 0x1, 3, 8, 1004), (1252, 0, 0, 8, 87), (1252, 0, 3, u32::MAX, 87)] {
        assert_eq!(call_service(text::wide_char_to_multi_byte, &mut m, &[cp, flags, src, cch, dst, cb, 0, 0]).eax, 0);
        assert_eq!(last_error(&mut m), err, "cp {cp} flags {flags:#x}");
    }
    // UTF-8
    assert_eq!(conv(&mut m, 65001, 0, &[0x61, 0x20AC], 8, 0, 0).1[..4], [0x61, 0xE2, 0x82, 0xAC]);
    assert_eq!(conv(&mut m, 65001, 0, &[0xD800, 0x61], 8, 0, 0).1[..4], [0xEF, 0xBF, 0xBD, 0x61]);
    assert_eq!(conv(&mut m, 65001, 0x80, &[0xD800, 0x61], 8, 0, 0).0, 0, "WC_ERR_INVALID_CHARS");
    assert_eq!(last_error(&mut m), 1113);
    assert_eq!(m.read(dst, 4).unwrap(), [0xEF, 0xBF, 0xBD, 0x61], "written anyway");
    assert_eq!(conv(&mut m, 65001, 0, &[0x61, 0x20AC], 2, 0, 0).1[..3], [0x61, 0xEE, 0xEE], "whole characters only");
    assert_eq!(last_error(&mut m), 122);
    assert_eq!(conv(&mut m, 65001, 0x400, &[0x61], 8, def, used), (1, vec![0x61, 0xEE, 0xEE, 0xEE, 0xEE, 0xEE], 0),
               "Windows 11 accepts 1252 flags, lpDefaultChar and lpUsedDefaultChar for UTF-8");
}

#[test]
fn string_types_case_mapping_and_sort_keys_follow_en_us_windows() {
    let mut m = guest();
    let (src, dst, src2) = (0x0030_0000, 0x0030_0100, 0x0030_0200);
    put_w(&mut m, src, "aB1");
    fill(&mut m, dst, 16, 0xEE);
    let cpu = call_service(text::get_string_type_w, &mut m, &[1, src, u32::MAX, dst]);
    assert_eq!((cpu.eax, cpu.esp), (1, 0x0011_0000));
    assert_eq!(units(&m, dst, 5), [0x0382, 0x0381, 0x0284, 0x0220, 0xEEEE], "CT_CTYPE1, the NUL included");
    assert_eq!(call_service(text::get_string_type_w, &mut m, &[3, src, 3, dst]).eax, 0);
    assert_eq!(last_error(&mut m), 1004);
    assert_eq!(call_service(text::get_string_type_w, &mut m, &[1, src, 0, dst]).eax, 0);
    assert_eq!(last_error(&mut m), 87);
    // LCMapStringW case mapping
    put_w(&mut m, src, "Abc");
    assert_eq!(call_service(text::lc_map_string_w, &mut m, &[0x409, 0x200, src, u32::MAX, 0, 0]).eax, 4);
    assert_eq!(call_service(text::lc_map_string_w, &mut m, &[0x409, 0x200, src, u32::MAX, dst, 4]).eax, 4);
    assert_eq!(units(&m, dst, 4), [0x41, 0x42, 0x43, 0]);
    fill(&mut m, dst, 16, 0xEE);
    assert_eq!(call_service(text::lc_map_string_w, &mut m, &[0x409, 0x100, src, u32::MAX, dst, 2]).eax, 0);
    assert_eq!((last_error(&mut m), m.u16(dst)), (122, 0xEEEE), "nothing written");
    put_w(&mut m, src, "\u{FF}\u{160}");
    call_service(text::lc_map_string_w, &mut m, &[0x400, 0x200, src, 2, dst, 4]);
    assert_eq!(units(&m, dst, 2), [0x178, 0x160]);
    call_service(text::lc_map_string_w, &mut m, &[0, 0x100, src, 2, dst, 4]);
    assert_eq!(units(&m, dst, 2), [0xFF, 0x161]);
    for (flags, err) in [(0, 1004), (0x500, 1004), (0x1200, 1004), (0x201, 1004)] {
        assert_eq!(call_service(text::lc_map_string_w, &mut m, &[0x409, flags, src, 2, dst, 4]).eax, 0);
        assert_eq!(last_error(&mut m), err, "{flags:#x}");
    }
    // sort keys: "A" and "ab" as Windows builds them
    put_w(&mut m, src, "A");
    assert_eq!(call_service(text::lc_map_string_w, &mut m, &[0x409, 0x400, src, u32::MAX, dst, 32]).eax, 8);
    assert_eq!(m.read(dst, 8).unwrap(), [0x0E, 0x02, 0x01, 0x01, 0x12, 0x01, 0x01, 0x00]);
    put_w(&mut m, src, "ab");
    assert_eq!(call_service(text::lc_map_string_w, &mut m, &[0x409, 0x400, src, u32::MAX, 0, 0]).eax, 9);
    assert_eq!(call_service(text::lc_map_string_w, &mut m, &[0x409, 0x400, src, u32::MAX, dst, 4]).eax, 0);
    assert_eq!(last_error(&mut m), 122);
    // CompareStringW
    let cmp = |m: &mut GuestMemory, flags: u32, a: &str, b: &str| {
        put_w(m, src, a);
        put_w(m, src2, b);
        let cpu = call_service(text::compare_string_w, m, &[0x409, flags, src, u32::MAX, src2, u32::MAX]);
        assert_eq!(cpu.esp, 0x0011_0000);
        cpu.eax
    };
    assert_eq!(cmp(&mut m, 0x1001, "abc", "ABC"), 2, "NORM_IGNORECASE | SORT_STRINGSORT (the CRT's flags)");
    assert_eq!(cmp(&mut m, 0, "abc", "ABC"), 1);
    assert_eq!(cmp(&mut m, 0x1000, "co-op", "coop"), 1);
    assert_eq!(cmp(&mut m, 0x2, "\u{E9}", "e"), 2, "NORM_IGNORENONSPACE");
    put_w(&mut m, src, "a");
    m.poke(src2, &[0x61, 0, 0, 0, 0x62, 0]).unwrap();
    assert_eq!(call_service(text::compare_string_w, &mut m, &[0x409, 0x1000, src, u32::MAX, src2, 3]).eax, 1, "NUL is ignorable");
    assert_eq!(call_service(text::compare_string_w, &mut m, &[0x409, 0x1000, src, u32::MAX, src2, 2]).eax, 2);
    assert_eq!(call_service(text::compare_string_w, &mut m, &[0x409, 0x8000_0000, src, 1, src2, 1]).eax, 0);
    assert_eq!(last_error(&mut m), 1004);
    assert_eq!(call_service(text::compare_string_w, &mut m, &[0x409, 0, 0, 1, src2, 1]).eax, 0);
    assert_eq!(last_error(&mut m), 87);
    // not modelled: word sort of a hyphen, other locales, other characters
    put_w(&mut m, src, "co-op");
    assert!(stops(text::compare_string_w, &mut m, &[0x409, 0, src, u32::MAX, src2, 1]).contains("word sort"));
    assert!(stops(text::compare_string_w, &mut m, &[0x411, 0, src, u32::MAX, src2, 1]).contains("locale 0x411"));
    put_w(&mut m, src, "\u{4E00}");
    assert!(stops(text::get_string_type_w, &mut m, &[1, src, 1, dst]).contains("U+4E00"));
}

/// Differential check against vectors recorded from the host's own
/// functions by research/runtime/windows/Vectors.cs (not committed; set
/// DDRUNTIME_WINDOWS_VECTORS to their directory to run it).
#[test]
fn conversions_and_sort_keys_match_recorded_windows_vectors() {
    let Some(dir) = std::env::var_os("DDRUNTIME_WINDOWS_VECTORS") else { return };
    let dir = std::path::PathBuf::from(dir);
    let hex = |s: &str| -> Vec<u8> { (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect() };
    let hexw = |s: &str| -> Vec<u16> {
        if s == "-" { return Vec::new(); }
        (0..s.len()).step_by(4).map(|i| u16::from_str_radix(&s[i..i + 4], 16).unwrap()).collect()
    };
    let mut checked = 0;
    for line in std::fs::read_to_string(dir.join("utf8_decode.txt")).unwrap().lines() {
        let p: Vec<&str> = line.split(' ').collect();
        let (out, invalid) = text::check::decode_utf8(&hex(p[0]));
        if p[1] == "8" && invalid {
            assert_eq!((p[2], p[3]), ("0", "1113"), "{line}");
        } else {
            assert_eq!(out, hexw(p.get(4).copied().unwrap_or("-")), "{line}");
        }
        checked += 1;
    }
    for line in std::fs::read_to_string(dir.join("wc_encode.txt")).unwrap().lines() {
        let p: Vec<&str> = line.split(' ').collect();
        if p[0] != "1252" {
            continue;
        }
        let flags: u32 = p[2].parse().unwrap();
        let mut used = false;
        let out: Vec<u8> = hexw(p[1]).iter().map(|&u| text::check::encode_1252(u, flags & 0x400 == 0).unwrap_or_else(|| { used = true; b'?' })).collect();
        assert_eq!((out, used as u32), (hex(p[5]), p[6].parse().unwrap()), "{line}");
        checked += 1;
    }
    std::panic::set_hook(Box::new(|_| {}));
    for line in std::fs::read_to_string(dir.join("sortkeys.txt")).unwrap().lines() {
        let p: Vec<&str> = line.split(' ').collect();
        if line.contains("ERR") {
            continue; // empty strings: ERROR_INVALID_PARAMETER for LCMapStringW
        }
        let flags: u32 = p[1].parse().unwrap();
        if p[0] == "K" {
            if let Some(k) = text::check::sort_key(&hexw(p[2]), flags) {
                assert_eq!(k, hex(p[3]), "{line}");
                checked += 1;
            }
        } else if let (Some(a), Some(b)) = (text::check::sort_key(&hexw(p[2]), flags), text::check::sort_key(&hexw(p[3]), flags)) {
            assert_eq!(a.cmp(&b) as i32 + 2, p[4].parse::<i32>().unwrap(), "{line}");
            checked += 1;
        }
    }
    let _ = std::panic::take_hook();
    assert!(checked > 40_000, "{checked}");
}

#[test]
fn time_zone_and_file_times_follow_windows() {
    let mut m = guest();
    let (tzi, ft, st) = (0x0030_0000, 0x0030_0200, 0x0030_0300);
    fill(&mut m, tzi, 172, 0xEE);
    let cpu = call_service(system::get_time_zone_information, &mut m, &[tzi]);
    assert_eq!((cpu.eax, cpu.esp), (0, 0x0011_0000), "TIME_ZONE_ID_UNKNOWN");
    let name = |m: &GuestMemory, at: u32| String::from_utf16(&units(m, at, 26)).unwrap();
    assert_eq!((m.u32(tzi), m.u32(tzi + 84), m.u32(tzi + 168)), (0, 0, 0), "biases");
    assert_eq!((name(&m, tzi + 4), name(&m, tzi + 88)), ("Coordinated Universal Time".into(), "Coordinated Universal Time".into()));
    assert_eq!(m.read(tzi + 68, 16).unwrap(), [0; 16], "no transition date");
    for (t, want) in [
        (0u64, [1601, 1, 1, 1, 0, 0, 0, 0]),
        (0x7FFF_FFFF_FFFF_FFFF, [30828, 9, 4, 14, 2, 48, 5, 477]),
        (0x01CD_DA4F_ACCD_0000, [2012, 12, 5, 14, 23, 6, 40, 0]),
        (0x0123_4567_89AB_CDEF, [1860, 10, 6, 20, 15, 48, 41, 648]),
        (0x0058_D15E_1762_8000, [1680, 3, 5, 22, 4, 26, 40, 0]),
    ] {
        m.set_u32(ft, t as u32);
        m.set_u32(ft + 4, (t >> 32) as u32);
        assert_eq!(call_service(system::file_time_to_system_time, &mut m, &[ft, st]).eax, 1);
        assert_eq!(units(&m, st, 8), want, "{t:#x}");
    }
    m.set_u32(ft + 4, 0x8000_0000);
    fill(&mut m, st, 16, 0xEE);
    assert_eq!(call_service(system::file_time_to_system_time, &mut m, &[ft, st]).eax, 0);
    assert_eq!((last_error(&mut m), m.u16(st)), (87, 0xEEEE));
    // UTC: local file time is the same
    m.set_u32(ft, 0x1234_5678);
    m.set_u32(ft + 4, 0x01D0_0000);
    let cpu = call_service(system::file_time_to_local_file_time, &mut m, &[ft, st]);
    assert_eq!((cpu.eax, cpu.esp, m.u32(st), m.u32(st + 4)), (1, 0x0011_0000, 0x1234_5678, 0x01D0_0000));
}

#[test]
fn global_and_local_memory_follow_windows() {
    let mut m = guest();
    // GHND: a handle, locked to a zeroed block
    let h = call_service(system::global_alloc, &mut m, &[0x42, 10]).eax;
    assert!(h != 0 && m.perm(h) == 0, "a moveable handle is not a pointer");
    let p = call_service(system::global_lock, &mut m, &[h]).eax;
    assert!(p >= services::HEAP_BASE);
    assert_eq!(m.read(p, 10).unwrap(), [0; 10]);
    assert_eq!(call_service(system::global_lock, &mut m, &[h]).eax, p);
    set_last_error(&mut m, 77);
    assert_eq!(call_service(system::global_unlock, &mut m, &[h]).eax, 1, "still locked");
    assert_eq!(last_error(&mut m), 77, "untouched");
    assert_eq!(call_service(system::global_unlock, &mut m, &[h]).eax, 0);
    assert_eq!(last_error(&mut m), 0, "unlocked: NO_ERROR");
    assert_eq!(call_service(system::global_unlock, &mut m, &[h]).eax, 0);
    assert_eq!(last_error(&mut m), 158, "ERROR_NOT_LOCKED");
    let cpu = call_service(system::global_free, &mut m, &[h]);
    assert_eq!((cpu.eax, cpu.esp), (0, 0x0011_0000));
    assert_eq!(call_service(system::global_free, &mut m, &[h]).eax, h, "a second free fails");
    assert_eq!(last_error(&mut m), 6);
    assert_eq!(call_service(system::global_lock, &mut m, &[h]).eax, 0);
    // GPTR: the block itself
    let f = call_service(system::global_alloc, &mut m, &[0x40, 16]).eax;
    assert_eq!(call_service(system::global_lock, &mut m, &[f]).eax, f);
    assert_eq!(call_service(system::global_unlock, &mut m, &[f]).eax, 1);
    assert_eq!(call_service(system::global_free, &mut m, &[f]).eax, 0);
    assert_eq!(call_service(system::global_free, &mut m, &[0]).eax, 0, "NULL is accepted");
    // a moveable object of 0 bytes is discarded
    let z = call_service(system::global_alloc, &mut m, &[2, 0]).eax;
    assert_eq!(call_service(system::global_lock, &mut m, &[z]).eax, 0);
    assert_eq!(last_error(&mut m), 157);
    assert_ne!(call_service(system::global_alloc, &mut m, &[0, 0]).eax, 0, "a fixed 0-byte block exists");
    // flags: GlobalAlloc takes 0x7F72, LocalAlloc 0x0F72
    for (f, flags, ok) in [(system::global_alloc as fn(&mut dyn Mem, &mut Cpu), 0x8, false), (system::global_alloc, 0x80, false),
                           (system::global_alloc, 0x7F72, true), (system::local_alloc, 0x7F72, false), (system::local_alloc, 0x0F72, true)] {
        let r = call_service(f, &mut m, &[flags, 8]).eax;
        assert_eq!(r != 0, ok, "{flags:#x}");
        if !ok {
            assert_eq!(last_error(&mut m), 87);
        }
    }
    let l = call_service(system::local_alloc, &mut m, &[0x40, 16]).eax;
    assert_eq!(m.read(l, 16).unwrap(), [0; 16], "LPTR zeroes");
}

#[test]
fn the_environment_starts_empty_and_keeps_windows_order() {
    let mut m = guest();
    let block = |m: &mut GuestMemory| {
        let p = call_service(win32::get_environment_strings_w, m, &[]).eax;
        let mut v = Vec::new();
        for k in 0.. {
            let u = m.u16(p + 2 * k);
            v.push(u);
            if u == 0 && v.len() >= 2 && v[v.len() - 2] == 0 {
                break;
            }
        }
        assert_eq!(call_service(win32::free_environment_strings_w, m, &[p]).eax, 1);
        String::from_utf16(&v).unwrap()
    };
    assert_eq!(block(&mut m), "\0\0");
    let set = |m: &mut GuestMemory, name: Option<&str>, value: Option<&str>| {
        let (n, v) = (0x0030_0000, 0x0030_0100);
        if let Some(s) = name { put_str(m, n, s); }
        if let Some(s) = value { put_str(m, v, s); }
        let cpu = call_service(system::set_environment_variable_a, m, &[if name.is_some() { n } else { 0 }, if value.is_some() { v } else { 0 }]);
        assert_eq!(cpu.esp, 0x0011_0000);
        cpu.eax
    };
    assert_eq!(set(&mut m, Some("ZZ_B"), Some("2")), 1);
    assert_eq!(set(&mut m, Some("ZZ_A"), Some("1")), 1);
    assert_eq!(set(&mut m, Some("zz_c"), Some("3")), 1);
    assert_eq!(set(&mut m, Some("zz_a"), Some("8")), 1, "replaces ZZ_A, keeping its name");
    assert_eq!(set(&mut m, Some("=X"), Some("1")), 1, "a leading '=' is allowed");
    assert_eq!(block(&mut m), "=X=1\0ZZ_A=8\0ZZ_B=2\0zz_c=3\0\0");
    assert_eq!(set(&mut m, Some("ZZ_B"), None), 1);
    assert_eq!(set(&mut m, Some("NOPE"), None), 1, "deleting a missing variable succeeds");
    assert_eq!(block(&mut m), "=X=1\0ZZ_A=8\0zz_c=3\0\0");
    for name in [Some("A=B"), Some(""), None] {
        assert_eq!(set(&mut m, name, Some("1")), 0, "{name:?}");
        assert_eq!(last_error(&mut m), 87);
    }
}

#[test]
fn standard_handles_and_the_absent_console_follow_windows() {
    let mut m = guest();
    let out = 0x0030_0000;
    assert_eq!(call_service(win32::get_std_handle, &mut m, &[(-11i32) as u32]).eax, 0, "a GUI process has none");
    let cpu = call_service(system::set_std_handle, &mut m, &[(-11i32) as u32, 0x1234]);
    assert_eq!((cpu.eax, cpu.esp), (1, 0x0011_0000));
    assert_eq!(call_service(win32::get_std_handle, &mut m, &[(-11i32) as u32]).eax, 0x1234);
    assert_eq!(call_service(system::set_std_handle, &mut m, &[5, 0x1234]).eax, 0);
    assert_eq!(last_error(&mut m), 6);
    assert_eq!(call_service(win32::get_std_handle, &mut m, &[5]).eax, u32::MAX);
    assert_eq!(last_error(&mut m), 6);
    m.set_u32(out, 0x55);
    let cpu = call_service(system::get_console_mode, &mut m, &[0x1234, out]);
    assert_eq!((cpu.eax, cpu.esp, m.u32(out)), (0, 0x0011_0000, 0x55));
    assert_eq!(last_error(&mut m), 6);
    set_last_error(&mut m, 0);
    assert_eq!(call_service(system::get_console_cp, &mut m, &[]).eax, 0);
    assert_eq!(last_error(&mut m), 6);
    let cpu = call_service(system::write_console_w, &mut m, &[0x1234, out + 8, 2, out, 0]);
    assert_eq!((cpu.eax, cpu.esp, m.u32(out)), (0, 0x0011_0000, 0x55));
}

#[test]
fn version_checks_answer_as_windows_does_for_an_exe_without_supported_os() {
    let mut m = guest();
    // VerSetConditionMask: one condition per call, bit 63 marks the mask
    let mask = |m: &mut GuestMemory, prev: u64, t: u32, c: u32| {
        let cpu = call_service(system::ver_set_condition_mask, m, &[prev as u32, (prev >> 32) as u32, t, c]);
        assert_eq!(cpu.esp, 0x0011_0000, "16 bytes of arguments");
        cpu.eax as u64 | (cpu.edx as u64) << 32
    };
    assert_eq!(mask(&mut m, 0, 3, 3), 0x8000_0000_0000_0018, "MAJOR wins over MINOR");
    assert_eq!(mask(&mut m, 0, 1, 3), 0x8000_0000_0000_0003);
    assert_eq!(mask(&mut m, 0, 0, 3), 0);
    assert_eq!(mask(&mut m, 0, 3, 8), 0x8000_0000_0000_0000);
    assert_eq!(mask(&mut m, 0, 0xC0, 1), 0x8000_0000_0020_0000);
    assert_eq!(mask(&mut m, 0x8000_0000_0000_0018, 1, 3), 0x8000_0000_0000_001B);
    // the game's call (sPad 0x00DFA0C0): OSVERSIONINFOEXA 5.1, MAJOR|MINOR, >=
    let info = 0x0030_0000;
    let verify = |m: &mut GuestMemory, major: u32, minor: u32, build: u32, t: u32, mask: u64| {
        fill(m, info, 156, 0);
        for (off, v) in [(0, 156), (4, major), (8, minor), (12, build), (16, 2)] {
            m.set_u32(info + off, v);
        }
        m.set_u8(info + 154, 1);
        let cpu = call_service(system::verify_version_info_a, m, &[info, t, mask as u32, (mask >> 32) as u32]);
        assert_eq!(cpu.esp, 0x0011_0000);
        cpu.eax
    };
    assert_eq!(verify(&mut m, 5, 1, 0, 3, 0x8000_0000_0000_0018), 1);
    assert_eq!(verify(&mut m, 6, 2, 0, 3, 0x8000_0000_0000_0009), 1, "6.2 ==");
    assert_eq!(verify(&mut m, 6, 3, 0, 3, 0x8000_0000_0000_0018), 0, "not 8.1");
    assert_eq!(last_error(&mut m), 1150);
    assert_eq!(verify(&mut m, 10, 0, 0, 3, 0x8000_0000_0000_0018), 0, "not 10");
    assert_eq!(verify(&mut m, 0, 0, 9200, 4, 0x8000_0000_0000_00C0), 1, "build >= 9200");
    assert_eq!(verify(&mut m, 0, 0, 9201, 4, 0x8000_0000_0000_00C0), 0);
    assert_eq!(verify(&mut m, 0, 0, 9200, 4, 0x8000_0000_0000_0080), 0, "build > 9200");
    assert_eq!(verify(&mut m, 0, 0, 0, 8, 0x8000_0000_0000_0200), 1, "platform == 2");
    assert_eq!(verify(&mut m, 0, 0, 0, 0x80, 0x8000_0000_0020_0000), 1, "workstation");
    assert_eq!(verify(&mut m, 5, 1, 0, 0, 0x8000_0000_0000_0018), 0);
    assert_eq!(last_error(&mut m), 160, "ERROR_BAD_ARGUMENTS");
    // the hierarchy rules, replayed against the answers of a Windows 11
    // process that sees 10.0 (research/runtime/windows/Probe3.cs, Probe4.cs)
    let ten = system::OsVersion { major: 10, minor: 0, ..system::VERSION };
    let v = |major, minor| system::OsVersion { major, minor, ..system::VERSION };
    let ge = 0x8000_0000_0000_0018u64;
    for (want, info, t, mask) in [
        (true, v(5, 1), 3, ge), (true, v(10, 0), 3, ge), (false, v(10, 1), 3, ge), (false, v(11, 0), 3, ge),
        (false, v(0, 1), 1, ge), (false, v(0, 1), 1, 0x8000_0000_0000_0003), (true, v(10, 5), 2, 0x8000_0000_0000_0008),
        (true, v(10, 0), 3, 0x8000_0000_0000_0009), (true, v(6, 9), 3, 0x8000_0000_0000_001D), (false, v(10, 9), 3, 0x8000_0000_0000_001D),
        (true, v(10, 1), 3, 0x8000_0000_0000_002B), (true, v(10, 0), 2, 0x8000_0000_0000_0000), (false, v(5, 0), 2, 0x8000_0000_0000_0000),
        (false, v(11, 0), 2, 0x8000_0000_0000_0000), (false, v(5, 0), 2, 0x8000_0000_0000_0030), (true, v(10, 0), 2, 0x8000_0000_0000_0030),
        (true, v(9, 0), 2, 0x8000_0000_0000_0010), (true, v(10, 0), 2, 0x8000_0000_0000_0010), (true, v(11, 0), 2, 0x8000_0000_0000_0020),
        (true, v(10, 0), 3, 0x8000_0000_0000_0011), (true, v(10, 0), 3, 0x8000_0000_0000_000A), (true, v(10, 1), 3, 0x8000_0000_0000_000C),
        (true, v(10, 0), 3, 0x8000_0000_0000_0019), (true, v(10, 0), 3, 0x8000_0000_0000_001A),
    ] {
        assert_eq!(system::verify_version(&ten, &info, t, mask), Ok(want), "{info:?} type {t:#x} mask {mask:#x}");
    }
}

#[test]
fn debug_break_and_raise_exception_stop_with_a_report() {
    let mut m = guest();
    let msg = stops(system::debug_break, &mut m, &[]);
    assert!(msg.contains("DebugBreak called from 0x00405678"), "{msg}");
    let args = 0x0030_0000;
    for (k, v) in [0x1993_0520u32, 0x0012_3456, 0x0140_0000].into_iter().enumerate() {
        m.set_u32(args + 4 * k as u32, v);
    }
    let msg = stops(system::raise_exception, &mut m, &[0xE06D_7363, 1, 3, args]);
    assert!(msg.contains("code 0xe06d7363 (a C++ exception, msc), flags 0x1, arguments [0x19930520, 0x123456, 0x1400000]"), "{msg}");
    assert!(msg.contains("from 0x00405678"), "{msg}");
}

#[test]
fn psapi_module_name_and_wininet_state() {
    let mut m = guest();
    services::set_exe_path(r"E:\Games\DDDA\DDDA.exe");
    let buf = 0x0030_0000;
    let cpu = call_service(system::get_module_file_name_ex_a, &mut m, &[win32::PSEUDO_PROCESS, 0, buf, 260]);
    assert_eq!((cpu.eax, cpu.esp), (22, 0x0011_0000));
    assert_eq!(m.read(buf, 23).unwrap(), b"E:\\Games\\DDDA\\DDDA.exe\0");
    assert_eq!(call_service(system::get_module_file_name_ex_a, &mut m, &[win32::PSEUDO_PROCESS, win32::IMAGE_BASE, buf, 5]).eax, 4);
    assert_eq!(m.read(buf, 5).unwrap(), b"E:\\G\0");
    assert_eq!(call_service(system::get_module_file_name_ex_a, &mut m, &[0x1234, 0, buf, 260]).eax, 0);
    assert_eq!(last_error(&mut m), 6);
    m.set_u32(buf, 0x55);
    let cpu = call_service(system::internet_get_connected_state, &mut m, &[buf, 0]);
    assert_eq!((cpu.eax, cpu.esp, m.u32(buf)), (0, 0x0011_0000, 0));
}

#[test]
fn the_registry_answers_the_recorded_processor_key() {
    let mut m = guest();
    let (name, out, typ, data, cb) = (0x0030_0000, 0x0030_0100, 0x0030_0104, 0x0030_0200, 0x0030_0108);
    const HKLM: u32 = 0x8000_0002;
    let open = |m: &mut GuestMemory, root: u32, path: Option<&str>, sam: u32| {
        if let Some(p) = path { put_str(m, name, p); }
        m.set_u32(out, 0x55);
        let cpu = call_service(registry::reg_open_key_ex_a, m, &[root, if path.is_some() { name } else { 0 }, 0, sam, out]);
        assert_eq!(cpu.esp, 0x0011_0000);
        (cpu.eax, m.u32(out))
    };
    let cpu_key = r"HARDWARE\DESCRIPTION\System\CentralProcessor\0";
    let (r, key) = open(&mut m, HKLM, Some(cpu_key), 0x2_0019);
    assert_eq!(r, 0);
    assert_ne!(key, 0);
    assert_eq!(open(&mut m, HKLM, Some(&cpu_key.to_lowercase()), 0x2_0019).0, 0, "case-insensitive");
    assert_eq!(open(&mut m, HKLM, Some(r"HARDWARE\\DESCRIPTION\"), 0x2_0019).0, 0, "empty components");
    assert_eq!(open(&mut m, HKLM, Some(r"HARDWARE\Nope"), 0x2_0019), (2, 0));
    assert_eq!(open(&mut m, HKLM, Some(r"SOFTWARE\Capcom"), 0x2_0019), (2, 0));
    assert_eq!(open(&mut m, HKLM, Some(cpu_key), 0x2_0006), (5, 0), "KEY_WRITE");
    assert_eq!(open(&mut m, HKLM, Some(&format!("\\{cpu_key}")), 0x2_0019), (161, 0));
    assert_eq!(open(&mut m, 0x1234, Some("x"), 0x2_0019), (6, 0));
    assert_eq!(open(&mut m, 0x8000_0001, None, 0x2_0019).0, 0, "a root itself");
    // values
    let query = |m: &mut GuestMemory, value: Option<&str>, size: Option<u32>, with_data: bool| {
        if let Some(v) = value { put_str(m, name, v); }
        m.set_u32(typ, 0x55);
        if let Some(s) = size { m.set_u32(cb, s); }
        fill(m, data, 0x110, 0xEE);
        let cpu = call_service(registry::reg_query_value_ex_a, m, &[key, if value.is_some() { name } else { 0 }, 0, typ,
                                                                     if with_data { data } else { 0 }, if size.is_some() { cb } else { 0 }]);
        assert_eq!(cpu.esp, 0x0011_0000);
        (cpu.eax, m.u32(typ), m.u32(cb))
    };
    assert_eq!(query(&mut m, Some("~MHz"), Some(4), true), (0, 4, 4));
    assert_eq!(m.u32(data), 3000);
    assert_eq!(query(&mut m, Some("~mhz"), Some(2), true), (234, 4, 4), "ERROR_MORE_DATA");
    assert_eq!(m.u32(data), 0xEEEE_EEEE);
    assert_eq!(query(&mut m, Some("~MHz"), Some(0), false), (0, 4, 4), "size only");
    m.set_u32(cb, 0x77);
    assert_eq!(query(&mut m, Some("~MHz"), None, true).0, 87);
    assert_eq!(m.u32(typ), 0x55, "lpType untouched");
    assert_eq!(query(&mut m, Some("ProcessorNameString"), Some(0xFF), true), (0, 1, 48));
    assert_eq!(m.read(data, 48).unwrap(), b"AMD Ryzen 7 5700X3D 8-Core Processor           \0");
    assert_eq!(m.read(data + 48, 4).unwrap(), [b'r', 0, b'e', 0], "the UTF-16 text's tail, as Windows leaves it");
    assert_eq!(m.u8(data + 98), 0xEE);
    assert_eq!(query(&mut m, Some("ProcessorNameString"), Some(48), true), (0, 1, 48));
    assert_eq!(m.u8(data + 48), 0xEE, "a buffer too small for UTF-16 gets the ANSI text only");
    assert_eq!(query(&mut m, None, Some(4), true), (2, 0, 4), "no default value; lpType zeroed");
    put_str(&mut m, name, "Update Revision");
    assert!(stops(registry::reg_query_value_ex_a, &mut m, &[key, name, 0, 0, 0, 0]).contains("not modelled"));
    let cpu = call_service(registry::reg_query_value_ex_a, &mut m, &[0x1234, 0, 0, typ, 0, 0]);
    assert_eq!((cpu.eax, m.u32(typ)), (6, 0));
    // an intermediate key exists and has no values here
    let (_, mid) = open(&mut m, HKLM, Some(r"HARDWARE\DESCRIPTION"), 0x2_0019);
    put_str(&mut m, name, "x");
    assert_eq!(call_service(registry::reg_query_value_ex_a, &mut m, &[mid, name, 0, 0, 0, 0]).eax, 2);
    // a sub key of an open key
    put_str(&mut m, name, r"System\CentralProcessor\0");
    assert_eq!(call_service(registry::reg_open_key_ex_a, &mut m, &[mid, name, 0, 0x2_0019, out]).eax, 0);
    // closing
    assert_eq!(call_service(registry::reg_close_key, &mut m, &[key]).eax, 0);
    assert_eq!(call_service(registry::reg_close_key, &mut m, &[key]).eax, 6);
    assert_eq!(call_service(registry::reg_close_key, &mut m, &[HKLM]).eax, 6, "a root (measured)");
}

/// Install and sandbox roots in a fresh temporary directory.
fn file_roots(tag: &str) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("ddruntime-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (install, sandbox) = (root.join("install"), root.join("sandbox"));
    std::fs::create_dir_all(install.join("nativePC")).unwrap();
    std::fs::create_dir_all(&sandbox).unwrap();
    files::set_roots(&install, &sandbox);
    (root, install, sandbox)
}

#[test]
fn profile_files_read_and_write_as_windows_does() {
    let mut m = guest();
    let (root, _install, sandbox) = file_roots("profile");
    let docs = sandbox.join(r"csidl-0x001c\CAPCOM\DRAGONS DOGMA DARK ARISEN");
    std::fs::create_dir_all(&docs).unwrap();
    let ini = docs.join("p.ini");
    std::fs::write(&ini, b"[Display]\r\nWidth = 1280 \r\n  Name=\"quoted\"  \r\nSingle='q'\r\nEmpty=\r\nNoEq\r\n;comment=1\r\n[ other ]\r\nk=v\r\nk=second\r\n[Display]\r\nLater=x\r\n").unwrap();
    let (app_p, key_p, def_p, buf, file_p) = (0x0030_0000, 0x0030_0100, 0x0030_0200, 0x0030_0400, 0x0030_1000);
    let get = |m: &mut GuestMemory, file: &std::path::Path, app: Option<&str>, key: Option<&str>, def: Option<&str>, size: u32| {
        for (p, s) in [(app_p, app), (key_p, key), (def_p, def)] {
            if let Some(s) = s { put_w(m, p, s); }
        }
        put_w(m, file_p, &file.to_string_lossy());
        fill(m, buf, 0x100, 0xEE);
        set_last_error(m, 0x77);
        let cpu = call_service(profile::get_private_profile_string_w, m, &[
            if app.is_some() { app_p } else { 0 }, if key.is_some() { key_p } else { 0 }, if def.is_some() { def_p } else { 0 }, buf, size, file_p]);
        assert_eq!(cpu.esp, 0x0011_0000, "24 bytes");
        let shown: String = units(m, buf, cpu.eax + 2).into_iter().map(|u| match u {
            0 => "|".to_string(),
            0xEEEE => "#".to_string(),
            u => char::from_u32(u as u32).unwrap().to_string(),
        }).collect();
        (cpu.eax, last_error(m), shown)
    };
    let d = Some("d");
    for (app, key, def, size, want) in [
        (Some("Display"), Some("Width"), d, 32, (4, 0, "1280|#")),
        (Some("display"), Some("WIDTH"), d, 32, (4, 0, "1280|#")),
        (Some("Display"), Some("Name"), d, 32, (6, 0, "quoted|#")),
        (Some("Display"), Some("Single"), d, 32, (1, 0, "q|#")),
        (Some("Display"), Some("Empty"), d, 32, (0, 0, "|#")),
        (Some("Display"), Some("NoEq"), d, 32, (1, 2, "d|#")),
        (Some("Display"), Some(";comment"), d, 32, (1, 2, "d|#")),
        (Some("Display"), Some("Missing"), Some("def  "), 32, (3, 2, "def|#")),
        (Some("Display"), Some("Missing"), Some("  def"), 32, (5, 2, "  def|#")),
        (Some("Display"), Some("Missing"), None, 32, (0, 2, "|#")),
        (Some("other"), Some("k"), d, 32, (1, 0, "v|#")),
        (Some("Display"), Some("Later"), d, 32, (1, 2, "d|#")),
        (Some("Display"), Some("Width"), d, 3, (2, 234, "12|#")),
        (Some("Display"), Some("Width"), d, 1, (0, 234, "|#")),
        (Some("Display"), Some("Width"), d, 0, (0, 234, "##")),
        (Some("Display"), Some("Name"), d, 4, (3, 234, "quo|#")),
        (None, None, d, 64, (22, 0, "Display|other|Display||#")),
        (None, None, d, 10, (8, 234, "Display|||")),
        (Some("Display"), None, d, 64, (24, 0, "Width|Name|Single|Empty||#")),
        (Some("Display"), None, d, 8, (6, 234, "Width|||")),
        (Some("Nope"), None, Some("dd"), 64, (2, 2, "dd|#")),
        (Some("Display"), Some("Missing"), Some("default"), 4, (3, 2, "def|#")),
    ] {
        assert_eq!(get(&mut m, &ini, app, key, def, size), (want.0, want.1, want.2.to_string()), "{app:?} {key:?} {size}");
    }
    assert_eq!(get(&mut m, &docs.join("nope.ini"), Some("Display"), Some("Width"), Some("dflt"), 32), (4, 2, "dflt|#".into()));
    // quotes, odd section lines, line breaks, tabs, a UTF-8 BOM
    let q = docs.join("q.ini");
    std::fs::write(&q, b"[A]\r\na=\"x\r\nb=x\"\r\nc=\"x'\r\nd=\"\"\r\ne=\"\r\nf= \"x y\" \r\ng=  \r\n[B] junk\r\nh=1\r\n[C\ri=1\n k=2\r\n\tt\t=\t1\t\r\n").unwrap();
    for (app, key, want) in [("A", "a", "\"x"), ("A", "b", "x\""), ("A", "c", "\"x'"), ("A", "d", ""), ("A", "e", "\""), ("A", "f", "x y"), ("A", "g", ""),
                             ("B", "h", "1"), ("C", "i", "1"), ("C", "k", "2"), ("C", "t", "1")] {
        assert_eq!(get(&mut m, &q, Some(app), Some(key), d, 32).2, format!("{want}|#"), "{app}.{key}");
    }
    assert_eq!(get(&mut m, &q, Some("A"), None, d, 15), (13, 234, "a|b|c|d|e|f|g||".into()), "an exact fit is too small");
    std::fs::write(&q, [0xEF, 0xBB, 0xBF, b'[', b'A', b']', 13, 10, b'x', b'=', b'1']).unwrap();
    assert_eq!(get(&mut m, &q, Some("A"), Some("x"), d, 32).1, 2, "a UTF-8 BOM is part of the first line");

    // writing: the file the measured sequence produced on Windows
    let w = docs.join("w.ini");
    let put = |m: &mut GuestMemory, file: &std::path::Path, app: Option<&str>, key: Option<&str>, val: Option<&str>| {
        for (p, s) in [(app_p, app), (key_p, key), (def_p, val)] {
            if let Some(s) = s { put_w(m, p, s); }
        }
        put_w(m, file_p, &file.to_string_lossy());
        set_last_error(m, 0x77);
        let cpu = call_service(profile::write_private_profile_string_w, m, &[
            if app.is_some() { app_p } else { 0 }, if key.is_some() { key_p } else { 0 }, if val.is_some() { def_p } else { 0 }, file_p]);
        assert_eq!(cpu.esp, 0x0011_0000, "16 bytes");
        (cpu.eax, last_error(m))
    };
    let content = |p: &std::path::Path| String::from_utf8_lossy(&std::fs::read(p).unwrap()).into_owned();
    for (app, key, val, want) in [
        (Some("Sec"), Some("Key"), Some("Val"), "[Sec]\r\nKey=Val\r\n"),
        (Some("Sec"), Some("Key2"), Some("V2"), "[Sec]\r\nKey=Val\r\nKey2=V2\r\n"),
        (Some("sec"), Some("key"), Some("New Value"), "[Sec]\r\nKey=New Value\r\nKey2=V2\r\n"),
        (Some("Two"), Some("A"), Some("1"), "[Sec]\r\nKey=New Value\r\nKey2=V2\r\n[Two]\r\nA=1\r\n"),
        (Some("Two"), Some("B"), Some("  x  "), "[Sec]\r\nKey=New Value\r\nKey2=V2\r\n[Two]\r\nA=1\r\nB=  x  \r\n"),
        (Some("Sec"), Some("Key2"), None, "[Sec]\r\nKey=New Value\r\n[Two]\r\nA=1\r\nB=  x  \r\n"),
        (Some("Two"), None, None, "[Sec]\r\nKey=New Value\r\n"),
        (Some("Sec"), Some("E"), Some(""), "[Sec]\r\nKey=New Value\r\nE=\r\n"),
        (Some(" S "), Some(" k "), Some(" v "), "[Sec]\r\nKey=New Value\r\nE=\r\n[S]\r\nk= v \r\n"),
        (Some("Nope"), None, None, "[Sec]\r\nKey=New Value\r\nE=\r\n[S]\r\nk= v \r\n"),
    ] {
        assert_eq!(put(&mut m, &w, app, key, val), (1, 0x77), "{app:?} {key:?} {val:?}: TRUE, last error untouched");
        assert_eq!(content(&w), want, "{app:?} {key:?} {val:?}");
    }
    put(&mut m, &w, Some("Sec"), Some("U"), Some("\u{E9}\u{4E00}"));
    assert!(std::fs::read(&w).unwrap().ends_with(b"U=\xE9?\r\n[S]\r\nk= v \r\n"), "code page 1252, best fit/default");
    assert_eq!(get(&mut m, &w, Some("Sec"), Some("U"), d, 32).2, "\u{E9}?|#");
    // a value's surrounding text is kept; new keys go after the last key line
    let s = docs.join("s.ini");
    std::fs::write(&s, b"[A]\r\n  x = 1  \r\n\r\n;c\r\n[B]\r\ny=2").unwrap();
    put(&mut m, &s, Some("A"), Some("X"), Some("new"));
    put(&mut m, &s, Some("A"), Some("z"), Some("3"));
    put(&mut m, &s, Some("B"), Some("w"), Some("4"));
    assert_eq!(content(&s), "[A]\r\n  x =new  \r\nz=3\r\n\r\n;c\r\n[B]\r\ny=2\r\nw=4\r\n");
    let n = docs.join("n.ini");
    std::fs::write(&n, b"[A]\r\nx=1").unwrap();
    put(&mut m, &n, Some("B"), Some("y"), Some("2"));
    assert_eq!(content(&n), "[A]\r\nx=1\r\n[B]\r\ny=2\r\n");
    // a UTF-16 file stays UTF-16 (a BOM-only file gets a line break first)
    let u = docs.join("u.ini");
    std::fs::write(&u, [0xFF, 0xFE]).unwrap();
    put(&mut m, &u, Some("S"), Some("K"), Some("\u{E9}\u{4E00}"));
    let want: Vec<u8> = [0xFF, 0xFE].into_iter().chain("\r\n[S]\r\nK=\u{E9}\u{4E00}\r\n".encode_utf16().flat_map(u16::to_le_bytes)).collect();
    assert_eq!(std::fs::read(&u).unwrap(), want);
    assert_eq!(get(&mut m, &u, Some("S"), Some("K"), d, 32).2, "\u{E9}\u{4E00}|#");
    // deleting from a missing file creates it empty
    let e = docs.join("e.ini");
    assert_eq!(put(&mut m, &e, Some("A"), Some("x"), None).0, 1);
    assert_eq!(std::fs::read(&e).unwrap(), b"");
    // failures
    assert_eq!(put(&mut m, &w, None, Some("k"), Some("v")), (0, 2), "lpAppName NULL");
    assert_eq!(put(&mut m, &docs.join(r"nodir\x.ini"), Some("A"), Some("k"), Some("v")), (0, 3));
    assert_eq!(put(&mut m, std::path::Path::new(r"C:\Windows\ddruntime-test.ini"), Some("A"), Some("k"), Some("v")), (0, 5));
    assert!(!std::path::Path::new(r"C:\Windows\ddruntime-test.ini").exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn delete_move_copy_and_truncate_never_touch_the_install() {
    let mut m = guest();
    let (root, install, sandbox) = file_roots("files17");
    std::fs::write(install.join(r"nativePC\orig.arc"), b"original").unwrap();
    let (a, b, h_buf) = (0x0030_0000, 0x0030_0200, 0x0030_0400);
    let path = |m: &mut GuestMemory, at: u32, s: &str| put_str(m, at, s);
    let call1 = |m: &mut GuestMemory, f: fn(&mut dyn Mem, &mut Cpu), args: &[u32], pops: u32| {
        let cpu = call_service(f, m, args);
        assert_eq!(cpu.esp, 0x0011_0000 - 4 * args.len() as u32 + pops, "stdcall pops {pops}");
        (cpu.eax, if cpu.eax == 0 { last_error(m) } else { 0 })
    };
    // the install's file is read-only for the guest
    path(&mut m, a, r"nativePC\orig.arc");
    assert_eq!(call1(&mut m, files::delete_file_a, &[a], 4), (0, 5));
    path(&mut m, b, r"nativePC\moved.arc");
    assert_eq!(call1(&mut m, files::move_file_a, &[a, b], 8), (0, 5));
    // copying it makes an overlay file the guest can change
    assert_eq!(call1(&mut m, files::copy_file_a, &[a, b, 0], 12).0, 1);
    assert_eq!(std::fs::read(sandbox.join(r"overlay\nativePC\moved.arc")).unwrap(), b"original");
    assert_eq!(call1(&mut m, files::copy_file_a, &[a, b, 1], 12), (0, 80), "bFailIfExists");
    path(&mut m, a, r"nativePC\moved.arc");
    path(&mut m, b, r"nativePC\renamed.arc");
    assert_eq!(call1(&mut m, files::move_file_a, &[a, b], 8).0, 1);
    assert!(sandbox.join(r"overlay\nativePC\renamed.arc").exists() && !sandbox.join(r"overlay\nativePC\moved.arc").exists());
    path(&mut m, a, r"nativePC\orig.arc");
    assert_eq!(call1(&mut m, files::move_file_a, &[b, a], 8), (0, 183), "the target exists (in the install)");
    path(&mut m, a, r"nativePC\renamed.arc");
    assert_eq!(call1(&mut m, files::delete_file_a, &[a], 4).0, 1);
    assert_eq!(call1(&mut m, files::delete_file_a, &[a], 4), (0, 2));
    path(&mut m, a, r"nope\x.arc");
    assert_eq!(call1(&mut m, files::delete_file_a, &[a], 4), (0, 3));
    path(&mut m, a, r"C:\Windows\win.ini");
    assert_eq!(call1(&mut m, files::delete_file_a, &[a], 4), (0, 5));
    assert_eq!(std::fs::read(install.join(r"nativePC\orig.arc")).unwrap(), b"original", "the install is untouched");
    // SetEndOfFile at the file pointer
    path(&mut m, a, r"nativePC\t.bin");
    let h = call_service(files::create_file_a, &mut m, &[a, 0xC000_0000, 0, 0, 2, 0x80, 0]).eax;
    m.poke(h_buf, b"12345678").unwrap();
    call_service(files::write_file, &mut m, &[h, h_buf, 8, 0, 0]);
    call_service(files::set_file_pointer, &mut m, &[h, 3, 0, 0]);
    assert_eq!(call1(&mut m, files::set_end_of_file, &[h], 4).0, 1);
    call_service(files::close_handle, &mut m, &[h]);
    assert_eq!(std::fs::read(sandbox.join(r"overlay\nativePC\t.bin")).unwrap(), b"123");
    path(&mut m, a, r"nativePC\orig.arc");
    let h = call_service(files::create_file_a, &mut m, &[a, 0x8000_0000, 1, 0, 3, 0x80, 0]).eax;
    assert_eq!(call1(&mut m, files::set_end_of_file, &[h], 4), (0, 5), "opened for reading");
    assert_eq!(call1(&mut m, files::set_end_of_file, &[0x4444], 4), (0, 6));
    // the recorded volume; NULL out-parameters are skipped
    let free = 0x0030_0600;
    assert_eq!(call1(&mut m, files::get_disk_free_space_a, &[0, 0, 0, free, 0], 20).0, 1);
    assert_eq!(m.u32(free), files::VOLUME[2]);
    path(&mut m, a, &install.to_string_lossy()[..3]);
    let outs = [0x0030_0610, 0x0030_0614, 0x0030_0618, 0x0030_061C];
    assert_eq!(call1(&mut m, files::get_disk_free_space_a, &[a, outs[0], outs[1], outs[2], outs[3]], 20).0, 1);
    assert_eq!(outs.map(|p| m.u32(p)), files::VOLUME);
    // MtFile::vf05's question (overlapped mode): the file's own path fails
    m.set_u32(free, 0x55);
    path(&mut m, a, &install.join(r"nativePC\orig.arc").to_string_lossy());
    assert_eq!(call1(&mut m, files::get_disk_free_space_a, &[a, 0, 0, free, 0], 20), (0, 267), "ERROR_DIRECTORY");
    assert_eq!(m.u32(free), 0x55, "untouched");
    path(&mut m, a, &install.join(r"nativePC\nope.arc").to_string_lossy());
    assert_eq!(call1(&mut m, files::get_disk_free_space_a, &[a, 0, 0, free, 0], 20), (0, 2));
    path(&mut m, a, &install.join("nativePC").to_string_lossy());
    assert_eq!(call1(&mut m, files::get_disk_free_space_a, &[a, 0, 0, free, 0], 20).0, 1, "a directory");
    // SHCreateDirectoryExW in the home
    let home = sandbox.join(r"csidl-0x001c\CAPCOM\DRAGONS DOGMA DARK ARISEN");
    let shc = |m: &mut GuestMemory, p: &str| {
        put_w(m, a, p);
        let cpu = call_service(files::sh_create_directory_ex_w, m, &[0, a, 0]);
        assert_eq!(cpu.esp, 0x0011_0000, "12 bytes");
        cpu.eax
    };
    assert_eq!(shc(&mut m, &home.to_string_lossy()), 0);
    assert!(home.is_dir());
    assert_eq!(shc(&mut m, &home.to_string_lossy()), 183);
    std::fs::write(home.join("f"), b"x").unwrap();
    assert_eq!(shc(&mut m, &home.join("f").to_string_lossy()), 183);
    assert_eq!(shc(&mut m, &home.join(r"f\g").to_string_lossy()), 3);
    assert_eq!(shc(&mut m, r"rel\dir"), 161);
    assert_eq!(shc(&mut m, ""), 161);
    assert_eq!(shc(&mut m, &home.join(r"x\..\y").to_string_lossy()), 0);
    assert!(home.join("y").is_dir() && !home.join("x").exists());
    assert_eq!(shc(&mut m, r"C:\Windows\ddruntime-test"), 5);
    // the home is readable through the other file services too
    put_w(&mut m, a, &home.join("f").to_string_lossy());
    assert_eq!(call_service(files::get_file_attributes_w, &mut m, &[a]).eax, 0x80);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn every_l17_import_is_bound_to_a_service() {
    let Some(exe) = installed_exe() else { return };
    let mut m = GuestMemory::new();
    ddruntime::image::load(&mut m, &exe).unwrap();
    let imports = ddruntime::image::bind_imports(&mut m, &exe).unwrap();
    let mut registry = Registry::new();
    services::bind_apis(&mut registry, &imports).unwrap();
    for name in ["MultiByteToWideChar", "WideCharToMultiByte", "LCMapStringW", "CompareStringW", "GetStringTypeW",
                 "GetPrivateProfileStringW", "WritePrivateProfileStringW", "RegOpenKeyExA", "RegQueryValueExA", "RegCloseKey",
                 "DeleteFileA", "MoveFileA", "CopyFileA", "SetEndOfFile", "GetDiskFreeSpaceA", "SHCreateDirectoryExW",
                 "GetTimeZoneInformation", "FileTimeToSystemTime", "FileTimeToLocalFileTime", "GlobalAlloc", "GlobalLock",
                 "GlobalUnlock", "GlobalFree", "LocalAlloc", "SetEnvironmentVariableA", "SetStdHandle", "GetConsoleMode",
                 "GetConsoleCP", "WriteConsoleW", "VerifyVersionInfoA", "VerSetConditionMask", "DebugBreak", "RaiseException",
                 "GetModuleFileNameExA", "InternetGetConnectedState"] {
        let i = imports.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name} is not imported"));
        assert!(matches!(registry.get(i.token), Some(Binding::Service(_))), "{} has no service", i.full_name());
    }
}

// --- L21: thread information blocks -----------------------------------------

/// A start routine that records its FS base and the NT_TIB it points at
/// (ExceptionList, StackBase, StackLimit, Self) at 0x300400.
fn record_tib(m: &mut dyn Mem, cpu: &mut Cpu) {
    let fs = cpu.fs_base;
    let out = 0x0030_0400;
    for (k, v) in [fs, m.u32(fs), m.u32(fs + 4), m.u32(fs + 8), m.u32(fs + 0x18), cpu.esp].into_iter().enumerate() {
        m.set_u32(out + 4 * k as u32, v);
    }
    cpu.eax = 0;
    let _ = m.u32(cpu.esp);
    cpu.esp += 8;
}

#[test]
fn every_guest_thread_gets_a_thread_information_block() {
    let mut m = GuestMemory::new();
    m.map(0x0010_0000, 0x10000, READ | WRITE).unwrap();
    m.map(0x0030_0000, 0x1000, READ | WRITE).unwrap();
    m.map_lazy(threads::STACKS_BASE, threads::STACKS_SIZE, READ | WRITE).unwrap();
    let mut r = Registry::new();
    r.insert(0x0060_0000, Binding::Lifted(record_tib)).unwrap();
    let r = Rc::new(r);
    r.install();
    threads::set_machine(m.share(), r.clone());
    // the main thread's block, as Session::new sets it up
    let main = threads::init_teb(&mut m, services::MAIN_THREAD_ID, 0x7F00_0000, 0x7EF0_0000);
    assert_eq!(main, threads::TEBS_BASE);
    assert_eq!([m.u32(main), m.u32(main + 4), m.u32(main + 8), m.u32(main + 0x18)], [0xFFFF_FFFF, 0x7F00_0000, 0x7EF0_0000, main]);
    let mut cpu = Cpu { esp: 0x0011_0000, ..Cpu::default() };
    let tid_out = 0x0030_0200;
    let h = call_from_guest(&mut m, &mut cpu, threads::create_thread, &[0, 0, 0x0060_0000, 0, 0, tid_out]);
    assert_eq!(call_from_guest(&mut m, &mut cpu, threads::wait_for_single_object, &[h, threads::INFINITE]), threads::WAIT_OBJECT_0);
    let tid = m.u32(tid_out);
    let got: Vec<u32> = (0..6).map(|k| m.u32(0x0030_0400 + 4 * k)).collect();
    let teb = threads::teb_of(tid);
    assert_ne!(teb, main);
    assert_eq!(&got[..5], &[teb, 0xFFFF_FFFF, got[2], got[2] - 0x0010_0000, teb], "FS: ExceptionList end marker, 1 MB stack, Self");
    assert!(got[5] < got[2] && got[5] > got[3], "the thread runs on the stack its block describes");
    assert!(threads::faults().is_empty(), "{:?}", threads::faults());
    x86::set_ext_calls(None);
}

#[test]
fn code_page_queries_answer_as_windows_measured() {
    let mut m = guest();
    let info = 0x0030_0000;
    for (cp, ok, valid, max, leads) in [
        (0u32, true, false, 1u32, vec![]), (1, true, false, 1, vec![]), (2, true, false, 1, vec![]), (3, true, false, 1, vec![]),
        (437, true, true, 1, vec![]), (1252, true, true, 1, vec![]), (932, true, true, 2, vec![0x81u8, 0x9F, 0xE0, 0xFC]),
        (65000, true, true, 5, vec![]), (65001, true, true, 4, vec![]), (42, false, false, 0, vec![]), (12345, false, false, 0, vec![]),
    ] {
        fill(&mut m, info, 20, 0xEE);
        let cpu = call_service(win32::get_cp_info, &mut m, &[cp, info]);
        assert_eq!((cpu.eax, cpu.esp), (ok as u32, 0x0011_0000), "GetCPInfo({cp})");
        if ok {
            let mut want = vec![0u8; 18];
            want[..4].copy_from_slice(&max.to_le_bytes());
            want[4] = b'?';
            want[6..6 + leads.len()].copy_from_slice(&leads);
            assert_eq!(m.read(info, 20).unwrap(), [want, vec![0xEE, 0xEE]].concat(), "GetCPInfo({cp})");
        } else {
            assert_eq!((last_error(&mut m), m.u8(info)), (87, 0xEE), "GetCPInfo({cp})");
        }
        assert_eq!(call_service(win32::is_valid_code_page, &mut m, &[cp]).eax, valid as u32, "IsValidCodePage({cp})");
    }
    assert!(stops(win32::get_cp_info, &mut m, &[1250, info]).contains("not modelled"));
}

#[test]
fn module_file_names_truncate_as_windows_does() {
    let mut m = guest();
    services::set_exe_path(r"E:\Games\DDDA\DDDA.exe");
    let buf = 0x0030_0000;
    for (wide, f) in [(false, win32::get_module_file_name_a as fn(&mut dyn Mem, &mut Cpu)), (true, win32::get_module_file_name_w)] {
        let text = |m: &GuestMemory, n: u32| -> Vec<u16> { (0..n).map(|k| if wide { m.u16(buf + 2 * k) } else { m.u8(buf + k) as u16 }).collect() };
        fill(&mut m, buf, 100, 0xEE);
        set_last_error(&mut m, 0x77);
        let cpu = call_service(f, &mut m, &[0, buf, 260]);
        assert_eq!((cpu.eax, cpu.esp, last_error(&mut m)), (22, 0x0011_0000, 0), "wide {wide}");
        assert_eq!(text(&m, 23), "E:\\Games\\DDDA\\DDDA.exe\0".encode_utf16().collect::<Vec<_>>());
        fill(&mut m, buf, 100, 0xEE);
        assert_eq!(call_service(f, &mut m, &[win32::IMAGE_BASE, buf, 5]).eax, 5, "truncated: nSize");
        assert_eq!(last_error(&mut m), 122);
        assert_eq!(text(&m, 5), [b'E' as u16, b':' as u16, b'\\' as u16, b'G' as u16, 0]);
        fill(&mut m, buf, 100, 0xEE);
        assert_eq!(call_service(f, &mut m, &[0, buf, 0]).eax, 0);
        assert_eq!((last_error(&mut m), m.u8(buf)), (122, 0xEE));
        assert_eq!(call_service(f, &mut m, &[0x1234_0000, buf, 260]).eax, 0);
        assert_eq!(last_error(&mut m), 126, "not a module");
    }
}

#[test]
fn virtual_memory_reserves_commits_and_frees_as_windows_measured() {
    let mut m = guest();
    let r = Rc::new(Registry::new());
    threads::set_machine(m.share(), r);
    let va = |m: &mut GuestMemory, a: u32, n: u32, t: u32, p: u32| {
        set_last_error(m, 0x77);
        let cpu = call_service(win32::virtual_alloc, m, &[a, n, t, p]);
        assert_eq!(cpu.esp, 0x0011_0000, "16 bytes");
        (cpu.eax, last_error(m))
    };
    let vf = |m: &mut GuestMemory, a: u32, n: u32, t: u32| {
        set_last_error(m, 0x77);
        let cpu = call_service(win32::virtual_free, m, &[a, n, t]);
        assert_eq!(cpu.esp, 0x0011_0000, "12 bytes");
        (cpu.eax, last_error(m))
    };
    // the MtVirtualAllocator's 512 MB reservation: 64 KB-aligned, not accessible
    let (base, err) = va(&mut m, 0, 0x2000_0000, 0x2000, 4);
    assert_eq!(err, 0x77, "success leaves the last error");
    assert!(base != 0 && base % 0x1_0000 == 0);
    assert!(!m.is_mapped(base), "reserved pages fault");
    // commits inside it return the page address and read as zero
    assert_eq!(va(&mut m, base + 0x1_0000, 0x3_0000, 0x1000, 4), (base + 0x1_0000, 0x77));
    assert!(m.is_mapped(base + 0x1_0000) && m.is_mapped(base + 0x3_F000) && !m.is_mapped(base + 0x4_0000));
    assert_eq!(m.u32(base + 0x1_0000), 0);
    m.set_u32(base + 0x1_0000, 0x1234);
    assert_eq!(va(&mut m, base + 0x1_2345, 0x10, 0x1000, 4).0, base + 0x1_2000, "rounded down to the page");
    assert_eq!(va(&mut m, base + 0x4_1FF0, 0x20, 0x1000, 4).0, base + 0x4_1000, "two pages");
    assert!(m.is_mapped(base + 0x4_2000));
    assert_eq!(va(&mut m, base + 0x1_0000, 0x1000, 0x1000, 4).0, base + 0x1_0000, "recommit");
    assert_eq!(m.u32(base + 0x1_0000), 0x1234, "a recommitted page keeps its contents");
    // errors
    for (a, n, t, p, e) in [(base + 0x2000_0000, 0x1000, 0x1000, 4, 487), (base + 0x1FFF_F000, 0x2000, 0x1000, 4, 487),
                            (base + 0x10_0000, 0x1_0000, 0x2000, 4, 487), (base + 0x10_0000, 0x1_0000, 0x3000, 4, 487),
                            (0, 0, 0x2000, 4, 87), (0, 0x1000, 0, 4, 87), (0, 0x1000, 0x2000, 0, 87), (base + 0x20_0000, 0x1000, 0x1000, 0, 87)] {
        assert_eq!(va(&mut m, a, n, t, p), (0, e), "VirtualAlloc({a:#x}, {n:#x}, {t:#x}, {p:#x})");
    }
    // commit with NULL: a new committed region elsewhere
    let (c, _) = va(&mut m, 0, 0x5000, 0x1000, 4);
    assert!(c % 0x1_0000 == 0 && !(base..base + 0x2000_0000).contains(&c) && m.is_mapped(c + 0x4000));
    // decommit, release
    assert_eq!(vf(&mut m, base + 0x1_0000, 0x1_0000, 0x4000), (1, 0x77));
    assert!(!m.is_mapped(base + 0x1_0000) && !m.is_mapped(base + 0x1_F000) && m.is_mapped(base + 0x2_0000));
    assert_eq!(vf(&mut m, base + 0x4_0010, 0x10, 0x4000).0, 1);
    assert_eq!(vf(&mut m, base + 0x1_0000, 0x1000, 0x4000).0, 1, "decommitting decommitted pages succeeds");
    for (a, n, t, e) in [(base, 0x1000, 0x8000, 87), (base + 0x1_0000, 0, 0x8000, 487), (base, 0, 0xC000, 87), (base, 0, 0, 87), (0, 0, 0x8000, 487)] {
        assert_eq!(vf(&mut m, a, n, t), (0, e), "VirtualFree({a:#x}, {n:#x}, {t:#x})");
    }
    assert_eq!(vf(&mut m, base, 0, 0x4000).0, 1, "decommit all");
    assert!(!m.is_mapped(base + 0x2_0000) && !m.is_mapped(base + 0x4_1000));
    assert_eq!(vf(&mut m, base, 0, 0x8000), (1, 0x77));
    assert_eq!(vf(&mut m, base, 0, 0x8000), (0, 487), "released");
    assert_eq!(vf(&mut m, c, 0, 0x8000).0, 1);
    assert!(!m.is_mapped(c));
    // the freed space is reused bottom-up
    assert_eq!(va(&mut m, 0, 0x2000_0000, 0x2000, 4).0, base);
}

#[test]
fn string_from_guid2_formats_as_windows_measured() {
    let mut m = guest();
    let (g, out) = (0x0030_0000, 0x0030_0100);
    for k in 0..16u32 {
        m.set_u8(g + k, (0x10 * k + 0xA + k) as u8);
    }
    fill(&mut m, out, 100, 0xEE);
    let cpu = call_service(system::string_from_guid2, &mut m, &[g, out, 40]);
    assert_eq!((cpu.eax, cpu.esp), (39, 0x0011_0000));
    let text: Vec<u16> = units(&m, out, 39);
    assert_eq!(String::from_utf16(&text).unwrap(), "{3D2C1B0A-5F4E-8170-92A3-B4C5D6E7F809}\0");
    fill(&mut m, out, 100, 0xEE);
    assert_eq!(call_service(system::string_from_guid2, &mut m, &[g, out, 38]).eax, 0);
    assert_eq!(m.u16(out), 0xEEEE, "nothing written");
}
