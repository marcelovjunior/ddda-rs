# Quests, scripts and story flags

Measured 2026-10-09 against the Steam install and the 2017-12-13 exe
(identity in METHODOLOGY.md). Ghidra was read-only. **Implemented** in
`crates/ddquest`: typed quest-script orders (`order`), the quest control
table reader (`qct`), the story-flag banks (`story`), and survey/dump
examples. **Tested:** every QCT (338 / 338) and every script parses; the
opening's rows decode as below. **Compared with the original (live, via
the oracle, 2026-10-09, read-only):** the flag layout, quest bits 95/94
and the SceNo band index (section "Live check"). Nothing runs yet: there
is no quest interpreter.

Cutscenes are in [EVENTS.md](EVENTS.md).

## Overview

The story is driven by four kinds of data and one number:

| Piece | What it is | Where |
|---|---|---|
| **Quest scripts** | `rAIFSM` state machines owned by `cFSMOrder`, one per quest branch | `quest\qNNNN_bBB` in `rom\quest\fNN\qNNNN.arc` |
| **Stage scripts** | the same machines, per stage | `scr\stSSS\fsm\**` in the stage archive |
| **Quest control tables** (QCT) | judgment -> result rows: quest log, rewards, flags, markers | `etc\questCtrl\qNNNN_bBB` (quests), `etc\questCtrl\nNNNQuestCtrl` (NPC talk) |
| **Stage scenarios** | trigger volumes that boot scripts, set flags, jump areas, show messages | `scr\stSSS\etc\stSSS_{flag,door,gsf,fsm,chp,ladder,cam,mes,pawn,etc,npc}` (rScenario, XFS) |
| **SceNo** | the scenario number: one integer that orders the whole story | sGameSys +0x30 |

Scripts set **flags**; QCT rows and other scripts react to flags; events
(cutscenes) are requested by scripts and raise SceNo when they end.

## Data

### Quest archive

`rom\quest\fNN\qNNNN.arc` (279 archives in f00..f07) holds, e.g. for
q0001 "Newly Arisen":

