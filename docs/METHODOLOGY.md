# Method

Adapted from nv-rs's `docs/METHODOLOGY.md` (Fallout: New Vegas in Rust),
which surveyed about 35 comparable projects. Its conclusion holds here:
the fastest faithful route is **decompilation-informed reimplementation
checked against the original**. It is not "decompile everything, then
port it", and it is not matching decompilation.

## The loop

1. **Scope by route.** Each milestone names what the player does (e.g.
   "wake in Cassardis, walk to the beach"). Only code that route executes
   gets read and rewritten.
2. **Names first.** MT Framework carries its own reflection system (MtDTI):
   about 3,500 class names (`sPawnManager`, `uPlayer`, `cAIFSM`...), with
   sizes, parents and reflected properties (`mPawnID`, `mpSession`...).
   Labelling vtables and constructors from the DTI registry turns "find the
   code for X" into a name search. This is the single biggest accelerator
   and comes before reading individual functions.
3. **Data before code.** Much of the game is data: `.arc` archives holding
   meshes, textures, animations, materials, collision, text, and serialised
   DTI objects (XFS) for parameters and AI. Reading the data often answers a
   question without touching the exe (dishonored-bevy's lesson).
4. **Read the exe for rules.** Ghidra through the MCP server for formulas,
   constants, branch conditions, state machines and frame order. Check the
   disassembly where float code or the decompiler looks odd.
5. **Write Rust**, into headless core crates, with a regression test.
6. **Check against the original** using the cheapest oracle that answers
   the question:
   - side-by-side play and screenshots
   - Cheat Engine or x64dbg for live values and breakpoints
   - an injected probe DLL logging function arguments and state (nv-rs's
     `nv-probe`, which needs a 32-bit Rust target)
   - RenderDoc or apitrace frame captures for rendering (D3D9)

## Binary identity

| | |
| --- | --- |
| Build | Steam, PE timestamp 1513180209 (2017-12-13), 32-bit, base 0x400000, LAA |
| Original SHA-256 | `19facc2642f79a09d14ce55eace5f228161f2f8bab60ec67eb3c6196cc6277b8` |
| Analysis copy | SteamStub 3.1 removed with Steamless 3.1.0.5 on a private copy |
| Unpacked SHA-256 | `a1831a438685c50ca530bbb91dc4cbdc36449ffa7d9183e345a8a106782286b5` |
| Imports | D3D9 + D3DX9_43, DINPUT8, XINPUT1_3, WS2_32, WININET, steam_api |

Every address in this repo refers to this build.

## Reading tables from the installed exe

Bulk tables that the exe holds as data (skill records, name tables, action
tables, event tables) are never copied into the repo (constants policy in
`AGENTS.md`). Code reads them at run time from the player's own
`DDDA.exe` through the `ddexe` crate:

- `ddexe::locate(native_pc)` finds `DDDA.exe` next to `nativePC`;
  `ExeFile::open` / `ExeFile::next_to` read it.
- `Exe::parse` refuses every build but PE timestamp 1513180209, with a
  message naming both builds' dates. Addresses are the build's virtual
  addresses, as everywhere in the repo.
- `Exe::bytes(va, len)` maps an address to file bytes and refuses ranges
  that leave the file-backed part of a section (the zero-filled tail of
  `.data` is not in the file). Typed reads build on it: `u8`..`f32`,
  `u16s`/`u32s`/`f32s`, `cstr`, `record` / `records` with typed field access.
- `Exe::class_names()` names reflected classes: the `MtDTI` objects live in
  uninitialised `.data`, so the names come from the registration calls
  `push name; mov ecx, dti; call 0x00cf5780` in `.text`.

The installed exe is wrapped by SteamStub, but `.text`, `.rdata` and the
file-backed `.data` have the same bytes as the unpacked analysis copy
(compared for every table read so far), so no unpacking is needed. Each
reader names its table's address and its exe user, and has an install test
that skips when the game is absent (`DDDA_NATIVE_PC`, `DDDA_EXE`).

## Tools

- Ghidra 12.1.4 with JDK 21; the project is `E:\ddda-re\ghidra\DDDA.gpr`.
- GhidraMCP 6.0.0 (bethington/ghidra-mcp, Apache-2.0), built locally,
  headless server on 127.0.0.1:8089, Python bridge in its own venv.
- Cheat Engine, x64dbg, RenderDoc: installed when a question needs them.

## What not to do

- Bulk-translate decompiler output. Without an oracle it is wrong too
  often, and it mirrors a C++ object model that fights Bevy.
- Commit anything from `E:\ddda-re`, or game data.
- Aim for coverage numbers. A route that plays correctly beats a thousand
  unverified functions.
