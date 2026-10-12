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

### Evidence and census

Read-only Ghidra decompilation and Capstone disassembly of DDDA.exe
2017-12-13 Steam. Ghidra loses several COM stack arguments; the census
uses the actual pushes and the SDK vtables. Private exports/disassembly:
`E:/ddda-re/notes/runtime-L18/`. No canonical project changes.

**Correction:** reader `+0x28` is `SetReadStreamSamples`, not
`SetStreamsSelected`. `0x00F7DEB0` requests uncompressed video/audio
(`FALSE`). Playback discovers outputs directly; `IWMProfile` and
`IWMStreamConfig3` belong to the separate capture path.

All offsets below are x86 vtable byte offsets. COM calls use stdcall,
including `this`. The guest checks failed HRESULTs unless noted.

| Interface/method | Call-site evidence | Arguments and result use |
|---|---|---|
| `WMCreateSyncReader` / `IWMSyncReader::OpenStream +0x5C` | `0x00F7DEB0` | `(NULL, rights=1, &state+0x1F8)`; opens guest stream at state+0x58, failure aborts |
| `GetOutputCount +0x38` / `GetOutputProps +0x3C` | `0x00F7CA57`, `0x00F7CACE` | DWORD output count, one `IWMOutputMediaProps` per output |
| `IWMMediaProps::GetMediaType +0x10` | `0x00F7CA40` two calls; `0x00F7BBC4`, `0x00F7BC0E`, `0x00F7BD0B`, `0x00F7BD37` | query size with NULL, allocate, fetch header and format block |
| `SetOutputSetting +0x34` | `0x00F7BB4B`, `0x00F7BB71`, `0x00F7CB7E` | audio `EnableDiscreteOutput` BOOL=1, `SpeakerConfig` DWORD=6; video `VideoSampleDurations` BOOL=1 |
| `GetOutputFormat +0x48` / `GetOutputFormatCount +0x44` | `0x00F7BB8E`, `0x00F7BCF5`, `0x00F7BC96` | first audio format establishes channels, then enumerate candidates |
| `SetOutputProps +0x40` | `0x00F7BBAA`, `0x00F7B2C1`, `0x00F7B385` | first audio format, then selected video/audio properties |
| `GetStreamNumberForOutput +0x50` | `0x00F7B2FA`, `0x00F7B3B1` | WORD stream number stored as DWORD at state+0x1EC/video or +0x1F0/audio |
| `SetReadStreamSamples +0x28` | `0x00F7DEB0`, after negotiation | selected WORD stream and FALSE: decode samples |
| `QueryInterface` / `IWMHeaderInfo::GetAttributeByName +0x14` | `0x00F7CBAB`, `0x00F7CBE0`; IID at `0x014E764C` | `Duration`, NULL stream-number pointer, 8-byte QWORD; convert 100 ns ticks to seconds at state+0x28; failure leaves zero |
| `SetRange +0x14` | `0x00F7B460`, `0x00F7C2A4` | two 64-bit arguments, 20-byte cleanup including this; loop reset `(0,0)`, seek from state+0x208 |
| `GetNextSample +0x1C` | `0x00F7C389` | stream=0/interleaved, sample**, QWORD time/duration*, DWORD flags/output*, WORD stream*; EOF `0xC00D0BCF` ends or loops |
| `INSSBuffer::GetBuffer +0x18` / `GetBufferAndLength +0x1C` | `0x00F7B530` video/audio branches | split video into Y,V,U planes; flag bit4 means unusable frame; PCM pointer/length goes to `0x00DC9A30`, timestamps converted to ms |
| `Release +8` | `0x00F7B170`, `0x00F7C464`, negotiation cleanup | reader, stream, props and each sample; ordinary teardown releases reader without explicitly calling Close |

Selector `0x00F7B250` requires uncompressed video GUID `73646976-...`
(`0x014E759C`), subtype **YV12** `32315659-...` (`0x014E75CC`),
`FORMAT_VideoInfo` (`0x014E761C`). It reads width/height at format+52/+56
and average frame duration at +40. Audio uses `73647561-...`
(`0x014E75DC`), PCM (`0x014E75EC`), `FORMAT_WaveFormatEx`
(`0x014E762C`), **48,000 Hz**, at least **16 bits**, and channels obtained
from the first discrete format. These are guest choices, not host defaults.

