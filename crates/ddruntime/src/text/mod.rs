//! String and code-page services (KERNEL32): MultiByteToWideChar,
//! WideCharToMultiByte, GetStringTypeW, LCMapStringW and CompareStringW,
//! answered as an English (United States) Windows does (the runtime's
//! GetACP is 1252, crate::win32).
//!
//! Callers in DDDA.exe (Ghidra): the CRT's locale code (`setSBUpLow`
//! 0x012FABB0 through `__crtGetStringTypeA_stat` 0x01305900 and
//! `__crtLCMapStringA_stat` 0x012FDB1B; `__crtCompareStringA_stat`
//! 0x01309B41), its environment, time-zone (0x01302F75), console and
//! UTF-8 file paths, and the game's profile wrappers 0x00CFFD20 and
//! 0x00D00010 (CP_ACP, MB_PRECOMPOSED / flags 0).
//!
//! What is modelled, exactly as measured on Windows 11 (build 26200) with
//! the probes in research/runtime/windows (tables in [`tables`]):
//! - code pages 1252 (also CP_ACP and CP_THREAD_ACP) and 65001 (UTF-8);
//! - the character data (types, case, sort weights) of the 256 characters
//!   code page 1252 decodes to: everything the CRT can ask for, because
//!   it only passes text it converted from the ANSI code page;
//! - the en-US locale (0, LOCALE_USER_DEFAULT, LOCALE_SYSTEM_DEFAULT,
//!   0x409).
//!
//! Anything outside that (another code page, locale or character, or a
//! documented flag whose transformation is not modelled) stops the session
//! with a report naming it instead of answering something invented.
use crate::services::{arg, ret};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;

mod tables;
use tables::{BEST_FIT_1252, CP1252_80_9F, REPERTOIRE};

const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
const ERROR_INVALID_PARAMETER: u32 = 87;
const ERROR_INVALID_FLAGS: u32 = 1004;
const ERROR_NO_UNICODE_TRANSLATION: u32 = 1113;

const CP_UTF8: u32 = 65001;
const MB_PRECOMPOSED: u32 = 0x1;
const MB_COMPOSITE: u32 = 0x2;
const MB_USEGLYPHCHARS: u32 = 0x4;
const MB_ERR_INVALID_CHARS: u32 = 0x8;
const WC_COMPOSITECHECK: u32 = 0x200;
const WC_DISCARDNS: u32 = 0x10;
const WC_SEPCHARS: u32 = 0x20;
const WC_DEFAULTCHAR: u32 = 0x40;
const WC_ERR_INVALID_CHARS: u32 = 0x80;
const WC_NO_BEST_FIT_CHARS: u32 = 0x400;
const NORM_IGNORECASE: u32 = 0x1;
const NORM_IGNORENONSPACE: u32 = 0x2;
const SORT_STRINGSORT: u32 = 0x1000;
const LCMAP_LOWERCASE: u32 = 0x100;
const LCMAP_UPPERCASE: u32 = 0x200;
const LCMAP_SORTKEY: u32 = 0x400;

/// One character of the modelled repertoire (see [`tables::REPERTOIRE`]).
pub(crate) struct Char {
    pub(crate) code: u16,
    /// GetStringTypeW CT_CTYPE1, CT_CTYPE2, CT_CTYPE3.
    pub(crate) ctype: [u16; 3],
    pub(crate) lower: u16,
    pub(crate) upper: u16,
    /// In word sort the character goes to the special-weight section.
    pub(crate) word_special: bool,
    /// Sort elements: (primary weight, diacritic, case, case under
    /// NORM_IGNORECASE).
    pub(crate) sort: &'static [(u16, u8, u8, u8)],
}

fn set_error(e: u32) {
    crate::runtime::set_last_error(e);
}

/// Stop the session: the guest asked for something this model does not
/// cover (never answered with a guess).
fn not_modelled(api: &str, what: std::fmt::Arguments) -> ! {
    panic!("{api}: {what} is not modelled (crate::text)")
}

