"""Gen-time lean emission (L73): smaller Rust per x86 instruction.

These passes rewrite one lowered arm (the text cpu_arm in lift.py produces,
registers already `cpu.<reg>`, flags `cpu.fl`) into equivalent, shorter
Rust. They run at gen time only; the cached arms in patterns/lift.json keep
their representation.

dead_flags: within one arm (a basic block, straight-line text), a flag
write that a later unconditional flag write replaces before anything can
read the flags is unobservable, so the ALU operation emits plain arithmetic
instead of `cpu.fl.alu_*`. Everything that may read the flags counts as a
reader: any other `cpu.fl` use (cond, lahf, adc/sbb, rotate-through-carry,
INC/DEC's carried CF, ...), any bare `cpu` (calls, call_ext, string and
platform helpers, pushfd, closures), and every arm exit (return, continue,
break, panic). Writes inside a conditional block never kill an earlier
write; an arm with a loop is analysed with the loop as one reader.
A guest fault inside a whole-function body is a host failure (executor.rs
runs restartable instructions, which these passes do not touch), so the
flag state between two writes is never observed through a panic either.

idioms: push/pop/return-address pushes become calls to the tiny helpers in
ddnative_base::emit, and the closure wrappers around call targets become
direct calls.
"""
import re

STRING = r'"(?:\\.|[^"\\])*"'
GPR = ("eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi")
MASK = {"8": "0xFF", "16": "0xFFFF", "32": None}

EVENT = re.compile(
    STRING
    + r"|\bcpu\.fl\.alu_(?:add|sub|and|or|xor)\("
    + r"|\bcpu\.fl = Flags::new\(x86::\w+, \w+, \w+, \w+, \d+\);"
    + r"|\bcpu\.fl = Flags::partial\(\w+, \w+\);"
    + r"|\bcpu\.fl\.inc_dec\(x86::\w+, \w+, \w+, \d+\);"
    + r"|\bcpu\.fl = "
    + r"|\bcpu\.fl\b"
    + r"|\bcpu\b(?!\.)"
    + r"|\b(?:return|continue|break)\b|\bpanic!|\bunreachable!"
    + r"|[{}]")


def _close(text, start):
    """Index of the parenthesis closing the one opened just before start."""
    depth = 1
    k = start
    while k < len(text):
        c = text[k]
        if c == '"':
            m = re.compile(STRING).match(text, k)
            k = m.end()
            continue
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
            if depth == 0:
                return k
        k += 1
    raise ValueError("unbalanced call")


def _split_args(text):
    """Top-level comma split of a call's argument text."""
    out, depth, cur, k = [], 0, 0, 0
    while k < len(text):
        c = text[k]
        if c == '"':
            k = re.compile(STRING).match(text, k).end()
            continue
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        elif c == "," and depth == 0:
            out.append(text[cur:k].strip())
            cur = k + 1
        k += 1
    out.append(text[cur:].strip())
    return out


def _postfix_safe(e):
    """No top-level space: a path, literal, call chain or parenthesized
    expression, safe as a method receiver or an operand of & | ^."""
    depth, k = 0, 0
    while k < len(e):
        c = e[k]
        if c == '"':
            k = re.compile(STRING).match(e, k).end()
            continue
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        elif c == " " and depth == 0:
            return False
        k += 1
    return True


def _plain(op, a, b, bits):
    """The value alu_<op>(a, b, bits) returns (ddnative-base x86.rs), without
    the flag write. The operands are evaluated in the same order."""
    a = a if _postfix_safe(a) else f"({a})"
    b = b if _postfix_safe(b) else f"({b})"
    mask = MASK[bits]
    if op in ("add", "sub"):
        e = f"{a}.wrapping_{op}({b})"
        return f"({e} & {mask})" if mask else e
    sym = {"and": "&", "or": "|", "xor": "^"}[op]
    return f"({a} {sym} {b} & {mask})" if mask and op == "and" else (
        f"(({a} {sym} {b}) & {mask})" if mask else f"({a} {sym} {b})")


