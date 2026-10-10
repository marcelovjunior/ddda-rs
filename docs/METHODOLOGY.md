# Method

Adapted from nv-rs's `docs/METHODOLOGY.md` (Fallout: New Vegas in Rust),
which surveyed about 35 comparable projects. Its conclusion holds here:
the fastest faithful route is **decompilation-informed reimplementation
checked against the original**. It is not "decompile everything, then
port it", and it is not matching decompilation.

## Systematic rebuild (adopted 2026-10-10)

The route-driven method (trace what the opening needs next, translate it,
fill gaps with stand-ins to keep the route green) produced traced pieces
but a game full of gaps: intro bypasses, scripted host shortcuts, NPCs that
stop on untranslated variables. From now on the project follows the nv-rs
approach: map the whole executable, rebuild it in dependency order, accept
a piece only when it is traced and tested against the original, and wire
pieces to the game state last. Playthroughs become acceptance tests.

**Phase 0, the map (done 2026-10-10).** `research/ghidra/ExportInventory.java`
exports every function and call edge from the DTI-labelled Ghidra project
(headless, read-only); `scripts/inventory.py build` loads them into
`E:/ddda-re/notes/inventory/inventory.sqlite` (private: derived from the
exe) and marks each function the Rust code cites by address. First count:

| status | functions | share | code bytes |
|---|---:|---:|---:|
| translated (cited in Rust; upper bound, some are partial) | 1,701 | 3.4% | 8.5% |
| documented only | 762 | 1.5% | 2.8% |
| untouched | 48,015 | 95.1% | 88.7% |
| total (thunks excluded) | 50,478 | | 15.1 MB |

`python scripts/inventory.py summary` prints the current numbers;
`python scripts/inventory.py next N` lists the untranslated functions most
called by translated ones (the dependency frontier).

**Phase 1, the frame.** The main loop, its job/move lines and pass order
(FRAME.md), rebuilt exactly, with every system an explicit slot, so that
pieces plug into the original's order instead of ad-hoc host code.

**Phase 2, the core in dependency order.** Runtime base classes first
(the tracker's frontier: memory/containers, MtObject/DTI, resources, unit
and model bases, actions, collision, the AI core), then families on top.
Each function moves `untouched -> translated -> verified`: verified means a
differential test against the original function (the bounded emulation
harness the agents already use: the original's instructions on original
pages, synthetic inputs, outputs compared with the Rust port) or a live
oracle reading. Stand-ins are not allowed: a missing piece is an explicit
"not yet", never a shortcut that makes a route pass.

**The 1:1 pipeline (first run 2026-10-10).** A function moves to
`verified` only through these steps:
1. pick it from the frontier (`inventory.py next`); skip platform-layer
   code (allocators such as 0x00CF5E20 / 0x00DE0B80, the CRT, threads,
   device I/O), which Rust replaces rather than translates;
2. disassemble the original (`research/native/harness.py disasm`);
3. port the WHOLE function into `crates/ddnative`: original branch order,
   state as named fields at the original offsets, every callee it does not
   own behind an explicit trait (e.g. `player_action::Effects` for the
   effect release 0x00427560), no stand-ins;
4. record vectors from the original instructions in Unicorn
   (`research/native/vectors_*.py`, private output under
   `E:/ddda-re/notes/native/vectors`), including the paths that reach an
   unowned callee (the vector records the arguments it was reached with);
5. replay them in `crates/ddnative/tests/differential.rs` (skips when the
   vectors are absent) and run `inventory.py build`: ddnative citations
   count as `verified`.

Known emulator limit: Unicorn keeps a signalling NaN through x87
`fld`/`fstp`; real hardware quiets it (checked on the maintainer's CPU).
x87 paths use quiet NaNs in the vectors, and `ddnative::x87_copy` is tested
against the CPU itself with inline assembly.

First ports: `cPlAction::vf06` (0x00AC5100, the base update of every player
action: 720 vectors, 120 through the effect release) and `uObjModel::vf85`
(0x004C6B50, world origin: 25 vectors).

**Families.** Many functions share one body except a constant (the action
in-place constructors differ only in their vtable). One parameterized port
verifies a whole family, but membership must be **byte-exact except that
constant**: a looser shape match admitted 15 constructors writing a float
at another offset, and only the per-member differential test caught it.
Each member is replayed individually; a private `families/*.json` lists
the members and the tracker credits them when the representative is
ported. Commit only behind a passing gate (`cargo test -p ddnative`
succeeding), never chained after it unconditionally.

**Phase 3, wiring.** Connect the rebuilt pieces to the host's game state;
the hand-written glue (ScriptedHost shortcuts, the demo driver, route-only
adapters) is retired as the rebuilt systems take over.

**Acceptance.** The route checkpoints (S46) and playthrough tests prove
behaviour against the original; S60 (oracle numbers) becomes standard.

**Kept from the route era:** format readers, renderer/platform layer
(windows, input, audio output, network transport), research docs and the
tools (oracle, board, checkpoints, emulation harness).

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
