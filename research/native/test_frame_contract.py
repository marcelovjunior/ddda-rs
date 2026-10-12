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


    def test_noreturn_guard_exempts_only_the_root_entry(self):
        # push ebp; mov ebp,esp; call 0x400100; pop ebp; ret
        code=bytes.fromhex('558bece8f8000000 5dc3'.replace(' ',''))
        body,(p1,p2)=self.image(code)
        with p1,p2,patch.object(h,'static_pop',return_value=0):
            run=lambda targets:h.run_sparse(0x400000,3,body=body,end=0x400000+len(code),noreturn_targets=targets)
            self.assertEqual([e['target'] for e in run({0x400000})['calls']],[0x400100])
            with self.assertRaisesRegex(h.Rejected,'noreturn target at 0x00400100'):
                h.run_sparse(0x400000,3,body=body,end=0x400000+len(code),lifted={0x400100},
                             noreturn_targets={0x400000,0x400100})


    def memset_run(self, text):
        code=bytes.fromhex(text.replace(' ',''))
        body,(p1,p2)=self.image(code)
        with p1,p2,patch.dict(h.PLATFORM,{0x400100:'memset'}),patch.object(h,'static_pop',return_value=0):
            return h.run_sparse(0x400000,5,body=body,end=0x400000+len(code),input_strategy='small-v1')

    def test_constant_model_sizes_get_their_own_budget(self):
        # push 0x80000; push 0; push 0x30000000; call memset; add esp,12; ret
        c=self.memset_run('6800000800 6a00 6800000030 e8ef000000 83c40c c3')
        self.assertIn([0x30000000,'00'*0x80000],c['writes'])
        # the same size from a register is an input: the ordinary 64 KB guard
        with self.assertRaisesRegex(h.Rejected,'memset of 0x80000'):
            self.memset_run('b800000800 50 6a00 6800000030 e8ee000000 83c40c c3')
        # a constant slot overwritten (even with the same value) is no longer proven
        with self.assertRaisesRegex(h.Rejected,'memset of 0x80000'):
            self.memset_run('6800000800 c7042400000800 6a00 6800000030 e8e8000000 83c40c c3')


    def test_cleanup_search_uses_the_decode_limit(self):
        # 600 x `inc eax` then `ret 4`: past the old 512-instruction bound
        code=bytes([0x40])*600+bytes.fromhex('c20400')
        body,(p1,p2)=self.image(code)
        with p1,p2:
            self.assertEqual(h.static_pop(0x400000,limit=512),0)
            self.assertEqual(h.static_pop(0x400000),4)


if __name__=='__main__':unittest.main()