### Guest IStream

Constructor `0x00F7DFE0` installs vtable `0x01436E78`. `0x00F7E090` supplies
archive memory/length at +0x144/+0x148 (cursor +0x14C), or a file wrapper
at +0x13C opened by `0x00F7B970`.

| Slots | Target and contract |
|---|---|
| 0 QueryInterface | `0x00F7B690`: IUnknown/ISequentialStream/IStream IIDs at `0x014612F0`/`0x014612D0`/`0x014612E0`; same pointer, AddRef |
| 1 AddRef / 2 Release | `0x00F7B650` / `0x00F7B660`: counter +4, destroy at zero |
| 3 Read | `0x00F7B700`: memory/file read, advances cursor, writes pcbRead; exhausted input returns S_FALSE without initializing pcbRead, so bridge initializes it |
| 5 Seek | `0x00F7B7D0`: 64-bit ABI, internally low DWORD movement; SET/CUR/END, optional QWORD position; memory cursor clamped to length |
| 12 Stat | `0x00F7B900`: STATFLAG_NONAME=1 only, zero 72-byte guest STATSTG, type2, size from +0x148 |
| 4/6/7/8/9/10/11/13 | `0x00F7B8B0`/`0x00F7B8D0`/`0x00F7B8F0`/`0x00F7B8C0`/`0x00F7B8E0`/`0x00F7B8F0`/`0x00F7B8F0`/`0x00F7B8C0`: E_NOTIMPL, respective stdcall cleanup |

Base resolver lacks AddRef/Release/Seek at those three addresses. Reported
to head/claude-3, board #9118. No production replacement of guest bodies;
fixture and original-IStream integration probes are separate.

### Writer/capture (not playback)

`0x00F97AAF` calls `0x00F95320` only when `sCapture+0xC8 == 3`;
`0x00F94B60` chooses `.wmv` for that case. Live reachability during ordinary
startup is **unconfirmed**. The recording census is:

| Interface | Methods, sites and use |
|---|---|
| creation exports | writer `0x00F95349` -> state+0x6C; profile manager `0x00F95363` |
| `IWMProfileManager` | `CreateEmptyProfile +0x0C` at `0x00F9537C`, WMT_VER_9_0=0x90000 |
| `IWMProfile` | `SetName +0x14` at `0x00F9538D`; `CreateNewStream +0x3C` at `0x00F953A3` (video); `RemoveStream +0x2C` at `0x00F9577C`, `AddStream +0x34` at `0x00F95878` |
| `IWMStreamConfig3` | QI `0x00F953B8`, IID `0x014E765C`; `SetBufferWindow +0x34` at `0x00F9556D`; `SetStreamNumber +0x14` at `0x00F9578A` (1), `SetStreamName +0x1C` at `0x00F9579B`, `SetLanguage +0x54` at `0x00F957AC`, `SetTransportType +0x3C` at `0x00F957B9` (0) |
| `IWMVideoMediaProps` / `IWMMediaProps` | QI `0x00F9571B`, IID `0x014E763C`; `SetQuality +0x24` at `0x00F95729` (100); `SetMaxKeyFrameSpacing +0x1C` at `0x00F95743` (0 or 30,000,000 ticks); `SetMediaType +0x14` at `0x00F95757`, RGB24/IYUV/WMV3/WVC1 branches |
| `IWMPropertyVault` | QI `0x00F957E3`, IID `0x014E766C`; `SetProperty +0x14` at `0x00F9580D`, `0x00F95825`, `0x00F9583E`, `0x00F95857`, encoder properties in WMV3/WVC1 branches |
| `IWMWriter` | `SetProfile +0x10` at `0x00F95888`, `SetOutputFilename +0x14` at `0x00F95905`, `GetInputProps +0x1C` at `0x00F95AEF` (input0), `SetInputProps +0x20` at `0x00F95B14`, `BeginWriting +0x2C` at `0x00F95B33` |
| `IWMInputMediaProps` | `SetMediaType +0x14` at `0x00F95B03`; Release at `0x00F95B24` |
| writer/sample | `0x00F949D0`: `AllocateSample +0x34`, buffer `GetBuffer +0x18`, write converted bytes, `WriteSample +0x38` (input0, QWORD timestamp), Release buffer |
| writer teardown | `0x00F97440`, capture mode3: `EndWriting +0x30`, Release and clear state+0x6C |

