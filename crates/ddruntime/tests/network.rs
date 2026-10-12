//! Network services (brief L32): Winsock before WSAStartup and the SSPI
//! stop. The expected values are research/runtime/windows/NetProbe.cs's
//! output (part A) on Windows 11 build 26200, 32-bit process.
use ddnative_base::{mem::Mem, x86::Cpu};
use ddruntime::{
    dispatch::{Binding, Registry},
    memory::{GuestMemory, READ, WRITE},
    net, runtime, services, win32,
};
use std::sync::Arc;

const STACK: u32 = 0x1001_0000;
const DATA: u32 = 0x10000;
const SENTINEL: u32 = 0x5a5a_5a5a;

fn setup() -> GuestMemory {
    runtime::enter(Arc::new(runtime::Runtime::default()));
    let mut m = GuestMemory::new();
    m.map(DATA, 0x10000, READ | WRITE).unwrap();
    m.map(STACK - 0x10000, 0x10000, READ | WRITE).unwrap();
    let mut r = Registry::new();
    services::install(&mut m, &mut r).unwrap();
    m
}
/// Calls a stdcall service with `args`; checks the cleanup; returns EAX.
fn api(m: &mut dyn Mem, f: fn(&mut dyn Mem, &mut Cpu), args: &[u32]) -> u32 {
    let esp = STACK - 0x100;
    m.set_u32(esp, 0xdeadbeef);
    for (i, v) in args.iter().enumerate() {
        m.set_u32(esp + 4 + 4 * i as u32, *v);
    }
    let mut c = Cpu {
        esp,
        ..Cpu::default()
    };
    f(m, &mut c);
    assert_eq!(c.esp, esp + 4 + 4 * args.len() as u32, "stdcall cleanup");
    c.eax
}
fn last_error(m: &mut dyn Mem) -> (u32, u32) {
    (
        api(m, net::wsa_get_last_error, &[]),
        api(m, win32::get_last_error, &[]),
    )
}
fn fill(m: &mut dyn Mem, p: u32, n: u32, b: u8) {
    for k in 0..n {
        m.set_u8(p + k, b);
    }
}
fn fd_set(m: &mut dyn Mem, p: u32, fds: &[u32]) {
    m.set_u32(p, fds.len() as u32);
    for (i, s) in fds.iter().enumerate() {
        m.set_u32(p + 4 + 4 * i as u32, *s);
    }
}

#[test]
fn every_socket_call_fails_not_initialised_and_writes_nothing() {
    let mut m = setup();
    let s = 0x1234;
    let (addr, len, argp, buf, set_r, set_e, tv, name) = (
        DATA,
        DATA + 0x20,
        DATA + 0x30,
        DATA + 0x40,
        DATA + 0x100,
        DATA + 0x300,
        DATA + 0x500,
        DATA + 0x600,
    );
    for (i, b) in b"dune.dragonsdogma.com\0".iter().enumerate() {
        m.set_u8(name + i as u32, *b);
    }
    let calls: &[(&str, fn(&mut dyn Mem, &mut Cpu), Vec<u32>, u32)] = &[
        (
            "socket(AF_INET, SOCK_STREAM, 0)",
            net::socket,
            vec![2, 1, 0],
            net::INVALID_SOCKET,
        ),
        (
            "socket(AF_INET, SOCK_DGRAM, 0)",
            net::socket,
            vec![2, 2, 0],
            net::INVALID_SOCKET,
        ),
        (
            "gethostbyname(dune.dragonsdogma.com)",
            net::gethostbyname,
            vec![name],
            0,
        ),
        (
            "ioctlsocket(FIONBIO)",
            net::ioctlsocket,
            vec![s, 0x8004_667e, argp],
            net::SOCKET_ERROR,
        ),
        (
            "ioctlsocket(FIONREAD)",
            net::ioctlsocket,
            vec![s, 0x4004_667f, argp],
            net::SOCKET_ERROR,
        ),
        (
            "setsockopt(SO_REUSEADDR)",
            net::setsockopt,
            vec![s, 0xffff, 4, buf, 4],
            net::SOCKET_ERROR,
        ),
        (
            "setsockopt(TCP_NODELAY)",
            net::setsockopt,
            vec![s, 6, 1, buf, 4],
            net::SOCKET_ERROR,
        ),
        ("bind", net::bind, vec![s, addr, 16], net::SOCKET_ERROR),
        ("listen", net::listen, vec![s, 3], net::SOCKET_ERROR),
        ("accept", net::accept, vec![s, addr, len], net::INVALID_SOCKET),
        ("connect", net::connect, vec![s, addr, 16], net::SOCKET_ERROR),
        (
            "getsockname",
            net::getsockname,
            vec![s, addr, len],
            net::SOCKET_ERROR,
        ),
        ("send", net::send, vec![s, buf, 8, 0], net::SOCKET_ERROR),
        ("recv(MSG_PEEK)", net::recv, vec![s, buf, 5, 2], net::SOCKET_ERROR),
        (
            "sendto",
            net::sendto,
            vec![s, buf, 8, 0, addr, 16],
            net::SOCKET_ERROR,
        ),
        (
            "recvfrom",
            net::recvfrom,
            vec![s, buf, 8, 0, addr, len],
            net::SOCKET_ERROR,
        ),
        (
            "select(0, {s}, NULL, {s}, 0s)",
            net::select,
            vec![0, set_r, 0, set_e, tv],
            net::SOCKET_ERROR,
        ),
        (
            "select(0, NULL, NULL, NULL, 0s)",
            net::select,
            vec![0, 0, 0, 0, tv],
            net::SOCKET_ERROR,
        ),
        (
            "closesocket(0x1234)",
            net::closesocket,
            vec![s],
            net::SOCKET_ERROR,
        ),
        (
            "closesocket(INVALID_SOCKET)",
            net::closesocket,
            vec![net::INVALID_SOCKET],
            net::SOCKET_ERROR,
        ),
    ];
    for (what, f, args, failure) in calls {
        fill(&mut m, addr, 16, 0xa5);
        m.set_u32(len, 16);
        m.set_u32(argp, 0x7777_7777);
        fd_set(&mut m, set_r, &[s]);
        fd_set(&mut m, set_e, &[s]);
        api(&mut m, win32::set_last_error, &[SENTINEL]);
        assert_eq!(api(&mut m, *f, args), *failure, "{what}");
        assert_eq!(last_error(&mut m), (10093, 10093), "{what}");
        // Out-parameters and sets keep what the caller put there.
        assert!((0..16).all(|k| m.u8(addr + k) == 0xa5), "{what}");
        assert_eq!((m.u32(len), m.u32(argp)), (16, 0x7777_7777), "{what}");
        assert_eq!(
            (m.u32(set_r), m.u32(set_r + 4), m.u32(set_e)),
            (1, s, 1),
            "{what}"
        );
    }
}

