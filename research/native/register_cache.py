"""Cache GPRs and flags across memory/x87 helpers in whole lifted functions.

Only known full-Cpu guest-call boundaries are accepted. Unknown bare Cpu
uses reject the function, without changing its text. Step functions never
use this pass. Memory access order and guest stack bytes are unchanged.
Like dead_flags, whole-function failure/panic is not a restart checkpoint.
"""
import re

from lean_emission import GPR, STRING, _close, _split_args

TOKEN = re.compile(STRING + r'|\bcpu\.(eax|ecx|edx|ebx|esp|ebp|esi|edi|fl)\b')
BARE = re.compile(STRING + r'|\bcpu\b(?!\.)')
CALL = re.compile(STRING + r'|\b((?:x86::call_(?:ext|local|lifted)|super::lifted::f_[0-9a-f]{8})\()')
HELPER = re.compile(STRING + r'|\b((?:x86::(?:pushfd_image|popfd|scan|string|string_from)|crate::platform::[a-z0-9_]+)\()')
STACK = re.compile(STRING + r'|\bemit::(push|pop)\(')
EXIT = re.compile(STRING + r'|\breturn(?: (?:None|Some\(pc\)))?(?=\s*[;,}])')


def cache_body(body):
    """Return transformed body, or the original when a boundary is unknown."""
    original = body
    # A Cpu method can observe the cache without containing a bare Cpu.
    fields = set(GPR) | {'fl', 'st', 'xmm', 'df', 'fs_base', 'seg', 'sys_flags'}
    if any(m[1] not in fields for m in re.finditer(STRING + r'|\bcpu\.(\w+)', body) if m[1]):
        return original
    returns = [m.start() for m in re.finditer(STRING + r'|\breturn\b', body) if m[0] == 'return']
    if returns != [m.start() for m in EXIT.finditer(body) if m[0].startswith('return')]:
        return original
    # Narrow stack helpers to ESP itself. Value evaluation precedes the
    # decrement, including PUSH ESP; POP ESP's assignment follows increment.
    edits = []
    for match in STACK.finditer(body):
        if not match[1]:
            continue
        end = _close(body, match.end())
        args = _split_args(body[match.end():end])
        if match[1] == 'push' and len(args) == 4 and args[1:] == ['m', 'pt', 'cpu']:
            value = ('{ let l161_push = ' + args[0] + '; cpu.esp = cpu.esp.wrapping_sub(4); '
                     'm.fast_set_u32(pt, cpu.esp, l161_push); }')
        elif match[1] == 'pop' and args == ['m', 'pt', 'cpu']:
            value = ('{ let l161_pop = m.fast_u32(pt, cpu.esp); '
                     'cpu.esp = cpu.esp.wrapping_add(4); l161_pop }')
        else:
            return original
        edits.append((match.start(), end + 1, value))
    for start, end, value in reversed(edits):
        body = body[:start] + value + body[end:]
    used = set(m[1] for m in TOKEN.finditer(body) if m[1])
    registers = [r for r in (*GPR, 'fl') if r in used]
    if not registers:
        return original
    save = ' '.join(f'cpu.{r} = l161_{r};' for r in registers)
    load = ' '.join(f'l161_{r} = cpu.{r};' for r in registers)
    placeholders = []
    edits = []
    for match in HELPER.finditer(body):
        if not match[1]:
            continue
        end = _close(body, match.end())
        call = body[match.start():end + 1]
        args = _split_args(body[match.end():end])
        if 'cpu' not in args or any('cpu' in a and a != 'cpu' for a in args):
            return original
        key = f'L161_BOUNDARY_{len(placeholders)}_END'
        placeholders.append('{ ' + save + ' let l161_value = ' + call + '; ' + load + ' l161_value }')
        edits.append((match.start(), end + 1, key))
    for match in CALL.finditer(body):
        if not match[1]:
            continue
        end = _close(body, match.end())
        call = body[match.start():end + 1]
        # Only a statement, never an argument/closure/assignment expression.
        before = body[:match.start()].rstrip()
        if not before.endswith((';', '{', '}')) or body[end + 1:end + 2] != ';':
            return original
        args = _split_args(body[match.end():end])
        if len(args) < 2 or args[:2] != ['m', 'cpu']:
            return original
        key = f'L161_BOUNDARY_{len(placeholders)}_END'
        placeholders.append('{ ' + save + ' ' + call + '; ' + load + ' }')
        edits.append((match.start(), end + 2, key))
    ordered = sorted(edits)
    if any(left[1] > right[0] for left, right in zip(ordered, ordered[1:])):
        return original  # nested CPU-taking expressions need their own proof
    for start, end, value in reversed(ordered):
        body = body[:start] + value + body[end:]
    if any(m[0] == 'cpu' for m in BARE.finditer(body)):
        return original
    body = TOKEN.sub(lambda m: f'l161_{m[1]}' if m[1] else m[0], body)
    body = EXIT.sub(lambda m: '{ ' + save + ' ' + m[0] + ' }' if m[0].startswith('return') else m[0], body)
    for index, replacement in enumerate(placeholders):
        body = body.replace(f'L161_BOUNDARY_{index}_END', replacement)
    declarations = '\n'.join(f'    let mut l161_{r} = cpu.{r};' for r in registers)
    return '\n' + declarations + body


def cache_functions(source):
    """Each split part has its own cache, flushed on Some(pc)/None exits."""
    starts = list(re.finditer(r'(?m)^(?:pub )?fn f_[0-9a-f]{8}(?:_p\d+)?\(', source))
    for match in reversed(starts):
        opening = source.index('{', match.end())
        closing = _close(source, opening + 1)
        body = source[opening + 1:closing]
        source = source[:opening + 1] + cache_body(body) + source[closing:]
    return source
