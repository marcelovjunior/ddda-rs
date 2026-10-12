//! System services (brief L17): the time zone and FILETIME conversions,
//! Global/Local memory, the process environment, standard handles and the
//! (absent) console, the OS version checks, DebugBreak/RaiseException,
//! PSAPI GetModuleFileNameExA and WININET InternetGetConnectedState.
//!
//! Each follows the Microsoft Learn contract with stdcall cleanup. Where
//! the documentation leaves an answer open, the behaviour was measured on
//! Windows 11 (build 26200) with research/runtime/windows/Probe2.cs,
//! Probe3.cs, NoConsole.cs and Unmanifested.cs; the comments say which.
use crate::services::{arg, ret, with_state};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;
use std::collections::HashMap;

const ERROR_NOT_ENOUGH_MEMORY: u32 = 8;
const ERROR_INVALID_HANDLE: u32 = 6;
const ERROR_INVALID_PARAMETER: u32 = 87;
const ERROR_DISCARDED: u32 = 157;
const ERROR_NOT_LOCKED: u32 = 158;
const ERROR_BAD_ARGUMENTS: u32 = 160;
const ERROR_OLD_WIN_VERSION: u32 = 1150;

/// Runtime state of these services (in crate::services::State).
#[derive(Default)]
pub struct System {
    /// GMEM_MOVEABLE/LMEM_MOVEABLE handles: handle -> block.
    movable: HashMap<u32, Movable>,
    next_handle: u32,
    /// Fixed blocks handed out by GlobalAlloc/LocalAlloc.
    fixed: std::collections::HashSet<u32>,
    /// The environment the guest set (SetEnvironmentVariableA), in
    /// Windows' order: sorted by name, case-insensitively.
    pub(crate) environment: Vec<(String, String)>,
    /// SetStdHandle: input, output, error (NULL in a GUI process).
    pub(crate) std_handles: [u32; 3],
}

struct Movable {
    /// The block (0: allocated with 0 bytes, "discarded").
    block: u32,
    locks: u32,
}

fn with_system<R>(f: impl FnOnce(&mut System) -> R) -> R {
    with_state(|s| f(&mut s.system))
}

fn set_error(e: u32) {
    with_state(|s| s.last_error = e);
}

// --- time ---------------------------------------------------------------

/// The runtime's time zone: Coordinated Universal Time, fixed, so the
/// guest's local time does not depend on the host (determinism, and the
/// same clock for every co-op peer). It is Windows' own "UTC" zone: TZI
/// all zero (no bias, no transition dates), standard and daylight names
/// "Coordinated Universal Time" (HKLM\SOFTWARE\Microsoft\Windows
/// NT\CurrentVersion\Time Zones\UTC on Windows 11).
const UTC_NAME: &str = "Coordinated Universal Time";

/// KERNEL32 `GetTimeZoneInformation(TIME_ZONE_INFORMATION*)` (Microsoft
/// Learn "GetTimeZoneInformation function (timezoneapi.h)"): fills the 172
/// bytes (Bias +0, StandardName[32] +4, StandardDate +68, StandardBias
/// +84, DaylightName[32] +88, DaylightDate +152, DaylightBias +168) and
/// returns TIME_ZONE_ID_UNKNOWN (0): the zone has no transition dates. The
/// caller is the CRT's `__tzset` 0x01302F75.
pub fn get_time_zone_information(m: &mut dyn Mem, cpu: &mut Cpu) {
    let p = arg(m, cpu, 0);
    crate::services::zero(m, p, 172);
    for name_at in [p + 4, p + 88] {
        for (k, u) in UTC_NAME.encode_utf16().enumerate() {
            m.set_u16(name_at + 2 * k as u32, u);
        }
    }
    cpu.eax = 0; // TIME_ZONE_ID_UNKNOWN
    ret(m, cpu, 4);
}