| Entry | Type | Role |
|---|---|---|
| `quest\q0001_b00` | rAIFSM | the quest script (owner `cFSMOrder`) |
| `etc\questCtrl\q0001_b00` | rQuestCtrlTbl | control table |
| `id\questlog_data\q0001_log_<lang>` | rGUIMessage | quest log text |
| `id\npc_wind\npc_quest_talk\q0001[_<lang>]` | rMsgSet, rMsgSerial, rGUIMessage | quest dialogue (codex-7's NPC lane) |
| `sound\stream\npc_quest_talk\q0001\def\q0001` | rSoundStreamRequest | voice |

`_bBB` is the branch number: a quest with several phases boots its next
branch (`BootQuest q0023 branch 1..6`). Exe path formats: `quest\q%04d_b%02d`
(0x0159623c), `etc\questCtrl\q%04d_b%02d` (0x01596250),
`id\questlog_data\q%04d_log_%s` (0x0159617c). Quest names are
`id\message\common\quest_name_<lang>` (label `qNNNN`).

### Quest log

`id\questlog_data\qNNNN_log_<lang>`: message 0 is the title, 1 the
summary; entry *n* is message `5n + 10` (heading) and `5n + 11`
(text), with blank padding. The HUD shows the title and the entry the
QCT last logged (0x0066f740, `lea edi,[eax+eax*4+0xa]` at 0x0066f8ec;
`ddquest::qct::log_message_index`).

### Quest control table (rQuestCtrlTbl, QCT)

Loader 0x00cd7200; all u32: magic `qct\0`, version 0x10, allocation
size, table count; per table rows of (judgments of 5 u32, results of 11
u32) and a NUL-terminated name; one trailing u32. Reader
`ddquest::qct::QuestCtrl`; 338 / 338 archive entries parse with no
trailing bytes, 2,510 rows.

**Evaluation** (per quest, per frame: sQuestManager vf06 0x00482dd0 ->
cQuest 0x0078c830 -> 0x00792360): rows in order; a row runs when all its
judgments hold (0x0078ece0) and it is not disabled (bitmask at
`*(cQuestCtrl+0xc)`, MSB-first by row id); if its first result is type 1
evaluation jumps to table `a`; otherwise its results run (0x0078fbb0)
and the pass stops: **one row per pass**. A row that fired is disabled
for good unless it contains result 22. When a quest is active
(sGameSys +0xbe30c), it is evaluated first. NPC tables
(`nNNNQuestCtrl`) are evaluated from NPC talk code (0x00b9dc20 and
neighbours) with the talking NPC as context.

Judgments (`[type, a, b, c, d]`, jump table 0x0078faac):

| Type | Holds when |
|---|---|
| 0 | always |
| 1 / 2 | quest `a` bit `b` set / clear |
| 3 / 4 | GSF `a` set / clear |
| 7 | quest `a` bit `b` clear; once set, the row is disabled |
| 8, 28 | item missing (disables the row once held) |
| 10 / 11 / 12 | SceNo == / >= / < `a` |
| 13 / 14 / 15 | NPC `a` friendship == / >= / < `b` (default 500) |
| 16, 30, 31 | item `a` held, count `b` |
| 18 / 19 | NPC bit arrays (sGameSys +0xad060 / +0xacfec) set / clear |
| 20 / 21 / 22 | character `a` record +0x700 == / >= / < `b` |
| 23 / 24 | player record +0xc == 0 / 1 (used for gendered text: likely male / female) |
| 25 / 26 / 27 | sGameSys +0xa7a18 == / >= / < `a` |
| 32 / 33 | current stage == / != `a` |
| 34-37 | stage == `a` and a per-stage bit `b` |
| 9, 17, 29, 0x3b | NPC talk / hand-over context |
| 50-63 | board-quest checks |

Results (`[type, a, b, c, d, ...]`, jump table 0x00791f28):

| Type | Effect |
|---|---|
| 1 | continue in table `a` (first result only) |
| 2 / 3 | set / clear quest `a` bit `b` (0x00437520 / 0x00437590) |
| 4 / 5 | set / clear GSF `a` |
| 6 / 7 | stage `a` layout flag `b` on / off (0x00443ca0) |
| 8 / 9 | NPC `a` friendship + / - `b` |
| 10 | gold |
| 13 | show quest `a` message `b` |
| 14, 31 | boot quest `a` branch `b` |
| 23 | **start**: banner 1, log entry `d` |
| 17 | **objective**: banner 2, log entry `d` |
| 15 | report-back objective: banner 3, log entry `d` |
| 16 | **clear**: banner 6, log entry `d`, rewards |
| 18 | **fail**: banner 4 |
| 19, 30, 60 / 20, 61 | set / clear a quest marker |
| 22 | keep the row enabled |

Banners are queued on sIdCockpit (0x004525d0); log entries go to
sQuestManager +0x154 + (q*20 + n)*4 (0x00485d50).

### Rewards

`etc\questreward\questreward` (XFS, 131 `rQuestReward::cParam`), used
only by result 16 (0x00790539): the first record with mQuestNo == the
quest and mQuestFlg == -1 or that quest flag set. Gold (0x004431e0),
experience (0x00443470), rift points (0x00443310), three item/count
pairs (0x0045e680), NPC friendship (mNpcId, mAddFriendPoint). q0001: 2,000
gold, 50 experience, rift points 3, items 64 x1, 1 x5, 43 x5.

### Other quest tables (layout only)

- rQuestMarker `scr\stSSS\etc\stSSS` (XFS): marker positions per stage.
- rQuestSudden `scr\st100\etc\st100`: `qsp\0` v0x20, u32 count, 32-byte
  records (loader 0x00cd98b0); the field-encounter quests 41-49 except 45
  are booted from it by 0x00509a20.
- rQuestInfinity `etc\infquest\infquest`: `qif\0` v0x41, 478 records
  (loader 0x00cd80b0), the notice-board quests 98-122; partly read.

## Scripts (cFSMOrder)

A script is an rAIFSM (reader `mtfsm`, semantics in AI.md) whose owner
is `cFSMOrder` (546 of 3,087 FSM paths). States run **processes**
("orders") by name; transitions test conditions over the owner's
variables. Readable dump:
`cargo run -p ddquest --example script_dump -- <nativePC> quest\q0001_b00`.

### Who runs scripts

| Script | Started by |
|---|---|
| Quest `quest\qNNNN_bBB` | quest boot (0x00483f20 / 0x00484d90 -> 0x0078d0d0): the cQuest gets a uFSMQuest unit whose cFSMOrder points back at it (+0x1a0) |
| Stage `scr\stSSS\fsm\fix\*`, `fix_nosave\*` | automatically on stage load: every rAIFSM in the stage archive whose path contains `fsm\fix` (0x005081a0 -> 0x005080e0) |
| Stage `scr\stSSS\fsm\*` (others) | scenario BootFSM entries (0x00799be0), Note entries (0x0079bfe0), and save restore (0x00495150) |
| `event\stSSS\stage_FSM\stage_NNN[_nosave]` | as cFSMEvent tasks on stage load (0x005081a0 -> 0x005d0ed0) |

Quest boot refuses a branch-0 story quest whose bit 94 or 93 is set
(0x00483f20; exceptions listed there). A quest ends when its script
stops (cFSMOrder +0x70 bit 0; 0x0078c830).

### Per-state bookkeeping

On every state change of the root machine the owner's vf05 runs
(cBaseFSM ctor registers 0x00749820 through 0x00e05720 for level 0,
called at 0x00e0940e). For cFSMOrder (0x005bacd0) it clears the 32
**FreeFlag** bits and **mCntState**. Each frame 0x005bac30 adds
`max(mDeltaTime, 0.01) / mFps` (seconds) to mCntState, mCntFSM and the
active FreeReal timer. So `FreeFlag[n]` in a condition means "a process
of this state reported result n", and `mCntState > 60.0` "60 s in this
state".

**Result flags.** Most orders inherit `cFSMOrderParamFlag {mFlagType,
mFlagNo, mFlagBool}`; 0x005cc030 writes it: type 0 = FreeFlag bit
(< 32), type 1 = GSF bit (< 0x600), anything else (100 in the data) =
nothing.

### Orders

`cFSMOrder`'s constructor (0x005b7a60) registers 70 orders (registrar
0x00e088b0: name, enter handler, update handler, parameter class). The
opening uses the **bold** ones; `ddquest::order::Order` decodes them.

