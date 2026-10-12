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

(open)
