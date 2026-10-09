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

General cue commands, volume/pan/pitch/delay fields, speaker
settings, source metadata words and runtime loop behavior are not assigned
semantics by this reader. Stream requests may reference assets absent from
the loose installation; report these rather than substituting another sound.
Next step: trace one runtime cue's command/program selection through the
package table and compare playback with the original under an authorized
audio test. No original-game comparison has been performed.

## Playback work in progress (audio-playback)

`mtsound::decode` uses Lewton for Vorbis and Symphonia 0.5.5 for
Microsoft ADPCM/PCM RIFF. These are device-independent decoders, with an
explicit caller-provided decoded-sample budget. No audio device is opened
by this crate. Symphonia's codec documentation is at
https://docs.rs/symphonia-codec-adpcm/0.5.5.

Stream request IDs are **u16 keys**, not array indices (`0x00dc09b0`).
`0x00ddb4e0` dispatches commands 1/5/10 to stream allocation and command
11 to `0x00dca2f0`, which starts previously prepared matching voices.
`uSoundStream::vf05` (`0x00c79d30`) queues the prepare ID before the
stream ID; mode 2's `0x00c79fa0` sends the nonpositional start request.
The narrow resolver handles direct play and this prepare/start pair;
other commands and request chains are explicitly rejected.

st200 ev10 frame zero sends stream ID 3 in both banks. Its prepared BGM
ID 24 resolves `event_bgm/wav/st200ev10-BGM`; voice/SE ID 203 resolves
`event_vo_se/wave/st200ev10_m`. Both are installed loose SNGW files.
Installed decoding checks produce six-channel 48 kHz audio, respectively
61.183 and 56.757 seconds. The Arisen's separate SPAC grunt decodes as
Microsoft ADPCM, mono 48019 Hz, 41344 samples. These are decode checks,
not audible playback or comparison against the original.

Base SREQ selection now follows request u16 keys (0x00f73b90), signed
program/variation fields at +0x1e/+0x20 (0x00dd3510 / 0x00dd8dd0), and
SPAC program and variation lookup (0x00e218e0, relocation 0x00e21600).
Runtime request overrides are not applied by the base-key reader. Synthetic
tests cover variation selection and absent waves; the installed ev10 Arisen
request resolves and decodes through this lookup.

The viewer now forwards event stream cues to a Bevy audio source in standalone
events and opening cutscenes. Sources are cached and replaced per event unit;
offline pause pauses the sinks. Six-channel Vorbis is provisionally folded to
stereo, preserving the center dialogue channel and omitting LFE. This is
explicitly not the original game's mixer, gain, reverb or positional model.
The release viewer builds, and all 11 mtsound tests pass including the full
installed reader corpus. Two viewer audio regressions verify center-channel
preservation and same-unit replacement within one deferred command batch.
A bounded 960x540 event-22 run resolves both ev10 streams, saves its screenshot
and exits without reported audio-device errors. Audible output and original-game
timing comparison have not been verified. Conditional Arisen voices, footsteps, attack sounds
and field BGM remain unconnected, so the audio-playback brief remains open.

Motion-SFX trace leads: uSoundMotionSe constructor 0x00c71c20 chooses LMT
event page 3; 0x00c73630 detects rising bits, 0x00c721c0 uses the group's u16
remap, and 0x00c722d0 resolves a UniqueID in rSoundMotionSe. AttributeSeFlag
uses the surface helper 0x00c72b60; direct requests use 0x00c724b0. Bank binding
and surface selection still need tracing before motion cues can be played.

`mtsound::motion` now reads all 92 installed rSoundMotionSe maps. It resolves
entries by UniqueID, preserving the three signed request variants, bank type,
joint/attachment, offset and surface-ray margins. Reflection is 0x00ce1250;
0x00ce10a0 builds the lookup with the last duplicate ID winning. The request
selector is 0x00c736f0 (selectors 0/1/2 use the three variants; other selectors
return zero). Player-state selection is still a host input. The rising-edge
helper follows 0x00c73630, and assembly at 0x00c72287 confirms the u16 LMT
remap before 0x00c722d0. Tests cover held-bit suppression, remap ordering,
the absent-ID sentinel and signed request variants. The existing ddcombat
motion-event reader already retains all four remap pages, so another LMT
parser is unnecessary.