fn char_data(api: &str, u: u16) -> &'static Char {
    match REPERTOIRE.binary_search_by_key(&u, |c| c.code) {
        Ok(k) => &REPERTOIRE[k],
        Err(_) => not_modelled(api, format_args!("character U+{u:04X} (outside the code page 1252 repertoire)")),
    }
}

/// A code page argument resolved to what is modelled.
#[derive(Clone, Copy, PartialEq)]
enum CodePage {
    Ansi1252,
    Utf8,
}

fn code_page(api: &str, cp: u32) -> CodePage {
    match cp {
        0 | 3 | 1252 => CodePage::Ansi1252, // CP_ACP, CP_THREAD_ACP: GetACP is 1252
        CP_UTF8 => CodePage::Utf8,
        _ => not_modelled(api, format_args!("code page {cp}")),
    }
}

/// Bytes at `p`: `n` of them, or up to and including the NUL when `n` < 0.
fn guest_bytes(m: &dyn Mem, p: u32, n: i32) -> Vec<u8> {
    if n >= 0 {
        return (0..n as u32).map(|k| m.u8(p.wrapping_add(k))).collect();
    }
    let mut v = Vec::new();
    loop {
        let b = m.u8(p.wrapping_add(v.len() as u32));
        v.push(b);
        if b == 0 {
            return v;
        }
    }
}

/// UTF-16 units at `p`: `n` of them, or up to and including the NUL when
/// `n` < 0.
fn guest_units(m: &dyn Mem, p: u32, n: i32) -> Vec<u16> {
    if n >= 0 {
        return (0..n as u32).map(|k| m.u16(p.wrapping_add(2 * k))).collect();
    }
    let mut v = Vec::new();
    loop {
        let u = m.u16(p.wrapping_add(2 * v.len() as u32));
        v.push(u);
        if u == 0 {
            return v;
        }
    }
}

// --- code page 1252 -------------------------------------------------------

pub(crate) fn decode_1252(b: u8) -> u16 {
    match b {
        0x80..=0x9F => CP1252_80_9F[(b - 0x80) as usize],
        _ => b as u16,
    }
}

/// The byte a UTF-16 unit converts to in code page 1252: its own byte,
/// else (unless `best_fit` is off) Windows' best-fit byte, else None (the
/// default character).
pub(crate) fn encode_1252(u: u16, best_fit: bool) -> Option<u8> {
    match u {
        0x00..=0x7F | 0xA0..=0xFF => return Some(u as u8),
        _ => {}
    }
    if let Some(k) = CP1252_80_9F.iter().position(|&c| c == u) {
        return Some(0x80 + k as u8);
    }
    if !best_fit {
        return None;
    }
    BEST_FIT_1252.binary_search_by_key(&u, |&(c, _)| c).ok().map(|k| BEST_FIT_1252[k].1)
}

// --- UTF-8 ------------------------------------------------------------------

/// UTF-8 to UTF-16 as Windows 11 decodes it, and whether anything was
/// invalid. Each invalid part becomes U+FFFD: a byte that cannot start a
/// sequence; a lead byte not followed by a continuation byte; a lead byte
/// with a continuation byte outside its allowed second-byte range (one
/// U+FFFD for both: overlong forms, surrogates, code points above
/// U+10FFFF); a sequence cut short by a non-continuation byte or the end
/// (one U+FFFD for the bytes so far). Derived from and checked against the
/// host's MultiByteToWideChar on 6,000 random inputs
/// (research/runtime/windows/Vectors.cs).
pub(crate) fn decode_utf8(b: &[u8]) -> (Vec<u16>, bool) {
    let (mut out, mut bad, mut i) = (Vec::new(), false, 0);
    let cont = |k: usize| b.get(k).is_some_and(|&c| (0x80..=0xBF).contains(&c));
    while i < b.len() {
        let c = b[i];
        if c < 0x80 {
            out.push(c as u16);
            i += 1;
            continue;
        }
        // (length, allowed range of the second byte)
        let (n, lo, hi) = match c {
            0xC2..=0xDF => (2, 0x80, 0xBF),
            0xE0 => (3, 0xA0, 0xBF),
            0xE1..=0xEC | 0xEE | 0xEF => (3, 0x80, 0xBF),
            0xED => (3, 0x80, 0x9F),
            0xF0 => (4, 0x90, 0xBF),
            0xF1..=0xF3 => (4, 0x80, 0xBF),
            0xF4 => (4, 0x80, 0x8F),
            _ => {
                out.push(0xFFFD);
                bad = true;
                i += 1;
                continue;
            }
        };
        if !cont(i + 1) {
            out.push(0xFFFD);
            bad = true;
            i += 1;
            continue;
        }
        if !(lo..=hi).contains(&b[i + 1]) {
            out.push(0xFFFD);
            bad = true;
            i += 2;
            continue;
        }
        let mut j = i + 2;
        while j < i + n && cont(j) {
            j += 1;
        }
        if j < i + n {
            out.push(0xFFFD);
            bad = true;
            i = j;
            continue;
        }
        let mut cp = (c as u32) & (0x7F >> n);
        for &x in &b[i + 1..j] {
            cp = cp << 6 | (x as u32 & 0x3F);
        }
        if cp >= 0x1_0000 {
            let v = cp - 0x1_0000;
            out.extend([0xD800 | (v >> 10) as u16, 0xDC00 | (v & 0x3FF) as u16]);
        } else {
            out.push(cp as u16);
        }
        i = j;
    }
    (out, bad)
}

