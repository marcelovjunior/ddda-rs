import sys
sys.path.insert(0, 'E:/ddda-re/notes/native-L191')
import callee_inputs as ci
import re
WRITE = re.compile(r'cpu\.fl\.(alu_|inc_dec)|cpu\.fl = |comiss_hw')
READ = re.compile(r'cpu\.fl\.(zf|cf|sf|of|pf|af|lahf|a\b|b\b)')
def first_flag(addr, depth=0, seen=None):
    seen = seen if seen is not None else set()
    if addr in seen or depth > 12:
        return 'none'
    seen.add(addr)
    t = ci.GLOBAL.get(addr)
    if t is None:
        return 'unknown'
    w, r = WRITE.search(t), READ.search(t)
    if w and (not r or w.start() < r.start()):
        return 'write-first'
    if r:
        return 'READ-first'
    n = ci.NEXT.search(t) if hasattr(ci, 'NEXT') else None
    import re as _re
    n = _re.search(r'pc = (0x[0-9A-Fa-f]+);', t)
    if n:
        return first_flag(int(n.group(1), 16), depth + 1, seen)
    return 'none'
for name, a in [('vf11', 0x886880), ('vf08', 0x885DC0), ('char_vf09', 0x84C720), ('vf09', 0x886530), ('probe', 0x1165B40), ('owner_visit', 0x116B670)]:
    print(name, first_flag(a))
