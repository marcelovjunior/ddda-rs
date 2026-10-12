# L17: the remaining Windows services, and the SEH frame contract

Owners: Part A, a second Claude Code agent (headless session "claude-2",
driven by the head like Codex), branch `task/L17-windows-services`,
worktree `E:\ddda-rs-wt\L17`. Parts B and C, a third Claude Code agent
("claude-3"), branch `task/L17-seh-workers`, worktree `E:\ddda-rs-wt\L17BC`.

## Why

The runtime (`crates/ddruntime`) answers the Windows APIs the game calls
with services that follow the documented contracts. The boot will next
reach the imports that have no service yet; each stops the session with its
name. This task implements them before the boot gets there, and closes the
lifter gap L15 left for the CRT's SEH helpers.

## Part A: services (crates/ddruntime)

Unbound imports, with what the game needs from them (verify call sites in
Ghidra, 127.0.0.1:8089; follow Microsoft Learn's contracts exactly, cite
them in doc comments, test every service):

- Strings and code pages (KERNEL32): MultiByteToWideChar,
  WideCharToMultiByte (code page 1252 = the runtime's GetACP, CP_UTF8,
  CP_ACP; flags; the size-query form), LCMapStringW, CompareStringW,
  GetStringTypeW (the CRT's locale code calls these; use the en-US/1252
  answers Windows gives, with a table for the character classes).
- Profile (ini) files: GetPrivateProfileStringW,
  WritePrivateProfileStringW (through crate::files: reads from the
  install or the overlay, writes only to the overlay; the game's settings
  file).
- Registry (ADVAPI32): RegOpenKeyExA, RegQueryValueExA, RegCloseKey: a
  read-only model with the keys the game reads (find them; report each
  with its call site); unknown keys answer ERROR_FILE_NOT_FOUND.
- Files: DeleteFileA, MoveFileA, CopyFileA, SetEndOfFile,
  GetDiskFreeSpaceA, SHELL32 SHCreateDirectoryExW (overlay only; refuse
  anything outside the roots like crate::files does).
- Time: GetTimeZoneInformation, FileTimeToSystemTime,
  FileTimeToLocalFileTime (a fixed UTC zone for determinism: document it).
- Memory: GlobalAlloc/Lock/Unlock/Free, LocalAlloc (guest heap).
- Misc: SetEnvironmentVariableA, SetStdHandle, GetConsoleMode/CP,
  WriteConsoleW (no console: the documented failures), VerifyVersionInfoA
  and VerSetConditionMask (Windows 10 answers), DebugBreak (stop with a
  report), RaiseException (stop with a report naming the exception code;
  dispatch is not modelled), GetModuleFileNameExA (PSAPI),
  InternetGetConnectedState (WININET: not connected).

Leave the D3D9 files and crates (the head's) and the L16 input/audio
modules alone.

## Part B: SEH frame fixtures (research/native)

L15 lifted `__SEH_prolog4` 0x012F71E0 and `__SEH_epilog4` 0x012F7225 but
their fixtures fail the recorder's ordinary-stack guard: prolog returns ESP
= entry ESP - 24 - entry [esp+8], epilog returns ESP = entry EBP + 4.
Design and implement the smallest general recorder contract for such
caller-frame helpers (e.g. paired prolog/epilog fixtures in a caller-shaped
frame, or a declared exit-ESP relation checked exactly), record them and
0x01306F7F, and report. No address exceptions without a general rule.

## Part C (Codex, with part B): a strategy for worker-thread loops

The game's worker threads are loops of the shape "wait for an event until
the wait returns 0, then break if a quit flag in memory is set, else do one
step and signal" (the renderer worker 0x00DAC090, `DAT_018D08F4+0xDE`; the
resource worker 0x00DBA9A0). No current strategy drives them: random mock
returns almost never make the wait return 0 (runaway), and zero inputs make
the wait succeed but keep the flag clear forever (runaway). Add a
structured-input strategy in which mocked calls return 0 while memory stays
random (`mockzero-v1`, a separate strategy as before), record the remaining
runaway/outside-image failures with it, and report what verifies; these two
thread entries first (the runtime cannot run their threads until they are
lifted and verified).

