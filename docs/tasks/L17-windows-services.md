# L17: the remaining Windows services, and the SEH frame contract

Owners: Part A, a second Claude Code agent (headless session "claude-2",
driven by the head like Codex), branch `task/L17-windows-services`,
worktree `E:\ddda-rs-wt\L17`. Part B, Codex after L16 (its own branch).

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

(to be filled by the owner)
