# L170: guest-to-runtime call overhead on the game threads

Owner: claude-2. Branch `task/L170-service-calls` (from fps-line c0a9a793).
Assigned by the head (#13503): count service calls on the game threads (main
and job workers) and cut the top ones with fast exact paths. Proven with
isolated tests and microbenchmarks; game runs are the head's.

## Sizing (c31def profile, main thread plus job workers 0x1070..0x1084)

Of 1,474 busy samples:

| Item | Share |
|---|---|
| Guest memory accessors called out of line (`fast_u32` 26.6%, `fast_set_u32` 9.1%, `fast_u8`, `fast_u16`, `page_table`) | ~40% |
| `call_ext` dispatch, mostly lifted-to-lifted calls (install closure, `run_binding`, handler clone, `call_entry`) | ~6% |
| `run_service_as` inclusive | 11.2% |
| of which `wait_for_single_object` | 7.1% |
| so real service work | ~4% |

The memory accessors belong to Team A (claude-9, #13696).

## Done (each tested; not yet run in game)

- **7860cd92.** `call_ext` reads its handler through a destructor-free
  thread-local pointer: no Rc clone and no RefCell borrow per call.
  - A handler replaced while a call runs is kept until no call runs.
  - Tests: `crates/ddnative-base/tests/call_ext.rs`.
  - `DDRUNTIME_SERVICE_CENSUS=1` reports service calls/s and ms/s by service,
    by (service, guest return address) and by thread, plus misses of the
    per-thread binding cache. Off, it costs one relaxed load per service call.
- **5708f35b.** memcpy/memset copy and fill up to 256 bytes through a stack
  buffer; they used to allocate an n-byte heap buffer per call.
  - memcpy, memset and the ported CRT string routines run on the Integer
    service path (no MXCSR switch, no x87 park).
  - Tests: byte-wise model equivalence, `tests/crt_integer.rs` (guest FP
    state preserved).
- **7019fca6.** TlsGetValue, TlsSetValue, GetLastError and SetLastError on
  the Integer path, covered by L135's audited-imports harness.

## Microbenchmarks

`perf_micro`, release, best of 3, on a machine loaded by other builds:

| Call | Before | After |
|---|---|---|
| `call_ext` to a lifted function (direct call: 5 ns) | 19.2 ns | 15.0–17.1 ns |
| `call_ext` to a service | 49.9 ns | 37.7–46 ns |
| memcpy 32 B through `call_ext`, as the runtime binds it | 100.8 ns | 48.1 ns |
| memcpy 256 B, same | 113.5 ns | 56.6 ns |

## Next

- A `DDRUNTIME_SERVICE_CENSUS=1` game run, to rank the remaining services
  and call sites on the job threads.
