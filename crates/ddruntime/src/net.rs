//! Network services (brief L32): WS2_32 (Winsock) and Secur32 (SSPI).
//!
//! The rebuild runs offline. DDDA.exe never calls WSAStartup (it is not
//! among its imports and its name is nowhere in the image); in the original
//! process Winsock is initialised by Steam's `steamclient.dll`, which the
//! rebuild does not load. So the runtime answers what Windows answers to a
//! process in which no module has called WSAStartup: every socket function
//! fails with WSANOTINITIALISED (10093) and sets the thread's last error
//! to it; out-parameters and fd_sets are left untouched. The data-only
//! helpers (`ntohs`, `__WSAFDIsSet`, `WSAGetLastError`) work as usual.
//! Measured on Windows 11 build 26200 with
//! research/runtime/windows/NetProbe.cs, part A (32-bit process).
//!
//! The faithful state of the original would be "Winsock initialised, no
//! network"; it could not be measured here (Results of the L32 brief).
//! Every caller is gated by the online state, which
//! InternetGetConnectedState already reports as offline (crate::system).
//!
//! SSPI (Schannel) runs only after a socket connected (the TLS start at
//! 0x00DA27F7 follows a select that reports the socket writable), which
//! cannot happen here, so the Secur32 imports stop with a report.
use crate::services::{arg, ret};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;

/// WSANOTINITIALISED (winerror.h): "A successful WSAStartup call must occur
/// before using this function."
pub const WSANOTINITIALISED: u32 = 10093;
/// SOCKET_ERROR and INVALID_SOCKET (winsock2.h).
pub const SOCKET_ERROR: u32 = 0xFFFF_FFFF;
pub const INVALID_SOCKET: u32 = 0xFFFF_FFFF;

/// Reports the first call of each socket function once (to stderr): the
/// census expects none before the game goes online, so a boot run shows
/// whether any path reaches them.
fn first_call(name: &'static str) {
    static SEEN: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let mut seen = SEEN.lock().unwrap();
    if !seen.contains(&name) {
        seen.push(name);
        eprintln!("net: the game called {name} (offline: WSANOTINITIALISED)");
    }
}

/// A socket function before WSAStartup: `failure` in EAX, last error
/// WSANOTINITIALISED, no output written; stdcall cleanup of `args` words.
fn not_initialised(m: &mut dyn Mem, cpu: &mut Cpu, name: &'static str, args: u32, failure: u32) {
    first_call(name);
    crate::runtime::set_last_error(WSANOTINITIALISED);
    cpu.eax = failure;
    ret(m, cpu, 4 * args);
}

/// #1 accept(s, addr, addrlen)
pub fn accept(m: &mut dyn Mem, cpu: &mut Cpu) {
    not_initialised(m, cpu, "accept", 3, INVALID_SOCKET);
}
/// #2 bind(s, name, namelen)
pub fn bind(m: &mut dyn Mem, cpu: &mut Cpu) {
    not_initialised(m, cpu, "bind", 3, SOCKET_ERROR);
}
/// #3 closesocket(s)
pub fn closesocket(m: &mut dyn Mem, cpu: &mut Cpu) {
    not_initialised(m, cpu, "closesocket", 1, SOCKET_ERROR);
}
/// #4 connect(s, name, namelen)
pub fn connect(m: &mut dyn Mem, cpu: &mut Cpu) {
    not_initialised(m, cpu, "connect", 3, SOCKET_ERROR);
}
/// #6 getsockname(s, name, namelen)
pub fn getsockname(m: &mut dyn Mem, cpu: &mut Cpu) {
    not_initialised(m, cpu, "getsockname", 3, SOCKET_ERROR);
}
/// #10 ioctlsocket(s, cmd, argp)
pub fn ioctlsocket(m: &mut dyn Mem, cpu: &mut Cpu) {
    not_initialised(m, cpu, "ioctlsocket", 3, SOCKET_ERROR);
}
/// #13 listen(s, backlog)
pub fn listen(m: &mut dyn Mem, cpu: &mut Cpu) {
    not_initialised(m, cpu, "listen", 2, SOCKET_ERROR);
}
/// #16 recv(s, buf, len, flags)
pub fn recv(m: &mut dyn Mem, cpu: &mut Cpu) {
    not_initialised(m, cpu, "recv", 4, SOCKET_ERROR);
}
/// #17 recvfrom(s, buf, len, flags, from, fromlen)
pub fn recvfrom(m: &mut dyn Mem, cpu: &mut Cpu) {
    not_initialised(m, cpu, "recvfrom", 6, SOCKET_ERROR);
}
/// #18 select(nfds, readfds, writefds, exceptfds, timeout): the sets are
/// returned as given.
pub fn select(m: &mut dyn Mem, cpu: &mut Cpu) {
    not_initialised(m, cpu, "select", 5, SOCKET_ERROR);
}
/// #19 send(s, buf, len, flags)
pub fn send(m: &mut dyn Mem, cpu: &mut Cpu) {
    not_initialised(m, cpu, "send", 4, SOCKET_ERROR);
}
/// #20 sendto(s, buf, len, flags, to, tolen)
pub fn sendto(m: &mut dyn Mem, cpu: &mut Cpu) {
    not_initialised(m, cpu, "sendto", 6, SOCKET_ERROR);
}
/// #21 setsockopt(s, level, optname, optval, optlen)
pub fn setsockopt(m: &mut dyn Mem, cpu: &mut Cpu) {
    not_initialised(m, cpu, "setsockopt", 5, SOCKET_ERROR);
}
/// #23 socket(af, type, protocol)
pub fn socket(m: &mut dyn Mem, cpu: &mut Cpu) {
    not_initialised(m, cpu, "socket", 3, INVALID_SOCKET);
}
/// #52 gethostbyname(name): NULL, even for a numeric address (measured:
/// "127.0.0.1" fails the same way before WSAStartup). The game's resolver
/// 0x00D8B8F0 checks only for NULL.
pub fn gethostbyname(m: &mut dyn Mem, cpu: &mut Cpu) {
    not_initialised(m, cpu, "gethostbyname", 1, 0);
}