/// Days since 1601-01-01 to (year, month, day) in the proleptic Gregorian
/// calendar (H. Hinnant's civil_from_days, shifted to 1601).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // days since 0000-03-01: 1601-01-01 is 134,774 days before 1970-01-01,
    // which is day 719,468
    let z = days + 719_468 - 134_774;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

/// KERNEL32 `FileTimeToSystemTime(const FILETIME*, SYSTEMTIME*)` (Microsoft
/// Learn "FileTimeToSystemTime function (timezoneapi.h)"): 100 ns ticks
/// since 1601-01-01 to wYear, wMonth, wDayOfWeek (0 = Sunday; 1601-01-01
/// was a Monday), wDay, wHour, wMinute, wSecond, wMilliseconds. TRUE; a
/// value with the top bit set fails with ERROR_INVALID_PARAMETER and the
/// SYSTEMTIME untouched (measured: 0x7FFFFFFFFFFFFFFF is 30828-09-14
/// 02:48:05.477, a Thursday). The caller is 0x00D0CB80 (file times to
/// time_t).
pub fn file_time_to_system_time(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (ft, st) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let t = m.u32(ft) as u64 | (m.u32(ft + 4) as u64) << 32;
    cpu.eax = if t >> 63 != 0 {
        set_error(ERROR_INVALID_PARAMETER);
        0
    } else {
        let ms_total = t / 10_000;
        let days = (ms_total / 86_400_000) as i64;
        let ms_day = ms_total % 86_400_000;
        let (y, mo, d) = civil_from_days(days);
        let fields = [
            y as u16, mo as u16, ((days + 1) % 7) as u16, d as u16,
            (ms_day / 3_600_000) as u16, (ms_day / 60_000 % 60) as u16, (ms_day / 1000 % 60) as u16, (ms_day % 1000) as u16,
        ];
        for (k, v) in fields.into_iter().enumerate() {
            m.set_u16(st + 2 * k as u32, v);
        }
        1
    };
    ret(m, cpu, 8);
}

/// KERNEL32 `FileTimeToLocalFileTime(const FILETIME*, FILETIME*)`
/// (Microsoft Learn "FileTimeToLocalFileTime function (fileapi.h)"): UTC
/// plus the zone's current bias, which is 0 in the runtime's UTC zone: a
/// copy. TRUE (Windows 11 also accepts values with the top bit set,
/// measured).
pub fn file_time_to_local_file_time(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (from, to) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let (lo, hi) = (m.u32(from), m.u32(from + 4));
    m.set_u32(to, lo);
    m.set_u32(to + 4, hi);
    cpu.eax = 1;
    ret(m, cpu, 8);
}

// --- Global/Local memory ------------------------------------------------

/// Moveable handles live in this range: never a heap address, so a handle
/// and a fixed pointer cannot be confused.
const MOVABLE_HANDLES: u32 = 0x0E80_0000;
const GMEM_MOVEABLE: u32 = 0x2;
const GMEM_ZEROINIT: u32 = 0x40;

/// GlobalAlloc/LocalAlloc: the flags Windows accepts (measured: GlobalAlloc
/// takes 0x7F72, LocalAlloc 0x0F72; any other bit fails with
/// ERROR_INVALID_PARAMETER). The obsolete flags among them (discardable,
/// DDE share, no-compact, ...) change nothing.
fn alloc(m: &mut dyn Mem, flags: u32, size: u32, valid: u32) -> Result<u32, u32> {
    if flags & !valid != 0 {
        return Err(ERROR_INVALID_PARAMETER);
    }
    let block = if flags & GMEM_MOVEABLE != 0 && size == 0 {
        0 // a moveable object of 0 bytes is allocated discarded
    } else {
        let b = with_state(|s| s.heap.alloc(size)).ok_or(ERROR_NOT_ENOUGH_MEMORY)?;
        if flags & GMEM_ZEROINIT != 0 {
            crate::services::zero(m, b, size);
        }
        b
    };
    Ok(with_system(|s| {
        if flags & GMEM_MOVEABLE == 0 {
            s.fixed.insert(block);
            return block;
        }
        if s.next_handle == 0 {
            s.next_handle = MOVABLE_HANDLES;
        }
        let h = s.next_handle;
        s.next_handle += 8;
        s.movable.insert(h, Movable { block, locks: 0 });
        h
    }))
}

