"""ddnative-base, with the ddnative-host policy installed behind its hooks
(L74), as `#[path]` modules of a single-file rustc test crate."""
from pathlib import Path


def modules(root: Path) -> str:
    """The module lines for the crate root. `extern crate self` lets the
    host's `ddnative_base::` paths name this crate. Each test calls
    `ddnative_host::install()` before running lifted code."""
    base = root / 'crates/ddnative-base/src'
    host = root / 'crates/ddnative-host/src/lib.rs'
    lines = ['extern crate self as ddnative_base;']
    lines += [f'#[path="{(base / f"{m}.rs").as_posix()}"] mod {m};' for m in ('mem', 'x86', 'x87', 'hooks')]
    lines.append(f'#[path="{host.as_posix()}"] mod ddnative_host;')
    return '\n'.join(lines) + '\n'
