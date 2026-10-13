"""Compile and execute a synthetic call graph split across hot/cold crates."""
import json
from pathlib import Path
import unittest
from unittest.mock import patch
import lift
import test_lift_shards


class HotShardGraphTests(unittest.TestCase):
    def test_execution_across_hot_and_cold_packages(self):
        generate = lift.gen
        def hot_generate(*args, **kwargs):
            functions = json.loads(Path(lift.N, 'patterns/lift.json').read_text())
            hot = {f['addr'] for f in functions} & {0x400000}
            generate(*args, hot_functions=hot, **kwargs)
        # Reuse the existing real Rust graph assertions, including nested
        # calls, external boundaries, another thread and recovery after panic.
        with patch.object(lift, 'gen', side_effect=hot_generate):
            case = test_lift_shards.ShardTests('test_crate_graph_calls_and_stable_writes')
            case.test_crate_graph_calls_and_stable_writes()


if __name__ == '__main__':
    unittest.main()
