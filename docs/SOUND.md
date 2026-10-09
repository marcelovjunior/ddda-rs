# Sound resource readers

The new headless `mtsound` crate reads SPAC v12, SREQ v19 and STRQ v27.
It performs no playback, mixing, decoding or simulation. All executable
addresses below refer to the 2017-12-13 Steam build.

## Evidence and layouts

The PC loaders check the magic/version at `0x00e21af0` (SPAC),
`0x00f73f10` (SREQ), and `0x00dc0ef0` (STRQ). The following relocation
routines were inspected directly. The private MIT Riftstone format notes,
“Sound cues,” supplied initial leads; no implementation or game bytes were
copied. The reflected DTI layouts of rSoundPackage/rSoundStreamRequest expose
only inherited cResource fields, so unnamed serialized bytes remain raw.

| Format | Tables and links confirmed in executable |
| --- | --- |
| SPAC | `0x00e21600`: 32-byte header; wave/program/metadata counts at +8/+12/+16; program/metadata/sample offsets at +20/+24/+28. Programs stride20, metadata stride44; metadata +40 is a signed wave index; every negative value means absent. |
| SREQ | `0x00f735b0`: 52-byte header, +8 request count, requests immediately follow with stride144. Request +128 indexes the package-path offset array addressed by header+28; -1 means no package. Request+132 indexes the section table. |
| STRQ | `0x00dc0ef0`: 60-byte header; +8 source count and +12 request count, source/request offsets+32/+36. Sources stride24: absolute string offset followed by five raw words. Requests stride156: +140 source index, +144 section index; -1 means absent. |

Both request forms also carry four auxiliary tables with strides12,28,16,8.
Their offsets and counts are checked, as are the chained indices at row
offsets8,24,12. Their complete mixing semantics remain unresolved. Optional
random-resource string offsets are preserved; rSoundRandom itself is outside
this brief. SREQ's package-path array has no explicit header count: each
referenced entry and terminated string is individually checked.

### Split RIFF payloads

SPAC wave headers begin at byte32. A word of -1 represents an empty slot.
Otherwise a RIFF/WAVE header includes `fmt `, a `data` header, and possible
following chunks, but its sample bytes are in the separate sample section.
The loader advances the header cursor by `RIFF.size + 8 - data.size` and
the sample cursor by `data.size`. `Wave::riff()` reconstructs a conventional
container by inserting those bytes immediately after the data chunk header.
The corpus test checks every reconstructed chunk boundary.

`0x00e21490` also inspects the following `smpl` chunk: when loops exist it
reads start/end words and adds one to a nonzero end. This observation is
documented only; loop playback and all program/metadata semantics are not
implemented. Format tag, channel count, sample rate and bits/sample are
read from each wave's actual `fmt` bytes.

### Loose stream paths

`0x00dbffe0` prepends `sound\\stream\\` when absent and requests
rSoundSourceMusic. `stream_path` provides the corresponding install-relative
`.sngw` path, rejecting rooted paths and traversal. Existing prefix is
preserved. The extension is confirmed by the installed loose files rather
than guessed from the ARC type. `validate_vorbis` checks Ogg page bounds,
CRC, serial/sequence and the first Vorbis identification packet. It does
not decode packets or assert that audio sounds correct.

### Concrete chain

The SREQ at `sound\se\armor\arm_cha_cha\arm_cha_cha` contains 18
request records. Its package-path table resolves to the rSoundPackage at
the same path, which has two nonempty RIFF waves. The first wave is
format tag 2 (MS ADPCM), mono, four bits/sample. The STRQ at
`sound\stream\bgm\bgm` contains 125 sources and 171 request records;
source zero names `bgm\wave\No_001`. These are serialized links, not
a claim about when those cues play.

## Validation

Measured against the maintainer's install on 2026-10-09:
`cargo test -p mtsound -- --nocapture` passes six synthetic tests and one
installation test. The latter visits every archive occurrence, not merely
the first VFS-selected copy.

| Type | Unique paths | Archive copies | Records across copies |
| --- | ---: | ---: | ---: |
| rSoundPackage | 1,490 | 11,866 | 70,303 wave slots |
| rSoundRequest | 1,601 | 9,308 | 320,547 requests |
| rSoundStreamRequest | 2,650 | 3,851 | 141,419 requests |

All 1,490 distinct referenced package paths resolve. Every nonempty SPAC
wave reconstructs into a bounded RIFF chunk sequence. The sample
`sound/stream/bgm/wave/Bridge.sngw` passes all 108 Ogg page CRCs and
identifies as six channels at 48,000 Hz. Synthetic cases cover truncated
headers/strings, invalid source/auxiliary indices, split RIFF and empty
wave slots, all-negative metadata wave sentinels versus nonnegative out-of-range indices, unsafe paths, and Ogg CRC corruption.

The source tables contain 76,468 distinct stream paths; 285 resolve to
loose SNGW files and 76,183 do not. This is **not** evidence of missing
game audio. The loader bypasses the immediate loose-resource load for
request+0x0c equal to 1. All 76,183 unresolved paths are referenced only
by that mode; 284 loose paths also use only that mode, and one loose path
is shared by modes 0 and 1. The mode therefore does not by itself establish
which storage backend supplies a sound. Three additional installed banks exist at
`rom/sound/{npc_base_talk,npc_quest_talk,pwn}.pck`; their source-name/hash
lookup has not been traced. Japanese-labeled loose candidates such as
`bgm/wave/Epitaph_jp` also do not resolve. No rSoundSource class instances
exist in the ARC index. PCK parsing and language selection remain separate
follow-up work.

## Remaining work

Program selection, cue commands, volume/pan/pitch/delay fields, speaker
settings, source metadata words and runtime loop behavior are not assigned
semantics by this reader. Stream requests may reference assets absent from
the loose installation; report these rather than substituting another sound.
Next step: trace one runtime cue's command/program selection through the
package table and compare playback with the original under an authorized
audio test. No original-game comparison has been performed.
