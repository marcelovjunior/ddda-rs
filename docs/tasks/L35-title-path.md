# L35: the path from WinMain to the title screen's first frame (research map)

- **Owner:** claude-7
- **Branch:** `task/L35-title-path` (worktree `E:\ddda-rs-wt\L35`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md). Research only (Ghidra and
  the lifted sources); CPU-light.

## Why

claude-2 (L21) drives the boot stop by stop. Each stop is found only when
the run reaches it. A map of what the game does between WinMain and the
first Present of the title screen lets every owner prepare ahead of the
boot instead of after it.

## Task

1. **Trace the start-up sequence** from WinMain (start from FRAME.md
   section 1, docs/RUNTIME.md and L21's boot log), through `sApp`/`sMain`
   construction, the singletons (`sBbsRpgMain`'s constructor 0x0041B400),
   the device and window setup, and resource loading, up to the title
   scene's first frame and Present.
2. **For each step, list:**
   - the imports and COM methods it calls (D3D9, XAudio2, DirectInput,
     Steam, files, threads);
   - the files it opens (archive names, config.ini, shader packages);
   - the threads it starts;
   - the lifted functions it needs.
   Mark each item as:
   - *covered*: the runtime implements it, with a test;
   - *missing*: give the owner (claude-2 runtime, claude-5 D3D9, claude-4
     audio/network, claude-10 options/Steam, Codex SEH, claude-3/claude-6
     lifter);
   - *unknown*.
3. **Cross-check the lifter side:** every function on the path should be
   verified in the inventory (`scripts/inventory.py`). List any that are
   not.
4. **Write `docs/TITLE_PATH.md`:** the ordered map with addresses, the
   *missing* list grouped by owner, and the risks (long waits, thread
   handshakes, device-lost paths). Send each owner their part with
   SendMessage.

## Results

(open)
