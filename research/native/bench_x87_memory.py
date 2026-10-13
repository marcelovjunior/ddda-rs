"""Build/run an isolated x87/flag predicate benchmark, with opt0 or opt1 callers.

No generated shards or game process. The base crate uses opt3 in both
cases, as in the play profile. Run through heavy.py and outside head runs.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--suite', choices=('x87_memory', 'flag_predicate'), default='x87_memory')
    parser.add_argument('--caller-opt', type=int, choices=(0, 1), default=0)
    parser.add_argument('--iterations', type=int, default=100_000)
    parser.add_argument('--build-only', action='store_true')
    parser.add_argument('--cpu', type=int, help='Windows logical CPU affinity')
    args = parser.parse_args()
    if args.iterations <= 0:
        parser.error('--iterations must be positive')
    if not os.environ.get('CARGO_TARGET_DIR'):
        parser.error('set CARGO_TARGET_DIR to the task target directory')
    repo = Path(__file__).resolve().parents[2]
    target = Path(os.environ['CARGO_TARGET_DIR']).resolve()
    out = target / f'{args.suite}-driver-o{args.caller_opt}'
    out.mkdir(parents=True, exist_ok=True)
    quote = lambda p: json.dumps(p.as_posix())
    name = f'{args.suite}-cost-o{args.caller_opt}'
    (out / 'Cargo.toml').write_text(f'''[package]
name="{name}"
version="0.0.0"
edition="2021"
[workspace]
[dependencies]
ddnative-base={{path={quote(repo / 'crates/ddnative-base')}}}
ddnative-host={{path={quote(repo / 'crates/ddnative-host')}}}
[[bin]]
name="{name}"
path={quote(repo / ('crates/ddnative-base/examples/' + args.suite + '_cost.rs'))}
[profile.measure]
inherits="release"
opt-level={args.caller_opt}
[profile.measure.package.ddnative-base]
opt-level=3
[profile.measure.package.ddnative-host]
opt-level=3
''', encoding='utf-8')
    env = dict(os.environ, CARGO_BUILD_JOBS='4', CARGO_INCREMENTAL='0')
    subprocess.run(['cargo', 'build', '--manifest-path', str(out / 'Cargo.toml'),
                    '--profile', 'measure'], env=env, check=True)
    if args.build_only:
        return
    if args.cpu is not None:
        if os.name != 'nt' or not 0 <= args.cpu < 64:
            parser.error('--cpu requires Windows and a CPU index from 0 to 63')
        import ctypes
        kernel = ctypes.windll.kernel32
        kernel.GetCurrentProcess.restype = ctypes.c_void_p
        kernel.SetProcessAffinityMask.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
        if not kernel.SetProcessAffinityMask(kernel.GetCurrentProcess(), 1 << args.cpu):
            raise OSError('SetProcessAffinityMask failed')
    exe = target / 'measure' / (name + ('.exe' if os.name == 'nt' else ''))
    subprocess.run([str(exe), str(args.iterations)], env=env, check=True, timeout=30)


if __name__ == '__main__':
    main()