| Order | Enter / update | What it does (traced) |
|---|---|---|
| **Flag** | 0x005bc280 / 0x005bc2b0 | write the result flag once (mSetOnce) or every frame |
| **Event** | 0x005bcb50 / 0x005bcb70 | request a cutscene (EVENTS.md) |
| **SceNo** | 0x005bc960 / - | set SceNo (0x0043d750, raise-only) |
| **JumpArea** | 0x005bc700 / - | move to stage/position (0x00517ca0); optional SceNo (raise-only) |
| **QuestFlag** | 0x005c5530 / 0x005c55d0 | mActType 0: result = quest bit set; 1: = bit clear; 2: set bit; 3: clear bit |
| **SetLayout** | 0x005c5080 / - | stage layout flag on (0) / off (1) |
| CheckLayout | - / 0x005c52a0 | result = layout flag set (0) / clear (1) |
| **BootQuest** | 0x005c5a00 / - | boot quest (mQuestNo, mBranchNo) when mActType 0 |
| **Message** | 0x005c5bc0 / 0x005c5d40 | show a message; mType 2 = from the quest's message sets (0x004560d0), 3 = the stage's |
| **AreaHit** | 0x005c2ce0 / 0x005c2d70 | enable (mAct 1) / disable (0) a scenario entry; poll it (2 fired this frame, 3 shape hit, 4 has fired, 5 targets inside, 6 NPCs inside, 7 owner inside) |
| **TimeJump** | 0x005c2890 / - | set the clock |
| **Camera** | 0x005c3750 / 0x005c37c0 | scripted camera / "simple event"; raises SceNo to mSceNo at the end (0x005c4790) |
| Fade | - / 0x005c9a40 | fade, optionally SceNo, clock, start position |
| NpcAlive, NpcCheck, NpcGroup, OmSet, EnemyCheck, ... | see table below | spawn/check NPCs, objects, enemies (not traced here) |
| **Finish** | (cBaseFSM ctor 0x007495f0) | end state |