fn alloc_api(m: &mut dyn Mem, cpu: &mut Cpu, valid: u32) {
    let (flags, size) = (arg(m, cpu, 0), arg(m, cpu, 1));
    cpu.eax = alloc(m, flags, size, valid).unwrap_or_else(|e| {
        set_error(e);
        0
    });
    ret(m, cpu, 8);
}

/// KERNEL32 `GlobalAlloc(uFlags, dwBytes)` (Microsoft Learn "GlobalAlloc
/// function (winbase.h)"): GMEM_FIXED returns the block itself,
/// GMEM_MOVEABLE a handle for GlobalLock; GMEM_ZEROINIT zeroes. NULL with
/// ERROR_INVALID_PARAMETER (flags) or ERROR_NOT_ENOUGH_MEMORY. Callers:
/// MtClipboardStream::vf10 at 0x00D01E86 (GHND), the restart helpers
/// 0x011797F0/0x011799A0 (GPTR).
pub fn global_alloc(m: &mut dyn Mem, cpu: &mut Cpu) {
    alloc_api(m, cpu, 0x7F72)
}

/// KERNEL32 `LocalAlloc(uFlags, uBytes)` (Microsoft Learn "LocalAlloc
/// function (winbase.h)"): as GlobalAlloc with the LMEM flags (the same
/// heap and handles; LocalLock/LocalFree are not imported). Caller:
/// `___delayLoadHelper2@8` at 0x0130A469 (LPTR).
pub fn local_alloc(m: &mut dyn Mem, cpu: &mut Cpu) {
    alloc_api(m, cpu, 0x0F72)
}

/// KERNEL32 `GlobalLock(hMem)` (Microsoft Learn "GlobalLock function
/// (winbase.h)"): a fixed block is its own pointer (no lock count); a
/// moveable handle counts a lock and returns its block. NULL with
/// ERROR_DISCARDED (a 0-byte moveable object) or ERROR_INVALID_HANDLE
/// (measured for freed handles).
pub fn global_lock(m: &mut dyn Mem, cpu: &mut Cpu) {
    let h = arg(m, cpu, 0);
    let r = with_system(|s| {
        if s.fixed.contains(&h) {
            return Ok(h);
        }
        match s.movable.get_mut(&h) {
            Some(mv) if mv.block == 0 => Err(ERROR_DISCARDED),
            Some(mv) => {
                mv.locks += 1;
                Ok(mv.block)
            }
            None => Err(ERROR_INVALID_HANDLE),
        }
    });
    cpu.eax = r.unwrap_or_else(|e| {
        set_error(e);
        0
    });
    ret(m, cpu, 4);
}

/// KERNEL32 `GlobalUnlock(hMem)` (Microsoft Learn "GlobalUnlock function
/// (winbase.h)"): for a moveable object, one lock less; nonzero while
/// still locked, else 0 with the last error set to NO_ERROR. Unlocking an
/// unlocked object fails with ERROR_NOT_LOCKED; a fixed block answers
/// TRUE (both measured). Unknown handles: ERROR_INVALID_HANDLE.
pub fn global_unlock(m: &mut dyn Mem, cpu: &mut Cpu) {
    let h = arg(m, cpu, 0);
    let r = with_system(|s| {
        if s.fixed.contains(&h) {
            return Ok(true);
        }
        match s.movable.get_mut(&h) {
            Some(mv) if mv.locks == 0 => Err(ERROR_NOT_LOCKED),
            Some(mv) => {
                mv.locks -= 1;
                Ok(mv.locks > 0)
            }
            None => Err(ERROR_INVALID_HANDLE),
        }
    });
    cpu.eax = match r {
        Ok(true) => 1,
        Ok(false) => {
            set_error(0);
            0
        }
        Err(e) => {
            set_error(e);
            0
        }
    };
    ret(m, cpu, 4);
}

