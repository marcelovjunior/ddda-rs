//! ADVAPI32 registry services: a read-only model holding the keys the game
//! reads, with Windows' answers for everything else (unknown keys do not
//! exist: ERROR_FILE_NOT_FOUND).
//!
//! The game reads one key, twice (Ghidra):
//! - `sAI`'s constructor 0x00F79F70 (RegOpenKeyExA at 0x00F79FE1,
//!   RegQueryValueExA at 0x00F7A00D): HKLM\HARDWARE\DESCRIPTION\System\
//!   CentralProcessor\0, value `~MHz` (REG_DWORD, stored x 1,000,000 as a
//!   64-bit clock rate at sAI+0x20).
//! - the system-information collector 0x00DBD170 (0x00DBD1BD, 0x00DBD1E5,
//!   RegCloseKey 0x00DBD22A): the same key, `ProcessorNameString`
//!   (REG_SZ, copied with runs of spaces collapsed).
//!
//! The values are those of one real machine, recorded like the D3D9
//! profile (the maintainer's, 2026-10-11, `reg query` of that key): a
//! fixed answer, so a session does not depend on the host. Only processor
//! 0 exists, matching GetSystemInfo's one processor (crate::win32).
//!
//! The behaviour of every call follows Microsoft Learn (RegOpenKeyExA,
//! RegQueryValueExA, RegCloseKey) and was measured on Windows 11 26200
//! with research/runtime/windows/Reg.cs where the documentation is silent
//! (error results, what the out-parameters hold on failure).
use crate::services::{arg, ret, with_state};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;
use std::collections::HashMap;

const ERROR_SUCCESS: u32 = 0;
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_INVALID_HANDLE: u32 = 6;
const ERROR_INVALID_PARAMETER: u32 = 87;
const ERROR_BAD_PATHNAME: u32 = 161;
const ERROR_MORE_DATA: u32 = 234;

const HKEY_LOCAL_MACHINE: u32 = 0x8000_0002;
const REG_SZ: u32 = 1;
const REG_DWORD: u32 = 4;
/// Access rights that write (winnt.h): KEY_SET_VALUE, KEY_CREATE_SUB_KEY,
/// KEY_CREATE_LINK, DELETE, WRITE_DAC, WRITE_OWNER, GENERIC_WRITE,
/// GENERIC_ALL. HKLM is not writable for the game (it runs asInvoker, as a
/// standard user); measured: ERROR_ACCESS_DENIED.
const WRITE_ACCESS: u32 = 0x2 | 0x4 | 0x20 | 0x1_0000 | 0x4_0000 | 0x8_0000 | 0x4000_0000 | 0x1000_0000;

/// Handles RegOpenKeyExA gives out (distinct from file and kernel handles).
const FIRST_HANDLE: u32 = 0x00E0_0004;

