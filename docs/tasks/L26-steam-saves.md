# L26: Steam services and save data

- **Owner:** claude-10
- **Branch:** `task/L26-steam-saves` (worktree `E:\ddda-rs-wt\L26`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

The game is the Steam build. `crates/ddruntime/src/steam.rs` answers the
Steam checks the boot reaches so far. A playable game needs more:

- the full set of Steam calls the game makes (user, stats and
  achievements, remote storage, friends, utils, apps, and the online pawn
  features where DDDA uses them);
- its save data and configuration, read and written the way the original
  does, but only inside the runtime's sandbox overlay.

The player's real saves and install are read-only.

## Task

1. **Census.** List every `steam_api.dll` import and every Steam interface
   method the game calls, with call sites from Ghidra: flat API and
   interface vtables. Then list how saves and config are stored: Steam
   Remote Storage or plain files, which paths, file names and sizes
   (`docs/SAVE_PAWNS.md` and the `ddsave` crate describe the format). Put
   it in Results.
2. **Implement** each call the game makes with offline Steam semantics:
   - documented return values from the Steamworks SDK headers and docs;
   - one local user with a stable SteamID;
   - stats and achievements stored in the sandbox;
   - callbacks and call results delivered through `SteamAPI_RunCallbacks`
     as Steam does;
   - online features answer "offline" as Steam does without a connection.
   Test each.
3. **Saves.**
   - Writes go to the sandbox overlay (see `files.rs`, including the "home"
     root L17A added). A new game creates its save there.
   - An option imports the player's existing save read-only, as a copy
     into the sandbox, so the rebuilt game can continue it.
   - Verify that a save the rebuilt game writes has the same format: the
     `ddsave` crate reads it.
4. Update the `docs/RUNTIME.md` rows.

## Results

Branch `task/L26-steam-saves` (claude-10). Implemented in
`crates/ddruntime/src/steam.rs`, tested in
`crates/ddruntime/tests/steam_saves.rs`. You can reproduce the census with
`research/runtime/steam_census.py <unpacked DDDA.exe>`.

### Census

**SDK version.** The installed `steam_api.dll` carries SteamClient017,
SteamUser018, SteamFriends015, SteamUtils007, STEAMAPPS_INTERFACE_VERSION007,
STEAMUSERSTATS_INTERFACE_VERSION011, STEAMREMOTESTORAGE_INTERFACE_VERSION012,
STEAMSCREENSHOTS_INTERFACE_VERSION002 and STEAMUGC_INTERFACE_VERSION005.
Only Steamworks SDK 1.33 has that set (1.32 has UGC003). The vtable slots
come from the 1.33 headers in declaration order, with two adjustments:
the `_PS3` sections are left out, and overloads of one name are in reverse
order. That reversal is MSVC's rule, and the game's own CCallbackBase
vtables show it (below).

**Flat API: all 14 steam_api.dll imports** (IAT 0x0139D454..0x0139D488):

| Import | Call sites |
|---|---|
| SteamAPI_Init | 0x00407A20 (WinMain 0x004079E0) |
| SteamAPI_Shutdown | 0x00407AB9, 0x00407AE8 |
| SteamAPI_RunCallbacks | 0x0041DDF0 (the frame, in 0x0041DD60) |
| SteamAPI_RegisterCallback | 0x00408466 and 0x00757B1D: id 331 GameOverlayActivated_t; 0x0075DB2D: id 714 GamepadTextInputDismissed_t |
| SteamAPI_UnregisterCallback | 0x0040844D, 0x00408533, 0x004085EB, 0x00757A69, 0x00757B04, 0x0075D3AD, 0x0075DB14 |
| SteamAPI_RegisterCallResult | 0x00CA437D (sDDCaProto: EncryptedAppTicketResponse_t), 0x00D261A7 (MtNetRanking: leaderboards), 0x00DA2E10 (MtNetObject: leaderboards, FileShare, UGCDownload) |
| SteamAPI_UnregisterCallResult | 0x00CA23D5, 0x00CA434F, 0x00D26172, 0x00D261F2, 0x00D27D54, 0x00DA2DE2, 0x00DA39F0 |
| SteamUtils, SteamUser, SteamUserStats, SteamFriends, SteamApps, SteamRemoteStorage, SteamScreenshots | accessors for the methods below |

**Interface methods: 37** (slot, method, call sites):

| Interface | Methods |
|---|---|
| ISteamUser | 1 BLoggedOn 0x00D2A3E5; 2 GetSteamID 0x0040FA2D, 0x00467787, 0x004B7ABE, 0x00D2A374, 0x00D2A557; 20 RequestEncryptedAppTicket 0x00CA4284; 21 GetEncryptedAppTicket 0x00CA42E0 |
| ISteamFriends | 0 GetPersonaName 0x00D2A490; 3 GetFriendCount 0x00D90D63; 4 GetFriendByIndex 0x00D90D81; 5 GetFriendRelationship 0x00D2A45A; 7 GetFriendPersonaName 0x00C967D2, 0x00D25E24, 0x00D25F14; 29 ActivateGameOverlayToUser 0x0069D51D, 0x006C2146, 0x00B9E63B; 37 RequestUserInformation 0x00C9685E, 0x00D25DF2 |
| ISteamUtils | 4 GetIPCountry 0x0041E23E, 0x00D9741D; 9 GetAppID 0x00DF3130; 16 SetWarningMessageHook 0x00407A5F; 20 ShowGamepadTextInput 0x0075D496; 22 GetEnteredGamepadTextInput 0x0075D866 |
| ISteamApps | 4 GetCurrentGameLanguage 0x00DF2F9B; 17 GetInstalledDepots 0x00DF3138 |
| ISteamRemoteStorage | 0 FileWrite 0x004B73BE, 0x00DA3974; 1 FileRead 0x004B7315, 0x004B73EA, 0x004B7D6A, 0x00DA2C41, 0x00DA399F; 4 FileShare 0x00DA3BC2; 10 FileExists 0x004B72CE; 12 GetFileSize 0x004B72EB; 18 IsCloudEnabledForAccount 0x004B7322; 19 IsCloudEnabledForApp 0x004B7331; 21 UGCDownload 0x00DA32AC; 24 UGCRead 0x00DA31BB |
| ISteamUserStats | 7 SetAchievement 0x00D96FAC; 10 StoreStats 0x00D9700F; 23 FindLeaderboard 0x00D27F0A, 0x00D282BF, 0x00D28664, 0x00DA3E48; 28 DownloadLeaderboardEntries 0x00D2834F; 29 DownloadLeaderboardEntriesForUsers 0x00D28718, 0x00DA3624, 0x00DA38EA; 30 GetDownloadedLeaderboardEntry 0x00D2747F, 0x00DA327A; 31 UploadLeaderboardScore 0x00D28010, 0x00DA358D, 0x00DA3865 |
| ISteamScreenshots | 2 TriggerScreenshot 0x00DA9651 |

Method: the census starts at every instruction that names a steam_api IAT
slot. From there it follows the interface pointer through registers,
including forward branches, to the `call [vtable+off]`. Ghidra lists 77
code references to the 14 IAT slots, in 51 functions plus four code
stretches with no function defined (0x004B72BA, 0x00CA42C3, 0x00DA3196,
0x0075D857). I decompiled or disassembled all of them and checked every
method site the census reports against them. Two UnregisterCallResult sites call
through a register (0x00D27D54, 0x00DA39F0); both are flat calls.

**Callbacks and call results.**

- The game's CCallbackBase vtables (e.g. 0x015912E0, 0x01408EF0) have
  three slots:
  - slot 0: `Run(void*, bool, SteamAPICall_t)` (`ret 0x10`);
  - slot 1: `Run(void*)` (`ret 4`);
  - slot 2: GetCallbackSizeBytes.
- The sizes those objects report match the SDK structures:
  EncryptedAppTicketResponse_t 4, LeaderboardFindResult_t 0x10,
  LeaderboardScoresDownloaded_t 0x18, LeaderboardScoreUploaded_t 0x20,
  RemoteStorageFileShareResult_t 0x118, RemoteStorageDownloadUGCResult_t
  0x128.
- MtNetObject's constructor (0x00DA3E90) sets the ids 1104, 1105, 1106,
  1317 and 1307.
- The installed steam_api.dll handles them like this:
  - RegisterCallback (0x3B404D70) sets flag bit 0 and `m_iCallback`.
  - UnregisterCallback (0x3B4056A0) clears the flag.
  - The dispatcher (0x3B404EF0) calls `Run(void*)` on every object
    registered for the id, except game-server objects.
  - For a call result (0x3B405510) it checks `m_iCallback` and
    GetCallbackSizeBytes against the result, calls
    `Run(pv, bIOFailure, call)`, then forgets the registration.
  - A nested RunCallbacks returns at once (guard 0x3B42C585).

**Online state.** 0x00D2A390 works out the connection state from
InternetGetConnectedState and BLoggedOn: the game counts as offline (1)
unless both report a connection. The online paths check this state or the
call results first: leaderboards 0x00D27E50/0x00D28190/0x00D28510, pawn
sharing 0x00DA3C80, and the Capcom ticket 0x00CA4270.

**Saves and configuration.**

- The save is a Steam Cloud file, not a plain file: `DDDA.sav`.
  - The name is the string at 0x01564F1C, set at 0x004B8280 case 0xB.
  - Size: 524,288 bytes (the 0x80000 buffer at `[0x018FA4F8]+0x21B0E4`).
  - 0x004B72B0 checks it (FileExists, GetFileSize, FileRead,
    IsCloudEnabledForAccount/App).
  - 0x004B7390 writes it (FileWrite, then FileRead back and compare).
  - 0x004B7CA0 reads it.
  - Format: what `ddsave` reads. That is the 32-byte header of 0x0040F1A0,
    a zlib stream, and the MtDTI XML (0x0040F6E0).
  - Owner check: before writing, 0x004B7A30 stores GetSteamID at
    `+0x21AF98`, which is serialised as `mSteamID`. On load, 0x0040F880
    refuses (result 5) a save whose SteamID is not GetSteamID.
- The player's Steam folder (`userdata\<account>\367500\remote`) also holds
  files `0` to `7`, 8,192 bytes each. They are online pawn files of
  MtNetObject (FileWrite 0x00DA3974, then FileShare). Their names come from
  MtNetObject's callers, which I did not trace.
- Steam names the folder after the low 32 bits of the SteamID: the private
  backup's `mSteamID` is exactly 76561197960265728 plus its account folder's
  number.
- Configuration is a plain file, `<Documents>\CAPCOM\DRAGONS DOGMA DARK
  ARISEN\config.ini` (MtRegistry 0x00CFFF00). The sandbox home of
  `files.rs` (L17A) already serves it.

### Implementation (offline Steam)

There is one local user. Their SteamID is a public individual desktop
account: `0x0110000100000000 | account`. Account, persona name, country and
language come from `<sandbox>\steam\user.txt`, then from
`DDRUNTIME_STEAM_ACCOUNT`/`DDRUNTIME_STEAM_NAME`, then from the defaults.

| Call | Offline answer and evidence |
|---|---|
| SteamAPI_Init / Shutdown | true / nothing |
| Register/Unregister Callback and CallResult; RunCallbacks | as steam_api.dll does (above). Posts are delivered at the next RunCallbacks, in order; anything posted during delivery waits for the next one |
| BLoggedOn | false (no live connection) |
| GetSteamID | the user's SteamID (through the hidden result pointer) |
| RequestEncryptedAppTicket | a call whose EncryptedAppTicketResponse_t carries k_EResultNoConnection (a documented result). GetEncryptedAppTicket: false |
| GetPersonaName / GetFriendPersonaName | the user's name; "" for anyone else (documented for unknown users) |
| GetFriendCount | -1 (documented for a user who is not logged on). GetFriendByIndex: k_steamIDNil. GetFriendRelationship: None |
| RequestUserInformation | false: nothing is pending (**runtime choice**: true would promise a PersonaStateChange_t that never comes) |
| ActivateGameOverlayToUser, TriggerScreenshot | nothing (**runtime choice**: the runtime has no overlay, so no GameOverlayActivated_t) |
| GetIPCountry | "US" (**runtime choice**: the runtime's locale) |
| GetAppID / GetCurrentGameLanguage / GetInstalledDepots | 367500 / "english" / 0 |
| SetWarningMessageHook | accepted; the hook is never called |
| ShowGamepadTextInput / GetEnteredGamepadTextInput | false (documented when Big Picture is not running) |
| FileWrite / FileRead / FileExists / GetFileSize | the sandbox's `steam\userdata\<account>\367500\remote\<name>`, with the documented failure cases (NULL data, a size that is negative or over 100 MiB, invalid names, names of 260 characters or more) |
| IsCloudEnabledForAccount/App | true (**runtime choice**: Steam's default; the player's settings are not read) |
| FileShare, UGCDownload, FindLeaderboard, DownloadLeaderboardEntries(ForUsers), UploadLeaderboardScore | a call whose result arrives with bIOFailure set and a zeroed structure (k_ESteamAPICallFailureNetworkFailure: "the network connection to the Steam servers has been lost, or was already broken"). ForUsers with more than 100 users: k_uAPICallInvalid (documented) |
| GetDownloadedLeaderboardEntry / UGCRead | false / 0: nothing was downloaded |
| SetAchievement / StoreStats | true. StoreStats keeps the unlocks in `...\367500\achievements.txt` (the runtime's own format) and posts UserStatsStored_t (k_EResultOK) plus one UserAchievementStored_t per new unlock. Stats are available from the start: the SDK documents RequestCurrentStats as no longer necessary, and the game never calls it |

Unresolved: achievement names are not checked against Steam's schema,
which lives in the Steam client, not in the game's files. Every other slot
of the seven interfaces stops with its SDK name if the game ever reaches
it.

### Saves

- **Writes.** The game's FileWrite lands in the sandbox, at
  `<sandbox>\steam\userdata\<account>\367500\remote\DDDA.sav`, and a new
  game creates its save there. Nothing outside the sandbox is written.
- **Import.** Call `steam::import_save(source, account, sandbox)`, or set
  `DDRUNTIME_IMPORT_SAVE` to the player's `...\userdata\<account>\367500\remote`
  folder at session start. The import:
  - reads the player's folder and never writes to it;
  - copies every file into the sandbox's remote folder;
  - keeps any file the sandbox already has, so the rebuilt game's progress
    is never overwritten;
  - makes the folder's account the sandbox user's, so the game's SteamID
    check on load passes.

### Verification and what is left

- **Verified on the branch.** The census (script output, cross-checked
  against Ghidra). `ddruntime` with the new `steam.rs` compiles without
  warnings (one full build in the heavy slot). That build stopped on five
  borrow errors in the test file only; those are fixed in 5ddf15e and not
  yet recompiled.
- **Unverified, waiting for the merge train** (batched verification
  rule): `cargo test -p ddruntime --test steam_saves`, seven tests:
  - the census bindings and slot names;
  - the local user, friends, utils and apps answers;
  - callbacks and their registration flags;
  - call results, their sizes and the offline failures;
  - RunCallbacks not nesting and deferring requests made inside a
    callback;
  - remote storage in the sandbox, including the refused names;
  - the import keeping the player's folder untouched and taking its
    account.
  - The last test needs the installed exe: the lifted container writer
    0x0040F1A0 plus FileWrite produce a 0x80000-byte `DDDA.sav` that
    `ddsave` parses (a synthetic XML, and the private save's own stream
    reproducing its header and stream byte for byte). It then imports the
    private save and checks GetSteamID against its `mSteamID`, printing
    neither value.
- **Not done:**
  - A save written by the rebuilt game as a whole. The boot does not reach
    gameplay yet, so the test covers the game's container writer and the
    Steam path, not the MtDTI serialisation 0x0040F6E0.
  - Achievement names are not checked against Steam's schema.
  - The import does not read the player's persona name (it lives in
    Steam's `loginusers.vdf`; the default is "Arisen").
  - Nothing is compared yet against the original game running on real
    Steam.
