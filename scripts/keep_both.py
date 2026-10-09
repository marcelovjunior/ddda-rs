#!/usr/bin/env python3
"""Resolves git conflict blocks by keeping both sides (ours, then theirs).

For additive clashes only, e.g. two branches each adding a `mod` line or a
subcommand to crates/ddtool/src/main.rs. Usage: keep_both.py <file>...
"""
import re
import sys

BLOCK = re.compile(r'<<<<<<< [^\n]*\n(.*?)=======\n(.*?)>>>>>>> [^\n]*\n', re.S)

for path in sys.argv[1:]:
    text = open(path, encoding='utf-8', newline='').read()
    text, n = BLOCK.subn(lambda m: m.group(1) + m.group(2), text)
    open(path, 'w', encoding='utf-8', newline='').write(text)
    print(f'{path}: {n} block(s) resolved')