Writer/manager creation returns host COM objects with guest IUnknown
lifetime/identity and named unbound recording slots. Encoding, profile
construction, property vault and host output filenames are not implemented;
unknown methods stop through the dispatcher rather than claim success.

Capture strings at `0x01437B28`/`0x01437B40`/`0x01437B58`/`0x01437B64`
name `_VBRENABLED` (BOOL1), `_VBRQUALITY` (DWORD state+0x74), `_RMAX`
and `_BMAX` (DWORD0). `0x01437C04` supplies language `ja`; stream/profile
names at `0x01437C0C`/`0x01437C28` are `Video Stream`/`Custom Profile`.

### Implementation

`movies.rs` registers all three exports in the existing dynamic export map.
`Session::new` allocates their tokens through the interface token allocator.
The guest delay IAT is not shortcut-patched: descriptor `0x0161EEE4`
(RVA 0x121EEE4) has slots `0x018765C0` manager, `0x018765C4` writer,
`0x018765C8` reader, thunks `0x012ED143`/`0x012ED153`/`0x012ED133`.

A per-session `Decoder` trait separates the guest ABI from Windows. The
system adapter owns COM objects on one MTA thread and drives installed
64-bit WMVCore. Its read-only IStream sends read/seek/stat requests back to
the service thread, which calls the guest dispatcher without holding a
runtime RefCell borrow. Transfers are split into 64 KiB chunks; the runtime
does not preload movies. Returned media headers use the 72-byte guest
layout and guest format pointers. Samples own guest bytes through final
Release. Reader/header IUnknown identity is shared; the source stream is
AddRef'd through Close/final Release. Host pointers never enter guest memory.

