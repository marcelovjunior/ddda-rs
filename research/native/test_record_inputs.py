import math
import struct
import unittest
from record_inputs import RecordRangeInputs


class RecordRangeTests(unittest.TestCase):
    LAYOUT = dict(stride=20, first={'stack':8}, last={'stack':12}, result={'stack':4},
                  uint_offsets=[8,12], float_offsets=[16])

    def test_related_arguments_finite_keys_and_cross_page_reads(self):
        for seed in range(1,6):
            r = RecordRangeInputs(self.LAYOUT,seed)
            self.assertEqual(r.stack,{4:r.RESULT,8:r.first,12:r.last})
            self.assertEqual(r.last-r.first, r.count*20)
            recovered = b''.join(r.page(seed,p) for p in range(r.first & ~4095,(r.last+4095)&~4095,4096))
            self.assertEqual(recovered[r.first%4096:r.first%4096+len(r.data)],r.data)
            floats = [struct.unpack_from('<f',r.data,i*20+16)[0] for i in range(r.count)]
            self.assertTrue(all(math.isfinite(v) for v in floats))
            if seed == 1:self.assertEqual(floats,sorted(floats))
            if seed == 2:self.assertEqual(floats,sorted(floats,reverse=True))
            if seed == 3:self.assertEqual(set(floats),{0.0})
            self.assertEqual(r.data,RecordRangeInputs(self.LAYOUT,seed).data)

    def test_register_bindings_and_invalid_layouts(self):
        r=RecordRangeInputs(dict(stride=4,first={'register':'ecx'},last={'register':'edx'}),1)
        self.assertEqual(r.register(None,'ecx'),r.first)
        self.assertEqual(r.register(None,'edx'),r.last)
        for patch in (dict(stride=0),dict(first={'stack':0}),dict(last={'stack':8}),
                      dict(first={'register':'esp'}),dict(float_offsets=[20]),
                      dict(float_offsets=[8]),dict(unknown=1)):
            with self.assertRaises(ValueError):RecordRangeInputs({**self.LAYOUT,**patch},1)


if __name__ == '__main__':unittest.main()
