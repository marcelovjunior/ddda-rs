"""L170: independently selectable inlinable ALU helpers, after flag liveness.

Only the method name changes: evaluation order and the complete lazy flag
state survive. Run before register caching. No instruction/condition folding.
"""
import os
import re

TOKEN = re.compile(r'"(?:\\.|[^"\\])*"|//[^\n]*|\b(cpu\.fl|l161_fl)\.alu_(add|sub|and|or|xor)\(')


def retarget(source):
    if os.environ.get('LIFT_INLINE_FLAGS', '0') != '1':
        return source
    return TOKEN.sub(lambda m: m[0] if m[1] is None else
                     f'{m[1]}.alu_{m[2]}_inline(', source)