Direct request dispatch (0x00c724b0) selects sound-unit +0x80 for type 0,
+0x40..+0x64 for types 1..10, and stream +0x68 for type 11. AttributeSeFlag,
rather than ReqType alone, routes to 0x00c72b60's surface lookup. That path
also uses the authored joint/offset and vertical ray margins. The resource
bindings and actual collision surface still need to be supplied before
motion SFX can be presented faithfully.

Player initialization 0x00b5f880 binds pl_com_bmse and the type-0 request
bank sound/se/pl_com/pl_com. It also binds mt_attri and the 30 surface banks
from the installed executable's pointer table 0x01875288. Weapon helpers
0x004b0ed0 and 0x004b11f0 build the primary and secondary sound-bank paths
from the weapon model category and its model suffix modulo 100; the common
and specific banks serve direct types 3/4 and 5/6. `weapon_banks` translates
those helpers with regression coverage for primary/offhand categories and
the exceptional suffixes, including war002.

`SurfaceRequests` parses the installed rSoundAttributeSe map's 32 request
IDs per row, bank type and UseFindIntersect. Selection 0x00ce00e0 uses the
surface index, falling back to entry zero for indices outside 0..31; the
absent-request sentinel remains intact. Three motion-audio unit tests and
the installed 92-motion-map/one-surface-map scan pass. Actual surface queries
use query material word one (getter 0x00423b80). The binding is now confirmed:
0x01165820 / 0x011637f0 put the leaf's material-index-selected 32-byte record
at query +0x40, and the getter reads that record +4. A private comparison of
both st200 collision resources found zero leaf/triangle material-index
differences over 40,949 pairs. This check is limited to those resources.

With head's approval, Terrain now preserves each triangle's full raw material
and exposes `ray_with_material`; `Ground::ray` retains its existing API and
hit geometry. Render terrain and flags-only fixtures report unknown material,
not a fabricated surface. Six world unit tests and the installed higher-ground
landing regression pass. The new unit check covers nearest-hit material,
reverse-direction selection, missing material and unchanged movement flags.

`direct_bank` and `attribute_bank` now preserve the two different ReqType
numberings from 0x00c724b0 and 0x00c72b60. Direct 0 is common, 1..10 are
bound slots, and 11 is stream; attribute 0 is the selected attribute bank,
1 is common, and 2..11 are bound slots. In the collision path, material word
one's first set bit among 0..28 selects the request column (no low bit means
column zero). It does **not** select the attribute bank. The separate bank
selector starts at zero and becomes two when getter 0x00423be0 reports bit
1; that bit's physical meaning remains unresolved. Five motion unit tests
pass, including the distinct numberings, high-bit exclusion and simultaneous
material bits. Actor bindings and playback remain the next integration step.

### Ordinary event sound effects

SREQ dispatch 0x00ddb340 / 0x00dda9f0 reads command +8: one plays through
0x00dd8dd0, two/three call the control path, and four selects via the random
resource. The next-request key is +0x1c; 0xffff or the current key terminates
the chain. `Requests::single_wave` accepts only command one with a terminal
chain and a referenced package. It rejects commands needing a stateful mixer.
Its regression covers missing IDs/packages, control/random commands, a
self-terminating key and a chain to another request.

The viewer now plays unconditional event SE cues through that checked path,
following the request's package reference and decoding its base program and
variation. Effects are separate one-shots and may overlap; stream-unit
replacement applies only to streams. Conditional cues require the explicit
actor context described below. Runtime overrides, original priority/group limits,
gain/pitch/delay and spatial processing remain unimplemented. Fifteen mtsound
unit tests and the installed ev10 stream/ADPCM regression pass; the ev10 SE
request is confirmed to be an ordinary terminal play. Audible comparison is
still pending.

### Conditional event voices