def _scan(arm):
    """Flag events in text order: (kind, start, end, removable, info).
    kind: 'kill' (unconditional full write), 'write' (a write that does not
    kill: conditional, or INC/DEC which carries CF), 'read'."""
    events = []
    stack = []  # per open brace: 'plain', 'cond' or 'loop'
    last = 0  # end of the previous statement boundary, for brace headers
    for m in EVENT.finditer(arm):
        t = m.group(0)
        if t.startswith('"'):
            continue
        conditional = any(s != "plain" for s in stack)
        in_loop = "loop" in stack
        if t == "{":
            before = arm[:m.start()].rstrip()
            if not before or before.endswith((";", "{", "}", "=>")):
                stack.append("plain")
            else:
                header = before[max(before.rfind(";"), before.rfind("{"), before.rfind("}")) + 1:].strip()
                if re.match(r"(?:for|while|loop)\b", header) or re.search(r"\b(?:loop|while|for)\b", header):
                    stack.append("loop")
                    events.append(("read", m.start(), m.end(), False, None))
                else:
                    stack.append("cond")
            continue
        if t == "}":
            if stack:
                stack.pop()
            continue
        if in_loop:
            events.append(("read", m.start(), m.end(), False, None))
            continue
        if t.startswith("cpu.fl.alu_"):
            end = _close(arm, m.end())
            op = t[len("cpu.fl.alu_"):-1]
            args = _split_args(arm[m.end():end])
            if len(args) != 3 or args[2] not in MASK or re.search(r"\bcpu\.fl\b|\bcpu\b(?!\.)", args[0] + args[1]):
                events.append(("read", m.start(), m.end(), False, None))
                continue
            events.append(("write" if conditional else "kill", m.start(), end + 1, True, (op, *args)))
        elif t.startswith("cpu.fl = Flags::"):
            events.append(("write" if conditional else "kill", m.start(), m.end(), True, None))
        elif t.startswith("cpu.fl.inc_dec"):
            events.append(("read", m.start(), m.start(), False, None))
            events.append(("write", m.start(), m.end(), True, None))
        elif t == "cpu.fl = ":
            end = arm.find(";", m.end())
            rhs = arm[m.end():end]
            if end < 0 or re.search(r"\bcpu\.fl\b|\bcpu\b(?!\.)", rhs):
                events.append(("read", m.start(), m.end(), False, None))
            else:
                events.append(("write" if conditional else "kill", m.start(), end + 1, False, None))
        else:
            events.append(("read", m.start(), m.end(), False, None))
    return events


def dead_flags(arm):
    """Drop flag writes that the same arm overwrites before any reader."""
    if "cpu.fl" not in arm:
        return arm
    events = _scan(arm)
    pending, dead = [], []
    for e in events:
        kind = e[0]
        if kind == "read":
            pending = []
        elif kind == "kill":
            dead += [p for p in pending if p[3]]
            pending = [e]
        else:
            pending.append(e)
    if not dead:
        return arm
    out, k = [], 0
    for kind, start, end, _, info in sorted(dead, key=lambda e: e[1]):
        out.append(arm[k:start])
        if info is not None:
            e = _plain(*info)
            if arm[end:end + 1] == ";" and e.startswith("(") and _close(e, 1) == len(e) - 1:
                e = e[1:-1]  # `let r = a & b;` needs no parentheses
            out.append(e)
        k = end
        if info is None and arm[k:k + 1] == " ":
            k += 1  # a removed statement takes its separating space along
    out.append(arm[k:])
    return fold_results("".join(out))


# A plain block: after a statement, a block or an arm head, never an `if`
# body. Zero-width, so adjacent matches all apply.
PLAIN = r"(?:(?<=[;{}])|(?<=[;{}] )|(?<=[;{}]  )|(?<==> )|^)"
REG = r"cpu\.(?:" + "|".join(GPR) + r")"
XMM = r"cpu\.xmm\[[0-7]\]"
FOLD = re.compile(PLAIN + r"\{ let r = ([^;{}\"]*); (" + REG + r") = r; \}")


def fold_results(arm):
    """`{ let r = E; cpu.R = r; }` -> `cpu.R = E;` (E has no side effect on
    cpu.R, and is evaluated before the store either way)."""
    return FOLD.sub(lambda m: f"{m[2]} = {m[1]};", arm)


