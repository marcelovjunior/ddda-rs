import re, glob, collections
ROOT = 'E:/ddda-rs-wt/L191/crates/ddlift'
BLOCK = re.compile(r'^\s*(0x[0-9A-Fa-f]+)\s*=>\s*\{(.*)$')
CALL = re.compile(r'x86::call_(ext|local|lifted)\(m, cpu, (0x[0-9A-Fa-f]+|t)|x86::call_ext\(m, cpu, t\)|x86::call_local|x86::call_lifted')
WRITE = re.compile(r'cpu\.fl\.(alu_|inc_dec)|cpu\.fl = |comiss_hw|ucomiss|fl = ')
READ = re.compile(r'cpu\.fl\.(zf|cf|sf|of|pf|af|lahf|a\b|b\b)|cpu\.fl\.a\b')
NEXT = re.compile(r'pc = (0x[0-9A-Fa-f]+);')
GLOBAL = {}
calls = []  # (block-text, end-index-of-call, callee)
for f in glob.glob(ROOT + '/*/src/generated/*.rs'):
    for ln in open(f, encoding='utf-8', errors='replace'):
        m = BLOCK.match(ln)
        if m:
            lab = int(m.group(1), 16)
            text = m.group(2)
            GLOBAL.setdefault(lab, text)
            for cm in CALL.finditer(text):
                t = cm.group(2) or 'indirect'
                calls.append((text, cm.end(), t))

def flag_after(text, pos, depth=0):
    rest = text[pos:]
    w = WRITE.search(rest); r = READ.search(rest)
    if w and (not r or w.start() < r.start()):
        return 'fresh-write'
    if r:
        return 'READ'
    n = NEXT.search(rest)
    if n and depth < 4:
        nt = GLOBAL.get(int(n.group(1), 16))
        if nt is None:
            return 'unknown-block'
        return flag_after(nt, 0, depth + 1)
    return 'none'

counts = collections.Counter()
reads = collections.Counter()
for text, pos, callee in calls:
    use = flag_after(text, pos)
    counts[use] += 1
    if use == 'READ':
        reads[callee] += 1
print('call sites', len(calls)); print(dict(counts))
print('READ callees (top 30):', reads.most_common(30))

import random
random.seed(1)
nones = [(t, p, c) for (t, p, c) in calls if flag_after(t, p) == 'none']
ends = collections.Counter()
for t, p, c in nones:
    rest = t[p:]
    ends[rest.strip()[:40] if len(rest.strip()) < 40 else ('return' if 'return' in rest else rest.strip()[:30])] += 1
print('none tails (top 10):', ends.most_common(10))
for t, p, c in random.sample(nones, 3):
    print('SAMPLE', c, t[max(0,p-80):p+160].replace('\n',' '))
for t, p, c in calls:
    if c == '0x013006A5' or (flag_after(t,p) == 'READ'):
        print('READ-SITE', c, t[max(0,p-120):p+200])
