import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest

import l170_check as check


class AssemblyAuditTests(unittest.TestCase):
    def test_local_jumps_are_not_helper_calls(self):
        result = check.counts('''
    callq _RNv5Flags7alu_sub
    callq *96(%rdx)
    jmp .LBB0_17
    jmpq *%rax
    jmp _RNv5Flags2zf
''')
        self.assertEqual(result['calls'], 3)
        self.assertEqual(result['flags'], 2)
        self.assertEqual(result['indirect'], 1)

    def test_split_parts_are_counted_with_the_root(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            (target / 'measure/deps').mkdir(parents=True)
            source = ''
            for variant in ('baseline', 'candidate'):
                for address in check.ADDRESSES:
                    source += f'_RN{variant}10f_{address}:\n callq _RNfast_u32\n.seh_endproc\n'
            source += '_RNbaseline13f_00fee320_p2:\n callq _RNfast_u32\n.seh_endproc\n'
            (target / 'measure/deps/l170_isolated-test.s').write_text(source)
            with contextlib.redirect_stdout(io.StringIO()):
                check.assembly_report(target, target, dict.fromkeys(check.ADDRESSES, 4))
            report = json.loads((target / 'assembly-counts.json').read_text())
            self.assertEqual(report['baseline-00fee320']['memory'], 2)
            self.assertEqual(report['baseline-00fee320']['host_functions'], 2)
            self.assertEqual(report['candidate-00fee320']['memory'], 1)
            self.assertEqual(report['candidate-00fee320']['host_functions'], 1)


if __name__ == '__main__':
    unittest.main()