## Rules

As before (docs/tasks/L12-remaining-lift-rejections.md, "Rules"):
CARGO_BUILD_JOBS=4, CARGO_INCREMENTAL=0, at most six vector workers, never
two heavy jobs at once (wait and poll, do not end the turn), no pushes,
commit on your branch, results below.

## Results

### Part A (claude-2, branch `task/L17-windows-services`)

All 35 imports of the brief now have services (`bind_apis`; test
`every_l17_import_is_bound_to_a_service`). Each follows the Microsoft Learn
contract with the original's stdcall cleanup. Where the documentation says
nothing about an answer (error codes, what out-parameters hold on failure,
flag combinations, file format details), the answer was **measured on
Windows 11 build 26200 (en-US)** with small C# probes, now in
`research/runtime/windows/` (README there: how to rerun them). The tests
use those measured values. Anything outside the model stops the session
with a report naming it; no answer is invented.

**Method check.** `research/runtime/windows/Vectors.cs` records about
68,000 random cases from the host's own functions: UTF-8 decoding (12,000),
UTF-16 to 1252/UTF-8 (16,000), sort keys and CompareStringW (40,000). The
runtime matches all of them: 30,644 sort keys and comparisons in the
modelled range, the rest outside it, where the model stops. Test
`conversions_and_sort_keys_match_recorded_windows_vectors` reruns this
when `DDRUNTIME_WINDOWS_VECTORS` points at the recorded files. The
vectors are not committed; the probe that writes them is.