CONST_SHIFT = re.compile(r"let n = (0x[0-9A-F]+)u32 & 31; if n != 0 \{ ([^{}]*) \} \}")


def const_shifts(arm):
    """SHL/SHR/SAR by an immediate: `let n = imm & 31; if n != 0 { .. }`
    with a nonzero masked count always runs its body, so the body (and its
    flag write, which then kills like any other) is emitted unconditionally.
    A zero count stays as it is (no effect, flags unchanged)."""
    def replace(m):
        n = int(m[1], 16) & 31
        return f"let n = 0x{n:X}u32; {m[2]} }}" if n else m[0]
    return CONST_SHIFT.sub(replace, arm)


DROP_R = re.compile(PLAIN + r"\{ let r = (cpu\.fl\.alu_(?:add|sub|and|or|xor)\([^;{}\"]*\)); \}")
INC_DEC = re.compile(PLAIN + r"\{ let a = (" + REG + r") & u32::MAX; let r = a\.(wrapping_add|wrapping_sub)\(1\) & u32::MAX; \1 = r; \}")
SHIFT = re.compile(PLAIN + r"\{ let a = (" + REG + r") & u32::MAX; let n = (0x[0-9A-F]+u32); let r = \(a (<<|>>) n\) & u32::MAX; \1 = r; \}")


def tidy(arm):
    """Blocks the dead-flag pass left trivial: a compare whose result is
    unused (CMP/TEST: only the flags), a 32-bit INC/DEC or SHL/SHR by an
    immediate without its flag write."""
    arm = DROP_R.sub(lambda m: f"{m[1]};", arm)
    arm = INC_DEC.sub(lambda m: f"{m[1]} = {m[1]}.{m[2]}(1);", arm)
    return SHIFT.sub(lambda m: f"{m[1]} = {m[1]} {m[3]} {m[2]};", arm)


PUSH = re.compile(PLAIN + r"\{ let v = ([^;{}\"]*); cpu\.esp = cpu\.esp\.wrapping_sub\(4\); m\.fast_set_u32\(pt, cpu\.esp, v\); \}")
PUSH_CONST = re.compile(r"cpu\.esp = cpu\.esp\.wrapping_sub\(4\); m\.fast_set_u32\(pt, cpu\.esp, (0x[0-9A-F]+)\);")
POP = re.compile(PLAIN + r"\{ let v = m\.fast_u32\(pt, cpu\.esp\); cpu\.esp = cpu\.esp\.wrapping_add\(4\); ")
POP_REG = re.compile(r"\{ let v = emit::pop\(m, pt, cpu\); (" + REG + r") = v; \}")
CLOSURE = re.compile(r"\(\|m: &mut dyn Mem, cpu: &mut Cpu\| (x86::call_(?:lifted|ext)\(m, cpu, 0x[0-9A-F]+\))\)\(m, cpu\)")
# a constant call_ext target needs no binding before the return address
CALL_CONST = re.compile(PLAIN + r"\{ let t = (0x[0-9A-F]+u32); (emit::push\(0x[0-9A-F]+, m, pt, cpu\);) +x86::call_ext\(m, cpu, t\); +\}")
ABSOLUTE = re.compile(r"\b0u32\.wrapping_add\((0x[0-9A-F]+)\)")  # 0 + disp: an absolute address


# Flags::cond (ddnative-base x86.rs) per condition code, with the same
# predicate calls in the same order: at opt-level 0 cond() compares the
# condition string on every evaluation.
CONDITIONS = {
    ("e", "z"): "f.zf()", ("ne", "nz"): "!f.zf()",
    ("b", "c", "nae"): "f.cf()", ("ae", "nb", "nc"): "!f.cf()",
    ("be", "na"): "f.cf() || f.zf()", ("a", "nbe"): "!f.cf() && !f.zf()",
    ("s",): "f.sf()", ("ns",): "!f.sf()",
    ("l", "nge"): "f.sf() != f.of()", ("ge", "nl"): "f.sf() == f.of()",
    ("le", "ng"): "f.zf() || f.sf() != f.of()", ("g", "nle"): "!f.zf() && f.sf() == f.of()",
    ("o",): "f.of()", ("no",): "!f.of()",
    ("p", "pe"): "f.pf()", ("np", "po"): "!f.pf()",
}
PREDICATE = {cc: e for codes, e in CONDITIONS.items() for cc in codes}
COND = re.compile(r'\bcpu\.fl\.cond\("(\w+)"\)')