/// #15 ntohs(netshort): the byte-swapped low word. Windows leaves EAX's
/// upper half as it happens to be (measured: 0x014F for one call); the
/// runtime clears it. The game's caller 0x00DA5D2A zero-extends AX itself.
pub fn ntohs(m: &mut dyn Mem, cpu: &mut Cpu) {
    let v = arg(m, cpu, 0) as u16;
    cpu.eax = v.swap_bytes() as u32;
    ret(m, cpu, 4);
}

/// #111 WSAGetLastError(): the thread's last error, the same value as
/// GetLastError (measured: a SetLastError value reads back unchanged).
pub fn wsa_get_last_error(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = crate::runtime::last_error();
    ret(m, cpu, 0);
}

/// #151 __WSAFDIsSet(fd, set): 1 when one of the set's first `fd_count`
/// entries equals `fd` (counts above FD_SETSIZE are honoured: found at
/// index 64 of a 65-entry set), else 0; works without WSAStartup and does
/// not touch the last error (measured).
pub fn wsa_fd_is_set(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (fd, set) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let count = m.u32(set);
    cpu.eax = (0..count).any(|i| m.u32(set.wrapping_add(4 + 4 * i)) == fd) as u32;
    ret(m, cpu, 8);
}

/// A Secur32 (SSPI) import: only reached after a socket connected, which
/// the offline runtime never allows. Stops with a report.
fn sspi(name: &str) -> ! {
    panic!("Secur32 {name} is not modelled (crate::net): SSPI runs only on a connected socket, and the offline runtime has none");
}
macro_rules! sspi_stop {
    ($($f:ident => $name:literal),* $(,)?) => {
        $(pub fn $f(_m: &mut dyn Mem, _cpu: &mut Cpu) { sspi($name) })*
    };
}
sspi_stop! {
    acquire_credentials_handle_a => "AcquireCredentialsHandleA",
    initialize_security_context_a => "InitializeSecurityContextA",
    query_context_attributes_a => "QueryContextAttributesA",
    apply_control_token => "ApplyControlToken",
    encrypt_message => "EncryptMessage",
    decrypt_message => "DecryptMessage",
    delete_security_context => "DeleteSecurityContext",
    free_credentials_handle => "FreeCredentialsHandle",
    free_context_buffer => "FreeContextBuffer",
}

/// DDDA.exe imports WS2_32 by ordinal (names from the host's ws2_32.dll
/// export table) and Secur32 by name.
pub const APIS: &[(&str, &str, fn(&mut dyn Mem, &mut Cpu))] = &[
    ("WS2_32.dll", "#1", accept),
    ("WS2_32.dll", "#2", bind),
    ("WS2_32.dll", "#3", closesocket),
    ("WS2_32.dll", "#4", connect),
    ("WS2_32.dll", "#6", getsockname),
    ("WS2_32.dll", "#10", ioctlsocket),
    ("WS2_32.dll", "#13", listen),
    ("WS2_32.dll", "#15", ntohs),
    ("WS2_32.dll", "#16", recv),
    ("WS2_32.dll", "#17", recvfrom),
    ("WS2_32.dll", "#18", select),
    ("WS2_32.dll", "#19", send),
    ("WS2_32.dll", "#20", sendto),
    ("WS2_32.dll", "#21", setsockopt),
    ("WS2_32.dll", "#23", socket),
    ("WS2_32.dll", "#52", gethostbyname),
    ("WS2_32.dll", "#111", wsa_get_last_error),
    ("WS2_32.dll", "#151", wsa_fd_is_set),
    (
        "Secur32.dll",
        "AcquireCredentialsHandleA",
        acquire_credentials_handle_a,
    ),
    (
        "Secur32.dll",
        "InitializeSecurityContextA",
        initialize_security_context_a,
    ),
    (
        "Secur32.dll",
        "QueryContextAttributesA",
        query_context_attributes_a,
    ),
    ("Secur32.dll", "ApplyControlToken", apply_control_token),
    ("Secur32.dll", "EncryptMessage", encrypt_message),
    ("Secur32.dll", "DecryptMessage", decrypt_message),
    ("Secur32.dll", "DeleteSecurityContext", delete_security_context),
    ("Secur32.dll", "FreeCredentialsHandle", free_credentials_handle),
    ("Secur32.dll", "FreeContextBuffer", free_context_buffer),
];