/// UTF-16 to UTF-8 as Windows encodes it: each character's bytes (one
/// group per character, for the whole-character truncation of a short
/// buffer) and whether a lone surrogate (written as U+FFFD) was met.
pub(crate) fn encode_utf8(units: &[u16]) -> (Vec<Vec<u8>>, bool) {
    let mut out = Vec::new();
    let mut bad = false;
    for r in char::decode_utf16(units.iter().copied()) {
        let c = r.unwrap_or_else(|_| {
            bad = true;
            '\u{FFFD}'
        });
        let mut buf = [0u8; 4];
        out.push(c.encode_utf8(&mut buf).as_bytes().to_vec());
    }
    (out, bad)
}

// --- MultiByteToWideChar / WideCharToMultiByte -----------------------------

/// KERNEL32 `MultiByteToWideChar(CodePage, dwFlags, lpMultiByteStr,
/// cbMultiByte, lpWideCharStr, cchWideChar)` (stdcall, Microsoft Learn
/// "MultiByteToWideChar function (stringapiset.h)"). Returns the UTF-16
/// units written (or needed when cchWideChar is 0), counting the NUL when
/// cbMultiByte is -1. Failures return 0 with: ERROR_INVALID_PARAMETER (no
/// input, cbMultiByte 0, cchWideChar < 0, input and output the same
/// buffer), ERROR_INVALID_FLAGS, ERROR_INSUFFICIENT_BUFFER (Windows has
/// then written the first cchWideChar units), ERROR_NO_UNICODE_TRANSLATION
/// (MB_ERR_INVALID_CHARS and invalid UTF-8; the converted text is still
/// written, as measured; a short buffer reports ERROR_INSUFFICIENT_BUFFER
/// first). Code page 1252 defines all 256 bytes; MB_COMPOSITE and
/// MB_USEGLYPHCHARS are not modelled for it (stop). UTF-8 ignores
/// MB_PRECOMPOSED, MB_COMPOSITE and MB_USEGLYPHCHARS (as measured; the
/// documentation asks for 0 or MB_ERR_INVALID_CHARS).
pub fn multi_byte_to_wide_char(m: &mut dyn Mem, cpu: &mut Cpu) {
    const API: &str = "MultiByteToWideChar";
    let (cp, flags, src, cb, dst, cch) =
        (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3) as i32, arg(m, cpu, 4), arg(m, cpu, 5) as i32);
    let r = (|| {
        if src == 0 || cb == 0 || cch < 0 || (cch > 0 && src == dst) {
            return Err(ERROR_INVALID_PARAMETER);
        }
        let page = code_page(API, cp);
        if flags & !(MB_PRECOMPOSED | MB_COMPOSITE | MB_USEGLYPHCHARS | MB_ERR_INVALID_CHARS) != 0 {
            return Err(ERROR_INVALID_FLAGS);
        }
        // UTF-8 ignores the composition and glyph flags (measured)
        if page == CodePage::Ansi1252 {
            if flags & (MB_PRECOMPOSED | MB_COMPOSITE) == MB_PRECOMPOSED | MB_COMPOSITE {
                return Err(ERROR_INVALID_FLAGS);
            }
            if flags & (MB_COMPOSITE | MB_USEGLYPHCHARS) != 0 {
                not_modelled(API, format_args!("dwFlags {flags:#x} (MB_COMPOSITE/MB_USEGLYPHCHARS)"));
            }
        }
        let bytes = guest_bytes(m, src, cb);
        let (units, bad) = match page {
            CodePage::Ansi1252 => (bytes.iter().map(|&b| decode_1252(b)).collect(), false),
            CodePage::Utf8 => decode_utf8(&bytes),
        };
        let invalid = bad && flags & MB_ERR_INVALID_CHARS != 0;
        if cch == 0 {
            return if invalid { Err(ERROR_NO_UNICODE_TRANSLATION) } else { Ok(units.len() as u32) };
        }
        for (k, &u) in units.iter().take(cch as usize).enumerate() {
            m.set_u16(dst.wrapping_add(2 * k as u32), u);
        }
        if units.len() > cch as usize {
            Err(ERROR_INSUFFICIENT_BUFFER)
        } else if invalid {
            Err(ERROR_NO_UNICODE_TRANSLATION)
        } else {
            Ok(units.len() as u32)
        }
    })();
    cpu.eax = r.unwrap_or_else(|e| {
        set_error(e);
        0
    });
    ret(m, cpu, 24);
}