def predicates(arm):
    """`cpu.fl.cond("be")` -> `(cpu.fl.cf() || cpu.fl.zf())`."""
    def replace(m):
        e = PREDICATE.get(m[1])
        if e is None:
            return m[0]  # unknown code: keep cond()'s panic
        e = e.replace("f.", "cpu.fl.")
        return e if re.fullmatch(r"cpu\.fl\.\w+\(\)", e) else f"({e})"
    return COND.sub(replace, arm)


def idioms(arm):
    """Push/pop through ddnative_base::emit, direct call targets."""
    arm = PUSH.sub(lambda m: f"emit::push({m[1]}, m, pt, cpu);", arm)
    arm = PUSH_CONST.sub(lambda m: f"emit::push({m[1]}, m, pt, cpu);", arm)
    arm = POP.sub("{ let v = emit::pop(m, pt, cpu); ", arm)
    arm = POP_REG.sub(lambda m: f"{m[1]} = emit::pop(m, pt, cpu);", arm)
    arm = CLOSURE.sub(lambda m: m[1], arm)
    arm = CALL_CONST.sub(lambda m: f"{m[2]} x86::call_ext(m, cpu, {m[1]});", arm)
    arm = ABSOLUTE.sub(lambda m: f"{m[1]}u32", arm)
    return predicates(sse(arm))


LOAD128 = re.compile(r"\[m\.fast_u32\(pt, a(?:\.wrapping_add\(0\))?\), m\.fast_u32\(pt, a\.wrapping_add\(4\)\), "
                     r"m\.fast_u32\(pt, a\.wrapping_add\(8\)\), m\.fast_u32\(pt, a\.wrapping_add\(12\)\)\]")
LOAD64 = re.compile(r"\[m\.fast_u32\(pt, a(?:\.wrapping_add\(0\))?\), m\.fast_u32\(pt, a\.wrapping_add\(4\)\), 0, 0\]")
STORE128 = re.compile(r"for k in 0\.\.4u32 \{ m\.fast_set_u32\(pt, a\.wrapping_add\(4 \* k\), (" + XMM + r")\[k as usize\]\); \}")
STORE64 = re.compile(r"m\.fast_set_u32\(pt, a, (" + XMM + r")\[0\]\); m\.fast_set_u32\(pt, a\.wrapping_add\(4\), \1\[1\]\);")
LANES_FLOAT = re.compile(r"for k in 0\.\.4 \{ (" + XMM + r")\[k\] = x86::(add|sub|mul)ss\(\1\[k\], (" + XMM + r")\[k\]\); \}")
LANES_LOGIC = re.compile(r"for k in 0\.\.4 \{ let \(d, s\) = \((" + XMM + r")\[k\], (" + XMM + r"|s)\[k\]\); \1\[k\] = "
                         r"(d \^ s|d & s|d \| s|!d & s); \}")
LOGIC = {"d ^ s": "xor", "d & s": "and", "d | s": "or", "!d & s": "andn"}
# a single-use address binding (`let a`) folded into the one helper call
ADDRESS = r"\{ let a = ([^;{}\"]*); "
A_LOAD = re.compile(PLAIN + ADDRESS + r"(" + XMM + r") = (emit::load(?:128|64))\(m, pt, a\); \}")
A_LOAD_EXPR = re.compile(ADDRESS + r"(emit::load(?:128|64))\(m, pt, a\) \}")
A_STORE = re.compile(PLAIN + ADDRESS + r"(emit::store(?:128|64))\((" + XMM + r"), m, pt, a\); \}")
A_LOGIC = re.compile(PLAIN + ADDRESS + r"let s = emit::load128\(m, pt, a\); (" + XMM + r") = (emit::\w+ps)\(\2, s\); \}")


