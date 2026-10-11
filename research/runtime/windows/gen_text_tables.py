"""Write crates/ddruntime/src/text/tables.rs from the host measurements.

Usage: python gen_text_tables.py <dir with bestfit.txt and chars.txt> <tables.rs>
(bestfit.txt from Nls.cs [Nls]::BestFit, chars.txt from Chars.cs [Chars]::Run.)
"""
import sys
H = sys.argv[1]
out = [
    '//! Tables measured from Windows 11 (build 26200, en-US) by',
    '//! research/runtime/windows/Nls.cs and Chars.cs, written by',
    '//! research/runtime/windows/gen_text_tables.py. Do not edit by hand.',
    'use super::Char;',
    '',
]
# cp1252 decode for 0x80..0x9F
dec = {}
bf = []
for line in open(H + '/bestfit.txt'):
    p = line.split()
    if p[0] == 'D':
        dec[int(p[1], 16)] = int(p[2], 16)
    elif p[0] == 'B' and p[4] == '0':
        bf.append((int(p[1], 16), int(p[3], 16)))
for b in range(256):
    if not (0x80 <= b <= 0x9F): assert dec[b] == b, b
out.append('/// Code page 1252 bytes 0x80..=0x9F as Windows decodes them (the rest\n/// are U+0000..U+00FF). Measured on Windows 11 26200 (MultiByteToWideChar).')
out.append('pub(crate) const CP1252_80_9F: [u16; 32] = [')
row = [f'0x{dec[b]:04X}' for b in range(0x80, 0xA0)]
for i in range(0, 32, 8): out.append('    ' + ', '.join(row[i:i+8]) + ',')
out.append('];')
out.append('')
out.append(f'/// WideCharToMultiByte(1252) best-fit mappings: the {len(bf)} UTF-16 units that are\n/// not in code page 1252 but map to a byte other than the default character\n/// (flags 0). Measured on Windows 11 26200 over U+0000..U+FFFF; sorted.')
out.append('pub(crate) const BEST_FIT_1252: [(u16, u8); %d] = [' % len(bf))
items = [f'(0x{u:04X}, 0x{b:02X})' for u, b in sorted(bf)]
for i in range(0, len(items), 8): out.append('    ' + ', '.join(items[i:i+8]) + ',')
out.append('];')
out.append('')

def parse(key, ic=False):
    b = bytes.fromhex(key)
    i = 0; prim = []
    while b[i] != 1:
        prim.append(b[i] << 8 | b[i+1]); i += 2
    i += 1; dia = []
    while b[i] != 1: dia.append(b[i]); i += 1
    i += 1; case = []
    while b[i] != 1: case.append(b[i]); i += 1
    i += 1
    assert b[i] == 1 and b[i+1] == 0 and i + 2 == len(b), key
    assert len(dia) <= len(prim) and len(case) <= len(prim), key
    dia += [2] * (len(prim) - len(dia)); case += [2] * (len(prim) - len(case))
    return prim, dia, case

rows = []
for line in open(H + '/chars.txt'):
    f = dict(x.split('=') for x in line.split()[1:])
    code = int(line.split()[0], 16)
    lo = int(f['lo'].split(':')[1], 16); up = int(f['up'].split(':')[1], 16)
    assert f['lo'].startswith('1:') and f['up'].startswith('1:')
    assert int(f['lo0'], 16) == lo and int(f['up0'], 16) == up
    prim, dia, case = parse(f['ks'])
    p2, d2, case_ic = parse(f['ki'])
    assert p2 == prim and d2 == dia
    word_special = f['k'] != f['ks']
    els = ', '.join(f'(0x{p:04X}, 0x{d:02X}, 0x{c:02X}, 0x{ci:02X})' for p, d, c, ci in zip(prim, dia, case, case_ic))
    rows.append((code, f'    Char {{ code: 0x{code:04X}, ctype: [0x{int(f["c1"],16):04X}, 0x{int(f["c2"],16):04X}, 0x{int(f["c3"],16):04X}], lower: 0x{lo:04X}, upper: 0x{up:04X}, word_special: {str(word_special).lower()}, sort: &[{els}] }},'))
rows.sort()
out.append('/// The 256 characters code page 1252 decodes to, as Windows 11 26200 answers\n/// for them in en-US (0x409; locale 0 gives the same): GetStringTypeW\n/// CT_CTYPE1/2/3, LCMapStringW LCMAP_LOWERCASE/UPPERCASE, and the\n/// LCMAP_SORTKEY|SORT_STRINGSORT elements (primary, diacritic, case, case\n/// under NORM_IGNORECASE). `word_special`: in word sort (no SORT_STRINGSORT)\n/// the character moves to the special-weight section. Sorted by code.')
out.append('pub(crate) const REPERTOIRE: [Char; 256] = [')
out += [r for _, r in rows]
out.append('];')
open(sys.argv[2], 'w', newline='\n').write('\n'.join(out) + '\n')
print(len(bf), len(rows))
