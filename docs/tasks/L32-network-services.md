# L32: network services offline (Winsock, WinINet, SSPI/Schannel)

- **Owner:** claude-4
- **Branch:** `task/L32-network` (worktree `E:\ddda-rs-wt\L32`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

DDDA.exe imports Winsock (`socket`, `connect`, `send`, `recv`, `select`,
`gethostbyname`, `bind`, `listen`, `accept`, `sendto`, `recvfrom`,
`ioctlsocket`, `setsockopt`, `getsockname`, `WSAGetLastError`,
`__WSAFDIsSet`, `ntohs`), SSPI/Schannel (`AcquireCredentialsHandleA`,
`InitializeSecurityContextA`, `EncryptMessage`, `DecryptMessage`,
`QueryContextAttributesA`, `ApplyControlToken`, `DeleteSecurityContext`,
`FreeCredentialsHandle`, `FreeContextBuffer`) and WinINet. claude-2 found
the sockets are reached through the socket factory 0x00D8B690. L17A made
`InternetGetConnectedState` answer "offline".

The rebuild runs offline (online pawn features stay off). The game must
see exactly what Windows gives a program without network access: the
documented failures, in the order the game checks them, so it takes its
own offline paths. It must never crash or loop.

## Task

1. **Census** (Results, with addresses): every call site of these imports,
   the call graph above the socket factory, and which game features use
   them (pawn sharing, Steam-independent servers, crash reporting, and so
   on). Record which of these the start-up and title screen can reach.
2. **Offline semantics, measured on Windows.** Disable networking in a
   small probe (or use an unreachable address) and record the exact
   return values and WSA error codes for each API in the census's call
   order. That includes WSAStartup (an import? check), socket creation
   success vs failure, connect to an unresolvable host, select with zero
   sockets, and gethostbyname failing with WSAHOST_NOT_FOUND. Do the same
   for SSPI with no network.
3. **Implement** the services in `crates/ddruntime` with those answers,
   plus tests, following L17A's pattern (`research/runtime/windows/`
   probes, measured values in tests, stop with a report outside the
   model).
4. Results and the RUNTIME.md row. Merge-train rule: cheap checks on your
   branch, then hand off.

## Results

### Summary

- `crates/ddruntime/src/net.rs`: the 18 WS2_32 imports (all by ordinal)
  and the 9 Secur32 imports. Winsock answers exactly what Windows answers
  to a process in which no module has called WSAStartup (measured).
  SSPI stops with a report, because it runs only on a connected socket.
- `research/runtime/windows/NetProbe.cs`: the probe (32-bit, never sends
  a packet off the machine). `crates/ddruntime/tests/network.rs`: the
  measured contracts, plus a check that every network import of the real
  exe binds.
- Decision with the head (messages of 2026-10-11): the faithful state of
  the original is "Winsock initialised by steamclient.dll, no network".
  It could not be measured here (below), so the deterministic,
  fully measured "not initialised" state is implemented. The paths are
  gated anyway.

### Census

Imports (IAT, Ghidra xrefs, names from the host's `SysWOW64\ws2_32.dll`
export table): WS2_32 `0x0139D3B8..0x0139D3FC`, ordinals 1 accept, 2 bind,
3 closesocket, 4 connect, 6 getsockname, 10 ioctlsocket, 13 listen,
15 ntohs, 16 recv, 17 recvfrom, 18 select, 19 send, 20 sendto,
21 setsockopt, 23 socket, 52 gethostbyname, 111 WSAGetLastError,
151 __WSAFDIsSet. **No WSAStartup or WSACleanup**: neither is imported,
and neither name occurs anywhere in the image (nor `getaddrinfo`).
`steam_api.dll` (game folder) imports no Winsock either; in the original
process, Steam's `steamclient.dll`, which `steam_api` loads, initialises
Winsock. Secur32 `0x0139D298..0x0139D2B8`: the nine SSPI functions in the
brief. WININET: only `InternetGetConnectedState` (`0x0139D3A0`).

Call sites:

| Import | Sites (enclosing function) |
|---|---|
| socket | `0x00DA2168` (socket ctor `0x00DA2100`: `socket(AF_INET, type == 2 ? SOCK_DGRAM : SOCK_STREAM, 0)`; on failure the WSA code goes to the object's error report, vtable `+0x20`, code `0x80010031`) |
| ioctlsocket | `0x00DA1A6B` (`0x00DA19D0`), `0x00DA1AD7` (`0x00DA1AB0`), `0x00DA1C07` (`0x00DA1BD0`: FIONBIO, then setsockopt) |
| setsockopt | `0x00DA1C19` (`0x00DA1BD0`: SO_REUSEADDR, TCP_NODELAY, SO_SNDBUF/SO_RCVBUF 0x8000 for TCP, SO_BROADCAST for UDP) |
| connect | `0x00DA1EB6` (`0x00DA1DF0`, vtable slot `0x01428C24`; WSAEWOULDBLOCK, WSAEALREADY and WSAEISCONN count as in progress) |
| getsockname | `0x00DA1F11` (`0x00DA1DF0`), `0x00DA2302` (`0x00DA21B0`) |
| bind, listen | `0x00DA202E`, `0x00DA2076` (`0x00DA1F90`) |
| accept | `0x00DA2241` (`0x00DA21B0`; WSAEWOULDBLOCK is "nothing pending") |
| select | `0x00DA1A22` (`0x00DA19D0`), `0x00DA276E` (connect poll `0x00DA2670`: `select(0, NULL, {s}, {s}, 0)`) |
| __WSAFDIsSet | thunk `0x012ED0D0` from `0x00DA1A3D`, `0x00DA1A4F`, `0x00DA27A7`, `0x00DA27DF` |
| recv | `0x00DA1B1C` (`0x00DA1AB0`), `0x00DA252C` (`0x00DA24C0`), `0x00DA297A` (`0x00DA2850`) |
| send / sendto / recvfrom | `0x00DA244A`, `0x00DA258F` / `0x00DA23F3` / `0x00DA2912` |
| closesocket | `0x00DA25FF` (`0x00DA25C0`, from the destructor `0x00DA2B60`) |
| ntohs | `0x00DA5D2A` (TLS record length `0x00DA5D10`; zero-extends AX itself) |
| gethostbyname | `0x00D8B918` (resolver `0x00D8B8F0`; checks only for NULL, never the WSA code) |
| WSAGetLastError | 11 sites in the socket class, all feeding the error report |
| AcquireCredentialsHandleA | thunk `0x0130A2E8` from `0x00DA5CF2` (`0x00DA5CA0`: "Microsoft Unified Security Protocol Provider", outbound, SCHANNEL_CRED v4 with `grbitEnabledProtocols = 0xFC0` pushed at `0x00DA27F7`), called at `0x00DA2802` in the connect poll, only after select reported the socket writable |
| InitializeSecurityContextA, QueryContextAttributesA, EncryptMessage, DecryptMessage, ApplyControlToken, DeleteSecurityContext, FreeCredentialsHandle, FreeContextBuffer | `0x00DA624A` (`0x00DA6150`), `0x00DA5E1B` (`0x00DA5DE0`), `0x00DA5F8A` (`0x00DA5ED0`), `0x00DA6076` (`0x00DA5FD0`), `0x00DA5EBA` (`0x00DA5E60`), `0x00DA6101` (`0x00DA60F0`), `0x00DA5C7F` (`0x00DA5C70`), thunk `0x0130A2EE` from `0x00DA5DE0`/`0x00DA60F0`/`0x00DA6150` |
| InternetGetConnectedState | `0x00D2A3A1` (`0x00D2A390`) |

The socket class (vtable `0x01428BD8`, no MSVC RTTI) is made only by the
factory `0x00D8B690` (type 3 = this TCP/UDP socket). The factory's users:

1. **sDDCaProto** (Capcom's online pawn/login server), `sDDCaProto::vf06`
   `0x00CA23F0` -> `sCaProto::vf06` `0x00CEEA80` -> `0x00CEF210` ->
   `0x00CEF0A0` -> the connection state machine `0x00CEF430`: resolve the
   host (resolver object ctor `0x00D8B9E0`, `gethostbyname` in
   `0x00D8B8F0`), `0x00D8B690(3, 1, 0)` for a TCP socket, connect (vtable
   `+0x48`), poll (`+0x50`, where TLS starts). The host is
   `dune.dragonsdogma.com`, port 12501 (`0x30D5`), set by the login
   sequence `0x00CA2430` (string `0x016130F8`). That sequence runs only
   while `sDDCaProto+0x4BD` is set: at `0x00CA1479`, `0x00CA15F9`,
   `0x00CA1779` (requests from `sNetworkExt::vf06` `0x00467F50` via
   `0x0046DCA0`, and from the object with vtable `0x0156CF40` via
   `0x004FC620`), and at `0x00CA3E19`, `0x00CA3F76` (from
   `sNetworkExt`'s task `0x00C9D030`).
2. **sNetwork sessions**: `0x011562C0` (from `0x012BAF30`, table
   `0x01443CBC`) creates a session through `0x00D8B5F0` (`0x00D9BFF0`,
   which calls the factory twice, or `0x00D9FD70`), only when the network
   state `0x00DE2E70()` is 3 or 4.
3. **sNetworkExt tasks** at `+0x4F0/+0x4F4/+0x4F8/+0x500` (e.g.
   `cNetStorageReward` at `0x00468A5C`) are created by request state
   machines that each require an online session (`sNetwork+0x220`) and a
   logged-on check (`0x00DE2AB0()+0x888`).

The game's connectivity check `0x00D2A390` maps
`InternetGetConnectedState == FALSE` (and no LAN/modem bit) to state 1,
"offline". Otherwise it maps to 2 when `SteamUser()->BLoggedOn()`. The
runtime answers FALSE (L17A), so every request above stays off. No crash
reporting or other server code uses these imports. **Start-up and title
screen**: statically, no path from them reaches a socket, DNS or SSPI call
without one of the online requests. Not yet confirmed by a boot run:
every socket function prints one `net: the game called ...` line on its
first call, so the integration build's boot route shows it.

### Measured answers (NetProbe.cs, Windows 11 build 26200, 32-bit)

Part A, before WSAStartup (what the runtime implements):

| Call | Result |
|---|---|
| socket (TCP and UDP), accept | INVALID_SOCKET, WSAGetLastError = GetLastError = 10093 (WSANOTINITIALISED) |
| bind, listen, connect, getsockname, ioctlsocket (FIONBIO, FIONREAD), setsockopt, send, recv, sendto, recvfrom, select (with sets, and with all-NULL sets), closesocket (any handle, INVALID_SOCKET too) | SOCKET_ERROR, 10093; out-parameters (address, length, FIONREAD count) and fd_sets unchanged |
| gethostbyname("dune.dragonsdogma.com"), gethostbyname("127.0.0.1") | NULL, 10093 (no resolution attempted, not even numeric) |
| ntohs(0x3412) | 0x1234; EAX's upper half is leftover register content (0x014F seen) |
| __WSAFDIsSet | 1 if among the first fd_count entries (index 2 and index 64 of a 65-entry set both found), else 0; fd_count 0 ignores the entries; the last error is untouched |
| WSAGetLastError | the thread's last error (a SetLastError sentinel reads back) |

Part B, after WSAStartup(2.2), only calls that stay on the machine (for
the "initialised" model later): WSAStartup returns 0, version 2.2; TCP
socket valid; FIONBIO, SO_REUSEADDR, TCP_NODELAY, SO_SNDBUF/RCVBUF 0x8000
return 0; FIONREAD 0 on an unconnected socket; getsockname unbound
WSAEINVAL (10022); send/recv unconnected WSAENOTCONN (10057);
`select(0, NULL, NULL, NULL, 0)` WSAEINVAL; bind 0.0.0.0:0 then
getsockname 0.0.0.0 with a port; listen 0; accept with nothing pending
WSAEWOULDBLOCK (10035); select on the listening socket 0. A non-blocking
connect to a closed loopback port: WSAEWOULDBLOCK, then select with 2 s
reports nothing yet (Windows retries the refused SYN) and a second connect
gives WSAEALREADY (10037). UDP: SO_BROADCAST 0, recvfrom empty
WSAEWOULDBLOCK, sendto to itself 8 bytes, and recvfrom then gets them;
closesocket 0, then WSAENOTSOCK (10038); gethostbyname("127.0.0.1") gives
AF_INET, length 4, one address 127.0.0.1.

SSPI was not measured: it is unreachable here (no socket connects). With
Schannel the application itself carries every handshake token over its
own connection (Microsoft Learn, "Creating a Secure Connection Using
Schannel"), so SSPI has no network-dependent failure of its own before
the socket I/O, which already fails.

### The faithful model, and why it is not the one implemented

In the original, Winsock is initialised, and an offline machine answers
from its network stack: DNS failure for `dune.dragonsdogma.com` (expected
WSAHOST_NOT_FOUND, unmeasured), and connects that fail with
"unreachable"-class errors. Measuring that needs a machine without
network. This one cannot be taken offline (other agents, and an
outward-facing change). Windows Sandbox is not installed. Hyper-V is
present (vmms running), but this account lacks the rights (`Get-VM`:
permission denied) and there is no VM image. If the player enables
Windows Sandbox: a `.wsb` with `<Networking>Disable</Networking>` and a
mapped folder holding `netprobe.exe`, then run part B with
`gethostbyname("dune.dragonsdogma.com")` and a connect to a public
address added. What would change: socket creation and the socket options
succeed, so the game's socket objects exist. sDDCaProto's state machine
still fails at the DNS step (`gethostbyname` NULL -> error `0x80020031`)
before any connect, and SSPI stays unreachable. The game's offline result
is the same either way, given that the online requests are themselves
gated off.

### Verification

- Written, not compiled yet: the head paused all builds, and under the
  batched-verification rule the integration build checks it.
  `cargo test -p ddruntime --test network` holds four tests: every socket
  call against part A (return, both error readers, untouched outputs),
  the data helpers, the SSPI stop, and the real exe's 27 network imports
  binding (skips without the install).
- The probe ran on this machine; its output is quoted above (not
  committed: it is a measurement, regenerable with the README's command).

### Left open

- Confirm in a boot run that no `net:` first-call line appears before an
  online request (the static census says none).
- The "initialised, no network" measurement, if the player enables Windows
  Sandbox.