def sse(arm):
    """Packed SSE templates: the 16/8-byte loads and stores as one call
    each (same accesses in the same order), the lane loops of ADDPS/SUBPS/
    MULPS xmm, xmm as the packed helper the memory form already uses (the
    original instruction, one host MULPS instead of four MULSS: per-lane
    IEEE results and the sticky MXCSR flags are the same), and the bitwise
    lane loops as emit helpers. The address is evaluated before the access
    either way, and neither reads nor writes what the other uses."""
    if "let a = " not in arm and "for k" not in arm:
        return arm
    arm = LOAD128.sub("emit::load128(m, pt, a)", arm)
    arm = LOAD64.sub("emit::load64(m, pt, a)", arm)
    arm = STORE128.sub(lambda m: f"emit::store128({m[1]}, m, pt, a);", arm)
    arm = STORE64.sub(lambda m: f"emit::store64({m[1]}, m, pt, a);", arm)
    arm = LANES_FLOAT.sub(lambda m: f"{m[1]} = x86::{m[2]}ps({m[1]}, {m[3]});", arm)
    arm = LANES_LOGIC.sub(lambda m: f"{m[1]} = emit::{LOGIC[m[3]]}ps({m[1]}, {m[2]});", arm)
    arm = A_LOAD.sub(lambda m: f"{m[2]} = {m[3]}(m, pt, {m[1]});", arm)
    arm = A_LOAD_EXPR.sub(lambda m: f"{m[2]}(m, pt, {m[1]})", arm)
    arm = A_STORE.sub(lambda m: f"{m[2]}({m[3]}, m, pt, {m[1]});", arm)
    arm = A_LOGIC.sub(lambda m: f"{m[2]} = {m[3]}({m[2]}, emit::load128(m, pt, {m[1]}));", arm)
    return arm


def lean(arm):
    """All lean passes, in order, for one lowered whole-function arm."""
    return x87_operands(idioms(tidy(fold_results(dead_flags(const_shifts(arm))))))


# L129: successor proofs are deliberately limited to literal dispatcher edges.
# In particular call_ext/call_lifted share Cpu (including flags); no ABI
# clobber assumption is valid here. Unknown transfers and exits stay live.
EDGE = re.compile(r'pc = (?:0x([0-9A-Fa-f]+)|if [^;]+? \{ 0x([0-9A-Fa-f]+) \} else \{ 0x([0-9A-Fa-f]+) \}); continue;\s*\}\s*$')
KILL_SENTINEL = ' cpu.fl = Flags::new(x86::LOGIC, a, b, r, 32);'


def _flow(arm):
    edge = EDGE.search(arm)
    if edge is None:
        return arm, ()
    # Keep the branch condition as a reader, but remove its final continue.
    return arm[:arm.rfind('continue;')], tuple(int(x, 16) for x in edge.groups() if x)


def _kills_before_read(text, at_exit=False):
    for kind, *_ in _scan(text):
        if kind == 'read':
            return False
        if kind == 'kill':
            return True
    return at_exit


def entry_kills(arms):
    """Least fixed point: every path overwrites before any observation.

    Starting false retains flag state on cycles with no proven overwrite.
    Conditional writes, INC/DEC and variable shifts cannot prove a kill.
    """
    bodies = {}
    for arm in arms:
        head = ARM_HEAD.match(arm)
        if head is not None:
            bodies[int(head[1], 16)] = _flow(arm)
    proven = set()
    changed = True
    while changed:
        changed = False
        for address, (body, successors) in bodies.items():
            if address not in proven and _kills_before_read(
                    body, bool(successors) and all(s in proven for s in successors)):
                proven.add(address)
                changed = True
    return proven


# Both literal cond() and L73's expanded predicates are accepted. Match
# longest first so compound conditions are one observation, not two.
CONDITION_FORMS = {}
for _cc in PREDICATE:
    CONDITION_FORMS[predicates(f'cpu.fl.cond("{_cc}")')] = _cc
    CONDITION_FORMS[f'cpu.fl.cond("{_cc}")'] = _cc
CONDITION_SITE = re.compile('|'.join(re.escape(s) for s in sorted(CONDITION_FORMS, key=len, reverse=True)))