/// KERNEL32 `WideCharToMultiByte(CodePage, dwFlags, lpWideCharStr,
/// cchWideChar, lpMultiByteStr, cbMultiByte, lpDefaultChar,
/// lpUsedDefaultChar)` (stdcall, Microsoft Learn "WideCharToMultiByte
/// function (stringapiset.h)"). Returns the bytes written (or needed when
/// cbMultiByte is 0), counting the NUL when cchWideChar is -1.
///
/// Code page 1252: each UTF-16 unit becomes its byte, else Windows'
/// best-fit byte (unless WC_NO_BEST_FIT_CHARS), else the default character
/// (lpDefaultChar's first byte, or '?') with *lpUsedDefaultChar = TRUE (a
/// surrogate pair is two units, two default characters). A short buffer
/// gets the first cbMultiByte bytes and ERROR_INSUFFICIENT_BUFFER.
///
/// UTF-8: lone surrogates become U+FFFD; WC_ERR_INVALID_CHARS then fails
/// with ERROR_NO_UNICODE_TRANSLATION after writing (as measured); a short
/// buffer gets whole characters only (and ERROR_INSUFFICIENT_BUFFER takes
/// precedence). Windows 11 accepts the 1252 flags,
/// lpDefaultChar and lpUsedDefaultChar for UTF-8 (writing FALSE), unlike
/// the documentation's ERROR_INVALID_FLAGS/PARAMETER: the measured
/// behaviour is followed.
///
/// Other failures: ERROR_INVALID_PARAMETER (no input, cchWideChar 0,
/// cbMultiByte < 0), ERROR_INVALID_FLAGS (unknown bits, WC_ERR_INVALID_CHARS
/// for 1252, WC_DISCARDNS/SEPCHARS/DEFAULTCHAR without WC_COMPOSITECHECK).
/// WC_COMPOSITECHECK itself is not modelled (stop).
pub fn wide_char_to_multi_byte(m: &mut dyn Mem, cpu: &mut Cpu) {
    const API: &str = "WideCharToMultiByte";
    let (cp, flags, src, cch, dst, cb, def, used_p) = (
        arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3) as i32,
        arg(m, cpu, 4), arg(m, cpu, 5) as i32, arg(m, cpu, 6), arg(m, cpu, 7),
    );
    let r = (|| {
        if src == 0 || cch == 0 || cb < 0 {
            return Err(ERROR_INVALID_PARAMETER);
        }
        let page = code_page(API, cp);
        let units = guest_units(m, src, cch);
        let (out, used, invalid) = match page {
            CodePage::Ansi1252 => {
                let known = WC_COMPOSITECHECK | WC_DISCARDNS | WC_SEPCHARS | WC_DEFAULTCHAR | WC_NO_BEST_FIT_CHARS;
                if flags & !known != 0 || (flags & (WC_DISCARDNS | WC_SEPCHARS | WC_DEFAULTCHAR) != 0 && flags & WC_COMPOSITECHECK == 0) {
                    return Err(ERROR_INVALID_FLAGS);
                }
                if flags & WC_COMPOSITECHECK != 0 {
                    not_modelled(API, format_args!("dwFlags {flags:#x} (WC_COMPOSITECHECK)"));
                }
                let default = if def != 0 { m.u8(def) } else { b'?' };
                let mut used = false;
                let bytes: Vec<Vec<u8>> = units.iter().map(|&u| {
                    vec![encode_1252(u, flags & WC_NO_BEST_FIT_CHARS == 0).unwrap_or_else(|| {
                        used = true;
                        default
                    })]
                }).collect();
                (bytes, used, false)
            }
            CodePage::Utf8 => {
                // Windows 11 accepts the 1252 flags here and ignores them
                let known = WC_COMPOSITECHECK | WC_DISCARDNS | WC_SEPCHARS | WC_DEFAULTCHAR | WC_NO_BEST_FIT_CHARS | WC_ERR_INVALID_CHARS;
                if flags & !known != 0 {
                    return Err(ERROR_INVALID_FLAGS);
                }
                let (groups, bad) = encode_utf8(&units);
                (groups, false, bad && flags & WC_ERR_INVALID_CHARS != 0)
            }
        };
        if used_p != 0 {
            m.set_u32(used_p, used as u32);
        }
        let total: usize = out.iter().map(Vec::len).sum();
        if cb == 0 {
            return if invalid { Err(ERROR_NO_UNICODE_TRANSLATION) } else { Ok(total as u32) };
        }
        // whole characters while they fit (1252: one byte each)
        let mut at = 0usize;
        for g in &out {
            if at + g.len() > cb as usize {
                break;
            }
            for &b in g {
                m.set_u8(dst.wrapping_add(at as u32), b);
                at += 1;
            }
        }
        if total > cb as usize {
            Err(ERROR_INSUFFICIENT_BUFFER)
        } else if invalid {
            Err(ERROR_NO_UNICODE_TRANSLATION)
        } else {
            Ok(total as u32)
        }
    })();
    cpu.eax = r.unwrap_or_else(|e| {
        set_error(e);
        0
    });
    ret(m, cpu, 32);
}