#[test]
fn data_helpers_work_without_wsastartup() {
    let mut m = setup();
    api(&mut m, win32::set_last_error, &[SENTINEL]);
    // ntohs: the low word swapped (Windows' upper EAX half is leftover
    // register content; the game's caller 0x00DA5D2A zero-extends AX).
    assert_eq!(api(&mut m, net::ntohs, &[0x3412]), 0x1234);
    assert_eq!(api(&mut m, net::ntohs, &[0xabcd_3412]) & 0xffff, 0x1234);
    let set = DATA;
    fd_set(&mut m, set, &[0x1234]);
    assert_eq!(api(&mut m, net::wsa_fd_is_set, &[0x1234, set]), 1);
    fd_set(&mut m, set, &[]);
    assert_eq!(api(&mut m, net::wsa_fd_is_set, &[0x1234, set]), 0);
    fd_set(&mut m, set, &[1, 2, 0x1234]);
    assert_eq!(api(&mut m, net::wsa_fd_is_set, &[0x1234, set]), 1);
    // fd_count beyond FD_SETSIZE (64) is honoured; fd_count 0 ignores entries.
    let mut fds = vec![0u32; 65];
    fds[64] = 0x1234;
    fd_set(&mut m, set, &fds);
    assert_eq!(api(&mut m, net::wsa_fd_is_set, &[0x1234, set]), 1);
    m.set_u32(set, 0);
    m.set_u32(set + 4, 0x1234);
    assert_eq!(api(&mut m, net::wsa_fd_is_set, &[0x1234, set]), 0);
    // None of them touches the last error; WSAGetLastError reads it.
    assert_eq!(last_error(&mut m), (SENTINEL, SENTINEL));
}

#[test]
fn sspi_stops_with_a_report() {
    let mut m = setup();
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        api(
            &mut m,
            net::acquire_credentials_handle_a,
            &[0, 0, 2, 0, 0, 0, 0, 0, 0],
        )
    }));
    let report = r.expect_err("SSPI is outside the model");
    let text = report.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(
        text.contains("AcquireCredentialsHandleA") && text.contains("not modelled"),
        "{text}"
    );
}

/// Every WS2_32 and Secur32 import of DDDA.exe binds to a service (the
/// census in the L32 brief: 18 WS2_32 ordinals, 9 Secur32 names).
#[test]
fn the_games_network_imports_all_bind() {
    let path = std::env::var("DDDA_EXE")
        .unwrap_or_else(|_| r"E:\SteamLibrary\steamapps\common\DDDA\DDDA.exe".into());
    let Ok(exe) = ddexe::ExeFile::open(&path) else {
        eprintln!("skipped: no DDDA.exe at {path}");
        return;
    };
    let mut m = GuestMemory::new();
    // the import table lives in the image: map it first, as Session::new does
    ddruntime::image::load(&mut m, &exe).expect("image");
    let imports = ddruntime::image::bind_imports(&mut m, &exe).expect("imports");
    let network: Vec<_> = imports
        .into_iter()
        .filter(|i| i.dll.eq_ignore_ascii_case("ws2_32.dll") || i.dll.eq_ignore_ascii_case("secur32.dll"))
        .collect();
    assert_eq!(network.len(), 27);
    let mut r = Registry::new();
    assert_eq!(services::bind_apis(&mut r, &network).unwrap(), 27);
    for i in network {
        assert!(
            matches!(r.get(i.token), Some(Binding::Service(_))),
            "{}",
            i.full_name()
        );
    }
}