| Module | Services | Call sites (Ghidra) | Notes |
|---|---|---|---|
| `text/` (new) | MultiByteToWideChar, WideCharToMultiByte, GetStringTypeW, LCMapStringW, CompareStringW | CRT `setSBUpLow` 0x012FABB0 via `__crtGetStringTypeA_stat` (GetStringTypeW at 0x013059BB) and `__crtLCMapStringA_stat` (0x012FDC16/4F/B5); `__crtCompareStringA_stat` 0x01309D81 (only reachable from `getenv` in a non-C locale; the game never calls setlocale); tzset 0x01302F75; `__write_nolock`, `__read_nolock`, `__mbtowc_l`, `__wctomb_s_l`; game ini wrappers 0x00CFFD20 / 0x00D00010; 0x00F79F70, 0x00F95320, 0x00F9FBA0, 0x00DBD170, 0x00DF59BE | Code pages 1252 (= CP_ACP, CP_THREAD_ACP) and UTF-8. `text/tables.rs` is generated by `gen_text_tables.py` from the host: the 32 bytes 0x80..0x9F, the 452 best-fit mappings, and for the 256 characters 1252 decodes to their CT_CTYPE1/2/3, lower/upper case and sort elements. That covers everything the CRT can ask, since it only passes text converted from the ANSI code page. Locales 0/0x400/0x800/0x409. Not modelled (stop): other code pages, locales and characters; MB_COMPOSITE and MB_USEGLYPHCHARS for 1252; WC_COMPOSITECHECK; title case, linguistic casing and kana/width mappings; word-sort special weights (`'`, `-`, dashes, controls without SORT_STRINGSORT). |
| `profile.rs` (new) | GetPrivateProfileStringW, WritePrivateProfileStringW | MtRegistry 0x00CFFF00 (path `<Documents>\CAPCOM\DRAGONS DOGMA DARK ARISEN\config.ini`); reader 0x00D000CB, writer 0x00CFFE09 | Goes through `crate::files` (install read-only, writes to the overlay/home). Measured file rules: UTF-16LE BOM or 1252; CR/LF/CRLF; trimming; quotes; the first section only; where new keys and sections go; exact-fit buffers counted as too small; deletes on a missing file create it empty. Not modelled (stop): file names without a path, UTF-16BE files. |
| `registry.rs` (new) | RegOpenKeyExA, RegQueryValueExA, RegCloseKey | sAI ctor 0x00F79F70 (open 0x00F79FE1, query `~MHz` 0x00F7A00D); system info 0x00DBD170 (open 0x00DBD1BD, query `ProcessorNameString` 0x00DBD1E5, close 0x00DBD22A) | Read-only model: HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0 (and its parent keys) with five values recorded from the maintainer's machine (Ryzen 7 5700X3D, 3000 MHz), so sessions do not depend on the host. Unknown keys return ERROR_FILE_NOT_FOUND. Write access returns ERROR_ACCESS_DENIED. Other values of the CPU key stop. Measured quirks are reproduced: the UTF-16 tail left in a large REG_SZ buffer, *lpType = 0 on failure, closing a root returns ERROR_INVALID_HANDLE. |
| `files.rs` | DeleteFileA, MoveFileA, CopyFileA, SetEndOfFile, GetDiskFreeSpaceA, SHCreateDirectoryExW | 0x00D0CDC5, 0x00D0CE0D, 0x00D0CFCC; MtFile::vf14 0x00D0D031, MtFileEx::vf14 0x00D0DE1A, CRT `__chsize_nolock`; MtFile::vf05 0x00D0D109; MtRegistry 0x00CFFF82 | New third root, the **home** (the sandbox outside the overlay, where SHGetFolderPathW's folders already pointed but which the file services refused before). A file that exists only in the install is read-only to the guest (ERROR_ACCESS_DENIED). GetDiskFreeSpaceA answers one recorded volume (8 x 512-byte sectors, 16,192,600 of 122,092,543 clusters free); paths off the install's and sandbox's drives stop. |
| `system.rs` (new) | GetTimeZoneInformation, FileTimeToSystemTime, FileTimeToLocalFileTime | tzset 0x0130309E; 0x00D0CB80 | Fixed UTC zone: Windows' own "UTC" zone data (TZI all zero, both names "Coordinated Universal Time"), TIME_ZONE_ID_UNKNOWN, for determinism. |
| | GlobalAlloc/Lock/Unlock/Free, LocalAlloc | MtClipboardStream::vf10 0x00D01E86; restart helpers 0x011797F0/0x011799A0/0x01179A70; `___delayLoadHelper2@8` 0x0130A469 | Guest heap. Moveable handles live in their own range. Measured flag masks (0x7F72 / 0x0F72), lock counting and errors (DISCARDED, NOT_LOCKED, INVALID_HANDLE). |
| | SetEnvironmentVariableA (and GetEnvironmentStringsW / FreeEnvironmentStringsW in win32.rs) | `___crtsetenv` 0x0130A175 | Environment starts empty; kept sorted case-insensitively, as measured. |
| | SetStdHandle (and GetStdHandle in win32.rs), GetConsoleMode, GetConsoleCP, WriteConsoleW | `__set_osfhnd`/`__free_osfhnd`; `__write_nolock` 0x01303E12/32; `__putwch_nolock` 0x01308668 | No console: failures measured in a console-less GUI process (ERROR_INVALID_HANDLE, out-parameters untouched). |
| | VerSetConditionMask, VerifyVersionInfoA | sPad ctor 0x00DFA0C0 (>= 5.1, game gets TRUE) | DDDA.exe's manifest has no `supportedOS`, so Windows 10/11 answer as 6.2.9200 (Microsoft Learn; measured with an exe that has the same kind of manifest). Mask arithmetic and the major/minor/SP hierarchy were measured; 24 hierarchy cases are replayed in a test. A mask without bit 63 stops. |
| | DebugBreak, RaiseException | 0x00DB9767; `__CxxThrowException@8`, `__raise_exc_ex`, delay-load helper | Stop with a report: caller, code (C++/delay-load named), flags, arguments. |
| | PSAPI GetModuleFileNameExA | thunk 0x012E1B18 from 0x011797F0 | This process and the exe only; other modules stop. |
| | WININET InternetGetConnectedState | 0x00D2A3A1 | Not connected: FALSE, flags 0. The game reads only the result and `flags & 3`. |

Shared files changed (small): `services.rs` (State fields `system`,
`registry`, `environment_blocks`; bind list), `lib.rs` (modules),
`win32.rs` (GetStdHandle reads SetStdHandle and rejects invalid ids;
GetEnvironmentStringsW returns a fresh copy of the guest environment,
which FreeEnvironmentStringsW frees), `files.rs` (home root, `Place`,
six services), `tests/runtime.rs` (14 tests).

Open:
- The CPU registry values are one recorded machine. CPUID is not yet
  modelled (lifter rejects it). When it is, it should report the same
  processor.
- Locale/sort behaviour beyond the 1252 repertoire and word sort's
  special weights stop rather than answer. Nothing in the game is known to
  reach them.
- Policy question for head: `text/tables.rs` holds 452 best-fit mappings
  and per-character data measured from Windows (operating-system data,
  not game data).

Tests: `cargo test -p ddruntime --test runtime` (through `heavy.py`,
jobs 2, incremental off): 38 passed, 14 of them new
(`multi_byte_to_wide_char_follows_windows`,
`wide_char_to_multi_byte_follows_windows`,
`string_types_case_mapping_and_sort_keys_follow_en_us_windows`,
`conversions_and_sort_keys_match_recorded_windows_vectors`,
`time_zone_and_file_times_follow_windows`,
`global_and_local_memory_follow_windows`,
`the_environment_starts_empty_and_keeps_windows_order`,
`standard_handles_and_the_absent_console_follow_windows`,
`version_checks_answer_as_windows_does_for_an_exe_without_supported_os`,
`debug_break_and_raise_exception_stop_with_a_report`,
`psapi_module_name_and_wininet_state`,
`the_registry_answers_the_recorded_processor_key`,
`profile_files_read_and_write_as_windows_does`,
`delete_move_copy_and_truncate_never_touch_the_install`,
`every_l17_import_is_bound_to_a_service`). With
`DDRUNTIME_WINDOWS_VECTORS` set, the vector test checked the full
recorded set and passed. Not done: no boot run reaching these call sites
(`examples/winmain` still stops earlier, at the CRT's `__snprintf_s`), so
the services are tested against Windows' behaviour, not yet exercised by
the game itself.

(to be filled by the owner)

### Parts B and C (claude-3, branch `task/L17-seh-workers`)

Recorded against the original exe under the contracts below. Every addition
must also pass the required-vector Rust replay (see Verification). All data
stays private in `E:/ddda-re/notes/native-L17`.

| Measure | Before | After |
|---|---:|---:|
| Selected lifted entries | 58,913 | 58,989 |
| Required lifted cases | 175,425 | 175,600 |
| Missing candidates (liftable, no fixture) | 495 | 418 |
| Lift rejections | 157 | 157 |
| Quarantined | 10 | 11 |

Canonical per-record hashes (`L17BC/audit.py`) confirm that all 58,913
prior records and 175,425 prior cases are unchanged; the 76 functions and
175 cases are appended.

**Replay quarantine.** 77 functions recorded; the first required replay
failed exactly one, 0x0061FDA0 (objects-mockzero-v1): `call target`
0x0062019C versus recorded 0x00626400. Its switch `jmp [eax*4+0x6201C0]`
at 0x0061FDB5 has `push ebx; push edi` between the `cmp eax,8; ja` bound and
the jump, so the strict recognizer keeps a 3-entry table; the ext fallback
then sent entry 3 (0x0062019C, a body instruction with its own arm) through
`call_ext` while the recorder (and the CPU) continue locally. Not caused by
these strategies: a lifter emission gap, now fixed by claude-6 on
`task/L22-lift-rejections` (dispatch set = table targets plus all block
leaders; body key unchanged). 0x0061FDA0 is quarantined with its record,
cases and evidence (`structured_quarantine.json`, `quarantine_evidence.json`,
`L17BC/quarantine-0061FDA0.json`) until that lifter lands.

**Part B: the caller-frame contract.** `harness.frame_exit` derives a
call-free function's exit ESP from its own code (symbolic entry ESP/EBP plus
constants plus or minus entry stack dwords read before any write; every
`ret` must agree; calls, indirect/outward jumps, traps, `leave`/`and esp`,
aliasing stores before a stack load all give None). When the result is
frame-relative (EBP-based or with slot terms), `run_sparse` checks the
observed ESP exactly against it (slot values from the case's own input
bytes) instead of the 4..0x404 range, and tags the case
`stack_contract: {rule: caller-frame-v1, exit_esp}`. The own-return check
(the `ret` must reach the sentinel) and every other guard are unchanged;
plain `ESP + constant` exits keep the range check even outside it. This was
tightened during the run: misdecoded data at 0x004E4843 (`... ret 0x57e`,
exit ESP + 0x582) first passed under a looser draft, and its trial record
was discarded before adoption. It is rejected again. Among the 495
candidates exactly three bodies have frame-relative exits, and they record
under the ordinary `random` strategy (3 seeds each):

| Address | Function | Derived exit ESP |
|---|---|---|
| 0x012F71E0 | `__SEH_prolog4` | `esp - 0x18 - [esp+0x8]` |
| 0x012F7225 | `__SEH_epilog4` | `ebp + 0x4` |
| 0x01306F7F | `__EH_prolog3` body (inventory `__EH_prolog3_catch`) | `esp - 0x18 - [esp+0x4]` |

These are the relations L15 measured. No address appears in the rule.
Replay needed no change: it already compares all eight registers. I chose
the exact derived relation over paired prolog/epilog fixtures because pairs
would need a synthetic caller, which is not a lifted function the per-function
replay can run. The helpers' callers are unchanged (still mocked as before).

**Part C: zero-return strategies.** `mockzero-v1` keeps the default random
pages and registers and makes every mocked callee return 0 (EAX, ECX, EDX,
xmm0, st0 push; the inferred pop is unchanged). The resource worker showed
that random memory alone cannot end some of these loops. 0x00DBA9A0 shifts
its queue in a loop bounded by the count at `[esi+0x150F8]` (0x00DBAA22..34),
and with random memory that is a 32-bit count. Its only exit needs the quit
byte `[esi+0x140E0]` set while the count is 0. So the same zero-return rule
is offered over the two existing bounded-count page distributions:
`small-mockzero-v1` and `objects-mockzero-v1`. Each is identical to
small-v1/objects-v1 up to the first mocked call. All three are versioned,
separate trial logs.

- Renderer worker **0x00DAC090**: 3/3 seeds under `mockzero-v1`. The wait
  (IAT 0x0139D1A4) returns 0, ResetEvent, then the quit byte at
  `[[0x018D08F4]+0xDE]` is nonzero and the thread returns (2 mocked calls).
- Resource worker **0x00DBA9A0**: no case in any 3-seed pass. In the
  12-seed pass, seeds 9 and 10 of `objects-mockzero-v1` record, with 29 and
  265 mocked calls (queue pop, item processing through the vtable,
  MtFileEx, release, back to the wait, then exit on the quit byte).

Passes (each over all remaining candidates, at most six workers):

| Pass | Seeds / budget | Tried | Added functions | Cases |
|---|---|---:|---:|---:|
| random (frame contract) | 3 / 20 s | 495 | 3 | 9 |
| mockzero-v1 | 3 / 20 s | 492 | 13 | 39 |
| objects-mockzero-v1 | 3 / 20 s | 479 | 32 | 68 |
| small-mockzero-v1 | 3 / 20 s | 447 | 3 | 4 |
| objects-mockzero-v1 | 12 / 60 s | 444 | 19 | 44 |
| mockzero-v1 | 12 / 60 s | 413 | 3 | 6 |
| small-mockzero-v1 | 12 / 60 s | 407 | 3 | 5 |
| zero, small-v1, objects-v1, zero-objects-v1, image-v1 | 3 / 20 s | 419..418 | 1 (zero) | 3 |
| **Total recorded** | | | **77** | **178** |
| Replay quarantine (0x0061FDA0) | | | -1 | -3 |
| **Net** | | | **76** | **175** |

Attribution, honestly (before the quarantine, which removes one of the
mocked-call group): of the 73 zero-return additions, 36 have mocked calls
in their cases, so they depend on the zero returns. The other 37 (34 under
objects-mockzero, two under small-mockzero, one under mockzero) made no
mocked call. Those runs are identical to their page strategy's, and they are
mostly address-taken supplementary entries (066d970) that had only ever been
recorded with `random`. The final historical-strategy sweep added one more
(`zero`). The 12-seed passes are the same uniform rule for every remaining
candidate, not a per-address retry.

Remaining 418 candidates, primary reason under mockzero-v1: execution
outside the image 170, NaN/Inf float input 84, runaway 72, over 256 pages 14,
noreturn call 13, noreturn target 5, large memset 6, undefined flags 4,
overwritten return 4, two NaNs 4, unmasked x87 2, other 40.

**Verification.** All 70 native Python tests pass. New: `test_frame_contract.py`
(derived prolog/epilog expressions; ordinary, call, `and esp`, `leave`,
disagreeing-return, aliasing and written-slot bodies give None or ordinary;
out-of-range ordinary exit still rejected; recorded exact check; wrong
return target still rejected) and the zero-return checks in
`test_structured_inputs.py` (identical inputs to the base strategy, zero
returns including st0 push, the wait loop terminates where random runs away).
The final required-vector replay
(`DDNATIVE_REQUIRE_VECTORS=1 cargo test -p ddnative --test lifted --test units`,
`CARGO_BUILD_JOBS=4`, `CARGO_INCREMENTAL=0`, through `heavy.py`) passes:
**every one of the 175,600 cases over 58,989 lifted entries**, plus the 7
unit tests. The lifted replay took 76.7 s (build 1m20s incremental). An
earlier full build was killed by the machine's memory guard while a game
held 9 GB; it was not a code failure. Private inventory rebuild
(`inventory.py build` on native-L17's database):

| Inventory | Before | After |
|---|---:|---:|
| Verified functions (of 59,290) | 58,628 (98.9%) | 58,704 (99.0%) |
| Verified bytes (of 15,677,160) | 15,049,828 (96.0%) | 15,106,343 (96.4%) |
| CRT/library-range rule verified | 5,447 / 5,588 | 5,454 / 5,588 |

All four verified CRT 'platform' rows (`vf20` 0x0130A800, `vf31`
0x0130AAC0, 0x013201C0, 0x01320700) are among the additions. The `platform`
and `documented` status counts also move because docs now cite addresses;
those are status labels, not verification.

Compared against the original: the recorded vectors only (Unicorn running
the original instructions, under the input contracts above). No live-game
or boot comparison. The two worker loops are verified as bodies with mocked
Windows calls. Running their threads is the runtime's job.

**Adoption (head).** Copy the matching private pair
`E:/ddda-re/notes/native-L17/patterns/lift.json` and `vectors/lift.json`,
plus `patterns/lift_novector.json` and `lift_novector_keys.json` (accepted
addresses pruned), into the shared set. Merge this branch's recorder before
any re-recording. Do not rescan first. Then `lift.py gen`, the required-vector
replay, and `scripts/inventory.py build` / `summary` with the shared
`INVENTORY_DB`. No supplementary-inventory change. Evidence:
`structured-*.jsonl`, `structured-*-adopted.json`, `L17BC/` (logs, traces of
both workers, baseline hashes, the pre-fix random log). Generated Rust is
not committed on this branch.
