"""L190 experiment: rewrite the generated Rust so that dword accesses through ESP or EBP (and
push/pop) use the CPU's stack window (`emit::stack_u32`, `stack_set_u32`, `push_s`, `pop_s`).
    python stack_window.py in.rs out.rs
The text transformation is what a lifter change would emit; it is used to measure the gain on
the existing lifted functions before touching lift.py."""
import re
import sys


def args(s, start):
    """Split the call arguments that begin at `start` (just after the opening parenthesis);
    returns (list of argument texts, index after the closing parenthesis)."""
    depth, cur, out, i = 0, start, [], start
    while True:
        c = s[i]
        if c in "([{":
            depth += 1
        elif c in ")]}":
            if depth == 0:
                out.append(s[cur:i].strip())
                return out, i + 1
            depth -= 1
        elif c == "," and depth == 0:
            out.append(s[cur:i].strip())
            cur = i + 1
        i += 1


def transform(src):
    out, pos, n = [], 0, 0
    pat = re.compile(r"m\.fast_u32\(pt, |m\.fast_set_u32\(pt, |emit::push\(|emit::pop\(")
    while True:
        m = pat.search(src, pos)
        if not m:
            out.append(src[pos:])
            break
        a, end = args(src, m.end())
        head = m.group(0)
        out.append(src[pos:m.start()])
        if head.startswith("m.fast_u32"):
            if re.match(r"cpu\.(esp|ebp)\b", a[0]):
                out.append(f"emit::stack_u32(m, pt, cpu, {a[0]})"); n += 1
            else:
                out.append(f"m.fast_u32(pt, {a[0]})")
        elif head.startswith("m.fast_set_u32"):
            if re.match(r"cpu\.(esp|ebp)\b", a[0]):
                out.append(f"emit::stack_set_u32(m, pt, cpu, {a[0]}, {a[1]})"); n += 1
            else:
                out.append(f"m.fast_set_u32(pt, {a[0]}, {a[1]})")
        elif head == "emit::push(":
            out.append("emit::push_s(" + ", ".join(a) + ")"); n += 1
        else:
            out.append("emit::pop_s(" + ", ".join(a) + ")"); n += 1
        pos = end
    return "".join(out), n


if __name__ == "__main__":
    text, n = transform(open(sys.argv[1]).read())
    open(sys.argv[2], "w", newline="\n").write(text)
    print(n, "stack accesses rewritten")