All 70 (enter / update; `-` none): Flag 005bc280/005bc2b0, FreeIns
005bbeb0, FreeReal 005bc090, SetTimer 005bc410, SetGameHour 005bc590,
JumpArea 005bc700, SceNo 005bc960, Event 005bcb50/005bcb70, EnemySet
005bd6a0/005bd6c0, EnemyRandom 005bdb60/005bdba0, EnemyGroup
005bd6a0/005bdec0, EnemyAdd 005be250/005be260, EnemyKill 005be110,
EnemyCheck -/005bef10, EnemyKillNum 005bf570/005bf5a0, ItemSet, ItemRandom,
ItemGroup (as Enemy*), ItemCheck -/005bf900, OmSet, OmRandom, OmGroup (as
Enemy*), OmCheck -/005bfbc0, NpcSet, NpcRandom, NpcGroup (as Enemy*),
NpcKill 005c0100, NpcAlive 005c0310, NpcCheck -/005c0420, NpcDieCheck
-/005c09f0, PlCheck -/005c0ca0, WeightCheck -/005c1a00, DistanceCheck
-/005c20d0, ClockCheck -/005c2440, TimeJump 005c2890, AreaHit
005c2ce0/005c2d70, Camera 005c3750/005c37c0, CameraEnemy 005c4980,
KeyItem 005c4d80/005c4e30, SetLayout 005c5080, CheckLayout -/005c52a0,
QuestFlag 005c5530/005c55d0, Announce (none), BootQuest 005c5a00,
Message 005c5bc0/005c5d40, CmcMessage 005c6130, TutorialMessage
005c6340/005c6430, SE Request 005c6bf0, SE Param Set 005c71a0, STREAM
Request 005c7530, BattleBGM 005c7a10, Sub Mixer (none), ApplyEffect
005c7c70, Weather 005c7f90, Collision 005c8140, Muteki -/005c8ce0,
CheckHp -/005c9250, SetDie -/005c9770, Fade -/005c9a40, ApplyFSM
-/005c8460, ApplyPosition -/005c88d0, StartQuake 005ca210, StopQuake
005cac30, SetShell 005cad60, CameraFlag 005cb2f0, LeaveCmc 005cb400,
PlaceName 005cb860, Achievement 005cba40, KillArea -/005cbc70, SetSlow
005cbe80; plus Finish from cBaseFSM. Use counts over all 546 scripts:
`cargo run -p ddquest --example script_survey -- <nativePC>`.

### Variables (conditions)

`cFSMOrder` reflected properties (vf03 0x005b8ff0), with getters:

| Variable | Getter | Meaning |
|---|---|---|
| GSF[0..0x600) | 0x005bad40 | global story flag |
| MyQuestFlag[0..96) | 0x005bae60 | bit of the script's own quest (cQuest +0x30 >> 16) |
| FreeFlag[0..32) | 0x005bada0 | per-state result bits (above) |
| AreaHitFlag[0..32) | 0x007abc70 | per-stage saved bits (cSTAGE_FLAG +0x18) |
| AreaStayFlag[0..32) | 0x005bae00 | per-frame word sGameSys +0xb884c |
| FreeIns[32] s32, FreeReal[32] f32 | fields +0x74, +0xf4 | script registers (saved as cSAVE_DATA_FSM mFreeS/mFreeF) |
| mCntState, mCntFSM | +0x178, +0x17c | seconds in state / in script |
| SceNo | 0x005baf00 | sGameSys +0x30 |
| StageNo, CurrentStageNo | 0x005baf40, 0x005baf70 | aStage +0x724 |
| IsEvent, IsEventStart, EndCamera, IsFadeBlack | 0x005bb080, 0x005bb0a0, 0x005c4970, 0x005bb380 | event/camera/fade state |
| GameHour | 0x005bb110 | clock |
| NpcFriend[], IsNpcDie[], IsNpcUnit[], IsOmFlag[] | 0x005bb420 ... | NPC/object state |
| JobPl, MainWpnPl, SubWpnPl, WeightPl, VoiceTypePl, PlayerNum, PlayerAliveNum, IsLevelUpPl, IsDamagePl, IsCantActPl, IsDispMess, IsDispCmcMess, IsMyCmcLost, CaughtNpcId, ValidQuestNum, EnemyNum, EnemyKillNum, StartPosNo, IsTrial | see vf03 | player and world state |

Condition operators are in AI.md; `ddquest::order::expr_text` prints them.

## Story state (save)

sGameSys is `*0x018fa4bc`; `cSAVE_DATA_STAGE` starts at +0xac6e0. Bits
are numbered **MSB-first** in 32-bit words: bit *i* is `word[i >> 5] &
(0x80000000 >> (i & 31))` (0x005bad40, 0x00401280, 0x004356c0). Model:
`ddquest::story::StoryFlags`.