// --- locale-dependent string services ----------------------------------------

/// The locales answered as en-US: 0 (LANG_NEUTRAL, the user's default),
/// LOCALE_USER_DEFAULT, LOCALE_SYSTEM_DEFAULT and en-US itself.
fn check_locale(api: &str, lcid: u32) {
    if !matches!(lcid, 0 | 0x400 | 0x800 | 0x409) {
        not_modelled(api, format_args!("locale {lcid:#x}"));
    }
}

/// KERNEL32 `GetStringTypeW(dwInfoType, lpSrcStr, cchSrc, lpCharType)`
/// (stdcall, Microsoft Learn "GetStringTypeW function (stringapiset.h)"):
/// one WORD of CT_CTYPE1 (1), CT_CTYPE2 (2) or CT_CTYPE3 (4) flags per
/// unit; a negative cchSrc means NUL-terminated, and the NUL gets a type
/// too (as measured). TRUE; FALSE with ERROR_INVALID_FLAGS (another info
/// type) or ERROR_INVALID_PARAMETER (no string, cchSrc 0).
pub fn get_string_type_w(m: &mut dyn Mem, cpu: &mut Cpu) {
    const API: &str = "GetStringTypeW";
    let (info, src, cch, out) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2) as i32, arg(m, cpu, 3));
    let r = (|| {
        let k = match info {
            1 => 0,
            2 => 1,
            4 => 2,
            _ => return Err(ERROR_INVALID_FLAGS),
        };
        if src == 0 || out == 0 || cch == 0 {
            return Err(ERROR_INVALID_PARAMETER);
        }
        for (i, u) in guest_units(m, src, cch).into_iter().enumerate() {
            m.set_u16(out.wrapping_add(2 * i as u32), char_data(API, u).ctype[k]);
        }
        Ok(())
    })();
    cpu.eax = match r {
        Ok(()) => 1,
        Err(e) => {
            set_error(e);
            0
        }
    };
    ret(m, cpu, 16);
}

