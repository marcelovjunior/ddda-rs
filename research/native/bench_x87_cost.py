"""Run the synthetic x87 benchmark with caller opt1 and shared helpers opt3.

Set CARGO_TARGET_DIR to the task's private target directory. Builds only
ddnative-base/host and the benchmark; no generated shards or game process.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--iterations', type=int, default=20_000_000)
    parser.add_argument('--build-only', action='store_true')
    args = parser.parse_args()
    if args.iterations <= 0:
        parser.error('--iterations must be positive')
    if not os.environ.get('CARGO_TARGET_DIR'):
        parser.error('set CARGO_TARGET_DIR to your private task target')
    repo = Path(__file__).resolve().parents[2]
    out = Path(os.environ['CARGO_TARGET_DIR']).resolve() / 'x87-cost-driver'
    out.mkdir(parents=True, exist_ok=True)
    quote = lambda path: json.dumps(path.as_posix())
    manifest = '''[package]
name="x87-cost"
version="0.0.0"
edition="2021"
[workspace]
[dependencies]
'''
    for name in ('ddnative-base', 'ddnative-host'):
        manifest += f'{name}={{path={quote(repo / "crates" / name)}}}\n'
    manifest += f'''[[bin]]
name="x87-cost"
path={quote(repo / 'crates/ddnative-base/examples/x87_cost.rs')}
[profile.play]
inherits="release"
opt-level=1
[profile.play.package.ddnative-base]
opt-level=3
[profile.play.package.ddnative-host]
opt-level=3
'''
    (out / 'Cargo.toml').write_text(manifest, encoding='utf-8')
    command = ['cargo', 'build' if args.build_only else 'run',
               '--manifest-path', str(out / 'Cargo.toml'), '--profile', 'play']
    if not args.build_only:
        command += ['--', str(args.iterations)]
    env = dict(os.environ, CARGO_BUILD_JOBS='4', CARGO_INCREMENTAL='0')
    raise SystemExit(subprocess.call(command, env=env))


if __name__ == '__main__':
    main()
