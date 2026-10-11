"""Synthetic x87 programs: emitted Rust versus original instructions in Unicorn."""
from pathlib import Path
import os
import struct
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import harness
import lift


class X87Tests(unittest.TestCase):
    def test_recorder_rejects_stack_faults(self):
        for code in (bytes.fromhex("d9e8") * 9 + b'\xc3', bytes.fromhex("ddd8c3")):
            with patch.object(harness, "image", return_value=(0x400000, [(0x400000, code.ljust(0x1000, b'\x90'))])):
                with self.assertRaisesRegex(harness.Rejected, "x87 stack fault"):
                    harness.run_sparse(0x400000, 17, zero_inputs=True, size=len(code))

    def test_immediate_mocked_float_return(self):
        # call outside the function; fstp [0x2000]; ret. L04's push-site
        # contract covers an immediate consumer as well as one after mov.
        code = bytes.fromhex("e8fb000000d91d00200000c3")
        with patch.object(harness, "code", return_value=code):
            arms = lift.lift(0x400000, len(code), ext=True)
        self.assertEqual(lift.X87_PUSH_SITES[0x400000], [0x400005])
        self.assertIn("call_ext", " ".join(arms))

    def test_stack_requirements_and_control(self):
        for code in ("d8c1c3", "d9c0c3", "d9e8ded9c3", "d9e8d9c9c3"):
            data = bytes.fromhex(code)
            with patch.object(harness, "code", return_value=data), self.assertRaises(lift.Unsupported):
                lift.lift(0x400000, len(data))

    def test_instruction_forms_against_unicorn(self):
        # Float 1.5 and integer 3 in a synthetic image. Outputs are sparse RAM.
        load = "d90500104000"  # fld dword [0x401000]
        integer = "db0508104000"  # fild dword [0x401008]
        store = "dd1d00200000"  # fstp qword [0x2000]
        programs = [
            (load + integer + op + store + "c3")
            for op in ("dec1", "dec9", "dee9", "dee1", "def9", "def1")
        ]
        programs += [load + integer + "d9c9" + "dd1d00200000dd1d08200000c3",
                     load + "d9c0" + "ddd9" + store + "c3",
                     load + integer + "ded9dfe09ec3",  # fcompp; fnstsw ax; sahf
                     load + integer + "dfe9" + store + "9fc3",  # fucomip; lahf
                     integer + "d91d00200000c3",  # fild -> f32
                     load + "df3d00200000c3",  # fistp i64
                     load + "d9fa" + store + "c3",  # sqrt
                     load + "dc0500104000" + store + "c3",  # fadd m64
                     load + "da0508104000" + store + "c3",  # fiadd m32
                     load + "c3", integer + "c3", load + integer + "c3",  # extended returns
                     load + "9b" + store + "c3"]  # waiting forms with masked exceptions
        root = Path(__file__).resolve().parents[2]
        source = "#![allow(warnings)]\n" + "".join(
            f'#[path="{(root / f"crates/ddnative-base/src/{m}.rs").as_posix()}"] mod {m};\n'
            for m in ("mem", "x86", "x87"))
        source += "use mem::Mem; use x86::{Cpu, Flags};\n"
        for k, hexcode in enumerate(programs):
            code = bytes.fromhex(hexcode)
            data = code.ljust(0x1000, b'\x90') + struct.pack('<fIi', 1.5, 0x3ff00000, 3)
            with patch.object(harness, "image", return_value=(0x400000, [(0x400000, data)])):
                case = harness.run_sparse(0x400000, 17, zero_inputs=True, size=len(code))
                arms = lift.lift(0x400000, len(code))
            body = lift.rust_fn(0x400000, 'synthetic x87', arms).replace('f_00400000', f'fixture_{k}')
            source += body
            source += f'#[test] fn case_{k}() {{ let mut m=mem::Sparse::default(); let mut c=Cpu::default();\n'
            for reg, value in case['regs_in'].items():
                source += f'c.{reg}={value};\n'
            for addr, raw in case['reads']:
                for offset, byte in enumerate(bytes.fromhex(raw)):
                    source += f'm.bytes.insert({addr + offset},{byte});\n'
            source += f'fixture_{k}(&mut m,&mut c);\n'
            source += f'assert_eq!(c.st.control(), {case["x87_control"]});\n'
            source += f'assert_eq!(c.st.depth(), {case["x87_depth"]});\n'
            extended = case['st0_ext']
            want = f'Some(({extended[0]}u64,{extended[1]}u16))' if extended else 'None'
            source += f'assert_eq!(c.st.top_extended(), {want});\n'
            for reg, value in case['regs_out'].items():
                source += f'assert_eq!(c.{reg},{value}, "{reg}");\n'
            source += 'let mut expected=std::collections::BTreeMap::new();\n'
            for addr, raw in case['writes']:
                for offset, byte in enumerate(bytes.fromhex(raw)):
                    source += f'expected.insert({addr + offset},{byte});\n'
            source += 'assert_eq!(m.written,expected); }\n'
        (root / 'target').mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=root / 'target', prefix='x87-test-') as folder:
            path = Path(folder)
            (path / 'test.rs').write_text(source)
            exe = path / ('test.exe' if os.name == 'nt' else 'test')
            build = subprocess.run(['rustc', '--edition=2021', '--test', str(path / 'test.rs'), '-o', str(exe)], capture_output=True, text=True)
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            run = subprocess.run([str(exe)], capture_output=True, text=True)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)


if __name__ == '__main__':
    unittest.main()
