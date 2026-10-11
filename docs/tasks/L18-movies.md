# L18: movies (WMVCore) in the runtime

Owner: Codex, after L16. Branch `task/L18-movies` from the head's `main`,
worktree `E:\ddda-rs-wt\L18`.

## Why

The exe's only delay-loaded DLL is WMVCore.DLL (WMCreateSyncReader,
WMCreateWriter, WMCreateProfileManager; delay-import directory at RVA
0x121EEE4). The movie player `uMovie` (0x00F7DEB0, called from
0x00F7E090) creates an IWMSyncReader (`WMCreateSyncReader(0, 1, &reader)`),
opens the movie as a stream (vtable +0x5C, IWMSyncReader::OpenStream over an
IStream the game provides from its archive data), selects streams (+0x28),
and reads decoded frames into Y/U/V planes it allocates itself (uMovie DTI
allocator), which the renderer turns into a texture. WinMain also checks
that WMVCORE.DLL loads at all (0x004079F6). The opening movie and
cutscene movies (nativePC/movie) need this.

## Task

1. Census: every WMVCore interface and method the game calls (IWMSyncReader,
   IWMProfile/IWMStreamConfig/IWMMediaProps, INSSBuffer, the IStream the game
   implements and passes in, the writer/profile manager if reached), with
   call sites, arguments and what the game does with each result.
2. Bind the delay imports (the delay-load helper `___delayLoadHelper2`
   resolves through LoadLibrary/GetProcAddress, which the runtime answers
   from its import table; make the three WMVCore functions resolvable and
   verify the game's delay-load path reaches them).
3. Implement IWMSyncReader & co. as runtime interface objects (crate
   ddruntime: interfaces.rs conventions; a new movies.rs) backed by a host
   decoder behind a small trait: the Windows implementation drives the
   system's own WMVCore (64-bit COM from the runtime process) or Media
   Foundation, fed from the game's IStream (call back into guest code through
   the dispatcher to read the stream, like other guest callbacks), and
   returns decoded samples in the format the game asked for (check the
   output format it sets: YUV planes?). Document what is host-backed.
4. Tests: open one of the game's movie files from the install through the
   service and read the first frames; check sizes, timestamps and that
   frame data is non-trivial; the game-facing contract (results, out
   params, sample buffers) is tested like other services.

Do not touch the head's d3d9*/ddgpu files or other agents' modules (L16
input/audio is yours already; L17's text/registry/profile modules are
claude-2's).

## Rules

As before: CARGO_BUILD_JOBS=4, CARGO_INCREMENTAL=0, never two heavy jobs at
once (wait and poll, do not end the turn), no pushes, commit on your
branch, results below.

## Results

(to be filled by the owner)