def _condition_sites(arm):
    return [(m.start(), m.end(), CONDITION_FORMS[m[0]]) for m in CONDITION_SITE.finditer(arm)]


def direct_compare(op, bits, cc):
    """Native CMP/TEST predicates; inputs a,b are already width-masked.

    Derived from Flags::alu_sub/alu_and and Flags::cond (x86.rs), preserving
    signed overflow through signed operand comparisons, not signed result.
    Unsupported conditions leave the flag materialisation intact.
    """
    canonical = next((codes[0] for codes in CONDITIONS if cc in codes), None)
    if op == 'sub':
        unsigned = {'e': '==', 'ne': '!=', 'b': '<', 'ae': '>=', 'be': '<=', 'a': '>'}
        signed = {'l': '<', 'ge': '>=', 'le': '<=', 'g': '>'}
        if canonical in unsigned:
            return f'a {unsigned[canonical]} b'
        if canonical in signed:
            return f'(a as i{bits}) {signed[canonical]} (b as i{bits})'
    if op == 'and' and canonical in ('e', 'ne', 's', 'ns', 'l', 'ge', 'le', 'g'):
        result = f'((a & b) as i{bits})'
        relation = {'e': '==', 'ne': '!=', 's': '<', 'ns': '>=', 'l': '<', 'ge': '>=', 'le': '<=', 'g': '>'}[canonical]
        return f'{result} {relation} 0'
    return None


def _fuse_compare(arm, exit_dead):
    # Only the exact CMP/TEST template (no destination) and an immediately
    # following Jcc/SETcc/CMOVcc. All operand loads remain once, in order.
    candidates = [e for e in _scan(arm) if e[0] == 'kill' and e[4] is not None]
    for _, start, end, _, info in reversed(candidates):
        op, a, b, bits = info
        prefix = arm[:start]
        block = re.search(r'\{ let r = $', prefix)
        if block is not None:
            begin = block.start()
            tail = re.match(r'; \}\s*', arm[end:])
        else:
            begin = start
            tail = re.match(r';\s*', arm[end:])
        if tail is None or (block is None and not prefix.rstrip().endswith(('{', ';', '}'))):
            continue
        after = end + tail.end()
        sites = _condition_sites(arm[after:])
        if not sites:
            continue
        cs, ce, cc = sites[0]
        # Exact instruction prefixes. A CMOV memory source is read eagerly
        # by the original template, including when its condition is false.
        gap = arm[after:after + cs]
        if not (gap == 'pc = if ' or
                re.fullmatch(r'cpu\.(?:eax|ebx|ecx|edx|esi|edi|ebp|esp) = [^;{}]*', gap) or
                re.fullmatch(r'\{ let v = [^;{}]+; if ', gap)):
            continue
        cs, ce = after + cs, after + ce
        expression = direct_compare(op, bits, cc)
        if expression is None:
            continue
        _, successors = _flow(arm)
        suffix = arm[ce:arm.rfind('continue;')] if successors else arm[ce:]
        if not _kills_before_read(suffix, exit_dead and bool(successors)):
            # The defining CMP/TEST remains, including its complete lazy
            # flag representation. Its masked operands give the immediately
            # following predicate without re-reading operands or helper calls.
            expression = re.sub(r'\b[ab]\b', lambda m: 'cpu.fl.' + m[0], expression)
            arm = arm[:cs] + '(' + expression + ')' + arm[ce:]
            continue
        # Each supported comparison uses a and b exactly once, in order.
        # Inline the operands without a temporary Flags or duplicate loads.
        operands = {name: f'({value})' if bits == '32' else f'(({value}) & {MASK[bits]})'
                    for name, value in (('a', a), ('b', b))}
        expression = re.sub(r'\b[ab]\b', lambda m: operands[m[0]], expression)
        binding = f'let l129_condition = {expression}; '
        arm = arm[:begin] + binding + arm[after:cs] + 'l129_condition' + arm[ce:]
    return arm


