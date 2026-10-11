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