/// KERNEL32 `GlobalFree(hMem)` (Microsoft Learn "GlobalFree function
/// (winbase.h)"): frees a fixed block or a moveable object (locked or
/// not, measured) and returns NULL; NULL itself is accepted. Anything else
/// (also a second free) returns the handle with ERROR_INVALID_HANDLE.
pub fn global_free(m: &mut dyn Mem, cpu: &mut Cpu) {
    let h = arg(m, cpu, 0);
    let block = with_system(|s| {
        if h == 0 {
            Some(0)
        } else if s.fixed.remove(&h) {
            Some(h)
        } else {
            s.movable.remove(&h).map(|mv| mv.block)
        }
    });
    cpu.eax = match block {
        Some(b) => {
            if b != 0 {
                with_state(|s| s.heap.free(b)).expect("a Global/Local block is a live heap block");
            }
            0
        }
        None => {
            set_error(ERROR_INVALID_HANDLE);
            h
        }
    };
    ret(m, cpu, 4);
}

// --- environment ----------------------------------------------------------

/// KERNEL32 `SetEnvironmentVariableA(lpName, lpValue)` (Microsoft Learn
/// "SetEnvironmentVariable function (processenv.h)"): sets, replaces
/// (keeping the existing name's case) or, with lpValue NULL, deletes the
/// variable (deleting a missing one succeeds). FALSE with
/// ERROR_INVALID_PARAMETER for a NULL or empty name or one containing '='
/// after its first character (all measured). The environment starts empty
/// (crate::win32 GetEnvironmentStringsW); names compare case-insensitively
/// and stay sorted that way, as Windows keeps them (measured). Caller: the
/// CRT's `___crtsetenv` at 0x0130A175 (`_putenv`).
pub fn set_environment_variable_a(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (n, v) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let name = if n == 0 { String::new() } else { ansi_str(m, n) };
    cpu.eax = if name.is_empty() || name.chars().skip(1).any(|c| c == '=') {
        set_error(ERROR_INVALID_PARAMETER);
        0
    } else {
        let value = (v != 0).then(|| ansi_str(m, v));
        with_system(|s| {
            let key = name.to_uppercase();
            let at = s.environment.binary_search_by(|(k, _)| k.to_uppercase().cmp(&key));
            match (at, value) {
                (Ok(i), Some(val)) => s.environment[i].1 = val,
                (Ok(i), None) => {
                    s.environment.remove(i);
                }
                (Err(i), Some(val)) => s.environment.insert(i, (name, val)),
                (Err(_), None) => {}
            }
        });
        1
    };
    ret(m, cpu, 8);
}

/// A NUL-terminated code page 1252 string.
fn ansi_str(m: &dyn Mem, p: u32) -> String {
    let mut s = String::new();
    for k in 0.. {
        let b = m.u8(p.wrapping_add(k));
        if b == 0 {
            break;
        }
        s.push(char::from_u32(crate::text::decode_1252(b) as u32).unwrap_or('\u{FFFD}'));
    }
    s
}

/// The environment block GetEnvironmentStringsW hands out: "name=value\0"
/// per variable, then a final NUL (two NULs when empty).
pub(crate) fn environment_block() -> Vec<u16> {
    let vars = with_system(|s| s.environment.clone());
    let mut out = Vec::new();
    for (k, v) in vars {
        out.extend(format!("{k}={v}").encode_utf16());
        out.push(0);
    }
    if out.is_empty() {
        out.push(0);
    }
    out.push(0);
    out
}