def function_flags(arms):
    """Eliminate cross-block dead stores and fuse single-use CMP/TEST.

    Only whole-function arms call this pass; restartable instruction entries
    must expose literal machine state at every instruction boundary.
    """
    proven = entry_kills(arms)
    result = []
    for arm in arms:
        body, successors = _flow(arm)
        exit_dead = bool(successors) and all(s in proven for s in successors)
        arm = _fuse_compare(arm, exit_dead)
        body, successors = _flow(arm)
        if exit_dead:
            # The synthetic kill encodes the successor proof for the existing
            # local pass; retain the original transfer and never emit it.
            end = arm.rfind('continue;')
            prefix = dead_flags(arm[:end] + KILL_SENTINEL)
            assert prefix.endswith(KILL_SENTINEL)
            arm = prefix[:-len(KILL_SENTINEL)] + arm[end:]
        result.append(arm)
    return result


# Whole functions whose arms exceed this many bytes are split into part
# functions (L73: shard 05 at opt-level 1, see docs/tasks/L73-lean-emission.md).
SPLIT_BYTES = 16384
ARM_HEAD = re.compile(r"\s*0x([0-9A-Fa-f]+) => \{")
RETURN = re.compile(STRING + r"|\breturn;")


def split_function(addr, arms, limit=None):
    """Split a dispatcher function's lowered arms into part functions over
    contiguous address ranges, or None (small, or not provably safe).

    Arms keep all guest state in `cpu` and memory; their locals never
    outlive the arm. A part runs its own `loop { match pc }` and returns
    Some(pc) when control leaves its range (the dispatcher picks the part
    owning pc) or None where the function returned; an unknown pc inside a
    range is unreachable!() as before. Returns (dispatcher lines, part
    lines)."""
    limit = SPLIT_BYTES if limit is None else limit
    if len(arms) < 2 or sum(len(a) for a in arms) <= limit:
        return None
    heads = []
    for a in arms:
        m = ARM_HEAD.match(a)
        if m is None or re.search(r"\|m: &mut dyn Mem[^|]*\|[^;]*\breturn\b", a):
            return None  # a `return` inside a closure must stay a closure return
        heads.append(int(m[1], 16))
    if heads != sorted(heads):
        return None
    groups, size = [[]], 0
    for a, h in zip(arms, heads):
        if groups[-1] and size + len(a) > limit:
            groups.append([]); size = 0
        groups[-1].append((h, a)); size += len(a)
    name = f"f_{addr:08x}"
    dispatch = ["    loop {", "        pc = match pc {"]
    parts = []
    for g, group in enumerate(groups):
        span = f"0x{group[0][0]:X}..=0x{group[-1][0]:X}"
        dispatch.append(f"            {span} => match {name}_p{g}(m, cpu, pt, pc) {{ Some(next) => next, None => return }},")
        parts += [f"fn {name}_p{g}(m: &mut dyn Mem, cpu: &mut Cpu, pt: Option<std::ptr::NonNull<crate::mem::PageTable>>, "
                  "mut pc: u32) -> Option<u32> {",
                  "    loop {", "        match pc {",
                  *(RETURN.sub(lambda m: "return None;" if m[0] == "return;" else m[0], a) for _, a in group),
                  f"            _ if !({span}).contains(&pc) => return Some(pc),",
                  "            _ => unreachable!(),", "        }", "    }", "}"]
    dispatch += ["            _ => unreachable!(),", "        };", "    }"]
    return dispatch, parts


# Only real operand triples measured/tested by L146. Integer, control-word,
# extended-real operands and restartable instruction templates stay literal.
X87_REAL = {(0x00d9, 4, False), (0x10d9, 4, True), (0x18d9, 4, True),
            (0x00dd, 8, False), (0x10dd, 8, True), (0x18dd, 8, True),
            (0x00d8, 4, False), (0x08d8, 4, False)}
X87_MEMORY = re.compile(STRING + r"|\bcpu\.st\.memory::<0x([0-9a-fA-F]+), (4|8), (true|false)>")


def x87_operands(arm):
    """Keep each hardware instruction/access boundary; only retarget its helper."""
    def replace(m):
        if m[1] is None or (int(m[1], 16), int(m[2]), m[3] == 'true') not in X87_REAL:
            return m[0]
        return m[0].replace('.memory::', '.real_memory::')
    return X87_MEMORY.sub(replace, arm)
