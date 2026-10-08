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