| Bank | Where | Size | Notes |
|---|---|---|---|
| SceNo | sGameSys +0x30 | u32 | set by 0x0043d750 (below) |
| GSF (`mGsfFlag`) | sGameSys +0xac6e4 | 1,536 bits | 534 indices referenced by data |
| Quest flags (`mQstFlag`) | sGameSys +0xac7a4 + 12*q | 96 bits, 128 quests | bits 0-92 per quest; **95 started, 94 cleared, 93 failed** |
| Layout ("lot") flags | sGameSys +0xadee8 + (s*0x53 + (n >> 5))*4 | 128 per stage index *s* | `cSTAGE_FLAG` Flag000-096; stage index from the 65-entry table 0x01530348; st100 has 128 more at +0xb3330 |
| AreaHitFlag | cSTAGE_FLAG +0x18 | 32 per stage | |
| Script FreeIns/FreeReal/timers | cSAVE_DATA_FSM | per saved script | non-`fix` scripts are saved by path (0x00495150) |

Quest bits 93-95 are a data convention: the QCT sets 95 with "start"
(50 rows), 94 with "clear" (52), 93 with "fail" (37). The engine only
reads them (0x00483f20 refuses to re-boot; 0x004369d0 treats 95 & !94 &
!93 as active).

Layout flags gate placement **groups**: sSetManager's lot target manager
(0x004a95c0, test 0x004a9a70) loads a group when its flag is set (and
its SceNo window and hour window hold) and unloads it when cleared.

### SceNo (scenario number)

Set by 0x0043d750 `(value, raise_only)`: with raise_only, values not
above the current one are ignored. It also sets GSF 530 once SceNo >=
6520, and a band index at sGameSys +0x3c (translated rule, live-checked
for 3400 -> 3):

| SceNo | Band | | SceNo | Band |
|---|---:|---|---|---:|
| 0-999 | 13 | | 6400-6549 | 7 |
| 1000-1800 | 1 | | 6550-6799 | 8 |
| 1801-2849 | 2 | | 6800-7199 | 9 |
| 2850-3599 | 3 | | 7200-7799 | 10 |
| 3600-4599 | 4 | | 7800-7999 | 11 |
| 4600-6399 | 6 (5 is dead code) | | 8000-9049 | 12 |
| other | 0 | | 9050-10000 | 14 |

Raise-only setters: the SceNo order (0x005bc960 pushes 1), event end
(0x0050c7a0), JumpArea (0x005bc700), Camera end (0x005c4790), a
scenario arg handler (0x0079a1e0), 0x0052b2d0 and 0x00c4a565 (1800).
Forced: new game (100, 0x00445ab0), prologue end (1000, 0x00444b20),
save load (0x004947b0), 0x0051e330 (4800 or 5500).

**The SceNo ladder** (every value the data sets; script and how):

| SceNo | Set by |
|---|---|
| 100 | new game (0x00445ab0: stage 380, start position 0) |
| 150, 200, 350, 370, 600, 650, 750, 800, 850, 900 | prologue scripts `scr\st380\fsm\...` (`fix\100ck_evmes` camera, `fix\set_00`, `fix\set_350on`, `370_on`, `harpy_cam`, `650_on`, `fix\set_02`, `fix\set_03`; 900 at the end of event st380 ev00) |
| 1000 | prologue end (0x00444b20: stage 800, start position 0) |
| 1025, 1050, 1100 | q0000 events st200 ev00, ev01, ev05 |
| 1200 | q0001 event st200 ev10 |
| 1300 | `scr\st200\fsm\1300_on` |
| 1400 | `scr\st200\fsm\bootEv_st200ev15` event st200 ev15 |
| 1410, 1450 | q0031, q0002 |
| 1500, 1550, 1700, 2000, 2100 | q0003 (2000 with event st210 ev00) |
| 2600, 2800, 2850 | q0005 (events st210 ev05, ev10) |
| 3400, 3550, 3600 | q0007 (3600 with event st220 ev00) |
| 3800 | q0008 |
| 4000, 4600 | q0009 |
| 4700 | `scr\st220\fsm\fix\setsecno_4700` |
| 4850 | q0030 |
| 5900, 6000, 6400, 6500 | q0015 (events st240 ev20, ev25) |
| 6520 | q0017, q0020 |
| 6550 | q0016, q0018 |
| 6570, 6600 | q0019 |
| 6580 | `scr\st220\fsm\fix\q0019_bossbattle` |
| 6800 | q0021 |
| 7000, 7150, 7200 | q0022 (events st310 ev00, ev05) |
| 7400, 7500 | q0023_b00 (event st500 ev00) |
| 7600 | q0023_b04 (event st502 ev00) |
| 7800, 7820 | q0023_b05, q0023_b06 |
| 7850 | `scr\st230\fsm\fix\changesecno7850` |
| 8000 | `scr\st240\fsm\q0024_oitaryououbatte` (event st240 ev15) |
| 8200 | q0024 |
| 8220, 8230 | `scr\st604\...\gsf_setctrl`, `scr\st601\...\8220ck_8230on` |
| 8250 | q0025 |
| 8400, 8600, 8800, 9000, 9100 | q0026 (events st610 ev00, ev05, ev12, ev15, ev17) |
| 9050 | q0026 (JumpArea to stage 200) |