/// A Windows sort key (LCMAP_SORTKEY) for `units`: primary weights (two
/// bytes per element), 01, diacritic weights, 01, case weights, 01, 01
/// (empty extra and special sections), 00; diacritic and case weights are
/// one byte per element with trailing 02s (the defaults) left out.
/// NORM_IGNORENONSPACE leaves out the diacritic weights, NORM_IGNORECASE
/// uses the case weights Windows keeps under it. Word sort (no
/// SORT_STRINGSORT) is the same unless a character is special in word sort
/// (apostrophe, hyphen, dashes, controls), which is not modelled (stop).
fn sort_key(api: &str, units: &[u16], flags: u32) -> Vec<u8> {
    let (mut primary, mut diacritic, mut case) = (Vec::new(), Vec::new(), Vec::new());
    for &u in units {
        let c = char_data(api, u);
        if c.word_special && flags & SORT_STRINGSORT == 0 {
            not_modelled(api, format_args!("word sort of U+{u:04X} (special weight)"));
        }
        for &(p, d, cs, ci) in c.sort {
            primary.extend(p.to_be_bytes());
            diacritic.push(d);
            case.push(if flags & NORM_IGNORECASE != 0 { ci } else { cs });
        }
    }
    if flags & NORM_IGNORENONSPACE != 0 {
        diacritic.clear();
    }
    for v in [&mut diacritic, &mut case] {
        while v.last() == Some(&2) {
            v.pop();
        }
    }
    let mut key = primary;
    key.push(1);
    key.extend(diacritic);
    key.push(1);
    key.extend(case);
    key.extend([1, 1, 0]);
    key
}

/// KERNEL32 `LCMapStringW(Locale, dwMapFlags, lpSrcStr, cchSrc, lpDestStr,
/// cchDest)` (stdcall, Microsoft Learn "LCMapStringW function
/// (winnls.h)"). LCMAP_LOWERCASE / LCMAP_UPPERCASE: one unit per unit (a
/// negative cchSrc includes the NUL); LCMAP_SORTKEY (with NORM_IGNORECASE,
/// NORM_IGNORENONSPACE, SORT_STRINGSORT): the key's bytes, cchDest counting
/// bytes. Returns what was written (or the size needed when cchDest is 0);
/// a short buffer gets nothing and ERROR_INSUFFICIENT_BUFFER (as measured).
/// ERROR_INVALID_PARAMETER: no source, cchSrc 0, cchDest < 0.
/// ERROR_INVALID_FLAGS: no mapping, or LCMAP_SORTKEY with a case mapping,
/// or a case mapping with sort flags. Title case (both case flags),
/// linguistic casing, kana/width/Chinese mappings and other sort flags are
/// not modelled (stop).
pub fn lc_map_string_w(m: &mut dyn Mem, cpu: &mut Cpu) {
    const API: &str = "LCMapStringW";
    let (lcid, flags, src, cch, dst, cd) =
        (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3) as i32, arg(m, cpu, 4), arg(m, cpu, 5) as i32);
    let r = (|| {
        if src == 0 || cch == 0 || cd < 0 {
            return Err(ERROR_INVALID_PARAMETER);
        }
        check_locale(API, lcid);
        let case_flags = flags & (LCMAP_LOWERCASE | LCMAP_UPPERCASE);
        let sort_flags = NORM_IGNORECASE | NORM_IGNORENONSPACE | SORT_STRINGSORT;
        if flags & LCMAP_SORTKEY != 0 {
            if case_flags != 0 {
                return Err(ERROR_INVALID_FLAGS);
            }
            if flags & !(LCMAP_SORTKEY | sort_flags) != 0 {
                not_modelled(API, format_args!("sort-key flags {flags:#x}"));
            }
            let mut units = guest_units(m, src, cch);
            if cch < 0 {
                units.pop(); // the NUL has no weights
            }
            let key = sort_key(API, &units, flags);
            if cd == 0 {
                return Ok(key.len() as u32);
            }
            if key.len() > cd as usize {
                return Err(ERROR_INSUFFICIENT_BUFFER);
            }
            for (k, &b) in key.iter().enumerate() {
                m.set_u8(dst.wrapping_add(k as u32), b);
            }
            return Ok(key.len() as u32);
        }
        if case_flags == 0 {
            return Err(ERROR_INVALID_FLAGS);
        }
        if flags & sort_flags != 0 {
            return Err(ERROR_INVALID_FLAGS);
        }
        if flags != LCMAP_LOWERCASE && flags != LCMAP_UPPERCASE {
            not_modelled(API, format_args!("mapping flags {flags:#x}"));
        }
        let out: Vec<u16> = guest_units(m, src, cch).into_iter().map(|u| {
            let c = char_data(API, u);
            if flags == LCMAP_LOWERCASE { c.lower } else { c.upper }
        }).collect();
        if cd == 0 {
            return Ok(out.len() as u32);
        }
        if out.len() > cd as usize {
            return Err(ERROR_INSUFFICIENT_BUFFER);
        }
        for (k, &u) in out.iter().enumerate() {
            m.set_u16(dst.wrapping_add(2 * k as u32), u);
        }
        Ok(out.len() as u32)
    })();
    cpu.eax = r.unwrap_or_else(|e| {
        set_error(e);
        0
    });
    ret(m, cpu, 24);
}

