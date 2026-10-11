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