// --- standard handles and the console ---------------------------------------

/// The STD_*_HANDLE index of nStdHandle (-10, -11, -12).
pub(crate) fn std_index(n: u32) -> Option<usize> {
    match n as i32 {
        -10 => Some(0),
        -11 => Some(1),
        -12 => Some(2),
        _ => None,
    }
}

/// KERNEL32 `SetStdHandle(nStdHandle, hHandle)` (Microsoft Learn
/// "SetStdHandle function"): records the handle GetStdHandle answers
/// afterwards. FALSE with ERROR_INVALID_HANDLE for another nStdHandle
/// (measured). Callers: the CRT's `__set_osfhnd`/`__free_osfhnd`.
pub fn set_std_handle(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (n, h) = (arg(m, cpu, 0), arg(m, cpu, 1));
    cpu.eax = match std_index(n) {
        Some(i) => {
            with_system(|s| s.std_handles[i] = h);
            1
        }
        None => {
            set_error(ERROR_INVALID_HANDLE);
            0
        }
    };
    ret(m, cpu, 8);
}

/// KERNEL32 `GetConsoleMode(hConsoleHandle, lpMode)`: the game is a GUI
/// process without a console, so no handle is a console handle: FALSE with
/// ERROR_INVALID_HANDLE, *lpMode untouched (measured in a console-less
/// process). Caller: the CRT's `__write_nolock` at 0x01303E12.
pub fn get_console_mode(m: &mut dyn Mem, cpu: &mut Cpu) {
    set_error(ERROR_INVALID_HANDLE);
    cpu.eax = 0;
    ret(m, cpu, 8);
}

/// KERNEL32 `GetConsoleCP()`: 0 with ERROR_INVALID_HANDLE without a
/// console (measured).
pub fn get_console_cp(m: &mut dyn Mem, cpu: &mut Cpu) {
    set_error(ERROR_INVALID_HANDLE);
    cpu.eax = 0;
    ret(m, cpu, 0);
}

/// KERNEL32 `WriteConsoleW(hConsoleOutput, lpBuffer, nChars,
/// lpCharsWritten, lpReserved)`: FALSE with ERROR_INVALID_HANDLE without a
/// console, *lpCharsWritten untouched (measured). Caller: the CRT's
/// `__putwch_nolock` at 0x01308668.
pub fn write_console_w(m: &mut dyn Mem, cpu: &mut Cpu) {
    set_error(ERROR_INVALID_HANDLE);
    cpu.eax = 0;
    ret(m, cpu, 20);
}

// --- OS version -------------------------------------------------------------

/// The version VerifyVersionInfo compares against. DDDA.exe's manifest has
/// no `supportedOS` (compatibility) section, so Windows 10 and 11 answer as
/// Windows 8 (Microsoft Learn, VerifyVersionInfoA remarks: "If the
/// application has no manifest, VerifyVersionInfo behaves as if the
/// operating system version is Windows 8 (6.2)"). Measured on Windows 11
/// 26200 in an exe with the same kind of manifest: 6.2 build 9200,
/// VER_PLATFORM_WIN32_NT, service pack 0.0, suite VER_SUITE_SINGLEUSERTS,
/// product VER_NT_WORKSTATION.
pub const VERSION: OsVersion = OsVersion { major: 6, minor: 2, build: 9200, platform: 2, sp_major: 0, sp_minor: 0, suite: 0x100, product: 1 };

#[derive(Clone, Copy, Debug)]
pub struct OsVersion {
    pub major: u32,
    pub minor: u32,
    pub build: u32,
    pub platform: u32,
    pub sp_major: u16,
    pub sp_minor: u16,
    pub suite: u16,
    pub product: u8,
}