Readers compare SceNo constantly (483 conditions; QCT judgments 10-12;
NPC layout choice at 0x00bbdaa0 uses < 3600 and >= 7800; enemy group
windows `mAppearBgn/End`, e.g. 7800 = the dragon's defeat per the
Riftstone GPL notes).

### Story flags (GSF)

534 GSF indices are written or read by data. The full index (writers:
script result flags of type 1, QCT results 4/5, scenario GsfFlag args
(0x007997c0: set when mFlagType 0 / mSetType 0, clear when mSetType 1),
NPC think scripts' "Set GSFFlag"; readers: script conditions, QCT
judgments 3/4, scenario gates `mGsfNo`) is generated by

    cargo run -p ddquest --example flag_index -- <nativePC>

Flags written by code: GSF 372 (vocation chosen, 0x00512fb0), GSF 530
(SceNo >= 6520, 0x0043d750), GSF 88 is written by q0000's QCT when the
player record +0xc == 0. The opening's flags:

| GSF | Written by | Read by | Meaning (from the data; *guessed* where marked) |
|---:|---|---|---|
| 88 | q0000 QCT row 0 (player record +0xc == 0) | - | *male player* |
| 280, 391, 392, 394, 398, 376, 325, 353 | q0000 state "FlagIni" (and later quests/NPC scripts) | scenario gates in st601, st220, st240 for 280, 353, 391, 398 | *world setup after the dragon attack* |
| 360 | q0000, q0001, q0005 and 19 more (on at start, off at events) | scenario gate `st706_gsf` | *quest scene in progress* |
| 372 | vocation menu close 0x00512fb0; q0001 QCT row 3 (stage != 200) | q0001, `q0001_noSoubiNuke`, scenario `st200_etc[0]` gate | vocation chosen at the weapon table |
| 377 | `soubiDanaMes` (on/off) | q0001 | equipment-shelf scene running |
| 378, 379 | 378: q0000 (every frame of the fight phase); 379: no data writer (set in the live save: code or an unindexed source) | 379: q0000 | dragon-fight phase checks |
| 381 | NPC script `ai\fsm\npc\st200\quest\q0002_n001_ai` | q0002 QCT row 0, `bootEv_st200ev15`, `fix\1400_chumes` | first pawn met (q0002 starts) |
| 520 | q0000 (on/off) | - | *dragon camera* |
| 622 | cleared by q0001; set by `sontyoHouseHatudeCam` | `fixB5_chumes`, scenario gates `st200_fsm[19,20]` | *chief's house first camera* |
| 704 | prologue `set_03` | - | |

## Scenarios and triggers (rScenario)

XFS `rScenario { mArray of cScenarioData { Param, Shape, Arg } }`; 216
files, 1,818 entries. The suffix of the file name is the **task**
(table 0x0163aefc): 1 flag, 2 door (AreaJump), 3 gsf, 4 fsm (BootFSM), 5
chp, 6 ladder, 7 cam, 8 mes, 9 pawn, 10 etc, 11 npc. Loader 0x005083c0
loads the current stage's files; sScenario (0x018fa4fc) keeps one list
per task (+0xd4[task]).

