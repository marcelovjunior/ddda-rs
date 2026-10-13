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


# a plain block: after a statement, a block or an arm head, never an `if`
# (group 1 keeps that prefix)
PLAIN = r"(^|[;{}] *|=> *)"
FOLD = re.compile(r"" + PLAIN + r"\{ let r = ([^;{}\"]*); (cpu\.(?:" + "|".join(GPR) + r")) = r; \}")


def fold_results(arm):
    """`{ let r = E; cpu.R = r; }` -> `cpu.R = E;` (E has no side effect on
    cpu.R, and is evaluated before the store either way)."""
    return FOLD.sub(lambda m: f"{m[1]}{m[3]} = {m[2]};", arm)


PUSH = re.compile(r"" + PLAIN + r"\{ let v = ([^;{}\"]*); cpu\.esp = cpu\.esp\.wrapping_sub\(4\); m\.fast_set_u32\(pt, cpu\.esp, v\); \}")
PUSH_CONST = re.compile(r"cpu\.esp = cpu\.esp\.wrapping_sub\(4\); m\.fast_set_u32\(pt, cpu\.esp, (0x[0-9A-F]+)\);")
POP = re.compile(r"" + PLAIN + r"\{ let v = m\.fast_u32\(pt, cpu\.esp\); cpu\.esp = cpu\.esp\.wrapping_add\(4\); ")
POP_REG = re.compile(r"\{ let v = emit::pop\(m, pt, cpu\); (cpu\.(?:" + "|".join(GPR) + r")) = v; \}")
CLOSURE = re.compile(r"\(\|m: &mut dyn Mem, cpu: &mut Cpu\| (x86::call_(?:lifted|ext)\(m, cpu, 0x[0-9A-F]+\))\)\(m, cpu\)")


def idioms(arm):
    """Push/pop through ddnative_base::emit, direct call targets."""
    arm = PUSH.sub(lambda m: f"{m[1]}emit::push({m[2]}, m, pt, cpu);", arm)
    arm = PUSH_CONST.sub(lambda m: f"emit::push({m[1]}, m, pt, cpu);", arm)
    arm = POP.sub(lambda m: m[1] + "{ let v = emit::pop(m, pt, cpu); ", arm)
    arm = POP_REG.sub(lambda m: f"{m[1]} = emit::pop(m, pt, cpu);", arm)
    arm = CLOSURE.sub(lambda m: m[1], arm)
    arm = ABSOLUTE.sub(lambda m: f"{m[1]}u32", arm)
    return sse(arm)


XMM = r"cpu\.xmm\[[0-7]\]"
ABSOLUTE = re.compile(r"\b0u32\.wrapping_add\((0x[0-9A-F]+)\)")  # 0 + disp: an absolute address
LOAD128 = re.compile(r"\[m\.fast_u32\(pt, a(?:\.wrapping_add\(0\))?\), m\.fast_u32\(pt, a\.wrapping_add\(4\)\), "
                     r"m\.fast_u32\(pt, a\.wrapping_add\(8\)\), m\.fast_u32\(pt, a\.wrapping_add\(12\)\)\]")
LOAD64 = re.compile(r"\[m\.fast_u32\(pt, a(?:\.wrapping_add\(0\))?\), m\.fast_u32\(pt, a\.wrapping_add\(4\)\), 0, 0\]")
STORE128 = re.compile(r"for k in 0\.\.4u32 \{ m\.fast_set_u32\(pt, a\.wrapping_add\(4 \* k\), (" + XMM + r")\[k as usize\]\); \}")
LANES_FLOAT = re.compile(r"for k in 0\.\.4 \{ (" + XMM + r")\[k\] = x86::(add|sub|mul)ss\(\1\[k\], (" + XMM + r")\[k\]\); \}")
LANES_LOGIC = re.compile(r"for k in 0\.\.4 \{ let \(d, s\) = \((" + XMM + r")\[k\], (" + XMM + r"|s)\[k\]\); \1\[k\] = "
                         r"(d \^ s|d & s|d \| s|!d & s); \}")
LOGIC = {"d ^ s": "xor", "d & s": "and", "d | s": "or", "!d & s": "andn"}


def sse(arm):
    """Packed SSE templates: the 16/8-byte loads and the 16-byte store as
    one call each (same accesses in the same order), the lane loops of
    ADDPS/SUBPS/MULPS xmm, xmm as the packed helper the memory form already
    uses (the original instruction, one host MULPS instead of four MULSS:
    per-lane IEEE results and the sticky MXCSR flags are the same), and
    the bitwise lane loops as emit helpers."""
    if "a" in arm:
        arm = LOAD128.sub("emit::load128(m, pt, a)", arm)
        arm = LOAD64.sub("emit::load64(m, pt, a)", arm)
        arm = STORE128.sub(lambda m: f"emit::store128({m[1]}, m, pt, a);", arm)
    if "for k" in arm:
        arm = LANES_FLOAT.sub(lambda m: f"{m[1]} = x86::{m[2]}ps({m[1]}, {m[3]});", arm)
        arm = LANES_LOGIC.sub(lambda m: f"{m[1]} = emit::{LOGIC[m[3]]}ps({m[1]}, {m[2]});", arm)
    return arm


def lean(arm):
    """All lean passes, in order, for one lowered whole-function arm."""
    return idioms(fold_results(dead_flags(arm)))