SDK references: [IWMSyncReader](https://learn.microsoft.com/en-us/windows/win32/api/wmsdkidl/nn-wmsdkidl-iwmsyncreader),
[uncompressed subtypes](https://learn.microsoft.com/en-us/windows/win32/wmformat/uncompressed-media-subtypes),
Microsoft's [GetMediaType contract](https://github.com/MicrosoftDocs/sdk-api/blob/docs/sdk-api-src/content/wmsdkidl/nf-wmsdkidl-iwmmediaprops-getmediatype.md)
(`ASF_E_BUFFERTOOSMALL`, `asferr.h` value `0xC00D07D1`), installed
`wmsdkidl.h` and `windows` 0.62.2 bindings. System codecs are
host-backed, not translated or compared with pixels from a live original.

A direct read-only system-WMVCore probe on `title_B.wmv` found that
`GetAttributeByName(NULL, L"Duration", ...)` returns `E_INVALIDARG`
(`0x80070057`), whereas a pointer to WORD stream zero returns QWORD
599,990,000 ticks. A seven-byte value buffer returns
`ASF_E_BUFFERTOOSMALL` and required length eight. The bridge preserves
NULL rather than silently changing the game's request; `0x00F7CBE0`
already tolerates this failure. This is a host observation, not a live
original-game comparison.

### Before/after and validation

| Runtime movie boundary | Before | After |
|---|---:|---:|
| Resolvable WMVCore delay exports | 0 | 3 |
| Bound COM slots across the six movie object kinds | 0 | 39 |
| Host decoder implementations | 0 | 1 (Windows system WMVCore) |
| Passing default runtime tests | 36 | 39 |
| Installed WMVs decoded through the guest movie boundary | 0 | 6 |

The 39 slots include IUnknown on writer/manager; their encoding methods
remain named unbound entries. Native lift/inventory data was not changed.

`cargo check -p ddruntime --tests` passes. A private native-independent
probe compiled the same Rust Windows adapter and decoded eight frames
from each of the six installed WMVs through its IStream callback channel:
1280x720 YV12, 1,382,400 bytes/frame, timestamps 0 through 2,330,000 ticks,
durations 330,000 or 340,000 ticks with `VideoSampleDurations` enabled.
It also verified successful reopening after rewinding the source. The
guest boundary retains old/new stream references during replacement and
releases both on a failed open (the host backend closes that reader).

**Tested:** `cargo test -p ddruntime` passed: **39 tests**, zero failures,
three ignored integration tests (13 input/audio, three movie, 23 runtime
tests). The movie tests cover stdcall cleanup, 32-bit media headers and
guest pointers, QWORD timestamps/ranges, WORD outputs, HRESULTs, metadata
size queries, shared IUnknown identity, sample ownership, stream callbacks,
short reads/EOF, reopening, failed-open cleanup and final Release.

The ignored `installed_movie_delay_and_guest_stream` test was then run
explicitly against **all six installed files**, all passing:

| Movie | Video samples checked | Audio checked | Run time |
|---|---|---|---:|
| `title_B.wmv` | eight opening YV12 frames | no audio output | 7.32 s |
| `st100ev00.wmv` | eight opening frames, eight after seeking to 3 s | 48 kHz / six channels / 16-bit PCM | 13.58 s |
| `st230ev00.wmv` | eight opening YV12 frames | same PCM format | 13.00 s |
| `st240ev05.wmv` | eight opening YV12 frames | same PCM format | 18.20 s |
| `st610ev21.wmv` | eight opening YV12 frames | same PCM format | 18.80 s |
| `title_A.wmv` | eight opening YV12 frames | no audio output | 13.07 s |

Each run traverses all three **original delay thunks**, checks their IAT
slots resolve to registered service tokens, and uses **lifted selector
`0x00F7B250`** for YV12 and PCM negotiation. The IStream fixture stores its
data/cursor in guest memory and is called through the guest dispatcher;
the system adapter never receives the filename. Checks include frame
size, advancing timestamps, positive durations, variation within the Y
plane, PCM block alignment (first sample 49,152 bytes at time zero),
interleaved `GetNextSample(0)` stream/output mapping, and stream lifetime.
`st100ev00` has a measured three-second black lead-in; checking only Y
versus neutral chroma would not have established nontrivial pixels.

**Not passed / prerequisite confirmed:** the separate
`installed_movie_original_stream` probe exits 101 before opening a movie,
listing exactly `0x00F7B650`, `0x00F7B660`, `0x00F7B7D0` as missing native
entries. Thus the six successful tests validate the guest-dispatched
fixture stream, not the original stream implementation or an entire live
uMovie session. No pixels/audio were compared against a live original.

Reproduce with `CARGO_BUILD_JOBS=4`, `CARGO_INCREMENTAL=0`:

```powershell
cargo test -p ddruntime
$env:DDDA_EXE = 'E:/SteamLibrary/steamapps/common/DDDA/DDDA.exe'
$env:DDDA_MOVIE = 'E:/SteamLibrary/steamapps/common/DDDA/nativePC/movie/title/title_B.wmv'
cargo test -p ddruntime --test movies installed_movie_delay_and_guest_stream -- --ignored --nocapture
```

The successful exclusive build/test sequence took **803.49 s**, including
a cold test-profile dependency pass with two shard targets per Cargo
invocation, both test commands and all six movie runs. Minimum observed
free RAM was **10.34 GiB**. An earlier four-shard cold attempt was stopped
at **5.90 GiB** free; interrupted attempts were not counted as validation.
Logs/measurements remain private in `E:/ddda-re/notes/runtime-L18/`.
No lifter, generated program, native data, game-install or D3D9 files were
changed. L16 was merged into this worktree in `219e491` before L18 work.

**Next action / open scope:** add the three original stream entries through
the native workflow, then rerun the original-IStream probe and integrate
uMovie playback in the running session. Capture encoding remains unbound;
ordinary-startup reachability of the capture branch is still unconfirmed.