/// KERNEL32 `CompareStringW(Locale, dwCmpFlags, lpString1, cchCount1,
/// lpString2, cchCount2)` (stdcall, Microsoft Learn "CompareStringW
/// function (stringapiset.h)"): CSTR_LESS_THAN (1), CSTR_EQUAL (2) or
/// CSTR_GREATER_THAN (3), the byte order of the two strings' sort keys
/// (LCMapStringW documents that the keys compare as CompareString does;
/// checked on the host). A negative count means NUL-terminated. 0 with
/// ERROR_INVALID_PARAMETER (a NULL string) or ERROR_INVALID_FLAGS (unknown
/// bits). Modelled flags: NORM_IGNORECASE, NORM_IGNORENONSPACE,
/// SORT_STRINGSORT (the CRT passes NORM_IGNORECASE | SORT_STRINGSORT); the
/// other documented flags stop.
pub fn compare_string_w(m: &mut dyn Mem, cpu: &mut Cpu) {
    const API: &str = "CompareStringW";
    let (lcid, flags, s1, n1, s2, n2) =
        (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3) as i32, arg(m, cpu, 4), arg(m, cpu, 5) as i32);
    // winnls.h: NORM_IGNORECASE 1, NORM_IGNORENONSPACE 2, NORM_IGNORESYMBOLS 4,
    // SORT_DIGITSASNUMBERS 8, LINGUISTIC_IGNORECASE 0x10,
    // LINGUISTIC_IGNOREDIACRITIC 0x20, SORT_STRINGSORT 0x1000,
    // NORM_IGNOREKANATYPE 0x10000, NORM_IGNOREWIDTH 0x20000,
    // NORM_LINGUISTIC_CASING 0x8000000
    const DOCUMENTED: u32 = 0x1 | 0x2 | 0x4 | 0x8 | 0x10 | 0x20 | 0x1000 | 0x1_0000 | 0x2_0000 | 0x800_0000;
    let r = (|| {
        if s1 == 0 || s2 == 0 {
            return Err(ERROR_INVALID_PARAMETER);
        }
        if flags & !DOCUMENTED != 0 {
            return Err(ERROR_INVALID_FLAGS);
        }
        check_locale(API, lcid);
        if flags & !(NORM_IGNORECASE | NORM_IGNORENONSPACE | SORT_STRINGSORT) != 0 {
            not_modelled(API, format_args!("compare flags {flags:#x}"));
        }
        let strip = |mut v: Vec<u16>, n: i32| {
            if n < 0 {
                v.pop();
            }
            v
        };
        let a = strip(guest_units(m, s1, n1), n1);
        let b = strip(guest_units(m, s2, n2), n2);
        Ok(match sort_key(API, &a, flags).cmp(&sort_key(API, &b, flags)) {
            std::cmp::Ordering::Less => 1,
            std::cmp::Ordering::Equal => 2,
            std::cmp::Ordering::Greater => 3,
        })
    })();
    cpu.eax = r.unwrap_or_else(|e| {
        set_error(e);
        0
    });
    ret(m, cpu, 24);
}