// winnt.h VER_* type bits, in VerSetConditionMask's priority order, with
// the 3-bit slot of each condition in the mask
const VER_SLOTS: [(u32, u32); 8] = [
    (0x80, 7), // VER_PRODUCT_TYPE
    (0x40, 6), // VER_SUITENAME
    (0x20, 5), // VER_SERVICEPACKMAJOR
    (0x10, 4), // VER_SERVICEPACKMINOR
    (0x08, 3), // VER_PLATFORMID
    (0x04, 2), // VER_BUILDNUMBER
    (0x02, 1), // VER_MAJORVERSION
    (0x01, 0), // VER_MINORVERSION
];

/// VerSetConditionMask's arithmetic, as measured: TypeMask 0 leaves the
/// mask unchanged; otherwise bit 63 is set and Condition & 7 is ORed into
/// the slot of the highest-priority type bit in TypeMask (product type,
/// suite, service pack major, minor, platform, build, major, minor): one
/// call sets one condition, so VerSetConditionMask(0, MAJOR|MINOR, GE)
/// (what the game passes) sets only the major one.
pub fn ver_set_condition_mask_value(mask: u64, type_mask: u32, condition: u8) -> u64 {
    if type_mask == 0 {
        return mask;
    }
    let mut out = mask | 1 << 63;
    if let Some(&(_, slot)) = VER_SLOTS.iter().find(|(bit, _)| type_mask & bit != 0) {
        out |= ((condition & 7) as u64) << (3 * slot);
    }
    out
}

/// KERNEL32 `VerSetConditionMask(ConditionMask, TypeMask, Condition)`
/// (Microsoft Learn "VerSetConditionMask function (winnt.h)"): stdcall
/// with a ULONGLONG first (two stack slots, 16 bytes in all), result in
/// edx:eax. Caller: sPad's constructor 0x00DFA0C0.
pub fn ver_set_condition_mask(m: &mut dyn Mem, cpu: &mut Cpu) {
    let mask = arg(m, cpu, 0) as u64 | (arg(m, cpu, 1) as u64) << 32;
    let r = ver_set_condition_mask_value(mask, arg(m, cpu, 2), arg(m, cpu, 3) as u8);
    cpu.eax = r as u32;
    cpu.edx = (r >> 32) as u32;
    ret(m, cpu, 16);
}

fn compare(have: u32, want: u32, condition: u64) -> bool {
    match condition {
        1 => have == want,
        2 => have > want,
        3 => have >= want,
        4 => have < want,
        5 => have <= want,
        _ => false, // measured: other conditions fail on these fields
    }
}

/// How a level's condition combines with the levels above it in the
/// major/minor/service-pack hierarchy: a following condition is used only
/// when it agrees in direction with the previous one, else the previous
/// one stays (measured: major >= then minor <= compares the minor with >=;
/// major <= then minor >= compares with <=; a 0 condition inherits).
/// (The same rule as Wine's RtlVerifyVersionInfo, which these
/// measurements agree with; after a 0 condition the hierarchy's condition
/// sticks.)
fn combine(last: &mut u64, condition: u64) -> u64 {
    match *last {
        0 => *last = condition,
        1 if (1..=5).contains(&condition) => {
            *last = condition;
            return condition;
        }
        2 | 3 if (1..=3).contains(&condition) => return condition,
        4 | 5 if condition == 1 || (4..=5).contains(&condition) => return condition,
        _ => {}
    }
    if condition == 0 {
        *last |= 0x10;
    }
    *last & 0xF
}