Each frame (sScenario vf06 0x0049a0e0 -> 0x00797df0) an entry is skipped
while disabled (status bit 4, which `AreaHit` mAct 0/1 toggles) or while
its gate GSF `mGsfNo` is set (pawn entries: while it is clear); then its
shape is tested against `mDetectAttr` (bit 0 = the player) and
`mBootType` decides when to fire: 0 every frame, 2 while inside, 3 once,
4 on each entry, 5 on a button press inside (prompt), 6 the same once,
8 once with a prompt, 10 on leaving (reading of 0x00797df0; the button
source is untraced). Firing calls the task's handler (0x00798ce0):
GsfFlag 0x007997c0, BootFSM 0x00799ca0, Etc 0x0079d010, ...

`AreaHit (task, no)` in a script addresses entry `no` of the current
stage's file for task `task` (0x005c3600).

## Worked example: the opening

From the title to the first pawn. Addresses above; data paths in the
install. Live-checked parts are marked.

1. **New game** (0x00445ab0): stage 380 (the prologue), SceNo 100.
2. **Prologue (stage 380).** `fsm\fix\*` scripts start on stage load and
   walk SceNo 200 -> 900 (`set_00`, `set_350on`, `370_on`, `650_on`,
   `set_02`, `set_03`); `set_03` plays event st380 ev00 ("Come,
   Arisen...", SceNo 900 at its end). The prologue's end reaches
   0x00444b20 through stage mode 0x11 (0x0050f610 -> 0x00444840): next
   stage 800, SceNo 1000 (forced).
3. **Quest boot** (0x004827e0, stage setup): stage 800 with SceNo 1000
   boots **q0000** and q0031.
4. **q0000 "Harbinger of Destruction"** (`quest\q0000_b00`, 28 states):
   GSF 360 on -> wait `SceNo == 1000 && StageNo == 800` -> event st100
   ev00 (the movie `movie\event\st100ev00.wmv`) -> `JumpArea` to stage
   801 -> wait `StageNo == 200` (*guessed*: stage 801 is character
   creation) ->
   event st200 ev00 (crier, "It's the dragon!", SceNo 1025) -> dragon
   camera (`Camera`, GSF 520) -> quest bit 2 -> **QCT row 1: start,
   log entry 0 "Challenge the Dragon"** -> fight checks (`EnemyCheck`,
   GSF 378/379, `mCntState > 17` / `> 43`, `AreaStayFlag[1]`) -> event
   st200 ev01 (state name 心臓取, "heart taken"; SceNo 1050) -> event st200
   ev05 ("Take up arms, newly Arisen", SceNo 1100) -> `TimeJump`,
   `Weather`, layout flags, NPC groups -> quest bit 0 -> **QCT row 2:
   clear, log entry 3 "Quest Successful", bit 94** -> `BootQuest q0001`
   -> Finish. QCT row 0 sets GSF 88 when the player record +0xc is 0.
5. **q0001 "Newly Arisen"** (`quest\q0001_b00`, 11 states; QCT
   `etc\questCtrl\q0001_b00`):
   - QCT row 0 (no judgment): **start, log entry 0 "Take a Weapon"**,
     bit 95, layout flags st200 30 and 33 on, quest marker.
   - State 0: enable scenario `st200_fsm[11]` (equipment-shelf staging,
     boots `scr\st200\fsm\soubiDanaMes`: camera on the shelf, message
     q0001 #2 "Take up arms, Arisen...", GSF 377 while it runs); layout
     st200 10, 11 on; GSF 360 on.
   - State 4 "dragonMes": every 60 s while GSF 377 is clear, message
     q0001 #1 ("To me... Come to me...": the dragon's voice; the index
     route is guessed).
   - **Weapon table**: scenario `st200_etc[0]` (task etc, press-button,
     active while GSF 372 is clear) -> Etc handler 0x0079d010 raises
     request bit 24 -> stage mode 0x19 (0x00512fb0) opens the vocation
     menu (`uGUIJobChange`) and on close sets **GSF 372**.
   - `GSF[372]` -> states 9, 1: quest bit 3 -> **QCT row 1: objective,
     log entry 1 "Exit the Room"**; disable the shelf staging; poll
     scenario `st200_fsm[0]` (the room exit, an OBB) with `AreaHit` mAct 3.
   - Exit reached -> **event st200 ev10** (EVENTS.md), SceNo 1200, start
     position 12; GSF 360 off.
   - After the event: quest bit 4 -> **QCT row 2: clear, log entry 4
     "Quest Successful", rewards, bit 94**; the script waits for bit 94
     and finishes.
   - Side script `scr\st200\fsm\fix\q0001_noSoubiNuke` counts shelf
     scenes (quest bit 5 set by `soubiDanaMes`) and after three sets
     quest bit 6, which also moves q0001 to the exit step (*meaning
     guessed: leaving without a weapon*).
6. **SceNo 1300** (`scr\st200\fsm\1300_on`, a non-fix stage script) ->
   `bootEv_st200ev15` (`SceNo == 1300`): event st200 ev15 (SceNo 1400),
   `ApplyFSM` gives NPC 1 the script `ai\fsm\npc\st200\quest\q0002_n001_ai`,
   which sets **GSF 381** -> `BootQuest q0002` and q0002's QCT row 0
   (GSF 381) starts **"Upon a Pawn"**.

Reproduce the decoded rows: `cargo test -p ddquest -- --nocapture`
(tests `newly_arisen_control_rows`, `opening_quest_scripts_decode`).

## Live check (2026-10-09, oracle, read-only)

claude-4 read the maintainer's save in the running game (party level 7,
night, stage 100):