/// Test access to the pure conversions (differential checks against the
/// host's recorded answers).
pub mod check {
    pub fn decode_utf8(b: &[u8]) -> (Vec<u16>, bool) {
        super::decode_utf8(b)
    }
    pub fn encode_1252(u: u16, best_fit: bool) -> Option<u8> {
        super::encode_1252(u, best_fit)
    }
    /// The sort key (None where the model would stop).
    pub fn sort_key(units: &[u16], flags: u32) -> Option<Vec<u8>> {
        std::panic::catch_unwind(|| super::sort_key("check", units, flags)).ok()
    }
}

/// USER32 `wsprintfA(lpOut, lpFmt, ...)` (cdecl, varargs; Microsoft Learn
/// "wsprintfA function (winuser.h)"), for the conversions the game uses:
/// literal characters, `%%`, and `%S` (a UTF-16 string written in code
/// page 1252, each unit as WideCharToMultiByte(1252, 0) converts it: its
/// byte, a best-fit byte or '?'; NULL writes nothing). Measured on Windows
/// 11 (research/runtime/windows/Wsprintf.cs): the output holds at most 1024
/// characters; a literal is written while fewer than 1024 are, a `%S`
/// argument only if all of it ends below 1024, and output stops at the
/// first piece that does not fit; a NUL follows. Returns the characters
/// written. Callers: 0x00DA3300 and 0x00DA3650 (`"%S"`). Other conversions
/// are not modelled (stop).
pub fn wsprintf_a(m: &mut dyn Mem, cpu: &mut Cpu) {
    const LIMIT: usize = 1024;
    let (out, fmt) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let mut next_arg = 2;
    let fmt = guest_bytes(m, fmt, -1);
    let mut text: Vec<u8> = Vec::new();
    let mut k = 0;
    'fmt: while k < fmt.len() && fmt[k] != 0 {
        let piece: Vec<u8> = if fmt[k] == b'%' {
            match fmt.get(k + 1) {
                Some(b'%') => vec![b'%'],
                Some(b'S') => {
                    let p = arg(m, cpu, next_arg);
                    next_arg += 1;
                    if p == 0 { Vec::new() } else {
                        guest_units(m, p, -1).iter().take_while(|&&u| u != 0).map(|&u| encode_1252(u, true).unwrap_or(b'?')).collect()
                    }
                }
                _ => not_modelled("wsprintfA", format_args!("format {:?}", String::from_utf8_lossy(&fmt[k..]))),
            }
        } else {
            vec![fmt[k]]
        };
        let conversion = fmt[k] == b'%' && fmt.get(k + 1) == Some(&b'S');
        let fits = if conversion { text.len() + piece.len() < LIMIT } else { text.len() < LIMIT };
        if !fits {
            break 'fmt;
        }
        text.extend(piece);
        k += if fmt[k] == b'%' { 2 } else { 1 };
    }
    for (i, &b) in text.iter().chain([0u8].iter()).enumerate() {
        m.set_u8(out.wrapping_add(i as u32), b);
    }
    cpu.eax = text.len() as u32;
    ret(m, cpu, 0);
}

pub const APIS: &[(&str, &str, fn(&mut dyn Mem, &mut Cpu))] = &[
    ("USER32.dll", "wsprintfA", wsprintf_a),
    ("KERNEL32.dll", "MultiByteToWideChar", multi_byte_to_wide_char),
    ("KERNEL32.dll", "WideCharToMultiByte", wide_char_to_multi_byte),
    ("KERNEL32.dll", "GetStringTypeW", get_string_type_w),
    ("KERNEL32.dll", "LCMapStringW", lc_map_string_w),
    ("KERNEL32.dll", "CompareStringW", compare_string_w),
];
