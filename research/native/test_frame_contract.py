"""Caller-frame recorder contract: derived exit ESP, exact check, ordinary fallback."""
import gc
import unittest
from unittest.mock import patch

from capstone import Cs,CS_ARCH_X86,CS_MODE_32
import harness as h


class FrameContractTests(unittest.TestCase):
    def tearDown(self):
        gc.collect()

    def image(self, code):
        image=code.ljust(0x1000,b'\xcc')
        body=[i.address for i in Cs(CS_ARCH_X86,CS_MODE_32).disasm(code,0x400000)]
        return body,(patch.object(h,'image',return_value=(0x400000,[(0x400000,image)])),
                     patch.object(h,'code',side_effect=lambda a,n:image[a-0x400000:a-0x400000+n]))

    def contract(self, text):
        body,(p1,p2)=self.image(bytes.fromhex(text))
        with p1,p2:
            return h.frame_exit(0x400000,body)

    def record(self, text, strategy='small-v1', seed=5):
        code=bytes.fromhex(text)
        body,(p1,p2)=self.image(code)
        with p1,p2,patch.object(h,'static_pop',return_value=0):
            return h.run_sparse(0x400000,seed,body=body,end=0x400000+len(code),input_strategy=strategy)

    # mov eax,[esp+4]; pop ecx; sub esp,eax; push ecx; ret
    PROLOG='8b44240459 2be0 51 c3'.replace(' ','')
    # pop ecx; mov esp,ebp; pop ebp; push ecx; ret
    EPILOG='59 8be5 5d 51 c3'.replace(' ','')

    def test_derived_exit_expressions(self):
        self.assertEqual(self.contract(self.PROLOG),('esp',4,((-1,4),)))
        self.assertEqual(self.contract(self.EPILOG),('ebp',4,()))
        self.assertTrue(h.caller_frame(self.contract(self.PROLOG)))
        self.assertEqual(h.frame_text(self.contract(self.PROLOG)),'esp + 0x4 - [esp+0x4]')
        self.assertEqual(h.frame_text(self.contract(self.EPILOG)),'ebp + 0x4')

    def test_ordinary_and_unknown_bodies_keep_the_ordinary_contract(self):
        # push ebp; mov ebp,esp; pop ebp; ret 8
        ordinary=self.contract('558bec5dc20800')
        self.assertEqual(ordinary,('esp',12,()))
        self.assertFalse(h.caller_frame(ordinary))
        # an out-of-range ordinary exit still fails the range check
        self.assertEqual(self.contract('c27e05'),('esp',0x582,()))
        self.assertFalse(h.caller_frame(self.contract('c27e05')))
        with self.assertRaisesRegex(h.Rejected,'misaligned stack'):
            self.record('c27e05')
        # call; ret / and esp,-16; ret / leave; ret / two disagreeing rets
        for text in ('e8fb000000c3','83e4f0c3','c9c3','85c07401c359c3'):
            self.assertIsNone(self.contract(text),text)
        # a store through an unknown pointer before the slot load aliases it
        self.assertIsNone(self.contract('c701000000008b442404592be051c3'))
        # a slot written before it is read is not an entry input
        self.assertIsNone(self.contract('89542404'+self.PROLOG))

    def test_recorded_runs_check_the_contract_exactly(self):
        for text,rule in ((self.PROLOG,'esp + 0x4 - [esp+0x4]'),(self.EPILOG,'ebp + 0x4')):
            c=self.record(text)
            self.assertEqual(c['stack_contract'],{'rule':'caller-frame-v1','exit_esp':rule})
            regs=c['regs_in']
            if text==self.EPILOG:
                self.assertEqual(c['regs_out']['esp'],(regs['ebp']+4)&0xffffffff)
            else:
                slot=b''.join(bytes.fromhex(v)[a2:a2+1] for a,v in c['reads']
                              for a2 in range(len(bytes.fromhex(v))) if regs['esp']+4<=a+a2<regs['esp']+8)
                self.assertEqual(c['regs_out']['esp'],(regs['esp']+4-int.from_bytes(slot,'little'))&0xffffffff)
        # ordinary functions are not tagged
        self.assertNotIn('stack_contract',self.record('558bec5dc3'))

    def test_contract_never_excuses_a_wrong_return_target(self):
        # pop ecx; pop edx; mov esp,ebp; pop ebp; push edx; ret: returns via [esp+4]
        with self.assertRaisesRegex(h.Rejected,'instead of the caller|outside the image'):
            self.record('595a8be55d52c3')


    def test_closed_frame_helper_keeps_the_caller_as_root(self):
        # caller: push ebp; mov ebp,esp; push 8; call P; call M (mocked);
        # mov esp,ebp; pop ebp; ret. P = PROLOG at 0x400100 (closed).
        caller=bytes.fromhex('558bec6a08e8f6000000e8f10100008be55dc3')
        image=bytearray(caller.ljust(0x1000,bytes([0xcc])))
        prolog=bytes.fromhex(self.PROLOG)
        image[0x100:0x100+len(prolog)]=prolog
        image=bytes(image)
        md=Cs(CS_ARCH_X86,CS_MODE_32)
        body=[i.address for i in md.disasm(caller,0x400000)]
        pbody=[i.address for i in md.disasm(prolog,0x400100)]
        with (patch.object(h,'image',return_value=(0x400000,[(0x400000,image)])),
              patch.object(h,'code',side_effect=lambda a,n:image[a-0x400000:a-0x400000+n]),
              patch.object(h,'static_pop',return_value=0)):
            contract=h.frame_exit(0x400100,pbody)
            run=lambda **kw:h.run_sparse(0x400000,7,body=body,end=0x400000+len(caller),
                                         lifted=frozenset({0x400100}),input_strategy='small-v1',**kw)
            with self.assertRaises(h.Rejected):
                run()  # the prolog returns below the call's ESP: the caller looks nested
            c=run(frame_contracts={0x400100:list(contract)})
        self.assertEqual([e['target'] for e in c['calls']],[0x400200])
        self.assertNotIn('stack_contract',c)


if __name__=='__main__':unittest.main()