/// VerifyVersionInfo's decision against `os` (see [`verify_version_info_a`]).
pub fn verify_version(os: &OsVersion, info: &OsVersion, type_mask: u32, mask: u64) -> Result<bool, u32> {
    if type_mask == 0 {
        return Err(ERROR_BAD_ARGUMENTS);
    }
    let cond = |slot: u32| (mask >> (3 * slot)) & 7;
    // independent fields
    for (bit, slot, have, want) in [
        (0x80, 7, os.product as u32, info.product as u32),
        (0x08, 3, os.platform, info.platform),
        (0x04, 2, os.build, info.build),
    ] {
        if type_mask & bit != 0 && !compare(have, want, cond(slot)) {
            return Ok(false);
        }
    }
    if type_mask & 0x40 != 0 {
        let (have, want) = (os.suite, info.suite);
        let ok = match cond(6) {
            6 => have & want == want, // VER_AND
            7 => have & want != 0,    // VER_OR
            c => panic!("VerifyVersionInfoA: VER_SUITENAME with condition {c} is not modelled (crate::system)"),
        };
        if !ok {
            return Ok(false);
        }
    }
    // the hierarchy: the first tested level whose values differ decides
    let mut last = 0;
    for (bit, slot, have, want) in [
        (0x02, 1, os.major, info.major),
        (0x01, 0, os.minor, info.minor),
        (0x20, 5, os.sp_major as u32, info.sp_major as u32),
        (0x10, 4, os.sp_minor as u32, info.sp_minor as u32),
    ] {
        if type_mask & bit == 0 {
            continue;
        }
        let c = combine(&mut last, cond(slot));
        if have != want {
            return Ok(compare(have, want, c));
        }
    }
    Ok(true)
}

/// KERNEL32 `VerifyVersionInfoA(OSVERSIONINFOEXA*, dwTypeMask,
/// dwlConditionMask)` (Microsoft Learn "VerifyVersionInfoA function
/// (winbase.h)"; stdcall, 16 bytes of arguments): TRUE if [`VERSION`]
/// meets the requirements, else FALSE with ERROR_OLD_WIN_VERSION;
/// ERROR_BAD_ARGUMENTS for a zero dwTypeMask. Product type, platform and
/// build compare independently; major, minor and service pack compare in
/// that order and the first level whose values differ decides (the
/// documented hierarchy; with equal values at every tested level the
/// answer is TRUE whatever the conditions, measured). A condition mask
/// without bit 63 (not made by VerSetConditionMask) behaves differently on
/// Windows and is not modelled (stop). The game's only call, sPad's
/// constructor 0x00DFA0C0, asks for major.minor >= 5.1 (Windows XP).
pub fn verify_version_info_a(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (p, type_mask) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let mask = arg(m, cpu, 2) as u64 | (arg(m, cpu, 3) as u64) << 32;
    if type_mask & !0xFF != 0 || (type_mask != 0 && mask >> 63 == 0) {
        panic!("VerifyVersionInfoA(type {type_mask:#x}, mask {mask:#x}) is not modelled (crate::system)");
    }
    let info = OsVersion {
        major: m.u32(p + 4), minor: m.u32(p + 8), build: m.u32(p + 12), platform: m.u32(p + 16),
        sp_major: m.u16(p + 148), sp_minor: m.u16(p + 150), suite: m.u16(p + 152), product: m.u8(p + 154),
    };
    cpu.eax = match verify_version(&VERSION, &info, type_mask, mask) {
        Ok(true) => 1,
        Ok(false) => {
            set_error(ERROR_OLD_WIN_VERSION);
            0
        }
        Err(e) => {
            set_error(e);
            0
        }
    };
    ret(m, cpu, 16);
}

// --- debugging and exceptions ----------------------------------------------

/// KERNEL32 `DebugBreak()`: dispatch a breakpoint at the software API
/// boundary (see `seh` for continuation limitations). The game's call is the
/// reference-count check in 0x00DB9740 (a count below 1).
pub fn debug_break(m: &mut dyn Mem, cpu: &mut Cpu) {
    crate::seh::debug_break(m, cpu);
}

/// KERNEL32 RaiseException: the software exception boundary is dispatched
/// through the guest SEH chain. See `seh` for continuation limitations.
pub fn raise_exception(m: &mut dyn Mem, cpu: &mut Cpu) {
    crate::seh::raise_exception(m, cpu);
}
// --- PSAPI and WININET --------------------------------------------------------

