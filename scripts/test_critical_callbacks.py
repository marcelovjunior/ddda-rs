import copy
import unittest
from critical_callbacks import rank


class Ranking(unittest.TestCase):
    def data(self):
        return dict(rejected={},source_sha256='a'*64,min_frame=7,max_frame=7,batches=[
            dict(batch=3,frame=7,queued=2,participants=7,last_job_solo_tail_ns=40,
                 last_job=dict(target=1,resolved=2,vtable=3,tid=4,start=100,end=200,panicked=False),
                 join_waits=[dict(start=180,end=230,result=0,last_job_overlap_ns=20)])])

    def test_duration_and_solo_are_distinct(self):
        out=rank(self.data(),{3:'fixture'})
        row=out['joins'][0]
        self.assertEqual((row['duration_ns'],row['solo_overlap_ns']),(100,20))
        self.assertEqual((row['frame'],row['batch'],row['join_ordinal']),(7,3,0))
        self.assertEqual(out['families'][0]['solo_ms_per_frame'],20/1e6)

    def test_invalid_overlap_rejected(self):
        data=self.data();data['batches'][0]['join_waits'][0]['last_job_overlap_ns']=21
        with self.assertRaises(ValueError): rank(data)

    def test_duplicate_missing_and_loss_rejected(self):
        for mode in ('duplicate','missing','loss','provenance'):
            data=self.data()
            if mode=='duplicate': data['batches']+=copy.deepcopy(data['batches'])
            elif mode=='missing': data['max_frame']=8
            elif mode=='loss': data['rejected']={'lost':1}
            else: data['source_sha256']=''
            with self.assertRaises(ValueError): rank(data)

    def test_join_after_callback_has_zero_solo(self):
        data=self.data();join=data['batches'][0]['join_waits'][0]
        join.update(start=210,last_job_overlap_ns=0)
        self.assertEqual(rank(data)['families'][0]['positive_solo_count'],0)


if __name__=='__main__': unittest.main()