The conditional gate is now traced: uSoundSe::vf21 (0x00c78570) runs before
the call-flag edge in vf08 (0x00c77f10). Conditional IDs 0..3 read the player
cPlayerInfo at sGameSys+0xa76d0; 4..7 read the save-edit gender/voice bytes
at +0x150a4/+0x150a5; 8..11 and 12..15 read the first pawn cPlayerInfo at
+0xa7ec0. cPlayerInfo reflection 0x0077d980 names +0xc `mModelType` and
+0x294 `mVoiceType`; save-edit reflection 0x00787010 names +0x28 `mGender`
and +0x29 `mVoice`. The chosen ID is group base plus two when the model/gender
field is zero, plus `(voice / 3) & 1`. No sex-code inversion is inferred.

`mtsound::voice::EventVoices` translates this gate with explicit optional
actor records. Two regressions cover all four groups, voice division/parity
boundaries, invalid conditions and absent records. `audio::cues_with_voices`
accepts this per-event context; an empty context suppresses conditional cues.
Both event callers supply saved records when available. Character-creation
state without a save remains unavailable. Stable context during an event is assumed;
changing conditions during a held call flag needs the original gate/edge order.

The save conversion is confirmed at 0x00780fa0: cPlayerInfo+0xc receives
saved mGender masked with one, and +0x294 receives mVoice only when it is
below 12. `Variant::from_edit` preserves this conversion and reports unknown
for an invalid voice (the original retains prior state, which is unavailable
to a fresh reader). `audio::voices_from_save_xml` reads the chosen save slot's
Arisen and first pawn separately. It does not substitute either for the
distinct edit-record condition group. Three voice regressions and all five
viewer audio regressions pass.

The standalone event and opening callers now use this context with --save;
no-save opening keeps it empty. Standalone events choose the saved actor's
sex-specific scheduler unless --sex was explicitly requested. A bounded
event-22 run from frame 510 with a private save reached both authored female
voice alternatives at frame 514 and resolved only the selected f1 package
(40,320 decoded samples). It saved a screenshot and exited without logged
decoder or audio-device errors. The release build and all 32 enabled viewer
tests pass (one installed HUD test ignored). This verifies cue selection and
playback submission, not audible fidelity or an original-game comparison.

### Fixed-tick player motion sounds

`PlayerMotionSounds` samples LMT page 3 after each offline play simulation
tick, preserving rising-bit history separately for each actor and layer.
It uses the authored joint transform, offset and ray margins. Surface queries
retain both material word one and the separate SBC triangle attribute
(getter 0x00423be0); the latter selects attribute bank two when bit 1 is set.
Missing metadata suppresses the request. Fractional frames remain available
for joint poses while event lookup truncates the wrapped frame.

The player selector at 0x00c736f0 reads sGameSys+0xbe2c1. Its writer,
sSoundManager::vf06 at 0x004b35c0, depends on actor and combat state; stage
initialization 0x00501020 resets it. Until those inputs are represented, the
adapter accepts only requests identical for selectors zero and two. All 18
installed surface requests satisfy this condition, as do 16 of 17 primary
common-weapon requests and all 41 primary specific-weapon requests.

Current bindings cover the common player bank, attribute banks loaded from
the executable and weapon banks from the current Fighter/Mage combat kits.
The installed regression checks actor/layer independence, held-bit suppression,
history reset and sword-motion requests decoded into nonzero samples. Six
viewer audio tests pass. The terrain regression separately verifies nearest
hit attributes without changing movement flags (six unit tests and one
installed landing test pass).

This hook currently covers offline players' layer zero. Opening/LAN host
sampling, upper motion layers, offhand equipment, clothing and motion voices,
cached-floor requests and motion streams remain unconnected. Ordinary SE
limitations above still apply; this is not an audible-fidelity comparison.

A bounded two-seat offline `--demo --unattended` run resolved player request
30 (15,360 samples) and soil request 11 (33,920 samples), saved its screenshot
and exited. Several other surface requests were explicitly rejected as
non-ordinary commands. This confirms the fixed-tick-to-playback path, while
also showing that random/control dispatch still limits footstep coverage.