#[derive(Clone, Copy)]
enum Value {
    Sz(&'static str),
    Dword(u32),
}

/// The CPU key, upper-cased path below HKLM.
const CPU_KEY: &str = r"HARDWARE\DESCRIPTION\SYSTEM\CENTRALPROCESSOR\0";

/// The recorded values of HKLM\HARDWARE\DESCRIPTION\System\
/// CentralProcessor\0 (REG_SZ and REG_DWORD values of the classic set;
/// the vendor-specific microcode bookkeeping values are not modelled).
const CPU_VALUES: &[(&str, Value)] = &[
    ("Identifier", Value::Sz("AMD64 Family 25 Model 33 Stepping 2")),
    ("ProcessorNameString", Value::Sz("AMD Ryzen 7 5700X3D 8-Core Processor           ")),
    ("VendorIdentifier", Value::Sz("AuthenticAMD")),
    ("FeatureSet", Value::Dword(0x383B_3DFF)),
    ("~MHz", Value::Dword(3000)),
];

/// Keys that exist, as upper-cased paths below HKLM.
fn key_exists(path: &str) -> bool {
    CPU_KEY == path || CPU_KEY.starts_with(&format!("{path}\\"))
}

/// Open registry handles: handle -> (root, upper-cased path).
#[derive(Default)]
pub struct Keys {
    open: HashMap<u32, (u32, String)>,
    next: u32,
}

fn with_keys<R>(f: impl FnOnce(&mut Keys) -> R) -> R {
    with_state(|s| f(&mut s.registry))
}

/// What a key handle names: a predefined root or an open key.
fn resolve(h: u32) -> Option<(u32, String)> {
    if (0x8000_0000..=0x8000_0006).contains(&h) || h == 0x8000_0050 || h == 0x8000_0060 {
        return Some((h, String::new()));
    }
    with_keys(|k| k.open.get(&h).cloned())
}

/// ADVAPI32 `RegOpenKeyExA(hKey, lpSubKey, ulOptions, samDesired,
/// phkResult)` (Microsoft Learn "RegOpenKeyExA function (winreg.h)";
/// stdcall, 20 bytes). Names compare case-insensitively; empty path
/// components (doubled or trailing backslashes) are skipped and a NULL or
/// empty lpSubKey opens a new handle to hKey itself (all measured).
/// Returns ERROR_SUCCESS with *phkResult a new handle; on failure
/// *phkResult is NULL and the result ERROR_FILE_NOT_FOUND (no such key),
/// ERROR_ACCESS_DENIED (write access to HKLM), ERROR_BAD_PATHNAME (a
/// leading backslash) or ERROR_INVALID_HANDLE (hKey). Only HKLM has keys
/// in the model; the other roots have none.
pub fn reg_open_key_ex_a(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (h, sub, sam, out) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 3), arg(m, cpu, 4));
    let sub = if sub == 0 { String::new() } else { crate::user32::guest_str_a(m, sub) };
    let r = (|| {
        let (root, base) = resolve(h).ok_or(ERROR_INVALID_HANDLE)?;
        if sub.starts_with('\\') {
            return Err(ERROR_BAD_PATHNAME);
        }
        let mut parts: Vec<String> = base.split('\\').filter(|s| !s.is_empty()).map(str::to_string).collect();
        parts.extend(sub.split('\\').filter(|s| !s.is_empty()).map(str::to_ascii_uppercase));
        let path = parts.join("\\");
        // a root itself always exists; below the roots only HKLM has keys
        if !(path.is_empty() || (root == HKEY_LOCAL_MACHINE && key_exists(&path))) {
            return Err(ERROR_FILE_NOT_FOUND);
        }
        if sam & WRITE_ACCESS != 0 {
            return Err(ERROR_ACCESS_DENIED);
        }
        Ok(with_keys(|k| {
            if k.next == 0 {
                k.next = FIRST_HANDLE;
            }
            let handle = k.next;
            k.next += 4;
            k.open.insert(handle, (root, path));
            handle
        }))
    })();
    if out != 0 {
        m.set_u32(out, *r.as_ref().unwrap_or(&0));
    }
    cpu.eax = r.err().unwrap_or(ERROR_SUCCESS);
    ret(m, cpu, 20);
}

