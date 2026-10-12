"""Production instruction emitter -> real Registry/Session/SEH, no game build.

Authored x86 fixtures always run; when the private exe exists, two original
EH4 bodies are emitted into the temporary crate too. The Windows SEH oracle
supplies the expected DIV, AV, split-page and nonlocal-unwind observations.
"""
import os
import contextlib
import io
import json
from pathlib import Path
import subprocess
import struct
import tempfile
import unittest
from unittest.mock import patch

import harness as h
import lift


PROGRAMS = {
    0x400000: 'b80100000031d231c9fdf7f1fcc3',  # DIV zero with saved DF
    0x401000: 'b8adde0000c70001000000c3',  # MOV through invalid EAX
    0x402000: 'b800900000ffd0b8eeeeeeeec3',  # service callback; outer __except at RET
    0x403000: 'b8adde0000c70001000000c20400',  # stdcall callback faults
    0x404000: 'b878563412b902000000bffc1f0100f3abc3',  # REP later-iteration fault
    0x405000: 'ff1424c3',  # CALL [ESP]: evaluate before push
    0x406000: 'b8fe1f0100c700ffffffffc3',  # split-page write
    0x407000: 'b800910000ffd0c3',  # software exception service
    0x408000: 'b800920000ffd0c20400',  # nested callback to another service
    0x409000: '8b442404c20400',  # ordinary stdcall callback
    # A cdecl handler repairs CONTEXT.Eax and returns ContinueExecution.
    0x410000: '8b44240cc780b00000000010010031c0c3',
    # A nonreturning handler restores the outer ESP then jumps to its body.
    0x411000: 'bcfcfeff7eb84d000000e9' + struct.pack('<i', 0x40200c-0x41100f).hex(),
    0x412000: '585031c0c3',  # transient POP/PUSH above callback return slot
    0x413000: '58ffe0',  # naked POP return-address; JMP EAX
    0x415000: '89ecb84d000000c3',  # outer scope body restores ESP from EBP
    0x510000: 'e9' + struct.pack('<i', 0x40200c-0x510005).hex(),  # cross-shard shared epilogue
}


class StepTests(unittest.TestCase):
    def test_gen_publishes_exact_keys_and_cross_shard_shared_bodies(self):
        blocks = {a: bytes.fromhex(code) for a, code in PROGRAMS.items()}
        def read(a, n):
            for base, data in blocks.items():
                if base <= a < base + len(data):
                    return data[a-base:a-base+n]
            raise KeyError(a)
        def executable(a, n=1):
            return any(base <= a and a+n <= base+len(data) for base, data in blocks.items())
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            out = root / 'crates/ddnative/src/generated'
            out.mkdir(parents=True)
            (out / 'mod.rs').write_text('pub mod lifted;\n')
            (root / 'patterns').mkdir()
            fs = [dict(addr=a, size=len(b), name='synthetic', ext=True,
                       arms=[f'0x{a:X} => {{ return; }}']) for a, b in blocks.items()]
            (root / 'patterns/lift.json').write_text(json.dumps(fs))
            with patch.object(lift, 'R', str(root)), patch.object(lift, 'N', str(root)), \
                 patch.object(h, 'code', read), patch.object(h, 'executable', executable), \
                 patch.object(lift, 'function_entries', return_value=frozenset(blocks)), \
                 patch.object(lift, 'noreturn_entries', return_value=frozenset()), contextlib.redirect_stdout(io.StringIO()):
                lift.gen()
                stamps = {p: p.stat().st_mtime_ns for p in root.rglob('*.rs')}
                lift.gen()
                self.assertEqual(stamps, {p: p.stat().st_mtime_ns for p in stamps})
            index = (root / 'crates/ddlift/lift_05/src/generated/mod.rs').read_text()
            self.assertIn('(0x0040200C, s_00510000)', index)
            self.assertNotIn('(0x0040200B,', index)
            facade = (out / 'lifted.rs').read_text()
            for k in range(16):
                self.assertIn(f'ddlift_{k:02}::instruction(ip)', facade)

    def test_emitted_instructions_in_session(self):
        root = Path(__file__).resolve().parents[2]
        blocks = {a: bytes.fromhex(code) for a, code in PROGRAMS.items()}
        def read(a, n):
            for base, data in blocks.items():
                if base <= a < base + len(data):
                    return data[a-base:a-base+n]
            raise KeyError(a)
        def executable(a, n=1):
            return any(base <= a and a+n <= base+len(data) for base, data in blocks.items())
        sources, owners = [], {}
        with patch.object(h, 'code', read), patch.object(h, 'executable', executable), \
             patch.object(lift, 'function_entries', return_value=frozenset(blocks)), \
             patch.object(lift, 'noreturn_entries', return_value=frozenset()):
            for addr, data in blocks.items():
                text, ips = lift.step_source(dict(addr=addr, size=len(data), name='authored fixture', ext=True), set(blocks))
                sources.append(text)
                owners.update((ip, addr) for ip in ips)
        self.assertNotIn('call_ext', '\n'.join(sources))
        self.assertNotIn('continue;', '\n'.join(sources))
        original = Path(h.EXE).exists()
        if original:
            # Private bytes are read locally and the temporary source is
            # removed. These two original bodies run, neither is mocked.
            addresses = {0x01303cb9, 0x013070e5}
            with patch.object(lift, 'function_entries', return_value=frozenset(addresses)), \
                 patch.object(lift, 'noreturn_entries', return_value=frozenset()):
                for addr in sorted(addresses):
                    text, ips = lift.step_source(dict(addr=addr, size=1, name='original EH4 transfer/notification', ext=True), addresses)
                    sources.append(text)
                    owners.update((ip, addr) for ip in ips)
        lookup = 'fn lookup(ip: u32) -> Option<ddnative_base::execution::Step> { match ip {\n'
        lookup += ''.join(f'0x{ip:X} => Some(s_{owner:08x}),\n' for ip, owner in sorted(owners.items()))
        lookup += '_ => None } }\n'
        target = Path(os.environ.get('CARGO_TARGET_DIR', 'C:/ddda-targets/L27b'))
        target.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=target, prefix='step-fixture-') as tmp:
            tmp = Path(tmp)
            (tmp / 'src').mkdir()
            (tmp / 'Cargo.toml').write_text(f'''[package]
name="seh-step-fixture"
version="0.0.0"
edition="2021"
[features]
original-fixture=[]
[dependencies]
ddnative-base={{path="{root.as_posix()}/crates/ddnative-base"}}
ddruntime={{path="{root.as_posix()}/crates/ddruntime",default-features=false}}
''', encoding='utf-8')
            fixture = (root / 'research/native/step_fixture.rs').read_text(encoding='utf-8')
            (tmp / 'src/lib.rs').write_text(fixture + '\n' + '\n'.join(sources) + lookup, encoding='utf-8')
            env = dict(os.environ, CARGO_TARGET_DIR=str(target), CARGO_INCREMENTAL='0', CARGO_BUILD_JOBS='4')
            command = ['cargo', 'test', '--offline', '--manifest-path', str(tmp / 'Cargo.toml')]
            if original:
                command += ['--features', 'original-fixture']
            subprocess.run(command, env=env, check=True)


if __name__ == '__main__':
    unittest.main()
