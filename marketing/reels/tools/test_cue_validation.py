"""Cue timing contracts must survive Python optimization with exact diagnostics."""
import json
from pathlib import Path
import subprocess
import sys
import unittest


TOOLS = Path(__file__).resolve().parent
PROBE = r'''
import json
import os
from pathlib import Path
import sys

tools = Path(sys.argv[1])
os.environ["EDYTLAB_REEL_ROOT"] = str(tools.parent)
sys.path.insert(0, str(tools))
import cues

cues.DEMOS = {"X": {"slug": "synthetic"}}
cues.caption_by_id = lambda _: ("X", ("CAP", 0, 10, "caption", (2, 4)))
cases = [
    ({"kind": "listen", "src": 1, "seconds": 1}, False),
    ({"kind": "listen", "src": 2, "seconds": 3}, False),
    ({"kind": "step", "src": -1, "seconds": 1}, False),
    ({"kind": "step", "src": 9, "seconds": 2}, False),
    ({"kind": "step", "src": -1, "seconds": 1, "line": "word"}, True),
    ({"kind": "listen", "src": 9, "seconds": 2, "line": ""}, True),
    ({"kind": "step", "src": 0, "seconds": 10}, False),
    ({"kind": "listen", "src": 1.8, "seconds": 2.2}, False),
    ({"kind": "step", "src": 0, "seconds": 10, "line": "word"}, True),
]
results = []
for item, narrated in cases:
    cues.REELS = {"fixture": {"prefix": "T", "file": "synthetic", "label": "fixture",
                              "items": [dict(item, cap="CAP")]}}
    try:
        cues.build_reel("fixture", narrated=narrated, speech={})
    except AssertionError as error:
        results.append(error.args)
    else:
        results.append(None)
print(json.dumps(results))
'''


class CueValidation(unittest.TestCase):
    """Every caption and sound guard rejects invalid cuts under normal and -O runs."""

    def test_optimization_preserves_boundary_errors_and_valid_cuts(self):
        """Exercise all four guards, both caption edges and valid end boundaries."""
        results = []
        for flags in ([], ['-O']):
            completed = subprocess.run(
                [sys.executable, '-I', *flags, '-c', PROBE, str(TOOLS)],
                capture_output=True, text=True, check=True,
            )
            results.append(json.loads(completed.stdout))
        self.assertEqual(results[0], results[1])
        self.assertEqual(results[0], [
            [['fixture', 'CAP', 'starts well before the sound', 1.0, 2]],
            [['fixture', 'CAP', 'runs past the end of the sound', 5.0, 4]],
            [['fixture', 'CAP', "outside its caption's window", -1.0, 0.0, 0, 10]],
            [['fixture', 'CAP', "outside its caption's window", 9.0, 11.0, 0, 10]],
            [['fixture', 'CAP', -1.0, 0.0, 0, 10]],
            [['fixture', 'CAP', 9, 11.0, 0, 10]],
            None, None, None,
        ])


if __name__ == '__main__':
    unittest.main()
