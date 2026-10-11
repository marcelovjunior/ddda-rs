"""Synthetic L11 indirect transfers and mock ABI inference; no game bytes."""
import gc
import struct
import unittest
from unittest.mock import patch
from capstone import Cs, CS_ARCH_X86, CS_MODE_32
import harness as h
import lift

class IndirectTests(unittest.TestCase):
    def record(self, text, ext=True):
        code=bytes.fromhex(text); image=code.ljust(0x1000,b'\xcc')
        body=[i.address for i in Cs(CS_ARCH_X86,CS_MODE_32).disasm(code,0x400000)]
        with patch.object(h,'image',return_value=(0x400000,[(0x400000,image)])):
            return h.run_sparse(0x400000,17,body=body,zero_inputs=True,
                                **({'end':0x400000+len(code)} if ext else {'size':len(code)}))

    def tearDown(self): gc.collect()

    def test_register_and_memory_tail_targets(self):
        for code in ('b878563412ffe0','b8785634128901ff21'):
            c=self.record(code)
            self.assertEqual([v['target'] for v in c['calls']],[0x12345678])
            self.assertEqual(c['regs_out']['esp'],c['regs_in']['esp']+4)
        with self.assertRaises(h.Rejected):self.record('b878563412ffe0',ext=False)

    def test_local_indirect_target_and_emit_mem(self):
        # jz ret discovers a local return; mov eax,ret; jmp eax.
        code=bytes.fromhex('85c97407b80b004000ffe0c3')
        def read(a,n):return code[a-0x400000:a-0x400000+n]
        with patch.object(h,'code',read),patch.object(h,'executable',return_value=True):
            arms=lift.lift(0x400000,len(code),ext=True)
            self.assertIn('0x40000B', ' '.join(arms))
            self.assertIn('pc = t; continue;', ' '.join(arms))
        c=self.record('b80b00400085c07402ffe0c3')
        self.assertEqual(c['calls'],[])
        for text in ('ff6114','ffe0'):
            code=bytes.fromhex(text)
            with patch.object(h,'code',read),patch.object(h,'executable',return_value=True):
                arms=' '.join(lift.lift(0x400000,len(code),ext=True))
                self.assertIn('m.u32',arms) if text=='ff6114' else self.assertIn('let t = eax',arms)
                with self.assertRaises(lift.Unsupported):lift.lift(0x400000,len(code))

    def test_interleaved_and_conditional_saved_registers(self):
        for text in ('5689ce57ffd75f5ec3','85c075095689ce57ffd75f5ec3'):
            c=self.record(text)
            self.assertEqual(c['calls'][0]['out']['pop'],0)
            self.assertEqual(c['regs_out']['esp'],c['regs_in']['esp']+4)
        # saved ESI, modified ESI argument; only the argument is callee-popped.
        c=self.record('5689ce466aff56ffd05ec3')
        self.assertEqual(c['calls'][0]['out']['pop'],8)

    def test_caller_cleanup_is_not_popped_twice(self):
        c=self.record('906a016a02ffd083c408c3')
        self.assertEqual(c['calls'][0]['out']['pop'],0)
        self.assertEqual(c['regs_out']['esp'],c['regs_in']['esp']+4)

    def test_static_pop_follows_tail_not_neighbour(self):
        image=(bytes.fromhex('eb0eccc20800')+b'\xcc'*10+bytes.fromhex('c3')).ljust(4096,b'\xcc')
        with patch.object(h,'image',return_value=(0x400000,[(0x400000,image)])):
            self.assertEqual(h.static_pop(0x400000),0)
        # Different reachable ret cleanups are unknown; never choose first.
        image=bytes.fromhex('85c07403c20400c20800').ljust(4096,b'\xcc')
        with patch.object(h,'image',return_value=(0x400000,[(0x400000,image)])):
            self.assertEqual(h.static_pop(0x400000),0)

    def test_invalid_return_still_rejected(self):
        with self.assertRaises(h.Rejected):self.record('6a00c3')

    def test_static_pop_follows_only_bounded_switches(self):
        # cmp eax,1; ja default; jmp [eax*4+table]; default: ret 4
        prefix=bytes.fromhex('83f8017707ff248500104000c20400')
        for tail,want in ((bytes.fromhex('c20400'),4),(bytes.fromhex('c20800'),0)):
            data=prefix.ljust(32,b'\xcc')+tail
            sections=[(0x400000,data.ljust(4096,b'\xcc')),
                      (0x401000,struct.pack('<II',0x40000c,0x400020))]
            with patch.object(h,'image',return_value=(0x400000,sections)),patch.object(h,'executable',return_value=True):
                self.assertEqual(h.static_pop(0x400000),want)
                self.assertEqual(h.static_pop(0x400005),0)  # no input bound
        # A table target in another instruction's interior is not a contract.
        sections[1]=(0x401000,struct.pack('<II',0x40000d,0x400020))
        with patch.object(h,'image',return_value=(0x400000,sections)),patch.object(h,'executable',return_value=True):
            self.assertEqual(h.static_pop(0x400000),0)

if __name__=='__main__': unittest.main()