- SceNo 3400 (set by q0007), band +0x3c = 3: matches 0x0043d750's table.
- Quest words (third word): q0 2 (cleared; q0000's QCT never sets 95),
  q1 3, q2 0x83, q3 0x02000003, q5 3 (started + cleared), q7 0x40000001
  (started, not cleared: consistent with SceNo 3400 from q0007), q4 0.
- First words: q1 0x18000000 = bits 3 and 4, exactly the two bits
  q0001's script sets; q0 0xe43e0c03 has bits 0, 2, 30, 31 that q0000's
  script sets, and 20, 21 that it waits for.
- GSF, MSB-first: 7 of the 8 flags q0000 sets in "FlagIni" are set (325
  is clear, perhaps cleared later); 88, 372, 378, 379, 381 set; 360, 377,
  520, 530 clear, as the scripts leave them. LSB-first numbering would
  put almost all of these wrong.

Not checked live: anything about events, QCT evaluation order, timing.

## Open questions

- The prologue's exact path to stage mode 0x11, and stage 800/801's
  roles (character creation lies between 801 and 200).
- QCT judgment/result types not in the tables above; the QCT trailer;
  whether `mQuestCtrlFlg` saves the disabled-row mask.
- How `Message` mMesId selects a line (GMD index or MSS row).
- Scenario detection bits other than the player; the button source.
- The GPL field that names a group's layout flag.
- NPC think-script orders (cThinkFSM::*) and their handlers.
- Board quests (rQuestInfinity) and sudden quests in detail.

## Proposed implementation briefs

1. **quest-runtime** (core, `ddquest` -> new `ddstory` sim crate):
   `StoryFlags` + a cFSMOrder interpreter for the orders in the table
   above (Flag, QuestFlag, SceNo, SetLayout, CheckLayout, BootQuest,
   AreaHit, Event request, Finish), per-state FreeFlag/mCntState reset,
   QCT evaluation (one row per pass, disable-after-fire, types 1-23).
   Done when q0000 -> q0001 -> q0002 run headless from scripted inputs
   and produce the SceNo/flag sequence of the opening; host-owned for
   co-op.
2. **event-player** (core + thin Bevy): event table + SDL evaluation
   (key modes, cut snapping) -> camera (needs an LCM reader), actor
   motions (mtlmt), subtitles (mtgmd), fades; the stage event mode
   (request, freeze, place, raise SceNo). Done when st200 ev10 plays
   with camera cuts at frames 460, 670, ... and returns control.
3. **scenario-triggers** (core): rScenario reader + per-frame evaluation
   (gate, shape test, boot types) for tasks fsm, gsf, flag, etc, mes,
   door. Done when st200's scenarios drive q0001 (shelf scene, weapon
   table, room exit) with a scripted walk.
4. **lot-flags-placement** (core, with the world lane): layout flags and
   GPL group gating (SceNo windows, hours) so q0000/q0001 layout changes
   show and hide NPCs and objects.
5. **lcm-camera** (reader): rCameraList v3 reader with an install test.
6. **quest-ui** (thin): quest log (entry rule), banners, markers, rewards
   from rQuestReward.