/// PSAPI `GetModuleFileNameExA(hProcess, hModule, lpFilename, nSize)`
/// (Microsoft Learn "GetModuleFileNameExA function (psapi.h)"): for this
/// process (the GetCurrentProcess pseudo handle) and the exe (hModule NULL
/// or the image base), the exe's path, truncated to nSize - 1 characters
/// and NUL-terminated; returns the characters copied. Another process:
/// FALSE with ERROR_INVALID_HANDLE. Other modules are not modelled (stop).
/// Caller: the restart helper 0x011797F0 (thunk 0x012E1B18).
pub fn get_module_file_name_ex_a(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (process, module, buf, size) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3));
    if process != crate::win32::PSEUDO_PROCESS {
        set_error(ERROR_INVALID_HANDLE);
        cpu.eax = 0;
        return ret(m, cpu, 16);
    }
    if module != 0 && module != crate::win32::IMAGE_BASE {
        panic!("GetModuleFileNameExA of module {module:#x} is not modelled (crate::system)");
    }
    let path = with_state(|s| s.exe_path.clone());
    let n = path.len().min(size.saturating_sub(1) as usize);
    if size > 0 {
        for (k, &b) in path.as_bytes()[..n].iter().chain([0u8].iter()).enumerate() {
            m.set_u8(buf.wrapping_add(k as u32), b);
        }
    }
    cpu.eax = n as u32;
    ret(m, cpu, 16);
}

/// WININET `InternetGetConnectedState(lpdwFlags, dwReserved)` (Microsoft
/// Learn "InternetGetConnectedState function (wininet.h)"): the runtime
/// has no network: FALSE, *lpdwFlags = 0. A real offline machine may set
/// configuration bits (INTERNET_CONNECTION_CONFIGURED, RAS_INSTALLED); the
/// game's only caller, 0x00D2A390, reads just the result and the LAN and
/// modem bits (flags & 3), which are 0 offline in every case.
pub fn internet_get_connected_state(m: &mut dyn Mem, cpu: &mut Cpu) {
    let flags = arg(m, cpu, 0);
    if flags != 0 {
        m.set_u32(flags, 0);
    }
    cpu.eax = 0;
    ret(m, cpu, 8);
}

pub const APIS: &[(&str, &str, fn(&mut dyn Mem, &mut Cpu))] = &[
    ("KERNEL32.dll", "GetTimeZoneInformation", get_time_zone_information),
    ("KERNEL32.dll", "FileTimeToSystemTime", file_time_to_system_time),
    ("KERNEL32.dll", "FileTimeToLocalFileTime", file_time_to_local_file_time),
    ("KERNEL32.dll", "GlobalAlloc", global_alloc),
    ("KERNEL32.dll", "GlobalLock", global_lock),
    ("KERNEL32.dll", "GlobalUnlock", global_unlock),
    ("KERNEL32.dll", "GlobalFree", global_free),
    ("KERNEL32.dll", "LocalAlloc", local_alloc),
    ("KERNEL32.dll", "SetEnvironmentVariableA", set_environment_variable_a),
    ("KERNEL32.dll", "SetStdHandle", set_std_handle),
    ("KERNEL32.dll", "GetConsoleMode", get_console_mode),
    ("KERNEL32.dll", "GetConsoleCP", get_console_cp),
    ("KERNEL32.dll", "WriteConsoleW", write_console_w),
    ("KERNEL32.dll", "VerSetConditionMask", ver_set_condition_mask),
    ("KERNEL32.dll", "VerifyVersionInfoA", verify_version_info_a),
    ("KERNEL32.dll", "DebugBreak", debug_break),
    ("KERNEL32.dll", "RaiseException", raise_exception),
    ("PSAPI.DLL", "GetModuleFileNameExA", get_module_file_name_ex_a),
    ("WININET.dll", "InternetGetConnectedState", internet_get_connected_state),
];