/// ADVAPI32 `RegQueryValueExA(hKey, lpValueName, lpReserved, lpType,
/// lpData, lpcbData)` (Microsoft Learn "RegQueryValueExA function
/// (winreg.h)"; stdcall, 24 bytes). *lpType gets the type and *lpcbData
/// the size (REG_SZ: the code page 1252 string with its NUL). lpData NULL
/// asks for the size only. A buffer smaller than the data: ERROR_MORE_DATA
/// with the size and nothing written. Measured details: when the buffer
/// also holds the UTF-16 form of a string, Windows reads that into the
/// buffer and converts it in place, so the bytes after the ANSI string
/// hold the rest of the UTF-16 text (reproduced); lpcbData NULL with
/// lpData set is ERROR_INVALID_PARAMETER (lpType untouched); a missing
/// value is ERROR_FILE_NOT_FOUND and a bad handle ERROR_INVALID_HANDLE,
/// both with *lpType = 0. Values of a key that are not in the model stop
/// the session (they exist on Windows; an answer would be invented),
/// except in keys the model has no values for.
pub fn reg_query_value_ex_a(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (h, name_p, type_p, data, cb_p) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 3), arg(m, cpu, 4), arg(m, cpu, 5));
    let name = if name_p == 0 { String::new() } else { crate::user32::guest_str_a(m, name_p) };
    let r = (|| {
        if data != 0 && cb_p == 0 {
            return Err((ERROR_INVALID_PARAMETER, false));
        }
        let (root, path) = resolve(h).ok_or((ERROR_INVALID_HANDLE, true))?;
        let value = if root == HKEY_LOCAL_MACHINE && path == CPU_KEY {
            match CPU_VALUES.iter().find(|(n, _)| n.eq_ignore_ascii_case(&name)) {
                Some(&(_, v)) => v,
                None if name.is_empty() => return Err((ERROR_FILE_NOT_FOUND, true)), // no default value
                None => panic!("RegQueryValueExA: value {name:?} of {CPU_KEY} is not modelled (crate::registry)"),
            }
        } else {
            return Err((ERROR_FILE_NOT_FOUND, true));
        };
        Ok(value)
    })();
    let value = match r {
        Ok(v) => v,
        Err((e, zero_type)) => {
            if zero_type && type_p != 0 {
                m.set_u32(type_p, 0);
            }
            cpu.eax = e;
            return ret(m, cpu, 24);
        }
    };
    let (kind, bytes, wide): (u32, Vec<u8>, Vec<u8>) = match value {
        Value::Dword(d) => (REG_DWORD, d.to_le_bytes().to_vec(), Vec::new()),
        Value::Sz(s) => {
            let ansi: Vec<u8> = s.chars().map(|c| crate::text::encode_1252(c as u16, true).unwrap_or(b'?')).chain([0]).collect();
            let wide = s.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
            (REG_SZ, ansi, wide)
        }
    };
    if type_p != 0 {
        m.set_u32(type_p, kind);
    }
    cpu.eax = ERROR_SUCCESS;
    if cb_p != 0 {
        let cb = m.u32(cb_p);
        m.set_u32(cb_p, bytes.len() as u32);
        if data != 0 {
            if (cb as usize) < bytes.len() {
                cpu.eax = ERROR_MORE_DATA;
            } else {
                if kind == REG_SZ && cb as usize >= wide.len() {
                    for (k, &b) in wide.iter().enumerate() {
                        m.set_u8(data + k as u32, b);
                    }
                }
                for (k, &b) in bytes.iter().enumerate() {
                    m.set_u8(data + k as u32, b);
                }
            }
        }
    }
    ret(m, cpu, 24);
}

/// ADVAPI32 `RegCloseKey(hKey)` (Microsoft Learn "RegCloseKey function
/// (winreg.h)"): ERROR_SUCCESS for an open key; ERROR_INVALID_HANDLE for
/// anything else, a predefined root included (measured on Windows 11).
pub fn reg_close_key(m: &mut dyn Mem, cpu: &mut Cpu) {
    let h = arg(m, cpu, 0);
    cpu.eax = if with_keys(|k| k.open.remove(&h).is_some()) { ERROR_SUCCESS } else { ERROR_INVALID_HANDLE };
    ret(m, cpu, 4);
}

pub const APIS: &[(&str, &str, fn(&mut dyn Mem, &mut Cpu))] = &[
    ("ADVAPI32.dll", "RegOpenKeyExA", reg_open_key_ex_a),
    ("ADVAPI32.dll", "RegQueryValueExA", reg_query_value_ex_a),
    ("ADVAPI32.dll", "RegCloseKey", reg_close_key),
];
